use loro::{ExportMode, LoroDoc, TreeParentId};
use lotype_core::test_support::{assert_editor, build, editor, outline};
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

/// Sends each editor what the other has and it lacks.
fn exchange(a: &mut Editor, b: &mut Editor) {
    let for_a = b.document().export_updates(&a.document().version());
    let for_b = a.document().export_updates(&b.document().version());
    a.import(&for_a.unwrap()).unwrap();
    b.import(&for_b.unwrap()).unwrap();
}

#[test]
fn undoing_a_removal_keeps_what_a_peer_did_to_the_block_meanwhile() {
    let mut a = editor(blocks![p!["keep"], p!["hello", p!["child"]]]);
    let mut b = Editor::new(peer(a.document()));
    let [_, hello, child] = a.document().blocks()[..] else {
        panic!()
    };
    b.apply(|doc, _| {
        doc.insert_text(hello, 5, " there")?;
        doc.insert_text(child, 0, "grand")?;
        doc.insert_block(Some(hello), 1, "paragraph").map(|_| None)
    })
    .unwrap();
    a.set_selection(Some(Selection::Blocks(vec![hello])))
        .unwrap();
    a.delete_selection().unwrap();
    exchange(&mut a, &mut b);
    assert_editor(&b, blocks![p!["keep"]]);

    assert!(a.undo().unwrap());
    exchange(&mut a, &mut b);
    let expected = blocks![p!["keep"], p!["hello there", p!["grandchild"], p![]]];
    assert_editor(&a, expected.clone());
    assert_editor(&b, expected);
    assert_eq!(
        a.document().blocks()[..3],
        [a.document().blocks()[0], hello, child]
    );
    assert_eq!(a.document().blocks(), b.document().blocks());
}

#[test]
fn two_peers_that_remove_and_restore_a_block_get_it_back_once() {
    let mut a = editor(blocks![p!["keep"], p!["hello"]]);
    let mut b = Editor::new(peer(a.document()));
    let hello = a.document().blocks()[1];
    for editor in [&mut a, &mut b] {
        editor
            .set_selection(Some(Selection::Blocks(vec![hello])))
            .unwrap();
        editor.delete_selection().unwrap();
    }
    exchange(&mut a, &mut b);
    assert!(a.undo().unwrap());
    assert!(b.undo().unwrap());
    exchange(&mut a, &mut b);

    assert_editor(&a, blocks![p!["keep"], p!["hello"]]);
    assert_eq!(a.document().blocks()[1], hello);
    assert_eq!(a.document().blocks(), b.document().blocks());
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
fn older_steps_undo_correctly_after_a_peer_edits_a_block_that_came_back() {
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

    assert!(a.undo().unwrap());
    assert_eq!(a.document().text(hello).unwrap(), ">>hello");
    assert!(a.redo().unwrap());
    assert_eq!(a.document().text(hello).unwrap(), ">>hello!");
    assert!(a.redo().unwrap());
    assert_eq!(a.document().blocks().len(), 1);
    assert!(a.undo().unwrap());
    assert_eq!(a.document().text(hello).unwrap(), ">>hello!");
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
fn later_steps_redo_correctly_after_a_peer_edits_a_block_redo_brought_back() {
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
    assert_eq!(a.document().blocks().len(), 1);
    assert!(a.redo().unwrap());
    assert!(a.redo().unwrap());
    assert_eq!(a.document().blocks()[1], block);
    let mut b = peer(a.document());
    b.insert_text(block, 0, ">>").unwrap();
    pull(&mut a, &b);

    assert!(a.redo().unwrap());
    assert_eq!(a.document().text(block).unwrap(), ">>helo");
    for _ in 0..3 {
        assert!(a.undo().unwrap());
    }
    assert_eq!(a.document().blocks().len(), 1);
    assert!(!a.can_undo());
}

#[test]
fn undo_restores_a_caret_whose_character_a_later_keystroke_removed() {
    let mut a = editor(blocks![p!["a🇦x<a>🇧z"]]);
    let mut b = peer(a.document());
    // Without the `x` the two halves are one flag, which goes as a whole.
    a.delete_backward().unwrap();
    a.delete_backward().unwrap();
    assert_editor(&a, blocks![p!["a<a>z"]]);
    b.insert_text(b.blocks()[0], 0, ">").unwrap();
    pull(&mut a, &b);

    assert!(a.undo().unwrap());
    assert_editor(&a, blocks![p![">a🇦x<a>🇧z"]]);
    assert!(!a.can_undo());
}

#[test]
fn a_selection_that_returns_follows_the_text_of_its_new_block() {
    let mut a = editor(blocks![p!["keep"], p!["ca<a>ret"]]);
    let mut b = Editor::new(peer(a.document()));
    let block = b.document().blocks()[1];
    b.set_selection(Some(Selection::Blocks(vec![block])))
        .unwrap();
    b.delete_selection().unwrap();
    pull(&mut a, b.document());
    b.undo().unwrap();
    pull(&mut a, b.document());
    b.apply(|doc, _| {
        doc.insert_text(block, 0, ">>")?;
        Ok(None)
    })
    .unwrap();
    pull(&mut a, b.document());

    assert_editor(&a, blocks![p!["keep"], p![">>ca<a>ret"]]);
}

#[test]
fn typing_over_blocks_makes_a_missing_trash_node_outside_the_step() {
    let mut a = editor(blocks![p!["one"], p!["two"], p!["three"], p!["four"]]);
    let [one, _, three, four] = a.document().blocks()[..] else {
        panic!()
    };
    // A peer takes the marker off the trash node.
    let hostile = LoroDoc::new();
    let snapshot = a.document().export_snapshot().unwrap();
    hostile.import(&snapshot).unwrap();
    let tree = hostile.get_tree("blocks");
    for node in tree.children(TreeParentId::Root).unwrap() {
        tree.get_meta(node).unwrap().delete("$trash").unwrap();
    }
    hostile.commit();
    let update = hostile.export(ExportMode::all_updates()).unwrap();
    a.import(&update).unwrap();

    let (anchor, head) = (Pos::new(one, 1), Pos::new(three, 1));
    a.set_selection(Some(Selection::Text { anchor, head }))
        .unwrap();
    a.insert_text("X").unwrap();
    let mut b = Editor::new(peer(a.document()));
    b.set_selection(Some(Selection::Blocks(vec![four])))
        .unwrap();
    b.delete_selection().unwrap();
    assert!(a.undo().unwrap());
    exchange(&mut a, &mut b);

    // What the peer removed went where undo did not reach.
    assert!(b.undo().unwrap());
    assert!(b.document().contains(four));
    assert!(a.redo().unwrap());
    exchange(&mut a, &mut b);
    assert_eq!(a.document().blocks(), b.document().blocks());
    assert_eq!(a.document().blocks(), peer(a.document()).blocks());
}

#[test]
fn a_remote_insert_before_the_caret_keeps_pending_marks() {
    let (doc, sel) = build(blocks![p!["hello <a>"]]);
    let mut ed = Editor::new(doc);
    ed.set_selection(sel).unwrap();
    ed.toggle_mark("bold").unwrap();
    let mut b = peer(ed.document());
    b.insert_text(b.blocks()[0], 0, "x").unwrap();
    pull(&mut ed, &b);
    assert!(ed.active_marks().unwrap().contains_key("bold"));
}
