use lotype_core::test_support::{build, outline};
use lotype_core::{BlockId, Document, Editor, Pos, Selection, blocks, p};

fn with_table(rows: usize, columns: usize) -> (Editor, BlockId) {
    let (mut doc, _) = build(blocks![p!["before"], p!["after"]]);
    let table = doc.insert_table(None, 1, rows, columns).unwrap();
    // Each cell says where it is: "r0c1" is the second cell of the first row.
    for (r, row) in doc.children(Some(table)).unwrap().into_iter().enumerate() {
        for (c, column) in doc.columns(table).unwrap().into_iter().enumerate() {
            let cell = doc.cell(row, column).unwrap();
            doc.insert_text(cell, 0, &format!("r{r}c{c}")).unwrap();
        }
    }
    (Editor::new(doc), table)
}

fn cell(doc: &Document, table: BlockId, row: usize, column: usize) -> BlockId {
    let rows = doc.children(Some(table)).unwrap();
    doc.cell(rows[row], doc.columns(table).unwrap()[column])
        .unwrap()
}

fn shown(ed: &Editor) -> String {
    outline(ed.document(), ed.selection())
}

#[test]
fn rows_and_columns_are_added_with_the_caret_in_them() {
    let (mut ed, table) = with_table(2, 2);
    let at = cell(ed.document(), table, 1, 1);
    ed.set_selection(Some(Selection::caret(Pos::new(at, 0))))
        .unwrap();
    ed.add_column(table, 1).unwrap();
    assert_eq!(
        shown(&ed),
        "paragraph \"before\"\ntable\n  table_row | \"r0c0\" | \"\" | \"r0c1\"\n  table_row | \"r1c0\" | \"<a>\" | \"r1c1\"\nparagraph \"after\"\n"
    );
    ed.add_row(table, 2).unwrap();
    assert_eq!(
        shown(&ed),
        "paragraph \"before\"\ntable\n  table_row | \"r0c0\" | \"\" | \"r0c1\"\n  table_row | \"r1c0\" | \"\" | \"r1c1\"\n  table_row | \"<a>\" | \"\" | \"\"\nparagraph \"after\"\n"
    );
    // Each is one undo step.
    ed.undo().unwrap();
    ed.undo().unwrap();
    assert_eq!(
        outline(ed.document(), None),
        "paragraph \"before\"\ntable\n  table_row | \"r0c0\" | \"r0c1\"\n  table_row | \"r1c0\" | \"r1c1\"\nparagraph \"after\"\n"
    );
}

#[test]
fn rows_and_columns_go_with_the_caret_to_the_cell_in_their_place() {
    let (mut ed, table) = with_table(2, 3);
    ed.delete_row(cell(ed.document(), table, 1, 2)).unwrap();
    assert_eq!(
        shown(&ed),
        "paragraph \"before\"\ntable\n  table_row | \"r0c0\" | \"r0c1\" | \"r0c2<a>\"\nparagraph \"after\"\n"
    );
    ed.delete_column(cell(ed.document(), table, 0, 1)).unwrap();
    assert_eq!(
        shown(&ed),
        "paragraph \"before\"\ntable\n  table_row | \"r0c0\" | \"r0c2<a>\"\nparagraph \"after\"\n"
    );
    ed.undo().unwrap();
    assert_eq!(
        outline(ed.document(), None),
        "paragraph \"before\"\ntable\n  table_row | \"r0c0\" | \"r0c1\" | \"r0c2\"\nparagraph \"after\"\n"
    );
}

#[test]
fn the_last_row_or_column_takes_the_table_with_it() {
    let (mut ed, table) = with_table(1, 2);
    ed.delete_row(cell(ed.document(), table, 0, 0)).unwrap();
    assert_eq!(shown(&ed), "paragraph \"before\"\nparagraph \"<a>after\"\n");

    // With no text around it, a paragraph takes its place.
    let mut doc = Document::new(lotype_core::Schema::builtin());
    let table = doc.insert_table(None, 0, 2, 1).unwrap();
    let mut ed = Editor::new(doc);
    ed.delete_column(cell(ed.document(), table, 1, 0)).unwrap();
    assert_eq!(shown(&ed), "paragraph \"<a>\"\n");
    ed.undo().unwrap();
    assert_eq!(
        outline(ed.document(), None),
        "table\n  table_row | \"\"\n  table_row | \"\"\n"
    );
}

#[test]
fn a_table_without_columns_takes_a_row_without_a_caret() {
    let mut doc = Document::new(lotype_core::Schema::builtin());
    let table = doc.insert_table(None, 0, 1, 0).unwrap();
    let mut ed = Editor::new(doc);
    ed.add_row(table, 1).unwrap();
    assert_eq!(ed.document().children(Some(table)).unwrap().len(), 2);
}

#[test]
fn selected_cells_are_copied_as_a_table_of_them() {
    let (ed, table) = with_table(3, 3);
    let doc = ed.document();
    let slice = doc
        .slice_cells(cell(doc, table, 2, 2), cell(doc, table, 1, 1))
        .unwrap();
    assert_eq!(
        outline(&slice, None),
        "table\n  table_row | \"r1c1\" | \"r1c2\"\n  table_row | \"r2c1\" | \"r2c2\"\n"
    );
}

#[test]
fn backspace_and_delete_after_a_table_take_an_empty_block_away() {
    for backward in [true, false] {
        let mut doc = Document::new(lotype_core::Schema::builtin());
        doc.insert_table(None, 0, 1, 2).unwrap();
        let after = doc.insert_block(None, 1, "paragraph").unwrap();
        let mut ed = Editor::new(doc);
        ed.set_selection(Some(Selection::caret(Pos::new(after, 0))))
            .unwrap();
        match backward {
            true => ed.delete_backward().unwrap(),
            false => ed.delete_forward().unwrap(),
        }
        assert_eq!(shown(&ed), "table\n  table_row | \"\" | \"<a>\"\n");
    }
    // A block with text stays, and Backspace goes into the last cell.
    let (mut ed, _) = with_table(1, 2);
    let after = ed.document().blocks()[3];
    ed.set_selection(Some(Selection::caret(Pos::new(after, 0))))
        .unwrap();
    ed.delete_backward().unwrap();
    assert_eq!(
        shown(&ed),
        "paragraph \"before\"\ntable\n  table_row | \"r0c0\" | \"r0c1<a>\"\nparagraph \"after\"\n"
    );
}
