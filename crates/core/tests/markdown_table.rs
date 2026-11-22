use lotype_core::test_support::{editor, outline};
use lotype_core::{
    BlockSpec, Content, Document, Schema, Selection, blocks, default_slash_items, p,
};

fn read(markdown: &str) -> Document {
    let mut doc = Document::new(Schema::builtin());
    doc.insert_markdown(None, 0, markdown).unwrap();
    doc
}

fn shown(doc: &Document) -> String {
    outline(doc, None)
}

const TABLE: &str = "\
| Name | **Role** |
| --- | --- |
| Ada \\| Lovelace | `code` |
| [Link](https://lotype.dev) |  |
";

#[test]
fn a_markdown_table_is_read_into_rows_and_cells() {
    let doc = read(TABLE);
    assert_eq!(
        shown(&doc),
        "table\n  table_row | \"Name\" | \"[bold|Role]\"\n  table_row | \"Ada | Lovelace\" | \"[code|code]\"\n  table_row | \"[link=https://lotype.dev|Link]\" | \"\"\n"
    );
    // Written the same, and read back the same.
    assert_eq!(doc.to_markdown(), TABLE);
    assert_eq!(shown(&read(&doc.to_markdown())), shown(&doc));
}

#[test]
fn rows_of_any_length_make_a_full_grid() {
    let doc = read("| a | b | c |\n|---|---|---|\n| d |\n| e | f | g | h |\n");
    let rows: Vec<_> = shown(&doc)
        .lines()
        .map(|line| line.matches('|').count())
        .collect();
    assert_eq!(rows, [0, 3, 3, 3]);
}

#[test]
fn a_table_keeps_its_place_among_other_blocks() {
    let markdown = "before\n\n- item\n\n  | a | b |\n  | --- | --- |\n  | c | d |\n\n  after in item\n\nafter\n";
    let doc = read(markdown);
    assert_eq!(
        shown(&doc),
        "paragraph \"before\"\nbullet_list_item \"item\"\n  table\n    table_row | \"a\" | \"b\"\n    table_row | \"c\" | \"d\"\n  paragraph \"after in item\"\nparagraph \"after\"\n"
    );
    assert_eq!(doc.to_markdown(), markdown);
}

#[test]
fn what_a_cell_cannot_say_in_markdown_is_written_plainly() {
    let mut doc = Document::new(Schema::builtin());
    let table = doc.insert_table(None, 0, 2, 2).unwrap();
    let (rows, columns) = (
        doc.children(Some(table)).unwrap(),
        doc.columns(table).unwrap(),
    );
    let cell = doc.cell(rows[0], columns[0]).unwrap();
    doc.insert_text(cell, 0, "two\nlines | and *signs*")
        .unwrap();
    let markdown = doc.to_markdown();
    assert_eq!(
        markdown,
        "| two lines \\| and \\*signs\\* |  |\n| --- | --- |\n|  |  |\n"
    );
    let back = read(&markdown);
    let cell = back.cell(
        back.children(back.blocks().first().copied()).unwrap()[0],
        back.columns(back.blocks()[0]).unwrap()[0],
    );
    assert_eq!(back.text(cell.unwrap()).unwrap(), "two lines | and *signs*");

    // Markdown has no table without a row or without a column.
    let mut empty = Document::new(Schema::builtin());
    empty.insert_table(None, 0, 0, 2).unwrap();
    empty.insert_table(None, 1, 2, 0).unwrap();
    assert_eq!(empty.to_markdown(), "");
}

#[test]
fn a_schema_without_tables_keeps_the_text_of_the_cells() {
    let plain = Schema::new().block(BlockSpec::new("paragraph", Content::Inline));
    let mut doc = Document::new(plain);
    doc.insert_markdown(None, 0, TABLE).unwrap();
    let texts: Vec<_> = doc.blocks().iter().map(|b| doc.text(*b).unwrap()).collect();
    assert_eq!(texts, ["Name", "Role", "Ada | Lovelace", "code", "Link"]);
}

#[test]
fn the_slash_menu_inserts_a_table_and_goes_to_its_first_cell() {
    let items = default_slash_items();
    let table = items.iter().find(|item| item.key == "table").unwrap();
    let mut ed = editor(blocks![p!["/tab<a>"], p!["after"]]);
    ed.accept_slash_item(table).unwrap();
    let row = "  table_row | \"\" | \"\" | \"\"\n";
    assert_eq!(
        outline(ed.document(), None),
        format!("table\n{row}{row}{row}paragraph \"after\"\n")
    );
    let Some(Selection::Text { head, .. }) = ed.selection() else {
        panic!()
    };
    assert_eq!(ed.document().index_of(head.block).unwrap(), 0);
    ed.insert_text("first").unwrap();
    assert!(outline(ed.document(), None).contains("table_row | \"first\" | \"\" | \"\""));

    assert!(ed.undo().unwrap());
    assert!(ed.undo().unwrap());
    assert_eq!(
        outline(ed.document(), ed.selection()),
        "paragraph \"/tab<a>\"\nparagraph \"after\"\n"
    );

    // In a block that holds something, the table goes below it.
    let mut ed = editor(blocks![p!["text /table<a>"]]);
    ed.accept_slash_item(table).unwrap();
    assert!(outline(ed.document(), None).starts_with("paragraph \"text \"\ntable\n"));
}
