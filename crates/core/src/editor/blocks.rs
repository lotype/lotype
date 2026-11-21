use std::collections::{BTreeMap, HashSet};

use super::{Editor, text_selection};
use crate::{BlockId, Document, Error, Pos, Result, Selection, Span, Value};

impl Editor {
    /// Turns the blocks of the selection into `block_type`, with these
    /// attributes and no others. Blocks without text stay as they are when
    /// the type holds text, and the other way round.
    pub fn set_block(&mut self, block_type: &str, attrs: &[(String, Value)]) -> Result<()> {
        let blocks = self.selected_blocks()?;
        self.apply(|doc, selection| {
            for block in blocks {
                if doc.text_len(block).is_ok() != doc.holds_text(block_type)? {
                    continue;
                }
                for key in doc.attrs(block)?.into_keys() {
                    doc.remove_attr(block, &key)?;
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
        self.shift(Document::indent_block)
    }

    /// Moves the selected blocks out of their parent, as Shift-Tab does.
    /// Blocks that cannot leave stay.
    pub fn outdent(&mut self) -> Result<()> {
        self.shift(Document::outdent_block)
    }

    fn shift(&mut self, shift: fn(&mut Document, BlockId) -> Result<bool>) -> Result<()> {
        let blocks = self.selected_blocks()?;
        self.apply(|doc, selection| {
            // A block inside another selected block moves with that one.
            let chosen: HashSet<_> = blocks.iter().copied().collect();
            let mut outermost = Vec::new();
            for block in blocks {
                if doc
                    .parent(block)?
                    .is_none_or(|parent| !chosen.contains(&parent))
                {
                    outermost.push(block);
                }
            }
            for block in outermost {
                shift(doc, block)?;
            }
            Ok(selection.cloned())
        })
    }

    /// Inserts pasted plain text. A web or mail address becomes a link: on
    /// the selected text when there is some, and else as text of its own.
    pub fn paste_text(&mut self, text: &str) -> Result<()> {
        let (anchor, head) = text_selection(self.selection.as_ref())?;
        let address = text.trim();
        if !is_address(address) || self.doc.mark_spec("link").is_err() {
            return self.insert_text(text);
        }
        self.apply(|doc, selection| {
            if anchor != head {
                doc.mark_range(anchor, head, "link", address)?;
                return Ok(selection.cloned());
            }
            let marks = BTreeMap::from([("link".to_owned(), Value::from(address))]);
            let text = address.to_owned();
            doc.insert_spans(head.block, head.offset, &[Span { text, marks }])?;
            let caret = Pos::new(head.block, head.offset + address.len());
            Ok(Some(Selection::caret(caret)))
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
            None => Err(Error::NoSelection),
        }
    }
}

fn is_address(text: &str) -> bool {
    let schemes = ["https://", "http://", "mailto:"];
    let named = |scheme: &&str| text.len() > scheme.len() && text.starts_with(*scheme);
    schemes.iter().any(named) && !text.contains(char::is_whitespace)
}
