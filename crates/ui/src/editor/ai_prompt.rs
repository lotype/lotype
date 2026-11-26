//! The editor-local AI prompt and its entry points.

use std::cell::Cell;
use std::collections::HashSet;
use std::rc::Rc;

use gpui::{Context, Entity, FocusHandle, Focusable, Pixels, Subscription, Window, prelude::*};
use gpui_component::input::{InputEvent, TextareaState};
use lotype_ai::Change;
use lotype_core::{BlockId, Selection};

use super::EditorView;
use super::ai::PromptScope;

/// A contextual field and the row that opened it.
pub(super) struct PromptField {
    pub(super) input: Entity<TextareaState>,
    pub(super) scope: PromptScope,
    pub(super) focus: FocusHandle,
    pub(super) history_start: usize,
    /// How tall the field was when last laid out, and when it was lowest:
    /// one line.
    pub(super) height: Rc<Cell<Option<Pixels>>>,
    pub(super) line: Cell<Option<Pixels>>,
    /// How long the text was when it no longer fit on one line next to
    /// the button that sends it, which is under the field since.
    pub(super) wrapped_at: Cell<Option<usize>>,
    pub(super) anchor: Option<BlockId>,
    _watches: [Subscription; 2],
}

impl EditorView {
    /// Opens an inline prompt for the caret or suggestions for a text selection.
    pub fn open_ai_prompt(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.open_context_prompt(self.context_scope(), window, cx);
    }

    /// Opens the selection prompt from the formatting toolbar.
    pub(super) fn open_selection_ai_prompt(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.open_context_prompt(PromptScope::Selection, window, cx);
    }

    /// Focuses a contextual field without submitting or changing the document.
    fn open_context_prompt(
        &mut self,
        scope: PromptScope,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.ai_can_submit(scope) || self.ai.field.is_some() {
            return;
        }
        let anchor = match self.editor().selection() {
            Some(Selection::Text { head, .. }) => Some(head.block),
            _ => None,
        };
        let history_start = self.ai_transcript().messages.len();
        let placeholder = match scope {
            PromptScope::Selection => "Ask AI to change this…",
            _ => "Write here with AI…",
        };
        // A line that grows with what is written in it.
        let input = cx.new(|cx| {
            TextareaState::new(window, cx)
                .placeholder(placeholder)
                .auto_grow(1, 6)
                .submit_on_enter(true)
        });
        let events = cx.subscribe_in(&input, window, |view, _, event, window, cx| match event {
            InputEvent::PressEnter { shift: false, .. } => view.ai_submit_field(window, cx),
            // Its button to send shows whether there is something to.
            InputEvent::Change => cx.notify(),
            _ => {}
        });
        let focus = cx.focus_handle();
        let blur = cx.on_focus_out(&focus, window, |view, _, window, cx| {
            if window.is_window_active() && view.ai.session.is_none() {
                view.ai.field = None;
                cx.notify();
            }
        });
        window.focus(&input.read(cx).focus_handle(cx), cx);
        self.ai.field = Some(PromptField {
            input,
            scope,
            focus,
            history_start,
            height: Default::default(),
            line: Default::default(),
            wrapped_at: Default::default(),
            anchor,
            _watches: [events, blur],
        });
        cx.notify();
    }

    /// Sends what the local prompt's field says, and empties it when taken.
    pub(super) fn ai_submit_field(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(field) = &self.ai.field else {
            return;
        };
        let (input, scope) = (field.input.clone(), field.scope);
        let text = input.read(cx).value().to_string();
        if self.ai_submit(&text, scope, cx) {
            input.update(cx, |input, cx| input.set_value("", window, cx));
            self.ai_hold_focus(window, cx);
        }
    }

    /// Keeps the focus with the prompt while a session hides its field,
    /// so that the prompt still closes when the focus leaves it.
    pub(super) fn ai_hold_focus(&self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(field) = &self.ai.field {
            window.focus(&field.focus, cx);
        }
    }

    /// Gives the focus back to the field once it shows again.
    pub(super) fn ai_refocus_field(&self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(field) = &self.ai.field else {
            return;
        };
        if self.ai.session.is_none() && field.focus.is_focused(window) {
            window.focus(&field.input.read(cx).focus_handle(cx), cx);
        }
    }

    /// Closes the local prompt and returns focus to the document.
    pub(super) fn ai_close_bar(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.ai.field.take().is_some() {
            window.focus(&self.focus, cx);
        }
        cx.notify();
    }

    /// Places the card below the prompt's row and any draft edits beneath it.
    pub(super) fn ai_bar_after(&self, rows: &[BlockId]) -> Option<BlockId> {
        let anchor = self.ai.field.as_ref().and_then(|field| field.anchor);
        let anchor = anchor.or_else(|| match self.editor().selection() {
            Some(Selection::Text { head, .. }) => Some(head.block),
            _ => None,
        });
        let anchor = anchor.and_then(|block| self.visible_cursor_row(block, rows));
        let changed: HashSet<_> = self
            .ai
            .changes
            .iter()
            .map(|change| match change {
                Change::Added(block) | Change::Updated(block) | Change::Removed(block) => *block,
            })
            .collect();
        rows.iter()
            .rev()
            .copied()
            .find(|block| Some(*block) == anchor || changed.contains(block))
    }

    /// Opens the inline prompt instead of inserting a literal Space.
    pub(super) fn ai_space(
        &mut self,
        text: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(Selection::Text { anchor, head }) = self.editor.selection() else {
            return false;
        };
        let doc = self.editor.document();
        let paragraph = doc.schema().text_block().is_some_and(|kind| {
            doc.block_type(head.block)
                .is_ok_and(|actual| actual == kind)
        });
        let empty = anchor == head
            && head.block.column().is_none()
            && paragraph
            && doc.text_len(head.block).is_ok_and(|len| len == 0);
        if text != " " || !empty || !self.has_ai() || self.composition.is_some() {
            return false;
        }
        self.open_context_prompt(PromptScope::Inline, window, cx);
        self.ai.field.is_some()
    }
}
