//! The relay in a server of its own, with clients over WebSockets.

use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use lotype_core::test_support::{build, outline};
use lotype_core::{Document, Presence, Schema, SyncMessage, Version, blocks, p};
use lotype_server::{Access, AuthError, Authorizer, DocId, DocStore, StoreError, router};
use lotype_storage::SqliteStore;
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::{Error, Message};
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream, connect_async};

const POLICY: u16 = 1008;
const INTERNAL: u16 = 1011;
const UNSUPPORTED: u16 = 1003;

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

/// A store that takes no updates while told not to.
#[derive(Clone)]
struct Flaky(SqliteStore, Arc<AtomicBool>);

impl DocStore for Flaky {
    async fn load(&self, doc: &DocId) -> Result<Vec<Vec<u8>>, StoreError> {
        self.0.load(doc).await
    }

    async fn append_update(&self, doc: &DocId, update: &[u8]) -> Result<(), StoreError> {
        if self.1.load(Ordering::Relaxed) {
            return Err("the disk is full".into());
        }
        self.0.append_update(doc, update).await
    }
}

fn store() -> SqliteStore {
    SqliteStore::open(":memory:").unwrap()
}

/// Serves the relay as an application would, under a path of its own.
async fn relay(store: impl DocStore) -> SocketAddr {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let app = axum::Router::new().nest("/docs", router(Tokens, store));
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    address
}

fn request(address: SocketAddr, token: Option<&str>) -> impl IntoClientRequest + Unpin {
    let mut request = format!("ws://{address}/docs/notes")
        .into_client_request()
        .unwrap();
    if let Some(token) = token {
        let bearer = format!("Bearer {token}").parse().unwrap();
        request.headers_mut().insert("Authorization", bearer);
    }
    request
}

fn empty() -> Document {
    Document::new(Schema::builtin())
}

fn opened(parts: Vec<Vec<u8>>) -> Document {
    let mut doc = empty();
    for part in parts {
        doc.import(&part).unwrap();
    }
    doc
}

/// What is stored of the document the tests are in, as text.
async fn stored(store: &impl DocStore) -> String {
    let parts = store.load(&DocId::new("notes")).await.unwrap();
    outline(&opened(parts), None)
}

/// A client as an application writes one.
struct Peer {
    doc: Document,
    presence: Presence,
    socket: WebSocketStream<MaybeTlsStream<TcpStream>>,
    /// What the relay has of this peer's changes.
    sent: Version,
    /// The relay's versions, as it asked for what it lacks.
    asked: Vec<Version>,
    updates: usize,
}

impl Peer {
    async fn connect(address: SocketAddr, token: &str, doc: Document) -> Self {
        let (socket, _) = connect_async(request(address, Some(token))).await.unwrap();
        let mut peer = Self {
            sent: doc.version(),
            doc,
            presence: Presence::new(),
            socket,
            asked: Vec::new(),
            updates: 0,
        };
        peer.send(SyncMessage::Version(peer.doc.version())).await;
        // The answer, after which the relay owes this peer none: the next
        // update to come is another's, or the answer `settle` waits for.
        peer.until(|peer| peer.updates > 0).await;
        peer
    }

    async fn send(&mut self, message: SyncMessage) {
        let message = Message::Binary(message.encode().into());
        self.socket.send(message).await.unwrap();
    }

    /// Sends the changes made here since the last time.
    async fn push(&mut self) {
        let update = self.doc.export_updates(&self.sent).unwrap();
        self.sent = self.doc.version();
        self.send(SyncMessage::Update(update)).await;
    }

    /// The next message, or the code the relay closed with.
    async fn next(&mut self) -> Result<SyncMessage, Option<u16>> {
        loop {
            let waited = tokio::time::timeout(Duration::from_secs(5), self.socket.next());
            match waited.await.expect("a message in time") {
                Some(Ok(Message::Binary(bytes))) => {
                    return Ok(SyncMessage::decode(&bytes).unwrap());
                }
                Some(Ok(Message::Close(frame))) => return Err(frame.map(|f| f.code.into())),
                Some(Ok(_)) => {}
                Some(Err(_)) | None => return Err(None),
            }
        }
    }

