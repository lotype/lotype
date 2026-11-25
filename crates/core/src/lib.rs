//! Headless WYSIWYM document engine with realtime collaboration, built on Loro.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

mod atom;
mod attrs;
mod block_kind;
mod clipboard;
mod collab;
mod commands;
mod document;
mod draft;
mod editor;
mod error;
mod keymap;
mod markdown;
mod marks;
mod order;
mod rules;
mod schema;
mod selection;
mod suggestion;
mod table;
#[cfg(any(test, feature = "test-support"))]
pub mod test_support;
#[cfg(test)]
mod tests;
mod text;
mod trash;
mod value;

pub use atom::{Atom, OBJECT};
pub use attrs::{Align, BlockColor, COLOR_NAMES, alert, attr, block, mark};
pub use block_kind::BlockKind;
pub use collab::Version;
pub use document::{BlockId, Document, MAX_DEPTH};
pub use editor::{Changes, Editor};
pub use error::{Error, Result};
pub use keymap::{Command, KeyBinding, default_keymap};
pub use schema::{
    BlockSpec, Content, Expand, InlineSpec, MAX_HEADING_LEVEL, MarkSpec, Role, Schema,
};
pub use selection::{Pos, Selection};
pub use suggestion::{
    SlashItem, SlashQuery, default_block_types, default_slash_items, filter_slash_items,
};
pub use table::ColumnId;
pub use text::Span;
pub use value::Value;
