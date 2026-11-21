//! Content this peer's schema does not describe, as another peer could send it.
//! It is planted through Loro directly because the public API rejects it.

use std::ops::Range;

use loro::TreeParentId;

use crate::document::TYPE;
use crate::test_support::{assert_document, build, outline};
use crate::{BlockId, Document, Editor, Error, MAX_DEPTH, Pos, Selection, b, blocks, p};

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
