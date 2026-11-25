//! AI for lotype documents: a model edits a private copy of a document, and
//! its edits are reviewed and accepted as one step.
//!
//! An [`AiSession`] is a chat about a document. Each time the user asks,
//! it gives the [`LlmRequest`] to send a model through the application's
//! [`LlmProvider`], and takes the answer as it comes. The model changes
//! the copy by calling tools, which the user sees happen, and nothing
//! reaches the document until [`AiSession::accept`].

#![forbid(unsafe_code)]
#![warn(missing_docs)]

mod apply;
mod context;
mod partial;
mod provider;
mod session;
mod store;
#[cfg(any(test, feature = "test-support"))]
pub mod test_support;
mod tools;

pub use context::DocumentState;
pub use provider::{
    LlmError, LlmEvent, LlmProvider, LlmRequest, LlmStream, Message, Role, ToolSpec,
};
pub use session::{Accepted, AiError, AiSession, AiState, Change};
pub use store::{AiSessionStore, AiTranscript, SessionId};
pub use tools::Op;
