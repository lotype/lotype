use loro::{ExportMode, LoroDoc, LoroText, TreeID};
use lotype_core::test_support::{assert_document, assert_editor, build, editor, outline};
use lotype_core::{BlockId, Document, Editor, Error, Pos, Schema, Selection, Version, blocks, p};

/// Another peer that starts from `doc` as it is now.
fn peer(doc: &Document) -> Document {
    let mut peer = Document::new(Schema::builtin());
    peer.import(&doc.export_snapshot().unwrap()).unwrap();
    peer
}

/// Sends each side what the other lacks.
fn sync(a: &mut Editor, b: &mut Document) {
    let for_b = a.document().export_updates(&b.version()).unwrap();
    let for_a = b.export_updates(&a.document().version()).unwrap();
    b.import(&for_b).unwrap();
    a.import(&for_a).unwrap();
}

#[test]
fn a_snapshot_starts_another_peer() {
    let (doc, _) = build(blocks![p!["one", p!["child"]], p!["two"]]);
    let peer = peer(&doc);
    assert_eq!(outline(&peer, None), outline(&doc, None));
    assert_eq!(peer.blocks(), doc.blocks());
    assert_eq!(peer.version(), doc.version());
}

#[test]
fn updates_carry_what_a_peer_lacks() {
    let (mut a, _) = build(blocks![p!["shared"]]);
    let mut b = peer(&a);
    let block = a.blocks()[0];
    a.insert_text(block, 0, "a ").unwrap();
    b.insert_text(block, 6, " b").unwrap();

    let for_b = a.export_updates(&b.version()).unwrap();
    let for_a = b.export_updates(&a.version()).unwrap();
    b.import(&for_b).unwrap();
    a.import(&for_a).unwrap();
    assert_document(&a, None, blocks![p!["a shared b"]]);
    assert_document(&b, None, blocks![p!["a shared b"]]);

    // Everything, for a peer that has nothing.
    let mut late = Document::new(Schema::builtin());
    late.import(&a.export_updates(&Version::default()).unwrap())
        .unwrap();
    assert_document(&late, None, blocks![p!["a shared b"]]);
    assert!(a.export_updates(&late.version()).unwrap().len() < for_b.len());
}

#[test]
fn bytes_that_are_not_an_update_are_rejected() {
    let (mut doc, _) = build(blocks![p!["kept"]]);
    let snapshot = doc.export_snapshot().unwrap();
    let mut flipped = snapshot.clone();
    flipped.iter_mut().skip(8).for_each(|byte| *byte ^= 0x5a);
    for bytes in [
        &b""[..],
        b"lotype",
        &snapshot[..snapshot.len() / 2],
        &flipped,
    ] {
        let error = doc.import(bytes).unwrap_err();
        assert!(matches!(error, Error::InvalidUpdate(_)), "{error}");
        assert_document(&doc, None, blocks![p!["kept"]]);
    }
}

#[test]
fn importing_inside_an_edit_cannot_undo_an_earlier_step() {
    let mut a = editor(blocks![p!["x<a>"], p!["other"]]);
    let mut b = peer(a.document());
    b.insert_text(b.blocks()[1], 0, "the ").unwrap();
    let update = b.export_updates(&a.document().version()).unwrap();
    a.insert_text("y").unwrap();

    let result = a.apply(|doc, _| {
        doc.import(&update)?;
        Ok(None)
    });
    assert!(matches!(result, Err(Error::ImportDuringEdit)));
    assert_editor(&a, blocks![p!["xy<a>"], p!["other"]]);
    a.import(&update).unwrap();
    assert!(a.undo().unwrap());
    assert_editor(&a, blocks![p!["x<a>"], p!["the other"]]);
    assert!(!a.can_undo());
}

#[test]
fn rejecting_a_removal_leaves_the_original_block_for_remote_edits() {
    let mut a = editor(blocks![p!["shared"]]);
    let mut b = peer(a.document());
    let block = b.blocks()[0];
    let before = a.document().version();
    b.insert_text(block, 6, " remote").unwrap();

    let rejected = a.apply(|draft, _| {
        draft.remove_block(block)?;
        draft.commit();
        assert!(matches!(
            draft.export_snapshot(),
            Err(Error::ExportDuringEdit)
        ));
        Err(Error::NoSelection)
    });
    assert!(matches!(rejected, Err(Error::NoSelection)));
    assert_eq!(a.document().version(), before);
    sync(&mut a, &mut b);
    assert_document(&b, None, blocks![p!["shared remote"]]);
    assert_editor(&a, blocks![p!["shared remote"]]);
    assert_eq!(a.document().blocks(), [block]);
}

