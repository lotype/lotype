use std::collections::HashMap;

use loro::{Frontiers, TreeID, TreeParentId};

use crate::{BlockId, Document, Error, MAX_DEPTH, Result};

#[derive(Default)]
pub(crate) struct OrderCache {
    version: Frontiers,
    groups: HashMap<TreeParentId, Siblings>,
}

struct Siblings {
    ids: Vec<TreeID>,
    indices: HashMap<TreeID, usize>,
}

/// Document order: a block comes before its children, which come before its next sibling.
///
/// Each method checks the block it is given once. Ancestors and children of a
/// block in the document are in it too, so the walks themselves skip the check.
impl Document {
    /// Every block, in document order.
    pub fn blocks(&self) -> Vec<BlockId> {
        let mut ordered = Vec::new();
        // An explicit stack: nesting depth is controlled by remote peers.
        let mut stack = Vec::new();
        let roots = self.child_ids(TreeParentId::Root);
        stack.extend(roots.into_iter().rev().map(|id| (id, 1)));
        while let Some((id, depth)) = stack.pop() {
            ordered.push(BlockId(id));
            if depth < MAX_DEPTH {
                let children = self.child_ids(TreeParentId::Node(id));
                stack.extend(children.into_iter().rev().map(|child| (child, depth + 1)));
            }
        }
        ordered
    }

    pub fn next_block(&self, block: BlockId) -> Result<Option<BlockId>> {
        let mut current = block.0;
        if self.depth(block)? < MAX_DEPTH
            && let Some(first_child) = self.child_ids(TreeParentId::Node(current)).first()
        {
            return Ok(Some(BlockId(*first_child)));
        }
        loop {
            let (parent, next) =
                self.with_siblings(current, |siblings, index| siblings.get(index + 1).copied())?;
            if let Some(next) = next {
                return Ok(Some(BlockId(next)));
            }
            match parent {
                TreeParentId::Node(parent) => current = parent,
                _ => return Ok(None),
            }
        }
    }

    pub fn prev_block(&self, block: BlockId) -> Result<Option<BlockId>> {
        let mut depth = self.depth(block)?;
        let (parent, prev) = self.with_siblings(block.0, |siblings, index| {
            index.checked_sub(1).map(|index| siblings[index])
        })?;
        let Some(mut current) = prev else {
            return Ok(match parent {
                TreeParentId::Node(parent) => Some(BlockId(parent)),
                _ => None,
            });
        };
        while depth < MAX_DEPTH
            && let Some(last_child) = self.child_ids(TreeParentId::Node(current)).last()
        {
            current = *last_child;
            depth += 1;
        }
        Ok(Some(BlockId(current)))
    }

    /// The position of `block` among its siblings.
    pub fn index_of(&self, block: BlockId) -> Result<usize> {
        let (_, index) = self.with_siblings(self.live(block)?, |_, index| index)?;
        Ok(index)
    }

    /// Sibling indices from the top level down to `block`. Paths sort in document order.
    pub fn path(&self, block: BlockId) -> Result<Vec<usize>> {
        let mut path = Vec::new();
        let mut current = self.live(block)?;
        loop {
            let (parent, index) = self.with_siblings(current, |_, index| index)?;
            path.push(index);
            match parent {
                TreeParentId::Node(parent) => current = parent,
                _ => break,
            }
        }
        path.reverse();
        Ok(path)
    }

    fn with_siblings<T>(
        &self,
        id: TreeID,
        read: impl FnOnce(&[TreeID], usize) -> T,
    ) -> Result<(TreeParentId, T)> {
        let parent = self.tree.parent(id).ok_or(Error::BlockNotFound)?;
        // State frontiers include pending changes, undo and redo.
        let version = self.doc.state_frontiers();
        let mut order = self.order.lock().unwrap();
        if order.version != version {
            order.version = version;
            order.groups.clear();
        }
        let siblings = order.groups.entry(parent).or_insert_with(|| {
            let ids = self.child_ids(parent);
            let indices = ids.iter().copied().zip(0..).collect();
            Siblings { ids, indices }
        });
        let index = *siblings.indices.get(&id).ok_or(Error::BlockNotFound)?;
        Ok((parent, read(&siblings.ids, index)))
    }
}
