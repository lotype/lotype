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

#[gpui::test]
fn language_picker_highlights_top_match_on_typing_and_supports_arrow_navigation(
    cx: &mut TestAppContext,
) {
    let mut doc = Document::new(Schema::builtin());
    let code = doc.insert_block(None, 0, block::CODE_BLOCK).unwrap();
    doc.set_attr(code, lotype_core::attr::LANGUAGE, "rs")
        .unwrap();
    doc.insert_text(code, 0, "fn main() {}").unwrap();

    let mut editor = Editor::new(doc);
    editor
        .set_selection(Some(Selection::caret(Pos::new(code, 0))))
        .unwrap();
    let mut ed = EditorTestContext::with_view(cx, |cx| {
        CodeBlocks::new().install(EditorView::new(editor, cx), cx)
    });
    ed.settle();

    let open_picker = |ed: &mut EditorTestContext| {
        let bounds = ed.cx.debug_bounds("code-language").expect("lang button");
        ed.cx.simulate_click(bounds.center(), Modifiers::none());
        ed.settle();
    };

    // Initially highlights the block's current language (`rust`).
    open_picker(&mut ed);
    assert!(ed.cx.debug_bounds("code-lang-query").is_some());
    assert!(ed.cx.debug_bounds("code-lang-active-rust").is_some());

    // Typing `"c"` ranks exact ID `C` first and highlights it immediately.
    ed.keys("c");
    assert!(ed.cx.debug_bounds("code-lang-active-c").is_some());

    // ArrowDown moves the highlight to `CSS`, and Enter commits the highlighted item.
    ed.keys("down");
    assert!(ed.cx.debug_bounds("code-lang-active-css").is_some());
    ed.keys("enter");
    assert_eq!(
        ed.editor(|e| e.document().code_language(code)),
        Some("css".to_owned())
    );

    // Reopening and typing alias `"ts"` highlights `TypeScript` immediately and commits on Enter.
    open_picker(&mut ed);
    assert!(ed.cx.debug_bounds("code-lang-active-css").is_some());
    ed.keys("t s");
    assert!(ed.cx.debug_bounds("code-lang-active-typescript").is_some());
    // Cmd-Enter must not commit the selection, while plain Enter commits it.
    ed.keys("cmd-enter");
    assert!(ed.cx.debug_bounds("code-language-menu").is_some());
    ed.keys("enter");
    assert_eq!(
        ed.editor(|e| e.document().code_language(code)),
        Some("typescript".to_owned())
    );

    // Spacebar works in filter queries ("plain text"), Shift-Tab navigates up,
    // and Cmd-Backspace clears the query back to all languages.
    open_picker(&mut ed);
    ed.keys("p l a i n space t e x t");
    assert!(ed.cx.debug_bounds("code-lang-active-text").is_some());
    ed.keys("cmd-backspace");
    assert!(ed.cx.debug_bounds("code-lang-active-rust").is_some());
    ed.keys("down shift-tab");
    assert!(ed.cx.debug_bounds("code-lang-active-rust").is_some());
    ed.keys("escape");
    assert!(ed.cx.debug_bounds("code-language-menu").is_none());
}
