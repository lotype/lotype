use lotype_ai::test_support::{MockProvider, tool_call};
use lotype_ai::{
    Accepted, AiError, AiSession, AiState, Change, EditLocation, LlmError, LlmEvent, Op, Role,
};
use lotype_core::test_support::{editor, outline};
use lotype_core::{BlockId, Document, Editor, Pos, Selection, blocks, p};
use serde_json::json;

/// The block with `text`.
fn block(doc: &Document, text: &str) -> BlockId {
    let found = doc
        .blocks()
        .into_iter()
        .find(|block| doc.plain_text(*block).is_ok_and(|of| of == text));
    found.unwrap_or_else(|| panic!("a block with {text:?}"))
}

fn shown(doc: &Document) -> String {
    outline(doc, None)
}

/// A whole call, in one part.
fn call(session: &mut AiSession, id: &str, name: &str, args: serde_json::Value) {
    let event = LlmEvent::ToolCallDelta {
        id: id.to_owned(),
        name: name.to_owned(),
        args_delta: args.to_string(),
    };
    session.feed(event).unwrap();
}

fn update(block: BlockId, markdown: &str) -> serde_json::Value {
    json!({"id": block.to_string(), "block": markdown})
}

fn add(reference: BlockId, position: &str, blocks: &[&str]) -> serde_json::Value {
    json!({"referenceId": reference.to_string(), "position": position, "blocks": blocks})
}

