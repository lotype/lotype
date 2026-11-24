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
    // From the cell above "d", Enter goes down to it; Shift-Enter breaks the line.
    ed.keys("shift-tab enter");
    ed.type_text("x");
    ed.keys("shift-enter");
    ed.type_text("y");
    assert_eq!(
        shown(&mut ed),
        "table\n  table_row | \"a\" | \"b\"\n  table_row | \"c\" | \"dx\ny<a>\"\n"
    );
}

fn table_doc(cx: &mut TestAppContext) -> EditorTestContext<'_> {
    let mut ed = EditorTestContext::new(cx, blocks![p!["<a>"]]);
    ed.type_text("/table");
    ed.keys("enter");
    ed.type_text("a");
    ed.keys("tab");
    ed.type_text("b");
    ed.settle();
    ed
}

fn cell_at(ed: &mut EditorTestContext, row: usize, column: usize) -> lotype_core::BlockId {
    ed.editor(|editor| {
        let doc = editor.document();
        let table = doc.blocks()[0];
        let row = doc.children(Some(table)).unwrap()[row];
        doc.cell(row, doc.columns(table).unwrap()[column]).unwrap()
    })
}

fn center_of(ed: &mut EditorTestContext, cell: lotype_core::BlockId) -> gpui::Point<gpui::Pixels> {
    ed.view
        .update(ed.cx, |view, _| view.text_bounds(cell).unwrap().center())
}

fn click(ed: &mut EditorTestContext, selector: &'static str) {
    ed.settle();
    let at = ed.cx.debug_bounds(selector).unwrap().center();
    ed.cx.simulate_click(at, Modifiers::none());
    ed.settle();
}

#[gpui::test]
fn the_menu_of_a_cell_adds_and_takes_rows_and_columns(cx: &mut TestAppContext) {
    let mut ed = table_doc(cx);
    click(&mut ed, "table-menu");
    click(&mut ed, "table-menu-column-right");
    assert_eq!(
        shown(&mut ed),
        "table\n  table_row | \"a\" | \"b\" | \"<a>\"\n  table_row | \"\" | \"\" | \"\"\n"
    );
    click(&mut ed, "table-menu");
    click(&mut ed, "table-menu-row-above");
    assert_eq!(
        shown(&mut ed),
        "table\n  table_row | \"<a>\" | \"\" | \"\"\n  table_row | \"a\" | \"b\" | \"\"\n  table_row | \"\" | \"\" | \"\"\n"
    );
    // The caret goes to the cell that takes the place of what went.
    click(&mut ed, "table-menu");
    click(&mut ed, "table-menu-delete-row");
    click(&mut ed, "table-menu");
    click(&mut ed, "table-menu-delete-column");
    assert_eq!(
        shown(&mut ed),
        "table\n  table_row | \"b<a>\" | \"\"\n  table_row | \"\" | \"\"\n"
    );
}

#[gpui::test]
fn the_bars_beside_a_table_add_a_row_and_a_column(cx: &mut TestAppContext) {
    let mut ed = table_doc(cx);
    let cell = cell_at(&mut ed, 1, 1);
    let over = center_of(&mut ed, cell);
    ed.cx.simulate_mouse_move(over, None, Modifiers::none());
    click(&mut ed, "table-add-row");
    click(&mut ed, "table-add-column");
    assert_eq!(
        shown(&mut ed),
        "table\n  table_row | \"a\" | \"b\" | \"\"\n  table_row | \"\" | \"\" | \"\"\n  table_row | \"\" | \"\" | \"<a>\"\n"
    );
}

#[gpui::test]
fn tab_in_the_last_cell_starts_a_row(cx: &mut TestAppContext) {
    let mut ed = table_doc(cx);
    ed.keys("tab tab tab");
    ed.type_text("c");
    assert_eq!(
        shown(&mut ed),
        "table\n  table_row | \"a\" | \"b\"\n  table_row | \"\" | \"\"\n  table_row | \"c<a>\" | \"\"\n"
    );
}

