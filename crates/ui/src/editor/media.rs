//! Editable image and file blocks. The host owns attachment storage and resolution.

use std::path::PathBuf;
use std::rc::Rc;

use gpui::{
    AnyElement, App, ClickEvent, Context, Div, Entity, Focusable, ImageSource, KeyDownEvent,
    MouseButton, PathPromptOptions, SharedString, StyledImage, Subscription, Task, Window, div,
    img, prelude::*, px,
};
use gpui_component::button::{Button, ButtonVariants};
use gpui_component::input::{Input, InputEvent, InputState};
use gpui_component::{ActiveTheme, Disableable, Sizable, h_flex, v_flex};
use lotype_core::{BlockId, BlockKind, Document, block};

use super::EditorView;
use super::canvas::Row;
use crate::icons;

/// A location the host authorizes the editor to load or open.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MediaSource {
    /// A web URL using HTTP or HTTPS.
    Url(String),
    /// A local file selected or resolved by the host application.
    Path(PathBuf),
}

type Importer = Rc<dyn Fn(PathBuf, &mut App) -> Task<Result<String, String>>>;
type Resolver = Rc<dyn Fn(&str) -> Option<MediaSource>>;

/// Per-view attachment integration and the open editor, if any.
#[derive(Default)]
pub(super) struct Media {
    importer: Option<Importer>,
    resolver: Option<Resolver>,
    form: Option<Form>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Image,
    File,
}

#[derive(PartialEq, Eq)]
struct Attachment {
    kind: Kind,
    reference: String,
    label: String,
}

impl Attachment {
    fn read(doc: &Document, block: BlockId) -> Option<Self> {
        let (kind, reference, label) = match BlockKind::of(doc, block)? {
            BlockKind::Image { url, caption } => (Kind::Image, url, caption),
            BlockKind::File { url, name } => (Kind::File, url, name),
            _ => return None,
        };
        Some(Self {
            kind,
            reference,
            label,
        })
    }
}

struct Form {
    block: BlockId,
    original: Attachment,
    reference: Entity<InputState>,
    label: Entity<InputState>,
    _submit: [Subscription; 2],
    importing: Option<Task<()>>,
    error: Option<String>,
}

impl EditorView {
    /// Enables the native file picker for image and file blocks.
    ///
    /// The host stores the selected file and returns the reference to put in the
    /// document, or an error message to display. The host controls where bytes go.
    pub fn with_media_importer(
        mut self,
        import: impl Fn(PathBuf, &mut App) -> Task<Result<String, String>> + 'static,
    ) -> Self {
        self.media.importer = Some(Rc::new(import));
        self
    }

    /// Resolves stored references for image previews and the file Open action.
    ///
    /// The callback runs while rendering and must return promptly. Validate any
    /// reference from a document before granting access to a local file. Without
    /// a resolver, only web URLs are used. Image requests use the application's GPUI HTTP client.
    pub fn with_media_resolver(
        mut self,
        resolve: impl Fn(&str) -> Option<MediaSource> + 'static,
    ) -> Self {
        self.media.resolver = Some(Rc::new(resolve));
        self
    }

    fn resolve_media(&self, reference: &str) -> Option<MediaSource> {
        if reference.is_empty() {
            return None;
        }
        let source = match &self.media.resolver {
            Some(resolve) => resolve(reference)?,
            None => MediaSource::Url(reference.to_owned()),
        };
        match &source {
            MediaSource::Url(url) if !web_address(url) => None,
            _ => Some(source),
        }
    }

