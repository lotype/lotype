use std::fmt;
use std::str::FromStr;

use loro::{ID, LoroMovableList, LoroText, TreeID, TreeParentId};

use crate::{BlockId, Document, Error, Result};

pub(crate) const TABLE: &str = "table";
pub(crate) const ROW: &str = "table_row";
pub(crate) const CELL: &str = "table_cell";
/// The key of a table's list of column ids.
pub(crate) const COLUMNS: &str = "$cols";

/// The identity of a column of a table. It stays the same when the column moves.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ColumnId(pub(crate) ID);

impl fmt::Display for ColumnId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl FromStr for ColumnId {
    type Err = Error;

    fn from_str(text: &str) -> Result<Self> {
        let id = ID::try_from(text).map_err(|_| Error::InvalidBlockId(text.to_owned()))?;
        Ok(Self(id))
    }
}

/// What the key of a cell's text starts with, in the metadata of its row.
pub(crate) const CELL_KEY: &str = "$cell:";

pub(crate) fn cell_key(column: ColumnId) -> String {
    format!("{CELL_KEY}{column}")
}

/// Tables. A table is a block whose children are its rows, and it lists its
/// columns by id. A cell is a row and a column: it is no node of its own, so
/// a row and a column added at the same time by two peers meet in a cell
/// without either of them making it, and no table ever has rows of different
/// lengths. The text of a cell is a child of the row's metadata that Loro
/// gives the same identity on every peer, so what two peers type into a cell
/// neither had before merges like any other text.
///
/// A cell has a [`BlockId`], and everything that reads or edits a block's
/// text takes it. It is not among [`Document::blocks`] or the children of its
/// row: get it with [`Document::cell`].
impl Document {
    /// Inserts a table with empty cells. Returns the table.
    pub fn insert_table(
        &mut self,
        parent: Option<BlockId>,
        index: usize,
        rows: usize,
        columns: usize,
    ) -> Result<BlockId> {
        let table = self.insert_block(parent, index, TABLE)?;
        for at in 0..columns {
            self.insert_column(table, at)?;
        }
        for at in 0..rows {
            self.insert_row(table, at)?;
        }
        Ok(table)
    }

    /// Inserts a row at `index` among the rows of `table`. A row is a block:
    /// [`Document::children`] of the table lists the rows, and a row is
    /// removed and moved like any block.
    pub fn insert_row(&mut self, table: BlockId, index: usize) -> Result<BlockId> {
        self.column_list(table)?;
        self.create_block(Some(table), index, ROW)
    }

    /// The columns of `table`, in order.
    pub fn columns(&self, table: BlockId) -> Result<Vec<ColumnId>> {
        self.column_list(table)?;
        Ok(self.column_ids(table.0))
    }

    /// Inserts a column at `index` among the columns of `table`.
    pub fn insert_column(&mut self, table: BlockId, index: usize) -> Result<ColumnId> {
        // The id of the next operation, which no other column can have.
        let peer = self.doc.peer_id();
        let done = self.doc.oplog_vv().get(&peer).copied().unwrap_or(0);
        let pending = i32::try_from(self.doc.get_pending_txn_len()).unwrap_or(i32::MAX);
        let column = ColumnId(ID::new(peer, done.saturating_add(pending)));
        self.put_column(table, index, column)?;
        Ok(column)
    }

    fn put_column(&mut self, table: BlockId, index: usize, column: ColumnId) -> Result<()> {
        let list = self.column_list(table)?;
        let len = list.len();
        if index > len {
            return Err(Error::OutOfBounds { index, len });
        }
        list.insert(index, column.to_string())?;
        self.record(|| Box::new(move |doc| doc.put_column(table, index, column)));
        Ok(())
    }

    /// Removes a column. The text of its cells stays in the shared data,
    /// where an editor's undo finds it.
    pub fn remove_column(&mut self, table: BlockId, column: ColumnId) -> Result<()> {
        let at = self.column_index(table, column)?;
        self.column_list(table)?.delete(at, 1)?;
        self.record(|| Box::new(move |doc| doc.remove_column(table, column)));
        Ok(())
    }

    /// Moves a column so that it ends up at `index` among the columns.
    pub fn move_column(&mut self, table: BlockId, column: ColumnId, index: usize) -> Result<()> {
        let at = self.column_index(table, column)?;
        let list = self.column_list(table)?;
        let len = list.len();
        if index >= len {
            return Err(Error::OutOfBounds { index, len });
        }
        list.mov(at, index)?;
        self.record(|| Box::new(move |doc| doc.move_column(table, column, index)));
        Ok(())
    }

