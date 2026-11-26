//! Making a model's edits in a document: the private copy first, and the
//! real one when they are accepted.

use std::collections::{BTreeMap, HashMap};

use lotype_core::{BlockId, ColumnId, Document, Pos, Result, Span, Value, block};

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
struct Content {
    kind: String,
    attrs: BTreeMap<String, Value>,
    /// `None` for a block without text.
    spans: Option<Vec<Span>>,
    table: Option<Table>,
}

/// A table's columns and rich cell contents, in reading order.
#[derive(Debug, Clone, PartialEq)]
struct Table {
    columns: Vec<ColumnId>,
    cells: Vec<Vec<Vec<Span>>>,
}

fn content(doc: &Document, block: BlockId) -> Result<Content> {
    let kind = doc.block_type(block)?;
    let table = if kind == block::TABLE {
        let columns = doc.columns(block)?;
        let cells = doc
            .children(Some(block))?
            .into_iter()
            .map(|row| {
                columns
                    .iter()
                    .map(|column| doc.spans(doc.cell(row, *column)?))
                    .collect()
            })
            .collect::<Result<_>>()?;
        Some(Table { columns, cells })
    } else {
        None
    };
    Ok(Content {
        attrs: doc.attrs(block)?,
        spans: doc.spans(block).ok(),
        kind,
        table,
    })
}

/// How the live document must match the preview before an edit is replayed.
#[derive(Debug)]
pub(crate) struct Before {
    content: Option<Content>,
    inside: Vec<(BlockId, Option<BlockId>, Content)>,
    placement: Option<Placement>,
}

/// The insertion context that determines which Markdown the location allows.
#[derive(Debug)]
struct Placement {
    parent: Option<BlockId>,
    parent_kind: Option<String>,
    depth: usize,
}

impl Before {
    /// Checks structure and content, resolving IDs minted by earlier edits.
    fn matches(&self, actual: &Self, made: &HashMap<BlockId, BlockId>, target: BlockId) -> bool {
        let remap = |block| made.get(&block).copied().unwrap_or(block);
        let same_place = match (&self.placement, &actual.placement) {
            (Some(expected), Some(actual)) => {
                expected.parent.map(remap) == actual.parent
                    && expected.parent_kind == actual.parent_kind
                    && expected.depth == actual.depth
            }
            (None, None) => true,
            _ => false,
        };
        same_place
            && same_content(
                self.content.as_ref(),
                actual.content.as_ref(),
                made.contains_key(&target),
            )
            && self.inside.len() == actual.inside.len()
            && self.inside.iter().zip(&actual.inside).all(
                |((id, parent, content), (actual_id, actual_parent, actual_content))| {
                    remap(*id) == *actual_id
                        && parent.map(remap) == *actual_parent
                        && same_content(Some(content), Some(actual_content), made.contains_key(id))
                },
            )
    }
}

/// Compares rich content; a table replayed from this draft receives new column IDs.
fn same_content(expected: Option<&Content>, actual: Option<&Content>, reminted: bool) -> bool {
    if reminted
        && let (Some(expected), Some(actual)) = (expected, actual)
        && let (Some(table), Some(other)) = (&expected.table, &actual.table)
    {
        return expected.kind == actual.kind
            && expected.attrs == actual.attrs
            && expected.spans == actual.spans
            && table.columns.len() == other.columns.len()
            && table.cells == other.cells;
    }
    expected == actual
}

