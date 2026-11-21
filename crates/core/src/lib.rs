//! Headless WYSIWYM document engine with realtime collaboration, built on Loro.

#![forbid(unsafe_code)]

mod commands;
mod document;
mod error;
mod order;
mod schema;
mod selection;
#[cfg(any(test, feature = "test-support"))]
pub mod test_support;
mod text;
mod value;

pub use document::{BlockId, Document};
pub use error::{Error, Result};
pub use schema::{BlockSpec, Content, Expand, MarkSpec, Schema};
pub use selection::{Pos, Selection};
pub use text::Span;
pub use value::Value;
