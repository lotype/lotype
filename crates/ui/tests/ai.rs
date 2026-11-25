use std::sync::Arc;

use gpui::{AppContext, Focusable, Modifiers, TestAppContext, VisualTestContext};
use lotype_ai::test_support::{MockProvider, tool_call};
use lotype_ai::{AiState, LlmError, LlmEvent, Role};
use lotype_core::test_support::{BlockNode, editor};
use lotype_core::{BlockId, blocks, p};
use lotype_ui::test_support::EditorTestContext;
use lotype_ui::{AiPanel, EditorView};

type Answer = Vec<Result<LlmEvent, LlmError>>;

/// An editor of `blocks` with a model that gives the answers `script`
/// writes, knowing the blocks.
fn with_model<'a>(
    cx: &'a mut TestAppContext,
    blocks: Vec<BlockNode>,
    script: impl FnOnce(&[BlockId]) -> Vec<Answer>,
) -> (EditorTestContext<'a>, Arc<MockProvider>) {
    let editor = editor(blocks);
    let mut provider = MockProvider::new();
    for answer in script(&editor.document().blocks()) {
        provider = provider.answer(answer);
    }
    let provider = Arc::new(provider);
    let model = provider.clone();
    let ed = EditorTestContext::with_view(cx, |cx| EditorView::new(editor, cx).with_ai(model));
    (ed, provider)
}

