use gpui::TestAppContext;
use lotype_core::test_support::outline;
use lotype_core::{blocks, bullet, divider, p};
use lotype_ui::test_support::EditorTestContext;

fn shown(ed: &mut EditorTestContext) -> String {
    ed.editor(|editor| outline(editor.document(), editor.selection()))
}

#[gpui::test]
fn escape_selects_blocks_that_arrows_move_and_extend(cx: &mut TestAppContext) {
    let mut ed = EditorTestContext::new(
        cx,
        blocks![p!["one<a>", bullet!["child"]], divider!(), p!["two"]],
    );
    ed.keys("escape");
    assert_eq!(
        shown(&mut ed),
        "* paragraph \"one\"\n  bullet_list_item \"child\"\ndivider\nparagraph \"two\"\n"
    );
    ed.keys("down down");
    assert_eq!(
        shown(&mut ed),
        "paragraph \"one\"\n  bullet_list_item \"child\"\n* divider\nparagraph \"two\"\n"
    );
    ed.keys("shift-down");
    assert_eq!(
        shown(&mut ed),
        "paragraph \"one\"\n  bullet_list_item \"child\"\n* divider\n* paragraph \"two\"\n"
    );
    // Enter goes back into the text, at the end of where the selection went.
    ed.keys("enter");
    ed.assert(blocks![
        p!["one", bullet!["child"]],
        divider!(),
        p!["two<a>"]
    ]);
}

#[gpui::test]
fn deleting_blocks_leaves_the_caret_next_to_them(cx: &mut TestAppContext) {
    let mut ed = EditorTestContext::new(
        cx,
        blocks![p!["one"], p!["two<a>", bullet!["child"]], p!["three"]],
    );
    ed.keys("escape backspace");
    ed.assert(blocks![p!["one<a>"], p!["three"]]);

    let mut ed = EditorTestContext::new(cx, blocks![p!["one<a>", bullet!["child"]], p!["two"]]);
    ed.keys("escape delete");
    ed.assert(blocks![p!["<a>two"]]);
    ed.keys("cmd-z");
    ed.assert(blocks![p!["one", bullet!["child"]], p!["two"]]);
}

#[gpui::test]
fn selected_blocks_indent_and_copy_whole(cx: &mut TestAppContext) {
    let mut ed = EditorTestContext::new(cx, blocks![p!["one"], p!["two<a>"], p!["three"], p![]]);
    ed.keys("escape shift-down tab");
    assert_eq!(
        shown(&mut ed),
        "paragraph \"one\"\n  * paragraph \"two\"\n  * paragraph \"three\"\nparagraph \"\"\n"
    );
    // Copied whole, the blocks paste in place of an empty paragraph.
    ed.keys("cmd-c down enter cmd-v");
    ed.assert(blocks![
        p!["one", p!["two"], p!["three"]],
        p!["two"],
        p!["three<a>"]
    ]);
}

#[gpui::test]
fn enter_on_a_block_without_text_goes_to_the_nearest_text(cx: &mut TestAppContext) {
    let mut ed = EditorTestContext::new(cx, blocks![p!["one<a>"], divider!(), p!["two"]]);
    ed.keys("escape down enter");
    ed.assert(blocks![p!["one"], divider!(), p!["two<a>"]]);
}

#[gpui::test]
fn closing_a_toggle_selects_it_for_blocks_it_hides(cx: &mut TestAppContext) {
    let toggle = lotype_core::test_support::block(
        "toggle_list_item",
        vec!["toggle".into(), p!["in<a>side"].into()],
    );
    let mut ed = EditorTestContext::new(cx, blocks![p!["before"], toggle, p!["after"]]);
    ed.keys("escape");
    let toggle = ed.editor(|editor| editor.document().blocks()[1]);
    // As a click on its arrow closes it.
    let arrow = ed.view.update(ed.cx, |view, _| {
        let text = view.text_bounds(toggle).unwrap();
        gpui::point(text.left() - gpui::px(12.), text.top() + gpui::px(11.))
    });
    ed.cx.simulate_click(arrow, gpui::Modifiers::none());
    assert_eq!(
        shown(&mut ed),
        "paragraph \"before\"\n* toggle_list_item \"toggle\"\n  paragraph \"inside\"\nparagraph \"after\"\n"
    );
    ed.keys("down backspace");
    let toggle = lotype_core::test_support::block(
        "toggle_list_item",
        vec!["toggle<a>".into(), p!["inside"].into()],
    );
    ed.assert(blocks![p!["before"], toggle]);
}

#[gpui::test]
fn a_block_selection_is_scrolled_after_its_head(cx: &mut TestAppContext) {
    let mut many: Vec<_> = (0..200).map(|_| p!["line"]).collect();
    many[0] = p!["<a>line"];
    let mut ed = EditorTestContext::new(cx, many);
    let row = ed.editor(|editor| editor.document().blocks()[60]);
    ed.keys("escape");
    ed.keys(&["down"; 60].join(" "));
    ed.settle();
    let height = ed.cx.update(|window, _| window.viewport_size().height);
    let bounds = ed
        .view
        .update(ed.cx, |view, _| view.text_bounds(row).unwrap());
    assert!(bounds.bottom() <= height, "{bounds:?} is below {height:?}");
    let selected = ed.editor(|editor| editor.selection().cloned());
    assert_eq!(selected, Some(lotype_core::Selection::Blocks(vec![row])));
}

#[gpui::test]
fn command_shift_arrows_move_the_selected_blocks(cx: &mut TestAppContext) {
    let mut ed = EditorTestContext::new(cx, blocks![p!["a"], p!["b<a>"], p!["c"]]);
    ed.keys("escape cmd-shift-down");
    assert_eq!(
        shown(&mut ed),
        "paragraph \"a\"\nparagraph \"c\"\n* paragraph \"b\"\n"
    );
    ed.keys("cmd-shift-up cmd-shift-up");
    assert_eq!(
        shown(&mut ed),
        "* paragraph \"b\"\nparagraph \"a\"\nparagraph \"c\"\n"
    );
    // In the text, the same keys still select to the document's ends.
    let mut ed = EditorTestContext::new(cx, blocks![p!["a"], p!["b<a>"]]);
    ed.keys("cmd-shift-up");
    ed.assert(blocks![p!["<b>a"], p!["b<a>"]]);
}

#[gpui::test]
fn escape_again_goes_back_into_the_text(cx: &mut TestAppContext) {
    let mut ed = EditorTestContext::new(cx, blocks![p!["one<a>"], p!["two"]]);
    ed.keys("escape");
    assert_eq!(shown(&mut ed), "* paragraph \"one\"\nparagraph \"two\"\n");
    ed.keys("escape");
    ed.assert(blocks![p!["one<a>"], p!["two"]]);
}
