use std::collections::HashMap;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, Weak};
use std::time::Instant;

use axum::body::Bytes;
use axum::extract::ws::close_code;
use lotype_core::{Document, Error, PRESENCE_TIMEOUT, SyncMessage, Version};
use lotype_storage::{DocId, DocStore, StoreError, restore};
use tokio::sync::broadcast;

/// How many messages a client can be behind the others before it is
/// closed, to connect again and get what it lacks at once.
const BACKLOG: usize = 1024;

/// In place of a client's number, where a message is for all of them.
pub(crate) const EVERYONE: u64 = u64::MAX;

/// What an update brought a room, and the client not to send it to.
pub(crate) type Brought = (u64, Vec<u8>);

/// A document with clients connected, kept in memory while any is.
pub(crate) struct Room {
    /// `None` once Loro panicked in it.
    doc: Mutex<Option<Document>>,
    /// What each client last said of itself, as the message the others
    /// got, and when.
    present: Mutex<HashMap<u64, (Bytes, Instant)>>,
    /// Held from importing an update until it is stored, and to say what
    /// the room has: so nothing leaves the room that the store has not,
    /// and no update is on its way to the store when the room is given up.
    pub(crate) stored: tokio::sync::Mutex<()>,
    /// Messages for the clients, each with the number of the one not to
    /// send it to: the client it came of.
    pub(crate) relay: broadcast::Sender<(u64, Bytes)>,
    /// The room has what the store has not, or no document. See [`Room::fail`].
    failed: AtomicBool,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

impl Room {
    fn new(stored: Vec<Vec<u8>>) -> Result<Self, StoreError> {
        let doc = restore(stored)?;
        // The room sends updates, which are of no use without the history.
        if doc.is_shallow() {
            return Err("the stored document has no history".into());
        }
        Ok(Self {
            doc: Mutex::new(Some(doc)),
            present: Mutex::default(),
            stored: tokio::sync::Mutex::default(),
            relay: broadcast::channel(BACKLOG).0,
            failed: AtomicBool::new(false),
        })
    }

    /// Reads or changes the document, or gives the code to close the client
    /// with. Loro checks what it imports, but bytes can be made that it
    /// panics on, and it panics again when a document it panicked in is
    /// used or dropped. Such a document is left where it is, never dropped,
    /// and the room given up.
    fn with_doc<T>(
        &self,
        work: impl FnOnce(&mut Document) -> lotype_core::Result<T>,
    ) -> Result<T, u16> {
        let mut slot = lock(&self.doc);
        let doc = slot.as_mut().filter(|_| !self.failed());
        let doc = doc.ok_or(close_code::ERROR)?;
        match catch_unwind(AssertUnwindSafe(|| work(doc))) {
            Ok(done) => done.map_err(|_| close_code::POLICY),
            Err(_) => {
                std::mem::forget(slot.take());
                self.fail();
                Err(close_code::ERROR)
            }
        }
    }

    pub(crate) fn version(&self) -> Result<Version, u16> {
        self.with_doc(|doc| Ok(doc.version()))
    }

    pub(crate) fn updates_since(&self, version: &Version) -> Result<Vec<u8>, u16> {
        self.with_doc(|doc| doc.export_updates(version))
    }

    /// Merges an update. Gives what it brought the room, as an update of
    /// the room's own to store and pass on, with the number of the client
    /// that has it all already, and whether the update was all good.
    ///
    /// What the room made holds everything that came into the document
    /// with the update, and nothing a client made up. That can be changes
    /// of others that came before and waited for these: then no client has
    /// it all, the one that sent the update either.
    pub(crate) fn import(&self, update: &[u8], from: u64) -> Result<(Option<Brought>, bool), u16> {
        self.with_doc(|doc| {
            let before = doc.version();
            // Loro can fail to say what waits; then something does.
            let has = match doc.export_waiting() {
                Ok(None) => from,
                _ => EVERYONE,
            };
            let valid = doc.import(update).is_ok();
            // A snapshot without its history leaves a document whose
            // updates are of no use to anyone. And what the room has and
            // cannot say is nothing to keep it for.
            let brought = match doc.version() == before {
                true => Ok(None),
                false if doc.is_shallow() => Err(Error::InvalidUpdate("no history".into())),
                false => doc
                    .export_updates(&before)
                    .map(|update| Some((has, update))),
            };
            brought
                .inspect_err(|_| self.fail())
                .map(|brought| (brought, valid))
        })
    }

    /// Keeps what a client says of itself for those who come later, and
    /// passes it on. A presence of only the client's number is its leaving.
    pub(crate) fn publish(&self, client: u64, presence: Vec<u8>) {
        let left = presence.len() == size_of::<u64>();
        let message = Bytes::from(SyncMessage::Presence(presence).encode());
        let mut present = lock(&self.present);
        match left {
            true => present.remove(&client),
            false => present.insert(client, (message.clone(), Instant::now())),
        };
        // Under the lock, so that the clients hear it in the order kept.
        let _ = self.relay.send((client, message));
    }

    /// What the clients here last said of themselves, unless so long ago
    /// that the others have forgotten them.
    pub(crate) fn present(&self) -> Vec<Bytes> {
        let present = lock(&self.present);
        let fresh = present
            .values()
            .filter(|(_, said)| said.elapsed() < PRESENCE_TIMEOUT);
        fresh.map(|(message, _)| message.clone()).collect()
    }

    /// Gives the room up, as it has what the store does not. Its clients
    /// are closed, and connecting again they find a room made of what is
    /// stored, which asks them for the rest.
    pub(crate) fn fail(&self) {
        self.failed.store(true, Ordering::Relaxed);
        // Wakes the clients.
        let _ = self.relay.send((EVERYONE, Bytes::new()));
    }

    pub(crate) fn failed(&self) -> bool {
        self.failed.load(Ordering::Relaxed)
    }
}

type Slot = Arc<tokio::sync::Mutex<Weak<Room>>>;

/// The rooms with clients in them.
#[derive(Default)]
pub(crate) struct Rooms(Mutex<HashMap<DocId, Slot>>);

impl Rooms {
    /// The document's room, loaded from the store when no client has it
    /// open. Clients of one document wait for its load, and no others.
    pub(crate) async fn join(
        &self,
        store: &impl DocStore,
        doc: &DocId,
    ) -> Result<Arc<Room>, StoreError> {
        let slot = {
            let mut rooms = lock(&self.0);
            // A slot goes when no client is in its room or on the way there.
            rooms.retain(|_, slot| {
                let open = |slot: &Slot| slot.try_lock().is_ok_and(|room| room.strong_count() > 0);
                Arc::strong_count(slot) > 1 || open(slot)
            });
            Arc::clone(rooms.entry(doc.clone()).or_default())
        };
        let mut slot = slot.lock().await;
        if let Some(room) = slot.upgrade().filter(|room| !room.failed()) {
            return Ok(room);
        }
        let room = Arc::new(Room::new(store.load(doc).await?)?);
        *slot = Arc::downgrade(&room);
        Ok(room)
    }
}
