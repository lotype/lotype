use gpui::TestAppContext;
use lotype_core::{blocks, divider, h1, p};
use lotype_ui::test_support::EditorTestContext;

#[gpui::test]
fn every_text_block_is_laid_out_with_its_text(cx: &mut TestAppContext) {
    let mut ed = EditorTestContext::new(
        cx,
        blocks![h1!["Title"], divider!(), p!["one ", "two"], p![]],
    );
    let blocks = ed.editor(|editor| editor.document().blocks());
    ed.view.update(ed.cx, |view, _| {
        let text = |at: usize| view.text_layout(blocks[at]).map(|layout| layout.text());
        assert_eq!(text(0).as_deref(), Some("Title"));
        assert_eq!(text(1), None);
        assert_eq!(text(2).as_deref(), Some("one two"));
        // An empty block still takes a line, for the caret to stand in.
        let empty = view.text_layout(blocks[3]).unwrap();
        assert_eq!(empty.text(), "");
        assert!(empty.bounds().size.height > gpui::px(0.));
        let heading = view.text_layout(blocks[0]).unwrap();
        assert!(heading.line_height() > empty.line_height());
    });
}
