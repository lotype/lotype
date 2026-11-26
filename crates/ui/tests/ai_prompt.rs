use std::sync::Arc;
use std::time::Duration;

use futures_util::{StreamExt, stream};
use gpui::{Modifiers, TestAppContext, point, px, size};
use lotype_ai::test_support::{MockProvider, tool_call};
use lotype_ai::{AiState, LlmError, LlmEvent, LlmProvider, LlmRequest, LlmStream};
use lotype_core::test_support::{BlockNode, editor};
use lotype_core::{BlockId, Document, Pos, blocks, p};
use lotype_ui::EditorView;
use lotype_ui::test_support::EditorTestContext;

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
fn the_prompt_opens_from_space_and_the_toolbar(cx: &mut TestAppContext) {
    let (mut ed, _) = with_model(cx, blocks![p!["text"], p!["<a>"]], |_| Vec::new());
    ed.type_text("/ai");
    assert!(!shows(&mut ed, "slash-ai") && !shows(&mut ed, "ai-bar"));
    ed.assert(blocks![p!["text"], p!["/ai<a>"]]);
    ed.keys("backspace backspace backspace");

    ed.type_text(" ");
    assert!(shows(&mut ed, "ai-bar"));
    ed.assert(blocks![p!["text"], p!["<a>"]]);
    ed.keys("escape");

    ed.keys("up shift-right shift-right");
    let selection = ed.editor(|editor| editor.selection().cloned());
    assert!(shows(&mut ed, "toolbar-bold"), "{selection:?}");
    // A selection's prompt is the toolbar's menu, which its button closes.
    click(&mut ed, "toolbar-ai");
    assert!(shows(&mut ed, "ai-menu") && !shows(&mut ed, "ai-bar"));
    assert!(shows(&mut ed, "toolbar-bold") && shows(&mut ed, "ai-suggestion-spelling"));
    click(&mut ed, "toolbar-ai");
    assert!(!shows(&mut ed, "ai-menu") && shows(&mut ed, "toolbar-bold"));
}

#[gpui::test]
fn space_opens_the_composer_next_to_the_empty_paragraph(cx: &mut TestAppContext) {
    let (mut ed, provider) = with_model(cx, blocks![p!["<a>"], p!["other"]], |_| Vec::new());
    let target = ed.view.update(ed.cx, |view, _| {
        let block = view.editor().document().blocks()[0];
        view.text_bounds(block).unwrap()
    });
    ed.type_text(" ");
    let prompt = ed.cx.debug_bounds("ai-bar").expect("inline prompt");
    assert!(prompt.top() >= target.bottom() && prompt.top() - target.bottom() <= px(12.));
    assert!(!shows(&mut ed, "ai-suggestion-spelling"));
    // Its button sends what the field says, and nothing while it is empty.
    click(&mut ed, "ai-send");
    assert!(provider.requests().is_empty());
    ed.assert(blocks![p!["<a>"], p!["other"]]);
    ed.cx.simulate_input("Write");
    click(&mut ed, "ai-send");
    assert!(provider.requests()[0].messages[0].text.ends_with("Write"));
}

