use loro::{ExportMode, LoroDoc, TreeID, TreeParentId};
use lotype_core::test_support::{assert_editor, editor, outline};
use lotype_core::{Document, Editor, Pos, Schema, Selection, blocks, p};

/// Another peer that starts from `doc` as it is now.
fn peer(doc: &Document) -> Document {
    let mut peer = Document::new(Schema::builtin());
    peer.import(&doc.export_snapshot().unwrap()).unwrap();
    peer
}

/// Brings `a` what `b` has and it lacks.
fn pull(a: &mut Editor, b: &Document) {
    let update = b.export_updates(&a.document().version()).unwrap();
    a.import(&update).unwrap();
}

#[test]
fn undo_keeps_a_range_start_where_a_peer_removed_the_character_before_it() {
    let mut a = editor(blocks![p!["ab<a>cd<b>e"]]);
    let mut b = peer(a.document());
    a.insert_text("Z").unwrap();
    b.delete_text(b.blocks()[0], 1..2).unwrap();
    pull(&mut a, &b);

    assert!(a.undo().unwrap());
    assert_editor(&a, blocks![p!["a<a>cd<b>e"]]);
}

#[test]
fn delivery_boundaries_do_not_change_the_caret_undo_restores() {
    let carets = [true, false].map(|split| {
        let mut a = editor(blocks![p!["xl<a>d"]]);
        let mut b = peer(a.document());
        let block = b.blocks()[0];
        a.insert_text("Z").unwrap();
        b.insert_text(block, 1, "ab").unwrap();
        if split {
            pull(&mut a, &b);
        }
        b.delete_text(block, 3..5).unwrap();
        pull(&mut a, &b);
        assert!(a.undo().unwrap());
        outline(a.document(), a.selection())
    });
    assert_eq!(carets[0], carets[1]);
}

#[test]
fn a_peers_edit_to_a_restored_block_drops_the_undo_steps_it_would_corrupt() {
    let mut a = editor(blocks![p!["keep"], p!["hello<a>"]]);
    let hello = a.document().blocks()[1];
    a.insert_text("!").unwrap();
    a.set_selection(Some(Selection::Blocks(vec![hello])))
        .unwrap();
    a.delete_selection().unwrap();
    assert!(a.undo().unwrap());
    let mut b = peer(a.document());
    b.insert_text(hello, 0, ">>").unwrap();
    pull(&mut a, &b);

    // Loro would take the `l` out here and leave the `!`.
    assert!(!a.undo().unwrap());
    assert_eq!(a.document().text(hello).unwrap(), ">>hello!");
    assert!(a.redo().unwrap());
    assert_eq!(a.document().blocks().len(), 1);
}

#[test]
fn delivery_boundaries_do_not_decide_whether_a_selection_survives() {
    for blocks_selected in [false, true] {
        let selections = [true, false].map(|split| {
            let mut a = editor(blocks![p!["keep"], p!["ca<a>ret"]]);
            let mut b = Editor::new(peer(a.document()));
            let block = b.document().blocks()[1];
            if blocks_selected {
                a.set_selection(Some(Selection::Blocks(vec![block])))
                    .unwrap();
            }
            b.set_selection(Some(Selection::Blocks(vec![block])))
                .unwrap();
            b.delete_selection().unwrap();
            if split {
                pull(&mut a, b.document());
                assert_eq!(a.selection(), None);
            }
            b.undo().unwrap();
            pull(&mut a, b.document());
            assert!(a.selection().is_some());
            outline(a.document(), a.selection())
        });
        assert_eq!(selections[0], selections[1]);
    }
}

#[test]
fn a_restored_block_that_lost_its_id_still_drops_the_undo_steps() {
    let mut a = editor(blocks![p!["keep"], p!["hello<a>"]]);
    let [keep, hello] = a.document().blocks()[..] else {
        panic!()
    };
    a.insert_text("!").unwrap();
    a.set_selection(Some(Selection::Blocks(vec![hello])))
        .unwrap();
    a.delete_selection().unwrap();
    assert!(a.undo().unwrap());

    // A peer has another block claim the id too, so that neither gets it.
    let raw = LoroDoc::new();
    raw.import(&a.document().export_snapshot().unwrap())
        .unwrap();
    let tree = raw.get_tree("blocks");
    let keep_node = TreeID::try_from(keep.to_string().as_str()).unwrap();
    let meta = tree.get_meta(keep_node).unwrap();
    meta.insert("$id", hello.to_string()).unwrap();
    let restored = tree.children(TreeParentId::Root).unwrap()[1];
    let text = tree.get_meta(restored).unwrap().get("$text").unwrap();
    let text = text.into_container().unwrap().into_text().unwrap();
    text.insert(0, ">>").unwrap();
    raw.commit();
    a.import(&raw.export(ExportMode::all_updates()).unwrap())
        .unwrap();

    let restored = a.document().blocks()[1];
    assert_ne!(restored, hello);
    assert!(!a.undo().unwrap());
    assert_eq!(a.document().text(restored).unwrap(), ">>hello!");
}

#[test]
fn a_peers_edit_to_a_block_redo_brought_back_drops_the_redo_steps() {
    let mut a = editor(blocks![p!["keep<a>"]]);
    a.apply(|doc, _| {
        let block = doc.insert_block(None, 1, "paragraph")?;
        Ok(Some(Selection::caret(Pos::new(block, 0))))
    })
    .unwrap();
    let block = a.document().blocks()[1];
    a.insert_text("hello").unwrap();
    a.apply(|doc, _| {
        doc.delete_text(block, 2..3)?;
        Ok(Some(Selection::caret(Pos::new(block, 4))))
    })
    .unwrap();
    for _ in 0..3 {
        assert!(a.undo().unwrap());
    }
    assert!(a.redo().unwrap());
    assert!(a.redo().unwrap());
    let mut b = peer(a.document());
    b.insert_text(block, 0, ">>").unwrap();
    pull(&mut a, &b);

    // Loro would take the `h` out here.
    assert!(!a.redo().unwrap());
    assert_eq!(a.document().text(block).unwrap(), ">>hello");
    assert!(a.undo().unwrap());
    assert_eq!(a.document().text(block).unwrap(), ">>");
}