    /// The cell of `row` in `column`.
    pub fn cell(&self, row: BlockId, column: ColumnId) -> Result<BlockId> {
        let cell = BlockId(self.live(row)?, Some(column));
        self.depth(cell).map(|_| cell)
    }

    fn column_index(&self, table: BlockId, column: ColumnId) -> Result<usize> {
        let columns = self.columns(table)?;
        let at = columns.iter().position(|other| *other == column);
        at.ok_or(Error::BlockNotFound)
    }

    fn column_list(&self, table: BlockId) -> Result<LoroMovableList> {
        let list = self.tree.get_meta(self.live(table)?)?.get(COLUMNS);
        let list = list.and_then(|list| list.into_container().ok()?.into_movable_list().ok());
        list.ok_or(Error::NotATable)
    }

    /// The column ids a node lists. A peer can write anything there: what is
    /// no id is skipped, and an id listed twice counts once.
    pub(crate) fn column_ids(&self, table: TreeID) -> Vec<ColumnId> {
        let list = self
            .tree
            .get_meta(table)
            .ok()
            .and_then(|meta| meta.get(COLUMNS));
        let list = list.and_then(|list| list.into_container().ok()?.into_movable_list().ok());
        let mut columns: Vec<ColumnId> = Vec::new();
        for value in list.map(|list| list.to_vec()).unwrap_or_default() {
            let column = value.as_string().and_then(|text| text.parse().ok());
            columns.extend(column.filter(|column| !columns.contains(column)));
        }
        columns
    }

    /// The table a row is in, if its parent lists `column`.
    pub(crate) fn table_of(&self, row: TreeID, column: ColumnId) -> Option<TreeID> {
        let Some(TreeParentId::Node(table)) = self.tree.parent(row) else {
            return None;
        };
        self.column_ids(table).contains(&column).then_some(table)
    }

    /// The text of a cell that something was written in, or an empty text
    /// that belongs to nothing: a cell has no text of its own until then.
    pub(crate) fn cell_text(&self, row: TreeID, column: ColumnId) -> Result<LoroText> {
        let text = self.tree.get_meta(row)?.get(&cell_key(column));
        let text = text.and_then(|text| text.into_container().ok()?.into_text().ok());
        Ok(text.unwrap_or_default())
    }

    /// Gives a cell its text. Loro notes that in the row's metadata, and an
    /// undo step that held the note would take the cell's text from view
    /// when undone, with what any peer wrote there. So an editor does this
    /// before the step that writes in the cell, not in it.
    pub(crate) fn make_cell(&self, row: TreeID, column: ColumnId) -> Result<LoroText> {
        let text = self
            .tree
            .get_meta(row)?
            .ensure_mergeable_text(&cell_key(column))?;
        if let Some(draft) = &self.draft {
            draft.made_cell(row, column);
        }
        Ok(text)
    }

    /// The cells between two cells of one table, both included: every cell
    /// in their rows and the rows between that is in their columns or the
    /// columns between.
    pub(crate) fn cells_between(&self, a: BlockId, b: BlockId) -> Result<Vec<BlockId>> {
        let (Some(first), Some(second)) = (a.1, b.1) else {
            return Err(Error::NotATable);
        };
        let table = self
            .table_of(a.0, first)
            .filter(|table| self.table_of(b.0, second) == Some(*table));
        let table = BlockId(table.ok_or(Error::NotATable)?, None);
        let (rows, columns) = (self.children(Some(table))?, self.columns(table)?);
        let span = |one: Option<usize>, other: Option<usize>| match (one, other) {
            (Some(one), Some(other)) => Ok(one.min(other)..=one.max(other)),
            _ => Err(Error::BlockNotFound),
        };
        let row = |cell: BlockId| rows.iter().position(|row| row.0 == cell.0);
        let column = |id| columns.iter().position(|other| *other == id);
        let (rows, columns) = (
            &rows[span(row(a), row(b))?],
            &columns[span(column(first), column(second))?],
        );
        let cells = rows
            .iter()
            .flat_map(|row| columns.iter().map(|column| BlockId(row.0, Some(*column))));
        Ok(cells.collect())
    }

    /// Whether `block` is a table, a row or a cell, which only the table
    /// methods shape.
    pub(crate) fn is_table_part(&self, block: BlockId) -> bool {
        let part = |block_type: String| block_type == TABLE || block_type == ROW;
        block.1.is_some() || self.block_type(block).is_ok_and(part)
    }
}
