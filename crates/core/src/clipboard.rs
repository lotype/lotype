use std::collections::HashMap;

use crate::{Document, Pos, Result};

impl Document {
    /// The text from `a` to `b` with the blocks it is in, as a document of
    /// its own: what a copy of that selection holds. A block keeps its
    /// parent when that is copied too. Tables are left out.
    pub fn slice(&self, a: Pos, b: Pos) -> Result<Document> {
        let (start, end) = self.ordered(a, b)?;
        let mut slice = Document::new(self.schema().clone());
        if start.block.column().is_some() {
            let copy = slice.insert_block(None, 0, "paragraph")?;
            let spans = self.spans_in(start.block, start.offset..end.offset)?;
            slice.insert_spans(copy, 0, &spans)?;
            return Ok(slice);
        }
        let mut copies = HashMap::new();
        for block in self.span(start.block, end.block)? {
            // Blocks of types this schema does not know are left out too.
            let known = self.spec(block).is_ok();
            if !known || self.is_table_part(block) || self.block_type(block)? == "table" {
                continue;
            }
            let parent = self.parent(block)?;
            let parent = parent.and_then(|parent| copies.get(&parent).copied());
            let index = slice.children(parent)?.len();
            let copy = slice.insert_block(parent, index, &self.block_type(block)?)?;
            for (key, value) in self.attrs(block)? {
                slice.set_attr(copy, &key, value)?;
            }
            if let Ok(len) = self.text_len(block) {
                let from = if block == start.block {
                    start.offset
                } else {
                    0
                };
                let to = if block == end.block { end.offset } else { len };
                slice.insert_spans(copy, 0, &self.spans_in(block, from..to)?)?;
            }
            copies.insert(block, copy);
        }
        Ok(slice)
    }
}
