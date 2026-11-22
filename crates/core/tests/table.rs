use lotype_core::test_support::{build, outline};
use lotype_core::{BlockId, ColumnId, Document, Editor, Error, Pos, Schema, Selection, blocks, p};

/// A paragraph, a table of `rows` by `columns`, and a paragraph.
fn with_table(rows: usize, columns: usize) -> (Document, BlockId) {
    let (mut doc, _) = build(blocks![p!["before"], p!["after"]]);
    let table = doc.insert_table(None, 1, rows, columns).unwrap();
    (doc, table)
}

fn grid(doc: &Document, table: BlockId) -> (Vec<BlockId>, Vec<ColumnId>) {
    (
        doc.children(Some(table)).unwrap(),
        doc.columns(table).unwrap(),
    )
}

fn peer(doc: &Document) -> Document {
    let mut peer = Document::new(Schema::builtin());
    peer.import(&doc.export_snapshot().unwrap()).unwrap();
    peer
}

fn sync(a: &mut Document, b: &mut Document) {
    let for_b = a.export_updates(&b.version()).unwrap();
    let for_a = b.export_updates(&a.version()).unwrap();
    b.import(&for_b).unwrap();
    a.import(&for_a).unwrap();
}

fn exchange(a: &mut Editor, b: &mut Editor) {
    let for_a = b.document().export_updates(&a.document().version());
    let for_b = a.document().export_updates(&b.document().version());
    a.import(&for_a.unwrap()).unwrap();
    b.import(&for_b.unwrap()).unwrap();
}

fn caret(ed: &mut Editor, cell: BlockId, offset: usize) {
    ed.set_selection(Some(Selection::caret(Pos::new(cell, offset))))
        .unwrap();
}

#[test]
fn a_table_is_rows_by_columns_and_a_cell_is_one_of_each() {
    let (mut doc, table) = with_table(2, 3);
    let (rows, columns) = grid(&doc, table);
    assert_eq!((rows.len(), columns.len()), (2, 3));
    assert_eq!(
        outline(&doc, None),
        "paragraph \"before\"\ntable\n  table_row | \"\" | \"\" | \"\"\n  table_row | \"\" | \"\" | \"\"\nparagraph \"after\"\n"
    );
    // Rows are blocks. Cells are found by row and column, and listed nowhere.
    assert_eq!(doc.blocks().len(), 5);
    assert_eq!(doc.children(Some(rows[0])).unwrap(), []);
    let cell = doc.cell(rows[1], columns[2]).unwrap();
    assert!(doc.contains(cell));
    assert_eq!(cell.column(), Some(columns[2]));
    assert_eq!(doc.parent(cell).unwrap(), Some(rows[1]));
    assert_eq!(doc.block_type(cell).unwrap(), "table_cell");
    assert_eq!(doc.index_of(cell).unwrap(), 2);
    assert_eq!(doc.children(Some(cell)).unwrap(), []);
    assert_eq!(cell.to_string().parse::<BlockId>().unwrap(), cell);
    assert_eq!(
        columns[2].to_string().parse::<ColumnId>().unwrap(),
        columns[2]
    );

    // A cell takes text like any block, and has none until then.
    assert_eq!(doc.text(cell).unwrap(), "");
    doc.insert_text(cell, 0, "héllo").unwrap();
    doc.mark(cell, 0..3, "bold", true).unwrap();
    doc.delete_text(cell, 4..6).unwrap();
    assert_eq!(doc.text(cell).unwrap(), "hél");
    assert_eq!(doc.spans(cell).unwrap().len(), 2);
    assert!(matches!(
        doc.insert_text(cell, 9, "x"),
        Err(Error::OutOfBounds { .. })
    ));
}

#[test]
fn rows_and_columns_come_go_and_move() {
    let (mut doc, table) = with_table(2, 2);
    let (rows, columns) = grid(&doc, table);
    for (row, name) in rows.iter().zip(["a", "b"]) {
        for (column, number) in columns.iter().zip(["1", "2"]) {
            let cell = doc.cell(*row, *column).unwrap();
            doc.insert_text(cell, 0, &format!("{name}{number}"))
                .unwrap();
        }
    }
    let shown = |doc: &Document| {
        let lines: Vec<_> = outline(doc, None).lines().map(str::to_owned).collect();
        lines[2..lines.len() - 1]
            .join(" /")
            .replace("  table_row", "")
    };
    assert_eq!(shown(&doc), " | \"a1\" | \"a2\" / | \"b1\" | \"b2\"");

    let middle = doc.insert_column(table, 1).unwrap();
    let last = doc.insert_row(table, 2).unwrap();
    doc.insert_text(doc.cell(last, middle).unwrap(), 0, "new")
        .unwrap();
    assert_eq!(
        shown(&doc),
        " | \"a1\" | \"\" | \"a2\" / | \"b1\" | \"\" | \"b2\" / | \"\" | \"new\" | \"\""
    );
    doc.move_column(table, columns[0], 2).unwrap();
    doc.move_block(last, Some(table), 0).unwrap();
    assert_eq!(
        shown(&doc),
        " | \"new\" | \"\" | \"\" / | \"\" | \"a2\" | \"a1\" / | \"\" | \"b2\" | \"b1\""
    );
    doc.remove_column(table, middle).unwrap();
    doc.remove_block(rows[0]).unwrap();
    assert_eq!(shown(&doc), " | \"\" | \"\" / | \"b2\" | \"b1\"");
    // A cell goes with its row or its column.
    assert!(doc.contains(doc.cell(rows[1], columns[0]).unwrap()));
    assert!(doc.cell(rows[1], middle).is_err());
    assert!(doc.cell(rows[0], columns[0]).is_err());
}

