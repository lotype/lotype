use loro::{TreeID, TreeParentId};

use crate::{BlockId, Document, Error, Result};

/// Document order: a block comes before its children, which come before its next sibling.
///
/// Each method checks liveness once, for the block it is given. Ancestors and
/// children of a live block are live, so the walks themselves skip the check.
impl Document {
    /// Every block, in document order.
    pub fn blocks(&self) -> Vec<BlockId> {
        let mut ordered = Vec::new();
        let mut stack = self.child_ids(TreeParentId::Root);
        stack.reverse();
        // An explicit stack: nesting depth is controlled by remote peers.
        while let Some(id) = stack.pop() {
            ordered.push(BlockId(id));
            stack.extend(self.child_ids(TreeParentId::Node(id)).into_iter().rev());
        }
        ordered
    }

    pub fn next_block(&self, block: BlockId) -> Result<Option<BlockId>> {
        let mut current = self.live(block)?;
        if let Some(first_child) = self.child_ids(TreeParentId::Node(current)).first() {
            return Ok(Some(BlockId(*first_child)));
        }
        loop {
            let (parent, siblings, index) = self.siblings(current)?;
            if let Some(next) = siblings.get(index + 1) {
                return Ok(Some(BlockId(*next)));
            }
            match parent {
                TreeParentId::Node(parent) => current = parent,
                _ => return Ok(None),
            }
        }
    }

    pub fn prev_block(&self, block: BlockId) -> Result<Option<BlockId>> {
        let (parent, siblings, index) = self.siblings(self.live(block)?)?;
        if index == 0 {
            return Ok(match parent {
                TreeParentId::Node(parent) => Some(BlockId(parent)),
                _ => None,
            });
        }
        let mut current = siblings[index - 1];
        while let Some(last_child) = self.child_ids(TreeParentId::Node(current)).last() {
            current = *last_child;
        }
        Ok(Some(BlockId(current)))
    }

    /// The position of `block` among its siblings.
    pub fn index_of(&self, block: BlockId) -> Result<usize> {
        Ok(self.siblings(self.live(block)?)?.2)
    }

    /// Sibling indices from the top level down to `block`. Paths sort in document order.
    pub fn path(&self, block: BlockId) -> Result<Vec<usize>> {
        let mut path = Vec::new();
        let mut current = self.live(block)?;
        loop {
            let (parent, _, index) = self.siblings(current)?;
            path.push(index);
            match parent {
                TreeParentId::Node(parent) => current = parent,
                _ => break,
            }
        }
        path.reverse();
        Ok(path)
    }

    /// The block at `path`, as returned by [`Document::path`].
    pub fn block_at(&self, path: &[usize]) -> Option<BlockId> {
        let mut parent = TreeParentId::Root;
        for index in path {
            parent = TreeParentId::Node(*self.child_ids(parent).get(*index)?);
        }
        match parent {
            TreeParentId::Node(id) => Some(BlockId(id)),
            _ => None,
        }
    }

    /// The parent of a live block, its children, and the block's index among them.
    fn siblings(&self, id: TreeID) -> Result<(TreeParentId, Vec<TreeID>, usize)> {
        let parent = self.tree.parent(id).ok_or(Error::BlockNotFound)?;
        let siblings = self.child_ids(parent);
        let index = siblings.iter().position(|sibling| *sibling == id);
        Ok((parent, siblings, index.ok_or(Error::BlockNotFound)?))
    }
}
