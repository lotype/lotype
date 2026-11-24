use lotype_core::test_support::{assert_editor, block, editor};
use lotype_core::{blocks, bullet, p};

fn tip(items: Vec<lotype_core::test_support::Item>) -> lotype_core::test_support::BlockNode {
    block("alert", items).attr("kind", "tip")
}

fn quote(items: Vec<lotype_core::test_support::Item>) -> lotype_core::test_support::BlockNode {
    block("quote", items)
}

#[test]
fn enter_in_an_alert_leaves_it_for_a_paragraph() {
    let mut ed = editor(blocks![tip(vec![p!["one<a>"].into()]), p!["after"]]);
    ed.split_block().unwrap();
    assert_editor(
        &ed,
        blocks![tip(vec![p!["one"].into()]), p!["<a>"], p!["after"]],
    );
    // One undo step.
    ed.undo().unwrap();
    assert_editor(&ed, blocks![tip(vec![p!["one<a>"].into()]), p!["after"]]);

    // An empty block goes out itself, and an alert it leaves empty goes.
    // An empty list item leaves its list first.
    let mut ed = editor(blocks![tip(vec![p!["one"].into(), bullet!["<a>"].into()])]);
    ed.split_block().unwrap();
    assert_editor(&ed, blocks![tip(vec![p!["one"].into(), p!["<a>"].into()])]);
    ed.split_block().unwrap();
    assert_editor(&ed, blocks![tip(vec![p!["one"].into()]), p!["<a>"]]);
    let mut ed = editor(blocks![tip(vec![p!["<a>"].into()])]);
    ed.split_block().unwrap();
    assert_editor(&ed, blocks![p!["<a>"]]);
}

#[test]
fn enter_in_a_quote_takes_the_text_after_the_caret_out() {
    let mut ed = editor(blocks![quote(vec!["q<a>uote".into(), p!["inside"].into()])]);
    ed.split_block().unwrap();
    assert_editor(
        &ed,
        blocks![quote(vec!["q".into(), p!["inside"].into()]), p!["<a>uote"]],
    );
    // From a block in it too; Shift-Enter breaks the line in it.
    let mut ed = editor(blocks![quote(vec!["q".into(), bullet!["item<a>"].into()])]);
    ed.split_block().unwrap();
    assert_editor(
        &ed,
        blocks![quote(vec!["q".into(), bullet!["item"].into()]), p!["<a>"]],
    );
    let mut ed = editor(blocks![quote(vec!["a<a>b".into()])]);
    ed.insert_line_break().unwrap();
    assert_editor(&ed, blocks![quote(vec!["a\n<a>b".into()])]);
}

#[test]
fn shift_enter_in_a_cell_of_a_table_in_an_alert_breaks_the_line() {
    use lotype_core::{Editor, Pos, Selection};
    let (mut doc, _) = lotype_core::test_support::build(blocks![tip(vec![p!["one"].into()])]);
    let alert = doc.blocks()[0];
    let table = doc.insert_table(Some(alert), 1, 1, 1).unwrap();
    let row = doc.children(Some(table)).unwrap()[0];
    let cell = doc.cell(row, doc.columns(table).unwrap()[0]).unwrap();
    let mut ed = Editor::new(doc);
    ed.set_selection(Some(Selection::caret(Pos::new(cell, 0))))
        .unwrap();
    ed.insert_line_break().unwrap();
    assert_eq!(ed.document().text(cell).unwrap(), "\n");
}
