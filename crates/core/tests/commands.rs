use lotype_core::test_support::{BlockNode, assert_document, block, build};
use lotype_core::{Document, Error, Pos, Selection, b, blocks, bullet, divider, i, p};

/// Builds a document and returns it with the marked anchor and head.
fn setup(blocks: Vec<BlockNode>) -> (Document, Pos, Pos) {
    match build(blocks) {
        (doc, Some(Selection::Text { anchor, head })) => (doc, anchor, head),
        _ => panic!("the document needs an `<a>` marker"),
    }
}

#[track_caller]
fn assert_caret(doc: &Document, caret: Pos, expected: Vec<BlockNode>) {
    assert_document(doc, Some(&Selection::caret(caret)), expected);
}

#[test]
fn split_moves_the_tail_with_its_marks() {
    let (mut doc, at, _) = setup(blocks![p!["He", b!["l<a>lo"], " world"]]);
    let caret = doc.split_block(at).unwrap();
    assert_caret(
        &doc,
        caret,
        blocks![p!["He", b!["l"]], p![b!["<a>lo"], " world"]],
    );
}

#[test]
fn split_at_the_edges_leaves_an_empty_block() {
    let (mut doc, at, _) = setup(blocks![p!["<a>ab"]]);
    let caret = doc.split_block(at).unwrap();
    assert_caret(&doc, caret, blocks![p![], p!["<a>ab"]]);

    let (mut doc, at, _) = setup(blocks![p![b!["ab<a>"]], p!["next"]]);
    let caret = doc.split_block(at).unwrap();
    assert_caret(&doc, caret, blocks![p![b!["ab"]], p!["<a>"], p!["next"]]);
    doc.insert_text(caret.block, 0, "x").unwrap();
    assert_document(&doc, None, blocks![p![b!["ab"]], p!["x"], p!["next"]]);
}

#[test]
fn split_keeps_type_and_attributes_and_hands_children_to_the_tail() {
    let (mut doc, at, _) = setup(blocks![
        bullet!["one<a>two", bullet!["child a"], bullet!["child b"]].attr("color", "red"),
        p!["after"],
    ]);
    let caret = doc.split_block(at).unwrap();
    assert_caret(
        &doc,
        caret,
        blocks![
            bullet!["one"].attr("color", "red"),
            bullet!["<a>two", bullet!["child a"], bullet!["child b"]].attr("color", "red"),
            p!["after"],
        ],
    );
}

#[test]
fn split_inside_a_nested_block_stays_nested() {
    let (mut doc, at, _) = setup(blocks![p!["parent", p!["ab<a>cd"], p!["sibling"]]]);
    let caret = doc.split_block(at).unwrap();
    assert_caret(
        &doc,
        caret,
        blocks![p!["parent", p!["ab"], p!["<a>cd"], p!["sibling"]]],
    );
}

#[test]
fn split_rejects_invalid_positions() {
    let (mut doc, _) = build(blocks![divider!(), p!["é"]]);
    let [divider, paragraph] = doc.blocks()[..] else {
        panic!()
    };
    assert!(matches!(
        doc.split_block(Pos::new(divider, 0)),
        Err(Error::NoInlineContent)
    ));
    assert!(matches!(
        doc.split_block(Pos::new(paragraph, 1)),
        Err(Error::NotCharBoundary(1))
    ));
    assert_document(&doc, None, blocks![divider!(), p!["é"]]);
}

#[test]
fn merge_joins_text_and_keeps_each_side_marks() {
    let (mut doc, at, _) = setup(blocks![p!["a", b!["b"]], p!["<a>c", i!["d"]]]);
    let caret = doc.merge_backward(at.block).unwrap().unwrap();
    assert_caret(&doc, caret, blocks![p!["a", b!["b"], "<a>c", i!["d"]]]);
}

#[test]
fn merge_of_the_first_block_does_nothing() {
    let (mut doc, at, _) = setup(blocks![p!["<a>a"], p!["b"]]);
    assert_eq!(doc.merge_backward(at.block).unwrap(), None);
    assert_document(&doc, None, blocks![p!["a"], p!["b"]]);
}

#[test]
fn merge_leaves_children_where_the_block_was() {
    let (mut doc, at, _) = setup(blocks![
        p!["a"],
        p!["<a>b", p!["child 1"], p!["child 2"]],
        p!["c"],
    ]);
    let caret = doc.merge_backward(at.block).unwrap().unwrap();
    assert_caret(
        &doc,
        caret,
        blocks![p!["a<a>b"], p!["child 1"], p!["child 2"], p!["c"]],
    );
}

