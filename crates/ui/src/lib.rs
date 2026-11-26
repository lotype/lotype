//! GPUI editor for lotype documents.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

mod ai_panel;
mod editor;
mod icons;
mod marks;
#[cfg(any(test, feature = "test-support"))]
pub mod test_support;
mod text_el;

pub use ai_panel::AiPanel;
pub use editor::{AlertLook, BlockContent, EditorView, MediaSource, Shade};
pub use text_el::{Interline, LineDecoration, TextDecoration};
