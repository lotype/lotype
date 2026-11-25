use lotype_ai::{AiSession, LlmEvent};
use lotype_core::test_support::editor;
use lotype_core::{BlockId, blocks, p};
use serde_json::json;

fn call(session: &mut AiSession, id: &str, name: &str, args: serde_json::Value) {
    let event = LlmEvent::ToolCallDelta {
        id: id.to_owned(),
        name: name.to_owned(),
        args_delta: args.to_string(),
    };
    session.feed(event).unwrap();
}

fn said(session: &AiSession, block: BlockId) -> Option<String> {
    let spans = session.original_spans(block)?;
    Some(spans.iter().map(|span| span.text.as_str()).collect())
}

#[test]
fn a_session_keeps_what_a_block_said_before_its_first_edit_there() {
    let mut main = editor(blocks![p!["one<a>"], p!["two"], p!["three"]]);
    let blocks = main.document().blocks();
    let mut session = AiSession::new(&main).unwrap();
    session.prompt("Rewrite").unwrap();
    let update = |block: BlockId, text: &str| json!({"id": block.to_string(), "block": text});
    call(&mut session, "1", "update", update(blocks[0], "ONE"));
    call(&mut session, "2", "update", update(blocks[0], "ONE AGAIN"));
    call(
        &mut session,
        "3",
        "delete",
        json!({"id": blocks[1].to_string()}),
    );
    session.feed(LlmEvent::Done).unwrap();
    // What others write in the document meanwhile is none of it.
    main.apply(|doc, _| {
        doc.insert_text(blocks[0], 0, "peer ")?;
        Ok(None)
    })
    .unwrap();
    assert_eq!(said(&session, blocks[0]).as_deref(), Some("one"));
    assert_eq!(said(&session, blocks[1]).as_deref(), Some("two"));
    assert_eq!(said(&session, blocks[2]), None);
}
