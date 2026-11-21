use std::collections::HashSet;
use std::sync::{Arc, Mutex};

use loro::event::Diff;
use loro::{TextDelta, UndoOrRedo};

use super::Editor;
use crate::Result;

impl Editor {
    /// Merges a snapshot or updates from another peer.
    ///
    /// The selection follows its text. It is cleared when its blocks are
    /// gone, and returns if a later import brings them back before the
    /// selection is set again. Remote changes are never part of this editor's
    /// undo history, but one that touches the text being typed ends the
    /// current typing step. One that changes the text of a block this
    /// editor's undo brought back clears the undo steps, and likewise for
    /// redo and the redo steps.
    /// An import that advances this editor's own peer counter clears its history.
    /// See [`Document::import`](crate::Document::import) for partial imports.
    pub fn import(&mut self, update: &[u8]) -> Result<()> {
        let mut anchored = self
            .live_anchors
            .take()
            .or_else(|| self.selection.as_ref().map(|s| self.doc.anchored(s)));
        let local_counter = self.local_counter();
        let changed_texts = Arc::new(Mutex::new(HashSet::new()));
        let collected = Arc::clone(&changed_texts);
        let subscription = self.doc.doc.subscribe_root(Arc::new(move |event| {
            if event.triggered_by.is_import() {
                let mut changed = collected.lock().unwrap();
                for diff in event.events {
                    if let Diff::Text(delta) = diff.diff
                        && delta
                            .iter()
                            .any(|op| !matches!(op, TextDelta::Retain { .. }))
                    {
                        changed.insert(diff.target.clone());
                    }
                }
            }
        }));
        let result = self.doc.import(update);
        drop(subscription);

        let changed_texts = std::mem::take(&mut *changed_texts.lock().unwrap());
        if let Some(anchored) = &mut anchored {
            anchored.record_import(&changed_texts);
        }
        let selection = anchored
            .as_ref()
            .and_then(|anchored| self.doc.resolve(anchored));
        // Loro tracks undo ranges by local op counter. If an import advances
        // this peer's counter, start a new history beyond the imported ops.
        if self.local_counter() != local_counter {
            self.forget_history();
        } else if !changed_texts.is_empty() {
            // Undo or redo brought the block back with new text, and Loro
            // runs the steps from before that at offsets these changes moved.
            let [undo, redo] = [UndoOrRedo::Undo, UndoOrRedo::Redo].map(|by| {
                let mut texts = changed_texts.iter();
                texts.any(|text| self.brought_back.get(text) == Some(&by))
            });
            if undo {
                self.finish_edit_group();
                self.history.clear_undo();
                self.undo_steps.clear();
            }
            if redo {
                self.history.clear_redo();
                self.redo_steps.clear();
            }
            self.brought_back.retain(|_, by| {
                !(undo && *by == UndoOrRedo::Undo || redo && *by == UndoOrRedo::Redo)
            });
            for step in self.undo_steps.iter_mut().chain(&mut self.redo_steps) {
                for anchored in step.before.iter_mut().chain(&mut step.after) {
                    anchored.record_import(&changed_texts);
                }
            }
        }
        // Loro ends the open undo group itself when the import touches what
        // the group changed. Starting a group only works when none is open.
        let group_ended = self.run.is_some() && self.history.group_start().is_ok();
        if selection != self.selection {
            self.pending_marks = None;
        }
        if group_ended || selection != self.selection {
            self.finish_edit_group();
        }
        // A block that came back has new text, which cursors made in the old
        // text do not follow.
        if let Some(selection) = &selection
            && anchored.as_ref().is_some_and(|a| self.doc.outdated(a))
        {
            anchored = Some(self.doc.anchored(selection));
        }
        // Kept while nothing resolves too: a later update can bring a block back.
        self.live_anchors = anchored;
        self.selection = selection;
        result
    }
}
