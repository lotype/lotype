//! Header controls for code blocks: language menu, title editor, and Copy.

use std::time::Duration;

use gpui::{
    AnyElement, App, AppContext as _, ClickEvent, ClipboardItem, Entity, Focusable as _,
    KeyDownEvent, MouseButton, MouseDownEvent, SharedString, Window, anchored, deferred, div,
    point, prelude::*, px,
};
use gpui_component::Icon;
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::input::{Input, InputEvent, InputState};
use gpui_component::list::ListItem;
use gpui_component::{
    ActiveTheme as _, Disableable as _, Sizable as _, ThemeStyled as _, h_flex, v_flex,
};
use lotype_core::BlockId;
use lotype_ui::EditorView;

use super::controls::scroll_reconciler;
use super::{BlockRenderArgs, CODE_FONT, COPIED_FEEDBACK, CodeState, TitleForm, elem_id};
use crate::languages::{LANGUAGES, canonical_language, language_label};
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
        let lang_button = Button::new(elem_id("code-lang", block_id))
            .ghost()
            .xsmall()
            .label(lang_text)
            .disabled(!args.editable)
            .debug_selector(|| "code-language".to_owned())
            .on_click(move |_: &ClickEvent, window, cx| {
                if state_entity.read(cx).language_menu == Some(block_id) {
                    state_entity.update(cx, |state, _| {
                        state.language_menu = None;
                        state.language_query.clear();
                    });
                    window.focus(&editor_for_lang.read(cx).focus_handle(cx), cx);
                } else {
                    state_entity.update(cx, |state, cx| {
                        state.ensure_observers(&editor_for_lang, window, cx);
                        state.hovered = None;
                        state.language_menu = Some(block_id);
                        state.language_query.clear();
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

    pub(super) fn render_language_menu(
        &self,
        args: &BlockRenderArgs,
        cx: &gpui::Context<Self>,
    ) -> Option<AnyElement> {
        if self.language_menu != Some(args.block) || !args.editable {
            return None;
        }
        let current = args
            .language
            .as_deref()
            .map(canonical_language)
            .unwrap_or_else(|| "text".to_owned());
        let block_id = args.block;
        let state_entity = cx.entity();
        let editor_view = args.editor_view.clone();
        let query = self.language_query.trim().to_lowercase();

        let items: Vec<AnyElement> = LANGUAGES
            .iter()
            .enumerate()
            .filter(|(_, entry)| query.is_empty() || language_matches(entry, &query))
            .map(|(index, entry)| {
                let selected = entry.id == current;
                let chosen = (entry.id != "text").then_some(entry.id);
                let state = state_entity.clone();
                let view = editor_view.clone();
                let id = entry.id;
                let option_id = SharedString::from(format!("code-lang-option-{block_id}-{index}"));
                div()
                    .debug_selector(move || format!("code-lang-{id}"))
                    .child(
                        ListItem::new(option_id)
                            .selected(selected)
                            .child(div().text_xs().child(entry.label))
                            .on_click(move |_: &ClickEvent, window, cx| {
                                state.update(cx, |state, _| {
                                    state.language_menu = None;
                                    state.language_query.clear();
                                });
                                view.update(cx, |view, cx| {
                                    let _ = view.update_editor(cx, |editor| {
                                        editor.set_code_language(block_id, chosen)
                                    });
                                    window.focus(&view.focus_handle(cx), cx);
                                });
                            }),
                    )
                    .into_any_element()
            })
            .collect();

        let state_for_close = state_entity.clone();
        let editor_for_close = editor_view.clone();
        let state_for_escape = state_entity.clone();
        let editor_for_escape = editor_view.clone();
        let menu = v_flex()
            .track_focus(&self.language_focus)
            .popover_style(cx)
            .occlude()
            .w(px(168.))
            .p_1()
            .debug_selector(|| "code-language-menu".to_owned())
            .on_key_down(move |event: &KeyDownEvent, window, cx| {
                let key = event.keystroke.key.as_str();
                match key {
                    "escape" => {
                        cx.stop_propagation();
                        state_for_escape.update(cx, |state, _| {
                            state.language_menu = None;
                            state.language_query.clear();
                        });
                        editor_for_escape.update(cx, |view, cx| {
                            window.focus(&view.focus_handle(cx), cx);
                            cx.notify();
                        });
                    }
                    "backspace" => {
                        cx.stop_propagation();
                        state_for_escape.update(cx, |state, _| {
                            state.language_query.pop();
                        });
                        editor_for_escape.update(cx, |_, cx| cx.notify());
                    }
                    "enter" => {
                        let query = state_for_escape.read(cx).language_query.to_lowercase();
                        if let Some(language) = LANGUAGES
                            .iter()
                            .find(|language| query.is_empty() || language_matches(language, &query))
                        {
                            cx.stop_propagation();
                            let chosen = (language.id != "text").then_some(language.id);
                            state_for_escape.update(cx, |state, _| {
                                state.language_menu = None;
                                state.language_query.clear();
                            });
                            editor_for_escape.update(cx, |view, cx| {
                                let _ = view.update_editor(cx, |editor| {
                                    editor.set_code_language(block_id, chosen)
                                });
                                window.focus(&view.focus_handle(cx), cx);
                            });
                        }
                    }
                    _ if key.chars().count() == 1
                        && !event.keystroke.modifiers.platform
                        && !event.keystroke.modifiers.control
                        && !event.keystroke.modifiers.alt =>
                    {
                        cx.stop_propagation();
                        state_for_escape.update(cx, |state, _| {
                            state.language_query.push_str(&key.to_lowercase());
                        });
                        editor_for_escape.update(cx, |_, cx| cx.notify());
                    }
                    _ => {}
                }
            })
            .on_mouse_down_out(move |_: &MouseDownEvent, window, cx| {
                let closed = state_for_close.update(cx, |state, _| {
                    if state.language_menu == Some(block_id) {
                        state.language_menu = None;
                        state.language_query.clear();
                        true
                    } else {
                        false
                    }
                });
                if closed {
                    if state_for_close.read(cx).language_focus.is_focused(window) {
                        window.focus(&editor_for_close.read(cx).focus_handle(cx), cx);
                    }
                    editor_for_close.update(cx, |_, cx| cx.notify());
                }
            })
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(scroll_reconciler(state_entity, editor_view))
            .child(
                div()
                    .id(elem_id("code-language-scroll", block_id))
                    .track_scroll(&self.language_scroll)
                    .overflow_y_scroll()
                    .max_h(px(240.))
                    .children(if items.is_empty() {
                        vec![
                            div()
                                .px_2()
                                .py_1()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child("No matching languages")
                                .into_any_element(),
                        ]
                    } else {
                        items
                    }),
            );
        let placed = anchored()
            .offset(point(px(0.), px(4.)))
            .snap_to_window_with_margin(px(8.))
            .child(menu);
        Some(deferred(placed).with_priority(3).into_any_element())
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
        state.language_menu = None;
        state.language_query.clear();
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

fn language_matches(language: &crate::languages::CodeLanguage, query: &str) -> bool {
    language.id.to_lowercase().contains(query)
        || language.label.to_lowercase().contains(query)
        || language
            .aliases
            .iter()
            .any(|alias| alias.to_lowercase().contains(query))
}