#[gpui::test]
fn a_toolbar_preset_sends_the_selected_phrase_for_review(cx: &mut TestAppContext) {
    let (mut ed, provider) = with_model(
        cx,
        blocks![p!["Keep <a>speling<b> here."], p!["Other"]],
        |blocks| {
            let target = blocks[0];
            let args = format!(r#"{{"id":"{target}","text":"spelling"}}"#);
            let mut answer = tool_call("fix", "replace_selected_text", &args, 5);
            answer.push(Ok(LlmEvent::Done));
            vec![answer]
        },
    );
    click(&mut ed, "toolbar-ai");
    click(&mut ed, "ai-suggestion-spelling");
    assert_eq!(state(&mut ed), Some(AiState::Reviewing));
    let requests = provider.requests();
    assert!(
        requests[0].messages[0]
            .text
            .contains(r#""selectedText":"speling""#)
    );
    assert!(
        requests[0].messages[0]
            .text
            .ends_with("Fix spelling and grammar in the selected text.")
    );
    assert_eq!(requests[0].tools.len(), 1);
    assert_eq!(requests[0].tools[0].name, "replace_selected_text");
    assert_eq!(
        first(&mut ed),
        (
            "Keep speling here.".into(),
            Some("Keep spelling here.".into())
        )
    );
    click(&mut ed, "ai-accept");
    assert_eq!(first(&mut ed).0, "Keep spelling here.");
}

#[gpui::test]
fn without_a_model_space_is_a_space_and_no_prompt_is_offered(cx: &mut TestAppContext) {
    let mut ed = EditorTestContext::new(cx, blocks![p!["<a>"]]);
    ed.type_text(" ");
    ed.assert(blocks![p![" <a>"]]);
    assert!(!shows(&mut ed, "ai-bar"));
}

#[gpui::test]
fn the_button_to_send_leaves_the_field_s_row_once_the_text_is_more_than_a_line(
    cx: &mut TestAppContext,
) {
    let (mut ed, provider) = with_model(cx, blocks![p!["<a>"]], |_| Vec::new());
    ed.type_text(" ");
    // How far under the top of the card the button is.
    let down = |ed: &mut EditorTestContext| {
        ed.settle();
        let (card, send) = (ed.cx.debug_bounds("ai-bar"), ed.cx.debug_bounds("ai-send"));
        send.unwrap().top() - card.unwrap().top()
    };
    let beside = down(&mut ed);
    ed.cx.simulate_input("short");
    assert_eq!(down(&mut ed), beside);
    ed.cx.simulate_input(&" and then much more".repeat(12));
    assert!(down(&mut ed) > beside + px(16.));
    // Sent, the field is empty, and the button is next to it again.
    click(&mut ed, "ai-send");
    assert_eq!(provider.requests().len(), 1);
    assert_eq!(down(&mut ed), beside);
}

#[gpui::test]
fn peer_deleting_the_prompt_row_keeps_the_card_near_where_it_opened(cx: &mut TestAppContext) {
    let mut rows = vec![p!["Above"], p!["<a>"], p!["Below"]];
    for index in 0..24 {
        let text = format!("Paragraph {index}");
        rows.push(p![text.as_str()]);
    }
    let (mut ed, _) = with_model(cx, rows, |_| Vec::new());
    let ids = ed.editor(|editor| editor.document().blocks());
    let (above, target, below, last) = (ids[0], ids[1], ids[2], *ids.last().unwrap());
    let height = px(180.);
    ed.cx.simulate_resize(size(px(360.), height));
    ed.settle();
    ed.type_text(" ");
    assert!(shows(&mut ed, "ai-bar"));

    let mut peer = Document::new(ed.editor(|editor| editor.document().schema().clone()));
    let snapshot = ed.editor(|editor| editor.document().export_snapshot().unwrap());
    let version = ed.editor(|editor| editor.document().version());
    peer.import(&snapshot).unwrap();
    peer.remove_block(target).unwrap();
    let updates = peer.export_updates(&version).unwrap();

    ed.view.update(ed.cx, |view, cx| {
        view.update_editor(cx, |editor| {
            editor.import(&updates).unwrap();
            editor.set_selection(None).unwrap();
        });
    });
    ed.settle();

    let bar = ed.cx.debug_bounds("ai-bar").expect("prompt stays open");
    let (above_bounds, below_bounds, last_drawn) = ed.view.update(ed.cx, |view, _| {
        (
            view.text_bounds(above).unwrap(),
            view.text_bounds(below).unwrap(),
            view.text_bounds(last),
        )
    });
    assert!(above_bounds.bottom() <= bar.top() && bar.bottom() <= below_bounds.top());
    assert!(bar.top() >= px(0.) && bar.bottom() <= height);
    assert!(last_drawn.is_none());
}

#[gpui::test]
fn stalled_stream_reveals_stop_after_caret_follow_skips_the_card_reveal(cx: &mut TestAppContext) {
    struct AddAndStall(BlockId);

    impl LlmProvider for AddAndStall {
        fn stream(&self, _: LlmRequest) -> LlmStream {
            let args = format!(
                r#"{{"referenceId":"{}","position":"after","blocks":["Added line"]}}"#,
                self.0
            );
            Box::pin(stream::iter(tool_call("1", "add", &args, 99)).chain(stream::pending()))
        }
    }

    let doc = editor(blocks![
        p!["First"],
        p!["Second"],
        p!["Third"],
        p!["<a>"],
        p!["Below"]
    ]);
    let target = doc.document().blocks()[3];
    let provider = Arc::new(AddAndStall(target));
    let mut ed = EditorTestContext::with_view(cx, |cx| EditorView::new(doc, cx).with_ai(provider));
    let bottom = ed
        .view
        .update(ed.cx, |view, _| view.text_bounds(target).unwrap().bottom());
    // Tall enough for the added line right below `target`, but not the Stop card under it.
    let height = bottom + px(28.);
    ed.cx.simulate_resize(size(px(360.), height));
    ed.settle();

    ed.view.update(ed.cx, |view, cx| view.ai_ask("Add one", cx));
    ed.settle();
    ed.cx.executor().advance_clock(Duration::from_millis(20));
    ed.settle();

    assert_eq!(state(&mut ed), Some(AiState::Writing));
    let stop = ed.cx.debug_bounds("ai-stop").expect("Stop button is drawn");
    assert!(
        stop.top() >= px(0.) && stop.bottom() <= height,
        "{stop:?} vs {height:?}"
    );
}

#[gpui::test]
fn streaming_selection_replacement_keeps_viewport_on_the_edited_phrase(cx: &mut TestAppContext) {
    struct ReplaceStalled(BlockId);

    impl LlmProvider for ReplaceStalled {
        fn stream(&self, _: LlmRequest) -> LlmStream {
            let args = format!(r#"{{"id":"{}","text":"spelling"}}"#, self.0);
            Box::pin(
                stream::iter(tool_call("fix", "replace_selected_text", &args, 99))
                    .chain(stream::pending()),
            )
        }
    }

    let tail = " Leave this long trailing paragraph untouched.".repeat(60);
    let text = format!("Fix <a>speling<b> here.{tail}");
    let doc = editor(blocks![p![text.as_str()]]);
    let block = doc.document().blocks()[0];
    let provider = Arc::new(ReplaceStalled(block));
    let mut ed = EditorTestContext::with_view(cx, |cx| EditorView::new(doc, cx).with_ai(provider));
    let height = px(220.);
    ed.cx.simulate_resize(size(px(640.), height));
    ed.settle();

    click(&mut ed, "toolbar-ai");
    click(&mut ed, "ai-suggestion-spelling");
    ed.cx.executor().advance_clock(Duration::from_millis(20));
    ed.settle();

    assert_eq!(state(&mut ed), Some(AiState::Writing));
    let (edited, end_caret, paragraph) = ed.view.update(ed.cx, |view, _| {
        let end = view.ai_document().unwrap().text_len(block).unwrap();
        (
            view.caret_bounds(Pos::new(block, "Fix spelling".len()))
                .unwrap(),
            view.caret_bounds(Pos::new(block, end)).unwrap(),
            view.text_bounds(block).unwrap(),
        )
    });
    assert!(paragraph.size.height > height * 2.);
    assert!(edited.top() >= px(0.) && edited.bottom() <= height);
    assert!(end_caret.bottom() > height);
}

#[gpui::test]
fn offscreen_writing_caret_stays_in_view_after_taller_rows_above_measure(cx: &mut TestAppContext) {
    struct OffscreenWrite(BlockId);

    impl LlmProvider for OffscreenWrite {
        fn stream(&self, _: LlmRequest) -> LlmStream {
            let args = format!(
                r#"{{"id":"{}","block":"Updated offscreen target"}}"#,
                self.0
            );
            Box::pin(stream::iter(tool_call("1", "update", &args, 99)).chain(stream::pending()))
        }
    }

    let mut rows = Vec::new();
    for index in 0..8 {
        let text = format!("Short {index}");
        rows.push(p![text.as_str()]);
    }
    let tall = "This unseen middle paragraph wraps across many lines when measured. ".repeat(8);
    for _ in 0..16 {
        rows.push(p![tall.as_str()]);
    }
    rows.push(p!["Bottom target"]);
    let mut doc = editor(rows);
    let first_id = doc.document().blocks()[0];
    doc.set_selection(Some(lotype_core::Selection::caret(Pos::new(first_id, 0))))
        .unwrap();
    let target = *doc.document().blocks().last().unwrap();
    let provider = Arc::new(OffscreenWrite(target));
    let mut ed = EditorTestContext::with_view(cx, |cx| EditorView::new(doc, cx).with_ai(provider));
    let height = px(140.);
    ed.cx.simulate_resize(size(px(320.), height));
    ed.settle();

    ed.view.update(ed.cx, |view, cx| {
        assert!(view.ai_ask_document("Update the last paragraph", cx));
    });
    ed.settle();
    ed.cx.executor().advance_clock(Duration::from_millis(20));
    ed.settle();

    assert_eq!(state(&mut ed), Some(AiState::Writing));
    let caret = ed.view.update(ed.cx, |view, _| {
        let end = view.ai_document().unwrap().text_len(target).unwrap();
        view.caret_bounds(Pos::new(target, end)).unwrap()
    });
    assert!(
        caret.top() >= px(0.) && caret.bottom() <= height,
        "{caret:?} vs {height:?}"
    );
}

#[gpui::test]
fn closing_a_scrolled_past_prompt_preserves_visible_anchor_position(cx: &mut TestAppContext) {
    let tall =
        "A tall opening paragraph whose wrapped height far exceeds the average row. ".repeat(10);
    let opening_text = format!("{tall}<a>");
    let mut rows = vec![p![opening_text.as_str()]];
    for index in 0..24 {
        let text = format!("Short row {index}");
        rows.push(p![text.as_str()]);
    }
    let (mut ed, _) = with_model(cx, rows, |_| Vec::new());
    let height = px(160.);
    ed.cx.simulate_resize(size(px(320.), height));
    ed.settle();

    ed.view
        .update_in(ed.cx, |view, window, cx| view.open_ai_prompt(window, cx));
    ed.settle();
    ed.cx.simulate_event(gpui::ScrollEvent {
        position: point(px(160.), px(80.)),
        delta: gpui::ScrollDelta::Pixels(point(px(0.), -px(180.))),
        ..Default::default()
    });
    ed.settle();
    assert!(ed.cx.debug_bounds("ai-bar").unwrap().bottom() <= px(0.));

    let (visible_row, before) = ed.view.update(ed.cx, |view, _| {
        view.editor()
            .document()
            .blocks()
            .into_iter()
            .skip(1)
            .find_map(|block| {
                let bounds = view.text_bounds(block)?;
                (bounds.top() >= px(16.) && bounds.bottom() <= height)
                    .then_some((block, bounds.top()))
            })
            .expect("a visible row below the scrolled-past prompt")
    });
    ed.keys("escape");
    assert!(!shows(&mut ed, "ai-bar"));
    let after = ed.view.update(ed.cx, |view, _| {
        view.text_bounds(visible_row).unwrap().top()
    });
    assert!(
        (after - before).abs() <= px(1.),
        "expected {visible_row:?} to stay put: before={before:?}, after={after:?}"
    );
}
