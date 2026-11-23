use std::sync::Arc;

use loro::{ExportMode, LoroDoc};

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
        let mut draft = self.fork()?;
        let (local, scratch) = (self.doc.doc.peer_id(), draft.doc.peer_id());
        let base = draft.doc.state_frontiers();
        draft.trash = self.doc.trash.clone();
        draft.draft = Some(Draft::new(Arc::clone(&token), local, scratch, base));

        let result = edit(&mut draft, self.selection.as_ref()).and_then(|after| {
            draft.doc.commit();
            if let Some(after) = &after {
                draft.check_selection(after)?;
            }
            Ok(after)
        });
        let after = match result {
            Ok(after) => after,
            Err(error) => {
                // A block it made has an id the document will give something else.
                let unused = draft
                    .draft
                    .take()
                    .filter(|record| Arc::ptr_eq(&record.token, &token) && record.minted_nothing());
                if unused.is_some() {
                    self.fork = Some(draft);
                }
                return Err(error);
            }
        };

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
        let reusable = record.tip.is_some();
        // Replay as local mutations so Loro keeps its undo history. The
        // callback itself only runs once. New blocks come first, outside the
        // step: undoing it then puts them away, and they stay the same blocks.
        let (minted, cells, edits) = record.into_parts();
        self.finish_edit_group();
        self.doc.mint_all(&minted, &cells)?;
        if reusable {
            self.fork = Some(draft);
        }
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

    /// A private copy of the document at its version, for a draft. It is kept
    /// between drafts and brought up to date, which costs what changed since
    /// rather than the whole document. Drafts it served stay in its history,
    /// off the version it is at.
    fn fork(&mut self) -> Result<Document> {
        // shortcut: the fork is made anew once old drafts double its history; tune with real sessions.
        let limit = 2 * self.doc.doc.len_ops();
        let kept = self.fork.take().filter(|fork| fork.doc.len_ops() <= limit);
        let fork = match kept {
            Some(fork) => fork,
            None => {
                let doc = LoroDoc::new();
                doc.import(&self.doc.export_snapshot()?)?;
                // Each checkout then gives it a new peer for the next draft.
                doc.set_detached_editing(true);
                Document::from_loro(doc, self.doc.schema().clone())
            }
        };
        let missing = self
            .doc
            .doc
            .export(ExportMode::updates(&fork.doc.oplog_vv()));
        fork.doc
            .import(&missing.map_err(|error| Error::Internal(Box::new(error)))?)?;
        fork.doc.checkout(&self.doc.doc.oplog_frontiers())?;
        Ok(fork)
    }
}

#[cfg(test)]
mod tests {
    use super::Editor;
    use crate::test_support::editor;
    use crate::{Document, Error, Schema, blocks, p};

    /// Brings the fork up to date and checks it is a copy of the document.
    /// Returns whether the fork was kept since the last draft.
    fn fork_matches(ed: &mut Editor) -> bool {
        let kept = ed.fork.is_some();
        let fork = ed.fork().unwrap();
        assert!(fork.doc.get_deep_value() == ed.doc.doc.get_deep_value());
        ed.fork = Some(fork);
        kept
    }

    #[test]
    fn the_fork_stays_a_copy_of_the_document_across_drafts() {
        let mut ed = editor(blocks![p!["hello <a>world"], p!["second"]]);
        let mut peer = Document::new(Schema::builtin());
        peer.import(&ed.doc.export_snapshot().unwrap()).unwrap();
        fork_matches(&mut ed);

        // Enter makes its block before anything else, so the fork serves on.
        ed.split_block().unwrap();
        assert!(fork_matches(&mut ed));
        ed.insert_text("typed ").unwrap();
        ed.toggle_mark("bold").unwrap();
        ed.split_block().unwrap();
        assert!(fork_matches(&mut ed));

        // A failed draft that made no block leaves the fork usable.
        let failed = ed.apply(|doc, _| {
            let block = doc.blocks()[0];
            doc.insert_text(block, 0, "lost")?;
            Err(Error::NoSelection)
        });
        assert!(failed.is_err());
        assert!(fork_matches(&mut ed));

        // One that made a block does not, nor one whose blocks came later.
        let failed = ed.apply(|doc, _| {
            doc.insert_block(None, 0, "paragraph")?;
            Err(Error::NoSelection)
        });
        assert!(failed.is_err());
        assert!(!fork_matches(&mut ed));
        ed.paste_markdown("one\n\ntwo\n\nthree").unwrap();
        assert!(!fork_matches(&mut ed));

        // Peers, undo and redo all reach the fork as changes of the document.
        let block = peer.blocks()[1];
        peer.insert_text(block, 0, "peer ").unwrap();
        ed.import(&peer.export_updates(&ed.doc.version()).unwrap())
            .unwrap();
        ed.join_backward().unwrap();
        assert!(fork_matches(&mut ed));
        ed.undo().unwrap();
        ed.undo().unwrap();
        ed.redo().unwrap();
        ed.split_block().unwrap();
        ed.indent().unwrap();
        assert!(fork_matches(&mut ed));
    }
}
