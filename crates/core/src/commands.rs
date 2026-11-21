use std::ops::Range;

use crate::{BlockId, Content, Document, Pos, Result, Value};

/// Edits that span more than one primitive operation.
impl Document {
    /// Splits a block at `pos`. The text after `pos` and the block's children
    /// move to a new sibling of the same type. Returns the start of that sibling.
    pub fn split_block(&mut self, pos: Pos) -> Result<Pos> {
        self.check_pos(pos)?;
        let block = pos.block;
        let len = self.text_len(block)?;
        let tail = self.spans_in(block, pos.offset..len)?;
        let block_type = self.block_type(block)?;
        let index = self.index_of(block)?;

        let new = self.insert_block(self.parent(block)?, index + 1, &block_type)?;
        for (key, value) in self.attrs(block)? {
            self.set_attr(new, &key, value)?;
        }
        self.insert_spans(new, 0, &tail)?;
        self.delete_text(block, pos.offset..len)?;
        // Children follow the tail, which keeps everything in the same visual order.
        for (index, child) in self.children(Some(block))?.into_iter().enumerate() {
            self.move_block(child, Some(new), index)?;
        }
        Ok(Pos::new(new, 0))
    }

    /// Joins `block` onto the end of the block before it, as Backspace at the
    /// start of a block does. Its children take its place.
    ///
    /// When the block before it holds no text, that block is removed instead.
    /// Returns the caret position, or `None` when there is nothing to join with.
    pub fn merge_backward(&mut self, block: BlockId) -> Result<Option<Pos>> {
        self.check_offset(block, 0)?;
        let Some(prev) = self.prev_block(block)? else {
            return Ok(None);
        };
        if self.spec(prev)?.content() != Content::Inline {
            if self.parent(block)? == Some(prev) {
                return Ok(None);
            }
            self.remove_block(prev)?;
            return Ok(Some(Pos::new(block, 0)));
        }
        let join = self.text_len(prev)?;
        let spans = self.spans(block)?;
        self.insert_spans(prev, join, &spans)?;
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
        let tail = self.spans_in(end.block, end.offset..self.text_len(end.block)?)?;
        let start_len = self.text_len(start.block)?;
        self.delete_text(start.block, start.offset..start_len)?;
        self.insert_spans(start.block, start.offset, &tail)?;
        // Deepest first: by the time a block is dissolved its removed
        // descendants are gone, so it only hands survivors to its parent.
        self.dissolve_block(end.block)?;
        for block in between.into_iter().rev() {
            self.dissolve_block(block)?;
        }
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

    /// Removes the mark when all of the range has it, and applies it otherwise.
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

    /// Removes `block`, leaving its children where it was.
    fn dissolve_block(&mut self, block: BlockId) -> Result<()> {
        let parent = self.parent(block)?;
        let index = self.index_of(block)?;
        for (offset, child) in self.children(Some(block))?.into_iter().enumerate() {
            self.move_block(child, parent, index + 1 + offset)?;
        }
        self.remove_block(block)
    }

    /// Blocks strictly between two blocks in document order.
    fn blocks_between(&self, first: BlockId, last: BlockId) -> Result<Vec<BlockId>> {
        let mut between = Vec::new();
        let mut current = self.next_block(first)?;
        while let Some(block) = current.filter(|block| *block != last) {
            between.push(block);
            current = self.next_block(block)?;
        }
        Ok(between)
    }

    /// The non-empty text ranges between two positions, one per block.
    fn segments(&self, a: Pos, b: Pos) -> Result<Vec<(BlockId, Range<usize>)>> {
        let (start, end) = self.ordered(a, b)?;
        if start.block == end.block {
            return Ok(non_empty(vec![(start.block, start.offset..end.offset)]));
        }
        let mut segments = vec![(start.block, start.offset..self.text_len(start.block)?)];
        for block in self.blocks_between(start.block, end.block)? {
            if self.spec(block)?.content() == Content::Inline {
                segments.push((block, 0..self.text_len(block)?));
            }
        }
        segments.push((end.block, 0..end.offset));
        Ok(non_empty(segments))
    }
}

fn non_empty(mut segments: Vec<(BlockId, Range<usize>)>) -> Vec<(BlockId, Range<usize>)> {
    segments.retain(|(_, range)| !range.is_empty());
    segments
}
