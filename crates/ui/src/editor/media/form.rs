//! Draft attachment metadata and defer storage until Save.

use std::path::{Path, PathBuf};

use gpui::{
    ClickEvent, Context, Entity, FocusHandle, Focusable, KeyDownEvent, PathPromptOptions,
    Subscription, Task, Window, div, prelude::*,
};
use gpui_component::button::ButtonVariants;
use gpui_component::input::{Input, InputEvent, InputState};
use gpui_component::{ActiveTheme, Disableable, Sizable, h_flex};
use lotype_core::{BlockId, block};

use super::{Attachment, EditorView, Kind, control, panel};
use crate::editor::canvas::Row;

const STALE: &str = "This block changed. Cancel and reopen it to see the latest version.";

enum Busy {
    Choosing { _task: Task<()> },
    Importing { _task: Task<()> },
}

pub(super) struct Form {
    pub(super) block: BlockId,
    original: Attachment,
    focus: FocusHandle,
    reference: Entity<InputState>,
    label: Entity<InputState>,
    _submit: [Subscription; 2],
    picked: Option<PathBuf>,
    auto_label: Option<String>,
    busy: Option<Busy>,
    error: Option<String>,
    reveal: bool,
}

impl EditorView {
    /// Closes a form whose row disappeared and restores focus if the form owned it.
    pub(in crate::editor) fn keep_media_form(
        &mut self,
        rows: &[Row],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(form) = &self.media.form else { return };
        let kind = match form.original.kind {
            Kind::Image => block::IMAGE,
            Kind::File => block::FILE,
        };
        if self.ai.session.is_some()
            || !rows
                .iter()
                .any(|row| row.block == form.block && row.block_type == kind)
        {
            if form.focus.contains_focused(window, cx) {
                window.focus(&self.focus, cx);
            }
            self.media.form = None;
        }
    }

    /// Schedules one scroll after the row has been replaced by the form.
    pub(in crate::editor) fn reveal_media_form(&mut self) {
        let Some(form) = &mut self.media.form else {
            return;
        };
        if std::mem::take(&mut form.reveal) {
            let block = form.block;
            if let Some(item) = self
                .rows
                .iter()
                .position(|row| *row == block)
                .and_then(|at| self.row_item(at))
            {
                self.scroll.scroll_to_item(item);
            }
        }
    }

