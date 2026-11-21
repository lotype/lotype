use std::collections::HashSet;
use std::sync::{Arc, Mutex};

use loro::TextDelta;
use loro::event::Diff;

use super::Editor;
use crate::Result;

impl Editor {
    /// Merges a snapshot or updates from another peer.
    ///
    /// The selection follows its text. It is cleared when its blocks are
    /// gone, and returns if a later import brings them back before the
    /// selection is set again. Remote changes are never part of this editor's
    /// undo history, but one that touches the text being typed ends the
    /// current typing step.
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
        // Kept while nothing resolves too: a later update can bring a block back.
        self.live_anchors = anchored;
        self.selection = selection;
        result
    }
}
