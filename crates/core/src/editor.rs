use std::collections::{BTreeMap, VecDeque};
use std::sync::{Arc, Mutex};

use loro::undo::CursorWithPos;
use loro::{Counter, InnerUndoManager, LoroValue, Subscription, UndoOrRedo};

use crate::collab::Anchored;
use crate::marks::{TypingMarks, visible_marks};
use crate::trash::SYSTEM_ORIGIN;
use crate::{Document, Error, Pos, Result, Selection, Value};

mod blocks;
mod changes;
mod history;
mod input;
mod remote;
mod table;
mod transaction;
mod typing;

pub(crate) use blocks::BLOCK_STYLE;
pub use changes::Changes;

const MAX_UNDO_STEPS: usize = 100;

/// A document with a selection and local undo history.
///
/// Undo and redo only touch edits made through this editor, never those of
/// other peers.
pub struct Editor {
    doc: Document,
    selection: Option<Selection>,
    /// The endpoints from the last local text selection. An import may
    /// temporarily collapse a range while its endpoints remain distinct.
    live_anchors: Option<Anchored>,
    history: InnerUndoManager,
    undo_steps: VecDeque<Step>,
    redo_steps: Vec<Step>,
    /// The run of typing or deleting that the next keystroke may extend.
    /// While it is set, its undo step is still open.
    run: Option<typing::Run>,
    /// Marks the next typed text gets instead of those around the caret.
    pending_marks: Option<TypingMarks>,
    /// What changed in the document since a view last asked.
    seen: Arc<Mutex<changes::Seen>>,
    _watching: Subscription,
    replaced: bool,
    /// The copy of the document that drafts run on, while it can serve another.
    fork: Option<Document>,
    /// The version and selection an input rule left, while nothing has
    /// changed since: Backspace then takes the rule back.
    rule: Option<(crate::Version, Option<Selection>)>,
}

/// The selection around one undo step. Anchored, so that it is still in the
/// right place when changes from other peers came in before the step is undone.
struct Step {
    before: Option<Anchored>,
    after: Option<Anchored>,
}

impl Editor {
    pub fn new(doc: Document) -> Self {
        let (seen, _watching) = changes::watch(&doc);
        Self {
            seen,
            _watching,
            replaced: false,
            rule: None,
            fork: None,
            history: new_history(&doc),
            doc,
            selection: None,
            live_anchors: None,
            undo_steps: VecDeque::new(),
            redo_steps: Vec::new(),
            run: None,
            pending_marks: None,
        }
    }

    /// Starts over with the document as it is now, with no undo or redo steps.
    fn forget_history(&mut self) {
        self.history = new_history(&self.doc);
        self.undo_steps.clear();
        self.redo_steps.clear();
        self.run = None;
        self.pending_marks = None;
    }

    /// This peer's next operation counter.
    fn local_counter(&self) -> Counter {
        let versions = self.doc.doc.state_vv();
        versions.get(&self.doc.doc.peer_id()).copied().unwrap_or(0)
    }

    /// Loro can fail to undo a step that changes from other peers have cut
    /// across, and it keeps such a step. No step under it can be reached
    /// then, so the history starts over.
    fn history_failed(&mut self, error: loro::LoroError) -> Error {
        self.forget_history();
        self.selection = self
            .selection
            .take()
            .filter(|selection| self.doc.check_selection(selection).is_ok());
        self.live_anchors = None;
        error.into()
    }

    pub fn document(&self) -> &Document {
        &self.doc
    }

    pub fn selection(&self) -> Option<&Selection> {
        self.selection.as_ref()
    }

    /// Sets the selection and ends the current typing or deletion run.
    /// Pending marks are cleared when the selection changes.
    pub fn set_selection(&mut self, selection: Option<Selection>) -> Result<()> {
        if let Some(selection) = &selection {
            self.doc.check_selection(selection)?;
        }
        self.finish_edit_group();
        if self.selection != selection {
            self.pending_marks = None;
        }
        self.selection = selection;
        self.live_anchors = None;
        Ok(())
    }

