use lotype_core::test_support::{assert_document, block, build, marked, outline};
use lotype_core::{Pos, Selection, Value, b, blocks, bullet, divider, h1, i, p};

#[test]
fn builds_blocks_marks_and_children() {
    let (doc, selection) = build(blocks![
        h1!["Title"],
        p!["Hello ", b!["wor", i!["ld"]], "!", bullet!["child"]],
        divider!(),
        block("quote", vec!["said".into()]).attr("author", "x"),
    ]);
    assert_eq!(selection, None);
    assert_eq!(
        outline(&doc, None),
        concat!(
            "heading level=1 \"Title\"\n",
            "paragraph \"Hello [bold|wor][bold,italic|ld]!\"\n",
            "  bullet_list_item \"child\"\n",
            "divider\n",
            "quote author=x \"said\"\n",
        )
    );
}

#[test]
fn reads_selection_markers() {
    let (doc, selection) = build(blocks![p!["a<a>é", b!["c<b>d"]], p![]]);
    let first = doc.blocks()[0];
    assert_eq!(
        selection,
        Some(Selection::Text {
            anchor: Pos::new(first, 1),
            head: Pos::new(first, 4),
        })
    );
    assert_eq!(doc.text(first).unwrap(), "aécd");
    assert_eq!(
        outline(&doc, selection.as_ref()),
        "paragraph \"a<a>é[bold|c<b>d]\"\nparagraph \"\"\n"
    );

    let (doc, selection) = build(blocks![p!["ab"], p!["<a>"]]);
    let second = doc.blocks()[1];
    assert_eq!(selection, Some(Selection::caret(Pos::new(second, 0))));
    assert_eq!(
        outline(&doc, selection.as_ref()),
        "paragraph \"ab\"\nparagraph \"<a>\"\n"
    );
}

#[test]
fn outlines_markers_at_span_edges_and_backwards_selections() {
    let (doc, selection) = build(blocks![p!["<b>ab", b!["cd"], "<a>"]]);
    assert_eq!(
        outline(&doc, selection.as_ref()),
        "paragraph \"<b>ab[bold|cd]<a>\"\n"
    );
    let (doc, selection) = build(blocks![p!["ab<a>", b!["cd"]]]);
    assert_eq!(
        outline(&doc, selection.as_ref()),
        "paragraph \"ab<a>[bold|cd]\"\n"
    );
}

#[test]
fn outlines_block_selections_and_mark_values() {
    let link = marked("link", "https://lotype.dev", vec!["site".into()]);
    let (doc, _) = build(blocks![p![link], p!["x"]]);
    let selection = Selection::Blocks(vec![doc.blocks()[1]]);
    assert_eq!(
        outline(&doc, Some(&selection)),
        "paragraph \"[link=https://lotype.dev|site]\"\n* paragraph \"x\"\n"
    );
    assert_eq!(
        doc.spans(doc.blocks()[0]).unwrap()[0].marks["link"],
        Value::from("https://lotype.dev")
    );
}

#[test]
fn assert_document_compares_selection_only_when_expected_has_markers() {
    let (doc, selection) = build(blocks![p!["a<a>b"]]);
    assert_document(&doc, selection.as_ref(), blocks![p!["ab"]]);
    assert_document(&doc, selection.as_ref(), blocks![p!["a<a>b"]]);
}

#[test]
#[should_panic(expected = "left: \"paragraph \\\"a<a>b\\\"")]
fn assert_document_fails_on_a_different_selection() {
    let (doc, selection) = build(blocks![p!["a<a>b"]]);
    assert_document(&doc, selection.as_ref(), blocks![p!["ab<a>"]]);
}

#[test]
#[should_panic(expected = "duplicate selection marker")]
fn rejects_duplicate_markers() {
    build(blocks![p!["<a>x<a>"]]);
}
