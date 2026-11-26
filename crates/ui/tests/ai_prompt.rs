use std::sync::Arc;

use gpui::{Modifiers, TestAppContext, px};
use lotype_ai::test_support::{MockProvider, tool_call};
use lotype_ai::{AiState, LlmError, LlmEvent};
use lotype_core::test_support::{BlockNode, editor};
use lotype_core::{BlockId, blocks, p};
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
