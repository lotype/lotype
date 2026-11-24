use lotype_core::test_support::{editor, outline};
use lotype_core::{Editor, Error, Selection, blocks, bullet, h1, p};

fn shown(ed: &Editor) -> String {
    outline(ed.document(), ed.selection())
}

#[test]
fn markdown_typed_in_a_nest_stays_text() {
    for typed in ["# ", "> ", "- ", "1. ", "[] ", "```", "---"] {
        let mut ed = editor(blocks![bullet!["item", p!["<a>"]]]);
        ed.insert_text(typed).unwrap();
        assert_eq!(
            shown(&ed),
            format!("bullet_list_item \"item\"\n  paragraph \"{typed}<a>\"\n")
        );
    }
    // Marks still are marks.
    let mut ed = editor(blocks![bullet!["item", p!["<a>"]]]);
    ed.insert_text("**b**").unwrap();
    assert_eq!(
        shown(&ed),
        "bullet_list_item \"item\"\n  paragraph \"[bold|b]<a>\"\n"
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

#[test]
fn a_heading_markdown_nests_is_read_as_a_paragraph() {
    let mut doc = lotype_core::Document::new(lotype_core::Schema::builtin());
    doc.insert_markdown(
        None,
        0,
        "# Top\n\n- item\n\n  ## Inside\n\n> [!NOTE]\n> ### Note\n",
    )
    .unwrap();
    assert_eq!(
        outline(&doc, None),
        "heading level=1 \"Top\"\nbullet_list_item \"item\"\n  paragraph \"Inside\"\nalert kind=note\n  paragraph \"Note\"\n"
    );
}
