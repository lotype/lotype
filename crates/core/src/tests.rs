//! Content this peer's schema does not describe, as another peer could send it.
//! It is planted through Loro directly because the public API rejects it.

use std::ops::Range;

use loro::TreeParentId;

use crate::document::TYPE;
use crate::test_support::{assert_document, build, outline};
use crate::trash::TRASH;
use crate::{BlockId, Document, Editor, Error, MAX_DEPTH, Pos, Schema, Selection, blocks, p};

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

    // Nothing joins over it, and it stays: it may hold what only a peer reads.
    assert_eq!(doc.merge_backward(three).unwrap(), None);
    assert_eq!(
        outline(&doc, None),
        "paragraph \"o[bold|ne]\"\nembed\nparagraph \"[bold|th]ree\"\n"
    );
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
        chain.push(BlockId(id, None));
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

    doc.set_block_type(last_visible, "code_block").unwrap();
    doc.remove_block(chain[1]).unwrap();
    assert_eq!(doc.blocks(), [top, after]);
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
fn a_removed_block_is_kept_out_of_sight() {
    let (mut doc, _) = build(blocks![p!["a"], p!["b", p!["child"]], p!["c"]]);
    let [a, b, child, c] = doc.blocks()[..] else {
        panic!()
    };
    doc.remove_block(b).unwrap();
    assert_eq!(doc.blocks(), [a, c]);
    assert_eq!(doc.children(None).unwrap(), [a, c]);
    assert_eq!(doc.index_of(c).unwrap(), 1);
    assert_eq!(doc.next_block(a).unwrap(), Some(c));
    for gone in [b, child] {
        assert!(!doc.contains(gone));
        assert!(matches!(doc.text(gone), Err(Error::BlockNotFound)));
        assert!(matches!(doc.remove_block(gone), Err(Error::BlockNotFound)));
    }
    // Nothing was deleted: the block and what it held are still shared data.
    let trash = doc.top_trash().unwrap();
    assert_eq!(doc.tree.parent(b.0), Some(TreeParentId::Node(trash)));
    assert_eq!(doc.tree.parent(child.0), Some(TreeParentId::Node(b.0)));

    // The trash node is a top-level node too, which no index counts.
    for index in 0..=2 {
        let new = doc.insert_block(None, index, "paragraph").unwrap();
        assert_eq!(doc.index_of(new).unwrap(), index);
        doc.move_block(new, None, 2 - index).unwrap();
        assert_eq!(doc.index_of(new).unwrap(), 2 - index);
        doc.remove_block(new).unwrap();
    }
    assert_eq!(doc.blocks(), [a, c]);
}

/// Two documents that each made a trash node before they met, so that the
/// second one's lies among the blocks of the first.
fn with_two_trash_nodes() -> (Document, Document) {
    let (mut one, _) = build(blocks![p!["a"], p!["b"]]);
    let (mut two, _) = build(blocks![p!["c"]]);
    let (for_one, for_two) = (
        two.export_snapshot().unwrap(),
        one.export_snapshot().unwrap(),
    );
    one.import(&for_one).unwrap();
    two.import(&for_two).unwrap();
    (one, two)
}

#[test]
fn indices_do_not_count_the_trash_nodes() {
    let (mut one, mut two) = with_two_trash_nodes();
    assert_eq!(one.trash.len(), 2);
    assert_eq!(one.blocks().len(), 3);
    assert_eq!(one.blocks(), two.blocks());

    for index in 0..=3 {
        let new = one.insert_block(None, index, "paragraph").unwrap();
        assert_eq!(one.index_of(new).unwrap(), index);
        assert_eq!(one.children(None).unwrap().len(), 4);
        one.remove_block(new).unwrap();
    }
    let out_of_bounds = one.insert_block(None, 4, "paragraph");
    let expected = Error::OutOfBounds { index: 4, len: 3 };
    assert_eq!(out_of_bounds.unwrap_err().to_string(), expected.to_string());

    let blocks = one.blocks();
    for (from, to) in [(0, 2), (2, 0), (1, 1), (2, 1)] {
        let moved = one.blocks()[from];
        one.move_block(moved, None, to).unwrap();
        assert_eq!(one.index_of(moved).unwrap(), to);
    }
    assert_eq!(one.blocks().len(), blocks.len());

    two.import(&one.export_updates(&two.version()).unwrap())
        .unwrap();
    assert_eq!(one.blocks(), two.blocks());
}

#[test]
fn peers_agree_on_a_trash_node_that_one_of_them_tampered_with() {
    for unmark in [false, true] {
        let (mut source, _) = build(blocks![p!["a"], p!["gone"]]);
        let [a, gone] = source.blocks()[..] else {
            panic!()
        };
        source.remove_block(gone).unwrap();
        let mut early = Document::new(Schema::builtin());
        early.import(&source.export_snapshot().unwrap()).unwrap();

        let trash = source.top_trash().unwrap();
        if unmark {
            let meta = source.tree.get_meta(trash).unwrap();
            meta.delete(TRASH).unwrap();
        } else {
            source.tree.mov(trash, TreeParentId::Node(a.0)).unwrap();
        }
        source.commit();
        early
            .import(&source.export_updates(&early.version()).unwrap())
            .unwrap();
        let mut late = Document::new(Schema::builtin());
        late.import(&source.export_snapshot().unwrap()).unwrap();

        assert_eq!(early.blocks(), late.blocks());
        // Without its marker the node is a block like any other, and what
        // was in it is back. Moved, it still hides what is in it.
        assert_eq!(early.contains(gone), unmark);
        assert_eq!(early.children(Some(a)).unwrap(), []);
        let again = early.insert_block(None, 1, "paragraph").unwrap();
        early.remove_block(again).unwrap();
        late.import(&early.export_updates(&late.version()).unwrap())
            .unwrap();
        assert_eq!(early.blocks(), late.blocks());
    }
}
