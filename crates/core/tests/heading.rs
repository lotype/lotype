use lotype_core::test_support::{editor, outline};
use lotype_core::{Editor, Error, Selection, blocks, bullet, h1, p};

fn shown(ed: &Editor) -> String {
    outline(ed.document(), ed.selection())
}

#[test]
fn a_heading_typed_in_a_nest_comes_out_where_it_reads() {
    let mut ed = editor(blocks![bullet!["item", p!["<a>"], p!["next"]]]);
    ed.insert_text("# ").unwrap();
    assert_eq!(
        shown(&ed),
        "bullet_list_item \"item\"\nheading level=1 \"<a>\"\nparagraph \"next\"\n"
    );
}

#[test]
fn a_heading_pasted_in_a_nest_comes_out() {
    let mut ed = editor(blocks![bullet!["item", p!["a<a>"]]]);
    ed.paste_markdown("# Title\n\nbody").unwrap();
    assert_eq!(
        shown(&ed),
        "bullet_list_item \"item\"\n  paragraph \"a\"\nheading level=1 \"Title\"\nparagraph \"body<a>\"\n"
    );
}

#[test]
fn a_heading_moves_past_blocks_and_never_into_them() {
    let mut ed = editor(blocks![bullet!["item", p!["child"]], h1!["title"]]);
    let title = ed.document().blocks()[2];
    ed.set_selection(Some(Selection::Blocks(vec![title])))
        .unwrap();
    ed.move_blocks(true).unwrap();
    assert_eq!(
        shown(&ed),
        "* heading level=1 \"title\"\nbullet_list_item \"item\"\n  paragraph \"child\"\n"
    );
    // Nor does a drop or Tab put it in one.
    let item = ed.document().blocks()[1];
    let moved = ed.apply(|doc, selection| {
        doc.move_block(title, Some(item), 0)?;
        Ok(selection.cloned())
    });
    assert!(matches!(moved, Err(Error::TopLevelOnly(_))));
    ed.indent().unwrap();
    assert!(ed.document().parent(title).unwrap().is_none());
}
