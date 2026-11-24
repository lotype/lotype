use std::collections::HashSet;

use lotype_core::test_support::{block as block_node, editor, outline};
use lotype_core::{
    BlockId, BlockSpec, Command, Content, Document, Editor, Schema, Selection, b, blocks,
    default_keymap, divider, h1, p,
};

fn shown(ed: &Editor) -> String {
    outline(ed.document(), ed.selection())
}

#[test]
fn indent_nests_under_the_block_before_and_outdent_brings_it_back() {
    let mut ed = editor(blocks![p!["a"], p!["b<a>"], p!["c"]]);
    ed.indent().unwrap();
    assert_eq!(
        shown(&ed),
        "paragraph \"a\"\n  paragraph \"b<a>\"\nparagraph \"c\"\n"
    );
    // Nothing before it at this level now, so it stays, and takes no undo step.
    ed.indent().unwrap();
    assert_eq!(
        shown(&ed),
        "paragraph \"a\"\n  paragraph \"b<a>\"\nparagraph \"c\"\n"
    );
    ed.outdent().unwrap();
    assert_eq!(
        shown(&ed),
        "paragraph \"a\"\nparagraph \"b<a>\"\nparagraph \"c\"\n"
    );
    ed.outdent().unwrap();
    assert!(ed.undo().unwrap());
    assert!(ed.undo().unwrap());
    assert!(!ed.can_undo());
    assert_eq!(
        shown(&ed),
        "paragraph \"a\"\nparagraph \"b<a>\"\nparagraph \"c\"\n"
    );
}

#[test]
fn outdent_takes_the_later_siblings_along_so_nothing_changes_place() {
    let mut ed = editor(blocks![p!["top", p!["x"], p!["y<a>", p!["own"]], p!["z"]]]);
    ed.outdent().unwrap();
    assert_eq!(
        shown(&ed),
        "paragraph \"top\"\n  paragraph \"x\"\nparagraph \"y<a>\"\n  paragraph \"own\"\n  paragraph \"z\"\n"
    );
}

#[test]
fn blocks_stay_where_the_move_does_not_fit() {
    // A heading takes no children: nothing nests under it, and it cannot
    // take its later siblings along.
    let mut ed = editor(blocks![h1!["title"], p!["b<a>"]]);
    ed.indent().unwrap();
    assert_eq!(
        shown(&ed),
        "heading level=1 \"title\"\nparagraph \"b<a>\"\n"
    );
    let mut ed = editor(blocks![p!["top", h1!["x<a>"], p!["z"]]]);
    ed.outdent().unwrap();
    assert_eq!(
        shown(&ed),
        "paragraph \"top\"\n  heading level=1 \"x<a>\"\n  paragraph \"z\"\n"
    );
    assert!(!ed.can_undo());
}

#[test]
fn a_selection_of_several_blocks_moves_together() {
    let mut ed = editor(blocks![
        p!["a"],
        p!["b<a>", p!["child"]],
        p!["c<b>"],
        p!["d"]
    ]);
    ed.indent().unwrap();
    let nested = "paragraph \"a\"\n  paragraph \"b<a>\"\n    paragraph \"child\"\n  paragraph \"c<b>\"\nparagraph \"d\"\n";
    assert_eq!(shown(&ed), nested);
    ed.outdent().unwrap();
    assert_eq!(
        shown(&ed),
        "paragraph \"a\"\nparagraph \"b<a>\"\n  paragraph \"child\"\nparagraph \"c<b>\"\nparagraph \"d\"\n"
    );
    assert!(ed.undo().unwrap());
    assert_eq!(shown(&ed), nested);
}

