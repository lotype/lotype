//! Header controls for code blocks: language menu, title editor, and Copy.

use std::time::Duration;

use gpui::{
    AnyElement, App, AppContext as _, ClickEvent, ClipboardItem, Entity, Focusable as _,
    KeyDownEvent, MouseButton, Window, div, prelude::*, px,
};
use gpui_component::Icon;
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::input::{Input, InputEvent, InputState};
use gpui_component::{ActiveTheme as _, Disableable as _, Sizable as _, h_flex, v_flex};
use lotype_core::BlockId;
use lotype_ui::EditorView;

use super::controls::scroll_reconciler;
use super::language_menu::initial_language_index;
use super::{BlockRenderArgs, CODE_FONT, COPIED_FEEDBACK, CodeState, TitleForm, elem_id};
use crate::languages::{canonical_language, language_label};
use crate::twoslash::clean_copy_text;

impl CodeState {
    pub(super) fn render_header(
        &mut self,
        args: &BlockRenderArgs,
        window: &mut Window,
        cx: &mut gpui::Context<Self>,
    ) -> AnyElement {
        let block_id = args.block;
        let label = language_label(args.language.as_deref());

        let state_entity = cx.entity();
        let editor_for_lang = args.editor_view.clone();
        let lang_text = if args.editable {
            format!("{label} ▾")
        } else {
            label
        };
        let current_lang = args
            .language
            .as_deref()
            .map(canonical_language)
            .unwrap_or_else(|| "text".to_owned());
        let lang_button = Button::new(elem_id("code-lang", block_id))
            .ghost()
            .xsmall()
            .label(lang_text)
            .disabled(!args.editable)
            .debug_selector(|| "code-language".to_owned())
            .on_click(move |_: &ClickEvent, window, cx| {
                if state_entity.read(cx).language_menu == Some(block_id) {
                    state_entity.update(cx, |state, _| state.close_language_menu());
                    window.focus(&editor_for_lang.read(cx).focus_handle(cx), cx);
                } else {
                    let initial_idx = initial_language_index(&current_lang);
                    state_entity.update(cx, |state, cx| {
                        state.ensure_observers(&editor_for_lang, window, cx);
                        state.hovered = None;
                        state.language_menu = Some(block_id);
                        state.language_query.clear();
                        state.language_highlighted = initial_idx;
                        state.language_scroll.scroll_to_item(initial_idx);
                    });
                    let focus = state_entity.read(cx).language_focus.clone();
                    window.focus(&focus, cx);
                }
                editor_for_lang.update(cx, |_, cx| cx.notify());
            });
        let menu = self.render_language_menu(args, cx);
        let under = menu.map(|menu| div().h_0().child(menu));
        let lang_slot = v_flex().child(lang_button).children(under);

        let title_slot = self.render_title_slot(args, window, cx);
        let copy_button = self.render_copy_button(args, cx);

        let theme = cx.theme();
        h_flex()
            .w_full()
            .rounded_t_md()
            .justify_between()
            .items_center()
            .gap_2()
            .px_2()
            .py_1()
            .border_b_1()
            .border_color(theme.border)
            .bg(theme.background.opacity(0.55))
            .debug_selector(|| "code-header".to_owned())
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(
                h_flex()
                    .gap_1p5()
                    .items_center()
                    .min_w_0()
                    .flex_1()
                    .child(lang_slot)
                    .children(title_slot),
            )
            .child(h_flex().gap_1().items_center().child(copy_button))
            .into_any_element()
    }

    fn render_title_slot(
        &mut self,
        args: &BlockRenderArgs,
        _window: &mut Window,
        cx: &mut gpui::Context<Self>,
    ) -> Option<AnyElement> {
        let block_id = args.block;
        if let Some(form) = &self.title_form
            && form.block == block_id
        {
            let input = form.input.clone();
            let state_entity = cx.entity();
            let editor_view = args.editor_view.clone();
            return Some(
                div()
                    .w(px(200.))
                    .debug_selector(|| "code-title-input".to_owned())
                    .on_key_down(move |event: &KeyDownEvent, window, cx| {
                        if event.keystroke.key == "escape" {
                            cx.stop_propagation();
                            state_entity.update(cx, |state, _| state.title_form = None);
                            editor_view.update(cx, |view, cx| {
                                window.focus(&view.focus_handle(cx), cx);
                                cx.notify();
                            });
                        }
                    })
                    .child(Input::new(&input).xsmall())
                    .child(scroll_reconciler(cx.entity(), args.editor_view.clone()))
                    .into_any_element(),
            );
        }

        match (&args.title, args.editable) {
            (Some(title), true) => {
                let current = title.clone();
                let state_entity = cx.entity();
                let editor_view = args.editor_view.clone();
                Some(
                    Button::new(elem_id("code-title", block_id))
                        .ghost()
                        .xsmall()
                        .label(title.clone())
                        .debug_selector(|| "code-title".to_owned())
                        .on_click(move |_: &ClickEvent, window, cx| {
                            open_title_editor(
                                &state_entity,
                                &editor_view,
                                block_id,
                                &current,
                                window,
                                cx,
                            );
                        })
                        .into_any_element(),
                )
            }
            (Some(title), false) => Some(
                div()
                    .px_1p5()
                    .text_xs()
                    .font_family(CODE_FONT)
                    .text_color(cx.theme().muted_foreground)
                    .debug_selector(|| "code-title".to_owned())
                    .truncate()
                    .child(title.clone())
                    .into_any_element(),
            ),
            (None, true) => {
                let state_entity = cx.entity();
                let editor_view = args.editor_view.clone();
                Some(
                    Button::new(elem_id("code-add-title", block_id))
                        .ghost()
                        .xsmall()
                        .label("+ Title")
                        .debug_selector(|| "code-add-title".to_owned())
                        .on_click(move |_: &ClickEvent, window, cx| {
                            open_title_editor(
                                &state_entity,
                                &editor_view,
                                block_id,
                                "",
                                window,
                                cx,
                            );
                        })
                        .into_any_element(),
                )
            }
            (None, false) => None,
        }
    }

