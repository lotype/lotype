//! Copy, cut and paste. The clipboard holds the selection as plain text
//! for other applications, and whole, as a document of its own, for
//! editors like this one: Markdown would lose underline, colors and atoms.

use gpui::{ClipboardItem, Context};
use lotype_core::{Document, Selection};

use super::EditorView;

/// Starts the document kept with the text, as Loro bytes in hex. Every
/// GPUI application keeps its metadata under the same pasteboard type,
/// Zed its own JSON.
const TAG: &str = "lotype/loro\n";

/// The most copied bytes read back, in hex: any application can put
/// metadata on the clipboard, and Loro's blocks unpack to any size.
const MAX_HEX: usize = 64 << 20;

impl EditorView {
    /// Puts the selected text on the clipboard. Returns whether there was some.
    pub(crate) fn copy(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(Selection::Text { anchor, head }) = self.editor.selection() else {
            return false;
        };
        if anchor == head {
            return false;
        }
        let doc = self.editor.document();
        let Ok(slice) = doc.slice(*anchor, *head) else {
            return false;
        };
        let Ok(bytes) = slice.export_snapshot() else {
            return false;
        };
        let texts = slice.blocks().into_iter();
        let texts = texts.filter_map(|block| slice.plain_text(block).ok());
        let text = texts.collect::<Vec<_>>().join("\n");
        let hex: String = bytes.iter().map(|byte| format!("{byte:02x}")).collect();
        let metadata = [TAG, &hex].concat();
        cx.write_to_clipboard(ClipboardItem::new_string_with_metadata(text, metadata));
        true
    }

    pub(crate) fn cut(&mut self, cx: &mut Context<Self>) {
        if self.copy(cx) {
            self.editor.delete_selection().ok();
        }
    }

    /// Pastes what this or another editor of ours copied, whole, and
    /// anything else as plain text. Copied bytes are read as a peer's
    /// update is: what the schema lacks is left out.
    pub(crate) fn paste(&mut self, cx: &mut Context<Self>) {
        let Some(item) = cx.read_from_clipboard() else {
            return;
        };
        let copied = item.metadata().and_then(|metadata| {
            let hex = metadata
                .strip_prefix(TAG)
                .filter(|hex| hex.len() <= MAX_HEX)?;
            let mut copied = Document::new(self.editor.document().schema().clone());
            copied.import(&from_hex(hex)?).ok()?;
            Some(copied)
        });
        // A copy that cannot go here, as blocks too deep for it, goes in
        // as its plain text.
        let pasted = copied.is_some_and(|copied| self.editor.paste_document(&copied).is_ok());
        if !pasted && let Some(text) = item.text() {
            self.editor.paste_text(&text).ok();
        }
    }
}

fn from_hex(hex: &str) -> Option<Vec<u8>> {
    let digits = hex.as_bytes().chunks(2);
    let byte = |pair: &[u8]| u8::from_str_radix(std::str::from_utf8(pair).ok()?, 16).ok();
    hex.len()
        .is_multiple_of(2)
        .then(|| digits.map(byte).collect())?
}
