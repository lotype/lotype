//! What a review found, each as it went wrong.

use lotype_core::test_support::{assert_editor, block, build, editor, outline};
use lotype_core::{
    BlockSpec, Content, Document, Editor, Error, Pos, Schema, Selection, blocks, bullet, divider, p,
};

fn peer(doc: &Document) -> Document {
    let mut peer = Document::new(Schema::builtin());
    peer.import(&doc.export_snapshot().unwrap()).unwrap();
    peer
}

fn sync(a: &mut Document, b: &mut Document) {
    a.import(&b.export_snapshot().unwrap()).unwrap();
    b.import(&a.export_snapshot().unwrap()).unwrap();
}

#[test]
fn enter_on_a_range_that_ends_in_a_quote_splits_where_it_starts() {
    let quote = block("quote", vec!["b<b>c".into()]);
    let mut ed = editor(blocks![p!["a<a>x"], quote]);
    ed.split_block().unwrap();
    assert_editor(&ed, blocks![p!["a"], p!["<a>c"]]);
}

#[test]
fn delete_in_an_empty_last_block_leaves_a_divider_before_it() {
    let mut ed = editor(blocks![p!["a"], divider![], p!["<a>"]]);
    ed.delete_forward().unwrap();
    assert_editor(&ed, blocks![p!["a"], divider![], p!["<a>"]]);
}

#[test]
fn an_alert_goes_when_its_last_block_moves_out() {
    let tip = || block("alert", vec![p!["<a>x"].into()]).attr("kind", "tip");
    let mut ed = editor(blocks![tip(), p!["after"]]);
    ed.outdent().unwrap();
    assert_editor(&ed, blocks![p!["<a>x"], p!["after"]]);

    let mut ed = editor(blocks![tip(), p!["after"]]);
    let inside = ed.document().blocks()[1];
    ed.set_selection(Some(Selection::Blocks(vec![inside])))
        .unwrap();
    ed.move_blocks(false).unwrap();
    assert_eq!(
        outline(ed.document(), None),
        "paragraph \"x\"\nparagraph \"after\"\n"
    );
}

#[test]
fn backspace_takes_an_input_rule_back_only_right_after_it() {
    let mut ed = editor(blocks![p!["a"], p!["<a>"]]);
    ed.insert_text("# ").unwrap();
    let (first, heading) = (ed.document().blocks()[0], ed.document().blocks()[1]);
    ed.set_selection(Some(Selection::caret(Pos::new(first, 0))))
        .unwrap();
    ed.set_selection(Some(Selection::caret(Pos::new(heading, 0))))
        .unwrap();
    ed.delete_backward().unwrap();
    assert_editor(&ed, blocks![p!["a"], p!["<a>"]]);
}

#[test]
fn a_block_a_merge_left_with_blocks_in_it_stays_when_they_leave() {
    let (mut a, _) = build(blocks![p![""], p!["x"]]);
    let mut b = peer(&a);
    let (holder, x) = (a.blocks()[0], a.blocks()[1]);
    a.indent_block(x).unwrap();
    b.set_block_type(holder, "image").unwrap();
    sync(&mut a, &mut b);
    // The heading comes out of the image, which is no alert to take away.
    a.set_block_type(x, "heading").unwrap();
    assert_eq!(outline(&a, None), "image\nheading \"x\"\n");
}

#[test]
fn a_heading_is_not_inserted_into_a_block() {
    let (mut doc, _) = build(blocks![bullet!["item"]]);
    let item = doc.blocks()[0];
    let nested = doc.insert_block(Some(item), 0, "heading");
    assert!(matches!(nested, Err(Error::TopLevelOnly(_))));
}

#[test]
fn a_paste_that_splits_a_checked_item_leaves_both_halves_checked() {
    let checked = block("check_list_item", vec!["ab<a>cd".into()]).attr("checked", true);
    let mut ed = editor(blocks![checked]);
    ed.paste_markdown("x\n\n---").unwrap();
    assert_eq!(
        outline(ed.document(), None),
        "check_list_item checked=true \"abx\"\ndivider\ncheck_list_item checked=true \"cd\"\n"
    );
}

#[test]
fn copied_cells_paste_into_a_cell_as_their_text() {
    let mut doc = Document::new(Schema::builtin());
    let table = doc.insert_table(None, 0, 2, 2).unwrap();
    let (rows, columns) = (
        doc.children(Some(table)).unwrap(),
        doc.columns(table).unwrap(),
    );
    let cell = |row: usize, column: usize| doc.cell(rows[row], columns[column]).unwrap();
    let (first, second, target) = (cell(0, 0), cell(0, 1), cell(1, 1));
    doc.insert_text(first, 0, "a").unwrap();
    doc.insert_text(second, 0, "b").unwrap();
    let copied = doc.slice_cells(first, second).unwrap();
    let mut ed = Editor::new(doc);
    ed.set_selection(Some(Selection::caret(Pos::new(target, 0))))
        .unwrap();
    ed.paste_document(&copied).unwrap();
    assert_eq!(ed.document().text(target).unwrap(), "a\tb");
}

#[test]
fn a_copy_goes_through_a_block_a_merge_gave_blocks_it_takes_none_of() {
    let (mut a, _) = build(blocks![p!["code"], p!["child"]]);
    let mut b = peer(&a);
    let (holder, child) = (a.blocks()[0], a.blocks()[1]);
    a.indent_block(child).unwrap();
    b.set_block_type(holder, "code_block").unwrap();
    sync(&mut a, &mut b);
    let slice = a.slice(Pos::new(holder, 0), Pos::new(child, 5)).unwrap();
    assert_eq!(
        outline(&slice, None),
        "code_block \"code\"\nparagraph \"child\"\n"
    );
}

#[test]
fn a_table_is_left_out_where_the_schema_lacks_its_rows() {
    let mut source = Document::new(Schema::builtin());
    source.insert_table(None, 0, 1, 1).unwrap();
    let text = source.insert_block(None, 1, "paragraph").unwrap();
    source.insert_text(text, 0, "after").unwrap();
    let lacking = Schema::new()
        .block(BlockSpec::new("paragraph", Content::Inline))
        .block(BlockSpec::new("table", Content::None).with_children());
    let mut doc = Document::new(lacking);
    doc.insert_document(None, 0, &source).unwrap();
    assert_eq!(outline(&doc, None), "paragraph \"after\"\n");
}
