#![cfg(feature = "test-support")]

use std::sync::{Arc, Mutex};
use std::time::Duration;

use gpui::{
    Focusable as _, Modifiers, MouseMoveEvent, ScrollDelta, ScrollEvent, TestAppContext, point, px,
};
use lotype_code::{
    AnalysisFuture, AnalysisRequest, CodeAnalysis, CodeAnalyzer, CodeBlocks, HoverAnnotation,
};
use lotype_core::{Document, Editor, Pos, Schema, Selection, block};
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
    ed.cx.simulate_event(ScrollEvent {
        position: count_bounds.center(),
        delta: ScrollDelta::Pixels(point(px(0.), px(40.))),
        ..Default::default()
    });
    ed.settle();
    assert!(ed.cx.debug_bounds("twoslash-hover").is_none());

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
fn enter_in_code_block_inserts_lines_then_leaves_edit_mode_on_empty_line(cx: &mut TestAppContext) {
    let mut doc = Document::new(Schema::builtin());
    doc.insert_markdown(None, 0, "```rs\nfn main() {}\n```\n")
        .unwrap();
    let code = doc.blocks()[0];
    let end = doc.text_len(code).unwrap();
    let mut editor = Editor::new(doc);
    editor
        .set_selection(Some(Selection::caret(Pos::new(code, end))))
        .unwrap();

    let mut ed = EditorTestContext::with_view(cx, |cx| {
        CodeBlocks::new().install(EditorView::new(editor, cx), cx)
    });
    ed.settle();

    ed.keys("enter");
    assert_eq!(
        ed.editor(|editor| editor.document().plain_text(code).unwrap()),
        "fn main() {}\n"
    );
    assert_eq!(ed.editor(|editor| editor.document().blocks().len()), 1);
    assert!(matches!(
        ed.editor(|editor| editor.selection().cloned()),
        Some(Selection::Text { head, .. }) if head.block == code
    ));

    ed.keys("enter");
    assert_eq!(ed.editor(|editor| editor.document().blocks().len()), 2);
    assert_eq!(
        ed.editor(|editor| editor.document().plain_text(code).unwrap()),
        "fn main() {}\n"
    );
    assert!(matches!(
        ed.editor(|editor| editor.selection().cloned()),
        Some(Selection::Text { head, .. }) if head.block != code
    ));
}

#[gpui::test]
fn enter_replaces_a_code_block_selection_with_a_line_break(cx: &mut TestAppContext) {
    let mut doc = Document::new(Schema::builtin());
    doc.insert_markdown(None, 0, "```rs\nalpha beta\n```\n")
        .unwrap();
    let code = doc.blocks()[0];
    let mut editor = Editor::new(doc);
    editor
        .set_selection(Some(Selection::Text {
            anchor: Pos::new(code, 0),
            head: Pos::new(code, 5),
        }))
        .unwrap();

    let mut ed = EditorTestContext::with_view(cx, |cx| {
        CodeBlocks::new().install(EditorView::new(editor, cx), cx)
    });
    ed.settle();
    ed.keys("enter");

    assert_eq!(
        ed.editor(|editor| editor.document().plain_text(code).unwrap()),
        "\n beta"
    );
    assert_eq!(ed.editor(|editor| editor.document().blocks().len()), 1);
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
    let intro_bounds = ed
        .view
        .update(ed.cx, |view, _| view.range_bounds(intro, 0..5).unwrap());
    ed.cx
        .simulate_click(intro_bounds.center(), Modifiers::none());
    ed.settle();
    assert!(ed.cx.debug_bounds("twoslash-cut").is_none());
}