#[test]
fn merge_joins_with_the_previous_block_in_document_order() {
    let (mut doc, at, _) = setup(blocks![p!["a", p!["deep", p!["deeper"]]], p!["<a>b"]]);
    let caret = doc.merge_backward(at.block).unwrap().unwrap();
    assert_caret(&doc, caret, blocks![p!["a", p!["deep", p!["deeper<a>b"]]]]);

    let (mut doc, at, _) = setup(blocks![p!["parent", p!["<a>first"], p!["second"]]]);
    let caret = doc.merge_backward(at.block).unwrap().unwrap();
    assert_caret(&doc, caret, blocks![p!["parent<a>first", p!["second"]]]);
}

#[test]
fn merge_removes_a_textless_block_before_it() {
    let (mut doc, at, _) = setup(blocks![p!["a"], divider!(), p!["<a>b"]]);
    let caret = doc.merge_backward(at.block).unwrap().unwrap();
    assert_caret(&doc, caret, blocks![p!["a"], p!["<a>b"]]);
}

#[test]
fn merge_does_not_remove_a_textless_parent() {
    let alert = |items| block("alert", items);
    let (mut doc, at, _) = setup(blocks![alert(vec![p!["<a>x"].into()])]);
    assert_eq!(doc.merge_backward(at.block).unwrap(), None);
    assert_document(&doc, None, blocks![alert(vec![p!["x"].into()])]);
}

#[test]
fn delete_within_one_block() {
    let (mut doc, a, b) = setup(blocks![p!["h<a>é😀l<b>lo"]]);
    let caret = doc.delete_range(a, b).unwrap();
    assert_caret(&doc, caret, blocks![p!["h<a>lo"]]);

    let (mut doc, a, b) = setup(blocks![p!["h<b>él<a>lo"]]);
    let caret = doc.delete_range(a, b).unwrap();
    assert_caret(&doc, caret, blocks![p!["h<a>lo"]]);

    let (mut doc, a, _) = setup(blocks![p!["h<a>i"]]);
    let caret = doc.delete_range(a, a).unwrap();
    assert_caret(&doc, caret, blocks![p!["h<a>i"]]);
}

#[test]
fn delete_across_blocks_joins_the_ends() {
    let (mut doc, a, b) = setup(blocks![
        p!["ke", b!["ep<a>drop"]],
        p!["gone"],
        divider!(),
        bullet!["drop<b>", i!["tail"], bullet!["nested"]],
        p!["after"],
    ]);
    let caret = doc.delete_range(b, a).unwrap();
    assert_caret(
        &doc,
        caret,
        blocks![
            p!["ke", b!["ep<a>"], i!["tail"]],
            bullet!["nested"],
            p!["after"],
        ],
    );
}

#[test]
fn delete_from_a_parent_into_its_child() {
    let (mut doc, a, b) = setup(blocks![p![
        "par<a>ent",
        p!["gone"],
        p!["ch<b>ild", p!["grandchild"]],
        p!["sibling"],
    ]]);
    let caret = doc.delete_range(a, b).unwrap();
    assert_caret(
        &doc,
        caret,
        blocks![p!["par<a>ild", p!["grandchild"], p!["sibling"]]],
    );
}

#[test]
fn delete_out_of_a_nested_block_keeps_what_comes_after() {
    let (mut doc, a, b) = setup(blocks![
        p!["a", p!["a1 <a>x", p!["a11"]], p!["a2"]],
        p![
            "b",
            p!["b1", p!["b11 <b>tail", p!["b111"]], p!["b12"]],
            p!["b2"]
        ],
        p!["c"],
    ]);
    let caret = doc.delete_range(a, b).unwrap();
    assert_caret(
        &doc,
        caret,
        blocks![
            p!["a", p!["a1 <a>tail"]],
            p!["b111"],
            p!["b12"],
            p!["b2"],
            p!["c"],
        ],
    );
}

#[test]
fn delete_keeps_later_siblings_at_every_level_in_order() {
    let (mut doc, a, b) = setup(blocks![
        p!["st<a>art"],
        p![
            "l1",
            p!["l2", p!["l3 <b>end", p!["e1"], p!["e2"]], p!["s3"]],
            p!["s2"]
        ],
        p!["s1"],
    ]);
    let caret = doc.delete_range(a, b).unwrap();
    assert_caret(
        &doc,
        caret,
        blocks![
            p!["st<a>end"],
            p!["e1"],
            p!["e2"],
            p!["s3"],
            p!["s2"],
            p!["s1"],
        ],
    );
}

