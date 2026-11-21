use std::collections::HashSet;

use loro::{ExportMode, LoroDoc, TreeID};
use lotype_core::test_support::{build, editor};
use lotype_core::{BlockId, Changes, Document, Editor, Error, Schema, Selection, blocks, p};

fn only(blocks: &[BlockId]) -> Changes {
    Changes {
        blocks: blocks.iter().copied().collect(),
        ..Changes::default()
    }
}

fn structure(blocks: &[BlockId]) -> Changes {
    Changes {
        structure: true,
        ..only(blocks)
    }
}

/// Another peer that starts from `doc` as it is now.
fn peer(doc: &Document) -> Document {
    let mut peer = Document::new(Schema::builtin());
    peer.import(&doc.export_snapshot().unwrap()).unwrap();
    peer
}

fn pull(a: &mut Editor, b: &Document) {
    let update = b.export_updates(&a.document().version()).unwrap();
    a.import(&update).unwrap();
}

#[test]
fn a_new_editor_has_no_changes_and_taking_them_empties_them() {
    let mut ed = editor(blocks![p!["a<a>"], p!["b"]]);
    assert!(ed.take_changes().is_empty());
    ed.insert_text("x").unwrap();
    assert!(!ed.take_changes().is_empty());
    assert!(ed.take_changes().is_empty());
}

#[test]
fn text_and_marks_report_their_block_only() {
    let mut ed = editor(blocks![p!["a<a>bc<b>"], p!["other"]]);
    let block = ed.document().blocks()[0];
    ed.toggle_mark("bold").unwrap();
    assert_eq!(ed.take_changes(), only(&[block]));
    ed.insert_text("x").unwrap();
    ed.insert_text("y").unwrap();
    ed.delete_backward().unwrap();
    assert_eq!(ed.take_changes(), only(&[block]));
    // Setting the selection, or a mark for the next typed text, changes no block.
    ed.toggle_mark("italic").unwrap();
    ed.set_selection(None).unwrap();
    assert!(ed.take_changes().is_empty());
}

#[test]
fn type_and_attributes_report_their_block() {
    let mut ed = editor(blocks![p!["a<a>"], p!["other"]]);
    let block = ed.document().blocks()[0];
    ed.apply(|doc, selection| {
        doc.set_block_type(block, "heading")?;
        doc.set_attr(block, "level", 2)?;
        Ok(selection.cloned())
    })
    .unwrap();
    assert_eq!(ed.take_changes(), only(&[block]));
}

#[test]
fn structural_edits_report_the_structure_and_the_blocks_still_there() {
    let mut ed = editor(blocks![p!["ab<a>cd"], p!["other"]]);
    let [first, other] = ed.document().blocks()[..] else {
        panic!()
    };
    ed.split_block().unwrap();
    let second = ed.document().blocks()[1];
    assert_eq!(ed.take_changes(), structure(&[first, second]));

    ed.join_backward().unwrap();
    assert_eq!(ed.take_changes(), structure(&[first]));

    ed.set_selection(Some(Selection::Blocks(vec![other])))
        .unwrap();
    ed.delete_selection().unwrap();
    assert_eq!(ed.take_changes(), structure(&[]));

    ed.apply(|doc, selection| {
        doc.insert_block(None, 0, "paragraph")?;
        Ok(selection.cloned())
    })
    .unwrap();
    let new = ed.document().blocks()[0];
    assert_eq!(ed.take_changes(), structure(&[new]));
    ed.apply(|doc, selection| {
        doc.move_block(new, None, 1)?;
        Ok(selection.cloned())
    })
    .unwrap();
    assert_eq!(ed.take_changes(), structure(&[]));
}

#[test]
fn undo_and_redo_report_what_they_changed() {
    let mut ed = editor(blocks![p!["ab<a>cd"]]);
    let first = ed.document().blocks()[0];
    ed.split_block().unwrap();
    let second = ed.document().blocks()[1];
    ed.take_changes();

    assert!(ed.undo().unwrap());
    assert_eq!(ed.take_changes(), structure(&[first]));
    assert!(ed.redo().unwrap());
    assert_eq!(ed.take_changes(), structure(&[first, second]));
}

