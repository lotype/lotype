//! Lifecycle management for interactive code-block header controls (language
//! menu and title input): blur dismissal, scroll/virtualization checks, and
//! focus restoration when the owning block disappears or becomes non-editable.

use gpui::{
    AnyElement, DispatchPhase, Entity, Focusable as _, ScrollEvent, Window, canvas, prelude::*,
};
use lotype_core::{BlockId, block};
use lotype_ui::EditorView;

use super::CodeState;

impl CodeState {
    pub(super) fn ensure_observers(
        &mut self,
        editor_view: &Entity<EditorView>,
        window: &mut Window,
        cx: &mut gpui::Context<Self>,
    ) {
        if self.editor_focus.is_none() {
            self.editor_focus = Some(editor_view.read(cx).focus_handle(cx));
        }
        if !self.control_subs.is_empty() {
            return;
        }
        let editor_for_blur = editor_view.clone();
        let blur = cx.on_blur(&self.language_focus, window, move |state, window, _cx| {
            if state.language_menu.take().is_some() {
                state.language_query.clear();
                let editor = editor_for_blur.clone();
                // Focus listeners run during `Window::draw`, while immediate refreshes
                // are suppressed; queue a next-frame notify so the menu disappears.
                window.on_next_frame(move |_, cx| {
                    editor.update(cx, |_, cx| cx.notify());
                });
            }
        });
        let observe = cx.observe_in(editor_view, window, |state, editor_view, window, cx| {
            state.schedule_reconcile(editor_view, window, cx);
        });
        self.control_subs = vec![blur, observe];
    }

    pub(super) fn close_controls_if_readonly(
        &mut self,
        window: &mut Window,
        cx: &mut gpui::Context<Self>,
    ) {
        let mut restore_focus = false;
        if self.language_menu.take().is_some() {
            restore_focus |= self.language_focus.is_focused(window);
            self.language_query.clear();
        }
        if let Some(form) = self.title_form.take() {
            restore_focus |= form.input.read(cx).focus_handle(cx).is_focused(window);
        }
        if restore_focus && let Some(editor_focus) = self.editor_focus.clone() {
            window.focus(&editor_focus, cx);
        }
    }

    pub(super) fn schedule_reconcile(
        &mut self,
        editor_view: Entity<EditorView>,
        window: &mut Window,
        cx: &mut gpui::Context<Self>,
    ) {
        if self.language_menu.is_none() && self.title_form.is_none() {
            return;
        }
        if self.reconcile_with(&editor_view, window, cx, |view, block| {
            view.editor()
                .document()
                .block_type(block)
                .is_ok_and(|kind| kind == block::CODE_BLOCK)
        }) {
            editor_view.update(cx, |_, cx| cx.notify());
        }
        if (self.language_menu.is_none() && self.title_form.is_none()) || self.reconcile_scheduled {
            return;
        }
        self.reconcile_scheduled = true;
        let state = cx.entity();
        window.on_next_frame(move |window, cx| {
            let changed = state.update(cx, |state, cx| {
                state.reconcile_scheduled = false;
                state.reconcile_visible(&editor_view, window, cx)
            });
            if changed {
                editor_view.update(cx, |_, cx| cx.notify());
            }
        });
    }

    pub(super) fn reconcile_visible(
        &mut self,
        editor_view: &Entity<EditorView>,
        window: &mut Window,
        cx: &mut gpui::Context<Self>,
    ) -> bool {
        self.reconcile_with(editor_view, window, cx, |view, block| {
            view.editor()
                .document()
                .block_type(block)
                .is_ok_and(|kind| kind == block::CODE_BLOCK)
                && view.text_bounds(block).is_some()
        })
    }

    fn reconcile_with(
        &mut self,
        editor_view: &Entity<EditorView>,
        window: &mut Window,
        cx: &mut gpui::Context<Self>,
        keep: impl Fn(&EditorView, BlockId) -> bool,
    ) -> bool {
        let mut closed = false;
        let mut restore_focus = false;
        if let Some(block) = self.language_menu
            && !keep(editor_view.read(cx), block)
        {
            restore_focus |= self.language_focus.is_focused(window);
            self.language_menu = None;
            self.language_query.clear();
            closed = true;
        }
        if let Some(block) = self.title_form.as_ref().map(|form| form.block)
            && !keep(editor_view.read(cx), block)
            && let Some(form) = self.title_form.take()
        {
            restore_focus |= form.input.read(cx).focus_handle(cx).is_focused(window);
            closed = true;
        }
        if restore_focus {
            let focus = self
                .editor_focus
                .clone()
                .unwrap_or_else(|| editor_view.read(cx).focus_handle(cx));
            window.focus(&focus, cx);
        }
        closed
    }
}

/// Zero-size sentinel painted alongside an open header control so any viewport
/// scroll triggers a one-shot visibility check on the next frame.
pub(super) fn scroll_reconciler(
    state: Entity<CodeState>,
    editor_view: Entity<EditorView>,
) -> AnyElement {
    canvas(
        |_, _, _| (),
        move |_, _, window, _| {
            let state = state.clone();
            let editor_view = editor_view.clone();
            window.on_mouse_event(move |_: &ScrollEvent, phase, window, cx| {
                if phase == DispatchPhase::Bubble {
                    let editor = editor_view.clone();
                    state.update(cx, |state, cx| {
                        state.schedule_reconcile(editor, window, cx);
                    });
                }
            });
        },
    )
    .absolute()
    .size_0()
    .into_any_element()
}
