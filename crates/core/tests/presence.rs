use lotype_core::test_support::{build, editor};
use lotype_core::{
    Document, MAX_PRESENCE_BYTES, Pos, Presence, Schema, Selection, SyncMessage, Version, blocks,
    divider, p,
};

/// Another peer that starts from `doc` as it is now.
fn peer(doc: &Document) -> Document {
    let mut peer = Document::new(Schema::builtin());
    peer.import(&doc.export_snapshot().unwrap()).unwrap();
    peer
}

#[test]
fn a_message_reads_back_as_it_was_written() {
    let (doc, _) = build(blocks![p!["text"]]);
    for message in [
        SyncMessage::Version(doc.version()),
        SyncMessage::Version(Version::default()),
        SyncMessage::Update(doc.export_snapshot().unwrap()),
        SyncMessage::Presence(vec![1, 2, 3]),
    ] {
        assert_eq!(SyncMessage::decode(&message.encode()).unwrap(), message);
    }
    for bytes in [&b""[..], &[3], &[0, 0xff, 0xff, 0xff, 0xff, 0xff]] {
        assert!(SyncMessage::decode(bytes).is_err());
    }
}

/// A presence as a relay passes it on: under the number of its client.
fn relayed(mut presence: Vec<u8>, client: u64) -> Vec<u8> {
    Presence::readdress(&mut presence, client).unwrap();
    presence
}

#[test]
fn a_peer_sees_the_others_until_they_leave() {
    let a = editor(blocks![p!["he<a>ll<b>o"]]);
    let b = peer(a.document());
    let at_b = Presence::new();
    assert!(at_b.collaborators(&b).is_empty());

    let ann = Presence::publish(a.document(), "Ann", 0xff_8800, a.selection());
    at_b.apply(&ann).unwrap();
    let seen = at_b.collaborators(&b);
    assert_eq!(seen.len(), 1);
    assert_eq!((seen[0].name.as_str(), seen[0].color), ("Ann", 0xff_8800));
    assert_eq!(seen[0].selection.as_ref(), a.selection());

    // The last said holds, however soon after the one before.
    at_b.apply(&Presence::publish(a.document(), "Ann B.", 1, None))
        .unwrap();
    let seen = at_b.collaborators(&b);
    assert_eq!(
        (seen[0].name.as_str(), seen[0].selection.clone()),
        ("Ann B.", None)
    );
    at_b.apply(&Presence::leave(a.document())).unwrap();
    assert!(at_b.collaborators(&b).is_empty());
}

#[test]
fn a_relay_says_whose_a_presence_is() {
    let (doc, _) = build(blocks![p!["text"]]);
    let presence = Presence::new();
    // Two clients that both say they are the same peer are two.
    for client in [7, 8] {
        let sent = Presence::publish(&doc, "Ann", 0, None);
        presence.apply(&relayed(sent, client)).unwrap();
    }
    let peers: Vec<u64> = presence
        .collaborators(&doc)
        .iter()
        .map(|c| c.peer)
        .collect();
    assert_eq!(peers, [7, 8]);
    // And one that leaves takes only itself out.
    presence.apply(&relayed(Presence::leave(&doc), 7)).unwrap();
    assert_eq!(presence.collaborators(&doc)[0].peer, 8);

    assert!(Presence::readdress(&mut [0; 7], 1).is_err());
    assert!(Presence::readdress(&mut vec![0; MAX_PRESENCE_BYTES + 1], 1).is_err());
}

#[test]
fn a_selection_follows_its_text_through_changes_it_was_not_sent_after() {
    let a = editor(blocks![p!["one <a>two<b>"]]);
    let mut b = peer(a.document());
    let block = b.blocks()[0];
    let presence = Presence::new();
    presence
        .apply(&Presence::publish(a.document(), "Ann", 0, a.selection()))
        .unwrap();

    b.insert_text(block, 0, "zero ").unwrap();
    let selection = presence.collaborators(&b)[0].selection.clone();
    let (anchor, head) = (Pos::new(block, 9), Pos::new(block, 12));
    assert_eq!(selection, Some(Selection::Text { anchor, head }));
}

