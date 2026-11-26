#![cfg(feature = "test-support")]

use std::sync::Arc;
use std::time::Duration;

use gpui::{Modifiers, MouseMoveEvent, TestAppContext};
use lotype_code::{
    AnalysisFuture, AnalysisRequest, CodeAnalysis, CodeAnalyzer, CodeBlocks, HoverAnnotation,
};
use lotype_core::{Document, Editor, Pos, Schema, Selection};
use lotype_ui::EditorView;
use lotype_ui::test_support::EditorTestContext;

struct DemoAnalyzer;

impl CodeAnalyzer for DemoAnalyzer {
    fn analyze(&self, request: AnalysisRequest) -> AnalysisFuture {
        Box::pin(async move {
            let mut queries = Vec::new();
            let mut hovers = Vec::new();
            for query in request.queries {
                let sig = format!("let {}: usize", query.symbol);
                queries.push((query.target_range.clone(), sig.clone()));
                hovers.push(HoverAnnotation {
                    range: query.target_range,
                    symbol: query.symbol,
                    signature: sig,
                    docs: Some("Resolved by async CodeAnalyzer".into()),
                });
            }
            Ok(CodeAnalysis {
                queries,
                completions: Vec::new(),
                hovers,
                diagnostics: Vec::new(),
            })
        })
    }
}

fn click(ed: &mut EditorTestContext, selector: &'static str) {
    ed.settle();
    let bounds = ed
        .cx
        .debug_bounds(selector)
        .unwrap_or_else(|| panic!("{selector} is visible"));
    ed.cx.simulate_click(bounds.center(), Modifiers::none());
    ed.settle();
}

