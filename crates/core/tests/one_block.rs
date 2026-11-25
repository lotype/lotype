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

#[test]
fn an_alert_kind_is_one_of_five_and_an_unknown_one_reads_as_a_note() {
    use lotype_core::{AlertKind, Error};
    let tip = block("alert", vec![p!["x"].into()]).attr(attr::KIND, "TIP");
    let other = block("alert", vec![p!["y"].into()]).attr(attr::KIND, "rumour");
    let mut ed = editor(blocks![tip, other]);
    let blocks = ed.document().blocks();
    let (tip, other) = (blocks[0], blocks[2]);
    assert_eq!(ed.document().alert_kind(tip), AlertKind::Tip);
    assert_eq!(ed.document().alert_kind(other), AlertKind::Note);
    ed.set_alert_kind(tip, AlertKind::Caution).unwrap();
    assert!(ed.document().to_markdown().starts_with("> [!CAUTION]"));
    // Only an alert has a kind, and only a check list item a box.
    let inside = blocks[1];
    assert!(matches!(
        ed.set_alert_kind(inside, AlertKind::Tip),
        Err(Error::NotOfType("alert"))
    ));
    assert!(matches!(
        ed.set_checked(inside, true),
        Err(Error::NotOfType("check_list_item"))
    ));
    assert_eq!(outline(ed.document(), None).matches("kind=").count(), 2);
}

#[test]
fn a_table_takes_a_color_and_its_rows_and_cells_none() {
    let mut doc = Document::new(Schema::builtin());
    let table = doc.insert_table(None, 0, 2, 2).unwrap();
    let row = doc.children(Some(table)).unwrap()[0];
    let cell = doc.cell(row, doc.columns(table).unwrap()[0]).unwrap();
    let mut ed = Editor::new(doc);
    let red = Some("red".to_owned());
    for (block, colored) in [(table, red), (row, None), (cell, None)] {
        ed.set_block_color(block, BlockColor::Background, Some("red"))
            .unwrap();
        let color = ed.document().block_color(block, BlockColor::Background);
        assert_eq!(color, colored);
    }
}

#[test]
fn numbered_items_count_on_from_where_their_list_starts() {
    let mut doc = Document::new(Schema::builtin());
    doc.insert_markdown(None, 0, "3. a\n4. b\n\n- c\n").unwrap();
    let blocks = doc.blocks();
    assert_eq!(doc.list_number(blocks[0], None), Some(3));
    assert_eq!(doc.list_number(blocks[1], Some(3)), Some(4));
    assert_eq!(
        doc.list_number(blocks[1], Some(999_999_999)),
        Some(999_999_999)
    );
    assert_eq!(doc.list_number(blocks[2], Some(4)), None);
    doc.set_attr(blocks[0], attr::START, -5).unwrap();
    assert_eq!(doc.list_number(blocks[0], None), Some(0));
}

#[test]
fn a_list_starts_where_it_is_told_from_any_of_its_items() {
    use lotype_core::{BlockKind, Error};
    let mut doc = Document::new(Schema::builtin());
    doc.insert_markdown(None, 0, "- x\n\n1. a\n2. b\n").unwrap();
    let blocks = doc.blocks();
    let (bullet, first, second) = (blocks[0], blocks[1], blocks[2]);
    let mut ed = Editor::new(doc);
    let numbers = |ed: &Editor| {
        let doc = ed.document();
        let first_number = doc.list_number(first, None);
        (first_number, doc.list_number(second, first_number))
    };
    ed.set_list_start(second, Some(7)).unwrap();
    assert_eq!(numbers(&ed), (Some(7), Some(8)));
    assert_eq!(
        BlockKind::of(ed.document(), first),
        Some(BlockKind::NumberedListItem { start: Some(7) })
    );
    assert_eq!(ed.document().attr(second, attr::START).unwrap(), None);
    ed.undo().unwrap();
    assert_eq!(numbers(&ed), (Some(1), Some(2)));
    ed.set_list_start(first, Some(4)).unwrap();
    assert_eq!(numbers(&ed), (Some(4), Some(5)));
    ed.set_list_start(first, None).unwrap();
    assert_eq!(numbers(&ed), (Some(1), Some(2)));
    assert!(matches!(
        ed.set_list_start(bullet, Some(2)),
        Err(Error::NotOfType("numbered_list_item"))
    ));
}

#[test]
fn the_block_types_a_toolbar_offers_describe_the_blocks_of_them() {
    use lotype_core::default_block_types;
    let types = default_block_types();
    let keys: Vec<_> = types.iter().map(|item| item.key.as_str()).collect();
    assert_eq!(keys[..2], ["paragraph", "heading_1"]);
    assert!(keys.contains(&"check_list") && keys.contains(&"code_block"));
    // No entry starts a block checked or unchecked: it keeps what it was.
    assert!(
        types
            .iter()
            .all(|item| item.attrs.iter().all(|(key, _)| key == attr::LEVEL))
    );

    let checked = block("check_list_item", vec!["t".into()]).attr(attr::CHECKED, true);
    let mut ed = editor(blocks![h1!["a"], checked]);
    let [heading, item] = ed.document().blocks()[..] else {
        panic!()
    };
    let describes = |ed: &Editor, block, key: &str| {
        let found = types
            .iter()
            .find(|item| item.describes(ed.document(), block));
        found.is_some_and(|item| item.key == key)
    };
    assert!(describes(&ed, heading, "heading_1"));
    assert!(describes(&ed, item, "check_list"));
    // A level a peer wrote out of range is the deepest heading.
    ed.apply(|doc, selection| {
        doc.set_attr(heading, attr::LEVEL, 9)?;
        Ok(selection.cloned())
    })
    .unwrap();
    assert!(describes(&ed, heading, "heading_5"));
}

#[test]
fn every_alert_kind_has_githubs_style_and_an_unknown_one_a_notes() {
    use lotype_core::{AlertKind, alert};
    let kinds: Vec<_> = alert::STYLES.iter().map(|style| style.kind).collect();
    assert_eq!(kinds, alert::KINDS);
    for kind in alert::KINDS {
        assert_eq!(kind.style().kind, kind);
        assert_eq!(AlertKind::parse(&kind.name().to_uppercase()), Some(kind));
    }
    assert_eq!(AlertKind::parse("rumour"), None);
    assert_eq!(AlertKind::Tip.style().title, "Tip");
    let hex = |value: &str| {
        value.len() == 7
            && value.starts_with('#')
            && value[1..].chars().all(|c| c.is_ascii_hexdigit())
    };
    for style in alert::STYLES {
        for color in [style.border, style.color] {
            assert!(color.token.starts_with("--"), "{color:?}");
            assert!(hex(color.light) && hex(color.dark), "{color:?}");
        }
    }
}
