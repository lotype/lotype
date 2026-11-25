use std::collections::{HashMap, HashSet};

use crate::{BlockId, Document, Error, Pos, Result};

impl Document {
    /// The text from `a` to `b` with the blocks it is in, as a document of
    /// its own: what a copy of that selection holds. A block keeps its
    /// parent when that is copied too, and a table it goes through is
    /// copied whole.
    pub fn slice(&self, a: Pos, b: Pos) -> Result<Document> {
        let (start, end) = self.ordered(a, b)?;
        let mut slice = Document::new(self.schema().clone());
        let in_cell = start.block.column().is_some() || end.block.column().is_some();
        if in_cell && start.block != end.block {
            return Err(Error::TablePart);
        }
        if start.block.column().is_some() {
            let copy = slice.insert_block(None, 0, &slice.text_block()?)?;
            let mut spans = self.spans_in(start.block, start.offset..end.offset)?;
            slice.fit_marks(&mut spans);
            slice.insert_spans(copy, 0, &spans)?;
            return Ok(slice);
        }
        let mut copies = HashMap::new();
        for block in self.span(start.block, end.block)? {
            // Blocks of types this schema does not know are left out too.
            let known = self.spec(block).is_ok();
            let parent = self.parent(block)?;
            let parent = parent.and_then(|parent| copies.get(&parent).copied());
            // A merge can leave blocks in one whose type takes none, or a
            // heading in a block: in the copy they are next to it.
            let top_level = self.spec(block).is_ok_and(|spec| spec.is_top_level());
            let takes = |parent: &BlockId| slice.check_allows_children(Some(*parent)).is_ok();
            let parent = parent.filter(|parent| !top_level && takes(parent));
            let index = slice.children(parent)?.len();
            // A selection goes through a table only whole: its ends are text.
            if known && self.block_type(block)? == "table" {
                if slice.has_tables() {
                    slice.copy_table(self, block, parent, index)?;
                }
                continue;
            }
            if !known || self.is_table_part(block) {
                continue;
            }
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

    /// The blocks, each with what is in it, as a document of its own: what
    /// a copy of a block selection holds.
    pub fn slice_blocks(&self, blocks: &[BlockId]) -> Result<Document> {
        let mut slice = Document::new(self.schema().clone());
        for block in self.outermost(blocks)? {
            let index = slice.children(None)?.len();
            slice.copy_block(self, block, None, index)?;
        }
        Ok(slice)
    }

    /// Each of `blocks` once, in document order, but not one inside another
    /// of them, which goes with that one.
    pub(crate) fn outermost(&self, blocks: &[BlockId]) -> Result<Vec<BlockId>> {
        let chosen: HashSet<_> = blocks.iter().copied().collect();
        let mut outermost = Vec::new();
        for block in &chosen {
            let mut above = self.parent(*block)?;
            while let Some(parent) = above.filter(|parent| !chosen.contains(parent)) {
                above = self.parent(parent)?;
            }
            if above.is_none() {
                outermost.push((self.path(*block)?, *block));
            }
        }
        outermost.sort_by(|a, b| a.0.cmp(&b.0));
        Ok(outermost.into_iter().map(|(_, block)| block).collect())
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
            // Without its rows and cells in the schema, a table is left out.
            if !self.has_tables() {
                return Ok(None);
            }
            return self.copy_table(other, block, parent, index).map(Some);
        }
        if !known || other.is_table_part(block) {
            return Ok(None);
        }
        let copy = match self.insert_block(parent, index, &block_type) {
            // A heading goes in no block: one pasted into a nest is made
            // there for the paste to lift out, as one a peer nested stays.
            Err(Error::TopLevelOnly(_)) => self.create_block(parent, index, &block_type)?,
            copy => copy?,
        };
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

    /// Copies the cells between two cells of one table into a document of
    /// its own, as a table of just those cells.
    pub fn slice_cells(&self, a: BlockId, b: BlockId) -> Result<Document> {
        let cells = self.cells_between(a, b)?;
        // In reading order, row by row: the first row's cells are as many
        // as the columns.
        let columns = cells.iter().filter(|cell| cell.0 == cells[0].0).count();
        let rows = cells.len() / columns;
        let mut slice = Document::new(self.schema().clone());
        let table = slice.insert_table(None, 0, rows, columns)?;
        let copied = (slice.children(Some(table))?, slice.columns(table)?);
        for (at, cell) in cells.into_iter().enumerate() {
            let mut spans = self.spans(cell)?;
            slice.fit_marks(&mut spans);
            let row = copied.0[at / columns];
            slice.insert_spans(slice.cell(row, copied.1[at % columns])?, 0, &spans)?;
        }
        Ok(slice)
    }

    /// The document as plain text: a block a line, and a table's rows a line
    /// each with tabs between its cells, as spreadsheets take them. Blocks
    /// without text, as a divider, are left out.
    pub fn to_plain_text(&self) -> String {
        let mut lines = Vec::new();
        for block in self.blocks() {
            if let Ok(text) = self.plain_text(block) {
                lines.push(text);
                continue;
            }
            let Ok(columns) = self.columns(block) else {
                continue;
            };
            for row in self.children(Some(block)).unwrap_or_default() {
                let cells = columns
                    .iter()
                    .filter_map(|column| self.cell(row, *column).ok());
                let cells = cells.filter_map(|cell| self.plain_text(cell).ok());
                lines.push(cells.collect::<Vec<_>>().join("\t"));
            }
        }
        lines.join("\n")
    }

    /// Whether the schema has tables whole, with their rows and cells.
    fn has_tables(&self) -> bool {
        let known = |name| self.schema().block_spec(name).is_some();
        known("table") && known("table_row") && known("table_cell")
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