#[test]
fn delete_rejects_invalid_positions() {
    let (mut doc, _) = build(blocks![p!["é"], p!["b"]]);
    let [first, second] = doc.blocks()[..] else {
        panic!()
    };
    assert!(matches!(
        doc.delete_range(Pos::new(first, 1), Pos::new(second, 1)),
        Err(Error::NotCharBoundary(1))
    ));
    assert!(matches!(
        doc.delete_range(Pos::new(first, 0), Pos::new(second, 9)),
        Err(Error::OutOfBounds { .. })
    ));
    assert_document(&doc, None, blocks![p!["é"], p!["b"]]);
}

#[test]
fn toggle_mark_applies_then_removes() {
    let (mut doc, a, b) = setup(blocks![p!["a<a>bc<b>d"]]);
    doc.toggle_mark(a, b, "bold").unwrap();
    assert_document(&doc, None, blocks![p!["a", b!["bc"], "d"]]);
    doc.toggle_mark(b, a, "bold").unwrap();
    assert_document(&doc, None, blocks![p!["abcd"]]);
}

#[test]
fn toggle_mark_removes_a_mark_whatever_its_value() {
    let (mut doc, a, b) = setup(blocks![p!["<a>ab<b>"]]);
    doc.mark_range(a, b, "link", "https://lotype.dev").unwrap();
    doc.toggle_mark(a, b, "link").unwrap();
    assert_document(&doc, None, blocks![p!["ab"]]);
}

#[test]
fn toggling_a_valued_mark_does_not_create_a_boolean() {
    let (mut doc, a, b) = setup(blocks![p!["<a>ab<b>"]]);
    let first = Pos::new(a.block, 1);
    doc.mark_range(a, first, "link", "https://lotype.dev")
        .unwrap();
    assert!(matches!(
        doc.toggle_mark(a, b, "link"),
        Err(Error::MarkValueRequired(_))
    ));
    let link = lotype_core::test_support::marked("link", "https://lotype.dev", vec!["a".into()]);
    assert_document(&doc, None, blocks![p![link, "b"]]);
}

#[test]
fn toggle_mark_fills_a_partly_marked_range() {
    let (mut doc, a, b) = setup(blocks![p!["<a>a", b!["b"], "c<b>"]]);
    doc.toggle_mark(a, b, "bold").unwrap();
    assert_document(&doc, None, blocks![p![b!["abc"]]]);
}

#[test]
fn toggle_mark_spans_blocks_and_skips_textless_ones() {
    let (mut doc, a, b) = setup(blocks![
        p!["on<a>e", p!["nested"]],
        divider!(),
        p![],
        p!["tw<b>o"],
    ]);
    doc.toggle_mark(a, b, "italic").unwrap();
    let marked = blocks![
        p!["on", i!["e"], p![i!["nested"]]],
        divider!(),
        p![],
        p![i!["tw"], "o"],
    ];
    assert_document(&doc, None, marked);

    doc.toggle_mark(a, b, "italic").unwrap();
    assert_document(
        &doc,
        None,
        blocks![p!["one", p!["nested"]], divider!(), p![], p!["two"]],
    );
}

#[test]
fn toggle_mark_on_a_caret_does_nothing() {
    let (mut doc, a, _) = setup(blocks![p![b!["a<a>b"]]]);
    doc.toggle_mark(a, a, "bold").unwrap();
    assert_document(&doc, None, blocks![p![b!["ab"]]]);
    for result in [
        doc.toggle_mark(a, a, "blink"),
        doc.mark_range(a, a, "blink", true),
        doc.unmark_range(a, a, "blink"),
    ] {
        assert!(matches!(result, Err(Error::UnknownMark(_))));
    }
}

#[test]
fn mark_range_sets_a_value_and_unmark_range_clears_it() {
    let (mut doc, a, b) = setup(blocks![p!["a<a>b"], p!["c<b>d"]]);
    doc.mark_range(a, b, "link", "https://lotype.dev").unwrap();
    let link = |text: &str| {
        lotype_core::test_support::marked("link", "https://lotype.dev", vec![text.into()])
    };
    assert_document(&doc, None, blocks![p!["a", link("b")], p![link("c"), "d"]]);
    doc.unmark_range(a, b, "link").unwrap();
    assert_document(&doc, None, blocks![p!["ab"], p!["cd"]]);
    assert!(matches!(
        doc.mark_range(a, b, "blink", true),
        Err(Error::UnknownMark(_))
    ));
}
