use std::collections::{BTreeMap, HashMap, VecDeque};
use std::sync::Arc;

use loro::{LoroValue, UndoManager};

use crate::marks::{TypingMarks, visible_marks};
use crate::{BlockId, Document, Error, Pos, Result, Selection, Value};

mod typing;

const MAX_UNDO_STEPS: usize = 100;

/// A document with a selection and local undo history.
///
/// Undo and redo only touch edits made through this editor, never those of
/// other peers.
pub struct Editor {
    doc: Document,
    selection: Option<Selection>,
    history: UndoManager,
    undo_steps: VecDeque<Step>,
    redo_steps: Vec<Step>,
    /// The run of typing or deleting that the next keystroke may extend.
    /// While it is set, its undo step is still open.
    run: Option<typing::Run>,
    /// Marks the next typed text gets instead of those around the caret.
    pending_marks: Option<TypingMarks>,
}

/// The selection around one undo step.
struct Step {
    before: Option<Anchor>,
    after: Option<Anchor>,
}

/// A selection stored by each block's position in document order. Undoing a
/// removal restores the block under a new id, so ids cannot be used here.
enum Anchor {
    Text {
        anchor: (usize, usize),
        head: (usize, usize),
    },
    Blocks(Vec<usize>),
}

/// Commits the edit and resets history if the edit replaced the document or panicked.
struct OpenStep<'a> {
    editor: &'a mut Editor,
    original_document: &'a Arc<()>,
}

impl Drop for OpenStep<'_> {
    fn drop(&mut self) {
        let editor = &mut *self.editor;
        editor.doc.commit();
        let replaced = !Arc::ptr_eq(self.original_document, &editor.doc.identity);
        if replaced || std::thread::panicking() {
            editor.history = new_history(&editor.doc);
            editor.undo_steps.clear();
            editor.redo_steps.clear();
            editor.selection = None;
            editor.run = None;
            editor.pending_marks = None;
        }
    }
}

impl Editor {
    pub fn new(doc: Document) -> Self {
        Self {
            history: new_history(&doc),
            doc,
            selection: None,
            undo_steps: VecDeque::new(),
            redo_steps: Vec::new(),
            run: None,
            pending_marks: None,
        }
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
        self.end_run();
        if self.selection != selection {
            self.pending_marks = None;
        }
        self.selection = selection;
        Ok(())
    }

    /// Runs `edit` as one undo step. `edit` returns the selection to set.
    ///
    /// If `edit` fails, the changes it made before failing stay in the
    /// document as an undo step. If it panics, they stay too, and the undo
    /// history and the selection are cleared.
    /// Replacing the document starts a new history with the replacement as its baseline.
    pub fn apply(
        &mut self,
        edit: impl FnOnce(&mut Document, Option<&Selection>) -> Result<Option<Selection>>,
    ) -> Result<()> {
        self.pending_marks = None;
        let result = self.step(false, edit);
        self.history.group_end();
        result.map(drop)
    }

    /// Runs `edit` and records it for undo: as a new step, or as part of the
    /// open run's step when `extend_run` is set. Returns whether the document
    /// changed. A new step is left open for the caller to close or keep.
    fn step(
        &mut self,
        extend_run: bool,
        edit: impl FnOnce(&mut Document, Option<&Selection>) -> Result<Option<Selection>>,
    ) -> Result<bool> {
        if !extend_run {
            self.end_run();
        }
        let new_step = self.run.is_none();
        let before = if new_step { self.anchor() } else { None };
        let original_document = Arc::clone(&self.doc.identity);
        let version = self.doc.doc.oplog_frontiers();
        let selection = self.selection.clone();
        if new_step {
            self.history.group_start()?;
        }
        let open = OpenStep {
            editor: self,
            original_document: &original_document,
        };
        let result = edit(&mut open.editor.doc, selection.as_ref());
        drop(open);

        let result = result.and_then(|selection| {
            if let Some(selection) = &selection {
                self.doc.check_selection(selection)?;
            }
            Ok(selection)
        });
        let doc = &self.doc;
        self.selection = match &result {
            Ok(selection) => selection.clone(),
            Err(_) => self
                .selection
                .take()
                .filter(|s| doc.check_selection(s).is_ok()),
        };
        let changed = Arc::ptr_eq(&original_document, &self.doc.identity)
            && self.doc.doc.oplog_frontiers() != version;
        if changed && new_step {
            let after = self.anchor();
            self.push_undo_step(Step { before, after });
            self.redo_steps.clear();
        } else if changed {
            let after = self.anchor();
            if let Some(step) = self.undo_steps.back_mut() {
                step.after = after;
            }
        }
        result.map(|_| changed)
    }

