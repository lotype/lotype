//! Content this peer's schema does not describe, as another peer could send it.
//! It is planted through Loro directly because the public API rejects it.

use std::ops::Range;

use loro::TreeParentId;

use crate::document::{ID, TYPE};
use crate::test_support::{assert_document, build, outline};
use crate::{BlockId, Document, Editor, Error, MAX_DEPTH, Pos, Schema, Selection, b, blocks, p};

fn highlight(doc: &Document, block: BlockId, range: Range<usize>) {
    let text = doc.raw_text(block).unwrap();
    text.mark(range, "highlight", 0.5).unwrap();
}

/// The text of a block as Loro stores it, marks included.
fn stored(doc: &Document, block: BlockId) -> String {
    format!("{:?}", doc.raw_text(block).unwrap().to_delta())
}

#[test]
fn split_keeps_unknown_marks_and_values() {
    let (mut doc, _) = build(blocks![p!["hello world"]]);
    let block = doc.blocks()[0];
    highlight(&doc, block, 0..8);
    let meta = doc.tree.get_meta(block.0).unwrap();
    meta.insert("ratio", 1.5).unwrap();

    let tail = doc.split_block(Pos::new(block, 5)).unwrap().block;
    assert_eq!(
        stored(&doc, block),
        r#"[Insert { insert: "hello", attributes: Some({"highlight": Double(0.5)}) }]"#
    );
    assert_eq!(
        stored(&doc, tail),
        concat!(
            r#"[Insert { insert: " wo", attributes: Some({"highlight": Double(0.5)}) }, "#,
            r#"Insert { insert: "rld", attributes: None }]"#
        )
    );
    let ratio = doc.tree.get_meta(tail.0).unwrap().get("ratio");
    assert_eq!(ratio.unwrap().into_value().unwrap(), 1.5.into());
}

#[test]
fn merge_and_delete_work_around_unknown_marks() {
    let (mut doc, _) = build(blocks![p!["one"], p!["two"], p!["three"]]);
    let [one, two, three] = doc.blocks()[..] else {
        panic!()
    };
    highlight(&doc, two, 0..3);
    highlight(&doc, three, 1..4);

    let caret = doc.merge_backward(two).unwrap().unwrap();
    assert_eq!(caret, Pos::new(one, 3));
    assert_eq!(doc.text(one).unwrap(), "onetwo");

    let caret = doc.delete_range(Pos::new(one, 4), Pos::new(three, 2));
    assert_eq!(caret.unwrap(), Pos::new(one, 4));
    assert_eq!(doc.blocks(), [one]);
    assert_eq!(
        stored(&doc, one),
        concat!(
            r#"[Insert { insert: "one", attributes: None }, "#,
            r#"Insert { insert: "tre", attributes: Some({"highlight": Double(0.5)}) }, "#,
            r#"Insert { insert: "e", attributes: None }]"#
        )
    );

    doc.insert_text(one, 4, "X").unwrap();
    doc.mark(one, 0..8, "bold", true).unwrap();
    doc.unmark(one, 0..8, "bold").unwrap();
    assert_eq!(doc.text(one).unwrap(), "onetXree");
}

#[test]
fn typing_with_pending_marks_keeps_inherited_foreign_values() {
    let (mut doc, _) = build(blocks![p!["ab"]]);
    let block = doc.blocks()[0];
    highlight(&doc, block, 0..2);
    doc.commit();

    let mut editor = Editor::new(doc);
    editor
        .set_selection(Some(Selection::caret(Pos::new(block, 1))))
        .unwrap();
    editor.toggle_mark("bold").unwrap();
    editor.insert_text("x").unwrap();
    let delta = editor
        .document()
        .raw_text(block)
        .unwrap()
        .slice_delta(1, 2, loro::cursor::PosType::Unicode)
        .unwrap();
    let [
        loro::TextDelta::Insert {
            attributes: Some(attrs),
            ..
        },
    ] = delta.as_slice()
    else {
        panic!("expected one marked insert");
    };
    assert_eq!(attrs.get("highlight"), Some(&0.5.into()));
    assert_eq!(attrs.get("bold"), Some(&true.into()));
}

#[test]
fn typing_does_not_extend_a_legacy_boolean_link() {
    let (mut doc, selection) = build(blocks![p!["a<a>b"]]);
    let block = doc.blocks()[0];
    doc.raw_text(block)
        .unwrap()
        .mark(0..2, "link", true)
        .unwrap();
    doc.commit();

    let mut editor = Editor::new(doc);
    editor.set_selection(selection).unwrap();
    assert!(!editor.active_marks().unwrap().contains_key("link"));
    editor.insert_text("X").unwrap();
    let spans = editor.document().spans(block).unwrap();
    assert_eq!(spans[0].marks["link"], true.into());
    assert_eq!(spans[1].text, "X");
    assert!(spans[1].marks.is_empty());
    assert_eq!(spans[2].marks["link"], true.into());
}

