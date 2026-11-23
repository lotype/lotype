//! A window with an editor in it, for tests of views built on this crate.

use std::ops::Range;

use gpui::{Context, Entity, EntityInputHandler, Focusable, TestAppContext, VisualTestContext};
use lotype_core::Editor;
use lotype_core::test_support::{BlockNode, assert_editor};

use crate::EditorView;

pub struct EditorTestContext<'a> {
    pub view: Entity<EditorView>,
    pub cx: &'a mut VisualTestContext,
}

impl<'a> EditorTestContext<'a> {
    /// Opens a window on an editor of `blocks`, built as
    /// [`lotype_core::test_support::editor`] builds them, focuses it and
    /// draws it.
    pub fn new(cx: &'a mut TestAppContext, blocks: Vec<BlockNode>) -> Self {
        let editor = lotype_core::test_support::editor(blocks);
        Self::with_view(cx, |cx| EditorView::new(editor, cx))
    }

    /// Opens a window on the view `build` makes, focuses it and draws it.
    pub fn with_view(
        cx: &'a mut TestAppContext,
        build: impl FnOnce(&mut Context<EditorView>) -> EditorView,
    ) -> Self {
        let (view, cx) = cx.add_window_view(|_, cx| build(cx));
        view.update_in(cx, |view, window, cx| {
            view.focus_handle(cx).focus(window, cx)
        });
        cx.run_until_parked();
        Self { view, cx }
    }

    /// Reads the editor.
    pub fn editor<R>(&mut self, read: impl FnOnce(&Editor) -> R) -> R {
        self.view.update(self.cx, |view, _| read(view.editor()))
    }

    /// Checks the document and selection, written with the markers of
    /// [`lotype_core::test_support`].
    pub fn assert(&mut self, expected: Vec<BlockNode>) {
        self.editor(|editor| assert_editor(editor, expected));
    }

    /// Types text, one key per character, as the system would deliver it.
    pub fn type_text(&mut self, text: &str) {
        self.cx.simulate_input(text);
    }

    /// Presses keys, written as GPUI writes them: `cmd-b enter`.
    pub fn keys(&mut self, keystrokes: &str) {
        self.cx.simulate_keystrokes(keystrokes);
    }

    /// Composes text as an IME does, with `selected` the part of it the IME
    /// selects, in UTF-16 code units.
    pub fn ime_compose(&mut self, text: &str, selected: Option<Range<usize>>) {
        self.view.update_in(self.cx, |view, window, cx| {
            view.replace_and_mark_text_in_range(None, text, selected, window, cx);
        });
        self.cx.run_until_parked();
    }

    /// Commits the composed text as an IME does, replacing it with `text`.
    pub fn ime_commit(&mut self, text: &str) {
        self.view.update_in(self.cx, |view, window, cx| {
            view.replace_text_in_range(None, text, window, cx);
        });
        self.cx.run_until_parked();
    }

    /// The text the system sees and its marked range, in UTF-16.
    pub fn ime_state(&mut self) -> (String, Option<Range<usize>>) {
        self.view.update_in(self.cx, |view, window, cx| {
            let len = view.text_length_utf16(window, cx).unwrap_or(0);
            let text = view.text_for_range(0..len, &mut None, window, cx);
            (text.unwrap_or_default(), view.marked_text_range(window, cx))
        })
    }
}