    /// Deletes the selected text or blocks.
    pub fn delete_selection(&mut self) -> Result<()> {
        self.apply(|doc, selection| match selection {
            Some(Selection::Text { anchor, head }) => {
                Ok(Some(Selection::caret(doc.delete_range(*anchor, *head)?)))
            }
            Some(Selection::Blocks(blocks)) => {
                // A block may already be gone as a descendant of an earlier one.
                for block in blocks {
                    if doc.contains(*block) {
                        doc.remove_block(*block)?;
                    }
                }
                Ok(None)
            }
            None => Err(Error::NoSelection),
        })
    }

    /// Replaces the selected text with a block break, as Enter does.
    pub fn split_block(&mut self) -> Result<()> {
        self.apply(|doc, selection| {
            let (anchor, head) = text_selection(selection)?;
            let caret = doc.delete_range(anchor, head)?;
            Ok(Some(Selection::caret(doc.split_block(caret)?)))
        })
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
        self.end_run();
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

    pub fn can_undo(&self) -> bool {
        self.history.can_undo()
    }

    pub fn can_redo(&self) -> bool {
        self.history.can_redo()
    }

    /// Undoes the last step. Returns whether there was one.
    pub fn undo(&mut self) -> Result<bool> {
        self.end_run();
        self.pending_marks = None;
        loop {
            let count = self.history.undo_count();
            let changed = self.history.undo()?;
            if self.history.undo_count() == count {
                return Ok(false);
            }
            let step = self.undo_steps.pop_back();
            if changed {
                self.restore(step.as_ref().and_then(|step| step.before.as_ref()));
                self.redo_steps.extend(step);
                return Ok(true);
            }
            // The step had no net effect, so Loro dropped it without changing
            // anything. Go on to the step before it.
        }
    }

    /// Redoes the last undone step. Returns whether there was one.
    pub fn redo(&mut self) -> Result<bool> {
        self.end_run();
        self.pending_marks = None;
        loop {
            let count = self.history.redo_count();
            let changed = self.history.redo()?;
            if self.history.redo_count() == count {
                return Ok(false);
            }
            let step = self.redo_steps.pop();
            if changed {
                self.restore(step.as_ref().and_then(|step| step.after.as_ref()));
                step.into_iter().for_each(|step| self.push_undo_step(step));
                return Ok(true);
            }
        }
    }

    fn push_undo_step(&mut self, step: Step) {
        self.undo_steps.push_back(step);
        if self.undo_steps.len() > MAX_UNDO_STEPS {
            self.undo_steps.pop_front();
        }
    }

    fn anchor(&self) -> Option<Anchor> {
        let selection = self.selection.as_ref()?;
        let order = self.doc.blocks();
        match selection {
            Selection::Text { anchor, head } => {
                let pos = |pos: &Pos| {
                    let index = order.iter().position(|block| *block == pos.block)?;
                    Some((index, pos.offset))
                };
                Some(Anchor::Text {
                    anchor: pos(anchor)?,
                    head: pos(head)?,
                })
            }
            Selection::Blocks(blocks) => {
                let order: HashMap<BlockId, usize> = order.into_iter().zip(0..).collect();
                let indices = blocks.iter().map(|block| order.get(block).copied());
                Some(Anchor::Blocks(indices.collect::<Option<_>>()?))
            }
        }
    }

    /// Sets the selection from a stored one, or clears it when it no longer fits the document.
    fn restore(&mut self, anchor: Option<&Anchor>) {
        let order = self.doc.blocks();
        let pos = |(index, offset): &(usize, usize)| Some(Pos::new(*order.get(*index)?, *offset));
        let selection = anchor.and_then(|anchor| match anchor {
            Anchor::Text { anchor, head } => Some(Selection::Text {
                anchor: pos(anchor)?,
                head: pos(head)?,
            }),
            Anchor::Blocks(indices) => {
                let blocks = indices.iter().map(|index| order.get(*index).copied());
                Some(Selection::Blocks(blocks.collect::<Option<_>>()?))
            }
        });
        self.selection = selection.filter(|s| self.doc.check_selection(s).is_ok());
    }
}

fn new_history(doc: &Document) -> UndoManager {
    let mut history = UndoManager::new(&doc.doc);
    history.set_max_undo_steps(MAX_UNDO_STEPS);
    history.set_merge_interval(0);
    history
}

fn text_selection(selection: Option<&Selection>) -> Result<(Pos, Pos)> {
    match selection {
        Some(Selection::Text { anchor, head }) => Ok((*anchor, *head)),
        _ => Err(Error::NoSelection),
    }
}
