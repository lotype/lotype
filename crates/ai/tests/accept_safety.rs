use lotype_ai::{Accepted, AiSession, AiState, LlmEvent};
use lotype_core::test_support::editor;
use lotype_core::{BlockId, Document, Editor, block, blocks, mark, p};
use serde_json::{Value, json};

/// Delivers one complete tool call.
fn call(session: &mut AiSession, name: &str, args: Value) {
    session
        .feed(LlmEvent::ToolCallDelta {
            id: name.to_owned(),
            name: name.to_owned(),
            args_delta: args.to_string(),
        })
        .unwrap();
}

/// Starts a peer from the live document and merges its independent edit.
fn peer_edits(main: &mut Editor, edit: impl FnOnce(&mut Document)) {
    let version = main.document().version();
    let mut peer = Document::new(main.document().schema().clone());
    peer.import(&main.document().export_snapshot().unwrap())
        .unwrap();
    edit(&mut peer);
    main.import(&peer.export_updates(&version).unwrap())
        .unwrap();
}

/// Starts an AI editing turn.
fn writing(main: &Editor) -> AiSession {
    let mut session = AiSession::new(main).unwrap();
    session.prompt("Edit the document").unwrap();
    session
}

/// Finishes a turn and accepts its changes.
fn accept(session: &mut AiSession, main: &mut Editor) -> Accepted {
    session.feed(LlmEvent::Done).unwrap();
    session.accept(main).unwrap()
}

#[test]
fn deleting_a_parent_skips_a_changed_subtree_but_keeps_unrelated_ai_edits() {
    let mut main = editor(blocks![p!["parent", p!["child"]], p!["other"]]);
    let [parent, child, other]: [BlockId; 3] = main.document().blocks().try_into().unwrap();
    let mut session = writing(&main);
    call(&mut session, "delete", json!({"id": parent.to_string()}));
    call(
        &mut session,
        "update",
        json!({"id": other.to_string(), "block": "updated"}),
    );
    assert!(!session.document().contains(child));

    peer_edits(&mut main, |peer| {
        peer.mark(child, 0..5, mark::UNDERLINE, true).unwrap();
        let added = peer
            .insert_block(Some(parent), 1, block::PARAGRAPH)
            .unwrap();
        peer.insert_text(added, 0, "new child").unwrap();
    });
    assert_eq!(
        accept(&mut session, &mut main),
        Accepted {
            applied: 1,
            skipped: 1
        }
    );
    let doc = main.document();
    let children = doc.children(Some(parent)).unwrap();
    assert_eq!(children.len(), 2);
    assert!(
        doc.spans(child).unwrap()[0]
            .marks
            .contains_key(mark::UNDERLINE)
    );
    assert_eq!(doc.plain_text(children[1]).unwrap(), "new child");
    assert_eq!(doc.plain_text(other).unwrap(), "updated");
}

#[test]
fn accepting_delete_matches_children_made_by_an_earlier_ai_call() {
    let mut main = editor(blocks![p!["parent", p!["child"]], p!["other"]]);
    let [parent, child, other]: [BlockId; 3] = main.document().blocks().try_into().unwrap();
    let mut session = writing(&main);
    call(
        &mut session,
        "add",
        json!({"referenceId": child.to_string(), "position": "after", "blocks": ["new child"]}),
    );
    call(&mut session, "delete", json!({"id": parent.to_string()}));
    assert_eq!(
        accept(&mut session, &mut main),
        Accepted {
            applied: 2,
            skipped: 0
        }
    );
    assert_eq!(main.document().blocks(), [other]);
}

#[test]
fn editing_only_a_descendants_rich_content_conflicts_with_delete() {
    let mut main = editor(blocks![p!["parent", p!["child"]]]);
    let [parent, child]: [BlockId; 2] = main.document().blocks().try_into().unwrap();
    let mut session = writing(&main);
    call(&mut session, "delete", json!({"id": parent.to_string()}));
    peer_edits(&mut main, |peer| {
        peer.mark(child, 0..5, mark::UNDERLINE, true).unwrap();
    });
    assert_eq!(
        accept(&mut session, &mut main),
        Accepted {
            applied: 0,
            skipped: 1
        }
    );
    assert_eq!(main.document().children(Some(parent)).unwrap(), [child]);
}