/// The arguments of an update as a model writes them: the block first.
fn update_text(block: BlockId, markdown: &str) -> String {
    format!(r#"{{"id": "{block}", "block": {}}}"#, json!(markdown))
}

fn add_text(reference: BlockId, position: &str, blocks: &[&str]) -> String {
    let blocks = json!(blocks);
    format!(r#"{{"referenceId": "{reference}", "position": "{position}", "blocks": {blocks}}}"#)
}

fn three() -> (Editor, [BlockId; 3]) {
    let main = editor(blocks![p!["one"], p!["two"], p!["three"]]);
    let ids = ["one", "two", "three"].map(|text| block(main.document(), text));
    (main, ids)
}

#[test]
fn a_model_edits_the_copy_and_accepting_makes_it_the_document() {
    let (mut main, [one, two, three]) = three();
    let before = shown(main.document());
    let mut session = AiSession::new(&main).unwrap();
    assert_eq!(session.state(), &AiState::UserInput);

    let request = session.prompt("Tidy this up").unwrap();
    assert_eq!(session.state(), &AiState::Thinking);
    let tools: Vec<_> = request.tools.iter().map(|tool| tool.name).collect();
    assert_eq!(tools, ["add", "update", "delete"]);
    let asked = &request.messages.last().unwrap().text;
    assert!(asked.contains(&one.to_string()) && asked.ends_with("Tidy this up"));

    call(&mut session, "1", "update", update(one, "# One"));
    assert_eq!(session.state(), &AiState::Writing);
    call(
        &mut session,
        "2",
        "add",
        add(two, "after", &["- a\n  - nested", "- b"]),
    );
    call(
        &mut session,
        "3",
        "delete",
        json!({"id": three.to_string()}),
    );
    session.feed(LlmEvent::TextDelta("Tidied.".into())).unwrap();
    session.feed(LlmEvent::Done).unwrap();
    assert_eq!(session.state(), &AiState::Reviewing);
    assert_eq!((session.ops().count(), session.refused()), (3, 0));

    // The document is as it was, and the copy shows what was done.
    assert_eq!(shown(main.document()), before);
    let copy = shown(session.document());
    assert_eq!(
        copy,
        "heading level=1 \"One\"\nparagraph \"two\"\nbullet_list_item \"a\"\n  bullet_list_item \"nested\"\nbullet_list_item \"b\"\n"
    );
    let a = block(session.document(), "a");
    let changes = session.changes();
    assert_eq!(changes.len(), 5);
    assert_eq!(changes[0], Change::Updated(one));
    assert_eq!(changes[1], Change::Added(a));
    assert_eq!(changes[4], Change::Removed(three));

    let accepted = session.accept(&mut main).unwrap();
    assert_eq!(
        accepted,
        Accepted {
            applied: 3,
            skipped: 0
        }
    );
    assert_eq!(session.state(), &AiState::Closed);
    assert_eq!(shown(main.document()), copy);
    // The block that was updated is the block it was.
    assert_eq!(block(main.document(), "One"), one);
    // One step back, and the document is what it was.
    assert!(main.undo().unwrap());
    assert_eq!(shown(main.document()), before);
    assert!(!main.can_undo());

    let said: Vec<_> = session.transcript().messages.iter().collect();
    assert_eq!(
        (said[0].role, said[0].text.as_str()),
        (Role::User, "Tidy this up")
    );
    assert_eq!(
        (said[1].role, said[1].text.as_str()),
        (Role::Assistant, "Tidied.")
    );
}

#[test]
fn an_edit_shows_in_the_copy_while_it_is_still_being_written() {
    let (main, [one, two, _]) = three();
    let mut session = AiSession::new(&main).unwrap();
    session.prompt("Rewrite").unwrap();
    let text = |session: &AiSession, block| session.document().plain_text(block).unwrap();

    let mut seen = Vec::new();
    for part in tool_call("1", "update", &update_text(one, "Hello **there**"), 3) {
        session.feed(part.unwrap()).unwrap();
        let preview = text(&session, one);
        if preview != "one" {
            assert_eq!(session.edit_location(), Some(EditLocation::Block(one)));
        }
        seen.push(preview);
    }
    // From what it was, a little more each time, and never anything else.
    assert_eq!(seen.first().map(String::as_str), Some("one"));
    assert_eq!(seen.last().map(String::as_str), Some("Hello there"));
    assert!(
        seen.iter()
            .any(|text| text.starts_with("Hel") && text.len() < 11)
    );
    // Signs of Markdown that are not closed yet show as they are written.
    let grown = seen.iter().filter(|text| *text != "one");
    let plain = |text: &String| text.replace('*', "");
    assert!(
        grown
            .clone()
            .all(|text| "Hello there".starts_with(&plain(text)))
    );

    let mut counts = Vec::new();
    for part in tool_call(
        "2",
        "add",
        &add_text(two, "before", &["first", "second"]),
        4,
    ) {
        session.feed(part.unwrap()).unwrap();
        counts.push(session.document().blocks().len());
    }
    assert!(counts.contains(&4) && counts.last() == Some(&5));
    assert!(counts.windows(2).all(|pair| pair[0] <= pair[1]));
    session.feed(LlmEvent::Done).unwrap();
    let last_added = block(session.document(), "second");
    assert_eq!(
        session.edit_location(),
        Some(EditLocation::Block(last_added))
    );
    assert_eq!(session.ops().count(), 2);
    assert_eq!(
        shown(session.document()),
        "paragraph \"Hello [bold|there]\"\nparagraph \"first\"\nparagraph \"second\"\nparagraph \"two\"\nparagraph \"three\"\n"
    );
}

#[test]
fn the_location_follows_tool_order_and_survives_review() {
    let (main, [one, _, three]) = three();
    let mut session = AiSession::new(&main).unwrap();
    session.prompt("Rewrite from bottom to top").unwrap();
    assert_eq!(session.edit_location(), None);
    call(&mut session, "1", "update", update(three, "THREE"));
    assert_eq!(session.edit_location(), Some(EditLocation::Block(three)));
    call(&mut session, "2", "update", update(one, "ONE"));
    assert_eq!(session.edit_location(), Some(EditLocation::Block(one)));
    session.feed(LlmEvent::Done).unwrap();
    assert_eq!(session.edit_location(), Some(EditLocation::Block(one)));

    session.prompt("Describe those edits").unwrap();
    session
        .feed(LlmEvent::TextDelta("Two changes.".into()))
        .unwrap();
    assert_eq!(session.edit_location(), None);
    session.feed(LlmEvent::Done).unwrap();
    assert_eq!(session.edit_location(), Some(EditLocation::Block(one)));
}

#[test]
fn a_draft_only_block_keeps_the_shared_anchor_that_introduced_it() {
    let (main, [one, _, three]) = three();
    let mut session = AiSession::new(&main).unwrap();
    session.prompt("Add, revise and remove").unwrap();
    call(&mut session, "1", "add", add(one, "after", &["draft"]));
    let draft = block(session.document(), "draft");
    assert_eq!(session.edit_location(), Some(EditLocation::Block(draft)));
    assert_eq!(session.active_anchor(), Some(one));
    call(&mut session, "2", "update", update(draft, "rewritten"));
    assert_eq!(session.active_anchor(), Some(one));
    call(
        &mut session,
        "3",
        "delete",
        json!({"id": three.to_string()}),
    );
    assert_eq!(session.active_anchor(), Some(three));
    session.feed(LlmEvent::Done).unwrap();
    assert_eq!(session.active_anchor(), None);
}

#[test]
fn a_removal_marks_the_nearby_row_or_the_empty_document() {
    let (main, [one, two, three]) = three();
    let mut session = AiSession::new(&main).unwrap();
    session.prompt("Remove everything").unwrap();
    call(&mut session, "1", "delete", json!({"id": two.to_string()}));
    assert_eq!(
        session.edit_location(),
        Some(EditLocation::Removal(Some(three)))
    );
    call(
        &mut session,
        "2",
        "delete",
        json!({"id": three.to_string()}),
    );
    assert_eq!(
        session.edit_location(),
        Some(EditLocation::Removal(Some(one)))
    );
    call(&mut session, "3", "delete", json!({"id": one.to_string()}));
    assert_eq!(session.edit_location(), Some(EditLocation::Removal(None)));
    session.feed(LlmEvent::Done).unwrap();
    assert_eq!(session.edit_location(), Some(EditLocation::Removal(None)));
}

#[test]
fn a_request_that_failed_is_tried_again_from_where_the_user_asked() {
    let (main, [one, two, three]) = three();
    let mut session = AiSession::new(&main).unwrap();
    session.prompt("First").unwrap();
    call(&mut session, "1", "update", update(one, "ONE"));
    session.feed(LlmEvent::Done).unwrap();
    let first = shown(session.document());

    session.prompt("Second").unwrap();
    call(&mut session, "1", "add", add(two, "after", &["added"]));
    let half = update_text(three, "THREE and more");
    for part in tool_call("2", "update", &half[..half.len() - 11], 5) {
        session.feed(part.unwrap()).unwrap();
    }
    assert_eq!(session.document().plain_text(three).unwrap(), "THREE");
    session.fail(&LlmError("the line dropped".into())).unwrap();
    assert_eq!(session.state(), &AiState::Error("the line dropped".into()));
    assert_eq!(session.edit_location(), None);
    // The call that did not end is taken back; the one that did is there.
    assert_eq!(session.document().plain_text(three).unwrap(), "three");
    assert_eq!(session.ops().count(), 2);
    assert!(matches!(session.prompt("Third"), Err(AiError::State)));

    let again = session.retry().unwrap();
    assert_eq!(session.edit_location(), None);
    assert_eq!(shown(session.document()), first);
    assert_eq!(session.ops().count(), 1);
    assert!(again.messages.last().unwrap().text.ends_with("Second"));
    assert!(!again.messages.last().unwrap().text.contains("added"));
    call(&mut session, "1", "update", update(three, "THREE"));
    session.feed(LlmEvent::Done).unwrap();
    assert_eq!(session.state(), &AiState::Reviewing);
    assert_eq!(
        shown(session.document()),
        "paragraph \"ONE\"\nparagraph \"two\"\nparagraph \"THREE\"\n"
    );
}

#[test]
fn stopping_an_answer_keeps_the_edits_that_were_whole() {
    let (main, [one, two, _]) = three();
    let mut session = AiSession::new(&main).unwrap();
    session.prompt("Go").unwrap();
    call(&mut session, "1", "update", update(one, "ONE"));
    let half = update_text(two, "TWO");
    for part in tool_call("2", "update", &half[..half.len() - 3], 4) {
        session.feed(part.unwrap()).unwrap();
    }
    assert_eq!(session.document().plain_text(two).unwrap(), "TW");
    assert_eq!(session.edit_location(), Some(EditLocation::Block(two)));
    session.abort().unwrap();
    assert_eq!(session.state(), &AiState::Reviewing);
    assert_eq!(session.edit_location(), Some(EditLocation::Block(one)));
    assert_eq!(session.document().plain_text(two).unwrap(), "two");
    assert_eq!(session.ops().collect::<Vec<_>>().len(), 1);
    assert!(matches!(session.ops().next(), Some(Op::Update { block, .. }) if *block == one));
}

#[test]
fn calls_that_are_no_edit_are_counted_and_the_others_made() {
    let (main, [one, two, _]) = three();
    let mut session = AiSession::new(&main).unwrap();
    session.prompt("Go").unwrap();
    call(&mut session, "1", "rename", json!({"id": one.to_string()}));
    call(
        &mut session,
        "2",
        "update",
        json!({"id": "99@1", "block": "no such block"}),
    );
    call(&mut session, "3", "update", json!([1, 2]));
    call(&mut session, "4", "update", update(one, "ONE"));
    call(
        &mut session,
        "5",
        "update",
        json!({"id": "not an id", "block": "x"}),
    );
    let cut = update_text(two, "never finished");
    for part in tool_call("6", "update", &cut[..cut.len() - 2], 6) {
        session.feed(part.unwrap()).unwrap();
    }
    session.feed(LlmEvent::Done).unwrap();
    assert_eq!((session.ops().count(), session.refused()), (1, 5));
    assert_eq!(
        shown(session.document()),
        "paragraph \"ONE\"\nparagraph \"two\"\nparagraph \"three\"\n"
    );
    // An answer with no edits leaves nothing to review.
    let (main, _) = three();
    let mut session = AiSession::new(&main).unwrap();
    session.prompt("What is this?").unwrap();
    session
        .feed(LlmEvent::TextDelta("Three paragraphs.".into()))
        .unwrap();
    session.feed(LlmEvent::Done).unwrap();
    assert_eq!(session.state(), &AiState::UserInput);
    assert!(session.changes().is_empty());
}

#[test]
fn accepting_keeps_what_others_did_meanwhile_and_leaves_out_what_they_touched() {
    let main = editor(blocks![p!["one"], p!["two"], p!["three"], p!["four"]]);
    let ids = ["one", "two", "three", "four"].map(|text| block(main.document(), text));
    let [one, two, three, four] = ids;
    let mut main = main;
    let mut session = AiSession::new(&main).unwrap();
    session.prompt("Go").unwrap();
    call(&mut session, "1", "update", update(one, "ONE"));
    call(&mut session, "2", "update", update(two, "TWO"));
    call(
        &mut session,
        "3",
        "add",
        add(three, "after", &["after three"]),
    );
    call(&mut session, "4", "delete", json!({"id": four.to_string()}));
    session.feed(LlmEvent::Done).unwrap();

    // Meanwhile, in the document: a block the model changed is changed, the
    // block it added next to is removed, the one it removed is written in,
    // and a new one is made.
    main.apply(|doc, _| {
        doc.insert_text(two, 3, " (edited)")?;
        doc.remove_block(three)?;
        doc.insert_text(four, 4, " more")?;
        let new = doc.insert_block(None, 0, "paragraph")?;
        doc.insert_text(new, 0, "theirs")?;
        Ok(None)
    })
    .unwrap();

    let accepted = session.accept(&mut main).unwrap();
    assert_eq!(
        accepted,
        Accepted {
            applied: 1,
            skipped: 3
        }
    );
    assert_eq!(
        shown(main.document()),
        "paragraph \"theirs\"\nparagraph \"ONE\"\nparagraph \"two (edited)\"\nparagraph \"four more\"\n"
    );
    // The step back takes the model's edit, and not theirs.
    main.undo().unwrap();
    assert_eq!(main.document().plain_text(one).unwrap(), "one");
    assert_eq!(main.document().plain_text(two).unwrap(), "two (edited)");
}

#[test]
fn a_later_turn_changes_what_an_earlier_one_made() {
    let (mut main, [one, ..]) = three();
    let mut session = AiSession::new(&main).unwrap();
    session.prompt("Add a list").unwrap();
    call(
        &mut session,
        "1",
        "add",
        add(one, "after", &["- item\n  - inside"]),
    );
    session.feed(LlmEvent::TextDelta("Added.".into())).unwrap();
    session.feed(LlmEvent::Done).unwrap();
    let inside = block(session.document(), "inside");
    assert!(!main.document().contains(inside));

    let request = session.prompt("Make the inner one a task").unwrap();
    let roles: Vec<_> = request
        .messages
        .iter()
        .map(|message| message.role)
        .collect();
    assert_eq!(roles, [Role::User, Role::Assistant, Role::User]);
    assert_eq!(request.messages[0].text, "Add a list");
    assert!(request.messages[2].text.contains(&inside.to_string()));
    call(&mut session, "1", "update", update(inside, "- [x] inside"));
    call(
        &mut session,
        "2",
        "add",
        add(inside, "after", &["- [ ] next"]),
    );
    session.feed(LlmEvent::Done).unwrap();

    assert_eq!(
        session.accept(&mut main).unwrap(),
        Accepted {
            applied: 3,
            skipped: 0
        }
    );
    assert_eq!(shown(main.document()), shown(session.document()));
    assert!(shown(main.document()).contains("  check_list_item checked=true \"inside\"\n"));
    main.undo().unwrap();
    assert_eq!(main.document().blocks().len(), 3);
}

#[test]
fn a_request_says_what_the_user_has_selected() {
    let (mut main, [one, two, _]) = three();
    let selected = |main: &Editor| {
        let request = AiSession::new(main).unwrap().prompt("Go").unwrap();
        let text = &request.messages[0].text;
        let json = text
            .trim_start_matches("<document>\n")
            .split("\n</document>")
            .next()
            .unwrap();
        serde_json::from_str::<serde_json::Value>(json).unwrap()
    };
    let state = selected(&main);
    assert_eq!(
        state["blocks"][0],
        json!({"id": one.to_string(), "depth": 0, "block": "one"})
    );
    assert!(state.get("cursor").is_none() && state.get("selected").is_none());

    main.set_selection(Some(Selection::caret(Pos::new(two, 1))))
        .unwrap();
    assert_eq!(selected(&main)["cursor"], json!(two.to_string()));

    let range = Selection::Text {
        anchor: Pos::new(one, 1),
        head: Pos::new(two, 2),
    };
    main.set_selection(Some(range)).unwrap();
    let state = selected(&main);
    assert_eq!(state["selected"], json!([one.to_string(), two.to_string()]));
    assert_eq!(state["selectedText"], json!("ne\ntw"));
}

#[tokio::test]
async fn a_provider_s_answer_is_taken_to_its_end_or_its_failure() {
    let (mut main, [one, two, _]) = three();
    let mut events = tool_call("1", "update", &update_text(one, "ONE"), 7);
    events.push(Ok(LlmEvent::TextDelta("Done.".into())));
    events.push(Ok(LlmEvent::Done));
    let mut failing = tool_call("1", "update", &update_text(two, "TWO"), 7);
    failing.truncate(2);
    failing.push(Err(LlmError("overloaded".into())));
    let provider = MockProvider::new().answer(events).answer(failing);

    let mut session = AiSession::new(&main).unwrap();
    let request = session.prompt("Shout the first").unwrap();
    session.run(&provider, request.clone()).await.unwrap();
    assert_eq!(session.state(), &AiState::Reviewing);
    assert_eq!(provider.requests(), [request]);

    let request = session.prompt("And the second").unwrap();
    session.run(&provider, request).await.unwrap();
    assert_eq!(session.state(), &AiState::Error("overloaded".into()));
    // No answer is written for the third request: it ends at once.
    let request = session.retry().unwrap();
    session.run(&provider, request).await.unwrap();
    assert_eq!(session.state(), &AiState::Reviewing);
    session.accept(&mut main).unwrap();
    assert_eq!(main.document().plain_text(one).unwrap(), "ONE");
    assert_eq!(main.document().plain_text(two).unwrap(), "two");
}
