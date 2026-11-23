use lotype_core::test_support::{assert_document, assert_editor, block, build, editor};
use lotype_core::{Selection, b, blocks, bullet, divider, h1, p};

fn slice_of(nodes: Vec<lotype_core::test_support::BlockNode>) -> lotype_core::Document {
    let (doc, selection) = build(nodes);
    let Some(Selection::Text { anchor, head }) = selection else {
        panic!("no text selection")
    };
    doc.slice(anchor, head).unwrap()
}

#[test]
fn a_slice_keeps_types_marks_and_nesting_inside_it() {
    let slice = slice_of(blocks![
        h1!["Ti<a>tle"],
        p!["a ", b!["bold"]],
        bullet!["x", bullet!["y<b>z"]],
    ]);
    let expected = blocks![h1!["tle"], p!["a ", b!["bold"]], bullet!["x", bullet!["y"]]];
    assert_document(&slice, None, expected);
    assert_eq!(slice.to_markdown(), "# tle\n\na **bold**\n\n- x\n\n  - y\n");
}

#[test]
fn a_slice_from_inside_a_child_starts_at_the_top() {
    let slice = slice_of(blocks![
        bullet!["x", bullet!["y<b>z"]],
        divider!(),
        p!["w<a>"]
    ]);
    assert_document(&slice, None, blocks![bullet!["z"], divider!(), p!["w"]]);
}

#[test]
fn one_pasted_paragraph_goes_into_the_text_with_its_marks() {
    let mut ed = editor(blocks![p!["ab<a>cd"]]);
    ed.paste_markdown("x **y**").unwrap();
    assert_editor(&ed, blocks![p!["abx ", b!["y"], "<a>cd"]]);
    assert!(ed.undo().unwrap());
    assert_editor(&ed, blocks![p!["ab<a>cd"]]);
}

#[test]
fn pasted_blocks_go_between_the_halves_and_join_paragraphs() {
    let mut ed = editor(blocks![p!["a<a>bc<b>d"]]);
    ed.paste_markdown("x\n\n# h\n\ny").unwrap();
    assert_editor(&ed, blocks![p!["ax"], h1!["h"], p!["y<a>d"]]);
    assert!(ed.undo().unwrap());
    assert_editor(&ed, blocks![p!["a<a>bc<b>d"]]);
}

#[test]
fn a_pasted_heading_or_divider_stays_whole() {
    let mut ed = editor(blocks![p!["ab<a>cd"]]);
    ed.paste_markdown("# h\n\n---").unwrap();
    assert_editor(&ed, blocks![p!["ab"], h1!["h"], divider!(), p!["<a>cd"]]);

    // An empty half goes.
    let mut ed = editor(blocks![p!["<a>cd"]]);
    ed.paste_markdown("# h\n\ny").unwrap();
    assert_editor(&ed, blocks![h1!["h"], p!["y<a>cd"]]);
}

#[test]
fn code_takes_pasted_text_as_plain_text() {
    let mut ed = editor(blocks![block("code_block", vec!["a<a>b".into()])]);
    ed.paste_markdown("**x**\n\ny").unwrap();
    assert_editor(&ed, blocks![block("code_block", vec!["ax\ny<a>b".into()])]);
}

#[test]
fn what_is_copied_pastes_back_the_same() {
    let source = blocks![
        p!["<a>a ", b!["bold"]],
        bullet!["x", bullet!["y"]],
        h1!["t<b>"]
    ];
    let markdown = slice_of(source).to_markdown();
    let mut ed = editor(blocks![p!["<a>"]]);
    ed.paste_markdown(&markdown).unwrap();
    assert_editor(
        &ed,
        blocks![
            p!["a ", b!["bold"]],
            bullet!["x", bullet!["y"]],
            h1!["t<a>"]
        ],
    );
}

#[test]
fn a_copied_document_pastes_back_whole_through_its_bytes() {
    let source = blocks![
        p![
            "<a>a ",
            lotype_core::test_support::marked("underline", true, vec!["u".into()])
        ],
        h1!["t<b>"]
    ];
    let (doc, selection) = build(source);
    let Some(Selection::Text { anchor, head }) = selection else {
        panic!()
    };
    let bytes = doc.slice(anchor, head).unwrap().export_snapshot().unwrap();
    let mut copied = lotype_core::Document::new(lotype_core::Schema::builtin());
    copied.import(&bytes).unwrap();
    let mut ed = editor(blocks![p!["x<a>y"]]);
    ed.paste_document(&copied).unwrap();
    // Markdown would have lost the underline.
    let underlined = lotype_core::test_support::marked("underline", true, vec!["u".into()]);
    assert_editor(&ed, blocks![p!["xa ", underlined], h1!["t<a>y"]]);
}

#[test]
fn what_the_schema_lacks_is_left_out_of_a_paste() {
    let schema = lotype_core::Schema::builtin()
        .block(lotype_core::BlockSpec::new(
            "poll",
            lotype_core::Content::Inline,
        ))
        .mark(lotype_core::MarkSpec::new(
            "glow",
            lotype_core::Expand::None,
        ));
    let mut other = lotype_core::Document::new(schema);
    let poll = other.insert_block(None, 0, "poll").unwrap();
    other.insert_text(poll, 0, "gone").unwrap();
    let kept = other.insert_block(None, 1, "heading").unwrap();
    other.insert_text(kept, 0, "kept").unwrap();
    other.mark(kept, 0..2, "glow", true).unwrap();
    // As another editor's copy reads here, with this editor's schema.
    let mut copied = lotype_core::Document::new(lotype_core::Schema::builtin());
    copied.import(&other.export_snapshot().unwrap()).unwrap();
    let mut ed = editor(blocks![p!["<a>"]]);
    ed.paste_document(&copied).unwrap();
    assert_editor(&ed, blocks![block("heading", vec!["kept<a>".into()])]);
}