    pub(super) fn open_media_form(
        &mut self,
        block: BlockId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.ai.session.is_some() || self.media.form.is_some() {
            return;
        }
        let Some(original) = Attachment::read(self.editor.document(), block) else {
            return;
        };
        let label_hint = match original.kind {
            Kind::Image => "Caption (optional)",
            Kind::File => "File name (optional)",
        };
        let reference = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("https://… or attachment reference")
                .default_value(original.reference.clone())
        });
        let label = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(label_hint)
                .default_value(original.label.clone())
        });
        let submit = [&reference, &label].map(|input| {
            cx.subscribe_in(input, window, |view, _, event, window, cx| {
                if matches!(event, InputEvent::PressEnter { .. }) {
                    view.save_media_form(window, cx);
                }
            })
        });
        window.focus(&reference.read(cx).focus_handle(cx), cx);
        self.media.form = Some(Form {
            block,
            original,
            focus: cx.focus_handle(),
            reference,
            label,
            _submit: submit,
            picked: None,
            auto_label: None,
            busy: None,
            error: None,
            reveal: true,
        });
        cx.notify();
    }

    fn close_media_form(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self
            .media
            .form
            .as_ref()
            .is_some_and(|form| matches!(form.busy, Some(Busy::Importing { .. })))
        {
            return;
        }
        if let Some(form) = self.media.form.take()
            && form.focus.contains_focused(window, cx)
        {
            window.focus(&self.focus, cx);
        }
        cx.notify();
    }

    fn save_media_form(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(form) = &mut self.media.form else {
            return;
        };
        if form.busy.is_some() || self.ai.session.is_some() {
            return;
        }
        if Attachment::read(self.editor.document(), form.block).as_ref() != Some(&form.original) {
            form.error = Some(STALE.into());
            cx.notify();
            return;
        }
        let label = form.label.read(cx).value().to_string();
        if let (Some(path), Some(import)) = (form.picked.clone(), self.media.importer.clone()) {
            let block = form.block;
            let import = import(path, cx);
            form.error = None;
            let task = cx.spawn_in(window, async move |view, cx| {
                let result = import.await.and_then(|reference| {
                    if reference.trim().is_empty() {
                        Err("The attachment store returned an empty reference.".into())
                    } else {
                        Ok(reference)
                    }
                });
                _ = view.update_in(cx, |view, window, cx| {
                    if view
                        .media
                        .form
                        .as_ref()
                        .is_some_and(|form| form.block == block)
                    {
                        view.finish_media_save(result, &label, window, cx);
                    }
                });
            });
            form.busy = Some(Busy::Importing { _task: task });
            cx.notify();
            return;
        }
        let reference = form.reference.read(cx).value().to_string();
        self.finish_media_save(Ok(reference), &label, window, cx);
    }

    fn finish_media_save(
        &mut self,
        result: Result<String, String>,
        label: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(form) = &mut self.media.form else {
            return;
        };
        form.busy = None;
        let result = result.and_then(|reference| {
            if self.ai.session.is_some()
                || Attachment::read(self.editor.document(), form.block).as_ref()
                    != Some(&form.original)
            {
                return Err(STALE.into());
            }
            let reference = blank_or_verbatim(&reference);
            let label = blank_or_verbatim(label);
            match form.original.kind {
                Kind::Image => self.editor.set_image(form.block, reference, label),
                Kind::File => self.editor.set_file(form.block, reference, label),
            }
            .map_err(|error| error.to_string())
        });
        match result {
            Ok(()) => self.close_media_form(window, cx),
            Err(error) => {
                form.error = Some(error);
                cx.notify();
            }
        }
    }

    fn choose_media(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(form) = &mut self.media.form else {
            return;
        };
        if form.busy.is_some() || self.media.importer.is_none() {
            return;
        }
        form.error = None;
        let selection = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: None,
        });
        let task = cx.spawn_in(window, async move |view, cx| {
            let result = match selection.await {
                Ok(Ok(paths)) => Ok(paths.and_then(|paths| paths.into_iter().next())),
                Ok(Err(error)) => Err(error.to_string()),
                Err(_) => Ok(None),
            };
            _ = view.update_in(cx, |view, window, cx| {
                let Some(form) = &mut view.media.form else {
                    return;
                };
                form.busy = None;
                match result {
                    Ok(Some(path)) => {
                        let name = file_name(&path);
                        let label = form.label.read(cx).value();
                        let use_name = match &form.auto_label {
                            Some(previous) => label.as_str() == previous,
                            None => label.trim().is_empty(),
                        };
                        if form.original.kind == Kind::File && use_name {
                            form.label
                                .update(cx, |input, cx| input.set_value(name.clone(), window, cx));
                            form.auto_label = Some(name);
                        }
                        form.picked = Some(path);
                        window.focus(&form.label.read(cx).focus_handle(cx), cx);
                    }
                    Ok(None) => {}
                    Err(error) => form.error = Some(error),
                }
                cx.notify();
            });
        });
        form.busy = Some(Busy::Choosing { _task: task });
        cx.notify();
    }

    fn use_media_reference(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(form) = &mut self.media.form else {
            return;
        };
        form.picked = None;
        if form.auto_label.take().as_deref() == Some(form.label.read(cx).value().as_str()) {
            form.label.update(cx, |input, cx| {
                input.set_value(form.original.label.clone(), window, cx)
            });
        }
        form.error = None;
        window.focus(&form.reference.read(cx).focus_handle(cx), cx);
        cx.notify();
    }

    pub(super) fn media_form(&self, form: &Form, cx: &mut Context<Self>) -> impl IntoElement {
        let busy = form.busy.is_some();
        let importing = matches!(form.busy, Some(Busy::Importing { .. }));
        let escape = cx.listener(|view, event: &KeyDownEvent, window, cx| {
            if event.keystroke.key == "escape" {
                cx.stop_propagation();
                view.close_media_form(window, cx);
            }
        });
        let save = control("media-save", if importing { "Importing…" } else { "Save" })
            .primary()
            .disabled(busy)
            .on_click(
                cx.listener(|view, _: &ClickEvent, window, cx| view.save_media_form(window, cx)),
            );
        let cancel = control("media-cancel", "Cancel")
            .disabled(importing)
            .on_click(
                cx.listener(|view, _: &ClickEvent, window, cx| view.close_media_form(window, cx)),
            );
        let browse = self.media.importer.is_some().then(|| {
            control("media-browse", "Choose file")
                .disabled(busy)
                .on_click(
                    cx.listener(|view, _: &ClickEvent, window, cx| view.choose_media(window, cx)),
                )
        });
        let reference = match &form.picked {
            Some(path) => h_flex()
                .gap_2()
                .child(div().min_w_0().flex_1().truncate().child(file_name(path)))
                .child(
                    control("media-use-reference", "Use reference")
                        .disabled(busy)
                        .on_click(cx.listener(|view, _: &ClickEvent, window, cx| {
                            view.use_media_reference(window, cx)
                        })),
                ),
            None => div().child(Input::new(&form.reference).small().disabled(busy)),
        };
        panel(cx)
            .flex_col()
            .items_start()
            .track_focus(&form.focus)
            .on_key_down(escape)
            .child(
                reference
                    .w_full()
                    .debug_selector(|| "media-reference".into()),
            )
            .child(
                div()
                    .w_full()
                    .debug_selector(|| "media-label".into())
                    .child(Input::new(&form.label).small().disabled(busy)),
            )
            .child(
                h_flex()
                    .gap_2()
                    .flex_wrap()
                    .children(browse)
                    .child(save)
                    .child(cancel),
            )
            .children(form.error.as_ref().map(|error| {
                div()
                    .text_sm()
                    .text_color(cx.theme().danger)
                    .debug_selector(|| "media-error".to_owned())
                    .child(error.clone())
            }))
    }
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .unwrap_or(path.as_os_str())
        .to_string_lossy()
        .into_owned()
}

fn blank_or_verbatim(value: &str) -> &str {
    if value.trim().is_empty() { "" } else { value }
}