#[test]
fn set_block_changes_the_type_and_replaces_the_attributes() {
    let mut ed = editor(blocks![h1!["a<a>"], divider![], p!["b<b>"]]);
    ed.run(&Command::set_block("heading").with("level", 2))
        .unwrap();
    // The divider holds no text, so it does not become a heading.
    assert_eq!(
        shown(&ed),
        "heading level=2 \"a<a>\"\ndivider\nheading level=2 \"b<b>\"\n"
    );
    ed.run(&Command::set_block("check_list_item").with("checked", false))
        .unwrap();
    assert_eq!(
        shown(&ed),
        "check_list_item checked=false \"a<a>\"\ndivider\ncheck_list_item checked=false \"b<b>\"\n"
    );
    ed.set_block("paragraph", &[]).unwrap();
    assert_eq!(
        shown(&ed),
        "paragraph \"a<a>\"\ndivider\nparagraph \"b<b>\"\n"
    );

    let block = ed.document().blocks()[2];
    ed.set_selection(Some(Selection::Blocks(vec![block])))
        .unwrap();
    ed.set_block("quote", &[]).unwrap();
    assert_eq!(ed.document().block_type(block).unwrap(), "quote");

    // A block's own colors stay, as BlockNote keeps them.
    let red = block_node("paragraph", vec!["a<a>".into()]).attr("text_color", "red");
    let mut ed = editor(blocks![red]);
    ed.set_block("heading", &[("level".to_owned(), 1.into())])
        .unwrap();
    assert_eq!(shown(&ed), "heading level=1 text_color=red \"a<a>\"\n");
    // A code block takes no colors.
    ed.set_block("code_block", &[]).unwrap();
    assert_eq!(shown(&ed), "code_block \"a<a>\"\n");
}

#[test]
fn a_pasted_address_becomes_a_link() {
    let mut ed = editor(blocks![p!["see <a>this<b> page"]]);
    ed.paste_text(" https://lotype.dev/docs\n").unwrap();
    assert_eq!(
        shown(&ed),
        "paragraph \"see <a>[link=https://lotype.dev/docs|this]<b> page\"\n"
    );

    let mut ed = editor(blocks![p!["see <a>"]]);
    ed.paste_text("mailto:hi@lotype.dev").unwrap();
    assert_eq!(
        shown(&ed),
        "paragraph \"see [link=mailto:hi@lotype.dev|mailto:hi@lotype.dev]<a>\"\n"
    );
    // Typing goes on outside the link.
    ed.insert_text("!").unwrap();
    assert_eq!(
        ed.document()
            .spans(ed.document().blocks()[0])
            .unwrap()
            .len(),
        3
    );

    for plain in ["just text", "javascript:alert(1)", "https://", "http://a b"] {
        let mut ed = editor(blocks![p!["<a>"]]);
        ed.paste_text(plain).unwrap();
        assert_eq!(shown(&ed), format!("paragraph \"{plain}<a>\"\n"));
    }
}

#[test]
fn the_default_keymap_runs_and_binds_each_key_once() {
    let keymap = default_keymap();
    let keys: HashSet<_> = keymap.iter().map(|binding| binding.key.as_str()).collect();
    assert_eq!(keys.len(), keymap.len());
    let command = |key: &str| {
        let binding = keymap.iter().find(|binding| binding.key == key);
        binding.unwrap().command.clone()
    };

    let mut ed = editor(blocks![p!["a"], p!["<a>bc<b>"]]);
    ed.run(&command("Mod-b")).unwrap();
    ed.run(&command("Mod-Alt-2")).unwrap();
    ed.run(&command("Tab")).unwrap();
    assert_eq!(
        shown(&ed),
        "paragraph \"a\"\n  heading level=2 \"<a>[bold|bc]<b>\"\n"
    );
    ed.run(&command("Mod-Shift-8")).unwrap();
    ed.run(&command("Tab")).unwrap();
    assert_eq!(
        shown(&ed),
        "paragraph \"a\"\n  bullet_list_item \"<a>[bold|bc]<b>\"\n"
    );
    ed.run(&command("Mod-z")).unwrap();
    ed.run(&command("Mod-Shift-z")).unwrap();
    ed.run(&command("Shift-Tab")).unwrap();
    ed.run(&command("Backspace")).unwrap();
    ed.run(&command("Enter")).unwrap();
    assert_eq!(
        shown(&ed),
        "paragraph \"a\"\nbullet_list_item \"\"\nbullet_list_item \"<a>\"\n"
    );
    // Every command names a mark or block type the built-in schema has.
    for binding in &keymap {
        let mut ed = editor(blocks![p!["a"], p!["<a>bc<b>"]]);
        ed.run(&binding.command).unwrap();
    }
}

fn find(ed: &Editor, text: &str) -> BlockId {
    let doc = ed.document();
    let found = |block: &BlockId| doc.text(*block).is_ok_and(|t| t == text);
    doc.blocks().into_iter().find(found).unwrap()
}

