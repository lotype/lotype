//! A relay in a server of its own, and clients over WebSockets.

#![allow(dead_code)]

use std::net::SocketAddr;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use lotype_core::test_support::outline;
use lotype_core::{Document, Presence, Schema, SyncMessage, SyncParts, Version};
use lotype_server::{Access, AuthError, Authorizer, DocId, DocStore, router};
use lotype_storage::SqliteStore;
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream, connect_async};

pub const POLICY: u16 = 1008;
pub const INTERNAL: u16 = 1011;
pub const UNSUPPORTED: u16 = 1003;

struct Tokens;

impl Authorizer for Tokens {
    async fn authorize(&self, token: &str, _: &DocId) -> Result<Access, AuthError> {
        match token {
            "writer" => Ok(Access::Write),
            "reader" => Ok(Access::Read),
            _ => Err(AuthError),
        }
    }
}

pub fn store() -> SqliteStore {
    SqliteStore::open(":memory:").unwrap()
}

/// Serves the relay as an application would, under a path of its own.
pub async fn relay(store: impl DocStore) -> SocketAddr {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let app = axum::Router::new().nest("/docs", router(Tokens, store));
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    address
}

pub fn request(address: SocketAddr, token: Option<&str>) -> impl IntoClientRequest + Unpin {
    let mut request = format!("ws://{address}/docs/notes")
        .into_client_request()
        .unwrap();
    if let Some(token) = token {
        let bearer = format!("Bearer {token}").parse().unwrap();
        request.headers_mut().insert("Authorization", bearer);
    }
    request
}

pub fn empty() -> Document {
    Document::new(Schema::builtin())
}

pub fn opened(parts: Vec<Vec<u8>>) -> Document {
    let mut doc = empty();
    for part in parts {
        doc.import(&part).unwrap();
    }
    doc
}

/// What is stored of the document the tests are in, as text.
pub async fn stored(store: &impl DocStore) -> String {
    let parts = store.load(&DocId::new("notes")).await.unwrap();
    outline(&opened(parts), None)
}

/// A client as an application writes one.
pub struct Peer {
    pub doc: Document,
    pub presence: Presence,
    pub socket: WebSocketStream<MaybeTlsStream<TcpStream>>,
    /// What the relay has of this peer's changes.
    pub sent: Version,
    /// The relay's versions, as it asked for what it lacks.
    pub asked: Vec<Version>,
    parts: SyncParts,
    /// The longest message the relay sent.
    pub longest: usize,
    pub updates: usize,
}

impl Peer {
    /// Connects and asks for what `doc` lacks, without waiting for it.
    pub async fn open(address: SocketAddr, token: &str, doc: Document) -> Self {
        let (socket, _) = connect_async(request(address, Some(token))).await.unwrap();
        let mut peer = Self {
            sent: doc.version(),
            doc,
            presence: Presence::new(),
            socket,
            asked: Vec::new(),
            parts: SyncParts::default(),
            longest: 0,
            updates: 0,
        };
        peer.send(SyncMessage::Version(peer.doc.version())).await;
        peer
    }

    pub async fn connect(address: SocketAddr, token: &str, doc: Document) -> Self {
        let mut peer = Self::open(address, token, doc).await;
        // The answer, after which the relay owes this peer none: the next
        // update to come is another's, or the answer `settle` waits for.
        peer.until(|peer| peer.updates > 0).await;
        peer
    }

    pub async fn send(&mut self, message: SyncMessage) {
        let message = Message::Binary(message.encode().into());
        self.socket.send(message).await.unwrap();
    }

    /// Sends the changes made here since the last time.
    pub async fn push(&mut self) {
        let update = self.doc.export_updates(&self.sent).unwrap();
        self.sent = self.doc.version();
        self.send(SyncMessage::Update(update)).await;
    }

    /// The next message, or the code the relay closed with.
    pub async fn next(&mut self) -> Result<SyncMessage, Option<u16>> {
        loop {
            let waited = tokio::time::timeout(Duration::from_secs(5), self.socket.next());
            match waited.await.expect("a message in time") {
                Some(Ok(Message::Binary(bytes))) => {
                    self.longest = self.longest.max(bytes.len());
                    let message = SyncMessage::decode(&bytes).unwrap();
                    if let Some(whole) = self.parts.join(message, usize::MAX).unwrap() {
                        return Ok(whole);
                    }
                }
                Some(Ok(Message::Close(frame))) => return Err(frame.map(|f| f.code.into())),
                Some(Ok(_)) => {}
                Some(Err(_)) | None => return Err(None),
            }
        }
    }

    /// Takes messages, and does what they ask, until `done`.
    pub async fn until(&mut self, done: impl Fn(&Self) -> bool) {
        while !done(self) {
            match self.next().await.expect("the relay to stay") {
                SyncMessage::Version(version) => {
                    let update = self.doc.export_updates(&version).unwrap();
                    self.asked.push(version);
                    self.send(SyncMessage::Update(update)).await;
                }
                SyncMessage::Update(update) => {
                    self.doc.import(&update).unwrap();
                    self.sent = self.doc.version();
                    self.updates += 1;
                }
                SyncMessage::Presence(presence) => self.presence.apply(&presence).unwrap(),
                // Put together before it comes here.
                SyncMessage::Part(_) => {}
            }
        }
    }

    /// Waits until the relay has done all that was sent to it: it answers
    /// in the order it is asked.
    pub async fn settle(&mut self) {
        let updates = self.updates;
        self.send(SyncMessage::Version(self.doc.version())).await;
        self.until(|peer| peer.updates > updates).await;
    }

    pub async fn closed(&mut self) -> Option<u16> {
        loop {
            if let Err(code) = self.next().await {
                return code;
            }
        }
    }

    pub fn text(&self) -> String {
        outline(&self.doc, None)
    }
}
