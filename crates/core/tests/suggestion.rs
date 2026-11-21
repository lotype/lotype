use std::collections::HashSet;

use lotype_core::test_support::{editor, outline};
use lotype_core::{
    Editor, Error, Schema, SlashItem, blocks, default_slash_items, filter_slash_items, p,
};

fn shown(ed: &Editor) -> String {
    outline(ed.document(), ed.selection())
}

fn item(key: &str) -> SlashItem {
    let items = default_slash_items();
    items.into_iter().find(|item| item.key == key).unwrap()
}

#[test]
fn the_default_items_are_distinct_and_fit_the_built_in_schema() {
    let items = default_slash_items();
    let keys: HashSet<_> = items.iter().map(|item| item.key.as_str()).collect();
    assert_eq!(keys.len(), items.len());
    let schema = Schema::builtin();
    for item in &items {
        assert!(
            schema.block_spec(&item.block_type).is_some(),
            "{}",
            item.key
        );
    }
}

#[test]
fn a_query_finds_items_by_title_or_alias_in_menu_order() {
    let items = default_slash_items();
    let keys = |query: &str| {
        let found = filter_slash_items(&items, query);
        found
            .into_iter()
            .map(|item| item.key.as_str())
            .collect::<Vec<_>>()
    };
    assert_eq!(keys("").len(), items.len());
    assert_eq!(keys("head"), ["heading_1", "heading_2", "heading_3"]);
    assert_eq!(keys("H2"), ["heading_2"]);
    assert_eq!(
        keys("LIST"),
        ["bullet_list", "numbered_list", "check_list", "toggle_list"]
    );
    assert_eq!(keys("todo"), ["check_list"]);
    assert_eq!(keys("zzz"), Vec::<&str>::new());
}

#[test]
fn the_query_is_what_follows_a_slash_that_starts_a_word() {
    let query = |blocks| {
        let ed = editor(blocks);
        ed.slash_query().map(|query| (query.range, query.query))
    };
    assert_eq!(query(blocks![p!["/<a>"]]), Some((0..1, String::new())));
    assert_eq!(
        query(blocks![p!["/he<a>ad"]]),
        Some((0..3, "he".to_owned()))
    );
    assert_eq!(query(blocks![p!["é /q<a>"]]), Some((3..5, "q".to_owned())));
    let code = lotype_core::test_support::block("code_block", vec!["ls /usr<a>".into()]);
    for none in [
        blocks![code],
        blocks![p!["a/x<a>"]],
        blocks![p!["/a b<a>"]],
        blocks![p!["<a>/a"]],
        blocks![p!["/<a>a<b>"]],
        blocks![p!["plain<a>"]],
    ] {
        assert_eq!(query(none), None);
    }
}

#[test]
fn an_item_takes_the_place_of_an_emptied_block() {
    let mut ed = editor(blocks![p!["/h2<a>"], p!["after"]]);
    ed.accept_slash_item(&item("heading_2")).unwrap();
    assert_eq!(shown(&ed), "heading level=2 \"<a>\"\nparagraph \"after\"\n");
    // One step, which gives the query back.
    assert!(ed.undo().unwrap());
    assert_eq!(shown(&ed), "paragraph \"/h2<a>\"\nparagraph \"after\"\n");
    assert!(!ed.can_undo());
}

#[test]
fn an_item_goes_below_a_block_that_holds_something() {
    let mut ed = editor(blocks![p!["hello /quo<a>", p!["child"]], p!["after"]]);
    ed.accept_slash_item(&item("quote")).unwrap();
    assert_eq!(
        shown(&ed),
        "paragraph \"hello \"\n  paragraph \"child\"\nquote \"<a>\"\nparagraph \"after\"\n"
    );

    // A heading takes no children, so it cannot take this block's place.
    let mut ed = editor(blocks![p!["/h<a>", p!["child"]]]);
    ed.accept_slash_item(&item("heading_1")).unwrap();
    assert_eq!(
        shown(&ed),
        "paragraph \"\"\n  paragraph \"child\"\nheading level=1 \"<a>\"\n"
    );
}

#[test]
fn the_caret_goes_where_text_can_go_next_to_a_block_without_text() {
    let mut ed = editor(blocks![p!["/div<a>"]]);
    ed.accept_slash_item(&item("divider")).unwrap();
    assert_eq!(shown(&ed), "divider\nparagraph \"<a>\"\n");

    let mut ed = editor(blocks![p!["text /alert<a>"]]);
    ed.accept_slash_item(&item("alert")).unwrap();
    assert_eq!(
        shown(&ed),
        "paragraph \"text \"\nalert kind=note\n  paragraph \"<a>\"\n"
    );
}

#[test]
fn accepting_needs_a_query() {
    let mut ed = editor(blocks![p!["plain<a>"]]);
    let result = ed.accept_slash_item(&item("quote"));
    assert!(matches!(result, Err(Error::NoSelection)));
    assert_eq!(shown(&ed), "paragraph \"plain<a>\"\n");
}
