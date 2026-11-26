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
    /// The start, or more, of an update too long for one message of the
    /// transport: more parts follow, and then the `Update` that is its end.
    /// [`SyncParts`] puts them together.
    Part(Vec<u8>),
}

impl SyncMessage {
    /// The message as bytes.
    pub fn encode(&self) -> Vec<u8> {
        let (tag, body) = match self {
            Self::Version(version) => (0, version.encode()),
            Self::Update(bytes) => (1, bytes.clone()),
            Self::Presence(bytes) => (2, bytes.clone()),
            Self::Part(bytes) => (3, bytes.clone()),
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
            Some((3, body)) => Ok(Self::Part(body.to_vec())),
            _ => Err(Error::InvalidUpdate("not a sync message".into())),
        }
    }

    /// An encoded message as a transport takes it that carries no more
    /// than `limit` bytes in one: an update longer than that is cut into
    /// parts, the last of them an `Update`. Any other message is as it was.
    pub fn parts(encoded: &[u8], limit: usize) -> Vec<Vec<u8>> {
        let update = encoded.split_first().filter(|(tag, _)| **tag == 1);
        let Some((_, body)) = update.filter(|_| encoded.len() > limit && limit > 1) else {
            return vec![encoded.to_vec()];
        };
        let count = body.len().div_ceil(limit - 1);
        let parts = body.chunks(limit - 1).enumerate();
        let tagged =
            parts.map(|(at, part)| [&[if at + 1 == count { 1 } else { 3 }], part].concat());
        tagged.collect()
    }
}

/// Puts an update that came in parts together again.
#[derive(Debug, Default)]
pub struct SyncParts(Vec<u8>);

impl SyncParts {
    /// Takes a message as it came. Gives it back, an `Update` with the
    /// parts before it, or `None` for a part, which is kept for its end.
    /// An update payload larger than `limit` bytes is an error. The limit
    /// counts all parts and the final update, and an error clears the parts.
    pub fn join(&mut self, message: SyncMessage, limit: usize) -> Result<Option<SyncMessage>> {
        match message {
            SyncMessage::Part(part) => {
                self.check_len(part.len(), limit)?;
                self.0.extend(part);
                Ok(None)
            }
            SyncMessage::Update(end) => {
                self.check_len(end.len(), limit)?;
                if self.0.is_empty() {
                    return Ok(Some(SyncMessage::Update(end)));
                }
                let mut update = std::mem::take(&mut self.0);
                update.extend(end);
                Ok(Some(SyncMessage::Update(update)))
            }
            whole => Ok(Some(whole)),
        }
    }

    /// Checks the payload length before growing the buffer.
    fn check_len(&mut self, incoming: usize, limit: usize) -> Result<()> {
        if self
            .0
            .len()
            .checked_add(incoming)
            .is_some_and(|len| len <= limit)
        {
            return Ok(());
        }
        self.0 = Vec::new();
        Err(Error::InvalidUpdate(
            "sync update exceeds the receive limit".into(),
        ))
    }
}