    /// Runs `edit` as one undo step. `edit` returns the selection to set.
    ///
    /// The callback edits a draft. An error or panic discards the draft,
    /// including any replacement document, and preserves selection and history.
    /// A successful replacement starts a new history with that document.
    /// Import and export through the editor outside the callback.
    ///
    /// ```
    /// use lotype_core::{Document, Editor, Error, Schema};
    ///
    /// # fn main() -> lotype_core::Result<()> {
    /// let mut editor = Editor::new(Document::new(Schema::builtin()));
    /// let result = editor.apply(|doc, _| {
    ///     let block = doc.insert_block(None, 0, "paragraph")?;
    ///     doc.insert_text(block, 0, "draft")?;
    ///     doc.mark(block, 0..5, "unknown", true)?;
    ///     Ok(None)
    /// });
    ///
    /// assert!(matches!(result, Err(Error::UnknownMark(_))));
    /// assert!(editor.document().blocks().is_empty());
    /// assert!(!editor.can_undo());
    /// # Ok(())
    /// # }
    /// ```
    pub fn apply(
        &mut self,
        edit: impl FnOnce(&mut Document, Option<&Selection>) -> Result<Option<Selection>>,
    ) -> Result<()> {
        self.apply_draft(edit)
    }

    /// Runs `edit` and records it for undo: as a new step, or as part of the
    /// open run's step when `extend_run` is set. Returns whether the document
    /// changed. A new step is left open for the caller to close or keep.
    fn step(
        &mut self,
        extend_run: bool,
        edit: impl FnOnce(&mut Document, Option<&Selection>) -> Result<Option<Selection>>,
    ) -> Result<bool> {
        if self.doc.draft.is_some() {
            return Err(Error::NestedEdit);
        }
        if !extend_run {
            self.finish_edit_group();
        }
        let new_step = self.run.is_none();
        if new_step {
            self.ensure_trash()?;
            self.ensure_cells()?;
        }
        let version = self.doc.doc.oplog_frontiers();
        let selection = self.selection.clone();
        let before = if new_step {
            selection.as_ref().map(|s| self.doc.anchored(s))
        } else {
            None
        };
        if new_step {
            self.history.group_start()?;
        }
        let result = edit(&mut self.doc, selection.as_ref());
        self.doc.commit();

        let result = result.and_then(|after| {
            if let Some(after) = &after {
                self.doc.check_selection(after)?;
            }
            Ok(after)
        });
        let changed = self.doc.doc.oplog_frontiers() != version;
        let after = match result {
            Ok(after) => after,
            Err(error) => {
                // Loro cannot abort a change, so the step is undone instead.
                // A keystroke that extends a run is a single operation, which
                // cannot fail halfway.
                if changed && new_step {
                    self.history.group_end();
                    if let Err(error) = self.history.undo() {
                        self.selection = None;
                        return Err(self.history_failed(error));
                    }
                    // Loro dropped its redo steps when it recorded the edit,
                    // and now holds the undone edit as one. Emptying this
                    // list keeps `redo` away from it.
                    self.redo_steps.clear();
                }
                let doc = &self.doc;
                self.selection = self
                    .selection
                    .take()
                    .filter(|s| doc.check_selection(s).is_ok());
                self.live_anchors = None;
                return Err(error);
            }
        };
        if changed {
            let after = after.as_ref().map(|s| self.doc.anchored(s));
            if new_step {
                let mut before = before;
                if let Some(before) = &mut before {
                    self.doc.record_cut(before);
                }
                self.history
                    .set_top_undo_meta(history::meta(before.as_ref()));
                self.push_undo_step(Step { before, after });
                self.redo_steps.clear();
            } else if let Some(step) = self.undo_steps.back_mut() {
                // A later keystroke of the run can remove the character too.
                if let Some(before) = &mut step.before {
                    self.doc.record_cut(before);
                }
                step.after = after;
            }
        }
        self.selection = after;
        self.live_anchors = None;
        Ok(changed)
    }

