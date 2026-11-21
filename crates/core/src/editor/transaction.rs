use std::sync::Arc;

use loro::LoroDoc;

use super::Editor;
use crate::draft::Draft;
use crate::{Document, Error, Result, Selection};

impl Editor {
    pub(super) fn apply_draft(
        &mut self,
        edit: impl FnOnce(&mut Document, Option<&Selection>) -> Result<Option<Selection>>,
    ) -> Result<()> {
        if self.doc.draft.is_some() {
            return Err(Error::NestedEdit);
        }
        let token = Arc::new(());
        let fork = LoroDoc::new();
        fork.import(&self.doc.export_snapshot()?)?;
        fork.set_peer_id(self.doc.doc.peer_id())?;
        let mut draft = Document::from_loro(fork, self.doc.schema().clone());
        draft.draft = Some(Draft::new(Arc::clone(&token)));
        draft.refresh_ids();

        let after = edit(&mut draft, self.selection.as_ref())?;
        draft.doc.commit();
        if draft.draft.is_none() {
            draft.refresh_ids();
        }
        if let Some(after) = &after {
            draft.check_selection(after)?;
        }

        let Some(record) = draft.draft.take() else {
            self.finish_edit_group();
            self.doc = draft;
            self.forget_history();
            self.selection = after;
            return Ok(());
        };
        if !Arc::ptr_eq(&record.token, &token) {
            return Err(Error::NestedEdit);
        }
        // Replay as local mutations so Loro keeps its undo history and the
        // IDs allocated by the draft. The callback itself only runs once.
        let result = self.step(false, |doc, _| {
            for edit in record.into_edits() {
                edit(doc)?;
            }
            Ok(after)
        });
        self.history.group_end();
        if result.is_ok() {
            self.pending_marks = None;
        }
        result.map(drop)
    }
}
