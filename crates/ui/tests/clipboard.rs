use gpui::{ClipboardItem, TestAppContext};
use lotype_core::{b, blocks, h1, p};
use lotype_ui::test_support::EditorTestContext;

#[gpui::test]
fn copied_text_pastes_back_with_its_blocks_and_marks(cx: &mut TestAppContext) {
    let mut ed = EditorTestContext::new(
        cx,
        blocks![h1!["T<a>itle"], p!["a ", b!["bo<b>ld"]], p!["end"]],
    );
    ed.keys("cmd-c");
    let copied = ed.cx.read_from_clipboard().unwrap();
    assert_eq!(copied.text().as_deref(), Some("itle\na bo"));
    ed.keys("cmd-down cmd-v");
    // Markdown does not say that the heading was copied from its middle,
    // so it is pasted as a heading.
    ed.assert(blocks![
        h1!["Title"],
        p!["a ", b!["bold"]],
        p!["end"],
        h1!["itle"],
        p!["a ", b!["bo"], "<a>"]
    ]);
}

#[gpui::test]
fn cut_takes_the_selection_out(cx: &mut TestAppContext) {
    let mut ed = EditorTestContext::new(cx, blocks![p!["a<a>bc<b>d"]]);
    ed.keys("cmd-x");
    ed.assert(blocks![p!["a<a>d"]]);
    ed.keys("cmd-v");
    ed.assert(blocks![p!["abc<a>d"]]);
    // A caret copies nothing and leaves the clipboard as it was.
    ed.keys("cmd-c cmd-x");
    assert_eq!(
        ed.cx.read_from_clipboard().unwrap().text().as_deref(),
        Some("bc")
    );
    ed.assert(blocks![p!["abc<a>d"]]);
}

#[gpui::test]
fn text_from_elsewhere_pastes_as_it_is(cx: &mut TestAppContext) {
    let mut ed = EditorTestContext::new(cx, blocks![p!["<a>"]]);
    ed.cx
        .write_to_clipboard(ClipboardItem::new_string("x **y**".into()));
    ed.keys("cmd-v");
    ed.assert(blocks![p!["x **y**<a>"]]);
    // Other GPUI applications keep metadata of their own with the text.
    let zed = ClipboardItem::new_string_with_metadata(" z".into(), "[{\"len\":2}]".into());
    ed.cx.write_to_clipboard(zed);
    ed.keys("cmd-v");
    ed.assert(blocks![p!["x **y** z<a>"]]);
}
