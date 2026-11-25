//! Realtime collaboration for lotype documents: a WebSocket relay to mount
//! in an Axum server.
//!
//! A client connects to `/{document id}` with `Authorization: Bearer <token>`
//! and exchanges [`SyncMessage`](lotype_core::SyncMessage)s as binary
//! messages. Whose token it is and what it may do is the application's to
//! say, with an [`Authorizer`]; where documents are kept, with a [`DocStore`].

#![forbid(unsafe_code)]
#![warn(missing_docs)]

mod auth;
mod room;
mod router;

pub use auth::{Access, AuthError, Authorizer};
pub use lotype_storage::{DocId, DocStore, StoreError};
pub use router::router;
