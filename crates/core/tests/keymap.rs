use std::collections::HashSet;

use lotype_core::test_support::{editor, outline};
use lotype_core::{Command, Editor, Selection, blocks, default_keymap, divider, h1, p};

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
