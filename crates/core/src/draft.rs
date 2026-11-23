use std::sync::{Arc, Mutex};

use loro::{Frontiers, PeerID, TreeID};

use crate::table::ColumnId;
use crate::{Document, Result};

pub(crate) type Minted = Vec<(Option<TreeID>, String)>;

pub(crate) type Edit = Box<dyn FnOnce(&mut Document) -> Result<()> + Send + Sync>;

/// What an editor's edit did to its private copy of the document, to do the
/// same to the document once the edit has succeeded.
pub(crate) struct Draft {
    pub(crate) token: Arc<()>,
    /// The peer of the document this is a draft of. New blocks are made under it.
    pub(crate) local: PeerID,
    /// The peer every other change of the draft is made under.
    pub(crate) scratch: PeerID,
    /// The draft's version while it has made nothing but blocks. The document
    /// makes those first, so they are the same operations there, and the
    /// fork stays a copy of the document. `None` once a block came later.
    pub(crate) tip: Option<Frontiers>,
    minted: Mutex<Vec<(Option<TreeID>, String)>>,
    /// The cells that got their text, each a row and a column.
    cells: Mutex<Vec<(TreeID, ColumnId)>>,
    edits: Mutex<Vec<Edit>>,
}

impl Draft {
    pub(crate) fn new(token: Arc<()>, local: PeerID, scratch: PeerID, base: Frontiers) -> Self {
        Self {
            token,
            local,
            scratch,
            tip: Some(base),
            minted: Mutex::default(),
            cells: Mutex::default(),
            edits: Mutex::default(),
        }
    }

    pub(crate) fn minted(&self, id: Option<TreeID>, block_type: &str) {
        let mut minted = self.minted.lock().unwrap();
        minted.push((id, block_type.to_owned()));
    }

    pub(crate) fn made_cell(&self, row: TreeID, column: ColumnId) {
        self.cells.lock().unwrap().push((row, column));
    }

    pub(crate) fn minted_nothing(&self) -> bool {
        self.minted.lock().unwrap().is_empty()
    }

    /// The blocks made, with their types, the cells that got their text,
    /// and the edits, each in order.
    pub(crate) fn into_parts(self) -> (Minted, Vec<(TreeID, ColumnId)>, Vec<Edit>) {
        let minted = self.minted.into_inner().unwrap();
        let cells = self.cells.into_inner().unwrap();
        (minted, cells, self.edits.into_inner().unwrap())
    }
}

impl Document {
    pub(crate) fn record(&self, edit: impl FnOnce() -> Edit) {
        if let Some(draft) = &self.draft {
            draft.edits.lock().unwrap().push(edit());
        }
    }
}
