//! The text ranges an AI selection command is allowed to change.

use std::ops::Range;

use lotype_core::{BlockId, Document, Error, OBJECT, Result, Selection, Span};
use serde_json::{Value, json};

use crate::{DocumentState, Op};

/// The selected text in each editable block, in document order.
pub(crate) struct TextSelection(Vec<(BlockId, Range<usize>)>);

impl TextSelection {
    /// Captures a nonempty text selection without including block boundaries or atoms.
    pub(crate) fn new(doc: &Document, selection: Option<&Selection>) -> Result<Self> {
        let Some(Selection::Text { anchor, head }) = selection else {
            return Err(Error::NoSelection);
        };
        if anchor.block.column().is_some() || head.block.column().is_some() {
            return Err(Error::TablePart);
        }
        let (start, end) = doc.ordered(*anchor, *head)?;
        let mut ranges = Vec::new();
        let mut next = Some(start.block);
        while let Some(block) = next {
            if let Ok(len) = doc.text_len(block) {
                let from = if block == start.block {
                    start.offset
                } else {
                    0
                };
                let to = if block == end.block { end.offset } else { len };
                let range = from..to;
                if !range.is_empty()
                    && doc
                        .spans_in(block, range.clone())?
                        .iter()
                        .all(|span| span.atom.is_none())
                {
                    ranges.push((block, range));
                }
            }
            if block == end.block {
                break;
            }
            next = doc.next_block(block)?;
        }
        if ranges.is_empty() {
            return Err(Error::NoSelection);
        }
        Ok(Self(ranges))
    }

    /// Returns the model-independent range for an allowed block ID.
    pub(crate) fn range(&self, block: BlockId) -> Option<Range<usize>> {
        self.0
            .iter()
            .find(|(id, _)| *id == block)
            .map(|(_, range)| range.clone())
    }

    /// Describes the current fragments, with the rest of the document as context.
    pub(crate) fn context(&self, doc: &Document) -> Value {
        let mut state = DocumentState::of(doc, None);
        let mut editable = Vec::new();
        let mut selected_text = Vec::new();
        for (block, range) in &self.0 {
            let Ok(current) = doc.text(*block) else {
                continue;
            };
            let Some(fragment) = current.get(range.clone()) else {
                continue;
            };
            state.selected.push(*block);
            selected_text.push(fragment.to_owned());
            editable.push(json!({ "id": block.to_string(), "text": fragment }));
        }
        state.selected_text = selected_text.join("\n");
        let mut state = state.to_json();
        state["editableText"] = Value::Array(editable);
        state
    }

    /// Updates the range after a completed call, or restores it when retrying.
    pub(crate) fn track(&mut self, op: &Op, applied: bool) {
        if let Op::ReplaceSelectedText { block, range, text } = op
            && let Some((_, current)) = self.0.iter_mut().find(|(id, _)| id == block)
        {
            let end = if applied {
                range.start + text.len()
            } else {
                range.end
            };
            *current = range.start..end;
        }
    }
}

/// Guidance for a request that can edit only its selected fragments.
pub(crate) const SYSTEM: &str = "You help the user edit selected text. The document JSON provides +    read-only Markdown context in `blocks` and the exact editable fragments in `editableText`. +    Use `replace_selected_text` with an ID from `editableText` and the full revised plain-text +    fragment for that ID. Do not include the surrounding text, Markdown markup, or other blocks. +    Leave fragments that do not need changes alone. If no edit is needed, reply without a tool call.";

/// Replaces only the changed part of a selected plain-text fragment.
pub(crate) fn replace(
    doc: &mut Document,
    block: BlockId,
    range: Range<usize>,
    replacement: &str,
) -> Result<bool> {
    let spans = doc.spans_in(block, range.clone())?;
    if replacement.contains(OBJECT) || spans.iter().any(|span| span.atom.is_some()) {
        return Ok(false);
    }
    let selected: String = spans.iter().map(|span| span.text.as_str()).collect();
    if selected == replacement {
        return Ok(false);
    }
    let prefix = common_bytes(selected.chars(), replacement.chars());
    let suffix = common_bytes(
        selected[prefix..].chars().rev(),
        replacement[prefix..].chars().rev(),
    );
    let from = range.start + prefix;
    let to = range.end - suffix;
    let insert = &replacement[prefix..replacement.len() - suffix];
    let reference = prefix.min(selected.len().saturating_sub(1));
    let mut offset = 0;
    let marks = spans
        .into_iter()
        .find(|span| {
            offset += span.text.len();
            offset > reference
        })
        .map(|span| span.marks)
        .unwrap_or_default();
    doc.delete_text(block, from..to)?;
    if !insert.is_empty() {
        doc.insert_spans(block, from, &[Span::new(insert, marks)])?;
    }
    Ok(true)
}

/// Counts the bytes in matching scalar values, without splitting UTF-8.
fn common_bytes(left: impl Iterator<Item = char>, right: impl Iterator<Item = char>) -> usize {
    left.zip(right)
        .take_while(|(left, right)| left == right)
        .map(|(ch, _)| ch.len_utf8())
        .sum()
}
