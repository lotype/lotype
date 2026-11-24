use lotype_core::test_support::{editor, outline};
use lotype_core::{Document, Editor, Schema, Selection, blocks, p};

fn shown(ed: &Editor) -> String {
    outline(ed.document(), ed.selection())
}

/// Selects the blocks at `indices`, in document order, whole.
fn select(ed: &mut Editor, indices: &[usize]) {
    let all = ed.document().blocks();
    let chosen = indices.iter().map(|at| all[*at]).collect();
    ed.set_selection(Some(Selection::Blocks(chosen))).unwrap();
}

#[test]
fn blocks_move_past_their_siblings_and_stop_at_the_ends() {
    let mut ed = editor(blocks![p!["a"], p!["b"], p!["c"]]);
    select(&mut ed, &[1]);
    ed.move_blocks(true).unwrap();
    assert_eq!(
        shown(&ed),
        "* paragraph \"b\"\nparagraph \"a\"\nparagraph \"c\"\n"
    );
    ed.move_blocks(true).unwrap();
    assert_eq!(
        shown(&ed),
        "* paragraph \"b\"\nparagraph \"a\"\nparagraph \"c\"\n"
    );
    ed.move_blocks(false).unwrap();
    ed.move_blocks(false).unwrap();
    assert_eq!(
        shown(&ed),
        "paragraph \"a\"\nparagraph \"c\"\n* paragraph \"b\"\n"
    );
    // One undo step each.
    ed.undo().unwrap();
    assert_eq!(
        shown(&ed),
        "paragraph \"a\"\n* paragraph \"b\"\nparagraph \"c\"\n"
    );
}

#[test]
fn blocks_go_into_a_sibling_with_children_and_out_of_their_parent() {
    let mut ed = editor(blocks![p!["a", p!["child"]], p!["b"]]);
    select(&mut ed, &[2]);
    ed.move_blocks(true).unwrap();
    assert_eq!(
        shown(&ed),
        "paragraph \"a\"\n  paragraph \"child\"\n  * paragraph \"b\"\n"
    );
    ed.move_blocks(true).unwrap();
    ed.move_blocks(true).unwrap();
    assert_eq!(
        shown(&ed),
        "* paragraph \"b\"\nparagraph \"a\"\n  paragraph \"child\"\n"
    );
    // Down goes in at the start of the next sibling's children.
    ed.move_blocks(false).unwrap();
    assert_eq!(
        shown(&ed),
        "paragraph \"a\"\n  * paragraph \"b\"\n  paragraph \"child\"\n"
    );
}

#[test]
fn selected_blocks_move_together_in_their_order() {
    let mut ed = editor(blocks![p!["a"], p!["b"], p!["c"], p!["d"]]);
    select(&mut ed, &[1, 2]);
    ed.move_blocks(false).unwrap();
    assert_eq!(
        shown(&ed),
        "paragraph \"a\"\nparagraph \"d\"\n* paragraph \"b\"\n* paragraph \"c\"\n"
    );
    ed.move_blocks(true).unwrap();
    ed.move_blocks(true).unwrap();
    assert_eq!(
        shown(&ed),
        "* paragraph \"b\"\n* paragraph \"c\"\nparagraph \"a\"\nparagraph \"d\"\n"
    );
}

#[test]
fn a_block_goes_past_a_table_and_not_into_its_rows() {
    let mut doc = Document::new(Schema::builtin());
    let first = doc.insert_block(None, 0, "paragraph").unwrap();
    doc.insert_table(None, 1, 1, 1).unwrap();
    let mut ed = Editor::new(doc);
    ed.set_selection(Some(Selection::Blocks(vec![first])))
        .unwrap();
    ed.move_blocks(false).unwrap();
    let top = ed.document().children(None).unwrap();
    assert_eq!(top.last(), Some(&first));
    assert_eq!(ed.document().block_type(top[0]).unwrap(), "table");
}
