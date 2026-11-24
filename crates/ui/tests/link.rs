use gpui::{Modifiers, TestAppContext};
use lotype_core::test_support::marked;
use lotype_core::{blocks, p};
use lotype_ui::test_support::EditorTestContext;

fn at(ed: &mut EditorTestContext, selector: &'static str) -> Option<gpui::Point<gpui::Pixels>> {
    ed.settle();
    ed.cx.debug_bounds(selector).map(|bounds| bounds.center())
}

fn click(ed: &mut EditorTestContext, selector: &'static str) {
    let point = at(ed, selector).unwrap_or_else(|| panic!("{selector} shows"));
    ed.cx.simulate_click(point, Modifiers::none());
    ed.settle();
}

#[gpui::test]
fn the_toolbar_links_the_selection_to_an_address(cx: &mut TestAppContext) {
    let mut ed = EditorTestContext::new(cx, blocks![p!["see <a>docs<b> here"]]);
    click(&mut ed, "toolbar-link");
    assert!(at(&mut ed, "link-field").is_some());
    ed.cx.simulate_input("https://lotype.dev");
    ed.keys("enter");
    let docs = marked("link", "https://lotype.dev", vec!["<a>docs<b>".into()]);
    ed.assert(blocks![p!["see ", docs, " here"]]);
    assert!(at(&mut ed, "link-field").is_none());
    // The editor has the keys again.
    ed.keys("right");
    let docs = marked("link", "https://lotype.dev", vec!["docs".into()]);
    ed.assert(blocks![p!["see ", docs, "<a> here"]]);
}

#[gpui::test]
fn escape_leaves_the_text_as_it_was(cx: &mut TestAppContext) {
    let mut ed = EditorTestContext::new(cx, blocks![p!["see <a>docs<b> here"]]);
    click(&mut ed, "toolbar-link");
    ed.cx.simulate_input("https://x.y");
    ed.keys("escape");
    assert!(at(&mut ed, "link-field").is_none());
    ed.assert(blocks![p!["see <a>docs<b> here"]]);
    // The editor has the keys again.
    ed.keys("right");
    ed.assert(blocks![p!["see docs<a> here"]]);
}

#[gpui::test]
fn the_link_toolbar_edits_and_removes_a_link(cx: &mut TestAppContext) {
    let docs = marked("link", "https://a.b", vec!["do<a>cs".into()]);
    let mut ed = EditorTestContext::new(cx, blocks![p!["see ", docs, " here"]]);
    assert!(at(&mut ed, "link-open").is_some());
    click(&mut ed, "link-edit");
    // The field holds the address: Enter keeps it.
    ed.keys("enter");
    let docs = marked("link", "https://a.b", vec!["<a>docs<b>".into()]);
    ed.assert(blocks![p!["see ", docs, " here"]]);
    ed.keys("left right");
    click(&mut ed, "link-edit");
    ed.keys("cmd-a");
    ed.cx.simulate_input("https://c.d");
    ed.keys("enter");
    let docs = marked("link", "https://c.d", vec!["<a>docs<b>".into()]);
    ed.assert(blocks![p!["see ", docs, " here"]]);

    ed.keys("left right");
    click(&mut ed, "link-remove");
    ed.assert(blocks![p!["see d<a>ocs here"]]);
}

#[gpui::test]
fn a_link_to_another_scheme_does_not_open(cx: &mut TestAppContext) {
    let docs = marked("link", "file:///etc/passwd", vec!["do<a>cs".into()]);
    let mut ed = EditorTestContext::new(cx, blocks![p![docs]]);
    assert!(at(&mut ed, "link-toolbar").is_some());
    assert!(at(&mut ed, "link-open").is_none());
}

#[gpui::test]
fn the_field_stays_while_another_app_is_in_front(cx: &mut TestAppContext) {
    let mut ed = EditorTestContext::new(cx, blocks![p!["see <a>docs<b> here"]]);
    ed.cx.update(|window, _| window.activate_window());
    click(&mut ed, "toolbar-link");
    ed.cx.simulate_input("https://x.y");
    ed.cx.deactivate_window();
    assert!(at(&mut ed, "link-field").is_some());
    ed.cx.update(|window, _| window.activate_window());
    ed.keys("enter");
    let docs = marked("link", "https://x.y", vec!["<a>docs<b>".into()]);
    ed.assert(blocks![p!["see ", docs, " here"]]);
}

#[gpui::test]
fn the_slash_menu_hides_the_link_toolbar(cx: &mut TestAppContext) {
    let docs = marked("link", "https://a.b", vec!["docs".into()]);
    let mut ed = EditorTestContext::new(cx, blocks![p!["<a>", docs]]);
    assert!(at(&mut ed, "link-toolbar").is_some());
    ed.type_text("/h");
    assert!(at(&mut ed, "link-toolbar").is_none());
}

#[gpui::test]
fn an_address_for_text_a_peer_deleted_links_nothing(cx: &mut TestAppContext) {
    let mut ed = EditorTestContext::new(cx, blocks![p!["see <a>docs<b> here"]]);
    click(&mut ed, "toolbar-link");
    // A peer deletes the selected text while the field is open.
    let block = ed.editor(|editor| editor.document().blocks()[0]);
    ed.view.update(ed.cx, |view, cx| {
        view.update_editor(cx, |editor| {
            let mut theirs = lotype_core::Document::new(lotype_core::Schema::builtin());
            theirs
                .import(&editor.document().export_snapshot().unwrap())
                .unwrap();
            theirs.delete_text(block, 4..8).unwrap();
            editor.import(&theirs.export_snapshot().unwrap()).unwrap();
        });
    });
    ed.cx.simulate_input("https://lotype.dev");
    ed.keys("enter");
    // What is typed next is no link.
    ed.type_text("x");
    ed.assert(blocks![p!["see x<a> here"]]);
}