#[test]
fn a_table_is_only_shaped_as_a_table() {
    let (mut doc, table) = with_table(1, 1);
    let (rows, columns) = grid(&doc, table);
    let cell = doc.cell(rows[0], columns[0]).unwrap();
    let before = doc.blocks()[0];
    let part = |result: lotype_core::Result<()>| matches!(result, Err(Error::TablePart));

    assert!(part(
        doc.insert_block(Some(table), 0, "paragraph").map(drop)
    ));
    assert!(part(doc.insert_block(None, 0, "table_row").map(drop)));
    assert!(part(doc.insert_block(None, 0, "table_cell").map(drop)));
    assert!(part(doc.move_block(before, Some(table), 0)));
    assert!(part(doc.move_block(rows[0], None, 0)));
    assert!(part(doc.remove_block(cell)));
    assert!(part(doc.move_block(cell, None, 0)));
    assert!(part(doc.set_block_type(table, "paragraph")));
    assert!(part(doc.set_block_type(rows[0], "paragraph")));
    assert!(part(doc.set_block_type(cell, "heading")));
    assert!(part(doc.set_block_type(before, "table")));
    assert!(part(doc.split_block(Pos::new(cell, 0)).map(drop)));
    assert!(part(
        doc.delete_range(Pos::new(before, 0), Pos::new(cell, 0))
            .map(drop)
    ));
    assert!(doc.insert_block(Some(rows[0]), 0, "paragraph").is_err());
    assert!(doc.insert_block(Some(cell), 0, "paragraph").is_err());
    assert!(matches!(doc.columns(before), Err(Error::NotATable)));
    assert!(matches!(doc.insert_row(before, 0), Err(Error::NotATable)));
    assert_eq!(doc.merge_backward(cell).unwrap(), None);
    // The block after a table does not take a row away.
    let after = *doc.blocks().last().unwrap();
    assert_eq!(doc.merge_backward(after).unwrap(), None);
    assert_eq!(doc.children(Some(table)).unwrap().len(), 1);
    // A range across the whole table takes it out as a whole.
    doc.delete_range(Pos::new(before, 6), Pos::new(after, 0))
        .unwrap();
    assert_eq!(outline(&doc, None), "paragraph \"beforeafter\"\n");
}

#[test]
fn a_row_and_a_column_added_at_once_meet_in_a_cell_that_merges() {
    let (mut a, table) = with_table(2, 2);
    let mut b = peer(&a);
    let row = a.insert_row(table, 2).unwrap();
    let column = b.insert_column(table, 2).unwrap();
    sync(&mut a, &mut b);
    assert_eq!(outline(&a, None), outline(&b, None));
    let widths: Vec<_> = outline(&a, None)
        .lines()
        .map(|line| line.matches('|').count())
        .collect();
    assert_eq!(widths, [0, 0, 3, 3, 3, 0]);

    // Neither of them made the cell where the two meet.
    let cell = a.cell(row, column).unwrap();
    a.insert_text(cell, 0, "from a").unwrap();
    b.insert_text(cell, 0, "from b").unwrap();
    sync(&mut a, &mut b);
    assert_eq!(a.text(cell).unwrap(), b.text(cell).unwrap());
    let text = a.text(cell).unwrap();
    assert!(
        text.contains("from a") && text.contains("from b"),
        "{text:?}"
    );
}

#[test]
fn structure_changed_at_once_stays_a_grid() {
    let (mut a, table) = with_table(2, 2);
    let mut b = peer(&a);
    let (rows, columns) = grid(&a, table);
    a.remove_column(table, columns[0]).unwrap();
    a.insert_row(table, 0).unwrap();
    a.move_column(table, columns[1], 0).unwrap();
    let doomed = b.cell(rows[0], columns[0]).unwrap();
    b.insert_text(doomed, 0, "gone with its column").unwrap();
    b.remove_column(table, columns[0]).unwrap();
    b.insert_column(table, 0).unwrap();
    b.remove_block(rows[1]).unwrap();
    sync(&mut a, &mut b);
    assert_eq!(outline(&a, None), outline(&b, None));
    assert_eq!(a.columns(table).unwrap(), b.columns(table).unwrap());
    assert_eq!(a.columns(table).unwrap().len(), 2);
    assert_eq!(a.children(Some(table)).unwrap().len(), 2);
}