#[test]
fn a_block_selection_moves_each_block_once_whatever_its_order() {
    let mut ed = editor(blocks![p!["a", p!["a1"]], p!["b"], p!["c"]]);
    let (b, c) = (find(&ed, "b"), find(&ed, "c"));
    ed.set_selection(Some(Selection::Blocks(vec![c, b, c])))
        .unwrap();
    ed.indent().unwrap();
    assert_eq!(
        outline(ed.document(), None),
        "paragraph \"a\"\n  paragraph \"a1\"\n  paragraph \"b\"\n  paragraph \"c\"\n"
    );

    // A block deep inside a selected one moves with it, not on its own.
    let mut ed = editor(blocks![p!["top", p!["g", p!["m", p!["deep"]]]]]);
    let picked = vec![find(&ed, "deep"), find(&ed, "g")];
    ed.set_selection(Some(Selection::Blocks(picked))).unwrap();
    ed.outdent().unwrap();
    assert_eq!(
        outline(ed.document(), None),
        "paragraph \"top\"\nparagraph \"g\"\n  paragraph \"m\"\n    paragraph \"deep\"\n"
    );
}

#[test]
fn outdent_frees_a_block_that_takes_no_children_from_the_end() {
    let mut ed = editor(blocks![p!["top", h1!["x<a>"], p!["y<b>"]]]);
    ed.outdent().unwrap();
    assert_eq!(
        shown(&ed),
        "paragraph \"top\"\nheading level=1 \"x<a>\"\nparagraph \"y<b>\"\n"
    );
}

#[test]
fn set_block_leaves_what_cannot_become_the_type() {
    let mut ed = editor(blocks![p!["<a>a"], p!["b", p!["child<b>"]]]);
    ed.run(&Command::set_block("heading").with("level", 1))
        .unwrap();
    assert_eq!(
        shown(&ed),
        "heading level=1 \"<a>a\"\nparagraph \"b\"\n  heading level=1 \"child<b>\"\n"
    );

    // A block of a type only another peer knows keeps its type and attributes.
    let theirs = Schema::builtin().block(BlockSpec::new("widget", Content::None));
    let mut source = Document::new(theirs);
    let widget = source.insert_block(None, 0, "widget").unwrap();
    source.set_attr(widget, "src", "keep").unwrap();
    let mut doc = Document::new(Schema::builtin());
    doc.import(&source.export_snapshot().unwrap()).unwrap();
    let mut ed = Editor::new(doc);
    ed.set_selection(Some(Selection::Blocks(vec![widget])))
        .unwrap();
    ed.set_block("page_break", &[]).unwrap();
    assert_eq!(outline(ed.document(), None), "widget src=keep\n");
}

#[test]
fn asking_for_what_a_block_already_is_makes_no_step() {
    let mut ed = editor(blocks![h1!["a<a>"]]);
    ed.insert_text("x").unwrap();
    assert!(ed.undo().unwrap());
    ed.run(&Command::set_block("heading").with("level", 1))
        .unwrap();
    assert!(ed.can_redo());
    assert!(!ed.can_undo());
}

#[test]
fn pasted_text_is_a_step_of_its_own_and_no_rule_reads_it() {
    let mut ed = editor(blocks![p!["<a>"]]);
    ed.paste_text("---").unwrap();
    assert_eq!(shown(&ed), "paragraph \"---<a>\"\n");
    let mut ed = editor(blocks![p!["<a>"]]);
    ed.paste_text("**a** and **b**").unwrap();
    assert_eq!(shown(&ed), "paragraph \"**a** and **b**<a>\"\n");

    let mut ed = editor(blocks![p!["<a>"]]);
    ed.insert_text("typed").unwrap();
    ed.paste_text(" pasted").unwrap();
    ed.insert_text("!").unwrap();
    assert!(ed.undo().unwrap());
    assert!(ed.undo().unwrap());
    assert_eq!(shown(&ed), "paragraph \"typed<a>\"\n");
}

#[test]
fn a_pasted_address_takes_the_marks_typing_would_and_replaces_an_empty_range() {
    let mut ed = editor(blocks![p![b!["ab<a>cd"]]]);
    ed.paste_text("https://x.dev").unwrap();
    assert_eq!(
        shown(&ed),
        "paragraph \"[bold|ab][bold,link=https://x.dev|https://x.dev]<a>[bold|cd]\"\n"
    );

    let mut ed = editor(blocks![p!["a<a>"], p!["<b>b"]]);
    ed.paste_text("https://x.dev").unwrap();
    assert_eq!(
        shown(&ed),
        "paragraph \"a[link=https://x.dev|https://x.dev]<a>b\"\n"
    );
}
