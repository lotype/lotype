use std::fmt;

use loro::{LoroError, LoroTreeError};

/// The result of an operation on a document or an editor.
pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug)]
#[non_exhaustive]
/// What went wrong in an operation on a document or an editor.
pub enum Error {
    /// No block type of the schema has the name here.
    UnknownBlockType(String),
    /// No mark of the schema has the name here.
    UnknownMark(String),
    /// No atom type of the schema has the name here.
    UnknownAtom(String),
    /// The mark requires a string or integer value.
    MarkValueRequired(String),
    /// The block is not in the document, or was removed.
    BlockNotFound,
    /// The text is not a block id as [`BlockId`](crate::BlockId) prints it.
    InvalidBlockId(String),
    /// The block's type holds no inline text.
    NoInlineContent,
    /// The schema names no block for text, as a paragraph, to make one of.
    NoTextBlock,
    /// The command is for blocks of the type named here, and the block is
    /// of another.
    NotOfType(&'static str),
    /// The block type named here cannot have child blocks.
    ChildrenNotAllowed(String),
    /// The block type named here goes only at the top level, in no block.
    TopLevelOnly(String),
    /// The block type named here holds no text, but the block has some.
    TextNotAllowed(String),
    /// The block would end up nested deeper than [`MAX_DEPTH`](crate::MAX_DEPTH).
    TooDeep,
    /// An index past the end of what it indexes.
    OutOfBounds {
        /// The index asked for.
        index: usize,
        /// How many there are.
        len: usize,
    },
    /// A range whose start is after its end.
    InvalidRange {
        /// Where the range starts.
        start: usize,
        /// Where it ends.
        end: usize,
    },
    /// Code line highlight ranges are not valid.
    InvalidLineRanges,
    /// A text offset falls inside a UTF-8 character.
    NotCharBoundary(usize),
    /// Attribute keys starting with `$` are reserved.
    ReservedAttr(String),
    /// A block cannot go inside itself.
    CyclicMove,
    /// The operation needs a selection of another kind, or one where there is none.
    NoSelection,
    /// The block is no table, or no row of one.
    NotATable,
    /// A table, its rows and its cells are only shaped through the table
    /// methods, and a cell is no block to move, remove or put blocks in.
    TablePart,
    /// Loro could not read or fully merge a snapshot or update.
    InvalidUpdate(Box<dyn std::error::Error + Send + Sync>),
    /// Importing into a document while its editor is running a local edit.
    ImportDuringEdit,
    /// Exporting an unpublished edit.
    ExportDuringEdit,
    /// Starting an editor edit on a document that is already a draft.
    NestedEdit,
    /// An error from Loro that is none of the others. A command on a document can stop
    /// halfway on it; an editor's edit is rolled back.
    Internal(Box<dyn std::error::Error + Send + Sync>),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownBlockType(name) => write!(f, "unknown block type `{name}`"),
            Self::UnknownMark(name) => write!(f, "unknown mark `{name}`"),
            Self::UnknownAtom(name) => write!(f, "unknown inline atom `{name}`"),
            Self::MarkValueRequired(name) => {
                write!(f, "mark `{name}` needs a string or integer value")
            }
            Self::BlockNotFound => f.write_str("block not found"),
            Self::InvalidBlockId(text) => write!(f, "`{text}` is not a block id"),
            Self::NoInlineContent => f.write_str("block has no inline content"),
            Self::NoTextBlock => f.write_str("the schema has no block for text"),
            Self::NotOfType(name) => write!(f, "the block is no `{name}`"),
            Self::ChildrenNotAllowed(name) => {
                write!(f, "block type `{name}` cannot have children")
            }
            Self::TopLevelOnly(name) => write!(f, "block type `{name}` cannot be nested"),
            Self::TextNotAllowed(name) => write!(f, "block type `{name}` cannot hold text"),
            Self::TooDeep => f.write_str("blocks cannot be nested this deep"),
            Self::InvalidRange { start, end } => {
                write!(f, "range start {start} is after its end {end}")
            }
            Self::InvalidLineRanges => f.write_str("invalid code highlight line ranges"),
            Self::OutOfBounds { index, len } => {
                write!(f, "index {index} is out of bounds (len {len})")
            }
            Self::NotCharBoundary(offset) => {
                write!(f, "offset {offset} is not on a character boundary")
            }
            Self::ReservedAttr(key) => write!(f, "attribute key `{key}` is reserved"),
            Self::CyclicMove => f.write_str("a block cannot be moved into itself"),
            Self::NoSelection => f.write_str("no selection"),
            Self::NotATable => f.write_str("the block is not a table"),
            Self::TablePart => f.write_str("not something a table, a row or a cell can do"),
            Self::InvalidUpdate(source) => write!(f, "invalid update: {source}"),
            Self::ImportDuringEdit => f.write_str("use Editor::import outside the edit"),
            Self::ExportDuringEdit => f.write_str("export the document after Editor::apply"),
            Self::NestedEdit => f.write_str("a draft cannot start another editor edit"),
            Self::Internal(source) => write!(f, "internal error: {source}"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::InvalidUpdate(source) | Self::Internal(source) => Some(source.as_ref()),
            _ => None,
        }
    }
}

impl From<LoroError> for Error {
    fn from(error: LoroError) -> Self {
        match error {
            LoroError::TreeError(LoroTreeError::CyclicMoveError) => Self::CyclicMove,
            other => Self::Internal(Box::new(other)),
        }
    }
}
