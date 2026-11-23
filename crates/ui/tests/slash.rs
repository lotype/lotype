use gpui::TestAppContext;
use lotype_core::test_support::block;
use lotype_core::{blocks, h1, p};
use lotype_ui::test_support::EditorTestContext;

#[gpui::test]
fn a_slash_query_picks_a_block_with_enter(cx: &mut TestAppContext) {
    let mut ed = EditorTestContext::new(cx, blocks![p!["<a>"]]);
    ed.type_text("/quo");
    ed.keys("enter");
    ed.assert(blocks![block("quote", vec!["<a>".into()])]);
}

#[gpui::test]
fn arrows_move_through_the_entries_and_wrap(cx: &mut TestAppContext) {
    let mut ed = EditorTestContext::new(cx, blocks![p!["<a>"]]);
    // Headings 1 to 3 match, and Up from the first goes to the last.
    ed.type_text("/heading");
    ed.keys("down tab");
    ed.assert(blocks![
        block("heading", vec!["<a>".into()]).attr("level", 2)
    ]);
    let mut ed = EditorTestContext::new(cx, blocks![p!["<a>"]]);
    ed.type_text("/heading");
    ed.keys("up enter");
    ed.assert(blocks![
        block("heading", vec!["<a>".into()]).attr("level", 3)
    ]);
}

#[gpui::test]
fn escape_closes_the_menu_until_another_slash(cx: &mut TestAppContext) {
    let mut ed = EditorTestContext::new(cx, blocks![p!["<a>"]]);
    ed.type_text("/h1");
    ed.keys("escape enter");
    // Enter splits the block, as it would without a menu.
    ed.assert(blocks![p!["/h1"], p!["<a>"]]);
    ed.type_text("/h1");
    ed.keys("enter");
    ed.assert(blocks![p!["/h1"], h1!["<a>"]]);
}

#[gpui::test]
fn without_a_match_keys_are_the_editors(cx: &mut TestAppContext) {
    let mut ed = EditorTestContext::new(cx, blocks![p!["<a>"]]);
    ed.type_text("/nothing");
    ed.keys("enter");
    ed.assert(blocks![p!["/nothing"], p!["<a>"]]);
}

#[gpui::test]
fn the_menu_opens_again_for_a_slash_typed_anew(cx: &mut TestAppContext) {
    let mut ed = EditorTestContext::new(cx, blocks![p!["<a>"]]);
    ed.type_text("/h1");
    ed.keys("escape backspace backspace backspace");
    ed.type_text("/h1");
    ed.keys("enter");
    ed.assert(blocks![h1!["<a>"]]);
}

#[gpui::test]
fn a_peer_typing_before_the_slash_keeps_the_menu_closed(cx: &mut TestAppContext) {
    let mut ed = EditorTestContext::new(cx, blocks![p!["<a>"]]);
    ed.type_text("/h1");
    ed.keys("escape");
    ed.view.update(ed.cx, |view, cx| {
        view.update_editor(cx, |editor| {
            let block = editor.document().blocks()[0];
            let mut peer = lotype_core::Document::new(lotype_core::Schema::builtin());
            peer.import(&editor.document().export_snapshot().unwrap())
                .unwrap();
            peer.insert_text(block, 0, "x ").unwrap();
            let update = peer.export_updates(&editor.document().version()).unwrap();
            editor.import(&update).unwrap();
        });
    });
    ed.keys("enter");
    ed.assert(blocks![p!["x /h1"], p!["<a>"]]);
}
