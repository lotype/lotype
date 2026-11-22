use std::collections::HashSet;

use super::{Editor, text_selection};
use crate::{BlockId, Content, Document, Error, Pos, Result, Selection, Span, Value};

impl Editor {
    /// Turns the blocks of the selection into `block_type`, with these
    /// attributes and no others. A block stays as it is when it cannot
    /// become that: when one of the two holds text and the other does not,
    /// when it has children and the type takes none, or when its own type
    /// is unknown here.
    pub fn set_block(&mut self, block_type: &str, attrs: &[(String, Value)]) -> Result<()> {
        let blocks = self.selected_blocks()?;
        self.apply(|doc, selection| {
            let (holds_text, nests) = {
                let spec = doc.schema().block_spec(block_type);
                let spec = spec.ok_or_else(|| Error::UnknownBlockType(block_type.to_owned()))?;
                (spec.content() == Content::Inline, spec.allows_children())
            };
            for block in blocks {
                let fits = doc.spec(block).is_ok()
                    && !doc.is_table_part(block)
                    && doc.text_len(block).is_ok() == holds_text
                    && (nests || doc.children(Some(block))?.is_empty());
                if !fits {
                    continue;
                }
                // Only what changes, so that asking for what a block already
                // is makes no undo step.
                for key in doc.attrs(block)?.into_keys() {
                    if !attrs.iter().any(|(kept, _)| *kept == key) {
                        doc.remove_attr(block, &key)?;
                    }
                }
                doc.set_block_type(block, block_type)?;
                for (key, value) in attrs {
                    doc.set_attr(block, key, value.clone())?;
                }
            }
            Ok(selection.cloned())
        })
    }

    /// Nests the selected blocks under the block before them, as Tab does.
    /// Blocks that cannot go there stay.
    pub fn indent(&mut self) -> Result<()> {
        self.shift(false, Document::indent_block)
    }

    /// Moves the selected blocks out of their parent, as Shift-Tab does.
    /// Blocks that cannot leave stay.
    pub fn outdent(&mut self) -> Result<()> {
        self.shift(true, Document::outdent_block)
    }

    fn shift(
        &mut self,
        backward: bool,
        shift: fn(&mut Document, BlockId) -> Result<bool>,
    ) -> Result<()> {
        let blocks = self.selected_blocks()?;
        self.apply(|doc, selection| {
            // Each block once, and not one inside another selected block,
            // which moves with that one.
            let chosen: HashSet<_> = blocks.into_iter().collect();
            let mut outermost = Vec::new();
            for block in &chosen {
                let mut above = doc.parent(*block)?;
                while let Some(parent) = above.filter(|parent| !chosen.contains(parent)) {
                    above = doc.parent(parent)?;
                }
                if above.is_none() {
                    outermost.push((doc.path(*block)?, *block));
                }
            }
            outermost.sort_by(|a, b| a.0.cmp(&b.0));
            // Moving out starts at the end: a block that takes no children
            // cannot leave while a sibling still follows it.
            if backward {
                outermost.reverse();
            }
            for (_, block) in outermost {
                shift(doc, block)?;
            }
            Ok(selection.cloned())
        })
    }

    /// Inserts pasted plain text, as a step of its own and as it is: no
    /// input rule reads it. A web or mail address becomes a link: on the
    /// selected text when there is some, and else as text of its own.
    pub fn paste_text(&mut self, text: &str) -> Result<()> {
        let (anchor, head) = text_selection(self.selection.as_ref())?;
        let address = text.trim();
        let links = is_address(address) && self.doc.mark_spec("link").is_ok();
        let selected = !self.doc.segments(anchor, head)?.is_empty();
        let mut marks = self.active_marks()?;
        if links {
            marks.insert("link".to_owned(), Value::from(address));
        }
        self.apply(|doc, selection| {
            if links && selected {
                doc.mark_range(anchor, head, "link", address)?;
                return Ok(selection.cloned());
            }
            let text = if links { address } else { text }.to_owned();
            let caret = doc.delete_range(anchor, head)?;
            let end = Pos::new(caret.block, caret.offset + text.len());
            doc.insert_spans(caret.block, caret.offset, &[Span { text, marks }])?;
            Ok(Some(Selection::caret(end)))
        })
    }

    /// The blocks the selection touches, in document order for a text selection.
    fn selected_blocks(&self) -> Result<Vec<BlockId>> {
        match &self.selection {
            Some(Selection::Text { anchor, head }) => {
                let (start, end) = self.doc.ordered(*anchor, *head)?;
                self.doc.span(start.block, end.block)
            }
            Some(Selection::Blocks(blocks)) => Ok(blocks.clone()),
            // Cells are no blocks to retype or move.
            Some(Selection::Cells { .. }) | None => Err(Error::NoSelection),
        }
    }
}

fn is_address(text: &str) -> bool {
    let schemes = ["https://", "http://", "mailto:"];
    let named = |scheme: &&str| text.len() > scheme.len() && text.starts_with(*scheme);
    schemes.iter().any(named) && !text.contains(char::is_whitespace)
}