/// An answer that makes `block` say `text`, and says so.
fn update(block: BlockId, text: &str) -> Answer {
    let args = format!(r#"{{"id": "{block}", "block": "{text}"}}"#);
    let mut answer = tool_call("1", "update", &args, 5);
    answer.push(Ok(LlmEvent::TextDelta("Changed.".into())));
    answer.push(Ok(LlmEvent::Done));
    answer
}

fn shows(ed: &mut EditorTestContext, selector: &'static str) -> bool {
    ed.settle();
    ed.cx.debug_bounds(selector).is_some()
}

fn click(ed: &mut EditorTestContext, selector: &'static str) {
    ed.settle();
    let bounds = ed.cx.debug_bounds(selector);
    let bounds = bounds.unwrap_or_else(|| panic!("{selector} shows"));
    ed.cx.simulate_click(bounds.center(), Modifiers::none());
    ed.settle();
}

/// Opens the bar and asks.
fn ask(ed: &mut EditorTestContext, text: &str) {
    ed.view
        .update_in(ed.cx, |view, window, cx| view.open_ai_prompt(window, cx));
    ed.settle();
    ed.cx.simulate_input(text);
    ed.keys("enter");
}

fn state(ed: &mut EditorTestContext) -> Option<AiState> {
    ed.view.update(ed.cx, |view, _| view.ai_state().cloned())
}

/// The text of the first block, in the document and as the view shows it.
fn first(ed: &mut EditorTestContext) -> (String, Option<String>) {
    ed.view.update(ed.cx, |view, _| {
        let text = |doc: &lotype_core::Document| doc.plain_text(doc.blocks()[0]).unwrap();
        (text(view.editor().document()), view.ai_document().map(text))
    })
}

#[gpui::test]
fn the_model_s_edits_show_for_review_and_are_the_document_s_once_accepted(cx: &mut TestAppContext) {
    let (mut ed, provider) = with_model(cx, blocks![p!["one<a>"], p!["two"]], |blocks| {
        vec![update(blocks[0], "ONE")]
    });
    assert!(!shows(&mut ed, "ai-bar"));
    ask(&mut ed, "Shout");
    assert_eq!(state(&mut ed), Some(AiState::Reviewing));
    assert!(provider.requests()[0].messages[0].text.ends_with("Shout"));
    // The view shows the copy, and the document is as it was.
    assert_eq!(first(&mut ed), ("one".to_owned(), Some("ONE".to_owned())));
    assert!(shows(&mut ed, "ai-accept") && shows(&mut ed, "ai-reject"));
    let status = ed.cx.debug_bounds("ai-status");
    assert!(status.is_some());

    // The copy is the model's to write in: text and keys do nothing.
    ed.ime_commit("typed");
    assert_eq!(first(&mut ed), ("one".to_owned(), Some("ONE".to_owned())));

    click(&mut ed, "ai-accept");
    assert_eq!(
        (state(&mut ed), first(&mut ed)),
        (None, ("ONE".to_owned(), None))
    );
    assert!(!shows(&mut ed, "ai-bar"));
    assert!(!shows(&mut ed, "ai-cursor"));
    // The editor has the keys again, and one step back takes the edit.
    ed.keys("cmd-z");
    assert_eq!(first(&mut ed).0, "one");
}

#[gpui::test]
fn the_ai_cursor_labels_the_last_edited_row_not_the_last_row_in_the_document(
    cx: &mut TestAppContext,
) {
    let (mut ed, _) = with_model(
        cx,
        blocks![p!["one<a>"], p!["two"], p!["three"]],
        |blocks| {
            let args = format!(r#"{{"id": "{}", "block": "THREE"}}"#, blocks[2]);
            let mut answer = tool_call("first", "update", &args, 5);
            answer.extend(update(blocks[0], "ONE"));
            vec![answer]
        },
    );
    ask(&mut ed, "Rewrite the bottom and then the top");
    assert_eq!(state(&mut ed), Some(AiState::Reviewing));
    let cursor = ed
        .cx
        .debug_bounds("ai-cursor")
        .expect("the labelled cursor");
    let (first, last) = ed.view.update(ed.cx, |view, _| {
        let blocks = view.ai_document().unwrap().blocks();
        (
            view.text_bounds(blocks[0]).unwrap(),
            view.text_bounds(blocks[2]).unwrap(),
        )
    });
    assert!(cursor.top() <= first.top() && cursor.bottom() > first.top());
    assert!(cursor.right() <= first.left() && cursor.bottom() <= last.top());
    click(&mut ed, "ai-reject");
    assert!(!shows(&mut ed, "ai-cursor"));
}

#[gpui::test]
fn the_ai_cursor_marks_a_preview_emptied_by_the_model(cx: &mut TestAppContext) {
    let (mut ed, _) = with_model(cx, blocks![p!["one<a>"]], |blocks| {
        let args = format!(r#"{{"id": "{}"}}"#, blocks[0]);
        vec![tool_call("1", "delete", &args, 5)]
    });
    ask(&mut ed, "Remove everything");
    assert_eq!(state(&mut ed), Some(AiState::Reviewing));
    assert!(shows(&mut ed, "empty-document") && shows(&mut ed, "ai-cursor"));
}

#[gpui::test]
fn rejected_edits_leave_the_document_as_it_was(cx: &mut TestAppContext) {
    let (mut ed, _) = with_model(cx, blocks![p!["one<a>"]], |blocks| {
        vec![update(blocks[0], "ONE")]
    });
    ask(&mut ed, "Shout");
    click(&mut ed, "ai-reject");
    assert_eq!(
        (state(&mut ed), first(&mut ed)),
        (None, ("one".to_owned(), None))
    );
    ed.assert(blocks![p!["one<a>"]]);
    ed.type_text("!");
    ed.assert(blocks![p!["one!<a>"]]);
    // What was said is kept for the next time.
    let said = ed
        .view
        .update(ed.cx, |view, _| view.ai_transcript().clone());
    assert_eq!(said.messages[0].role, Role::User);
    assert_eq!(said.messages[1].text, "Changed.");
}

#[gpui::test]
fn a_request_that_failed_can_be_tried_again(cx: &mut TestAppContext) {
    let (mut ed, provider) = with_model(cx, blocks![p!["one<a>"]], |blocks| {
        let mut failing = update(blocks[0], "ONE AND MORE");
        failing.truncate(3);
        failing.push(Err(LlmError("the model is busy".into())));
        vec![failing, update(blocks[0], "ONE")]
    });
    ask(&mut ed, "Shout");
    assert_eq!(
        state(&mut ed),
        Some(AiState::Error("the model is busy".into()))
    );
    // What the call had written so far is taken back.
    assert_eq!(first(&mut ed).1.as_deref(), Some("one"));
    assert!(shows(&mut ed, "ai-retry"));
    click(&mut ed, "ai-retry");
    assert_eq!(state(&mut ed), Some(AiState::Reviewing));
    assert_eq!(first(&mut ed).1.as_deref(), Some("ONE"));
    assert_eq!(provider.requests().len(), 2);
    click(&mut ed, "ai-accept");
    assert_eq!(first(&mut ed).0, "ONE");
}

#[gpui::test]
fn an_answer_with_no_edits_leaves_nothing_to_review(cx: &mut TestAppContext) {
    let (mut ed, _) = with_model(cx, blocks![p!["one<a>"]], |_| {
        vec![vec![
            Ok(LlmEvent::TextDelta("One paragraph.".into())),
            Ok(LlmEvent::Done),
        ]]
    });
    ask(&mut ed, "What is this?");
    assert_eq!(state(&mut ed), None);
    let said = ed
        .view
        .update(ed.cx, |view, _| view.ai_transcript().clone());
    assert_eq!(said.messages[1].text, "One paragraph.");
    // The bar stays to ask more, and Escape closes it.
    assert!(shows(&mut ed, "ai-bar") && !shows(&mut ed, "ai-accept"));
    ed.keys("escape");
    assert!(!shows(&mut ed, "ai-bar"));
    ed.type_text("!");
    ed.assert(blocks![p!["one!<a>"]]);
}

#[gpui::test]
fn the_prompt_opens_from_an_empty_paragraph_the_slash_menu_and_the_toolbar(
    cx: &mut TestAppContext,
) {
    let (mut ed, _) = with_model(cx, blocks![p!["text"], p!["<a>"]], |_| Vec::new());
    ed.type_text(" ");
    assert!(shows(&mut ed, "ai-bar"));
    ed.assert(blocks![p!["text"], p!["<a>"]]);
    ed.keys("escape");

    ed.type_text("/ai");
    assert!(shows(&mut ed, "slash-ai"));
    ed.keys("enter");
    assert!(shows(&mut ed, "ai-bar"));
    ed.assert(blocks![p!["text"], p!["<a>"]]);
    ed.keys("escape");

    ed.keys("up shift-right shift-right");
    let selection = ed.editor(|editor| editor.selection().cloned());
    assert!(shows(&mut ed, "toolbar-bold"), "{selection:?}");
    click(&mut ed, "toolbar-ai");
    assert!(shows(&mut ed, "ai-bar"));
}

#[gpui::test]
fn without_a_model_space_is_a_space_and_no_prompt_is_offered(cx: &mut TestAppContext) {
    let mut ed = EditorTestContext::new(cx, blocks![p!["<a>"]]);
    ed.type_text(" ");
    ed.assert(blocks![p![" <a>"]]);
    ed.type_text("/ai");
    assert!(!shows(&mut ed, "slash-ai") && !shows(&mut ed, "ai-bar"));
}

#[gpui::test]
fn the_panel_asks_shows_what_was_said_and_accepts(cx: &mut TestAppContext) {
    let editor = editor(blocks![p!["one<a>"]]);
    let block = editor.document().blocks()[0];
    let provider = Arc::new(MockProvider::new().answer(update(block, "ONE")));
    let (panel, cx) = cx.add_window_view(|window, cx| {
        let view = cx.new(|cx| EditorView::new(editor, cx).with_ai(provider));
        AiPanel::new(view.clone(), window, cx)
    });
    panel.update_in(cx, |panel, window, cx| {
        panel.focus_handle(cx).focus(window, cx)
    });
    cx.run_until_parked();
    let drawn = |cx: &mut VisualTestContext, selector: &'static str| {
        cx.run_until_parked();
        cx.debug_bounds(selector)
    };
    assert!(drawn(cx, "ai-panel").is_some() && drawn(cx, "ai-panel-accept").is_none());
    cx.simulate_input("Shout");
    cx.simulate_keystrokes("enter");
    assert!(drawn(cx, "ai-panel-status").is_some());
    let accept = drawn(cx, "ai-panel-accept").expect("the edits wait for review");
    cx.simulate_click(accept.center(), Modifiers::none());
    assert!(drawn(cx, "ai-panel-accept").is_none());
}