#[gpui::test]
fn analyzer_receives_exact_queries_during_ime_composition(cx: &mut TestAppContext) {
    struct RecordingAnalyzer(Arc<Mutex<Vec<AnalysisRequest>>>);

    impl CodeAnalyzer for RecordingAnalyzer {
        fn analyze(&self, request: AnalysisRequest) -> AnalysisFuture {
            let log = self.0.clone();
            Box::pin(async move {
                log.lock().unwrap().push(request);
                Ok(CodeAnalysis::default())
            })
        }
    }

    let recorded = Arc::new(Mutex::new(Vec::new()));
    let mut doc = Document::new(Schema::builtin());
    let code = doc.insert_block(None, 0, block::CODE_BLOCK).unwrap();
    doc.set_attr(code, lotype_core::attr::LANGUAGE, "ts")
        .unwrap();
    doc.insert_text(code, 0, "const alpha = 1;\n//    ^?")
        .unwrap();
    let end = doc.text_len(code).unwrap();
    let mut editor = Editor::new(doc);
    editor
        .set_selection(Some(Selection::caret(Pos::new(code, end))))
        .unwrap();

    let mut ed = EditorTestContext::with_view(cx, |cx| {
        CodeBlocks::new()
            .with_analyzer(Arc::new(RecordingAnalyzer(recorded.clone())))
            .install(EditorView::new(editor, cx), cx)
    });
    ed.cx.executor().advance_clock(Duration::from_millis(120));
    ed.settle();

    // Start IME composition so `decorate` caches the composed string rather
    // than `plain_text`, then simulate a peer update changing the code block.
    ed.ime_compose("draft", None);
    let mut peer = Document::new(Schema::builtin());
    let snap = ed.editor(|e| e.document().export_snapshot().unwrap());
    peer.import(&snap).unwrap();
    let peer_code = peer.blocks()[0];
    peer.delete_text(peer_code, 6..11).unwrap();
    peer.insert_text(peer_code, 6, "renamed").unwrap();
    let update = ed.editor(|e| peer.export_updates(&e.document().version()).unwrap());
    ed.view
        .update(ed.cx, |view, cx| {
            view.update_editor(cx, |editor| editor.import(&update))
        })
        .unwrap();
    ed.cx.executor().advance_clock(Duration::from_millis(120));
    ed.settle();

    let log = recorded.lock().unwrap();
    let latest = log
        .iter()
        .rev()
        .find(|req| req.code.contains("renamed"))
        .expect("recorded analysis request for updated code");
    assert_eq!(latest.queries.len(), 1);
    assert_eq!(latest.queries[0].symbol, "renamed");
    assert_eq!(
        &latest.code[latest.queries[0].target_range.clone()],
        "renamed"
    );
}