#[test]
fn updating_a_parent_cannot_drop_children_or_promote_a_nested_heading() {
    let main = editor(blocks![p!["parent", p!["child"], p!["sibling"]]]);
    let [parent, child, _]: [BlockId; 3] = main.document().blocks().try_into().unwrap();
    for (target, markdown) in [(parent, "---"), (child, "# New heading")] {
        let mut session = writing(&main);
        call(
            &mut session,
            "update",
            json!({"id": target.to_string(), "block": markdown}),
        );
        session.feed(LlmEvent::Done).unwrap();
        assert_eq!(session.state(), &AiState::UserInput);
        assert_eq!(session.refused(), 1);
        assert_eq!(session.document().blocks(), main.document().blocks());
    }
}

#[test]
fn moving_a_block_during_review_cannot_turn_accept_into_a_nested_promotion() {
    let mut main = editor(blocks![p!["section"], p!["target"], p!["other"]]);
    let [section, target, other]: [BlockId; 3] = main.document().blocks().try_into().unwrap();
    let mut session = writing(&main);
    call(
        &mut session,
        "update",
        json!({"id": target.to_string(), "block": "# Heading"}),
    );
    assert_eq!(
        session.document().block_type(target).unwrap(),
        block::HEADING
    );
    peer_edits(&mut main, |peer| {
        peer.move_block(target, Some(section), 0).unwrap();
        peer.move_block(other, Some(section), 1).unwrap();
    });
    assert_eq!(
        accept(&mut session, &mut main),
        Accepted {
            applied: 0,
            skipped: 1
        }
    );
    assert_eq!(
        main.document().children(Some(section)).unwrap(),
        [target, other]
    );
    assert_eq!(
        main.document().block_type(target).unwrap(),
        block::PARAGRAPH
    );
}

#[test]
fn moving_an_insertion_reference_skips_that_edit_and_its_dependents() {
    let mut main = editor(blocks![p!["section"], p!["anchor"]]);
    let [section, anchor]: [BlockId; 2] = main.document().blocks().try_into().unwrap();
    let mut session = writing(&main);
    call(
        &mut session,
        "add",
        json!({"referenceId": anchor.to_string(), "position": "after", "blocks": ["# Heading"]}),
    );
    let added = session.document().blocks()[2];
    call(
        &mut session,
        "update",
        json!({"id": added.to_string(), "block": "# Changed"}),
    );
    peer_edits(&mut main, |peer| {
        peer.move_block(anchor, Some(section), 0).unwrap()
    });
    assert_eq!(
        accept(&mut session, &mut main),
        Accepted {
            applied: 0,
            skipped: 2
        }
    );
    assert_eq!(main.document().blocks(), [section, anchor]);
}

#[test]
fn replacing_a_table_keeps_rich_cell_edits_made_during_review() {
    let mut doc = Document::new(lotype_core::Schema::builtin());
    let table = doc
        .insert_markdown(None, 0, "| Header |\n| --- |\n| Cell |")
        .unwrap()[0];
    let row = doc.children(Some(table)).unwrap()[1];
    let cell = doc.cell(row, doc.columns(table).unwrap()[0]).unwrap();
    let mut main = Editor::new(doc);
    let mut session = writing(&main);
    call(
        &mut session,
        "update",
        json!({"id": table.to_string(), "block": "Replacement"}),
    );
    assert!(!session.document().contains(table));

    peer_edits(&mut main, |peer| {
        peer.mark(cell, 0..4, mark::UNDERLINE, true).unwrap()
    });
    assert_eq!(
        accept(&mut session, &mut main),
        Accepted {
            applied: 0,
            skipped: 1
        }
    );
    assert!(
        main.document().spans(cell).unwrap()[0]
            .marks
            .contains_key(mark::UNDERLINE)
    );
}

#[test]
fn refusing_a_tool_argument_does_not_end_the_session() {
    let mut main = editor(blocks![p!["old"]]);
    let target = main.document().blocks()[0];
    let mut too_nested = Value::Null;
    for _ in 0..33 {
        too_nested = Value::Array(vec![too_nested]);
    }
    let mut session = writing(&main);
    call(
        &mut session,
        "delete",
        json!({"id": target.to_string(), "extra": too_nested}),
    );
    call(
        &mut session,
        "update",
        json!({"id": target.to_string(), "block": "new"}),
    );
    assert_eq!(
        accept(&mut session, &mut main),
        Accepted {
            applied: 1,
            skipped: 0
        }
    );
    assert_eq!(session.refused(), 1);
    assert_eq!(main.document().plain_text(target).unwrap(), "new");
}