    /// Takes messages, and does what they ask, until `done`.
    async fn until(&mut self, done: impl Fn(&Self) -> bool) {
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
            }
        }
    }

    /// Waits until the relay has done all that was sent to it: it answers
    /// in the order it is asked.
    async fn settle(&mut self) {
        let updates = self.updates;
        self.send(SyncMessage::Version(self.doc.version())).await;
        self.until(|peer| peer.updates > updates).await;
    }

    async fn closed(&mut self) -> Option<u16> {
        loop {
            if let Err(code) = self.next().await {
                return code;
            }
        }
    }

    fn text(&self) -> String {
        outline(&self.doc, None)
    }
}

#[tokio::test]
async fn clients_end_with_the_same_document_and_the_store_has_it() {
    let store = store();
    let address = relay(store.clone()).await;
    let (doc, _) = build(blocks![p!["from a"]]);
    let mut a = Peer::connect(address, "writer", doc).await;
    let mut b = Peer::connect(address, "writer", empty()).await;
    // The relay asks a for what it has, and passes it on to b.
    let text = a.text();
    b.until(|b| b.text() == text).await;

    let block = b.doc.blocks()[0];
    b.doc.insert_text(block, 6, ", and from b").unwrap();
    b.push().await;
    let text = b.text();
    a.until(|a| a.text() == text).await;
    assert_eq!(a.doc.version(), b.doc.version());

    assert_eq!(stored(&store).await, text);
}

#[tokio::test]
async fn a_relay_started_again_has_the_document_from_the_store() {
    let store = store();
    let address = relay(store.clone()).await;
    let (doc, _) = build(blocks![p!["kept"]]);
    let mut a = Peer::connect(address, "writer", doc).await;
    a.settle().await;

    let mut late = Peer::connect(relay(store).await, "writer", empty()).await;
    let text = a.text();
    late.until(|late| late.text() == text).await;
}

#[tokio::test]
async fn a_reader_gets_changes_and_may_send_none() {
    let store = store();
    let address = relay(store.clone()).await;
    let (doc, _) = build(blocks![p!["read me"]]);
    let mut writer = Peer::connect(address, "writer", doc).await;
    writer.settle().await;

    let mut reader = Peer::connect(address, "reader", empty()).await;
    let text = writer.text();
    reader.until(|reader| reader.text() == text).await;
    let block = writer.doc.blocks()[0];
    writer.doc.insert_text(block, 0, "now ").unwrap();
    writer.push().await;
    let text = writer.text();
    reader.until(|reader| reader.text() == text).await;
    // It is not asked for what it has, and is closed when it sends some.
    assert!(reader.asked.is_empty());

    reader.doc.insert_text(block, 0, "not ").unwrap();
    reader.push().await;
    assert_eq!(reader.closed().await, Some(POLICY));
    writer.settle().await;
    assert_eq!(writer.text(), text);
    assert_eq!(stored(&store).await, text);
}

#[tokio::test]
async fn no_token_and_one_that_is_not_known_are_refused() {
    let address = relay(store()).await;
    for token in [None, Some("stranger")] {
        match connect_async(request(address, token)).await {
            Err(Error::Http(response)) => assert_eq!(response.status(), 401),
            other => panic!("connected, or failed otherwise: {other:?}"),
        }
    }
}

fn names(peer: &Peer) -> Vec<String> {
    let all = peer.presence.collaborators(&peer.doc);
    all.into_iter().map(|one| one.name).collect()
}

#[tokio::test]
async fn presence_reaches_those_there_and_those_who_come() {
    let address = relay(store()).await;
    let mut ann = Peer::connect(address, "reader", empty()).await;
    let mut ben = Peer::connect(address, "writer", empty()).await;
    let here = Presence::publish(&ann.doc, "Ann", 0x33_66ff, None);
    ann.send(SyncMessage::Presence(here)).await;

    ben.until(|ben| names(ben) == ["Ann"]).await;
    let mut late = Peer::connect(address, "reader", empty()).await;
    late.until(|late| names(late) == ["Ann"]).await;

    ann.send(SyncMessage::Presence(Presence::leave(&ann.doc)))
        .await;
    ben.until(|ben| names(ben).is_empty()).await;
    // And one that goes without a word is gone as soon.
    let here = Presence::publish(&late.doc, "Late", 0, None);
    late.send(SyncMessage::Presence(here)).await;
    ben.until(|ben| names(ben) == ["Late"]).await;
    drop(late);
    ben.until(|ben| names(ben).is_empty()).await;
}

