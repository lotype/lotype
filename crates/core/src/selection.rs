use crate::{BlockId, Document, Result};

/// A place in a block's text. `offset` is in UTF-8 bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Pos {
    /// The block the position is in.
    pub block: BlockId,
    /// How far into the block's text, in bytes, on a character boundary.
    pub offset: usize,
}

impl Pos {
    /// The position `offset` bytes into `block`'s text.
    pub fn new(block: BlockId, offset: usize) -> Self {
        Self { block, offset }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// What is selected in a document.
pub enum Selection {
    /// `anchor` is where the selection started and `head` is where the cursor is.
    /// They may be in different blocks and in either order.
    Text {
        /// Where the selection started, which stays as it grows.
        anchor: Pos,
        /// Where the caret is, which keys and the mouse move.
        head: Pos,
    },
    /// Whole blocks, including their descendants.
    Blocks(Vec<BlockId>),
    /// The cells of one table from `anchor` to `head`: those in their rows
    /// and the rows between, and in their columns and the columns between.
    Cells {
        /// The cell the selection started in.
        anchor: BlockId,
        /// The cell it ends in, which keys and the mouse move.
        head: BlockId,
    },
}

impl Selection {
    /// A caret at `pos`: text selected from there to there.
    pub fn caret(pos: Pos) -> Self {
        Self::Text {
            anchor: pos,
            head: pos,
        }
    }

    /// Whether this is a caret with nothing selected.
    pub fn is_collapsed(&self) -> bool {
        match self {
            Self::Text { anchor, head } => anchor == head,
            Self::Blocks(_) | Self::Cells { .. } => false,
        }
    }
}

impl Document {
    /// Checks that `pos` is on a character boundary of an existing block with inline content.
    pub fn check_pos(&self, pos: Pos) -> Result<()> {
        self.check_offset(pos.block, pos.offset)
    }

    /// Checks that `selection` is one the document has: its positions in blocks with text, on
    /// character boundaries, its blocks there, its cells in one table.
    pub fn check_selection(&self, selection: &Selection) -> Result<()> {
        match selection {
            Selection::Text { anchor, head } => {
                self.check_pos(*anchor)?;
                self.check_pos(*head)
            }
            Selection::Blocks(blocks) => blocks.iter().try_for_each(|b| self.live(*b).map(drop)),
            Selection::Cells { anchor, head } => self.cells_between(*anchor, *head).map(drop),
        }
    }

    /// Returns the two positions in document order.
    pub fn ordered(&self, a: Pos, b: Pos) -> Result<(Pos, Pos)> {
        self.check_pos(a)?;
        self.check_pos(b)?;
        let a_first = if a.block == b.block {
            a.offset <= b.offset
        } else {
            self.path(a.block)? < self.path(b.block)?
        };
        Ok(if a_first { (a, b) } else { (b, a) })
    }
}
