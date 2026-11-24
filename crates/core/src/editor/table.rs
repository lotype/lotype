//! Rows and columns added to and taken from a table, each an undo step with
//! the caret where the writing goes on.

use super::Editor;
use super::typing::next_text_block;
use crate::{BlockId, ColumnId, Document, Error, Pos, Result, Selection};

/// Where a cell is: its table, the index of its row and of its column.
fn place(doc: &Document, cell: BlockId) -> Result<(BlockId, usize, usize)> {
    let column = cell.column().ok_or(Error::NotATable)?;
    let row = doc.parent(cell)?.ok_or(Error::NotATable)?;
    let table = doc.parent(row)?.ok_or(Error::NotATable)?;
    let at_row = doc.index_of(row)?;
    let at_column = doc.columns(table)?.iter().position(|id| *id == column);
    Ok((table, at_row, at_column.ok_or(Error::BlockNotFound)?))
}

/// The caret at the end of the cell of `table` at `row` and `column`.
fn caret_in(doc: &Document, table: BlockId, row: usize, column: usize) -> Result<Selection> {
    let rows = doc.children(Some(table))?;
    let columns = doc.columns(table)?;
    let cell = doc.cell(rows[row], columns[column])?;
    Ok(Selection::caret(Pos::new(cell, doc.text_len(cell)?)))
}

/// Removes `table`, and gives the caret the text after it, or before it, or
/// a paragraph in its place where there is no other text.
fn remove_table(doc: &mut Document, table: BlockId) -> Result<Option<Selection>> {
    let parent = doc.parent(table)?;
    let index = doc.index_of(table)?;
    let after = next_text_block(doc, table)?.filter(|block| block.column().is_none());
    let mut before = doc.prev_block(table)?;
    while let Some(block) = before.filter(|block| doc.text_len(*block).is_err()) {
        before = doc.prev_block(block)?;
    }
    doc.remove_block(table)?;
    let caret = match (after, before) {
        (Some(after), _) => Pos::new(after, 0),
        (None, Some(before)) => Pos::new(before, doc.text_len(before)?),
        (None, None) => Pos::new(doc.create_block(parent, index, "paragraph")?, 0),
    };
    Ok(Some(Selection::caret(caret)))
}

impl Editor {
    /// The cell the caret is in, or the head of a selection of cells.
    pub fn current_cell(&self) -> Option<BlockId> {
        let cell = match self.selection.as_ref()? {
            Selection::Text { head, .. } => head.block,
            Selection::Cells { head, .. } => *head,
            Selection::Blocks(_) => return None,
        };
        cell.column().is_some().then_some(cell)
    }

    /// Adds a row to `table` at `index`, as one undo step, with the caret in
    /// its first cell.
    pub fn add_row(&mut self, table: BlockId, index: usize) -> Result<()> {
        self.apply(|doc, _| {
            doc.insert_row(table, index)?;
            caret_in(doc, table, index, 0).map(Some)
        })
    }

    /// Adds a column to `table` at `index`, as one undo step, with the caret
    /// in its cell of the row the caret is in, or of the first row.
    pub fn add_column(&mut self, table: BlockId, index: usize) -> Result<()> {
        let current = self.current_cell();
        self.apply(|doc, _| {
            let row = match current.map(|cell| place(doc, cell)) {
                Some(Ok((at, row, _))) if at == table => row,
                _ => 0,
            };
            doc.insert_column(table, index)?;
            caret_in(doc, table, row, index).map(Some)
        })
    }

    /// Removes the row `cell` is in, as one undo step, and the table with
    /// its last row. The caret goes to the cell of its column in the row
    /// that takes its place.
    pub fn delete_row(&mut self, cell: BlockId) -> Result<()> {
        self.apply(|doc, _| {
            let (table, row, column) = place(doc, cell)?;
            let rows = doc.children(Some(table))?;
            if rows.len() == 1 {
                return remove_table(doc, table);
            }
            doc.remove_block(rows[row])?;
            caret_in(doc, table, row.min(rows.len() - 2), column).map(Some)
        })
    }

    /// Removes the column `cell` is in, as one undo step, and the table
    /// with its last column. The caret goes to the cell of its row in the
    /// column that takes its place.
    pub fn delete_column(&mut self, cell: BlockId) -> Result<()> {
        self.apply(|doc, _| {
            let (table, row, column) = place(doc, cell)?;
            let columns: Vec<ColumnId> = doc.columns(table)?;
            if columns.len() == 1 {
                return remove_table(doc, table);
            }
            doc.remove_column(table, columns[column])?;
            caret_in(doc, table, row, column.min(columns.len() - 2)).map(Some)
        })
    }
}
