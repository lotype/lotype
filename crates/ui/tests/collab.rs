use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use futures_util::stream;
use gpui::{BackgroundExecutor, TestAppContext};
use lotype_ai::{LlmEvent, LlmProvider, LlmRequest, LlmStream};
use lotype_core::test_support::editor;
use lotype_core::{
    BlockId, Document, Editor, Pos, Presence, Schema, Selection, SyncMessage, blocks, p,
};
use lotype_ui::EditorView;
use lotype_ui::test_support::EditorTestContext;

#[gpui::test]
fn drawing_peers_does_not_fail_whatever_they_call_themselves(cx: &mut TestAppContext) {
    let local = editor(blocks![p!["one<a>"], p!["two"]]);
    let mut peer = Document::new(Schema::builtin());
    peer.import(&local.document().export_snapshot().unwrap())
        .unwrap();
    let blocks = peer.blocks();
    let range = Selection::Text {
        anchor: Pos::new(blocks[0], 1),
        head: Pos::new(blocks[1], 2),
    };
    let presence = Presence::new();
    let shown = presence.clone();
    let mut ed =
        EditorTestContext::with_view(cx, |cx| EditorView::new(local, cx).with_presence(shown));

    for name in [
        "Ann",
        "",
        "two\nlines\tand a name far too long to show whole",
    ] {
        let sent = Presence::publish(&peer, name, 0xff_8800, Some(&range));
        presence.apply(&sent).unwrap();
        ed.view.update(ed.cx, |_, cx| cx.notify());
        ed.settle();
        let seen = ed.editor(|editor| presence.collaborators(editor.document()));
        assert_eq!(seen[0].selection.as_ref(), Some(&range));
    }
    // Typing before a peer's selection moves it along.
    ed.keys("cmd-up");
    ed.type_text("zero ");
    let seen = ed.editor(|editor| presence.collaborators(editor.document()));
    let Some(Selection::Text { anchor, .. }) = seen[0].selection else {
        panic!("the peer has text selected");
    };
    assert_eq!(anchor, Pos::new(blocks[0], 6));
}

/// A model that pauses between edits so the collaborator can see each one.
struct PausedModel(BackgroundExecutor, [BlockId; 2]);

impl LlmProvider for PausedModel {
    fn stream(&self, _: LlmRequest) -> LlmStream {
        let events = self
            .1
            .into_iter()
            .enumerate()
            .map(|(index, block)| LlmEvent::ToolCallDelta {
                id: index.to_string(),
                name: "update".into(),
                args_delta: format!(r#"{{"id":"{block}","block":"Private draft {index}"}}"#),
            });
        let events: Vec<_> = events.chain([LlmEvent::Done]).collect();
        Box::pin(stream::unfold(
            (self.0.clone(), events.into_iter(), false),
            |(executor, mut events, pause)| async move {
                let event = events.next()?;
                if pause {
                    executor.timer(Duration::from_secs(1)).await;
                }
                Some((Ok(event), (executor, events, true)))
            },
        ))
    }
}

#[gpui::test]
fn rara_sees_the_ai_cursor_move_without_receiving_the_draft(cx: &mut TestAppContext) {
    let andi = editor(blocks![p!["one<a>"], p!["two"], p!["three"]]);
    let blocks = andi.document().blocks();
    let mut shared = Document::new(Schema::builtin());
    shared
        .import(&andi.document().export_snapshot().unwrap())
        .unwrap();
    let rara_caret = Selection::caret(Pos::new(blocks[1], 2));
    let mut rara = Editor::new(shared);
    rara.set_selection(Some(rara_caret.clone())).unwrap();
    let model = Arc::new(PausedModel(cx.executor(), [blocks[2], blocks[0]]));
    let messages = Rc::new(RefCell::new(Vec::new()));
    let outgoing = messages.clone();
    let (sender, _) = cx.add_window_view(|_, cx| {
        EditorView::new(andi, cx)
            .with_ai(model)
            .with_presence_publisher(
                "Andi",
                0x33_66ff,
                move |message| outgoing.borrow_mut().push(message),
                cx,
            )
    });
    let presence = Presence::new();
    let mut rara = EditorTestContext::with_view(cx, |cx| {
        EditorView::new(rara, cx).with_presence(presence.clone())
    });
    sender.update(rara.cx, |view, cx| view.ai_ask("Rewrite", cx));
    rara.settle();

    let deliver = |rara: &mut EditorTestContext, advance| {
        rara.cx.executor().advance_clock(advance);
        rara.settle();
        for message in messages.take() {
            let SyncMessage::Presence(bytes) = message else {
                panic!("presence sends no document updates");
            };
            presence.apply(&bytes).unwrap();
        }
        rara.view.update(rara.cx, |_, cx| cx.notify());
        rara.settle();
        rara.cx.debug_bounds("ai-cursor")
    };

    for (advance, row) in [
        (Duration::from_millis(20), blocks[2]),
        (Duration::from_secs(1), blocks[0]),
    ] {
        let badge = deliver(&mut rara, advance).expect("Rara sees the agent's cursor");
        let text = rara
            .view
            .update(rara.cx, |view, _| view.text_bounds(row).unwrap());
        assert!(badge.top() <= text.top() && badge.bottom() > text.top());
        let seen = rara.editor(|editor| presence.collaborators(editor.document()));
        assert_eq!(
            seen[0].selection,
            Some(Selection::caret(Pos::new(blocks[0], 3)))
        );
        assert_eq!(seen[0].ai_block, Some(row));
        rara.assert(blocks![p!["one"], p!["tw<a>o"], p!["three"]]);
    }
    rara.type_text("!");
    assert!(rara.cx.debug_bounds("ai-cursor").is_some());
    assert!(deliver(&mut rara, Duration::from_secs(1)).is_none());
    let seen = rara.editor(|editor| presence.collaborators(editor.document()));
    assert_eq!(seen.len(), 1);
    assert_eq!(seen[0].ai_block, None);
    rara.assert(blocks![p!["one"], p!["tw!<a>o"], p!["three"]]);
}
