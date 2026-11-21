use std::collections::{HashMap, HashSet};

use loro::{ContainerID, ContainerTrait, TreeID, TreeParentId};

use crate::document::ID;
use crate::{BlockId, Document, MAX_DEPTH};

/// Blocks that came back under a new Loro node, by the id they were created
/// with. Loro never revives a removed node: undo and redo recreate it, and the
/// copy carries the first id in `$id`.
#[derive(Default)]
pub(crate) struct Restored {
    nodes: HashMap<TreeID, TreeID>,
    ids: HashMap<TreeID, TreeID>,
    /// Some block carries the id of another node, whether it got that id or not.
    pub(crate) claims: bool,
    pub(crate) hidden: bool,
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
    /// recreate blocks or change which block gets an id: undo, redo, an
    /// import, removing a claimant, or moving a subtree across the depth cap.
    ///
    /// `$id` is shared data a peer can set to anything, so a block only gets
    /// the id of a removed node, and only when no other block claims it too.
    /// A forged id can then cost a block the id it had, but never gives a
    /// block the id of another block in the document.
    pub(crate) fn refresh_ids(&mut self) {
        let mut restored = Restored::default();
        let mut claims: HashMap<TreeID, Option<TreeID>> = HashMap::new();
        for (node, depth) in self.raw_blocks() {
            if depth == MAX_DEPTH
                && self
                    .tree
                    .children_num(TreeParentId::Node(node))
                    .unwrap_or(0)
                    > 0
            {
                restored.hidden = true;
            }
            let Some(first) = self.first_id(node).filter(|first| *first != node) else {
                continue;
            };
            restored.claims = true;
            if self.is_removed(first) {
                let claim = claims.entry(first);
                claim.and_modify(|node| *node = None).or_insert(Some(node));
            }
        }
        for (first, node) in claims {
            if let Some(node) = node {
                restored.nodes.insert(first, node);
                restored.ids.insert(node, first);
            }
        }
        self.restored = restored;
    }

    /// Whether one of `texts` is in a block that this peer's undo or redo
    /// brought back.
    pub(crate) fn brought_back(&self, texts: &HashSet<ContainerID>) -> bool {
        let peer = self.doc.peer_id();
        let mut nodes = self.restored.ids.keys();
        nodes.any(|node| {
            let text = self.raw_text(self.id_of(*node));
            node.peer == peer && text.is_ok_and(|text| texts.contains(&text.id()))
        })
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