#[gpui::test]
fn unmounted_code_controls_restore_focus_and_keep_twoslash_hover_working(cx: &mut TestAppContext) {
    let mut doc = Document::new(Schema::builtin());
    let hover_code = doc.insert_block(None, 0, block::CODE_BLOCK).unwrap();
    doc.set_attr(hover_code, lotype_core::attr::LANGUAGE, "ts")
        .unwrap();
    let hover_src = "/** Says hello. */\nfunction greet() { return 1; }";
    doc.insert_text(hover_code, 0, hover_src).unwrap();
    let greet_pos = hover_src.find("greet").unwrap();

    // Placed last so `debug_bounds("code-language")` targets `doomed_code`.
    let doomed_code = doc.insert_block(None, 1, block::CODE_BLOCK).unwrap();
    doc.set_attr(doomed_code, lotype_core::attr::LANGUAGE, "ts")
        .unwrap();
    doc.insert_text(doomed_code, 0, "const doomed = 2;")
        .unwrap();

    let mut editor = Editor::new(doc);
    editor
        .set_selection(Some(Selection::caret(Pos::new(hover_code, 0))))
        .unwrap();

    let mut ed = EditorTestContext::with_view(cx, |cx| {
        CodeBlocks::new().install(EditorView::new(editor, cx), cx)
    });
    ed.settle();

    let editor_is_focused = |ed: &mut EditorTestContext| {
        ed.view.update_in(ed.cx, |view, window, cx| {
            view.focus_handle(cx).is_focused(window)
        })
    };

    // 1. Open language menu on `doomed_code`, then delete it via peer import
    //    (no mouse click, so `on_mouse_down_out` does not fire).
    let hover_bottom = ed
        .view
        .update(ed.cx, |view, _| view.text_bounds(hover_code).unwrap())
        .bottom();
    click(&mut ed, "code-language");
    let menu_bounds = ed
        .cx
        .debug_bounds("code-language-menu")
        .expect("language menu opens on doomed_code");
    assert!(menu_bounds.top() > hover_bottom);
    assert!(!editor_is_focused(&mut ed));

    let mut peer = Document::new(Schema::builtin());
    let snap = ed.editor(|e| e.document().export_snapshot().unwrap());
    peer.import(&snap).unwrap();
    peer.remove_block(doomed_code).unwrap();
    let update = ed.editor(|e| peer.export_updates(&e.document().version()).unwrap());
    ed.view
        .update(ed.cx, |view, cx| {
            view.update_editor(cx, |editor| editor.import(&update))
        })
        .unwrap();
    ed.settle();
    assert!(ed.cx.debug_bounds("code-language-menu").is_none());
    assert!(editor_is_focused(&mut ed));

    // Twoslash hover on `hover_code` must still work after the deleted block's menu closed.
    let greet_bounds = ed.view.update(ed.cx, |view, _| {
        view.range_bounds(hover_code, greet_pos..greet_pos + 5)
            .unwrap()
    });
    ed.cx.simulate_event(MouseMoveEvent {
        position: greet_bounds.center(),
        pressed_button: None,
        modifiers: Modifiers::none(),
    });
    ed.settle();
    assert!(ed.cx.debug_bounds("twoslash-hover").is_some());

    // 2. Blurring `language_focus` directly closes the language menu via `on_blur`.
    ed.cx.update(|window, _| window.activate_window());
    click(&mut ed, "code-language");
    assert!(ed.cx.debug_bounds("code-language-menu").is_some());
    ed.view.update_in(ed.cx, |view, window, cx| {
        window.focus(&view.focus_handle(cx), cx);
    });
    ed.settle();
    assert!(ed.cx.debug_bounds("code-language-menu").is_none());

    // 3. Open title editor on `hover_code`, then turn it into a paragraph.
    click(&mut ed, "code-add-title");
    assert!(ed.cx.debug_bounds("code-title-input").is_some());
    assert!(!editor_is_focused(&mut ed));
    ed.view
        .update(ed.cx, |view, cx| {
            view.update_editor(cx, |editor| {
                editor.set_selection(Some(Selection::caret(Pos::new(hover_code, 0))))?;
                editor.set_block(block::PARAGRAPH, &[])
            })
        })
        .unwrap();
    ed.settle();
    assert!(ed.cx.debug_bounds("code-title-input").is_none());
    assert!(editor_is_focused(&mut ed));

    // 4. Open language menu on a code block, then scroll it off-screen until virtualized.
    let mut long_doc = Document::new(Schema::builtin());
    let top_code = long_doc.insert_block(None, 0, block::CODE_BLOCK).unwrap();
    long_doc
        .set_attr(top_code, lotype_core::attr::LANGUAGE, "rs")
        .unwrap();
    long_doc.insert_text(top_code, 0, "fn main() {}").unwrap();
    let mut last_para = top_code;
    for i in 1..=80 {
        let p = long_doc.insert_block(None, i, block::PARAGRAPH).unwrap();
        long_doc.insert_text(p, 0, &format!("Line {i}")).unwrap();
        last_para = p;
    }
    // Keep the caret at the bottom so scrolling down allows `top_code` to virtualize.
    let mut long_editor = Editor::new(long_doc);
    long_editor
        .set_selection(Some(Selection::caret(Pos::new(last_para, 0))))
        .unwrap();
    let mut long_ed = EditorTestContext::with_view(cx, |cx| {
        CodeBlocks::new().install(EditorView::new(long_editor, cx), cx)
    });
    long_ed.settle();
    click(&mut long_ed, "code-language");
    assert!(long_ed.cx.debug_bounds("code-language-menu").is_some());
    assert!(!editor_is_focused(&mut long_ed));

    // Scroll downward far enough that `top_code` is virtualized out of view.
    for _ in 0..8 {
        long_ed.cx.simulate_event(ScrollEvent {
            position: point(px(300.), px(300.)),
            delta: ScrollDelta::Pixels(point(px(0.), px(-400.))),
            ..Default::default()
        });
        long_ed.settle();
    }
    assert!(
        long_ed
            .view
            .update(long_ed.cx, |view, _| view.text_bounds(top_code))
            .is_none()
    );
    assert!(long_ed.cx.debug_bounds("code-language-menu").is_none());
    assert!(editor_is_focused(&mut long_ed));
}