    pub(super) fn keep_media_form(&mut self, rows: &[Row]) {
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
            self.media.form = None;
        }
    }

    /// Keeps the active form mounted while its row is offscreen.
    pub(super) fn media_form_block(&self) -> Option<BlockId> {
        self.media.form.as_ref().map(|form| form.block)
    }

    pub(super) fn media_block(&self, block: BlockId, cx: &mut Context<Self>) -> Option<AnyElement> {
        let media = Attachment::read(self.editor.document(), block)?;
        let frame = v_flex().min_w_0().gap_2().id(block.to_string());
        if let Some(form) = &self.media.form
            && form.block == block
        {
            return Some(frame.child(self.media_form(form, cx)).into_any_element());
        }
        let editing = self.ai.session.is_none();
        let edit = cx.listener(move |view, _: &ClickEvent, window, cx| {
            view.open_media_form(block, window, cx);
        });
        let empty = media.reference.is_empty();
        let symbol = match media.kind {
            Kind::Image => icons::IMAGE,
            Kind::File => icons::FILE,
        };
        let action = match (media.kind, empty) {
            (Kind::Image, true) => "Add image",
            (Kind::File, true) => "Add file",
            (Kind::Image, false) => "Edit image",
            (Kind::File, false) => "Edit file",
        };
        let edit = control("media-edit", action).on_click(edit);
        if empty {
            let placeholder = if editing {
                h_flex().gap_2().child(icons::icon(symbol)).child(edit)
            } else {
                h_flex().gap_2().child(icons::icon(symbol)).child(action)
            };
            return Some(frame.child(panel(cx).child(placeholder)).into_any_element());
        }

        let source = self.resolve_media(&media.reference);
        let label = if media.label.is_empty() {
            &media.reference
        } else {
            &media.label
        };
        let row = h_flex().gap_2().min_w_0();
        let can_open = media.kind == Kind::File && source.is_some();
        let open = can_open.then(|| {
            control("media-open", "Open").on_click(cx.listener(
                move |view, _: &ClickEvent, _, cx| {
                    let Some(file) = Attachment::read(view.editor.document(), block)
                        .filter(|file| file.kind == Kind::File)
                    else {
                        return;
                    };
                    match view.resolve_media(&file.reference) {
                        Some(MediaSource::Url(url)) => cx.open_url(&url),
                        Some(MediaSource::Path(path)) => cx.open_with_system(&path),
                        None => {}
                    }
                },
            ))
        });
        let title = div().min_w_0().flex_1().truncate().child(label.clone());
        let header = row
            .child(icons::icon(symbol))
            .child(title)
            .children(open)
            .children(editing.then_some(edit));
        let frame = match media.kind {
            Kind::Image => frame
                .child(image_preview(source, cx))
                .child(header.text_color(cx.theme().muted_foreground)),
            Kind::File => frame.child(panel(cx).child(header)),
        };
        Some(frame.into_any_element())
    }

    fn open_media_form(&mut self, block: BlockId, window: &mut Window, cx: &mut Context<Self>) {
        if self.ai.session.is_some() {
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
            reference,
            label,
            _submit: submit,
            importing: None,
            error: None,
        });
        cx.notify();
    }

    fn close_media_form(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.media.form = None;
        window.focus(&self.focus, cx);
        cx.notify();
    }

    fn save_media_form(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(form) = &mut self.media.form else {
            return;
        };
        if form.importing.is_some() || self.ai.session.is_some() {
            return;
        }
        let reference = form.reference.read(cx).value().trim().to_owned();
        let label = form.label.read(cx).value().trim().to_owned();
        if Attachment::read(self.editor.document(), form.block).as_ref() != Some(&form.original) {
            form.error =
                Some("This block changed. Cancel and reopen it to see the latest version.".into());
            cx.notify();
            return;
        }
        let result = match form.original.kind {
            Kind::Image => self.editor.set_image(form.block, &reference, &label),
            Kind::File => self.editor.set_file(form.block, &reference, &label),
        };
        match result {
            Ok(()) => self.close_media_form(window, cx),
            Err(error) => {
                form.error = Some(error.to_string());
                cx.notify();
            }
        }
    }

    fn choose_media(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (Some(form), Some(import)) = (&mut self.media.form, self.media.importer.clone()) else {
            return;
        };
        if form.importing.is_some() {
            return;
        }
        form.error = None;
        let selection = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: None,
        });
        form.importing = Some(cx.spawn_in(window, async move |view, cx| {
            let picked = match selection.await {
                Ok(Ok(paths)) => Ok(paths.and_then(|paths| paths.into_iter().next())),
                Ok(Err(error)) => Err(error.to_string()),
                Err(_) => Ok(None),
            };
            let result = match picked {
                Ok(Some(path)) => {
                    let name = path
                        .file_name()
                        .map(|name| name.to_string_lossy().into_owned());
                    let Ok(task) = cx.update(|_, cx| import(path, cx)) else {
                        return;
                    };
                    task.await.and_then(|reference| {
                        if reference.trim().is_empty() {
                            Err("The attachment store returned an empty reference.".into())
                        } else {
                            Ok(Some((reference, name)))
                        }
                    })
                }
                Ok(None) => Ok(None),
                Err(error) => Err(error),
            };
            _ = view.update_in(cx, |view, window, cx| {
                let Some(form) = &mut view.media.form else {
                    return;
                };
                form.importing = None;
                match result {
                    Ok(Some((reference, name))) => {
                        form.reference
                            .update(cx, |input, cx| input.set_value(reference, window, cx));
                        if form.original.kind == Kind::File
                            && form.label.read(cx).value().trim().is_empty()
                            && let Some(name) = name
                        {
                            form.label
                                .update(cx, |input, cx| input.set_value(name, window, cx));
                        }
                    }
                    Ok(None) => {}
                    Err(error) => form.error = Some(error),
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    fn media_form(&self, form: &Form, cx: &mut Context<Self>) -> impl IntoElement {
        let busy = form.importing.is_some();
        let cancel =
            cx.listener(|view, _: &ClickEvent, window, cx| view.close_media_form(window, cx));
        let escape = cx.listener(|view, event: &KeyDownEvent, window, cx| {
            if event.keystroke.key == "escape" {
                cx.stop_propagation();
                view.close_media_form(window, cx);
            }
        });
        let save = control("media-save", "Save")
            .primary()
            .disabled(busy)
            .on_click(
                cx.listener(|view, _: &ClickEvent, window, cx| view.save_media_form(window, cx)),
            );
        let browse = self.media.importer.is_some().then(|| {
            let title = if busy { "Importing…" } else { "Choose file" };
            control("media-browse", title).disabled(busy).on_click(
                cx.listener(|view, _: &ClickEvent, window, cx| view.choose_media(window, cx)),
            )
        });
        panel(cx)
            .flex_col()
            .items_start()
            .on_key_down(escape)
            .child(
                div()
                    .w_full()
                    .debug_selector(|| "media-reference".into())
                    .child(Input::new(&form.reference).small().disabled(busy)),
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
                    .child(control("media-cancel", "Cancel").on_click(cancel)),
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

fn panel(cx: &App) -> Div {
    div()
        .flex()
        .gap_2()
        .p_2()
        .rounded_md()
        .border_1()
        .border_color(cx.theme().border)
        .bg(cx.theme().muted)
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
}

fn control(id: &'static str, title: &'static str) -> Button {
    Button::new(id)
        .ghost()
        .small()
        .label(title)
        .debug_selector(move || id.to_owned())
}

fn image_preview(source: Option<MediaSource>, cx: &App) -> Div {
    let (fill, ink) = (cx.theme().muted, cx.theme().muted_foreground);
    let message = move |text: SharedString| {
        div()
            .p_3()
            .w_full()
            .rounded_md()
            .bg(fill)
            .text_color(ink)
            .child(text)
    };
    let Some(source) = source else {
        return message("Image unavailable".into());
    };
    let source = match source {
        MediaSource::Url(url) => ImageSource::from(url),
        MediaSource::Path(path) => ImageSource::from(path),
    };
    let preview = img(source)
        .id("media-preview")
        .max_w_full()
        .max_h(px(420.))
        .debug_selector(|| "media-preview".to_owned())
        .with_loading(move || message("Loading image…".into()).into_any_element())
        .with_fallback(move || message("Image unavailable".into()).into_any_element());
    div().w_full().flex().items_start().child(preview)
}

fn web_address(address: &str) -> bool {
    let Some((scheme, host)) = address.split_once("://") else {
        return false;
    };
    (scheme.eq_ignore_ascii_case("https") || scheme.eq_ignore_ascii_case("http"))
        && !host.is_empty()
        && !address
            .chars()
            .any(|c| c.is_ascii_control() || c.is_ascii_whitespace())
}
