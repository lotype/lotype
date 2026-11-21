use std::collections::HashSet;
use std::ops::Range;

use crate::{BlockId, Document, Error, Pos, Result, Value};

/// Edits that span more than one primitive operation.
///
/// They copy text and attributes as stored, so marks and values this peer's
/// schema does not know survive, and they skip blocks they cannot read.
impl Document {
    /// Splits a block at `pos`. The text after `pos` and the block's children
    /// move to a new sibling of the same type. Returns the start of that sibling.
    pub fn split_block(&mut self, pos: Pos) -> Result<Pos> {
        self.check_pos(pos)?;
        let block = pos.block;
        let len = self.text_len(block)?;
        let block_type = self.block_type(block)?;
        let index = self.index_of(block)?;

        let new = self.create_block(self.parent(block)?, index + 1, &block_type)?;
        self.copy_attrs(block, new)?;
        self.copy_text(block, pos.offset..len, new, 0)?;
        self.delete_text(block, pos.offset..len)?;
        // Children follow the tail, which keeps everything in the same visual order.
        for (index, child) in self.children(Some(block))?.into_iter().enumerate() {
            self.relocate_block(child, Some(new), index)?;
        }
        Ok(Pos::new(new, 0))
    }

    /// Joins `block` onto the end of the block before it, as Backspace at the
    /// start of a block does. Its children take its place.
    ///
    /// When the block before it holds no text, that block is removed instead.
    /// Returns the caret position, or `None` when there is nothing to join with.
    pub fn merge_backward(&mut self, block: BlockId) -> Result<Option<Pos>> {
        let len = self.text_len(block)?;
        let Some(prev) = self.prev_block(block)? else {
            return Ok(None);
        };
        let Ok(join) = self.text_len(prev) else {
            if self.parent(block)? == Some(prev) {
                return Ok(None);
            }
            self.remove_block(prev)?;
            return Ok(Some(Pos::new(block, 0)));
        };
        self.copy_text(block, 0..len, prev, join)?;
        self.dissolve_block(block)?;
        Ok(Some(Pos::new(prev, join)))
    }

    /// Deletes everything between two positions and joins what is left of the
    /// last block onto the first. Returns the caret position.
    pub fn delete_range(&mut self, a: Pos, b: Pos) -> Result<Pos> {
        let (start, end) = self.ordered(a, b)?;
        if start.block == end.block {
            self.delete_text(start.block, start.offset..end.offset)?;
            return Ok(start);
        }
        let between = self.blocks_between(start.block, end.block)?;
        let start_len = self.text_len(start.block)?;
        let end_len = self.text_len(end.block)?;
        self.delete_text(start.block, start.offset..start_len)?;
        self.copy_text(end.block, end.offset..end_len, start.block, start.offset)?;
        self.remove_keeping_survivors(between, end.block)?;
        Ok(start)
    }

    /// Applies a mark to the text between two positions, across blocks.
    pub fn mark_range(
        &mut self,
        a: Pos,
        b: Pos,
        name: &str,
        value: impl Into<Value>,
    ) -> Result<()> {
        self.mark_spec(name)?;
        let value = value.into();
        for (block, range) in self.segments(a, b)? {
            self.mark(block, range, name, value.clone())?;
        }
        Ok(())
    }

    pub fn unmark_range(&mut self, a: Pos, b: Pos, name: &str) -> Result<()> {
        self.mark_spec(name)?;
        for (block, range) in self.segments(a, b)? {
            self.unmark(block, range, name)?;
        }
        Ok(())
    }

    /// Removes the mark when all of the range has it, with any value, and
    /// otherwise applies it with the value `true`.
    ///
    /// This suits on/off marks such as bold. For a mark that carries a value,
    /// such as a link, use [`Self::mark_range`] to apply it.
    pub fn toggle_mark(&mut self, a: Pos, b: Pos, name: &str) -> Result<()> {
        self.mark_spec(name)?;
        let mut all_marked = true;
        for (block, range) in self.segments(a, b)? {
            let spans = self.spans_in(block, range)?;
            all_marked &= spans.iter().all(|span| span.marks.contains_key(name));
        }
        if all_marked {
            self.unmark_range(a, b, name)
        } else {
            self.mark_range(a, b, name, true)
        }
    }

    /// Removes the blocks between the ends of a range, and the last block.
    ///
    /// Some of what they contain comes after the range and must stay: the
    /// last block's children, and the later children of each removed ancestor
    /// of the last block. Those move once, to where the outermost removed
    /// ancestor was, and then the removed blocks go with what is left in them.
    fn remove_keeping_survivors(&mut self, between: Vec<BlockId>, last: BlockId) -> Result<()> {
        let removed: HashSet<BlockId> = between.iter().copied().collect();
        let mut survivors = self.children(Some(last))?;
        let mut top = last;
        while let Some(parent) = self.parent(top)?.filter(|parent| removed.contains(parent)) {
            let siblings = self.children(Some(parent))?;
            let after = siblings.iter().position(|sibling| *sibling == top);
            survivors.extend(&siblings[after.map_or(siblings.len(), |index| index + 1)..]);
            top = parent;
        }
        let parent = self.parent(top)?;
        let index = self.index_of(top)?;
        for (offset, survivor) in survivors.into_iter().enumerate() {
            self.relocate_block(survivor, parent, index + 1 + offset)?;
        }
        // In document order, so a block inside one already removed is skipped.
        for block in between.into_iter().chain([last]) {
            if self.contains(block) {
                self.remove_block(block)?;
            }
        }
        Ok(())
    }

    /// Removes `block`, leaving its children where it was.
    fn dissolve_block(&mut self, block: BlockId) -> Result<()> {
        let children = self.children(Some(block))?;
        if !children.is_empty() {
            let parent = self.parent(block)?;
            let index = self.index_of(block)?;
            for (offset, child) in children.into_iter().enumerate() {
                self.relocate_block(child, parent, index + 1 + offset)?;
            }
        }
        self.remove_block(block)
    }

    /// Blocks strictly between two blocks, the first of which comes first in document order.
    fn blocks_between(&self, first: BlockId, last: BlockId) -> Result<Vec<BlockId>> {
        let mut blocks = self.blocks();
        let start = blocks.iter().position(|block| *block == first);
        let end = blocks.iter().position(|block| *block == last);
        match (start, end) {
            (Some(start), Some(end)) if start < end => {
                blocks.truncate(end);
                blocks.drain(..=start);
                Ok(blocks)
            }
            _ => Err(Error::BlockNotFound),
        }
    }

    /// The non-empty text ranges between two positions, one per block.
    pub(crate) fn segments(&self, a: Pos, b: Pos) -> Result<Vec<(BlockId, Range<usize>)>> {
        let (start, end) = self.ordered(a, b)?;
        let mut segments = Vec::new();
        if start.block == end.block {
            segments.push((start.block, start.offset..end.offset));
        } else {
            segments.push((start.block, start.offset..self.text_len(start.block)?));
            for block in self.blocks_between(start.block, end.block)? {
                // Blocks without readable text are skipped, not errors.
                if let Ok(len) = self.text_len(block) {
                    segments.push((block, 0..len));
                }
            }
            segments.push((end.block, 0..end.offset));
        }
        segments.retain(|(_, range)| !range.is_empty());
        Ok(segments)
    }
}
