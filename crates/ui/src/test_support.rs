//! A window with an editor in it, for tests of views built on this crate.

use gpui::{Entity, TestAppContext, VisualTestContext};
use lotype_core::Editor;
use lotype_core::test_support::BlockNode;

use crate::EditorView;

pub struct EditorTestContext<'a> {
    pub view: Entity<EditorView>,
    pub cx: &'a mut VisualTestContext,
}

impl<'a> EditorTestContext<'a> {
    /// Opens a window on an editor of `blocks`, built as
    /// [`lotype_core::test_support::editor`] builds them, and draws it.
    pub fn new(cx: &'a mut TestAppContext, blocks: Vec<BlockNode>) -> Self {
        let editor = lotype_core::test_support::editor(blocks);
        let (view, cx) = cx.add_window_view(|_, cx| EditorView::new(editor, cx));
        cx.run_until_parked();
        Self { view, cx }
    }

    /// Reads the editor.
    pub fn editor<R>(&mut self, read: impl FnOnce(&Editor) -> R) -> R {
        self.view.update(self.cx, |view, _| read(view.editor()))
    }
}
