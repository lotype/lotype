use gpui::{Modifiers, TestAppContext, point, px};
use lotype_core::test_support::outline;
use lotype_core::{blocks, p};
use lotype_ui::test_support::EditorTestContext;

fn shown(ed: &mut EditorTestContext) -> String {
    ed.editor(|editor| outline(editor.document(), editor.selection()))
}

#[gpui::test]
fn a_table_from_the_slash_menu_is_two_by_two_and_written_in(cx: &mut TestAppContext) {
    let mut ed = EditorTestContext::new(cx, blocks![p!["<a>"]]);
    ed.type_text("/table");
    ed.keys("enter");
    ed.type_text("a");
    ed.keys("tab");
    ed.type_text("b");
    ed.settle();
    assert_eq!(
        shown(&mut ed),
        "table\n  table_row | \"a\" | \"b<a>\"\n  table_row | \"\" | \"\"\n"
    );
    // A click in a cell puts the caret there, in the column clicked.
    let cell = ed.editor(|editor| {
        let doc = editor.document();
        let table = doc.blocks()[0];
        let row = doc.children(Some(table)).unwrap()[1];
        doc.cell(row, doc.columns(table).unwrap()[1]).unwrap()
    });
    let at = ed.view.update(ed.cx, |view, _| {
        let text = view.text_bounds(cell).unwrap();
        point(text.left() + px(2.), text.center().y)
    });
    ed.cx.simulate_click(at, Modifiers::none());
    ed.type_text("d");
    ed.keys("shift-tab");
    ed.type_text("c");
    assert_eq!(
        shown(&mut ed),
        "table\n  table_row | \"a\" | \"b\"\n  table_row | \"c<a>\" | \"d\"\n"
    );
}
