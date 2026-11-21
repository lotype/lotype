use std::collections::VecDeque;

use loro::UndoManager;

use crate::{BlockId, Document, Error, Pos, Result, Selection};

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
}

/// The selection around one undo step.
struct Step {
    before: Option<Anchor>,
    after: Option<Anchor>,
}

/// A selection stored as block paths. Undoing a removal restores the block
/// under a new id, so ids cannot be used here.
enum Anchor {
    Text {
        anchor: (Vec<usize>, usize),
        head: (Vec<usize>, usize),
    },
    Blocks(Vec<Vec<usize>>),
}

impl Editor {
    pub fn new(doc: Document) -> Self {
        let mut history = UndoManager::new(&doc.doc);
        history.set_max_undo_steps(MAX_UNDO_STEPS);
        history.set_merge_interval(0);
        Self {
            doc,
            selection: None,
            history,
            undo_steps: VecDeque::new(),
            redo_steps: Vec::new(),
        }
    }

    pub fn document(&self) -> &Document {
        &self.doc
    }

    pub fn selection(&self) -> Option<&Selection> {
        self.selection.as_ref()
    }

    pub fn set_selection(&mut self, selection: Option<Selection>) -> Result<()> {
        if let Some(selection) = &selection {
            self.doc.check_selection(selection)?;
        }
        self.selection = selection;
        Ok(())
    }

    /// Runs `edit` as one undo step. `edit` returns the selection to set.
    ///
    /// If `edit` fails, the changes it made before failing stay in the
    /// document as an undo step.
    pub fn apply(
        &mut self,
        edit: impl FnOnce(&mut Document, Option<&Selection>) -> Result<Option<Selection>>,
    ) -> Result<()> {
        let before = self.anchor();
        let version = self.doc.doc.oplog_frontiers();
        self.history.group_start()?;
        let result = edit(&mut self.doc, self.selection.as_ref());
        self.doc.commit();
        self.history.group_end();

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
        if self.doc.doc.oplog_frontiers() != version {
            let after = self.anchor();
            self.undo_steps.push_back(Step { before, after });
            if self.undo_steps.len() > MAX_UNDO_STEPS {
                self.undo_steps.pop_front();
            }
            self.redo_steps.clear();
        }
        result.map(drop)
    }

    /// Replaces the selected text with `text`.
    pub fn insert_text(&mut self, text: &str) -> Result<()> {
        self.apply(|doc, selection| {
            let (anchor, head) = text_selection(selection)?;
            let caret = doc.delete_range(anchor, head)?;
            doc.insert_text(caret.block, caret.offset, text)?;
            let caret = Pos::new(caret.block, caret.offset + text.len());
            Ok(Some(Selection::caret(caret)))
        })
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

    /// Toggles a mark on the selected text. Does nothing for a caret.
    pub fn toggle_mark(&mut self, name: &str) -> Result<()> {
        self.apply(|doc, selection| {
            let (anchor, head) = text_selection(selection)?;
            doc.toggle_mark(anchor, head, name)?;
            Ok(selection.cloned())
        })
    }

    pub fn can_undo(&self) -> bool {
        self.history.can_undo()
    }

    pub fn can_redo(&self) -> bool {
        self.history.can_redo()
    }

    /// Undoes the last step. Returns whether there was one.
    pub fn undo(&mut self) -> Result<bool> {
        if !self.history.undo()? {
            return Ok(false);
        }
        let step = self.undo_steps.pop_back();
        self.restore(step.as_ref().and_then(|step| step.before.as_ref()));
        self.redo_steps.extend(step);
        Ok(true)
    }

    /// Redoes the last undone step. Returns whether there was one.
    pub fn redo(&mut self) -> Result<bool> {
        if !self.history.redo()? {
            return Ok(false);
        }
        let step = self.redo_steps.pop();
        self.restore(step.as_ref().and_then(|step| step.after.as_ref()));
        self.undo_steps.extend(step);
        Ok(true)
    }

    fn anchor(&self) -> Option<Anchor> {
        let pos = |pos: &Pos| Some((self.doc.path(pos.block).ok()?, pos.offset));
        match self.selection.as_ref()? {
            Selection::Text { anchor, head } => Some(Anchor::Text {
                anchor: pos(anchor)?,
                head: pos(head)?,
            }),
            Selection::Blocks(blocks) => {
                let paths = blocks.iter().map(|block| self.doc.path(*block).ok());
                Some(Anchor::Blocks(paths.collect::<Option<_>>()?))
            }
        }
    }

    /// Sets the selection from a stored one, or clears it when it no longer fits the document.
    fn restore(&mut self, anchor: Option<&Anchor>) {
        let pos = |(path, offset): &(Vec<usize>, usize)| {
            Some(Pos::new(self.doc.block_at(path)?, *offset))
        };
        let selection = anchor.and_then(|anchor| match anchor {
            Anchor::Text { anchor, head } => Some(Selection::Text {
                anchor: pos(anchor)?,
                head: pos(head)?,
            }),
            Anchor::Blocks(paths) => {
                let blocks = paths.iter().map(|path| self.doc.block_at(path));
                Some(Selection::Blocks(blocks.collect::<Option<Vec<BlockId>>>()?))
            }
        });
        self.selection = selection.filter(|s| self.doc.check_selection(s).is_ok());
    }
}

fn text_selection(selection: Option<&Selection>) -> Result<(Pos, Pos)> {
    match selection {
        Some(Selection::Text { anchor, head }) => Ok((*anchor, *head)),
        _ => Err(Error::NoSelection),
    }
}