#[tokio::test]
async fn no_client_speaks_for_another() {
    let address = relay(store()).await;
    let mut ann = Peer::connect(address, "writer", empty()).await;
    let mut mallory = Peer::connect(address, "reader", empty()).await;
    let mut ben = Peer::connect(address, "writer", empty()).await;
    let here = Presence::publish(&ann.doc, "Ann", 0, None);
    ann.send(SyncMessage::Presence(here)).await;
    ben.until(|ben| names(ben) == ["Ann"]).await;

    // As Ann's peer, another name for her and then her leaving.
    let forged = Presence::publish(&ann.doc, "Not Ann", 0, None);
    mallory.send(SyncMessage::Presence(forged)).await;
    ben.until(|ben| names(ben).len() == 2).await;
    assert_eq!(names(&ben), ["Ann", "Not Ann"]);
    mallory
        .send(SyncMessage::Presence(Presence::leave(&ann.doc)))
        .await;
    ben.until(|ben| names(ben) == ["Ann"]).await;
}

#[tokio::test]
async fn updates_sent_out_of_order_reach_everyone_and_the_store() {
    let store = store();
    let address = relay(store.clone()).await;
    let (doc, _) = build(blocks![p!["base"]]);
    let mut writer = Peer::connect(address, "writer", doc).await;
    writer.settle().await;
    let mut reader = Peer::connect(address, "reader", empty()).await;

    let block = writer.doc.blocks()[0];
    let before = writer.doc.version();
    writer.doc.insert_text(block, 4, " one").unwrap();
    let first = writer.doc.export_updates(&before).unwrap();
    let before = writer.doc.version();
    writer.doc.insert_text(block, 8, " two").unwrap();
    let second = writer.doc.export_updates(&before).unwrap();
    writer.send(SyncMessage::Update(second)).await;
    writer.send(SyncMessage::Update(first)).await;
    writer.sent = writer.doc.version();

    let text = writer.text();
    reader.until(|reader| reader.text() == text).await;
    assert_eq!(stored(&store).await, text);
}

#[tokio::test]
async fn what_is_no_message_closes_the_client() {
    let address = relay(store()).await;
    let bad = [
        (Message::Binary(vec![9, 9].into()), POLICY),
        (Message::Binary(Vec::new().into()), POLICY),
        (Message::Text("hello".into()), UNSUPPORTED),
        (
            Message::Binary(SyncMessage::Update(b"no update".to_vec()).encode().into()),
            POLICY,
        ),
        (
            Message::Binary(SyncMessage::Presence(vec![0xff; 9]).encode().into()),
            POLICY,
        ),
        (
            Message::Binary(SyncMessage::Presence(vec![0; 5000]).encode().into()),
            POLICY,
        ),
    ];
    for (message, code) in bad {
        let mut peer = Peer::connect(address, "writer", empty()).await;
        peer.socket.send(message).await.unwrap();
        assert_eq!(peer.closed().await, Some(code));
    }
}

#[tokio::test]
async fn an_update_the_store_did_not_take_is_asked_for_again() {
    let store = Flaky(store(), Arc::new(AtomicBool::new(true)));
    let address = relay(store.clone()).await;
    let mut bystander = Peer::connect(address, "writer", empty()).await;
    let (doc, _) = build(blocks![p!["not lost"]]);
    let mut a = Peer::connect(address, "writer", doc).await;
    // The room has the update and the store has not: everyone is sent away.
    assert_eq!(a.closed().await, Some(INTERNAL));
    assert_eq!(bystander.closed().await, Some(INTERNAL));
    assert!(store.load(&DocId::new("notes")).await.unwrap().is_empty());

    store.1.store(false, Ordering::Relaxed);
    let mut a = Peer::connect(address, "writer", a.doc).await;
    assert_eq!(a.asked, [Version::default()]);
    a.settle().await;
    assert_eq!(stored(&store).await, a.text());
}

/// An update, and one made of the next by changing bytes of it, on which
/// Loro 1.19 panics instead of refusing it.
const POISON: &str = include_str!("fixtures/relay/poison.hex");

/// The two updates.
fn poison() -> (Vec<u8>, Vec<u8>) {
    let unhex = |line: &str| -> Vec<u8> {
        let byte = |at| u8::from_str_radix(&line[at..at + 2], 16).unwrap();
        (0..line.len()).step_by(2).map(byte).collect()
    };
    let (base, poison) = POISON.trim().split_once('\n').unwrap();
    (unhex(base), unhex(poison))
}

