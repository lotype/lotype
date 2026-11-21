use loro::{TreeID, TreeParentId};

use crate::{BlockId, Document, Error, MAX_DEPTH, Result};

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
        let mut depth = self.depth(block)?;
        let (parent, siblings, index) = self.siblings(block.0)?;
        if index == 0 {
            return Ok(match parent {
                TreeParentId::Node(parent) => Some(BlockId(parent)),
                _ => None,
            });
        }
        let mut current = siblings[index - 1];
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

    /// The parent of a block in the document, its children, and the block's index among them.
    fn siblings(&self, id: TreeID) -> Result<(TreeParentId, Vec<TreeID>, usize)> {
        let parent = self.tree.parent(id).ok_or(Error::BlockNotFound)?;
        let siblings = self.child_ids(parent);
        let index = siblings.iter().position(|sibling| *sibling == id);
        Ok((parent, siblings, index.ok_or(Error::BlockNotFound)?))
    }
}
