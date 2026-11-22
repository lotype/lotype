use std::sync::Arc;

use loro::LoroDoc;

use super::Editor;
use crate::draft::Draft;
use crate::{BlockId, Document, Error, Result, Selection};

impl Editor {
    /// Makes sure there is a trash node before a step that may remove
    /// blocks. Made inside the step, undo would delete it, with everything
    /// any peer has put in it by then.
    pub(super) fn ensure_trash(&mut self) -> Result<()> {
        if self.doc.top_trash().is_none() {
            self.finish_edit_group();
            self.doc.system(|doc| doc.trash_node().map(drop))?;
        }
        Ok(())
    }

    /// Gives the cells the selection is in their text before a step that
    /// may type there, for the same reason.
    pub(super) fn ensure_cells(&mut self) -> Result<()> {
        let Some(Selection::Text { anchor, head }) = &self.selection else {
            return Ok(());
        };
        let cells = [anchor.block, head.block];
        let unmade = |cell: &BlockId| {
            let text = cell.column().and_then(|_| self.doc.raw_text(*cell).ok());
            text.is_some_and(|text| !text.is_attached())
        };
        if cells.iter().any(unmade) {
            self.finish_edit_group();
            let made = |doc: &mut Document| {
                cells
                    .iter()
                    .try_for_each(|cell| doc.editable(*cell).map(drop))
            };
            self.doc.system(made)?;
        }
        Ok(())
    }

    pub(super) fn apply_draft(
        &mut self,
        edit: impl FnOnce(&mut Document, Option<&Selection>) -> Result<Option<Selection>>,
    ) -> Result<()> {
        if self.doc.draft.is_some() {
            return Err(Error::NestedEdit);
        }
        // The draft and the document must put removed blocks in the same place.
        self.ensure_trash()?;
        let token = Arc::new(());
        let fork = LoroDoc::new();
        fork.import(&self.doc.export_snapshot()?)?;
        let (local, scratch) = (self.doc.doc.peer_id(), fork.peer_id());
        let mut draft = Document::from_loro(fork, self.doc.schema().clone());
        draft.trash = self.doc.trash.clone();
        draft.draft = Some(Draft::new(Arc::clone(&token), local, scratch));

        let after = edit(&mut draft, self.selection.as_ref())?;
        draft.doc.commit();
        if let Some(after) = &after {
            draft.check_selection(after)?;
        }

        let Some(record) = draft.draft.take() else {
            self.finish_edit_group();
            self.doc = draft;
            (self.seen, self._watching) = super::changes::watch(&self.doc);
            self.replaced = true;
            self.forget_history();
            self.selection = after;
            self.live_anchors = None;
            return Ok(());
        };
        if !Arc::ptr_eq(&record.token, &token) {
            return Err(Error::NestedEdit);
        }
        // Replay as local mutations so Loro keeps its undo history. The
        // callback itself only runs once. New blocks come first, outside the
        // step: undoing it then puts them away, and they stay the same blocks.
        let (minted, cells, edits) = record.into_parts();
        self.finish_edit_group();
        self.doc.mint_all(&minted, &cells)?;
        let result = self.step(false, |doc, _| {
            for edit in edits {
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