#[test]
fn a_selection_in_text_not_here_yet_shows_once_it_is() {
    let (mut a, _) = build(blocks![p!["old"]]);
    let mut b = peer(&a);
    let block = a.blocks()[0];
    a.insert_text(block, 3, " new").unwrap();
    let caret = Selection::caret(Pos::new(block, 5));
    let presence = Presence::new();
    presence
        .apply(&Presence::publish(&a, "Ann", 0, Some(&caret)))
        .unwrap();
    assert_eq!(presence.collaborators(&b)[0].selection, None);

    b.import(&a.export_updates(&b.version()).unwrap()).unwrap();
    assert_eq!(presence.collaborators(&b)[0].selection, Some(caret));
}

#[test]
fn blocks_and_cells_are_selections_too() {
    let (mut a, _) = build(blocks![p!["one"], divider![]]);
    let blocks = Selection::Blocks(a.blocks());
    let table = a.insert_table(None, 2, 2, 2).unwrap();
    let rows = a.children(Some(table)).unwrap();
    let columns = a.columns(table).unwrap();
    let cells = Selection::Cells {
        anchor: a.cell(rows[0], columns[0]).unwrap(),
        head: a.cell(rows[1], columns[1]).unwrap(),
    };
    // A caret in a cell nothing was written in, which has no text to hold on to.
    let empty = Selection::caret(Pos::new(a.cell(rows[1], columns[0]).unwrap(), 0));
    let b = peer(&a);
    let presence = Presence::new();
    for selection in [blocks, cells, empty] {
        presence
            .apply(&Presence::publish(&a, "Ann", 0, Some(&selection)))
            .unwrap();
        assert_eq!(presence.collaborators(&b)[0].selection, Some(selection));
    }
}

#[test]
fn a_presence_is_never_more_than_a_relay_passes_on() {
    let (mut doc, _) = build(blocks![]);
    for at in 0..400 {
        doc.insert_block(None, at, "paragraph").unwrap();
    }
    let all = Selection::Blocks(doc.blocks());
    let name = "n".repeat(10_000);
    let sent = Presence::publish(&doc, &name, 0, Some(&all));
    assert!(sent.len() <= MAX_PRESENCE_BYTES);
    let presence = Presence::new();
    presence.apply(&sent).unwrap();
    // The selection did not fit, so the peer is there without one.
    let seen = presence.collaborators(&doc);
    assert_eq!((seen[0].name.len(), seen[0].selection.clone()), (64, None));
}

#[test]
fn what_is_no_presence_is_an_error_and_changes_nothing() {
    let (doc, _) = build(blocks![p!["text"]]);
    let caret = Selection::caret(Pos::new(doc.blocks()[0], 2));
    let good = Presence::publish(&doc, "Ann", 0, Some(&caret));
    let presence = Presence::new();
    presence.apply(&good).unwrap();
    let before = presence.collaborators(&doc);
    // Cut short anywhere but after the peer, where it is the peer leaving.
    let cuts = (0..good.len()).filter(|end| *end != 8);
    let mut bad: Vec<Vec<u8>> = cuts.map(|end| good[..end].to_vec()).collect();
    bad.push([good.as_slice(), &[0]].concat());
    bad.push(vec![0; MAX_PRESENCE_BYTES + 1]);
    for bytes in bad {
        assert!(presence.apply(&bytes).is_err(), "{} bytes", bytes.len());
        assert!(Presence::readdress(&mut bytes.clone(), 1).is_err());
        assert_eq!(presence.collaborators(&doc), before);
    }
}

#[test]
fn a_presence_with_any_byte_changed_is_taken_or_refused_without_fail() {
    let (doc, _) = build(blocks![p!["text"]]);
    let caret = Selection::caret(Pos::new(doc.blocks()[0], 2));
    let good = Presence::publish(&doc, "Ann", 0, Some(&caret));
    let presence = Presence::new();
    for at in 0..good.len() {
        for flip in [0x01, 0x55, 0x80, 0xff] {
            let mut changed = good.clone();
            changed[at] ^= flip;
            let _ = presence.apply(&changed);
            let _ = presence.collaborators(&doc);
        }
    }
}