#[tokio::test]
async fn a_stored_document_that_breaks_loro_is_not_opened() {
    let store = store();
    let (base, poison) = poison();
    for update in [base, poison] {
        let id = DocId::new("notes");
        store.append_update(&id, &update).await.unwrap();
    }
    let address = relay(store).await;
    // And again: the relay is still there to say so.
    for _ in 0..2 {
        match connect_async(request(address, Some("writer"))).await {
            Err(Error::Http(response)) => assert_eq!(response.status(), 500),
            other => panic!("connected, or failed otherwise: {other:?}"),
        }
    }
}

#[tokio::test]
async fn an_update_made_to_break_loro_takes_its_room_and_no_more() {
    let (base, poison) = poison();
    let store = store();
    let address = relay(store.clone()).await;
    let mut bystander = Peer::connect(address, "reader", empty()).await;
    let mut mallory = Peer::connect(address, "writer", empty()).await;
    mallory.send(SyncMessage::Update(base)).await;
    mallory.settle().await;
    let text = mallory.text();

    // The document it panicked in is no more use, so everyone is sent away.
    mallory.send(SyncMessage::Update(poison)).await;
    assert_eq!(mallory.closed().await, Some(INTERNAL));
    assert_eq!(bystander.closed().await, Some(INTERNAL));
    // The relay goes on, with the document as it was stored.
    let mut next = Peer::connect(address, "writer", empty()).await;
    next.until(|next| next.text() == text).await;
    assert_eq!(stored(&store).await, text);
}

#[tokio::test]
async fn what_waited_for_an_update_reaches_the_one_who_sent_it_too() {
    let store = store();
    let address = relay(store.clone()).await;
    let (doc, _) = build(blocks![p!["base"]]);
    let mut x = Peer::connect(address, "writer", doc).await;
    x.settle().await;
    let mut y = Peer::connect(address, "writer", empty()).await;
    let block = y.doc.blocks()[0];

    // y changes the text and tells only x, whose next change comes after it.
    let before = y.doc.version();
    y.doc.insert_text(block, 4, " y").unwrap();
    let unsent = y.doc.export_updates(&before).unwrap();
    x.doc.import(&unsent).unwrap();
    x.sent = x.doc.version();
    x.doc.insert_text(block, 6, " x").unwrap();
    x.push().await;
    x.settle().await;

    // With y's change the relay has both, and y, who had not, gets x's.
    y.push().await;
    let text = x.text();
    y.until(|y| y.text() == text).await;
    assert_eq!(stored(&store).await, text);
}

#[tokio::test]
async fn what_the_store_has_waiting_is_in_the_room_when_its_time_comes() {
    let store = store();
    let id = DocId::new("notes");
    let (mut doc, _) = build(blocks![p!["base"]]);
    let base = doc.export_snapshot().unwrap();
    let block = doc.blocks()[0];
    let before = doc.version();
    doc.insert_text(block, 4, " one").unwrap();
    let first = doc.export_updates(&before).unwrap();
    let before = doc.version();
    doc.insert_text(block, 8, " two").unwrap();
    let second = doc.export_updates(&before).unwrap();
    // Stored without the first, as when the store refused that one.
    store.append_update(&id, &base).await.unwrap();
    store.append_update(&id, &second).await.unwrap();

    let address = relay(store.clone()).await;
    let mut reader = Peer::connect(address, "reader", empty()).await;
    let mut writer = Peer::connect(address, "writer", opened(vec![base])).await;
    writer.send(SyncMessage::Update(first)).await;
    let text = outline(&doc, None);
    writer.until(|writer| writer.text() == text).await;
    reader.until(|reader| reader.text() == text).await;
    assert_eq!(stored(&store).await, text);
}

#[tokio::test]
async fn a_snapshot_without_its_history_is_no_update() {
    use loro::{ExportMode, LoroDoc};

    let store = store();
    let address = relay(store.clone()).await;
    let mut bystander = Peer::connect(address, "reader", empty()).await;
    let mut peer = Peer::connect(address, "writer", empty()).await;
    let doc = LoroDoc::new();
    doc.get_text("text").insert(0, "no past").unwrap();
    doc.commit();
    let frontiers = doc.oplog_frontiers();
    let shallow = ExportMode::shallow_snapshot(&frontiers);
    peer.send(SyncMessage::Update(doc.export(shallow).unwrap()))
        .await;
    // The room took it and cannot pass it on, so it is given up.
    assert_eq!(peer.closed().await, Some(POLICY));
    assert_eq!(bystander.closed().await, Some(INTERNAL));
    assert!(store.load(&DocId::new("notes")).await.unwrap().is_empty());
    let next = Peer::connect(address, "writer", empty()).await;
    assert_eq!(next.asked, [Version::default()]);
}