    /// Deletes the selected text or blocks, or empties the selected cells.
    pub fn delete_selection(&mut self) -> Result<()> {
        self.apply(|doc, selection| match selection {
            Some(Selection::Text { anchor, head }) => {
                Ok(Some(Selection::caret(doc.delete_range(*anchor, *head)?)))
            }
            Some(Selection::Blocks(blocks)) => {
                let around: Vec<_> = blocks.iter().map(|block| doc.parent(*block)).collect();
                doc.remove_blocks(blocks.iter().copied())?;
                // An alert whose blocks all went goes with them.
                for parent in around {
                    doc.remove_if_emptied(parent?)?;
                }
                // A document left empty gets a paragraph to type in, where
                // the schema has one.
                if !doc.children(None)?.is_empty() {
                    return Ok(None);
                }
                let paragraph = doc.insert_block(None, 0, "paragraph").ok();
                Ok(paragraph.map(|block| Selection::caret(Pos::new(block, 0))))
            }
            Some(Selection::Cells { anchor, head }) => {
                for cell in doc.cells_between(*anchor, *head)? {
                    doc.delete_text(cell, 0..doc.text_len(cell)?)?;
                }
                Ok(selection.cloned())
            }
            None => Err(Error::NoSelection),
        })
    }

    /// Replaces the selected text with a block break, as Enter does. In a
    /// cell of a table, which cannot split, it goes to the cell below, or
    /// into a new paragraph after the table from its last row.
    pub fn split_block(&mut self) -> Result<()> {
        if let Some(Selection::Text { head, .. }) = self.selection
            && head.block.column().is_some()
        {
            return self.enter_in_cell(head.block);
        }
        if self.leave_list()? {
            return Ok(());
        }
        self.apply(|doc, selection| {
            let (anchor, head) = text_selection(selection)?;
            let caret = doc.delete_range(anchor, head)?;
            Ok(Some(Selection::caret(doc.split_block(caret)?)))
        })
    }

    /// Replaces the selected text with a line break in the block, as
    /// Shift-Enter does, as an undo step of its own.
    pub fn insert_line_break(&mut self) -> Result<()> {
        self.finish_edit_group();
        let typed = self.insert_text("\n");
        self.finish_edit_group();
        typed
    }

    /// Joins the block onto the one before it when the caret is at its start.
    /// Does nothing for any other selection.
    pub fn join_backward(&mut self) -> Result<()> {
        self.apply(|doc, selection| {
            let (anchor, head) = text_selection(selection)?;
            if anchor != head || anchor.offset != 0 {
                return Ok(selection.cloned());
            }
            let caret = doc.merge_backward(anchor.block)?.unwrap_or(anchor);
            Ok(Some(Selection::caret(caret)))
        })
    }

    /// Toggles an on/off mark on the selected text or for subsequent typing.
    /// Use [`Self::set_mark`] for marks that require a value.
    pub fn toggle_mark(&mut self, name: &str) -> Result<()> {
        let (anchor, head) = text_selection(self.selection.as_ref())?;
        self.doc.mark_spec(name)?.check_value(&Value::Bool(true))?;
        if anchor != head {
            return self.apply(|doc, selection| {
                doc.toggle_mark(anchor, head, name)?;
                Ok(selection.cloned())
            });
        }
        self.edit_pending_marks(anchor, |marks| {
            if marks.remove(name).is_none() {
                marks.insert(name.to_owned(), LoroValue::Bool(true));
            }
        })
    }

    /// Sets a mark on the selection or for subsequent typing at a caret.
    /// Pass `None` to remove it.
    pub fn set_mark(&mut self, name: &str, value: Option<Value>) -> Result<()> {
        let (anchor, head) = text_selection(self.selection.as_ref())?;
        let spec = self.doc.mark_spec(name)?;
        if let Some(value) = &value {
            spec.check_value(value)?;
        }
        if anchor != head {
            return self.apply(|doc, selection| {
                match value {
                    Some(value) => doc.mark_range(anchor, head, name, value)?,
                    None => doc.unmark_range(anchor, head, name)?,
                }
                Ok(selection.cloned())
            });
        }
        self.edit_pending_marks(anchor, |marks| match value {
            Some(value) => {
                marks.insert(name.to_owned(), value.to_loro());
            }
            None => {
                marks.remove(name);
            }
        })
    }

    fn edit_pending_marks(&mut self, pos: Pos, edit: impl FnOnce(&mut TypingMarks)) -> Result<()> {
        let mut marks = match self.pending_marks.take() {
            Some(marks) => marks,
            None => self.doc.typing_marks(pos)?,
        };
        self.finish_edit_group();
        edit(&mut marks);
        self.pending_marks = Some(marks);
        Ok(())
    }

