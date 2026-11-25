//! The relay when the store, Loro or what a client sends fails it.

mod support;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use lotype_core::test_support::{build, outline};
use lotype_core::{SyncMessage, Version, blocks, p};
use lotype_server::{DocId, DocStore, StoreError};
use lotype_storage::SqliteStore;
use support::{INTERNAL, POLICY, Peer, empty, opened, relay, request, store, stored};
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Error;

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

/// A store that holds an update until told, and then does not take it.
#[derive(Clone, Default)]
struct Held(Arc<tokio::sync::Notify>);

impl DocStore for Held {
    async fn load(&self, _: &DocId) -> Result<Vec<Vec<u8>>, StoreError> {
        Ok(Vec::new())
    }

    async fn append_update(&self, _: &DocId, _: &[u8]) -> Result<(), StoreError> {
        self.0.notified().await;
        Err("the disk is full".into())
    }
}

#[tokio::test]
async fn nothing_leaves_a_room_before_the_store_has_it() {
    let store = Held::default();
    let address = relay(store.clone()).await;
    let (doc, _) = build(blocks![p!["not stored"]]);
    let mut writer = Peer::open(address, "writer", doc).await;
    // The relay asks the writer for what it has, and waits on the store.
    writer.until(|writer| !writer.asked.is_empty()).await;
    let mut reader = Peer::open(address, "reader", empty()).await;
    tokio::time::sleep(Duration::from_millis(100)).await;
    store.0.notify_one();

    // The reader asked meanwhile, and is sent away without an answer.
    let code = loop {
        match reader.next().await {
            Ok(SyncMessage::Update(update)) => reader.doc.import(&update).unwrap(),
            Ok(_) => {}
            Err(code) => break code,
        }
    };
    assert_eq!(code, Some(INTERNAL));
    assert_eq!(reader.text(), outline(&empty(), None));
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
