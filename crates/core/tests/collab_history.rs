use lotype_core::test_support::{assert_editor, editor, outline};
use lotype_core::{Document, Editor, Schema, blocks, p};

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