    /// The marks in effect: those of the next typed text for a caret, and
    /// those all of the selected text has for a range.
    pub fn active_marks(&self) -> Result<BTreeMap<String, Value>> {
        match (&self.selection, &self.pending_marks) {
            (Some(Selection::Text { anchor, head }), _) if anchor != head => {
                self.doc.marks_in(*anchor, *head)
            }
            (Some(Selection::Text { .. }), Some(pending)) => Ok(visible_marks(pending.clone())),
            (Some(Selection::Text { anchor, .. }), None) => self.doc.marks_at(*anchor),
            _ => Ok(BTreeMap::new()),
        }
    }

    /// Whether there is a step to undo. A step that changed nothing in the
    /// end, such as text inserted and deleted again in one [`Self::apply`],
    /// counts here, though [`Self::undo`] skips it.
    pub fn can_undo(&self) -> bool {
        !self.undo_steps.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo_steps.is_empty()
    }

    /// Undoes the last step that changed the document. Returns whether there was one.
    ///
    /// Changes from other peers can leave a step that cannot be undone. That
    /// is an error, and it clears the undo and redo history.
    pub fn undo(&mut self) -> Result<bool> {
        self.finish_edit_group();
        self.pending_marks = None;
        while let Some(step) = self.undo_steps.back() {
            // A step with no net effect changes nothing. Go on to the one before it.
            let mut after = self.reanchor(step.after.as_ref());
            let (changed, cursors) = match history::change(&self.history, UndoOrRedo::Undo) {
                Ok(result) => result,
                Err(error) => return Err(self.history_failed(error)),
            };
            if let Some(after) = &mut after {
                self.doc.record_cut(after);
            }
            if let Some(step) = self.undo_steps.pop_back()
                && changed
            {
                self.restore(step.before.as_ref(), &cursors);
                self.history
                    .set_top_redo_meta(history::meta(after.as_ref()));
                let before = self.selection.as_ref().map(|s| self.doc.anchored(s));
                self.redo_steps.push(Step { before, after });
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// Redoes the last undone step. Returns whether there was one.
    pub fn redo(&mut self) -> Result<bool> {
        self.finish_edit_group();
        self.pending_marks = None;
        while let Some(step) = self.redo_steps.last() {
            let mut before = self.reanchor(step.before.as_ref());
            let (changed, cursors) = match history::change(&self.history, UndoOrRedo::Redo) {
                Ok(result) => result,
                Err(error) => return Err(self.history_failed(error)),
            };
            if let Some(before) = &mut before {
                self.doc.record_cut(before);
            }
            if let Some(step) = self.redo_steps.pop()
                && changed
            {
                self.restore(step.after.as_ref(), &cursors);
                self.history
                    .set_top_undo_meta(history::meta(before.as_ref()));
                let after = self.selection.as_ref().map(|s| self.doc.anchored(s));
                self.push_undo_step(Step { before, after });
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn push_undo_step(&mut self, step: Step) {
        self.undo_steps.push_back(step);
        if self.undo_steps.len() > MAX_UNDO_STEPS {
            self.undo_steps.pop_front();
        }
    }

    /// Sets the selection from a stored one, or clears it when it no longer fits the document.
    fn restore(&mut self, selection: Option<&Anchored>, cursors: &[CursorWithPos]) {
        self.selection = selection.and_then(|anchored| self.doc.resolve_history(anchored, cursors));
        self.live_anchors = None;
    }

    fn reanchor(&self, selection: Option<&Anchored>) -> Option<Anchored> {
        let selection = self.doc.resolve(selection?)?;
        Some(self.doc.anchored(&selection))
    }
}

fn new_history(doc: &Document) -> InnerUndoManager {
    let history = InnerUndoManager::new(doc.doc.inner());
    // One more than the editor offers: Loro makes room for a step as it
    // records it, and a failed step that is rolled back must not cost a real one.
    history.set_max_undo_steps(MAX_UNDO_STEPS + 1);
    history.set_merge_interval(0);
    history.add_exclude_origin_prefix(SYSTEM_ORIGIN);
    history
}

fn text_selection(selection: Option<&Selection>) -> Result<(Pos, Pos)> {
    match selection {
        Some(Selection::Text { anchor, head }) => Ok((*anchor, *head)),
        _ => Err(Error::NoSelection),
    }
}
