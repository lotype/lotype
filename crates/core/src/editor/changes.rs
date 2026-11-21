use std::collections::HashSet;
use std::sync::{Arc, Mutex};

use loro::event::Diff;
use loro::{Index, Subscription, TreeExternalDiff, TreeID, TreeParentId};

use super::Editor;
use crate::trash::TRASH;
use crate::{BlockId, Document};

/// What changed in an editor's document, so a view can redraw only that.
///
/// It covers local edits, undo and redo, and imports alike.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Changes {
    /// The blocks in the document whose text, marks, type or attributes changed.
    pub blocks: HashSet<BlockId>,
    /// Blocks were inserted, removed or moved. A block that was not in the
    /// document before is not in `blocks`, whatever happened to it while it
    /// was away: read every such block anew.
    pub structure: bool,
    /// The editor has another document than before, so nothing read from the
    /// old one holds.
    pub replaced: bool,
}

impl Changes {
    pub fn is_empty(&self) -> bool {
        self.blocks.is_empty() && !self.structure && !self.replaced
    }
}

/// What Loro reported since the changes were last taken. Both sets are
/// bounded by the size of the document, however long nobody takes them.
#[derive(Default)]
pub(super) struct Seen {
    /// Nodes with a change in their metadata or in a container inside it.
    contents: HashSet<TreeID>,
    /// Where nodes came from and went to.
    parents: HashSet<TreeParentId>,
    /// A node became a trash node or stopped being one, with all that is in it.
    marked: bool,
}

pub(super) fn watch(doc: &Document) -> (Arc<Mutex<Seen>>, Subscription) {
    let seen = Arc::new(Mutex::new(Seen::default()));
    let noted = Arc::clone(&seen);
    let subscription = doc.doc.subscribe_root(Arc::new(move |event| {
        let mut seen = noted.lock().unwrap();
        for change in &event.events {
            if let Diff::Tree(tree) = &change.diff {
                for item in &tree.diff {
                    let (to, from) = match &item.action {
                        TreeExternalDiff::Create { parent, .. } => (Some(*parent), None),
                        TreeExternalDiff::Move {
                            parent, old_parent, ..
                        } => (Some(*parent), Some(*old_parent)),
                        TreeExternalDiff::Delete { old_parent, .. } => (None, Some(*old_parent)),
                    };
                    seen.parents.extend(to.into_iter().chain(from));
                }
                continue;
            }
            if let Diff::Map(map) = &change.diff {
                seen.marked |= map.updated.contains_key(TRASH);
            }
            // The node whose metadata the changed container is in.
            let node = change.path.iter().rev().find_map(|(_, index)| match index {
                Index::Node(node) => Some(*node),
                _ => None,
            });
            seen.contents.extend(node);
        }
    }));
    (seen, subscription)
}

impl Editor {
    /// What changed since this was last called, or since the editor was made.
    pub fn take_changes(&mut self) -> Changes {
        let seen = std::mem::take(&mut *self.seen.lock().unwrap());
        let blocks = seen.contents.into_iter().map(BlockId);
        Changes {
            blocks: blocks.filter(|block| self.doc.contains(*block)).collect(),
            // A move inside the trash has no end in the document.
            structure: seen.marked || seen.parents.iter().any(|parent| self.doc.shows(*parent)),
            replaced: std::mem::take(&mut self.replaced),
        }
    }
}