#[test]
fn moving_a_draft_into_a_nested_editor_cannot_allow_import() {
    let mut a = editor(blocks![p!["shared"]]);
    let mut b = peer(a.document());
    b.insert_text(b.blocks()[0], 6, " remote").unwrap();
    let update = b.export_updates(&a.document().version()).unwrap();

    a.apply(|doc, _| {
        let staged = std::mem::replace(doc, Document::new(Schema::builtin()));
        let mut nested = Editor::new(staged);
        assert!(matches!(
            nested.apply(|_, _| panic!("a nested edit ran")),
            Err(Error::NestedEdit)
        ));
        assert!(matches!(
            nested.import(&update),
            Err(Error::ImportDuringEdit)
        ));
        Err(Error::NoSelection)
    })
    .unwrap_err();
    assert_editor(&a, blocks![p!["shared"]]);
}

#[test]
fn a_block_a_peer_brings_back_keeps_its_id() {
    let mut a = editor(blocks![p!["keep"], p!["gone", p!["child"]]]);
    let mut b = peer(a.document());
    let before = a.document().blocks();
    a.set_selection(Some(Selection::Blocks(vec![before[1]])))
        .unwrap();
    a.delete_selection().unwrap();
    sync(&mut a, &mut b);
    assert_eq!(b.blocks(), before[..1]);

    a.undo().unwrap();
    sync(&mut a, &mut b);
    assert_eq!(a.document().blocks(), before);
    assert_eq!(b.blocks(), before);
    assert_eq!(b.text(before[2]).unwrap(), "child");
}

#[test]
fn the_selection_follows_its_text_through_remote_changes() {
    let mut a = editor(blocks![p!["hel<a>lo wor<b>ld"], p!["next"]]);
    let mut b = peer(a.document());
    let [first, next] = b.blocks()[..] else {
        panic!()
    };
    b.insert_text(first, 0, "é ").unwrap();
    b.delete_text(first, 8..10).unwrap();
    b.insert_text(next, 0, "the ").unwrap();
    sync(&mut a, &mut b);
    assert_editor(&a, blocks![p!["é hel<a>loor<b>ld"], p!["the next"]]);

    b.remove_block(first).unwrap();
    sync(&mut a, &mut b);
    assert_editor(&a, blocks![p!["the next"]]);
    assert_eq!(a.selection(), None);
}

#[test]
fn deleting_the_character_before_a_backward_range_keeps_its_text_selected() {
    let mut a = editor(blocks![p!["é🙂<b>cd<a>ef"]]);
    let mut b = peer(a.document());
    b.delete_text(b.blocks()[0], 2..6).unwrap();
    sync(&mut a, &mut b);
    assert_editor(&a, blocks![p!["é<b>cd<a>ef"]]);
}

#[test]
fn delivery_boundaries_do_not_change_a_range_over_replaced_text() {
    let mut a = editor(blocks![p!["x<a>abc<b>y"]]);
    let mut b = peer(a.document());
    let mut c = Editor::new(peer(a.document()));
    c.set_selection(a.selection().cloned()).unwrap();
    let block = b.blocks()[0];

    b.delete_text(block, 1..4).unwrap();
    a.import(&b.export_updates(&a.document().version()).unwrap())
        .unwrap();
    assert!(a.selection().unwrap().is_collapsed());
    b.insert_text(block, 1, "new").unwrap();
    a.import(&b.export_updates(&a.document().version()).unwrap())
        .unwrap();
    c.import(&b.export_updates(&c.document().version()).unwrap())
        .unwrap();

    assert_editor(&a, blocks![p!["x<a>new<b>y"]]);
    assert_eq!(a.selection(), c.selection());
}

#[test]
fn unrelated_remote_text_does_not_move_an_undone_caret() {
    let mut a = editor(blocks![p!["one t<a>o three"], p!["other"]]);
    let mut b = peer(a.document());
    let text = a.document().blocks()[0];
    a.insert_text("w").unwrap();
    a.set_selection(Some(Selection::Text {
        anchor: Pos::new(text, 4),
        head: Pos::new(text, 7),
    }))
    .unwrap();
    a.insert_text("2").unwrap();
    b.insert_text(b.blocks()[1], 0, "the ").unwrap();
    sync(&mut a, &mut b);

    assert!(a.undo().unwrap());
    assert_editor(&a, blocks![p!["one <a>two<b> three"], p!["the other"]]);
    assert!(a.undo().unwrap());
    assert_editor(&a, blocks![p!["one t<a>o three"], p!["the other"]]);
}

