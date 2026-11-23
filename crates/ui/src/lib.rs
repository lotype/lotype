//! GPUI editor for lotype documents.

#![forbid(unsafe_code)]

mod editor;
mod marks;
#[cfg(any(test, feature = "test-support"))]
pub mod test_support;

pub use editor::EditorView;