    fn render_copy_button(&self, args: &BlockRenderArgs, cx: &gpui::Context<Self>) -> AnyElement {
        let block_id = args.block;
        let is_copied = self
            .copied
            .as_ref()
            .is_some_and(|(copied_id, _)| *copied_id == block_id);
        let tooltip = if is_copied { "Copied" } else { "Copy code" };
        let code = args.code.clone();
        let twoslash = args.twoslash;
        let state_entity = cx.entity();
        let editor_view = args.editor_view.clone();

        Button::new(elem_id("code-copy", block_id))
            .ghost()
            .xsmall()
            .icon(Icon::default().data(include_bytes!("../../assets/icons/clipboard.svg")))
            .tooltip(tooltip)
            .tooltip_show_delay(Duration::from_millis(250))
            .accessibility_label(tooltip)
            .when(is_copied, |button| button.label("Copied"))
            .debug_selector(|| "code-copy".to_owned())
            .on_click(move |_: &ClickEvent, _, cx| {
                let text = clean_copy_text(&code, twoslash);
                cx.write_to_clipboard(ClipboardItem::new_string(text));
                let notify_editor = editor_view.clone();
                state_entity.update(cx, |state, cx| {
                    let reset = cx.spawn(async move |state, cx| {
                        cx.background_executor().timer(COPIED_FEEDBACK).await;
                        let cleared = state.update(cx, |state, cx| {
                            if state.copied.as_ref().is_some_and(|(id, _)| *id == block_id) {
                                state.copied = None;
                                cx.notify();
                                true
                            } else {
                                false
                            }
                        });
                        if matches!(cleared, Ok(true)) {
                            notify_editor.update(cx, |_, cx| cx.notify());
                        }
                    });
                    state.copied = Some((block_id, reset));
                    cx.notify();
                });
                editor_view.update(cx, |_, cx| cx.notify());
            })
            .into_any_element()
    }
}

fn commit_title_form(
    form: TitleForm,
    editor_view: &Entity<EditorView>,
    focus_editor: bool,
    notify_next_frame: bool,
    window: &mut Window,
    cx: &mut App,
) {
    let value = form.input.read(cx).value().trim().to_owned();
    let next = (!value.is_empty()).then_some(value);
    let unchanged = next == form.initial;
    let ai_active = editor_view.read(cx).ai_state().is_some();
    editor_view.update(cx, |view, cx| {
        if !unchanged && !ai_active {
            let _ = view.update_editor(cx, |editor| {
                editor.set_code_title(form.block, next.as_deref())
            });
        } else {
            cx.notify();
        }
        if focus_editor {
            window.focus(&view.focus_handle(cx), cx);
        }
    });
    if notify_next_frame {
        let editor = editor_view.clone();
        window.on_next_frame(move |_, cx| {
            editor.update(cx, |_, cx| cx.notify());
        });
    }
}

fn open_title_editor(
    state_entity: &Entity<CodeState>,
    editor_view: &Entity<EditorView>,
    block_id: BlockId,
    initial: &str,
    window: &mut Window,
    cx: &mut App,
) {
    let previous = state_entity.update(cx, |state, _| state.title_form.take());
    if let Some(prev) = previous {
        commit_title_form(prev, editor_view, false, false, window, cx);
    }
    let initial_value = initial.to_owned();
    let initial_trimmed = initial.trim();
    let initial_norm = (!initial_trimmed.is_empty()).then(|| initial_trimmed.to_owned());
    let input = cx.new(|cx| {
        InputState::new(window, cx)
            .placeholder("File or title (e.g. src/main.rs)")
            .default_value(initial_value)
    });
    window.focus(&input.read(cx).focus_handle(cx), cx);
    let view_for_sub = editor_view.clone();
    state_entity.update(cx, |state, cx| {
        state.ensure_observers(editor_view, window, cx);
        state.close_language_menu();
        let sub =
            cx.subscribe_in(
                &input,
                window,
                move |state, _input, event, window, cx| match event {
                    InputEvent::PressEnter { .. } => {
                        if state
                            .title_form
                            .as_ref()
                            .is_some_and(|form| form.block == block_id)
                            && let Some(form) = state.title_form.take()
                        {
                            commit_title_form(form, &view_for_sub, true, false, window, cx);
                        }
                    }
                    InputEvent::Blur => {
                        if !window.is_window_active() {
                            return;
                        }
                        if state
                            .title_form
                            .as_ref()
                            .is_some_and(|form| form.block == block_id)
                            && let Some(form) = state.title_form.take()
                        {
                            commit_title_form(form, &view_for_sub, false, true, window, cx);
                        }
                    }
                    _ => {}
                },
            );
        state.title_form = Some(TitleForm {
            block: block_id,
            initial: initial_norm,
            input,
            _sub: sub,
        });
    });
    editor_view.update(cx, |_, cx| cx.notify());
}
