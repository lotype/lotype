//! Who else is in a document, and what they have selected.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use loro::ContainerTrait;
use loro::cursor::{Cursor, PosType, Side};

use crate::{BlockId, Document, Error, Pos, Result, Selection};

/// How long a peer stays in a [`Presence`] after it was last heard of. A
/// peer that is still there publishes itself again before that.
pub const PRESENCE_TIMEOUT: Duration = Duration::from_secs(30);

/// The most bytes a peer's presence takes. A relay passes on no more.
pub const MAX_PRESENCE_BYTES: usize = 4096;

/// A name is published up to this many characters.
const NAME_CHARS: usize = 64;

/// Another peer in the document, as it published itself. All of it is the
/// peer's word, and no more to be trusted than the peer.
#[derive(Debug, Clone, PartialEq)]
pub struct Collaborator {
    /// The peer the presence came under: its id in the document between
    /// two peers, and the number a relay gave its connection through one.
    pub peer: u64,
    /// The name to show.
    pub name: String,
    /// The color to show it in, as `0xRRGGBB`.
    pub color: u32,
    /// What the peer has selected, where that is in this document now.
    pub selection: Option<Selection>,
}

/// The other peers in a document and their selections, which are no part
/// of the document or its history. Clones share one state.
///
/// A presence travels as the peer it is of, in eight bytes, and then what
/// the peer says of itself, or nothing when it leaves.
#[derive(Debug, Clone, Default)]
pub struct Presence(Arc<Mutex<HashMap<u64, (State, Instant)>>>);

#[derive(Debug)]
struct State {
    name: String,
    color: u32,
    selection: Option<Published>,
}

/// A selection as other peers can find it in their documents.
#[derive(Debug)]
enum Published {
    Text(End, End),
    Blocks(Vec<BlockId>),
    Cells(BlockId, BlockId),
}

/// An end of selected text, which holds on to its character.
#[derive(Debug)]
struct End {
    block: BlockId,
    /// Where it was when published, for text that takes no cursor.
    offset: u32,
    cursor: Option<Cursor>,
}

impl Presence {
    /// With no peers.
    pub fn new() -> Self {
        Self::default()
    }

    /// This peer of `doc` with its selection there, as bytes to send to the
    /// others. Send them when the selection changes, and again within
    /// [`PRESENCE_TIMEOUT`] while it does not. A selection too large for
    /// [`MAX_PRESENCE_BYTES`] is left out.
    pub fn publish(
        doc: &Document,
        name: &str,
        color: u32,
        selection: Option<&Selection>,
    ) -> Vec<u8> {
        let mut state = doc.doc.peer_id().to_le_bytes().to_vec();
        state.extend(color.to_le_bytes());
        let name: String = name.chars().take(NAME_CHARS).collect();
        put(&mut state, name.as_bytes());
        let none = [state.as_slice(), &[0]].concat();
        let published = selection.and_then(|selection| {
            write_selection(&mut state, doc, selection)?;
            Some(state).filter(|state| state.len() <= MAX_PRESENCE_BYTES)
        });
        published.unwrap_or(none)
    }

    /// This peer of `doc` leaving, as bytes to send to the others.
    pub fn leave(doc: &Document) -> Vec<u8> {
        doc.doc.peer_id().to_le_bytes().to_vec()
    }

    /// Makes a presence that of `peer`, whoever it says it is of, if it is
    /// one. A relay numbers its clients and does this to what they send, so
    /// that none speaks for another.
    pub fn readdress(presence: &mut [u8], peer: u64) -> Result<()> {
        let (_, state) = read(presence)?;
        drop(state);
        let id = presence.first_chunk_mut().ok_or_else(invalid)?;
        *id = peer.to_le_bytes();
        Ok(())
    }

    /// Takes what a peer sent of itself, and forgets the peers not heard of
    /// in time.
    pub fn apply(&self, presence: &[u8]) -> Result<()> {
        let (peer, state) = read(presence)?;
        let now = Instant::now();
        let mut peers = self.peers();
        peers.retain(|_, (_, heard)| now.duration_since(*heard) < PRESENCE_TIMEOUT);
        match state {
            Some(state) => peers.insert(peer, (state, now)),
            None => peers.remove(&peer),
        };
        Ok(())
    }

    /// The peers heard of in time, in the order of their ids, with their
    /// selections as they are in `doc`. A selection in text that `doc` does
    /// not have yet is `None` until it has.
    pub fn collaborators(&self, doc: &Document) -> Vec<Collaborator> {
        let now = Instant::now();
        let peers = self.peers();
        let mut all: Vec<_> = peers
            .iter()
            .filter(|(_, (_, heard))| now.duration_since(*heard) < PRESENCE_TIMEOUT)
            .map(|(peer, (state, _))| Collaborator {
                peer: *peer,
                name: state.name.clone(),
                color: state.color,
                selection: state.selection.as_ref().and_then(|s| doc.shown(s)),
            })
            .collect();
        all.sort_by_key(|collaborator| collaborator.peer);
        all
    }

