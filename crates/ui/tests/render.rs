use gpui::{TestAppContext, px};
use lotype_core::{Pos, blocks, divider, h1, p};
use lotype_ui::test_support::EditorTestContext;

#[gpui::test]
fn every_text_block_is_laid_out(cx: &mut TestAppContext) {
    let mut ed = EditorTestContext::new(
        cx,
        blocks![h1!["Title"], divider!(), p!["one ", "two"], p![]],
    );
    let blocks = ed.editor(|editor| editor.document().blocks());
    ed.view.update(ed.cx, |view, _| {
        let height = |at: usize| {
            view.text_bounds(blocks[at])
                .map(|bounds| bounds.size.height)
        };
        assert!(height(1).is_none());
        // An empty block still takes a line, for the caret to stand in.
        let empty = height(3).unwrap();
        assert!(empty > px(0.));
        assert_eq!(height(2), Some(empty));
        assert!(height(0).unwrap() > empty);
        let caret = view.caret_bounds(Pos::new(blocks[3], 0)).unwrap();
        assert_eq!(caret.size.height, empty);
    });
}
