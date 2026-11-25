//! Making a model's edits in a document: the private copy first, and the
//! real one when they are accepted.

use std::collections::{BTreeMap, HashMap};

use lotype_core::{BlockId, Document, Result, Span, Value, block};

use crate::Op;

/// Where the model last made an edit in the preview.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditLocation {
    /// A block the model added or updated.
    Block(BlockId),
    /// A removal, marked on a nearby surviving block or on the empty document.
    Removal(Option<BlockId>),
}

/// What a block is by itself, to tell whether someone changed it.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Content {
    kind: String,
    attrs: BTreeMap<String, Value>,
    /// `None` for a block without text.
    spans: Option<Vec<Span>>,
    /// A table's rows and cells, which are no blocks a model is shown.
    table: Option<String>,
}

fn content(doc: &Document, block: BlockId) -> Result<Content> {
    let kind = doc.block_type(block)?;
    let table = match kind == block::TABLE {
        true => Some(doc.slice_blocks(&[block])?.to_markdown()),
        false => None,
    };
    Ok(Content {
        attrs: doc.attrs(block)?,
        spans: doc.spans(block).ok(),
        kind,
        table,
    })
}

/// What an edit did.
#[derive(Debug)]
pub(crate) struct Effect {
    /// The blocks it made, with those in them, in reading order.
    pub(crate) made: Vec<BlockId>,
    /// What the block it changed or removed was.
    pub(crate) before: Option<Content>,
    pub(crate) location: EditLocation,
    pub(crate) target: BlockId,
}

/// `block` and the blocks in it, in reading order.
fn with_inside(doc: &Document, block: BlockId) -> Result<Vec<BlockId>> {
    let (mut all, mut stack) = (Vec::new(), vec![block]);
    while let Some(block) = stack.pop() {
        all.push(block);
        stack.extend(doc.children(Some(block))?.into_iter().rev());
    }
    Ok(all)
}

/// Makes an edit in `doc`. `None` where it cannot be made as it was meant:
/// its block is not there, or, with `expect`, is no longer what it was when
/// the edit was made in the copy, so that what is accepted is what was
/// seen. `made` has the blocks of `doc` that earlier edits made, by what
/// they are in the copy.
pub(crate) fn run(
    doc: &mut Document,
    op: &Op,
    made: &HashMap<BlockId, BlockId>,
    expect: Option<&Content>,
) -> Result<Option<Effect>> {
    let (Op::Add {
        reference: block, ..
    }
    | Op::Update { block, .. }
    | Op::Delete { block }) = op;
    let target = made.get(block).copied().unwrap_or(*block);
    if !doc.contains(target) {
        return Ok(None);
    }
    let before = match op {
        Op::Add { .. } => None,
        _ => Some(content(doc, target)?),
    };
    if expect.is_some_and(|expect| Some(expect) != before.as_ref()) {
        return Ok(None);
    }
    let (made, location) = match op {
        Op::Add { before, blocks, .. } => {
            let parent = doc.parent(target)?;
            let mut index = doc.index_of(target)? + usize::from(!before);
            let mut made = Vec::new();
            for markdown in blocks {
                let top = doc.insert_markdown(parent, index, markdown)?;
                index += top.len();
                for block in top {
                    made.extend(with_inside(doc, block)?);
                }
            }
            let location = EditLocation::Block(made.last().copied().unwrap_or(target));
            (made, location)
        }
        Op::Update { markdown, .. } => update(doc, target, markdown)?,
        Op::Delete { .. } => {
            let (parent, index) = (doc.parent(target)?, doc.index_of(target)?);
            doc.remove_block(target)?;
            (Vec::new(), removal_at(doc, parent, index)?)
        }
    };
    Ok(Some(Effect {
        made,
        before,
        location,
        target,
    }))
}

/// Locates a removed row at its next sibling, previous sibling, or parent.
fn removal_at(doc: &Document, parent: Option<BlockId>, index: usize) -> Result<EditLocation> {
    let siblings = doc.children(parent)?;
    let nearby = siblings
        .get(index)
        .or_else(|| siblings.last())
        .copied()
        .or(parent);
    Ok(EditLocation::Removal(nearby))
}

/// Makes `block` what `markdown` says. Text for a block with text stays in
/// the block, which keeps its id and what is in it; anything else takes
/// the block's place. Returns the blocks made then and the edit's location.
fn update(
    doc: &mut Document,
    block: BlockId,
    markdown: &str,
) -> Result<(Vec<BlockId>, EditLocation)> {
    let mut new = Document::new(doc.schema().clone());
    let top = new.insert_markdown(None, 0, markdown)?;
    let had_text = doc.spans(block).ok();
    let one = match top.as_slice() {
        [one] if new.children(Some(*one))?.is_empty() => Some(*one),
        _ => None,
    };
    let text = one.and_then(|one| new.spans(one).ok());
    match (had_text, one, text) {
        (Some(old), Some(one), Some(spans)) => {
            let kind = new.block_type(one)?;
            if doc.block_type(block)? != kind {
                doc.set_block_type(block, &kind)?;
            }
            let attrs = new.attrs(one)?;
            for key in doc
                .attrs(block)?
                .keys()
                .filter(|key| !attrs.contains_key(*key))
            {
                doc.remove_attr(block, key)?;
            }
            for (key, value) in attrs {
                doc.set_attr(block, &key, value)?;
            }
            // shortcut: all of the text again where any of it changed, so a
            // peer's caret in it goes to its start; replace only what
            // differs when that is seen to matter.
            if old != spans {
                doc.delete_text(block, 0..doc.text_len(block)?)?;
                doc.insert_spans(block, 0, &spans)?;
            }
            Ok((Vec::new(), EditLocation::Block(block)))
        }
        // No text to keep: an empty block of the type it was.
        (Some(_), None, _) if top.is_empty() => {
            doc.delete_text(block, 0..doc.text_len(block)?)?;
            Ok((Vec::new(), EditLocation::Block(block)))
        }
        // shortcut: what was in the block goes with it. A model shown a
        // block alone gives one block for it, save for tables and the like.
        _ => {
            let (parent, index) = (doc.parent(block)?, doc.index_of(block)?);
            let top = doc.insert_document(parent, index, &new)?;
            doc.remove_block(block)?;
            let mut made = Vec::new();
            for block in top {
                made.extend(with_inside(doc, block)?);
            }
            let location = match made.last() {
                Some(block) => EditLocation::Block(*block),
                None => removal_at(doc, parent, index)?,
            };
            Ok((made, location))
        }
    }
}
