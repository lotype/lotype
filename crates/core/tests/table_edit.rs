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
