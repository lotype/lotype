use std::sync::{Arc, Mutex};

use loro::undo::CursorWithPos;
use loro::{InnerUndoManager, LoroResult, UndoItemMeta, UndoOrRedo};

use crate::collab::Anchored;

/// What Loro keeps with a step it has just recorded. Set it right then: Loro
/// can only reach the newest step, and not even that one after an undo.
pub(super) fn meta(selection: Option<&Anchored>) -> UndoItemMeta {
    selection.map(Anchored::history_meta).unwrap_or_default()
}

/// Undoes or redoes one step. Returns whether the document changed, and the
/// step's cursors moved through the changes other peers made since.
pub(super) fn change(
    history: &InnerUndoManager,
    operation: UndoOrRedo,
) -> LoroResult<(bool, Vec<CursorWithPos>)> {
    let positions = Arc::new(Mutex::new(Vec::new()));
    let captured = Arc::clone(&positions);
    history.set_on_pop(Some(Box::new(move |_, _, meta| {
        *captured.lock().unwrap() = meta.cursors;
    })));
    let changed = match operation {
        UndoOrRedo::Undo => history.undo(),
        UndoOrRedo::Redo => history.redo(),
    };
    history.set_on_pop(None);
    let positions = std::mem::take(&mut *positions.lock().unwrap());
    changed.map(|changed| (changed, positions))
}