#[gpui::test]
fn code_block_renders_header_twoslash_analyzer_hover_and_live_editing(cx: &mut TestAppContext) {
    let markdown = "\
```rs twoslash title=\"src/count.rs\" {1}
let count = 3;
//  ^?
let bad: u8 = \"no\";
//            ^^^^ @error(E0308): expected `u8`, found `&str`
```
";
    let mut doc = Document::new(Schema::builtin());
    doc.insert_markdown(None, 0, markdown).unwrap();
    let code_block = doc.blocks()[0];
    let end = doc.text_len(code_block).unwrap();
    let mut editor = Editor::new(doc);
    editor
        .set_selection(Some(Selection::caret(Pos::new(code_block, end))))
        .unwrap();

    let mut ed = EditorTestContext::with_view(cx, |cx| {
        CodeBlocks::new()
            .with_analyzer(Arc::new(DemoAnalyzer))
            .install(EditorView::new(editor, cx), cx)
    });
    ed.cx.executor().advance_clock(Duration::from_millis(120));
    ed.settle();

    assert!(ed.cx.debug_bounds("code-header").is_some());
    assert!(ed.cx.debug_bounds("code-title").is_some());
    assert!(ed.cx.debug_bounds("twoslash-query").is_some());
    assert!(ed.cx.debug_bounds("twoslash-diagnostic").is_some());

    // Hovering over `count` opens the Twoslash hover popover.
    let count_bounds = ed.view.update(ed.cx, |view, _| {
        view.range_bounds(code_block, 4..9).unwrap()
    });
    ed.cx.simulate_event(MouseMoveEvent {
        position: count_bounds.center(),
        pressed_button: None,
        modifiers: Modifiers::none(),
    });
    ed.settle();
    assert!(ed.cx.debug_bounds("twoslash-hover").is_some());

    // Copy strips Twoslash directive lines while preserving code.
    click(&mut ed, "code-copy");
    let copied = ed.cx.read_from_clipboard().expect("clipboard item");
    assert_eq!(
        copied.text().as_deref(),
        Some("let count = 3;\nlet bad: u8 = \"no\";")
    );

    // Live typing in the code block keeps CRDT offsets intact beneath interlines.
    ed.type_text("\nlet ok = count;");
    let text = ed.editor(|editor| editor.document().plain_text(code_block).unwrap());
    assert!(text.ends_with("\nlet ok = count;"));

    // Language selector updates `attr::LANGUAGE` in one undo step.
    click(&mut ed, "code-language");
    assert!(ed.cx.debug_bounds("code-language-menu").is_some());
    ed.keys("t s");
    assert!(ed.cx.debug_bounds("code-lang-typescript").is_some());
    assert!(ed.cx.debug_bounds("code-lang-rust").is_none());
    click(&mut ed, "code-lang-typescript");
    assert_eq!(
        ed.editor(|editor| editor.document().code_language(code_block)),
        Some("typescript".to_owned())
    );

    // Editing the title updates `attr::TITLE`.
    click(&mut ed, "code-title");
    assert!(ed.cx.debug_bounds("code-title-input").is_some());
    ed.keys("cmd-a");
    ed.cx.simulate_input("src/index.ts");
    ed.keys("enter");
    ed.settle();
    assert_eq!(
        ed.editor(|editor| editor.document().code_title(code_block)),
        Some("src/index.ts".to_owned())
    );
}

#[gpui::test]
fn folded_twoslash_preview_hides_directives_shows_completions_and_unfolds_on_click(
    cx: &mut TestAppContext,
) {
    let markdown = "\
Intro paragraph.

```ts twoslash title=\"src/blocks.ts\"
// @noErrors
const hiddenSetup = 1
// ---cut---
/** Formats a block label. */
function describe(name: string) {
  return name.trim() // [!code ++]
}
const out = describe(\"hi\")
//    ^?
out.to
//    ^|
```
";
    let mut doc = Document::new(Schema::builtin());
    doc.insert_markdown(None, 0, markdown).unwrap();
    let blocks = doc.blocks();
    let (intro, code_block) = (blocks[0], blocks[1]);
    let code_text = doc.plain_text(code_block).unwrap();
    let hidden_pos = code_text.find("hiddenSetup").unwrap();
    let fn_pos = code_text.find("describe").unwrap();

    // Start with the caret in the intro paragraph so the code block opens in Preview Mode.
    let mut editor = Editor::new(doc);
    editor
        .set_selection(Some(Selection::caret(Pos::new(intro, 0))))
        .unwrap();

    let mut ed = EditorTestContext::with_view(cx, |cx| {
        lotype_code::install(EditorView::new(editor, cx), cx)
    });
    ed.settle();

    // Preview Mode: cut lines are collapsed, while `^?` and `^|` cards render automatically.
    assert!(ed.cx.debug_bounds("twoslash-cut").is_none());
    assert!(ed.cx.debug_bounds("twoslash-query").is_some());
    assert!(ed.cx.debug_bounds("twoslash-completion").is_some());
    let hidden_bounds = ed.view.update(ed.cx, |view, _| {
        view.range_bounds(code_block, hidden_pos..hidden_pos + 11)
    });
    assert!(hidden_bounds.is_none());

    // Hovering over `describe` opens the automatic JSDoc hover card without `@hover`.
    let fn_bounds = ed.view.update(ed.cx, |view, _| {
        view.range_bounds(code_block, fn_pos..fn_pos + 8).unwrap()
    });
    ed.cx.simulate_event(MouseMoveEvent {
        position: fn_bounds.center(),
        pressed_button: None,
        modifiers: Modifiers::none(),
    });
    ed.settle();
    assert!(ed.cx.debug_bounds("twoslash-hover").is_some());

    // Clicking inside the code block switches to Edit Mode, unfolding cut lines and directives.
    ed.cx.simulate_click(fn_bounds.center(), Modifiers::none());
    ed.settle();
    assert!(ed.cx.debug_bounds("twoslash-cut").is_some());
    assert!(ed.cx.debug_bounds("code-preview").is_none());
    let unfolded_bounds = ed.view.update(ed.cx, |view, _| {
        view.range_bounds(code_block, hidden_pos..hidden_pos + 11)
    });
    assert!(unfolded_bounds.is_some());

    // Leaving the code block folds it back to Shiki Twoslash Preview Mode.
    let intro_bounds = ed.view.update(ed.cx, |view, _| {
        view.range_bounds(intro, 0..5).unwrap()
    });
    ed.cx.simulate_click(intro_bounds.center(), Modifiers::none());
    ed.settle();
    assert!(ed.cx.debug_bounds("twoslash-cut").is_none());
}
