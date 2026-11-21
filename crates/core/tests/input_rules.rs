use lotype_core::test_support::{editor, outline};
use lotype_core::{BlockSpec, Content, Document, Editor, Pos, Schema, Selection, blocks, p};

/// Types `text` a character at a time, as a keyboard does.
fn type_in(ed: &mut Editor, text: &str) {
    for c in text.chars() {
        ed.insert_text(c.encode_utf8(&mut [0; 4])).unwrap();
    }
}

fn shown(ed: &Editor) -> String {
    outline(ed.document(), ed.selection())
}

#[test]
fn markdown_at_the_start_of_a_paragraph_turns_it_into_that_block() {
    for (typed, expected) in [
        ("# ", "heading level=1 \"<a>title\"\n"),
        ("### ", "heading level=3 \"<a>title\"\n"),
        ("- ", "bullet_list_item \"<a>title\"\n"),
        ("* ", "bullet_list_item \"<a>title\"\n"),
        ("1. ", "numbered_list_item \"<a>title\"\n"),
        ("7. ", "numbered_list_item start=7 \"<a>title\"\n"),
        ("[] ", "check_list_item checked=false \"<a>title\"\n"),
        ("[x] ", "check_list_item checked=true \"<a>title\"\n"),
        ("> ", "quote \"<a>title\"\n"),
        ("``` ", "code_block \"<a>title\"\n"),
        ("```rust ", "code_block language=rust \"<a>title\"\n"),
    ] {
        let mut ed = editor(blocks![p!["<a>title"]]);
        type_in(&mut ed, typed);
        assert_eq!(shown(&ed), expected, "{typed:?}");

        // Undo gives back what was typed, and redo the block.
        assert!(ed.undo().unwrap());
        assert_eq!(shown(&ed), format!("paragraph \"{typed}<a>title\"\n"));
        assert!(ed.redo().unwrap());
        assert_eq!(shown(&ed), expected);
    }
}

#[test]
fn three_dashes_make_a_divider_and_typing_goes_on_below() {
    let mut ed = editor(blocks![p!["<a>"], p!["after"]]);
    type_in(&mut ed, "---");
    assert_eq!(
        shown(&ed),
        "divider\nparagraph \"<a>\"\nparagraph \"after\"\n"
    );
    assert!(ed.undo().unwrap());
    assert_eq!(shown(&ed), "paragraph \"---<a>\"\nparagraph \"after\"\n");

    // Text after the caret has nowhere to go.
    let mut ed = editor(blocks![p!["<a>x"]]);
    type_in(&mut ed, "---");
    assert_eq!(shown(&ed), "paragraph \"---<a>x\"\n");
}

#[test]
fn block_rules_only_act_where_the_result_fits() {
    // Not at the start.
    let mut ed = editor(blocks![p!["a<a>"]]);
    type_in(&mut ed, " # ");
    assert_eq!(shown(&ed), "paragraph \"a # <a>\"\n");

    // Only in a paragraph.
    let mut ed = editor(blocks![p!["<a>"]]);
    type_in(&mut ed, "- ");
    type_in(&mut ed, "# ");
    assert_eq!(shown(&ed), "bullet_list_item \"# <a>\"\n");

    // A heading takes no children, a list item does.
    let mut ed = editor(blocks![p!["<a>", p!["child"]]]);
    type_in(&mut ed, "# ");
    assert_eq!(shown(&ed), "paragraph \"# <a>\"\n  paragraph \"child\"\n");
    let mut ed = editor(blocks![p!["<a>", p!["child"]]]);
    type_in(&mut ed, "- ");
    assert_eq!(
        shown(&ed),
        "bullet_list_item \"<a>\"\n  paragraph \"child\"\n"
    );

    // Not for a block type the schema lacks.
    let schema = Schema::new().block(BlockSpec::new("paragraph", Content::Inline));
    let mut doc = Document::new(schema);
    let block = doc.insert_block(None, 0, "paragraph").unwrap();
    let mut ed = Editor::new(doc);
    ed.set_selection(Some(Selection::caret(Pos::new(block, 0))))
        .unwrap();
    type_in(&mut ed, "# **b**");
    assert_eq!(ed.document().text(block).unwrap(), "# **b**");
}

#[test]
fn text_typed_between_delimiters_gets_the_mark() {
    for (typed, expected) in [
        ("a **b c**", "paragraph \"a [bold|b c]<a>\"\n"),
        ("*b*", "paragraph \"[italic|b]<a>\"\n"),
        ("_b_", "paragraph \"[italic|b]<a>\"\n"),
        ("~~b~~", "paragraph \"[strike|b]<a>\"\n"),
        ("f(`b`", "paragraph \"f([code|b]<a>\"\n"),
        ("2*3*", "paragraph \"2*3*<a>\"\n"),
        ("snake_case_", "paragraph \"snake_case_<a>\"\n"),
    ] {
        let mut ed = editor(blocks![p!["<a>"]]);
        type_in(&mut ed, typed);
        assert_eq!(shown(&ed), expected, "{typed:?}");
    }
}

#[test]
fn typing_after_a_mark_rule_is_outside_the_mark_and_undo_gives_the_text_back() {
    let mut ed = editor(blocks![p!["<a> tail"]]);
    type_in(&mut ed, "**b**");
    assert_eq!(shown(&ed), "paragraph \"[bold|b]<a> tail\"\n");
    type_in(&mut ed, "c");
    assert_eq!(shown(&ed), "paragraph \"[bold|b]c<a> tail\"\n");

    assert!(ed.undo().unwrap());
    assert!(ed.undo().unwrap());
    assert_eq!(shown(&ed), "paragraph \"**b**<a> tail\"\n");
}
