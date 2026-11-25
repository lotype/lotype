//! The relay with clients that do as they should, and some that do not.

mod support;

use futures_util::SinkExt;
use lotype_core::test_support::build;
use lotype_core::{Presence, SyncMessage, blocks, p};
use support::{POLICY, Peer, UNSUPPORTED, empty, relay, request, store, stored};
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::{Error, Message};

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