    fn peers(&self) -> std::sync::MutexGuard<'_, HashMap<u64, (State, Instant)>> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// The peer a presence is of, and what it says of itself unless it leaves.
fn read(presence: &[u8]) -> Result<(u64, Option<State>)> {
    let mut read = Reader(presence);
    let fits = presence.len() <= MAX_PRESENCE_BYTES;
    let peer = read.array().filter(|_| fits).ok_or_else(invalid)?;
    let state = match read.0 {
        [] => None,
        _ => Some(read.state().ok_or_else(invalid)?),
    };
    Ok((u64::from_le_bytes(peer), state))
}

fn invalid() -> Error {
    Error::InvalidUpdate("not a presence".into())
}

/// Bytes with their length before them. `None` for more than a length says.
fn put(out: &mut Vec<u8>, bytes: &[u8]) -> Option<()> {
    out.extend(u16::try_from(bytes.len()).ok()?.to_le_bytes());
    out.extend(bytes);
    Some(())
}

fn write_selection(out: &mut Vec<u8>, doc: &Document, selection: &Selection) -> Option<()> {
    let block = |out: &mut Vec<u8>, block: &BlockId| put(out, block.to_string().as_bytes());
    let end = |out: &mut Vec<u8>, pos: &Pos| {
        let text = doc.inline(pos.block).ok()?;
        // Not `convert_pos`, which rounds an offset inside a character.
        let at = crate::text::unicode_offset(&text, pos.offset).ok()?;
        let cursor = text.get_cursor(at, Side::Middle);
        block(out, &pos.block)?;
        out.extend(u32::try_from(pos.offset).ok()?.to_le_bytes());
        put(
            out,
            &cursor.map(|cursor| cursor.encode()).unwrap_or_default(),
        )
    };
    match selection {
        Selection::Text { anchor, head } => {
            out.push(1);
            end(out, anchor)?;
            end(out, head)
        }
        Selection::Blocks(blocks) => {
            out.push(2);
            out.extend(u16::try_from(blocks.len()).ok()?.to_le_bytes());
            blocks.iter().try_for_each(|one| block(out, one))
        }
        Selection::Cells { anchor, head } => {
            out.push(3);
            block(out, anchor)?;
            block(out, head)
        }
    }
}

/// What a peer sent, which may be anything.
struct Reader<'a>(&'a [u8]);

impl<'a> Reader<'a> {
    fn array<const N: usize>(&mut self) -> Option<[u8; N]> {
        let (array, rest) = self.0.split_first_chunk()?;
        self.0 = rest;
        Some(*array)
    }

    /// Bytes with their length before them.
    fn chunk(&mut self) -> Option<&'a [u8]> {
        let len = u16::from_le_bytes(self.array()?);
        let (chunk, rest) = self.0.split_at_checked(len.into())?;
        self.0 = rest;
        Some(chunk)
    }

    fn block(&mut self) -> Option<BlockId> {
        std::str::from_utf8(self.chunk()?).ok()?.parse().ok()
    }

    fn end(&mut self) -> Option<End> {
        let (block, offset) = (self.block()?, u32::from_le_bytes(self.array()?));
        let cursor = match self.chunk()? {
            [] => None,
            cursor => Some(Cursor::decode(cursor).ok()?),
        };
        Some(End {
            block,
            offset,
            cursor,
        })
    }

    fn state(&mut self) -> Option<State> {
        let color = u32::from_le_bytes(self.array()?) & 0xff_ffff;
        let name = std::str::from_utf8(self.chunk()?).ok()?;
        let name = name.chars().take(NAME_CHARS).collect();
        let selection = match self.array()? {
            [0] => None,
            [1] => Some(Published::Text(self.end()?, self.end()?)),
            [2] => {
                let count = u16::from_le_bytes(self.array()?);
                let blocks = (0..count).map(|_| self.block());
                Some(Published::Blocks(blocks.collect::<Option<_>>()?))
            }
            [3] => Some(Published::Cells(self.block()?, self.block()?)),
            _ => return None,
        };
        let state = State {
            name,
            color,
            selection,
        };
        self.0.is_empty().then_some(state)
    }
}

impl Document {
    /// The selection a peer published, where it is in this document, if it
    /// is all there.
    fn shown(&self, published: &Published) -> Option<Selection> {
        let end = |end: &End| {
            let text = self.inline(end.block).ok()?;
            let offset = match &end.cursor {
                // A cursor of another text says nothing of this one.
                Some(cursor) if cursor.container == text.id() => {
                    let at = self.doc.get_cursor_pos(cursor).ok()?.current.pos;
                    text.convert_pos(at, PosType::Unicode, PosType::Bytes)?
                }
                Some(_) => return None,
                None => end.offset as usize,
            };
            let pos = Pos::new(end.block, offset);
            self.check_pos(pos).ok().map(|()| pos)
        };
        let selection = match published {
            Published::Text(anchor, head) => Selection::Text {
                anchor: end(anchor)?,
                head: end(head)?,
            },
            Published::Blocks(blocks) => Selection::Blocks(blocks.clone()),
            Published::Cells(anchor, head) => Selection::Cells {
                anchor: *anchor,
                head: *head,
            },
        };
        self.check_selection(&selection).ok().map(|()| selection)
    }
}
