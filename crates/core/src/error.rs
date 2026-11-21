use std::fmt;

use loro::{LoroError, LoroTreeError};

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug)]
#[non_exhaustive]
pub enum Error {
    UnknownBlockType(String),
    UnknownMark(String),
    /// The mark requires a string or integer value.
    MarkValueRequired(String),
    BlockNotFound,
    /// The text is not a block id as [`BlockId`](crate::BlockId) prints it.
    InvalidBlockId(String),
    /// The block's type holds no inline text.
    NoInlineContent,
    /// The block type named here cannot have child blocks.
    ChildrenNotAllowed(String),
    /// The block type named here holds no text, but the block has some.
    TextNotAllowed(String),
    /// The block would end up nested deeper than [`MAX_DEPTH`](crate::MAX_DEPTH).
    TooDeep,
    OutOfBounds {
        index: usize,
        len: usize,
    },
    /// A range whose start is after its end.
    InvalidRange {
        start: usize,
        end: usize,
    },
    /// A text offset falls inside a UTF-8 character.
    NotCharBoundary(usize),
    /// Attribute keys starting with `$` are reserved.
    ReservedAttr(String),
    CyclicMove,
    NoSelection,
    /// Loro could not read or fully merge a snapshot or update.
    InvalidUpdate(Box<dyn std::error::Error + Send + Sync>),
    /// Importing into a document while its editor is running a local edit.
    ImportDuringEdit,
    Internal(Box<dyn std::error::Error + Send + Sync>),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownBlockType(name) => write!(f, "unknown block type `{name}`"),
            Self::UnknownMark(name) => write!(f, "unknown mark `{name}`"),
            Self::MarkValueRequired(name) => {
                write!(f, "mark `{name}` needs a string or integer value")
            }
            Self::BlockNotFound => f.write_str("block not found"),
            Self::InvalidBlockId(text) => write!(f, "`{text}` is not a block id"),
            Self::NoInlineContent => f.write_str("block has no inline content"),
            Self::ChildrenNotAllowed(name) => {
                write!(f, "block type `{name}` cannot have children")
            }
            Self::TextNotAllowed(name) => write!(f, "block type `{name}` cannot hold text"),
            Self::TooDeep => f.write_str("blocks cannot be nested this deep"),
            Self::InvalidRange { start, end } => {
                write!(f, "range start {start} is after its end {end}")
            }
            Self::OutOfBounds { index, len } => {
                write!(f, "index {index} is out of bounds (len {len})")
            }
            Self::NotCharBoundary(offset) => {
                write!(f, "offset {offset} is not on a character boundary")
            }
            Self::ReservedAttr(key) => write!(f, "attribute key `{key}` is reserved"),
            Self::CyclicMove => f.write_str("a block cannot be moved into itself"),
            Self::NoSelection => f.write_str("no selection"),
            Self::InvalidUpdate(source) => write!(f, "invalid update: {source}"),
            Self::ImportDuringEdit => f.write_str("use Editor::import outside the edit"),
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