#[gpui::test]
fn dragging_across_cells_selects_them_whole(cx: &mut TestAppContext) {
    let mut ed = table_doc(cx);
    let (from, to) = (cell_at(&mut ed, 0, 0), cell_at(&mut ed, 1, 1));
    let (from, to) = (center_of(&mut ed, from), center_of(&mut ed, to));
    ed.cx
        .simulate_mouse_down(from, gpui::MouseButton::Left, Modifiers::none());
    ed.cx
        .simulate_mouse_move(to, Some(gpui::MouseButton::Left), Modifiers::none());
    ed.cx
        .simulate_mouse_up(to, gpui::MouseButton::Left, Modifiers::none());
    ed.settle();
    let cells = ed.editor(|editor| {
        matches!(
            editor.selection(),
            Some(lotype_core::Selection::Cells { .. })
        )
    });
    assert!(cells);
    // Backspace empties them, and the table stays.
    ed.keys("backspace");
    assert_eq!(
        ed.editor(|editor| outline(editor.document(), None)),
        "table\n  table_row | \"\" | \"\"\n  table_row | \"\" | \"\"\n"
    );
}

#[gpui::test]
fn a_row_and_a_column_added_by_two_peers_meet_in_the_grid(cx: &mut TestAppContext) {
    use lotype_core::{Document, Editor, Schema};
    let mut ed = table_doc(cx);
    let snapshot = ed.editor(|editor| editor.document().export_snapshot().unwrap());
    let mut theirs = Document::new(Schema::builtin());
    theirs.import(&snapshot).unwrap();
    let mut theirs = Editor::new(theirs);
    let table = ed.editor(|editor| editor.document().blocks()[0]);
    theirs.add_column(table, 2).unwrap();
    ed.view.update(ed.cx, |view, cx| {
        view.update_editor(cx, |editor| {
            editor.add_row(table, 2).unwrap();
            editor
                .import(&theirs.document().export_snapshot().unwrap())
                .unwrap();
        });
    });
    let ours = ed.editor(|editor| editor.document().export_snapshot().unwrap());
    theirs.import(&ours).unwrap();
    ed.settle();
    let grid = ed.editor(|editor| outline(editor.document(), None));
    assert_eq!(grid, outline(theirs.document(), None));
    assert_eq!(grid.matches("table_row").count(), 3);
    assert!(
        grid.lines()
            .skip(1)
            .all(|row| row.matches('|').count() == 3)
    );
    // Every cell is drawn, the one where the two meet too.
    for row in 0..3 {
        for column in 0..3 {
            let cell = cell_at(&mut ed, row, column);
            assert!(
                ed.view
                    .update(ed.cx, |view, _| view.text_bounds(cell))
                    .is_some()
            );
        }
    }
}

#[gpui::test]
fn selected_cells_copy_as_rows_of_tabs_and_give_way_to_typing(cx: &mut TestAppContext) {
    let mut ed = table_doc(cx);
    // Shift and a click in another cell select the cells between.
    let corner = cell_at(&mut ed, 1, 1);
    let at = center_of(&mut ed, corner);
    ed.cx.simulate_click(at, Modifiers::none());
    let first = cell_at(&mut ed, 0, 0);
    let at = center_of(&mut ed, first);
    ed.cx.simulate_click(at, Modifiers::shift());
    ed.keys("cmd-c");
    let copied = ed.cx.read_from_clipboard().and_then(|item| item.text());
    assert_eq!(copied.as_deref(), Some("a\tb\n\t"));
    // Typing goes on in the cell the selection ends in.
    ed.type_text("!");
    assert_eq!(
        shown(&mut ed),
        "table\n  table_row | \"a!<a>\" | \"b\"\n  table_row | \"\" | \"\"\n"
    );
}

#[gpui::test]
fn shift_and_arrows_go_from_text_into_cells_and_from_cell_to_cell(cx: &mut TestAppContext) {
    let mut ed = table_doc(cx);
    let copied = |ed: &mut EditorTestContext| {
        ed.keys("cmd-c");
        ed.cx.read_from_clipboard().and_then(|item| item.text())
    };
    // In a cell's text, Shift and an arrow select text.
    ed.keys("shift-left");
    assert_eq!(copied(&mut ed).as_deref(), Some("b"));
    // Past its edge, cells; then a cell at a time, within the table.
    ed.keys("shift-down");
    assert_eq!(copied(&mut ed).as_deref(), Some("b\n"));
    ed.keys("shift-left");
    assert_eq!(copied(&mut ed).as_deref(), Some("a\tb\n\t"));
    ed.keys("shift-up shift-up shift-left");
    assert_eq!(copied(&mut ed).as_deref(), Some("a\tb"));
}