#[test]
fn range_commands_skip_blocks_of_an_unknown_type() {
    let (mut doc, _) = build(blocks![p!["one"], p!["two"], p!["three"]]);
    let [one, two, three] = doc.blocks()[..] else {
        panic!()
    };
    let meta = doc.tree.get_meta(two.0).unwrap();
    meta.insert(TYPE, "embed").unwrap();
    assert!(matches!(doc.text(two), Err(Error::UnknownBlockType(name)) if name == "embed"));

    let (from, to) = (Pos::new(one, 1), Pos::new(three, 2));
    doc.toggle_mark(from, to, "bold").unwrap();
    assert_eq!(
        outline(&doc, None),
        "paragraph \"o[bold|ne]\"\nembed\nparagraph \"[bold|th]ree\"\n"
    );

    let caret = doc.merge_backward(three).unwrap().unwrap();
    assert_eq!(caret, Pos::new(three, 0));
    let expected = blocks![p!["o", b!["ne"]], p![b!["th"], "ree"]];
    assert_document(&doc, None, expected);
}

#[test]
fn blocks_nested_beyond_max_depth_are_not_part_of_the_document() {
    let (mut doc, _) = build(blocks![p!["top"], p!["after"]]);
    let [top, after] = doc.blocks()[..] else {
        panic!()
    };
    let mut chain = vec![top];
    for _ in 0..MAX_DEPTH + 2 {
        let parent = TreeParentId::Node(chain.last().unwrap().0);
        let id = doc.tree.create(parent).unwrap();
        doc.tree
            .get_meta(id)
            .unwrap()
            .insert(TYPE, "paragraph")
            .unwrap();
        chain.push(BlockId(id));
    }
    let last_visible = chain[MAX_DEPTH - 1];
    let hidden = chain[MAX_DEPTH];

    assert_eq!(doc.blocks().len(), MAX_DEPTH + 1);
    assert!(doc.contains(last_visible));
    assert!(!doc.contains(hidden));
    assert!(matches!(doc.text(hidden), Err(Error::BlockNotFound)));
    assert!(matches!(doc.parent(hidden), Err(Error::BlockNotFound)));
    assert!(doc.children(Some(last_visible)).unwrap().is_empty());
    assert_eq!(doc.next_block(last_visible).unwrap(), Some(after));
    assert_eq!(doc.prev_block(after).unwrap(), Some(last_visible));
    let raw_children = doc.child_ids(TreeParentId::Node(last_visible.0));
    assert_eq!(raw_children, [hidden.0]);

    doc.set_block_type(last_visible, "heading").unwrap();
    doc.remove_block(chain[1]).unwrap();
    assert_eq!(doc.blocks(), [top, after]);
}

fn claim(doc: &Document, block: BlockId, id: BlockId) {
    let meta = doc.tree.get_meta(block.0).unwrap();
    meta.insert(ID, id.0.to_string()).unwrap();
}

#[test]
fn a_block_only_takes_a_removed_id_that_nothing_else_claims() {
    let (mut doc, _) = build(blocks![
        p!["a"],
        p!["b"],
        p!["c"],
        p!["d"],
        p!["x"],
        p!["y"]
    ]);
    let [a, b, c, d, x, y] = doc.blocks()[..] else {
        panic!()
    };
    doc.remove_block(x).unwrap();
    doc.remove_block(y).unwrap();
    claim(&doc, a, b);
    claim(&doc, b, x);
    claim(&doc, c, x);
    claim(&doc, d, y);
    doc.refresh_ids();
    assert_eq!(doc.blocks(), [a, b, c, y]);

    // An id no node has had yet.
    let unborn = loro::TreeID::new(d.0.peer, d.0.counter + 1000);
    claim(&doc, a, BlockId(unborn));
    doc.refresh_ids();
    assert_eq!(doc.blocks(), [a, b, c, y]);
    assert_eq!(doc.text(y).unwrap(), "d");
    assert_eq!(doc.next_block(c).unwrap(), Some(y));
    assert!(matches!(doc.text(d), Err(Error::BlockNotFound)));
    assert!(matches!(doc.text(x), Err(Error::BlockNotFound)));
}

#[test]
fn moving_a_previously_hidden_claimant_agrees_with_a_peer() {
    let (mut source, _) = build(blocks![p!["root"], p!["gone"]]);
    let [root, gone] = source.blocks()[..] else {
        panic!()
    };
    source.remove_block(gone).unwrap();
    let mut parent = root;
    let mut first_child = None;
    for _ in 0..MAX_DEPTH {
        parent = BlockId(source.tree.create(TreeParentId::Node(parent.0)).unwrap());
        first_child.get_or_insert(parent);
    }
    claim(&source, parent, gone);

    let snapshot = source.export_snapshot().unwrap();
    let mut a = Document::new(Schema::builtin());
    let mut b = Document::new(Schema::builtin());
    a.import(&snapshot).unwrap();
    b.import(&snapshot).unwrap();
    assert!(!a.contains(gone));

    a.move_block(first_child.unwrap(), None, 0).unwrap();
    b.import(&a.export_updates(&b.version()).unwrap()).unwrap();
    assert!(a.contains(gone));
    assert_eq!(a.blocks(), b.blocks());
}

