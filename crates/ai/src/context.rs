//! What a model is told of a document.

use lotype_core::{BlockId, BlockSpec, Content, Document, Role, Schema, Selection};
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

/// How a block type the schema writes as a fence is written, in a few
/// words: its tag, the attributes after it, and what goes inside.
fn fence(spec: &BlockSpec, tag: &str) -> String {
    let named = spec.hint().unwrap_or(spec.name());
    let attrs = spec.markdown_attrs();
    let attrs = match attrs {
        [] => String::new(),
        attrs => {
            let keys: Vec<_> = attrs.iter().map(|key| format!("`{key}`")).collect();
            format!(
                ", with {} after the tag as `key=value`, a value with spaces in quotes",
                keys.join(", ")
            )
        }
    };
    let inside = match (spec.content(), spec.role()) {
        (Content::None, _) => "nothing inside",
        (_, Role::Code) => "its text inside as it is",
        _ => "its text inside",
    };
    format!("{named} (a fenced block tagged `{tag}`{attrs}, and {inside})")
}

/// What the model is to do, with the Markdown the document's schema takes:
/// each of its block types says how it is written, those an application
/// added as those built in.
pub(crate) fn system(schema: &Schema) -> String {
    let known = schema.block_specs().filter_map(|spec| match spec.fence() {
        Some(tag) => Some(fence(spec, tag)),
        None => spec.hint().map(str::to_owned),
    });
    let known: Vec<_> = known.collect();
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
