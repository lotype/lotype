#![cfg(feature = "test-support")]

use std::sync::Arc;
use std::time::Duration;

use gpui::{Modifiers, MouseMoveEvent, TestAppContext};
use lotype_code::{AnalysisFuture, AnalysisRequest, CodeAnalyzer, CodeBlocks};
use lotype_core::{Document, Editor, Pos, Schema, Selection, block};
use lotype_ui::EditorView;
use lotype_ui::test_support::EditorTestContext;

#[gpui::test]
fn failing_analyzer_falls_back_to_builtin_instead_of_staying_analyzing(cx: &mut TestAppContext) {
    struct FailingAnalyzer;

    impl CodeAnalyzer for FailingAnalyzer {
        fn analyze(&self, _request: AnalysisRequest) -> AnalysisFuture {
            Box::pin(async move { Err("transient LSP error".to_owned()) })
        }
    }

    let mut doc = Document::new(Schema::builtin());
    let code = doc.insert_block(None, 0, block::CODE_BLOCK).unwrap();
    doc.set_attr(code, lotype_core::attr::LANGUAGE, "ts")
        .unwrap();
    let src = "const count = 42;\n//    ^?";
    doc.insert_text(code, 0, src).unwrap();
    let count_pos = src.find("count").unwrap();

    let mut editor = Editor::new(doc);
    editor
        .set_selection(Some(Selection::caret(Pos::new(code, 0))))
        .unwrap();
    let mut ed = EditorTestContext::with_view(cx, |cx| {
        CodeBlocks::new()
            .with_analyzer(Arc::new(FailingAnalyzer))
            .install(EditorView::new(editor, cx), cx)
    });
    ed.cx.executor().advance_clock(Duration::from_millis(120));
    ed.settle();

    assert!(ed.cx.debug_bounds("twoslash-query").is_some());
    // BuiltInAnalyzer fallback populates hover annotations for `count`.
    let count_bounds = ed.view.update(ed.cx, |view, _| {
        view.range_bounds(code, count_pos..count_pos + 5).unwrap()
    });
    ed.cx.simulate_event(MouseMoveEvent {
        position: count_bounds.center(),
        pressed_button: None,
        modifiers: Modifiers::none(),
    });
    ed.settle();
    assert!(ed.cx.debug_bounds("twoslash-hover").is_some());
}

#[gpui::test]
fn hover_popover_clears_on_unfold_and_live_edit(cx: &mut TestAppContext) {
    let markdown = "\
Intro paragraph.

```ts twoslash
const hiddenSetup = 1
// ---cut---
/** Formats a block label. */
function describe(name: string) {
  return name.trim()
}
```
";
    let mut doc = Document::new(Schema::builtin());
    doc.insert_markdown(None, 0, markdown).unwrap();
    let blocks = doc.blocks();
    let (intro, code) = (blocks[0], blocks[1]);
    let src = doc.plain_text(code).unwrap();
    let fn_pos = src.find("describe").unwrap();

    let mut editor = Editor::new(doc);
    editor
        .set_selection(Some(Selection::caret(Pos::new(intro, 0))))
        .unwrap();
    let mut ed = EditorTestContext::with_view(cx, |cx| {
        CodeBlocks::new().install(EditorView::new(editor, cx), cx)
    });
    ed.settle();

    // Hover `describe` in folded Preview Mode.
    let folded_bounds = ed.view.update(ed.cx, |view, _| {
        view.range_bounds(code, fn_pos..fn_pos + 8).unwrap()
    });
    ed.cx.simulate_event(MouseMoveEvent {
        position: folded_bounds.center(),
        pressed_button: None,
        modifiers: Modifiers::none(),
    });
    ed.settle();
    assert!(ed.cx.debug_bounds("twoslash-hover").is_some());

    // Clicking inside unfolds the block; stale hover from folded coordinates must clear.
    ed.cx
        .simulate_click(folded_bounds.center(), Modifiers::none());
    ed.settle();
    assert!(ed.cx.debug_bounds("twoslash-hover").is_none());

    // Hovering at the unfolded coordinates opens hover again, then typing clears it.
    let unfolded_bounds = ed.view.update(ed.cx, |view, _| {
        view.range_bounds(code, fn_pos..fn_pos + 8).unwrap()
    });
    assert!(unfolded_bounds.top() > folded_bounds.top());
    ed.cx.simulate_event(MouseMoveEvent {
        position: unfolded_bounds.center(),
        pressed_button: None,
        modifiers: Modifiers::none(),
    });
    ed.settle();
    assert!(ed.cx.debug_bounds("twoslash-hover").is_some());

    ed.type_text("x");
    ed.settle();
    assert!(ed.cx.debug_bounds("twoslash-hover").is_none());
}