#[test]
fn a_block_selection_removes_every_claimant_even_if_one_would_gain_the_id() {
    let (mut doc, _) = build(blocks![p!["first"], p!["second"], p!["gone"]]);
    let [first, second, gone] = doc.blocks()[..] else {
        panic!()
    };
    doc.remove_block(gone).unwrap();
    claim(&doc, first, gone);
    claim(&doc, second, gone);
    let mut editor = Editor::new(doc);
    editor
        .set_selection(Some(Selection::Blocks(vec![first, second])))
        .unwrap();
    editor.delete_selection().unwrap();
    assert!(editor.document().blocks().is_empty());
}

#[test]
fn a_range_command_returns_the_caret_under_the_survivors_new_id() {
    let (mut doc, _) = build(blocks![p!["first"], p!["second"], p!["gone"]]);
    let [first, second, gone] = doc.blocks()[..] else {
        panic!()
    };
    doc.remove_block(gone).unwrap();
    claim(&doc, first, gone);
    claim(&doc, second, gone);
    doc.refresh_ids();

    let caret = doc
        .delete_range(Pos::new(first, 5), Pos::new(second, 6))
        .unwrap();
    assert_eq!(caret, Pos::new(gone, 5));
    assert_eq!(doc.blocks(), [gone]);
}

#[test]
fn structural_deletion_preserves_a_concurrently_hidden_child() {
    for merge in [false, true] {
        let (mut a, _) = build(blocks![p!["root"]]);
        let mut parent = a.blocks()[0];
        for _ in 1..MAX_DEPTH - 2 {
            parent = a.insert_block(Some(parent), 0, "paragraph").unwrap();
        }
        let first = a.insert_block(Some(parent), 0, "paragraph").unwrap();
        let last = a.insert_block(Some(parent), 1, "paragraph").unwrap();
        let mut b = Document::new(Schema::builtin());
        b.import(&a.export_snapshot().unwrap()).unwrap();

        let child = b.insert_block(Some(last), 0, "paragraph").unwrap();
        b.insert_text(child, 0, "remote").unwrap();
        a.move_block(last, Some(first), 0).unwrap();
        a.import(&b.export_updates(&a.version()).unwrap()).unwrap();
        assert!(!a.contains(child));

        if merge {
            a.merge_backward(last).unwrap();
        } else {
            a.delete_range(Pos::new(first, 0), Pos::new(last, 0))
                .unwrap();
        }
        assert_eq!(a.parent(child).unwrap(), Some(first));
        assert_eq!(a.text(child).unwrap(), "remote");
    }
}

#[test]
fn importing_operations_for_the_local_peer_keeps_them_out_of_undo() {
    let (doc, selection) = build(blocks![p!["x<a>"], p!["other"]]);
    let other = doc.blocks()[1];
    let local_peer = doc.doc.peer_id();
    let mut editor = Editor::new(doc);
    editor.set_selection(selection).unwrap();
    editor.insert_text("y").unwrap();

    let update = {
        let mut replica = Document::new(Schema::builtin());
        replica
            .import(&editor.document().export_snapshot().unwrap())
            .unwrap();
        replica.doc.set_peer_id(local_peer).unwrap();
        replica.insert_text(other, 0, "the ").unwrap();
        replica
            .export_updates(&editor.document().version())
            .unwrap()
    };
    editor.import(&update).unwrap();

    assert!(!editor.can_undo());
    editor.insert_text("z").unwrap();
    assert!(editor.undo().unwrap());
    assert_document(editor.document(), None, blocks![p!["xy"], p!["the other"]]);
}

#[test]
fn a_replacement_document_is_addressed_by_its_restored_ids() {
    let (mut replacement, _) = build(blocks![p!["a"], p!["gone"]]);
    let [a, gone] = replacement.blocks()[..] else {
        panic!()
    };
    replacement.remove_block(gone).unwrap();
    claim(&replacement, a, gone);
    replacement.commit();

    let original = build(blocks![p!["old"]]).0;
    let original_ids = original.blocks();
    let mut editor = Editor::new(original);
    let result = editor.apply(|doc, _| {
        *doc = replacement;
        Ok(Some(Selection::caret(Pos::new(a, 0))))
    });
    assert!(matches!(result, Err(Error::BlockNotFound)));
    assert_eq!(editor.selection(), None);
    assert_eq!(editor.document().blocks(), original_ids);
}
