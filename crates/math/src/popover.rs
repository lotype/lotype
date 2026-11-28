//! Interactive popover for editing inline LaTeX equations using the shared
//! [`crate::card::render_equation_card`] component.

use std::sync::Arc;

use gpui::{
    Anchor, AnyElement, AppContext as _, Bounds, Entity, Focusable as _, KeyDownEvent, Pixels,
    SharedString, Subscription, Window, anchored, deferred, div, point, prelude::*, px,
};
use gpui_component::Sizable as _;
use gpui_component::input::{InputEvent, Position, Textarea, TextareaState};
use katex_native::Layout;
use lotype_core::{BlockId, OBJECT, Pos};
use lotype_ui::EditorView;

use crate::card::{EquationCardProps, render_equation_card};
use crate::inline_atom;
use crate::view::MathState;

pub(crate) struct InlineMathForm {
    pub(crate) block: BlockId,
    pub(crate) offset: usize,
    pub(crate) bounds: Bounds<Pixels>,
    pub(crate) last_valid: Option<Arc<Layout>>,
    pub(crate) input: Entity<TextareaState>,
    pub(crate) _sub: Subscription,
}

fn normalize_inline_tex(raw: &str) -> String {
    raw.replace("\r\n", " ")
        .replace(['\n', '\r'], " ")
        .trim()
        .to_owned()
}

pub(crate) fn open_inline_editor(
    state: &Entity<MathState>,
    editor_view: &Entity<EditorView>,
    block: BlockId,
    offset: usize,
    initial_tex: &str,
    window: &mut Window,
    cx: &mut gpui::App,
) {
    let Some(bounds) = editor_view
        .read(cx)
        .range_bounds(block, offset..offset + OBJECT.len_utf8())
    else {
        return;
    };
    let input = cx.new(|cx| {
        let mut field = TextareaState::new(window, cx)
            .placeholder(r"E = mc^2")
            .auto_grow(1, 5)
            .submit_on_enter(true);
        field.set_value(initial_tex.to_owned(), window, cx);
        field
    });
    let focus = input.read(cx).focus_handle(cx);
    let input_for_cursor = input.clone();
    window.on_next_frame(move |window, cx| {
        input_for_cursor.update(cx, |field, cx| {
            field.set_cursor_position(Position::new(u32::MAX, u32::MAX), window, cx);
        });
    });
    let editor_for_sub = editor_view.clone();
    let input_for_sub = input.clone();
    state.update(cx, |state, cx| {
        state.ensure_observer(editor_view, window, cx);
        let normalized = normalize_inline_tex(initial_tex);
        let initial_valid = (!normalized.is_empty())
            .then(|| state.layout_cached(&normalized, false).ok())
            .flatten();
        let sub = cx.subscribe_in(
            &input,
            window,
            move |state, _, event: &InputEvent, window, cx| match event {
                InputEvent::Change => {
                    editor_for_sub.update(cx, |_, cx| cx.notify());
                }
                InputEvent::PressEnter { shift: false, .. } => {
                    commit_from_state(state, &editor_for_sub, window, cx);
                }
                InputEvent::Blur => {
                    let still_open = state
                        .inline_form
                        .as_ref()
                        .is_some_and(|form| form.input == input_for_sub);
                    if still_open {
                        let state_entity = cx.entity();
                        let editor = editor_for_sub.clone();
                        window.on_next_frame(move |window, cx| {
                            commit_inline_editor(&state_entity, &editor, window, cx);
                        });
                    }
                }
                _ => {}
            },
        );
        state.inline_form = Some(InlineMathForm {
            block,
            offset,
            bounds,
            last_valid: initial_valid,
            input,
            _sub: sub,
        });
    });
    window.focus(&focus, cx);
    editor_view.update(cx, |_, cx| cx.notify());
}

fn commit_from_state(
    state: &mut MathState,
    editor_view: &Entity<EditorView>,
    window: &mut Window,
    cx: &mut gpui::Context<MathState>,
) {
    let Some(form) = state.inline_form.take() else {
        return;
    };
    let normalized = normalize_inline_tex(&form.input.read(cx).value());
    let block = form.block;
    let offset = form.offset;
    editor_view.update(cx, |view, cx| {
        let pos = Pos::new(block, offset);
        view.update_editor(cx, |editor| {
            if normalized.is_empty() {
                let _ = editor.remove_atom(pos);
            } else {
                let _ = editor.replace_atom(pos, inline_atom(normalized));
            }
        });
        let focus = view.focus_handle(cx);
        window.focus(&focus, cx);
    });
}

