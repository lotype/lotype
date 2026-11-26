use lotype_core::test_support::{build, editor};
use lotype_core::{
    Document, MAX_PRESENCE_BYTES, Pos, Presence, Schema, Selection, SyncMessage, SyncParts,
    Version, blocks, divider, p,
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
        SyncMessage::Part(vec![4, 5]),
    ] {
        assert_eq!(SyncMessage::decode(&message.encode()).unwrap(), message);
    }
    for bytes in [&b""[..], &[4], &[0, 0xff, 0xff, 0xff, 0xff, 0xff]] {
        assert!(SyncMessage::decode(bytes).is_err());
    }
}

#[test]
fn a_long_update_goes_in_parts_and_comes_together() {
    let update: Vec<u8> = (0..=255).cycle().take(1000).collect();
    let encoded = SyncMessage::Update(update.clone()).encode();
    for limit in [2, 7, 100, 1000, 1001, 5000] {
        let parts = SyncMessage::parts(&encoded, limit);
        assert!(parts.iter().all(|part| part.len() <= limit));
        assert_eq!(parts.len() == 1, limit > 1000);
        let mut joined = SyncParts::default();
        let mut whole = None;
        for part in &parts {
            assert!(whole.is_none());
            // Another message between the parts is itself.
            let presence = SyncMessage::Presence(vec![9]);
            assert_eq!(joined.join(presence.clone(), 5000).unwrap(), Some(presence));
            whole = joined
                .join(SyncMessage::decode(part).unwrap(), 5000)
                .unwrap();
        }
        assert_eq!(whole, Some(SyncMessage::Update(update.clone())));
    }
    // What is no update is never cut, and parts without end are refused.
    let presence = SyncMessage::Presence(vec![1; 50]).encode();
    assert_eq!(SyncMessage::parts(&presence, 10), [presence]);
    let mut joined = SyncParts::default();
    assert_eq!(
        joined.join(SyncMessage::Part(vec![0; 60]), 100).unwrap(),
        None
    );
    assert!(joined.join(SyncMessage::Part(vec![0; 60]), 100).is_err());
    let end = SyncMessage::Update(vec![7]);
    assert_eq!(joined.join(end.clone(), 100).unwrap(), Some(end));
}

#[test]
fn the_receive_limit_counts_the_whole_update_payload() {
    let mut parts = SyncParts::default();
    let whole = SyncMessage::Update(vec![1, 2, 3, 4]);
    assert_eq!(parts.join(whole.clone(), 4).unwrap(), Some(whole.clone()));
    assert!(parts.join(whole.clone(), 3).is_err());

    assert_eq!(parts.join(SyncMessage::Part(vec![1, 2]), 4).unwrap(), None);
    assert_eq!(
        parts.join(SyncMessage::Update(vec![3, 4]), 4).unwrap(),
        Some(whole.clone())
    );
    assert_eq!(parts.join(SyncMessage::Part(vec![1, 2]), 4).unwrap(), None);
    assert!(parts.join(SyncMessage::Update(vec![3, 4, 5]), 4).is_err());
    assert_eq!(parts.join(whole.clone(), 4).unwrap(), Some(whole));
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
fn an_ai_block_moves_and_clears_without_replacing_the_human_selection() {
    let (doc, _) = build(blocks![p!["one"], divider![], p!["three"]]);
    let copy = peer(&doc);
    let blocks = doc.blocks();
    let caret = Selection::caret(Pos::new(blocks[0], 1));
    let presence = Presence::new();
    for anchor in [Some(blocks[1]), Some(blocks[2]), None] {
        let sent = Presence::publish_with_ai(&doc, "Andi", 0xf0_8020, Some(&caret), anchor);
        presence.apply(&relayed(sent, 7)).unwrap();
        let seen = presence.collaborators(&copy);
        assert_eq!(seen.len(), 1);
        assert_eq!(seen[0].peer, 7);
        assert_eq!(seen[0].selection.as_ref(), Some(&caret));
        assert_eq!(seen[0].ai_block, anchor);
    }

    let mut draft = peer(&doc);
    let private = draft.insert_block(None, 0, "paragraph").unwrap();
    let sent = Presence::publish_with_ai(&doc, "Andi", 0, None, Some(private));
    assert_eq!(sent, Presence::publish(&doc, "Andi", 0, None));
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

    let block = doc.blocks()[0];
    let sent = Presence::publish_with_ai(&doc, &name, 0, Some(&all), Some(block));
    assert!(sent.len() <= MAX_PRESENCE_BYTES);
    presence.apply(&sent).unwrap();
    let seen = presence.collaborators(&doc);
    assert_eq!(seen[0].selection, None);
    assert_eq!(seen[0].ai_block, Some(block));
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
