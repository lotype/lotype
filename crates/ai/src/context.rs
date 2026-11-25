//! What a model is told of a document.

use lotype_core::{BlockId, Document, Schema, Selection, block};
use serde_json::{Value, json};

/// A document as a model is shown it: every block with its id, how deep it
/// is nested and its own Markdown, and what the user has selected.
#[derive(Debug, Clone, PartialEq)]
pub struct DocumentState {
    /// The blocks in reading order. A table is one, with its rows.
    pub blocks: Vec<(BlockId, usize, String)>,
    /// The blocks the selection is in, if it is more than a caret.
    pub selected: Vec<BlockId>,
    /// The selected text, where text is selected.
    pub selected_text: String,
    /// The block the caret is in, where nothing is selected.
    pub cursor: Option<BlockId>,
}

impl DocumentState {
    /// `doc` with `selection` in it.
    pub fn of(doc: &Document, selection: Option<&Selection>) -> Self {
        let blocks = doc.markdown_blocks();
        let at = |block: &BlockId| blocks.iter().position(|(shown, ..)| shown == block);
        let (mut selected, mut selected_text, mut cursor) = (Vec::new(), String::new(), None);
        match selection {
            Some(Selection::Text { anchor, head }) if anchor == head => cursor = Some(head.block),
            Some(Selection::Text { anchor, head }) => {
                // In a table, the blocks shown are not those the text is in.
                if let (Some(a), Some(b)) = (at(&anchor.block), at(&head.block)) {
                    let range = a.min(b)..=a.max(b);
                    selected = blocks[range].iter().map(|(block, ..)| *block).collect();
                }
                let slice = doc.slice(*anchor, *head);
                selected_text = slice.map(|slice| slice.to_plain_text()).unwrap_or_default();
            }
            Some(Selection::Blocks(chosen)) => {
                let shown = chosen.iter().filter(|block| at(block).is_some());
                selected = shown.copied().collect();
            }
            Some(Selection::Cells { .. }) | None => {}
        }
        let cursor = cursor.filter(|block| at(block).is_some());
        Self {
            blocks,
            selected,
            selected_text,
            cursor,
        }
    }

    /// The state as JSON, as it goes into a request.
    pub fn to_json(&self) -> Value {
        let id = |block: &BlockId| Value::String(block.to_string());
        let blocks = self.blocks.iter().map(
            |(block, depth, markdown)| json!({"id": id(block), "depth": depth, "block": markdown}),
        );
        let mut state = json!({"blocks": blocks.collect::<Vec<_>>()});
        if !self.selected.is_empty() {
            state["selected"] = self.selected.iter().map(id).collect();
        }
        if !self.selected_text.is_empty() {
            state["selectedText"] = Value::String(self.selected_text.clone());
        }
        if let Some(cursor) = &self.cursor {
            state["cursor"] = id(cursor);
        }
        state
    }
}

/// What the model is to do, with the Markdown the document's schema takes.
pub(crate) fn system(schema: &Schema) -> String {
    let kinds = [
        (block::HEADING, "headings (`# Title`)"),
        (block::BULLET_LIST_ITEM, "bullet items (`- item`)"),
        (block::NUMBERED_LIST_ITEM, "numbered items (`1. item`)"),
        (block::CHECK_LIST_ITEM, "tasks (`- [ ] task`)"),
        (block::QUOTE, "quotes (`> text`)"),
        (
            block::ALERT,
            "alerts (`> [!NOTE]`, with their text added after them)",
        ),
        (block::CODE_BLOCK, "fenced code"),
        (block::TABLE, "tables"),
        (block::DIVIDER, "dividers (`---`)"),
        (block::IMAGE, "images (`![caption](url)`)"),
    ];
    let known = kinds
        .iter()
        .filter(|(name, _)| schema.block_spec(name).is_some());
    let known: Vec<_> = known.map(|(_, written)| *written).collect();
    format!(
        "You edit a document for its user. Each request comes with the document as JSON: \
         `blocks` in reading order, each with its `id`, how deep it is nested (`depth`) and \
         its own Markdown (`block`), without the blocks nested in it. `selected` are the \
         blocks the user selected and `selectedText` the text, or `cursor` is the block the \
         caret is in.\n\n\
         Change the document only by calling the tools, once per block, and with ids taken \
         from the document as given. A block is one Markdown block: a paragraph, or one of \
         {}. Nest blocks in an added block by indenting them under it. Keep what the user \
         did not ask to change, and write in the language of the document unless asked \
         otherwise. When asked something that needs no change, answer in plain text and \
         call no tool. After the tools, say in a sentence what you did.",
        known.join(", ")
    )
}

/// The request of a turn: the document as it is now, then what is asked.
pub(crate) fn user(state: &Value, prompt: &str) -> String {
    format!("<document>\n{state}\n</document>\n\n{prompt}")
}
