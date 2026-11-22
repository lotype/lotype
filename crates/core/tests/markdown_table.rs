use lotype_core::test_support::{editor, outline};
use lotype_core::{
    BlockSpec, Content, Document, Editor, MAX_DEPTH, Pos, Schema, Selection, blocks,
    default_slash_items, p,
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

#[test]
fn a_table_that_cannot_be_made_is_read_as_what_is_in_it() {
    // No room below it for rows.
    let prefix = "> ".repeat(MAX_DEPTH - 1);
    let deep = format!("{prefix}| a | b |\n{prefix}| --- | --- |\n{prefix}| c | d |\n");
    let doc = read(&deep);
    let texts: Vec<_> = doc
        .blocks()
        .iter()
        .filter_map(|b| doc.text(*b).ok())
        .collect();
    assert_eq!(
        texts.iter().filter(|t| !t.is_empty()).count(),
        4,
        "{texts:?}"
    );

    // A schema with some of the types of a table, but not all.
    for lacking in ["table", "table_row", "table_cell"] {
        let mut schema = Schema::new().block(BlockSpec::new("paragraph", Content::Inline));
        for name in ["table", "table_row", "table_cell"] {
            if name != lacking {
                schema = schema.block(BlockSpec::new(name, Content::None).with_children());
            }
        }
        let mut doc = Document::new(schema);
        doc.insert_markdown(None, 0, TABLE).unwrap();
        let texts: Vec<_> = doc.blocks().iter().map(|b| doc.text(*b).unwrap()).collect();
        assert_eq!(
            texts,
            ["Name", "Role", "Ada | Lovelace", "code", "Link"],
            "{lacking}"
        );
    }
}

#[test]
fn lines_of_bars_and_dashes_in_a_paragraph_stay_text() {
    for text in [
        "a\n| x\n|-|",
        "a\n|x|\n|---|---|",
        "a\n||\n:-|-",
        "| a |\n| --- |",
    ] {
        for block_type in ["paragraph", "bullet_list_item"] {
            let mut doc = Document::new(Schema::builtin());
            let block = doc.insert_block(None, 0, block_type).unwrap();
            doc.insert_text(block, 0, text).unwrap();
            let back = read(&doc.to_markdown());
            assert_eq!(shown(&back), shown(&doc), "as {:?}", doc.to_markdown());
        }
    }
}

#[test]
fn a_task_keeps_its_box_before_a_block() {
    for first_child in ["table", "code_block", "none"] {
        let mut doc = Document::new(Schema::builtin());
        let item = doc.insert_block(None, 0, "check_list_item").unwrap();
        doc.set_attr(item, "checked", true).unwrap();
        match first_child {
            "table" => drop(doc.insert_table(Some(item), 0, 1, 2).unwrap()),
            "code_block" => {
                let code = doc.insert_block(Some(item), 0, "code_block").unwrap();
                doc.insert_text(code, 0, "x").unwrap();
            }
            _ => {}
        }
        let back = read(&doc.to_markdown());
        assert_eq!(shown(&back), shown(&doc), "as {:?}", doc.to_markdown());
    }
}

#[test]
fn the_slash_menu_inserts_a_table_wherever_it_inserts_a_block() {
    // A peer whose headings take children put a paragraph in one.
    let theirs =
        Schema::builtin().block(BlockSpec::new("heading", Content::Inline).with_children());
    let mut source = Document::new(theirs);
    let heading = source.insert_block(None, 0, "heading").unwrap();
    let inside = source.insert_block(Some(heading), 0, "paragraph").unwrap();
    source.insert_text(inside, 0, "text /x").unwrap();
    let mut doc = Document::new(Schema::builtin());
    doc.import(&source.export_snapshot().unwrap()).unwrap();
    let mut ed = Editor::new(doc);
    ed.set_selection(Some(Selection::caret(Pos::new(inside, 7))))
        .unwrap();
    let items = default_slash_items();
    let table = items.iter().find(|item| item.key == "table").unwrap();
    ed.accept_slash_item(table).unwrap();
    assert!(outline(ed.document(), None).contains("  table\n    table_row"));
}