/// What an edit did.
#[derive(Debug)]
pub(crate) struct Effect {
    /// The blocks it made, with those in them, in reading order.
    pub(crate) made: Vec<BlockId>,
    /// What the block it changed or removed was.
    pub(crate) before: Before,
    pub(crate) location: EditLocation,
    pub(crate) caret: Option<Pos>,
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
    expect: Option<&Before>,
) -> Result<Option<Effect>> {
    let (Op::Add {
        reference: block, ..
    }
    | Op::Update { block, .. }
    | Op::ReplaceSelectedText { block, .. }
    | Op::Delete { block }) = op;
    let target = made.get(block).copied().unwrap_or(*block);
    if !doc.contains(target)
        || target.column().is_some()
        || doc.block_type(target)? == block::TABLE_ROW
    {
        return Ok(None);
    }
    let parent = doc.parent(target)?;
    let update = match op {
        Op::Update { markdown, .. } => {
            let Some(update) = Update::read(doc, target, parent, markdown)? else {
                return Ok(None);
            };
            Some(update)
        }
        _ => None,
    };
    let replaces = matches!(&update, Some(Update::Replace(_)));
    let destructive = replaces || matches!(op, Op::Delete { .. });
    let positioned = replaces || matches!(op, Op::Add { .. });
    let before = Before {
        content: (!matches!(op, Op::Add { .. }))
            .then(|| content(doc, target))
            .transpose()?,
        inside: if destructive {
            with_inside(doc, target)?
                .into_iter()
                .skip(1)
                .map(|block| Ok((block, doc.parent(block)?, content(doc, block)?)))
                .collect::<Result<_>>()?
        } else {
            Vec::new()
        },
        placement: positioned
            .then(|| -> Result<Placement> {
                Ok(Placement {
                    parent,
                    parent_kind: parent.map(|parent| doc.block_type(parent)).transpose()?,
                    depth: doc.path(target)?.len(),
                })
            })
            .transpose()?,
    };
    if expect.is_some_and(|expect| !expect.matches(&before, made, *block)) {
        return Ok(None);
    }
    let (made, location, caret) = match op {
        Op::Add { before, blocks, .. } => {
            let mut index = doc.index_of(target)? + usize::from(!before);
            let mut made = Vec::new();
            for markdown in blocks {
                let top = doc.insert_markdown(parent, index, markdown)?;
                index += top.len();
                for block in top {
                    made.extend(with_inside(doc, block)?);
                }
            }
            let block = made.last().copied().unwrap_or(target);
            (made, EditLocation::Block(block), text_end(doc, block))
        }
        Op::Update { .. } => {
            let Some(update) = update else {
                return Ok(None);
            };
            update.apply(doc, target, parent)?
        }
        Op::Delete { .. } => {
            let index = doc.index_of(target)?;
            doc.remove_block(target)?;
            (Vec::new(), removal_at(doc, parent, index)?, None)
        }
        Op::ReplaceSelectedText { range, text, .. } => {
            if !crate::selected::replace(doc, target, range.clone(), text)? {
                return Ok(None);
            }
            let caret = Some(Pos::new(target, range.start + text.len()));
            (Vec::new(), EditLocation::Block(target), caret)
        }
    };
    Ok(Some(Effect {
        made,
        before,
        location,
        caret,
        target,
    }))
}