#[test]
fn a_block_selection_drops_the_blocks_a_peer_removed() {
    let mut a = editor(blocks![p!["one"], p!["two"], p!["three"]]);
    let mut b = peer(a.document());
    let all = b.blocks();
    a.set_selection(Some(Selection::Blocks(all.clone())))
        .unwrap();
    b.remove_block(all[1]).unwrap();
    sync(&mut a, &mut b);
    assert_eq!(
        a.selection(),
        Some(&Selection::Blocks(vec![all[0], all[2]]))
    );

    b.remove_block(all[0]).unwrap();
    b.remove_block(all[2]).unwrap();
    sync(&mut a, &mut b);
    assert_eq!(a.selection(), None);
}

#[test]
fn remote_changes_stay_out_of_undo() {
    let mut a = editor(blocks![p!["<a>"], p!["other"]]);
    let mut b = peer(a.document());
    let [first, other] = b.blocks()[..] else {
        panic!()
    };

    // A change elsewhere leaves the typing in one step.
    a.insert_text("a").unwrap();
    a.insert_text("b").unwrap();
    b.insert_text(other, 0, "the ").unwrap();
    sync(&mut a, &mut b);
    a.insert_text("c").unwrap();
    assert_editor(&a, blocks![p!["abc<a>"], p!["the other"]]);

    // A change to the text being typed ends the step.
    b.insert_text(first, 0, ">").unwrap();
    sync(&mut a, &mut b);
    a.insert_text("d").unwrap();
    assert_editor(&a, blocks![p![">abcd<a>"], p!["the other"]]);

    assert!(a.undo().unwrap());
    assert_editor(&a, blocks![p![">abc<a>"], p!["the other"]]);
    assert!(a.undo().unwrap());
    assert_editor(&a, blocks![p![">"], p!["the other"]]);
    assert!(!a.undo().unwrap());
    assert!(a.redo().unwrap());
    assert!(a.redo().unwrap());
    assert_editor(&a, blocks![p![">abcd<a>"], p!["the other"]]);
    assert!(!a.can_redo());

    sync(&mut a, &mut b);
    assert_document(&b, None, blocks![p![">abcd"], p!["the other"]]);
}

#[test]
fn pending_and_repeated_updates_elsewhere_keep_one_typing_step() {
    let mut a = editor(blocks![p!["<a>"], p!["other"]]);
    let mut b = peer(a.document());
    let other = b.blocks()[1];
    let base = b.version();
    b.insert_text(other, 0, "1").unwrap();
    let first = b.export_updates(&base).unwrap();
    let next = b.version();
    b.insert_text(other, 0, "2").unwrap();
    let second = b.export_updates(&next).unwrap();

    a.insert_text("a").unwrap();
    a.import(&second).unwrap();
    assert_editor(&a, blocks![p!["a<a>"], p!["other"]]);
    a.insert_text("b").unwrap();
    a.import(&first).unwrap();
    a.import(&first).unwrap();
    a.insert_text("c").unwrap();
    assert_editor(&a, blocks![p!["abc<a>"], p!["21other"]]);
    assert!(a.undo().unwrap());
    assert_editor(&a, blocks![p!["<a>"], p!["21other"]]);
    assert!(!a.can_undo());
}

#[test]
fn undo_puts_the_selection_where_remote_changes_moved_it() {
    let mut a = editor(blocks![p!["one <a>two<b> three"]]);
    let mut b = peer(a.document());
    let block = b.blocks()[0];
    a.insert_text("2").unwrap();
    b.insert_text(block, 0, "zero ").unwrap();
    sync(&mut a, &mut b);
    assert_editor(&a, blocks![p!["zero one 2<a> three"]]);

    a.undo().unwrap();
    assert_editor(&a, blocks![p!["zero one <a>two<b> three"]]);
    a.redo().unwrap();
    assert_editor(&a, blocks![p!["zero one 2<a> three"]]);
}

