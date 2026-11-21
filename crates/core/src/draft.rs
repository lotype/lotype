use std::sync::{Arc, Mutex};

use crate::{Document, Result};

pub(crate) type Edit = Box<dyn FnOnce(&mut Document) -> Result<()> + Send + Sync>;

pub(crate) struct Draft {
    pub(crate) token: Arc<()>,
    edits: Mutex<Vec<Edit>>,
}

impl Draft {
    pub(crate) fn new(token: Arc<()>) -> Self {
        Self {
            token,
            edits: Mutex::default(),
        }
    }

    pub(crate) fn into_edits(self) -> Vec<Edit> {
        self.edits.into_inner().unwrap()
    }
}

impl Document {
    pub(crate) fn record(&self, edit: impl FnOnce() -> Edit) {
        if let Some(draft) = &self.draft {
            draft.edits.lock().unwrap().push(edit());
        }
    }
}
