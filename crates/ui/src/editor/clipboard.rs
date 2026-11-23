//! Copy, cut and paste. The clipboard holds the selection as plain text
//! for other applications, and as Markdown for editors like this one.

use gpui::{ClipboardItem, Context};
use lotype_core::Selection;

use super::EditorView;

/// Starts the Markdown kept with the text. Every GPUI application keeps
/// its metadata under the same pasteboard type, Zed its own JSON.
const TAG: &str = "lotype/markdown\n";

impl EditorView {
    /// Puts the selected text on the clipboard. Returns whether there was some.
    pub(crate) fn copy(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(Selection::Text { anchor, head }) = self.editor.selection() else {
            return false;
        };
        if anchor == head {
            return false;
        }
        let Ok(slice) = self.editor.document().slice(*anchor, *head) else {
            return false;
        };
        let texts = slice
            .blocks()
            .into_iter()
            .filter_map(|block| slice.text(block).ok());
        let text = texts.collect::<Vec<_>>().join("\n");
        let markdown = [TAG, &slice.to_markdown()].concat();
        cx.write_to_clipboard(ClipboardItem::new_string_with_metadata(text, markdown));
        true
    }

    pub(crate) fn cut(&mut self, cx: &mut Context<Self>) {
        if self.copy(cx) {
            self.editor.delete_selection().ok();
        }
    }

    /// Pastes what this or another editor of ours copied with its blocks
    /// and marks, and anything else as plain text.
    pub(crate) fn paste(&mut self, cx: &mut Context<Self>) {
        let Some(item) = cx.read_from_clipboard() else {
            return;
        };
        let markdown = item
            .metadata()
            .and_then(|metadata| metadata.strip_prefix(TAG));
        let pasted = match (markdown, item.text()) {
            (Some(markdown), _) => self.editor.paste_markdown(markdown),
            (None, Some(text)) => self.editor.paste_text(&text),
            (None, None) => return,
        };
        pasted.ok();
    }
}
