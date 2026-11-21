//! Headless WYSIWYM document engine with realtime collaboration, built on Loro.

#![forbid(unsafe_code)]

mod document;
mod error;
mod order;
mod schema;
mod value;

pub use document::{BlockId, Document};
pub use error::{Error, Result};
pub use schema::{BlockSpec, Content, Expand, MarkSpec, Schema};
pub use value::Value;
