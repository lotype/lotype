//! GPUI editor for lotype documents.

#![forbid(unsafe_code)]

mod editor;
mod icons;
mod marks;
#[cfg(any(test, feature = "test-support"))]
pub mod test_support;
mod text_el;

pub use editor::{BlockContent, EditorView};
