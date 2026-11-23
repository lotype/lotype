use std::collections::HashMap;

use crate::{BlockId, Document, Pos, Result};

impl Document {
    /// The text from `a` to `b` with the blocks it is in, as a document of
    /// its own: what a copy of that selection holds. A block keeps its
    /// parent when that is copied too. Tables are left out.
    pub fn slice(&self, a: Pos, b: Pos) -> Result<Document> {
        let (start, end) = self.ordered(a, b)?;
        let mut slice = Document::new(self.schema().clone());
        if start.block.column().is_some() {
            let copy = slice.insert_block(None, 0, "paragraph")?;
            let mut spans = self.spans_in(start.block, start.offset..end.offset)?;
            slice.fit_marks(&mut spans);
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
                // Marks of a peer with another schema would stop the copy.
                let mut spans = self.spans_in(block, from..to)?;
                slice.fit_marks(&mut spans);
                slice.insert_spans(copy, 0, &spans)?;
            }
            copies.insert(block, copy);
        }
        Ok(slice)
    }

    /// Copies the blocks of `other`, with what is in them, among the
    /// children of `parent` from `index` on, as a paste does. Returns those
    /// it made at that level. What this schema lacks is left out: blocks of
    /// unknown types with what is in them, marks and atoms.
    pub fn insert_document(
        &mut self,
        parent: Option<BlockId>,
        index: usize,
        other: &Document,
    ) -> Result<Vec<BlockId>> {
        let mut made = Vec::new();
        for block in other.children(None)? {
            if let Some(copy) = self.copy_block(other, block, parent, index + made.len())? {
                made.push(copy);
            }
        }
        Ok(made)
    }

    fn copy_block(
        &mut self,
        other: &Document,
        block: BlockId,
        parent: Option<BlockId>,
        index: usize,
    ) -> Result<Option<BlockId>> {
        let block_type = other.block_type(block)?;
        let known = self.schema().block_spec(&block_type).is_some();
        if known && block_type == "table" {
            return self.copy_table(other, block, parent, index).map(Some);
        }
        if !known || other.is_table_part(block) {
            return Ok(None);
        }
        let copy = self.insert_block(parent, index, &block_type)?;
        for (key, value) in other.attrs(block)? {
            self.set_attr(copy, &key, value)?;
        }
        if let Ok(mut spans) = other.spans(block) {
            self.fit_marks(&mut spans);
            self.insert_spans(copy, 0, &spans)?;
        }
        // Children of a type that takes none are left out too.
        if self.check_allows_children(Some(copy)).is_ok() {
            let mut index = 0;
            for child in other.children(Some(block))? {
                index += usize::from(self.copy_block(other, child, Some(copy), index)?.is_some());
            }
        }
        Ok(Some(copy))
    }

    /// Copies a table with the text of its cells. Its rows and columns are
    /// all copied, whatever their attributes.
    fn copy_table(
        &mut self,
        other: &Document,
        table: BlockId,
        parent: Option<BlockId>,
        index: usize,
    ) -> Result<BlockId> {
        let (rows, columns) = (other.children(Some(table))?, other.columns(table)?);
        let copy = self.insert_table(parent, index, rows.len(), columns.len())?;
        let copied = (self.children(Some(copy))?, self.columns(copy)?);
        for (row, copied_row) in rows.into_iter().zip(copied.0) {
            for (column, copied_column) in columns.iter().zip(&copied.1) {
                let mut spans = other.spans(other.cell(row, *column)?)?;
                self.fit_marks(&mut spans);
                self.insert_spans(self.cell(copied_row, *copied_column)?, 0, &spans)?;
            }
        }
        Ok(copy)
    }
}