pub(crate) fn commit_inline_editor(
    state: &Entity<MathState>,
    editor_view: &Entity<EditorView>,
    window: &mut Window,
    cx: &mut gpui::App,
) {
    let Some((block, offset, value)) = state.update(cx, |state, cx| {
        let form = state.inline_form.take()?;
        let value = form.input.read(cx).value().to_string();
        Some((form.block, form.offset, value))
    }) else {
        return;
    };
    let normalized = normalize_inline_tex(&value);
    editor_view.update(cx, |view, cx| {
        let pos = Pos::new(block, offset);
        view.update_editor(cx, |editor| {
            if normalized.is_empty() {
                let _ = editor.remove_atom(pos);
            } else {
                let _ = editor.replace_atom(pos, inline_atom(normalized));
            }
        });
        let focus = view.focus_handle(cx);
        window.focus(&focus, cx);
    });
}

pub(crate) fn cancel_inline_editor(
    state: &Entity<MathState>,
    editor_view: &Entity<EditorView>,
    window: &mut Window,
    cx: &mut gpui::App,
) {
    let closed = state.update(cx, |state, _| state.inline_form.take().is_some());
    if closed {
        editor_view.update(cx, |view, cx| {
            let focus = view.focus_handle(cx);
            window.focus(&focus, cx);
            cx.notify();
        });
    }
}

pub(crate) fn delete_inline_equation(
    state: &Entity<MathState>,
    editor_view: &Entity<EditorView>,
    window: &mut Window,
    cx: &mut gpui::App,
) {
    let Some((block, offset)) = state.update(cx, |state, _| {
        let form = state.inline_form.take()?;
        Some((form.block, form.offset))
    }) else {
        return;
    };
    editor_view.update(cx, |view, cx| {
        view.update_editor(cx, |editor| {
            let _ = editor.remove_atom(Pos::new(block, offset));
        });
        let focus = view.focus_handle(cx);
        window.focus(&focus, cx);
    });
}

pub(crate) fn render_inline_popover(
    state: &mut MathState,
    editor_view: &Entity<EditorView>,
    block: BlockId,
    offset: usize,
    cx: &gpui::Context<MathState>,
) -> Option<AnyElement> {
    let (bounds, input) = {
        let form = state.inline_form.as_ref()?;
        if form.block != block || form.offset != offset {
            return None;
        }
        (form.bounds, form.input.clone())
    };
    let draft = input.read(cx).value().to_string();
    let normalized = normalize_inline_tex(&draft);
    let preview = if normalized.is_empty() {
        None
    } else {
        let res = state.layout_cached(&normalized, false);
        if let Ok(layout) = &res
            && let Some(form) = state.inline_form.as_mut()
        {
            form.last_valid = Some(layout.clone());
        }
        Some(res)
    };
    let stale_preview = state
        .inline_form
        .as_ref()
        .and_then(|form| form.last_valid.clone());

    let state_entity = cx.entity();
    let state_for_key = state_entity.clone();
    let editor_for_key = editor_view.clone();
    let state_for_done = state_entity.clone();
    let editor_for_done = editor_view.clone();
    let state_for_del = state_entity;
    let editor_for_del = editor_view.clone();

    let source = div()
        .w_full()
        .debug_selector(|| "inline-math-input".to_owned())
        .capture_key_down(move |event: &KeyDownEvent, window, cx| {
            if event.keystroke.key == "escape" {
                cx.stop_propagation();
                cancel_inline_editor(&state_for_key, &editor_for_key, window, cx);
            } else if event.keystroke.key == "enter" && !event.keystroke.modifiers.shift {
                cx.stop_propagation();
                commit_inline_editor(&state_for_key, &editor_for_key, window, cx);
            }
        })
        .child(Textarea::new(&input).appearance(false).xsmall())
        .into_any_element();

    let card = render_equation_card(
        state,
        editor_view.clone(),
        EquationCardProps {
            title: "Equation",
            hint: "Inline · Enter to save",
            empty_placeholder: "Type LaTeX (leave empty to remove)",
            tex: normalized,
            copy_key: SharedString::from(format!("math-copy-inline-{block}-{offset}")),
            preview,
            stale_preview,
            preview_em: px(17.),
            fleqn: false,
            leqno: false,
            source,
            delete_action: Some((
                SharedString::from("inline-math-delete"),
                "inline-math-delete",
                Box::new(move |_, window, cx| {
                    delete_inline_equation(&state_for_del, &editor_for_del, window, cx);
                }),
            )),
            done_id: SharedString::from("inline-math-done"),
            done_debug_id: "inline-math-done",
            on_done: Box::new(move |_, window, cx| {
                commit_inline_editor(&state_for_done, &editor_for_done, window, cx);
            }),
            floating: true,
            debug_id: "inline-math-popover",
        },
        cx,
    );

    let placed = anchored()
        .anchor(Anchor::TopLeft)
        .attach(bounds, Anchor::BottomLeft)
        .offset(point(px(0.), px(6.)))
        .snap_to_window_with_margin(px(8.))
        .child(card);
    Some(deferred(placed).with_priority(3).into_any_element())
}
