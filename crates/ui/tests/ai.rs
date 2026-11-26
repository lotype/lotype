use std::sync::Arc;
use std::time::Duration;

use futures_util::{StreamExt, stream};
use gpui::{
    AppContext, Focusable, Modifiers, MouseButton, TestAppContext, VisualTestContext, point, px,
};
use lotype_ai::test_support::{MockProvider, tool_call};
use lotype_ai::{AiState, LlmError, LlmEvent, LlmProvider, LlmRequest, LlmStream, Role};
use lotype_core::test_support::{BlockNode, editor};
use lotype_core::{BlockId, Pos, Selection, blocks, p};
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

#[gpui::test]
fn entering_ai_review_dismisses_editing_menus_from_the_live_editor(cx: &mut TestAppContext) {
    let (mut ed, _) = with_model(cx, blocks![p!["one<a>"], p!["two"]], |blocks| {
        vec![update(blocks[0], "ONE")]
    });
    let under_mouse = ed.view.update(ed.cx, |view, _| {
        let block = view.editor().document().blocks()[1];
        view.text_bounds(block).unwrap().center()
    });
    ed.cx
        .simulate_mouse_move(under_mouse, None, Modifiers::none());
    click(&mut ed, "side-handle");
    let deletion = ed.cx.debug_bounds("block-menu-delete").unwrap().center();
    ed.view.update(ed.cx, |view, cx| view.ai_ask("Rewrite", cx));
    assert!(shows(&mut ed, "ai-cursor"));
    assert!(!shows(&mut ed, "block-menu-delete") && !shows(&mut ed, "side-add"));
    ed.cx.simulate_click(deletion, Modifiers::none());
    let preview = ed
        .view
        .update(ed.cx, |view, _| view.ai_document().unwrap().to_plain_text());
    assert!(preview.contains("ONE") && preview.contains("two"));
    ed.assert(blocks![p!["one<a>"], p!["two"]]);
}

