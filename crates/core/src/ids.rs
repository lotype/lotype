use std::collections::HashMap;

use loro::{TreeID, TreeParentId};

use crate::document::ID;
use crate::{BlockId, Document, MAX_DEPTH};

/// Blocks that came back under a new Loro node, by the id they were created
/// with. Loro never revives a removed node: undo and redo recreate it, and the
/// copy carries the first id in `$id`.
#[derive(Default)]
pub(crate) struct Restored {
    nodes: HashMap<TreeID, TreeID>,
    ids: HashMap<TreeID, TreeID>,
}

impl Restored {
    pub(crate) fn is_restored(&self, node: TreeID) -> bool {
        self.ids.contains_key(&node)
    }
}

impl Document {
    pub(crate) fn id_of(&self, node: TreeID) -> BlockId {
        BlockId(*self.restored.ids.get(&node).unwrap_or(&node))
    }

    pub(crate) fn node(&self, block: BlockId) -> TreeID {
        *self.restored.nodes.get(&block.0).unwrap_or(&block.0)
    }

    /// Finds the blocks that came back. Run it after anything that can
    /// recreate blocks: undo and redo.
    ///
    /// `$id` is shared data a peer can set to anything, so a block only gets
    /// the id of a removed node, and only when no other block claims it too.
    /// A forged id can then cost a block the id it had, but never gives a
    /// block the id of another block in the document.
    pub(crate) fn refresh_ids(&mut self) {
        self.restored = Restored::default();
        let mut claims: HashMap<TreeID, Option<TreeID>> = HashMap::new();
        for BlockId(node) in self.blocks() {
            if let Some(first) = self.first_id(node)
                && first != node
                && self.is_removed(first)
            {
                let claim = claims.entry(first);
                claim.and_modify(|node| *node = None).or_insert(Some(node));
            }
        }
        for (first, node) in claims {
            if let Some(node) = node {
                self.restored.nodes.insert(first, node);
                self.restored.ids.insert(node, first);
            }
        }
    }

    /// Whether a node that was in the document has been removed. Bounded like
    /// every ancestor walk: a peer controls how deep the removed nodes go.
    fn is_removed(&self, node: TreeID) -> bool {
        let mut current = node;
        for _ in 0..MAX_DEPTH {
            match self.tree.parent(current) {
                Some(TreeParentId::Node(parent)) => current = parent,
                Some(TreeParentId::Deleted) => return true,
                _ => break,
            }
        }
        false
    }

    fn first_id(&self, node: TreeID) -> Option<TreeID> {
        let value = self.tree.get_meta(node).ok()?.get(ID)?.into_value().ok()?;
        TreeID::try_from(value.as_string()?.as_str()).ok()
    }
}