fn text_end(doc: &Document, block: BlockId) -> Option<Pos> {
    doc.text_len(block).ok().map(|end| Pos::new(block, end))
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

/// A parsed update, ready to apply without moving or discarding untargeted blocks.
enum Update {
    /// The block itself with another type, attributes or text: the text
    /// stays as it is where the block holds none.
    Text {
        kind: String,
        attrs: BTreeMap<String, Value>,
        spans: Option<Vec<Span>>,
    },
    Clear,
    Replace(Box<Document>),
}

impl Update {
    /// Parses the Markdown and refuses updates that would affect unrelated blocks.
    fn read(
        doc: &Document,
        block: BlockId,
        parent: Option<BlockId>,
        markdown: &str,
    ) -> Result<Option<Self>> {
        let mut new = Document::new(doc.schema().clone());
        let top = new.insert_markdown(None, 0, markdown)?;
        let kind = doc.block_type(block)?;
        let children = doc.children(Some(block))?;
        if doc.text_len(block).is_ok() {
            if top.is_empty() {
                return Ok(Some(Self::Clear));
            }
            if let [one] = top.as_slice()
                && new.children(Some(*one))?.is_empty()
                && let Ok(spans) = new.spans(*one)
            {
                let new_kind = new.block_type(*one)?;
                if kind != new_kind
                    && new.schema().block_spec(&new_kind).is_some_and(|spec| {
                        (parent.is_some() && spec.is_top_level())
                            || (!children.is_empty() && !spec.allows_children())
                    })
                {
                    return Ok(None);
                }
                return Ok(Some(Self::Text {
                    kind: new_kind,
                    attrs: new.attrs(*one)?,
                    spans: Some(spans),
                }));
            }
        }
        // A block without text that stays of its type is not made again:
        // it keeps its place, what is in it, and what Markdown says
        // nothing of.
        if let [one] = top.as_slice()
            && doc.text_len(block).is_err()
            && kind != block::TABLE
            && new.block_type(*one)? == kind
            && new.children(Some(*one))?.is_empty()
        {
            let (attrs, spans) = (new.attrs(*one)?, None);
            return Ok(Some(Self::Text { kind, attrs, spans }));
        }
        if !children.is_empty() {
            if kind != block::TABLE {
                return Ok(None);
            }
            for row in children {
                if doc.block_type(row)? != block::TABLE_ROW || !doc.children(Some(row))?.is_empty()
                {
                    return Ok(None);
                }
            }
        }
        if parent.is_some() {
            for block in top {
                let kind = new.block_type(block)?;
                if new
                    .schema()
                    .block_spec(&kind)
                    .is_some_and(lotype_core::BlockSpec::is_top_level)
                {
                    return Ok(None);
                }
            }
        }
        Ok(Some(Self::Replace(Box::new(new))))
    }

    /// Makes the checked update and returns its new blocks, location and caret.
    fn apply(
        self,
        doc: &mut Document,
        block: BlockId,
        parent: Option<BlockId>,
    ) -> Result<(Vec<BlockId>, EditLocation, Option<Pos>)> {
        match self {
            Self::Text { kind, attrs, spans } => {
                // Markdown says some of a block's attributes, as a heading's
                // level: those it no longer says go. The others, as colors
                // and alignment, it never said, and they stay.
                let was = doc.block_type(block)?;
                let said = doc.schema().block_spec(&was);
                let said = said.map_or_else(Vec::new, |spec| spec.markdown_attrs().to_vec());
                if was != kind {
                    doc.set_block_type(block, &kind)?;
                }
                let has = doc.attrs(block)?;
                for key in said.iter().filter(|key| !attrs.contains_key(*key)) {
                    if has.contains_key(key) {
                        doc.remove_attr(block, key)?;
                    }
                }
                for (key, value) in attrs {
                    doc.set_attr(block, &key, value)?;
                }
                // shortcut: all of the text again where any of it changed, so a
                // peer's caret in it goes to its start; replace only what
                // differs when that is seen to matter.
                if let Some(spans) = spans
                    && doc.spans(block)? != spans
                {
                    doc.delete_text(block, 0..doc.text_len(block)?)?;
                    doc.insert_spans(block, 0, &spans)?;
                }
                Ok((Vec::new(), EditLocation::Block(block), text_end(doc, block)))
            }
            // No text to keep: an empty block of the type it was.
            Self::Clear => {
                doc.delete_text(block, 0..doc.text_len(block)?)?;
                let caret = Some(Pos::new(block, 0));
                Ok((Vec::new(), EditLocation::Block(block), caret))
            }
            Self::Replace(new) => {
                let index = doc.index_of(block)?;
                let top = doc.insert_document(parent, index, &new)?;
                doc.remove_block(block)?;
                let mut made = Vec::new();
                for block in top {
                    made.extend(with_inside(doc, block)?);
                }
                let (location, caret) = match made.last() {
                    Some(&block) => (EditLocation::Block(block), text_end(doc, block)),
                    None => (removal_at(doc, parent, index)?, None),
                };
                Ok((made, location, caret))
            }
        }
    }
}
