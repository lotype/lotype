//! Native Rust SDK for block-based document editors with realtime collaboration.
//!
//! This umbrella crate re-exports the Lotype workspace crates behind Cargo
//! features so applications can depend on `lotype` with a single version pin:
//!
//! - [`core`] (always enabled): block tree, schema, commands, selection, local
//!   undo, sync updates, awareness, and Markdown (`lotype-core`).
//! - `ai`: AI session, provider trait, tools, private draft, and replay
//!   (`lotype-ai`).
//! - `code`: code block metadata, Twoslash directives, and built-in/LSP
//!   analysis (`lotype-code`). Combining `code` with `ui` also enables the
//!   GPUI code block view and Tree-sitter syntax highlighting.
//! - `code-all-languages`: enables every additional Tree-sitter grammar
//!   shipped by `gpui-component` (implies `code` and `ui`).
//! - `math`: LaTeX block and inline equations with native KaTeX typesetting
//!   (`lotype-math`). Combining `math` with `ui` also enables the GPUI vector
//!   equation views and inline equation popover.
//! - `server`: Axum WebSocket collaboration relay (`lotype-server`).
//! - `storage`: `DocId`, `DocStore`, and guarded snapshot/update merge
//!   (`lotype-storage`).
//! - `sqlite`: bundled SQLite `DocStore` implementation (implies `storage`).
//! - `ui`: GPUI editor and built-in block views (`lotype-ui`; macOS, Rust
//!   1.98+).
//! - `test-support`: enables test helpers on [`core`], and on `ai` and `ui`
//!   when those features are enabled.
//!
//! ```
//! use lotype::core::{Document, Editor, Schema, block};
//!
//! let mut doc = Document::new(Schema::builtin());
//! let id = doc.insert_block(None, 0, block::PARAGRAPH).unwrap();
//! doc.insert_text(id, 0, "Hello, Lotype!").unwrap();
//!
//! let editor = Editor::new(doc);
//! assert_eq!(editor.document().plain_text(id).unwrap(), "Hello, Lotype!");
//! ```

#![forbid(unsafe_code)]
#![warn(missing_docs)]

#[doc(inline)]
pub use lotype_core as core;

#[cfg(feature = "ai")]
#[doc(inline)]
pub use lotype_ai as ai;

#[cfg(feature = "code")]
#[doc(inline)]
pub use lotype_code as code;

#[cfg(feature = "math")]
#[doc(inline)]
pub use lotype_math as math;

#[cfg(feature = "server")]
#[doc(inline)]
pub use lotype_server as server;

#[cfg(feature = "storage")]
#[doc(inline)]
pub use lotype_storage as storage;

#[cfg(feature = "ui")]
#[doc(inline)]
pub use lotype_ui as ui;
