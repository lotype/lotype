//! What a view does to one block, and reads of it, without spelling a key.

use lotype_core::test_support::{block, editor, outline};
use lotype_core::{Align, BlockColor, Document, Editor, Schema, Selection, attr, blocks, h1, p};

fn shown(ed: &Editor) -> String {
    outline(ed.document(), ed.selection())
}

#[test]
fn a_check_list_item_is_checked_and_unchecked_as_undo_steps() {
    let mut ed = editor(blocks![block("check_list_item", vec!["task".into()])]);
    let item = ed.document().blocks()[0];
    assert!(!ed.document().is_checked(item));
    ed.set_checked(item, true).unwrap();
    assert!(ed.document().is_checked(item));
    ed.undo().unwrap();
    assert!(!ed.document().is_checked(item));
}

#[test]
fn colors_go_on_a_block_that_takes_a_style_and_not_on_code() {
    let code = block("code_block", vec!["x".into()]);
    let mut ed = editor(blocks![p!["a"], code]);
    let [text, code] = ed.document().blocks()[..] else {
        panic!()
    };
    ed.set_block_color(text, BlockColor::Background, Some("red"))
        .unwrap();
    ed.set_block_color(code, BlockColor::Text, Some("red"))
        .unwrap();
    let doc = ed.document();
    assert_eq!(
        doc.block_color(text, BlockColor::Background).as_deref(),
        Some("red")
    );
    assert_eq!(doc.block_color(text, BlockColor::Text), None);
    assert_eq!(doc.attr(code, attr::TEXT_COLOR).unwrap(), None);
    ed.set_block_color(text, BlockColor::Background, None)
        .unwrap();
    assert_eq!(
        ed.document().block_color(text, BlockColor::Background),
        None
    );
}

#[test]
fn alignment_is_stored_as_peers_read_it() {
    let mut ed = editor(blocks![p!["<a>a"]]);
    let block = ed.document().blocks()[0];
    ed.set_alignment(Align::Center).unwrap();
    assert_eq!(ed.document().alignment(block), Align::Center);
    assert_eq!(shown(&ed), "paragraph text_alignment=center \"<a>a\"\n");
    ed.set_alignment(Align::Left).unwrap();
    assert_eq!(shown(&ed), "paragraph \"<a>a\"\n");
}

#[test]
fn a_heading_level_a_peer_wrote_out_of_range_reads_within_it() {
    let mut ed = editor(blocks![h1!["t"]]);
    let heading = ed.document().blocks()[0];
    ed.apply(|doc, selection| {
        doc.set_attr(heading, attr::LEVEL, 9)?;
        Ok(selection.cloned())
    })
    .unwrap();
    assert_eq!(ed.document().heading_level(heading), 5);
}

#[test]
fn a_text_block_is_added_after_a_block_or_an_empty_one_is_taken() {
    let mut ed = editor(blocks![p!["a"], p!["b"]]);
    let first = ed.document().blocks()[0];
    ed.add_text_block_after(first).unwrap();
    assert_eq!(
        shown(&ed),
        "paragraph \"a\"\nparagraph \"<a>\"\nparagraph \"b\"\n"
    );
    let empty = ed.document().blocks()[1];
    ed.add_text_block_after(empty).unwrap();
    assert_eq!(ed.document().blocks().len(), 3);
    assert_eq!(
        ed.selection(),
        Some(&Selection::caret(lotype_core::Pos::new(empty, 0)))
    );
}

#[test]
fn a_block_moves_next_to_another_and_the_selection_stays() {
    let mut ed = editor(blocks![p!["a<a>"], p!["b"], p!["c"]]);
    let [a, _, c] = ed.document().blocks()[..] else {
        panic!()
    };
    ed.move_block_next_to(a, c, true).unwrap();
    assert_eq!(
        shown(&ed),
        "paragraph \"b\"\nparagraph \"c\"\nparagraph \"a<a>\"\n"
    );
}

#[test]
fn writing_starts_in_an_empty_document_only() {
    let mut ed = Editor::new(Document::new(Schema::builtin()));
    assert!(ed.start_writing().unwrap());
    assert_eq!(shown(&ed), "paragraph \"<a>\"\n");
    assert!(!ed.start_writing().unwrap());
    assert_eq!(ed.document().blocks().len(), 1);
}
