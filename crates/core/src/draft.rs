use std::sync::{Arc, Mutex};

use loro::{PeerID, TreeID};

use crate::{Document, Result};

pub(crate) type Edit = Box<dyn FnOnce(&mut Document) -> Result<()> + Send + Sync>;

/// What an editor's edit did to its private copy of the document, to do the
/// same to the document once the edit has succeeded.
pub(crate) struct Draft {
    pub(crate) token: Arc<()>,
    /// The peer of the document this is a draft of. New blocks are made under it.
    pub(crate) local: PeerID,
    /// The peer every other change of the draft is made under.
    pub(crate) scratch: PeerID,
    minted: Mutex<Vec<(Option<TreeID>, String)>>,
    edits: Mutex<Vec<Edit>>,
}

impl Draft {
    pub(crate) fn new(token: Arc<()>, local: PeerID, scratch: PeerID) -> Self {
        Self {
            token,
            local,
            scratch,
            minted: Mutex::default(),
            edits: Mutex::default(),
        }
    }

    pub(crate) fn minted(&self, id: Option<TreeID>, block_type: &str) {
        let mut minted = self.minted.lock().unwrap();
        minted.push((id, block_type.to_owned()));
    }

    /// The blocks made, with their types, and the edits, both in order.
    pub(crate) fn into_parts(self) -> (Vec<(Option<TreeID>, String)>, Vec<Edit>) {
        let minted = self.minted.into_inner().unwrap();
        (minted, self.edits.into_inner().unwrap())
    }
}

impl Document {
    pub(crate) fn record(&self, edit: impl FnOnce() -> Edit) {
        if let Some(draft) = &self.draft {
            draft.edits.lock().unwrap().push(edit());
        }
    }
}
