use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use axum::Router;
use axum::extract::ws::{CloseFrame, Message, WebSocket, WebSocketUpgrade, close_code};
use axum::extract::{Path, State};
use axum::http::header::AUTHORIZATION;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::any;
use lotype_core::{Presence, SyncMessage, SyncParts};

use crate::room::{Room, Rooms};
use crate::{Access, Authorizer, DocId, DocStore};

/// A client that takes no message for this long is gone, whatever its
/// connection says.
const SEND_TIMEOUT: Duration = Duration::from_secs(30);

/// An update goes in messages of no more than this, as [`SyncMessage::parts`]
/// cuts it: a client's WebSocket takes only so much in one.
const PART: usize = 1 << 20;

/// And one that comes in parts is no longer than this together, the most
/// one message can be.
const MAX_UPDATE: usize = 64 << 20;

struct Relay<A, S> {
    authorizer: A,
    store: S,
    rooms: Rooms,
    clients: AtomicU64,
}

/// The relay, to nest in the application's router: `/{document id}` is the
/// WebSocket of that document.
pub fn router<A: Authorizer, S: DocStore>(authorizer: A, store: S) -> Router {
    let relay = Relay {
        authorizer,
        store,
        rooms: Rooms::default(),
        clients: AtomicU64::new(0),
    };
    Router::new()
        .route("/{doc}", any(connect::<A, S>))
        .with_state(Arc::new(relay))
}

async fn connect<A: Authorizer, S: DocStore>(
    State(relay): State<Arc<Relay<A, S>>>,
    Path(doc): Path<String>,
    headers: HeaderMap,
    upgrade: WebSocketUpgrade,
) -> Response {
    let doc = DocId::new(doc);
    let token = headers.get(AUTHORIZATION).and_then(|value| {
        let value = value.to_str().ok()?;
        value.strip_prefix("Bearer ")
    });
    let access = match token {
        Some(token) => relay.authorizer.authorize(token, &doc).await,
        None => Err(crate::AuthError),
    };
    let Ok(access) = access else {
        return StatusCode::UNAUTHORIZED.into_response();
    };
    let Ok(room) = relay.rooms.join(&relay.store, &doc).await else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    upgrade.on_upgrade(move |socket| async move {
        let client = Client {
            id: relay.clients.fetch_add(1, Ordering::Relaxed),
            access,
            doc,
            room,
        };
        client.serve(socket, &relay.store).await;
        // The others need not wait to stop hearing of it.
        let left = client.id.to_le_bytes().to_vec();
        client.room.publish(client.id, left);
    })
}

struct Client {
    id: u64,
    access: Access,
    doc: DocId,
    room: Arc<Room>,
}

async fn send(socket: &mut WebSocket, message: Message) -> bool {
    let sent = tokio::time::timeout(SEND_TIMEOUT, socket.send(message));
    matches!(sent.await, Ok(Ok(())))
}

/// Sends an encoded message, a long update in parts.
async fn send_sync(socket: &mut WebSocket, encoded: &[u8]) -> bool {
    for part in SyncMessage::parts(encoded, PART) {
        if !send(socket, Message::Binary(part.into())).await {
            return false;
        }
    }
    true
}

/// Runs work on a document where it holds up no other task: a large one
/// takes a while to import or to export.
async fn apart<T: Send + 'static>(
    room: &Arc<Room>,
    work: impl FnOnce(&Room) -> Result<T, u16> + Send + 'static,
) -> Result<T, u16> {
    let room = Arc::clone(room);
    let done = tokio::task::spawn_blocking(move || work(&room)).await;
    done.unwrap_or(Err(close_code::ERROR))
}

impl Client {
    /// Talks to the client until it leaves or is closed.
    async fn serve(&self, mut socket: WebSocket, store: &impl DocStore) {
        // Listening before the greeting, so nothing falls between them.
        let mut relayed = self.room.relay.subscribe();
        let mut greeting = self.room.present();
        // The answer to the version is an update, which is not for a reader to send.
        if self.access == Access::Write {
            let _stored = self.room.stored.lock().await;
            if let Ok(version) = self.room.version() {
                greeting.push(SyncMessage::Version(version).encode().into());
            }
        }
        for message in greeting {
            if !send_sync(&mut socket, &message).await {
                return;
            }
        }
        let mut parts = SyncParts::default();
        let code = loop {
            if self.room.failed() {
                break close_code::ERROR;
            }
            tokio::select! {
                message = socket.recv() => {
                    let bytes = match message {
                        Some(Ok(Message::Binary(bytes))) => bytes,
                        Some(Ok(Message::Ping(_) | Message::Pong(_))) => continue,
                        Some(Ok(Message::Text(_))) => break close_code::UNSUPPORTED,
                        Some(Ok(Message::Close(_)) | Err(_)) | None => return,
                    };
                    match self.receive(&bytes, &mut parts, store).await {
                        Ok(Some(answer)) => {
                            if !send_sync(&mut socket, &answer.encode()).await {
                                return;
                            }
                        }
                        Ok(None) => {}
                        Err(code) => break code,
                    }
                }
                message = relayed.recv() => match message {
                    Ok((from, bytes)) if from != self.id && !self.room.failed() => {
                        if !send_sync(&mut socket, &bytes).await {
                            return;
                        }
                    }
                    Ok(_) => {}
                    // Too far behind the others to go on from here.
                    Err(_) => break close_code::AGAIN,
                },
            }
        };
        let reason = "".into();
        let close = Message::Close(Some(CloseFrame { code, reason }));
        send(&mut socket, close).await;
    }

    /// Does what a message from the client asks. Clients are not trusted:
    /// one that sends what it may not, or what is no message, is closed
    /// with the code here.
    async fn receive(
        &self,
        bytes: &[u8],
        parts: &mut SyncParts,
        store: &impl DocStore,
    ) -> Result<Option<SyncMessage>, u16> {
        let room = &self.room;
        let message = SyncMessage::decode(bytes).map_err(|_| close_code::POLICY)?;
        // Before its parts are kept: a reader's are never an update.
        let update = matches!(message, SyncMessage::Update(_) | SyncMessage::Part(_));
        if update && self.access != Access::Write {
            return Err(close_code::POLICY);
        }
        let message = parts.join(message, MAX_UPDATE);
        let Some(message) = message.map_err(|_| close_code::POLICY)? else {
            return Ok(None);
        };
        match message {
            SyncMessage::Version(version) => {
                let _stored = room.stored.lock().await;
                let updates = apart(room, move |room| room.updates_since(&version)).await?;
                return Ok(Some(SyncMessage::Update(updates)));
            }
            SyncMessage::Update(update) => {
                let _stored = room.stored.lock().await;
                let id = self.id;
                let (brought, valid) = apart(room, move |room| room.import(&update, id)).await?;
                if let Some((has, brought)) = brought {
                    if store.append_update(&self.doc, &brought).await.is_err() {
                        room.fail();
                        return Err(close_code::ERROR);
                    }
                    let message = SyncMessage::Update(brought).encode();
                    let _ = room.relay.send((has, message.into()));
                }
                if !valid {
                    return Err(close_code::POLICY);
                }
            }
            // Joined with their end above.
            SyncMessage::Part(_) => {}
            SyncMessage::Presence(mut presence) => {
                // Under the client's number, whoever it says it is.
                let own = Presence::readdress(&mut presence, self.id);
                own.map_err(|_| close_code::POLICY)?;
                room.publish(self.id, presence);
            }
        }
        Ok(None)
    }
}