#[test]
fn an_edit_in_a_block_a_peer_removed_is_gone_with_it() {
    let mut a = editor(blocks![p!["keep"], p!["x<a>"]]);
    let mut b = peer(a.document());
    let removed = b.blocks()[1];
    a.insert_text("y").unwrap();
    b.remove_block(removed).unwrap();
    sync(&mut a, &mut b);
    assert_editor(&a, blocks![p!["keep"]]);
    assert_eq!(a.selection(), None);
    assert!(matches!(a.insert_text("z"), Err(Error::NoSelection)));

    while a.undo().unwrap() {}
    sync(&mut a, &mut b);
    assert_eq!(outline(a.document(), None), outline(&b, None));
    assert_eq!(a.document().blocks(), b.blocks());
}

#[test]
fn an_undo_that_loro_cannot_do_clears_the_history() {
    let mut a = editor(blocks![p!["one<a>"], p!["two", p!["child"]], p!["three"]]);
    let mut b = peer(a.document());
    let two = b.blocks()[1];
    b.merge_backward(two).unwrap();

    // The successful removal followed by undo brings a copy.
    a.apply(|doc, _| {
        doc.remove_block(two)?;
        Ok(None)
    })
    .unwrap();
    assert!(a.undo().unwrap());
    a.set_selection(Some(Selection::caret(Pos::new(
        a.document().blocks()[0],
        3,
    ))))
    .unwrap();
    a.apply(|doc, selection| {
        doc.merge_backward(doc.blocks()[2])?;
        Ok(selection.cloned())
    })
    .unwrap();
    a.import(&b.export_updates(&a.document().version()).unwrap())
        .unwrap();
    let before = outline(a.document(), None);

    assert!(matches!(a.undo(), Err(Error::Internal(_))));
    assert_eq!(outline(a.document(), None), before);
    assert!(!a.can_undo());
    assert!(!a.undo().unwrap());

    a.insert_text("!").unwrap();
    assert!(a.undo().unwrap());
    assert_eq!(outline(a.document(), None), before);
}

fn raw_text(doc: &LoroDoc, block: BlockId) -> LoroText {
    let node = TreeID::try_from(block.to_string().as_str()).unwrap();
    doc.get_tree("blocks")
        .get_meta(node)
        .unwrap()
        .get("$text")
        .unwrap()
        .into_container()
        .unwrap()
        .into_text()
        .unwrap()
}

#[test]
fn a_draft_remains_undoable_on_a_document_loaded_from_shallow_history() {
    let (base, _) = build(blocks![p!["shared"]]);
    let block = base.blocks()[0];
    let raw = LoroDoc::new();
    raw.import(&base.export_snapshot().unwrap()).unwrap();
    let snapshot = raw
        .export(ExportMode::shallow_snapshot(&raw.oplog_frontiers()))
        .unwrap();
    let mut doc = Document::new(Schema::builtin());
    doc.import(&snapshot).unwrap();
    let mut editor = Editor::new(doc);
    editor
        .apply(|doc, _| {
            doc.insert_text(block, 6, " edit")?;
            Ok(None)
        })
        .unwrap();
    assert!(editor.undo().unwrap());
    assert_editor(&editor, blocks![p!["shared"]]);
}

#[test]
fn a_partial_import_preserves_selection_and_undo_group_boundaries() {
    let (base, _) = build(blocks![p!["hello world"]]);
    let block = base.blocks()[0];
    let origin = LoroDoc::new();
    origin.import(&base.export_snapshot().unwrap()).unwrap();
    let fork = origin.fork();
    raw_text(&origin, block).insert(11, "!").unwrap();
    origin.commit();
    let snapshot = origin
        .export(ExportMode::shallow_snapshot(&origin.oplog_frontiers()))
        .unwrap();
    let mut doc = Document::new(Schema::builtin());
    doc.import(&snapshot).unwrap();
    let mut a = Editor::new(doc);
    a.set_selection(Some(Selection::caret(Pos::new(block, 12))))
        .unwrap();
    a.insert_text("a").unwrap();
    a.insert_text("b").unwrap();

    raw_text(&fork, block).insert(0, "earlier ").unwrap();
    raw_text(&origin, block).delete(0, 6).unwrap();
    origin.commit();
    origin
        .import(&fork.export(ExportMode::all_updates()).unwrap())
        .unwrap();
    let update = origin.export(ExportMode::all_updates()).unwrap();

    assert!(matches!(a.import(&update), Err(Error::InvalidUpdate(_))));
    assert_editor(&a, blocks![p!["world!ab<a>"]]);
    a.insert_text("c").unwrap();
    assert!(a.undo().unwrap());
    assert_editor(&a, blocks![p!["world!ab<a>"]]);
    assert!(a.undo().unwrap());
    assert_editor(&a, blocks![p!["world!<a>"]]);
}
