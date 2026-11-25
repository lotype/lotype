//! What peers say to each other to keep a document the same on all of them.

use crate::{Error, Result, Version};

/// A message between two peers of a document, or a peer and a relay, as
/// `lotype-server` is. How it travels is up to the application.
///
/// A peer that connects sends its [`Version`] and gets an `Update` of what
/// it lacks. It is sent the other side's `Version` too, unless it may only
/// read, and answers with an `Update` of what that side lacks: the changes
/// made while it was away. After that it sends an `Update` of each of its
/// own changes, [`Document::export_updates`](crate::Document::export_updates)
/// since the version it last sent from, and imports the ones it gets.
///
/// An `Update` is only bytes here. See [`Document::import`](crate::Document::import)
/// for what one from a peer that is not trusted can do.
#[derive(Debug, Clone, PartialEq)]
pub enum SyncMessage {
    /// How much of the document the sender has. The answer is an `Update`
    /// of what it lacks.
    Version(Version),
    /// A snapshot or updates, to import.
    Update(Vec<u8>),
    /// Who is in the document, for [`Presence::apply`](crate::Presence::apply).
    Presence(Vec<u8>),
}

impl SyncMessage {
    /// The message as bytes.
    pub fn encode(&self) -> Vec<u8> {
        let (tag, body) = match self {
            Self::Version(version) => (0, version.encode()),
            Self::Update(bytes) => (1, bytes.clone()),
            Self::Presence(bytes) => (2, bytes.clone()),
        };
        let mut message = Vec::with_capacity(body.len() + 1);
        message.push(tag);
        message.extend(body);
        message
    }

    /// A message from the bytes of [`SyncMessage::encode`].
    pub fn decode(bytes: &[u8]) -> Result<Self> {
        match bytes.split_first() {
            Some((0, body)) => Version::decode(body).map(Self::Version),
            Some((1, body)) => Ok(Self::Update(body.to_vec())),
            Some((2, body)) if body.len() <= crate::MAX_PRESENCE_BYTES => {
                Ok(Self::Presence(body.to_vec()))
            }
            _ => Err(Error::InvalidUpdate("not a sync message".into())),
        }
    }
}