#[test]
fn typing_in_a_cell_is_typing() {
    let (doc, table) = with_table(1, 2);
    let (rows, columns) = grid(&doc, table);
    let cell = doc.cell(rows[0], columns[0]).unwrap();
    let mut ed = Editor::new(doc);
    ed.take_changes();
    caret(&mut ed, cell, 0);
    ed.insert_text("hi").unwrap();
    ed.insert_text(" **there**").unwrap();
    assert_eq!(
        outline(ed.document(), ed.selection()),
        "paragraph \"before\"\ntable\n  table_row | \"hi [bold|there]<a>\" | \"\"\nparagraph \"after\"\n"
    );
    assert!(ed.take_changes().blocks.contains(&cell));
    // Enter is a line break, and there is nothing to join with.
    ed.split_block().unwrap();
    assert_eq!(ed.document().text(cell).unwrap(), "hi there\n");
    caret(&mut ed, cell, 0);
    ed.delete_backward().unwrap();
    assert_eq!(ed.document().text(cell).unwrap(), "hi there\n");
    assert_eq!(ed.slash_query(), None);
    ed.set_block("heading", &[]).unwrap();
    ed.indent().unwrap();
    assert_eq!(ed.document().block_type(cell).unwrap(), "table_cell");

    while ed.undo().unwrap() {}
    assert_eq!(ed.document().text(cell).unwrap(), "");
    while ed.redo().unwrap() {}
    assert_eq!(ed.document().text(cell).unwrap(), "hi there\n");
}

#[test]
fn undoing_the_first_typing_in_a_cell_keeps_what_a_peer_typed_there() {
    let (doc, table) = with_table(1, 1);
    let (rows, columns) = grid(&doc, table);
    let cell = doc.cell(rows[0], columns[0]).unwrap();
    let mut a = Editor::new(doc);
    let mut b = Editor::new(peer(a.document()));
    caret(&mut a, cell, 0);
    caret(&mut b, cell, 0);
    a.insert_text("a").unwrap();
    b.insert_text("b").unwrap();
    exchange(&mut a, &mut b);
    assert_eq!(a.document().text(cell).unwrap().len(), 2);

    assert!(a.undo().unwrap());
    exchange(&mut a, &mut b);
    assert_eq!(a.document().text(cell).unwrap(), "b");
    assert_eq!(b.document().text(cell).unwrap(), "b");
}

#[test]
fn rows_and_columns_undo_and_redo_as_the_same_ones() {
    let (doc, table) = with_table(1, 1);
    let mut ed = Editor::new(doc);
    let mut made = None;
    ed.apply(|doc, _| {
        let row = doc.insert_row(table, 1)?;
        let column = doc.insert_column(table, 1)?;
        doc.insert_text(doc.cell(row, column)?, 0, "corner")?;
        made = Some((row, column));
        Ok(None)
    })
    .unwrap();
    let (row, column) = made.unwrap();
    let full = outline(ed.document(), None);
    assert!(full.contains("table_row | \"\" | \"corner\""));

    assert!(ed.undo().unwrap());
    assert_eq!(grid(ed.document(), table).1.len(), 1);
    assert!(!ed.document().contains(row));
    assert!(ed.redo().unwrap());
    assert_eq!(outline(ed.document(), None), full);
    assert_eq!(
        ed.document().cell(row, column).unwrap().column(),
        Some(column)
    );

    // Removing a column keeps what was in it for undo.
    ed.apply(|doc, _| doc.remove_column(table, column).map(|()| None))
        .unwrap();
    assert!(!outline(ed.document(), None).contains("corner"));
    assert!(ed.undo().unwrap());
    assert_eq!(outline(ed.document(), None), full);
}

#[test]
fn selected_cells_are_emptied_together() {
    let (mut doc, table) = with_table(3, 3);
    let (rows, columns) = grid(&doc, table);
    for row in &rows {
        for column in &columns {
            doc.insert_text(doc.cell(*row, *column).unwrap(), 0, "x")
                .unwrap();
        }
    }
    let corner = |row: usize, column: usize| doc.cell(rows[row], columns[column]).unwrap();
    let (anchor, head) = (corner(2, 1), corner(1, 2));
    let mut ed = Editor::new(doc);
    ed.set_selection(Some(Selection::Cells { anchor, head }))
        .unwrap();
    ed.delete_selection().unwrap();
    let emptied = "table\n  table_row | \"x\" | \"x\" | \"x\"\n  table_row | \"x\" | *\"\" | *\"\"\n  table_row | \"x\" | *\"\" | *\"\"\n";
    assert!(outline(ed.document(), ed.selection()).contains(emptied));
    assert!(ed.undo().unwrap());
    assert!(!outline(ed.document(), None).contains("\"\""));

    // The corners must be cells of one table.
    let other = ed.document().blocks()[0];
    let bad = Selection::Cells {
        anchor,
        head: other,
    };
    assert!(ed.set_selection(Some(bad)).is_err());
    // A text range may not run from a cell into another block.
    let range = Selection::Text {
        anchor: Pos::new(anchor, 0),
        head: Pos::new(other, 0),
    };
    ed.set_selection(Some(range)).unwrap();
    assert!(matches!(ed.insert_text("x"), Err(Error::TablePart)));
}