#[test]
fn a_rejected_edit_reports_nothing() {
    let mut ed = editor(blocks![p!["a<a>"]]);
    let result = ed.apply(|doc, _| {
        let block = doc.blocks()[0];
        doc.insert_text(block, 0, "draft")?;
        doc.remove_block(block)?;
        doc.insert_block(None, 0, "no such type").map(|_| None)
    });
    assert!(matches!(result, Err(Error::UnknownBlockType(_))));
    assert!(ed.take_changes().is_empty());
}

#[test]
fn imports_report_what_peers_changed() {
    let mut a = editor(blocks![p!["one<a>"], p!["two"]]);
    let mut b = peer(a.document());
    let [one, two] = b.blocks()[..] else { panic!() };

    b.insert_text(two, 0, "remote ").unwrap();
    pull(&mut a, &b);
    assert_eq!(a.take_changes(), only(&[two]));

    let new = b.insert_block(Some(one), 0, "paragraph").unwrap();
    b.insert_text(new, 0, "child").unwrap();
    pull(&mut a, &b);
    assert_eq!(a.take_changes(), structure(&[new]));

    b.remove_block(one).unwrap();
    pull(&mut a, &b);
    assert_eq!(a.take_changes(), structure(&[]));
}

#[test]
fn a_change_to_a_removed_block_is_not_reported() {
    let mut a = editor(blocks![p!["keep<a>"], p!["gone"]]);
    let mut b = peer(a.document());
    let gone = b.blocks()[1];
    a.set_selection(Some(Selection::Blocks(vec![gone])))
        .unwrap();
    a.delete_selection().unwrap();
    a.take_changes();

    b.insert_text(gone, 0, "still typing in ").unwrap();
    pull(&mut a, &b);
    assert!(a.take_changes().is_empty());
}

#[test]
fn a_block_that_returns_is_reported_as_structure_only() {
    let mut a = editor(blocks![p!["keep<a>"], p!["gone"]]);
    let mut b = peer(a.document());
    let gone = b.blocks()[1];
    a.set_selection(Some(Selection::Blocks(vec![gone])))
        .unwrap();
    a.delete_selection().unwrap();
    b.insert_text(gone, 0, "changed while ").unwrap();
    pull(&mut a, &b);
    a.take_changes();

    assert!(a.undo().unwrap());
    assert_eq!(a.take_changes(), structure(&[]));
    assert_eq!(a.document().text(gone).unwrap(), "changed while gone");
}

#[test]
fn a_peer_that_marks_a_block_as_trash_changes_the_structure() {
    let mut a = editor(blocks![p!["keep<a>"], p!["parent", p!["child"]]]);
    let parent = a.document().blocks()[1];
    let raw = LoroDoc::new();
    let snapshot = a.document().export_snapshot().unwrap();
    raw.import(&snapshot).unwrap();
    let node = TreeID::try_from(parent.to_string().as_str()).unwrap();
    let meta = raw.get_tree("blocks").get_meta(node).unwrap();
    for marked in [true, false] {
        if marked {
            meta.insert("$trash", true).unwrap();
        } else {
            meta.delete("$trash").unwrap();
        }
        raw.commit();
        let update = raw.export(ExportMode::all_updates()).unwrap();
        a.import(&update).unwrap();
        assert_eq!(a.document().blocks().len(), if marked { 1 } else { 3 });
        assert!(a.take_changes().structure);
    }
}

#[test]
fn replacing_the_document_says_so_and_goes_on_from_the_new_one() {
    let mut ed = editor(blocks![p!["old<a>"]]);
    ed.insert_text("!").unwrap();
    let (replacement, _) = build(blocks![p!["new"]]);
    let block = replacement.blocks()[0];
    ed.apply(move |doc, _| {
        *doc = replacement;
        Ok(None)
    })
    .unwrap();
    let changes = ed.take_changes();
    assert!(changes.replaced);
    assert_eq!(changes.blocks, HashSet::new());

    ed.apply(|doc, _| doc.insert_text(block, 3, "er").map(|()| None))
        .unwrap();
    assert_eq!(ed.take_changes(), only(&[block]));
}