#[gpui::test]
fn title_editor_survives_window_deactivation_saves_on_switch_and_skips_unchanged_blur(
    cx: &mut TestAppContext,
) {
    let mut doc = Document::new(Schema::builtin());
    let block_a = doc.insert_block(None, 0, block::CODE_BLOCK).unwrap();
    doc.set_attr(block_a, lotype_core::attr::LANGUAGE, "rs")
        .unwrap();
    doc.set_attr(block_a, lotype_core::attr::TITLE, "a.rs")
        .unwrap();
    doc.insert_text(block_a, 0, "let a = 1;").unwrap();

    let block_b = doc.insert_block(None, 1, block::CODE_BLOCK).unwrap();
    doc.set_attr(block_b, lotype_core::attr::LANGUAGE, "rs")
        .unwrap();
    doc.insert_text(block_b, 0, "let b = 2;").unwrap();

    let mut editor = Editor::new(doc);
    editor
        .set_selection(Some(Selection::caret(Pos::new(block_a, 0))))
        .unwrap();
    let mut ed = EditorTestContext::with_view(cx, |cx| {
        CodeBlocks::new().install(EditorView::new(editor, cx), cx)
    });
    ed.cx.update(|window, _| window.activate_window());
    ed.settle();

    // 1. Deactivating the OS window (Cmd-Tab) must keep the title editor open with its draft.
    click(&mut ed, "code-title");
    assert!(ed.cx.debug_bounds("code-title-input").is_some());
    ed.keys("cmd-a");
    ed.cx.simulate_input("draft.rs");
    ed.settle();
    ed.cx.deactivate_window();
    ed.settle();
    assert!(ed.cx.debug_bounds("code-title-input").is_some());
    assert_eq!(
        ed.editor(|e| e.document().code_title(block_a)),
        Some("a.rs".to_owned())
    );
    ed.cx.update(|window, _| window.activate_window());
    ed.settle();

    // 2. Switching directly to block_b's title button commits block_a's draft first.
    click(&mut ed, "code-add-title");
    assert_eq!(
        ed.editor(|e| e.document().code_title(block_a)),
        Some("draft.rs".to_owned())
    );
    assert!(ed.cx.debug_bounds("code-title-input").is_some());
    ed.keys("escape");
    ed.settle();
    assert!(ed.cx.debug_bounds("code-title-input").is_none());

    // 3. Blurring an unchanged title form must not overwrite a peer's concurrent title edit.
    click(&mut ed, "code-title");
    assert!(ed.cx.debug_bounds("code-title-input").is_some());
    let mut peer = Document::new(Schema::builtin());
    let snap = ed.editor(|e| e.document().export_snapshot().unwrap());
    peer.import(&snap).unwrap();
    peer.set_attr(block_a, lotype_core::attr::TITLE, "peer.rs")
        .unwrap();
    let update = ed.editor(|e| peer.export_updates(&e.document().version()).unwrap());
    ed.view
        .update(ed.cx, |view, cx| {
            view.update_editor(cx, |editor| editor.import(&update))
        })
        .unwrap();
    ed.settle();
    ed.view.update_in(ed.cx, |view, window, cx| {
        window.focus(&view.focus_handle(cx), cx);
    });
    ed.settle();
    assert!(ed.cx.debug_bounds("code-title-input").is_none());
    assert_eq!(
        ed.editor(|e| e.document().code_title(block_a)),
        Some("peer.rs".to_owned())
    );
}
