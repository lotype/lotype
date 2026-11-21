//! Headless WYSIWYM document engine with realtime collaboration, built on Loro.

#![forbid(unsafe_code)]

mod collab;
mod commands;
mod document;
mod draft;
mod editor;
mod error;
mod marks;
mod order;
mod schema;
mod selection;
#[cfg(any(test, feature = "test-support"))]
pub mod test_support;
#[cfg(test)]
mod tests;
mod text;
mod trash;
mod value;

pub use collab::Version;
pub use document::{BlockId, Document, MAX_DEPTH};
pub use editor::Editor;
pub use error::{Error, Result};
pub use schema::{BlockSpec, Content, Expand, MarkSpec, Schema};
pub use selection::{Pos, Selection};
pub use text::Span;
pub use value::Value;