#[gpui::test]
fn starting_an_ai_session_cancels_a_block_drag(cx: &mut TestAppContext) {
    let (mut ed, _) = with_model(cx, blocks![p!["one<a>"], p!["two"]], |blocks| {
        vec![update(blocks[0], "ONE")]
    });
    let (first_row, second_row) = ed.view.update(ed.cx, |view, _| {
        let blocks = view.editor().document().blocks();
        (
            view.text_bounds(blocks[0]).unwrap(),
            view.text_bounds(blocks[1]).unwrap(),
        )
    });
    ed.cx
        .simulate_mouse_move(first_row.center(), None, Modifiers::none());
    assert!(shows(&mut ed, "side-handle"));
    let handle = ed.cx.debug_bounds("side-handle").unwrap().center();
    ed.cx
        .simulate_mouse_down(handle, MouseButton::Left, Modifiers::none());
    let destination = point(second_row.left() + px(4.), second_row.bottom() - px(2.));
    ed.cx
        .simulate_mouse_move(destination, Some(MouseButton::Left), Modifiers::none());
    assert!(shows(&mut ed, "drop-line"));
    ed.view.update(ed.cx, |view, cx| view.ai_ask("Rewrite", cx));
    assert!(shows(&mut ed, "ai-cursor"));
    ed.cx
        .simulate_mouse_up(destination, MouseButton::Left, Modifiers::none());
    assert!(!shows(&mut ed, "drop-line"));
    assert_eq!(first(&mut ed), ("one".into(), Some("ONE".into())));
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

/// A model that starts rewriting a block and never finishes.
struct Stalled(BlockId);

impl LlmProvider for Stalled {
    fn stream(&self, _: LlmRequest) -> LlmStream {
        let args = format!(r#"{{"id": "{}", "block": "ON"#, self.0);
        Box::pin(stream::iter(tool_call("1", "update", &args, 99)).chain(stream::pending()))
    }
}

#[gpui::test]
fn stopping_during_a_pending_flush_keeps_calls_that_already_ended(cx: &mut TestAppContext) {
    struct Buffered(Answer);

    impl LlmProvider for Buffered {
        fn stream(&self, _: LlmRequest) -> LlmStream {
            Box::pin(stream::iter(self.0.clone()).chain(stream::pending()))
        }
    }

    let model = editor(blocks![p!["one<a>"], p!["two"]]);
    let blocks = model.document().blocks();
    let first = format!(r#"{{"id": "{}", "block": "ONE"}}"#, blocks[0]);
    let second = format!(r#"{{"id": "{}", "block": "TW"#, blocks[1]);
    let mut events = tool_call("first", "update", &first, 99);
    events.extend(tool_call("second", "update", &second, 99));
    let provider = Arc::new(Buffered(events));
    let mut ed =
        EditorTestContext::with_view(cx, |cx| EditorView::new(model, cx).with_ai(provider));
    ed.view.update(ed.cx, |view, cx| view.ai_ask("Rewrite", cx));
    ed.settle();
    assert_eq!(state(&mut ed), Some(AiState::Thinking));

    ed.view.update(ed.cx, |view, cx| view.ai_stop(cx));
    ed.settle();
    assert_eq!(state(&mut ed), Some(AiState::Reviewing));
    ed.view.update(ed.cx, |view, _| {
        let draft = view.ai_document().unwrap();
        assert_eq!(draft.plain_text(blocks[0]).unwrap(), "ONE");
        assert_eq!(draft.plain_text(blocks[1]).unwrap(), "two");
    });
    click(&mut ed, "ai-accept");
    ed.assert(blocks![p!["ONE<a>"], p!["two"]]);
}

#[gpui::test]
fn the_label_rides_the_live_caret_while_the_model_writes_text(cx: &mut TestAppContext) {
    let doc = editor(blocks![p!["one<a>"], p!["two"]]);
    let model = Arc::new(Stalled(doc.document().blocks()[0]));
    let mut ed = EditorTestContext::with_view(cx, |cx| EditorView::new(doc, cx).with_ai(model));
    ed.view.update(ed.cx, |view, cx| view.ai_ask("Shout", cx));
    ed.settle();
    ed.cx.executor().advance_clock(Duration::from_millis(20));
    assert!(!shows(&mut ed, "ai-cursor") && shows(&mut ed, "ai-stop"));
    assert_eq!(state(&mut ed), Some(AiState::Writing));
    assert_eq!(first(&mut ed), ("one".into(), Some("ON".into())));
}

#[gpui::test]
fn the_light_on_working_moves_only_while_a_frame_is_cheap(cx: &mut TestAppContext) {
    let doc = editor(blocks![p!["one<a>"]]);
    let model = Arc::new(Stalled(doc.document().blocks()[0]));
    let mut ed = EditorTestContext::with_view(cx, |cx| EditorView::new(doc, cx).with_ai(model));
    ed.view.update(ed.cx, |view, cx| {
        view.take_frames_as_cheap();
        view.ai_ask("Shout", cx);
    });
    let glow = |ed: &mut EditorTestContext| {
        ed.cx.executor().advance_clock(Duration::from_secs(1));
        ed.settle();
        ed.view.update(ed.cx, |view, _| view.ai_glow())
    };
    let moved = glow(&mut ed);
    assert!(moved > 0);
    ed.view.update(ed.cx, |view, _| view.take_frames_as_dear());
    assert_eq!(glow(&mut ed), moved);
}

#[gpui::test]
fn a_block_the_model_removed_stays_struck_out_where_it_was(cx: &mut TestAppContext) {
    let (mut ed, _) = with_model(
        cx,
        blocks![p!["one<a>"], p!["two"], p!["three"]],
        |blocks| {
            let args = format!(r#"{{"id": "{}"}}"#, blocks[1]);
            vec![tool_call("1", "delete", &args, 5)]
        },
    );
    let blocks = ed.editor(|editor| editor.document().blocks());
    ask(&mut ed, "Remove the middle");
    assert_eq!(state(&mut ed), Some(AiState::Reviewing));
    let (left, rows) = ed.view.update(ed.cx, |view, _| {
        let left = view.ai_document().unwrap().blocks().len();
        let tops = blocks
            .iter()
            .map(|block| view.text_bounds(*block).unwrap().top());
        (left, tops.collect::<Vec<_>>())
    });
    assert!(left == 2 && rows[0] < rows[1] && rows[1] < rows[2]);
    click(&mut ed, "ai-accept");
    ed.assert(blocks![p!["one<a>"], p!["three"]]);
}

#[gpui::test]
fn the_ai_cursor_marks_what_the_model_removed_from_an_emptied_preview(cx: &mut TestAppContext) {
    let (mut ed, _) = with_model(cx, blocks![p!["one<a>"]], |blocks| {
        let args = format!(r#"{{"id": "{}"}}"#, blocks[0]);
        vec![tool_call("1", "delete", &args, 5)]
    });
    ask(&mut ed, "Remove everything");
    assert_eq!(state(&mut ed), Some(AiState::Reviewing));
    assert!(!shows(&mut ed, "empty-document") && shows(&mut ed, "ai-cursor"));
    click(&mut ed, "ai-accept");
    assert!(shows(&mut ed, "empty-document") && !shows(&mut ed, "ai-cursor"));
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
    assert!(shows(&mut ed, "ai-status") && !shows(&mut ed, "ai-accept"));
    ed.keys("escape");
    assert!(!shows(&mut ed, "ai-bar"));
    ed.view
        .update_in(ed.cx, |view, window, cx| view.open_ai_prompt(window, cx));
    assert!(shows(&mut ed, "ai-bar") && !shows(&mut ed, "ai-status"));
    ed.keys("escape");
    ed.type_text("!");
    ed.assert(blocks![p!["one!<a>"]]);
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

#[gpui::test]
fn all_ai_entry_points_share_a_thread_but_only_the_current_context(cx: &mut TestAppContext) {
    let reply = |text: &str| [Ok(LlmEvent::TextDelta(text.into())), Ok(LlmEvent::Done)];
    let provider = Arc::new(
        MockProvider::new()
            .answer(reply("Selection answer"))
            .answer(reply("Document answer"))
            .answer(reply("Inline answer")),
    );
    let model = provider.clone();
    let doc = editor(blocks![p!["First <a>phrase<b>"], p!["Second paragraph"]]);
    let second = doc.document().blocks()[1];
    let view = cx.update(|cx| cx.new(|cx| EditorView::new(doc, cx).with_ai(model)));
    let (panel, cx) = cx.add_window_view(|window, cx| AiPanel::new(view.clone(), window, cx));
    view.update(cx, |view, cx| view.ai_ask("Selection only", cx));
    cx.run_until_parked();
    assert!(cx.debug_bounds("ai-panel-message-0").is_some());
    assert!(cx.debug_bounds("ai-panel-message-1").is_some());
    panel.update_in(cx, |panel, window, cx| {
        panel.focus_handle(cx).focus(window, cx)
    });

    cx.simulate_input("Summarize both");
    cx.simulate_keystrokes("shift-enter");
    cx.simulate_input("Use one sentence");
    cx.run_until_parked();
    assert_eq!(provider.requests().len(), 1);
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    view.update(cx, |view, cx| {
        view.update_editor(cx, |editor| {
            editor.set_selection(Some(Selection::caret(Pos::new(second, 0))))
        })
        .unwrap();
        view.ai_ask("Write here", cx);
    });
    cx.run_until_parked();

    let requests = provider.requests();
    assert!(
        requests[0].messages[0]
            .text
            .contains(r#""selectedText":"phrase""#)
    );
    let first_chat = &requests[1].messages;
    assert_eq!(first_chat.len(), 3);
    assert_eq!(first_chat[0].text, "Selection only");
    assert_eq!(first_chat[1].text, "Selection answer");
    let document = &first_chat[2].text;
    assert!(document.ends_with("Summarize both\nUse one sentence"));
    assert!(document.contains("Second paragraph"));
    for absent in [r#""selectedText":"#, r#""selected":"#, r#""cursor":"#] {
        assert!(!document.contains(absent));
    }
    let inline = &requests[2].messages;
    assert_eq!(inline.len(), 5);
    assert_eq!(inline[2].text, "Summarize both\nUse one sentence");
    assert_eq!(inline[3].text, "Document answer");
    assert!(inline[4].text.contains(&format!(r#""cursor":"{second}""#)));
    assert!(inline[4].text.ends_with("Write here"));
    assert!(cx.debug_bounds("ai-panel-message-5").is_some());
}

#[gpui::test]
fn document_composer_keeps_unsent_text_while_a_contextual_edit_is_under_review(
    cx: &mut TestAppContext,
) {
    let doc = editor(blocks![p!["one<a>"]]);
    let block = doc.document().blocks()[0];
    let provider = Arc::new(
        MockProvider::new()
            .answer(update(block, "ONE"))
            .answer([Ok(LlmEvent::Done)]),
    );
    let model = provider.clone();
    let view = cx.update(|cx| cx.new(|cx| EditorView::new(doc, cx).with_ai(model)));
    let (panel, cx) = cx.add_window_view(|window, cx| AiPanel::new(view.clone(), window, cx));
    panel.update_in(cx, |panel, window, cx| {
        panel.focus_handle(cx).focus(window, cx)
    });
    view.update(cx, |view, cx| view.ai_ask("Rewrite inline", cx));
    cx.run_until_parked();
    assert!(cx.debug_bounds("ai-panel-accept").is_none());
    cx.simulate_input("Keep this draft");
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    assert_eq!(provider.requests().len(), 1);

    view.update_in(cx, |view, window, cx| view.ai_reject(window, cx));
    cx.run_until_parked();
    let send = cx.debug_bounds("ai-panel-send").unwrap();
    cx.simulate_click(send.center(), Modifiers::none());
    cx.run_until_parked();
    let requests = provider.requests();
    assert_eq!(requests.len(), 2);
    assert!(
        requests[1]
            .messages
            .last()
            .unwrap()
            .text
            .ends_with("Keep this draft")
    );
}
