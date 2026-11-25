use lotype_ai::test_support::tool_call;
use lotype_ai::{Accepted, AiSession, AiState, Change, LlmEvent};
use lotype_core::test_support::editor;
use lotype_core::{BlockId, Document, Editor, b, blocks, i, p};
use serde_json::{Value, json};

/// Starts a session that may edit only the supplied editor selection.
fn session(editor: &Editor) -> AiSession {
    let mut session = AiSession::for_selection(editor).unwrap();
    let request = session.prompt("Fix spelling").unwrap();
    assert_eq!(
        request
            .tools
            .iter()
            .map(|tool| tool.name)
            .collect::<Vec<_>>(),
        ["replace_selected_text"]
    );
    session
}

/// Delivers one complete model call with its own identifier.
fn call(session: &mut AiSession, id: &str, name: &str, args: Value) {
    for event in tool_call(id, name, &args.to_string(), usize::MAX) {
        session.feed(event.unwrap()).unwrap();
    }
}

/// Describes the replacement of one selected fragment.
fn replace(session: &mut AiSession, id: &str, block: BlockId, text: &str) {
    call(
        session,
        id,
        "replace_selected_text",
        json!({"id": block.to_string(), "text": text}),
    );
}

#[test]
fn selection_refuses_block_edits_and_offsets_and_preserves_rich_neighbors() {
    let mut live = editor(blocks![
        p![b!["Déjà "], "<a>speling<b>", i![" café"]],
        p!["Other"]
    ]);
    let [target, other]: [BlockId; 2] = live.document().blocks().try_into().unwrap();
    let prefix = "Déjà ".len();
    let suffix = " café".len();
    let attrs = live.document().attrs(target).unwrap();
    let protected = |doc: &Document| {
        let end = doc.text_len(target).unwrap();
        (
            doc.spans_in(target, 0..prefix).unwrap(),
            doc.spans_in(target, end - suffix..end).unwrap(),
            doc.plain_text(other).unwrap(),
            doc.attrs(target).unwrap(),
        )
    };
    let before = protected(live.document());
    let mut writing = session(&live);
    let target_id = target.to_string();
    for (id, name, args) in [
        (
            "whole",
            "update",
            json!({"id": target_id, "block": "# Write a new heading"}),
        ),
        ("delete", "delete", json!({"id": target_id})),
        (
            "add",
            "add",
            json!({"referenceId": target_id, "position": "after", "blocks": ["new"]}),
        ),
        (
            "other",
            "replace_selected_text",
            json!({"id": other.to_string(), "text": "unselected"}),
        ),
        (
            "offset",
            "replace_selected_text",
            json!({"id": target_id, "text": "wrong", "start": 0, "end": 2}),
        ),
    ] {
        call(&mut writing, id, name, args);
    }
    let args = json!({"id": target_id, "text": "spelling"}).to_string();
    for part in tool_call("valid", "replace_selected_text", &args, 3) {
        writing.feed(part.unwrap()).unwrap();
        assert_eq!(protected(writing.document()), before);
        assert_eq!(writing.document().blocks(), [target, other]);
    }
    writing.feed(LlmEvent::Done).unwrap();
    assert_eq!(writing.refused(), 5);
    assert_eq!(writing.changes(), [Change::Updated(target)]);
    assert_eq!(
        writing.document().text(target).unwrap(),
        "Déjà spelling café"
    );
    assert_eq!(live.document().text(target).unwrap(), "Déjà speling café");
    assert_eq!(
        writing.accept(&mut live).unwrap(),
        Accepted {
            applied: 1,
            skipped: 0
        }
    );
    assert_eq!(protected(live.document()), before);
    assert_eq!(live.document().attrs(target).unwrap(), attrs);
    assert_eq!(live.document().text(target).unwrap(), "Déjà spelling café");
    live.undo().unwrap();
    assert_eq!(live.document().text(target).unwrap(), "Déjà speling café");
}

#[test]
fn a_cross_block_selection_never_replaces_the_outer_edges_or_merges_blocks() {
    let mut live = editor(blocks![p!["Keep <b>first"], p!["second<a> here"]]);
    let [first, last]: [BlockId; 2] = live.document().blocks().try_into().unwrap();
    let mut writing = session(&live);
    replace(&mut writing, "first", first, "new first");
    replace(&mut writing, "last", last, "");
    writing.feed(LlmEvent::Done).unwrap();
    assert_eq!(writing.document().text(first).unwrap(), "Keep new first");
    assert_eq!(writing.document().text(last).unwrap(), " here");

    let follow_up = writing
        .prompt("Restore the second phrase with a correction")
        .unwrap();
    let prompt = &follow_up.messages.last().unwrap().text;
    assert!(prompt.contains(r#""text":"new first""#));
    assert!(prompt.contains(&format!(r#"{{"id":"{last}","text":""}}"#)));
    replace(&mut writing, "again", last, "new second");
    writing.feed(LlmEvent::Done).unwrap();
    assert_eq!(
        writing.accept(&mut live).unwrap(),
        Accepted {
            applied: 3,
            skipped: 0
        }
    );
    assert_eq!(live.document().text(first).unwrap(), "Keep new first");
    assert_eq!(live.document().text(last).unwrap(), "new second here");
    assert_eq!(live.document().blocks(), [first, last]);
}

#[test]
fn retry_restores_the_range_before_a_failed_request() {
    let mut live = editor(blocks![p!["Keep <a>typo<b> here"]]);
    let block = live.document().blocks()[0];
    let mut writing = session(&live);
    replace(&mut writing, "first", block, "quite a long change");
    writing
        .feed(LlmEvent::ToolCallDelta {
            id: "second".into(),
            name: "replace_selected_text".into(),
            args_delta: format!(r#"{{"id":"{block}","text":"partial"#),
        })
        .unwrap();
    writing.fail(&"the connection dropped").unwrap();
    assert_eq!(
        writing.document().text(block).unwrap(),
        "Keep quite a long change here"
    );
    let request = writing.retry().unwrap();
    assert!(
        request
            .messages
            .last()
            .unwrap()
            .text
            .contains(r#""text":"typo""#)
    );
    replace(&mut writing, "retry", block, "fix");
    writing.feed(LlmEvent::Done).unwrap();
    assert_eq!(writing.state(), &AiState::Reviewing);
    assert_eq!(
        writing.accept(&mut live).unwrap(),
        Accepted {
            applied: 1,
            skipped: 0
        }
    );
    assert_eq!(live.document().text(block).unwrap(), "Keep fix here");
}

#[test]
fn accept_skips_a_replacement_if_a_peer_shifted_its_range() {
    let mut live = editor(blocks![p!["Keep <a>speling<b> here"]]);
    let block = live.document().blocks()[0];
    let mut writing = session(&live);
    replace(&mut writing, "spelling", block, "spelling");
    writing.feed(LlmEvent::Done).unwrap();
    let version = live.document().version();
    let mut peer = Document::new(live.document().schema().clone());
    peer.import(&live.document().export_snapshot().unwrap())
        .unwrap();
    peer.insert_text(block, 0, "Peer: ").unwrap();
    live.import(&peer.export_updates(&version).unwrap())
        .unwrap();
    assert_eq!(
        writing.accept(&mut live).unwrap(),
        Accepted {
            applied: 0,
            skipped: 1
        }
    );
    assert_eq!(
        live.document().text(block).unwrap(),
        "Peer: Keep speling here"
    );
}
