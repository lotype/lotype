//! Where the application keeps a chat, which is no part of the document.

use std::future::Future;

use lotype_storage::{DocId, StoreError};

use crate::Message;

/// The name an application gives a chat about a document.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SessionId(pub String);

/// What was said in a chat: what the user asked and what the model
/// answered, without the documents sent with each request.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct AiTranscript {
    /// In the order said.
    pub messages: Vec<Message>,
}

/// Keeps chats for the application, apart from the documents: a chat is
/// one person's, and is not sent to the other peers.
pub trait AiSessionStore: Send + Sync {
    /// The chat, or `None` when there is none of that name.
    fn load(
        &self,
        doc: &DocId,
        session: &SessionId,
    ) -> impl Future<Output = Result<Option<AiTranscript>, StoreError>> + Send;

    /// Keeps the chat in place of what was kept of it.
    fn save(
        &self,
        doc: &DocId,
        session: &SessionId,
        transcript: &AiTranscript,
    ) -> impl Future<Output = Result<(), StoreError>> + Send;
}
