use std::fmt;

use loro::{LoroError, LoroTreeError};

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug)]
#[non_exhaustive]
pub enum Error {
    UnknownBlockType(String),
    UnknownMark(String),
    BlockNotFound,
    /// The block's type holds no inline text.
    NoInlineContent,
    /// The block type named here cannot have child blocks.
    ChildrenNotAllowed(String),
    OutOfBounds {
        index: usize,
        len: usize,
    },
    /// A text offset falls inside a UTF-8 character.
    NotCharBoundary(usize),
    /// Attribute keys starting with `$` are reserved.
    ReservedAttr(String),
    CyclicMove,
    NoSelection,
    Internal(Box<dyn std::error::Error + Send + Sync>),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownBlockType(name) => write!(f, "unknown block type `{name}`"),
            Self::UnknownMark(name) => write!(f, "unknown mark `{name}`"),
            Self::BlockNotFound => f.write_str("block not found"),
            Self::NoInlineContent => f.write_str("block has no inline content"),
            Self::ChildrenNotAllowed(name) => {
                write!(f, "block type `{name}` cannot have children")
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
            Self::Internal(source) => write!(f, "internal error: {source}"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Internal(source) => Some(source.as_ref()),
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
