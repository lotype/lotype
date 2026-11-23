use gpui::{Bounds, EntityInputHandler, TestAppContext, px};
use lotype_core::{b, blocks, p};
use lotype_ui::test_support::EditorTestContext;

#[gpui::test]
fn typing_inserts_at_the_caret_and_replaces_a_selection(cx: &mut TestAppContext) {
    let mut ed = EditorTestContext::new(cx, blocks![p!["a<a>b"], p!["c<b>d"]]);
    ed.type_text("é🙂");
    ed.assert(blocks![p!["aé🙂<a>d"]]);
    ed.type_text("x");
    ed.assert(blocks![p!["aé🙂x<a>d"]]);
}

#[gpui::test]
fn keys_run_the_commands_of_the_core_keymap(cx: &mut TestAppContext) {
    let mut ed = EditorTestContext::new(cx, blocks![p!["ab<a>cd"]]);
    ed.keys("enter");
    ed.assert(blocks![p!["ab"], p!["<a>cd"]]);
    ed.keys("backspace");
    ed.assert(blocks![p!["ab<a>cd"]]);
    ed.keys("cmd-b");
    ed.type_text("x");
    ed.assert(blocks![p!["ab", b!["x"], "<a>cd"]]);
    ed.keys("cmd-z");
    ed.assert(blocks![p!["ab<a>cd"]]);
}

#[gpui::test]
fn composed_text_reaches_the_document_only_when_committed(cx: &mut TestAppContext) {
    let mut ed = EditorTestContext::new(cx, blocks![p!["a🙂<a>z"]]);
    ed.ime_compose("に", None);
    ed.assert(blocks![p!["a🙂<a>z"]]);
    assert_eq!(ed.ime_state(), ("a🙂にz".into(), Some(3..4)));
    ed.ime_compose("日本", Some(0..2));
    assert_eq!(ed.ime_state(), ("a🙂日本z".into(), Some(3..5)));
    ed.view.update_in(ed.cx, |view, window, cx| {
        let selected = view.selected_text_range(false, window, cx).unwrap();
        assert_eq!(selected.range, 3..5);
        // The IME places its candidates under the composed text.
        let bounds = view.bounds_for_range(3..5, Bounds::default(), window, cx);
        assert!(bounds.is_some_and(|bounds| bounds.size.width > px(0.)));
    });
    ed.ime_commit("日本");
    ed.assert(blocks![p!["a🙂日本<a>z"]]);
    assert_eq!(ed.ime_state(), ("a🙂日本z".into(), None));
    // One undo step takes the committed text out.
    ed.keys("cmd-z");
    ed.assert(blocks![p!["a🙂<a>z"]]);
}

#[gpui::test]
fn composing_over_a_selection_replaces_it_only_when_committed(cx: &mut TestAppContext) {
    let mut ed = EditorTestContext::new(cx, blocks![p!["a<a>bc<b>d"]]);
    ed.ime_compose("x", None);
    assert_eq!(ed.ime_state(), ("axd".into(), Some(1..2)));
    // Taking the composition back leaves the selection as it was.
    ed.ime_compose("", None);
    ed.assert(blocks![p!["a<a>bc<b>d"]]);
    assert_eq!(ed.ime_state(), ("abcd".into(), None));

    ed.ime_compose("x", None);
    ed.ime_commit("y");
    ed.assert(blocks![p!["ay<a>d"]]);
}

#[gpui::test]
fn unmarking_commits_the_composed_text(cx: &mut TestAppContext) {
    let mut ed = EditorTestContext::new(cx, blocks![p!["a<a>"]]);
    ed.ime_compose("ka", None);
    ed.view
        .update_in(ed.cx, |view, window, cx| view.unmark_text(window, cx));
    ed.assert(blocks![p!["aka<a>"]]);
}

#[gpui::test]
fn the_system_can_replace_a_range_it_names(cx: &mut TestAppContext) {
    // As macOS does when a held key's accent menu replaces the last character.
    let mut ed = EditorTestContext::new(cx, blocks![p!["cafe<a>"]]);
    ed.view.update_in(ed.cx, |view, window, cx| {
        view.replace_text_in_range(Some(3..4), "é", window, cx);
    });
    ed.assert(blocks![p!["café<a>"]]);
}

#[gpui::test]
fn a_composition_follows_its_place_through_remote_edits(cx: &mut TestAppContext) {
    let mut ed = EditorTestContext::new(cx, blocks![p!["ab<a>"]]);
    ed.ime_compose("に", None);
    let mut peer = lotype_core::Document::new(lotype_core::Schema::builtin());
    let snapshot = ed.editor(|editor| editor.document().export_snapshot().unwrap());
    peer.import(&snapshot).unwrap();
    let block = peer.blocks()[0];
    peer.insert_text(block, 0, "remote ").unwrap();
    let update = ed.editor(|editor| peer.export_updates(&editor.document().version()).unwrap());
    ed.view
        .update(ed.cx, |view, cx| {
            view.update_editor(cx, |editor| editor.import(&update))
        })
        .unwrap();
    assert_eq!(ed.ime_state(), ("remote abに".into(), Some(9..10)));
    ed.ime_commit("に");
    ed.assert(blocks![p!["remote abに<a>"]]);
}

#[gpui::test]
fn replacing_part_of_the_composed_text_keeps_the_rest(cx: &mut TestAppContext) {
    let mut ed = EditorTestContext::new(cx, blocks![p!["x<a>"]]);
    ed.ime_compose("abc", None);
    ed.view.update_in(ed.cx, |view, window, cx| {
        view.replace_text_in_range(Some(3..4), "C", window, cx);
    });
    ed.assert(blocks![p!["xabC<a>"]]);
}

#[gpui::test]
fn replacing_text_apart_from_the_composition_commits_that_first(cx: &mut TestAppContext) {
    let mut ed = EditorTestContext::new(cx, blocks![p!["x", b!["y"], "<a>"]]);
    ed.ime_compose("abc", None);
    ed.view.update_in(ed.cx, |view, window, cx| {
        view.replace_text_in_range(Some(0..1), "z", window, cx);
    });
    // The bold `y` between stays as it was, and what was composed after it
    // took its bold, as typed text does.
    ed.assert(blocks![p!["z<a>", b!["yabc"]]]);
}
