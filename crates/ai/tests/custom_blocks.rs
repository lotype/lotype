use lotype_ai::{AiSession, AiState, LlmEvent};
use lotype_core::{
    Atom, BlockId, BlockSpec, Content, Document, Editor, InlineSpec, Role, Schema, Value, attr,
    block,
};
use serde_json::json;

/// The built-in schema with a callout of rich text, a diagram of source
/// text and a video that holds no text.
fn schema() -> Schema {
    let callout = BlockSpec::new("callout", Content::Inline)
        .written_as_fence("callout")
        .with_attrs(["tone"])
        .with_hint("callouts that stand out from the text");
    let diagram = BlockSpec::new("diagram", Content::Inline)
        .with_role(Role::Code)
        .written_as_fence("mermaid")
        .with_hint("diagrams in Mermaid syntax");
    let video = BlockSpec::new("video", Content::None)
        .written_as_fence("video")
        .with_attrs(["url"]);
    Schema::builtin().block(callout).block(diagram).block(video)
}

fn editor_of(markdown: &str) -> (Editor, Vec<BlockId>) {
    let mut doc = Document::new(schema());
    let made = doc.insert_markdown(None, 0, markdown).unwrap();
    (Editor::new(doc), made)
}

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

#[test]
fn a_model_is_told_of_the_types_an_application_added_and_shown_their_blocks() {
    let (main, _) = editor_of("```callout tone=info\nRead **this**.\n```");
    let mut session = AiSession::new(&main).unwrap();
    let request = session.prompt("Tidy up").unwrap();
    // Each type says how it is written, whether or not the document has one.
    for told in [
        "headings (`# Title`)",
        "callouts that stand out from the text (a fenced block tagged `callout`, with `tone` after the tag",
        "diagrams in Mermaid syntax (a fenced block tagged `mermaid`, and its text inside as it is)",
        "video (a fenced block tagged `video`, with `url` after the tag",
    ] {
        assert!(request.system.contains(told), "{told}\n{}", request.system);
    }
    let shown = &request.messages.last().unwrap().text;
    assert!(
        shown.contains("```callout tone=info\\nRead **this**.\\n```"),
        "{shown}"
    );
}

#[test]
fn a_model_adds_and_updates_blocks_of_an_application_s_types() {
    let (mut main, made) = editor_of("First.\n\n```callout tone=info\nOld text.\n```");
    let mut session = AiSession::new(&main).unwrap();
    session.prompt("Rework").unwrap();
    let add = json!({
        "referenceId": made[0].to_string(),
        "position": "after",
        "blocks": ["```mermaid\ngraph TD\n  A --> B\n```", "```video url=https://lotype.dev/a.mp4\n```"],
    });
    call(&mut session, "1", "add", add);
    call(
        &mut session,
        "2",
        "update",
        update(made[1], "```callout tone=warning\nNew *text*.\n```"),
    );
    session.feed(LlmEvent::Done).unwrap();
    assert_eq!(session.state(), &AiState::Reviewing);
    session.accept(&mut main).unwrap();
    let doc = main.document();
    let kinds: Vec<_> = doc
        .blocks()
        .iter()
        .map(|block| doc.block_type(*block).unwrap())
        .collect();
    assert_eq!(kinds, ["paragraph", "diagram", "video", "callout"]);
    let blocks = doc.blocks();
    assert_eq!(doc.plain_text(blocks[1]).unwrap(), "graph TD\n  A --> B");
    assert_eq!(
        doc.attr(blocks[2], "url").unwrap(),
        Some("https://lotype.dev/a.mp4".into())
    );
    // The callout is the block it was, with its new tone and text.
    assert_eq!(blocks[3], made[1]);
    assert_eq!(doc.attr(made[1], "tone").unwrap(), Some("warning".into()));
    assert_eq!(doc.plain_text(made[1]).unwrap(), "New text.");
}

#[test]
fn an_update_keeps_what_markdown_says_nothing_of() {
    let (mut main, made) =
        editor_of("## Title\n\n```video url=https://lotype.dev/a.mp4\n```\n\n> [!NOTE]\n> Inside.");
    let [title, video, alert, inside] = main.document().blocks()[..] else {
        panic!("four blocks")
    };
    assert_eq!(made.len(), 3);
    main.apply(|doc, _| {
        doc.set_attr(title, attr::TEXT_ALIGNMENT, "center")?;
        doc.set_attr(title, attr::BACKGROUND_COLOR, "yellow")?;
        doc.set_attr(video, "poster", "https://lotype.dev/a.png")?;
        Ok(None)
    })
    .unwrap();
    let mut session = AiSession::new(&main).unwrap();
    session.prompt("Rework").unwrap();
    call(&mut session, "1", "update", update(title, "A plain line"));
    call(
        &mut session,
        "2",
        "update",
        update(video, "```video url=https://lotype.dev/b.mp4\n```"),
    );
    call(&mut session, "3", "update", update(alert, "> [!WARNING]"));
    session.feed(LlmEvent::Done).unwrap();
    session.accept(&mut main).unwrap();
    let doc = main.document();
    let value = |block, key| doc.attr(block, key).unwrap();
    // The heading is a paragraph: its level went, its look stayed.
    assert_eq!(doc.block_type(title).unwrap(), block::PARAGRAPH);
    assert_eq!(value(title, attr::LEVEL), None);
    assert_eq!(value(title, attr::TEXT_ALIGNMENT), Some("center".into()));
    assert_eq!(value(title, attr::BACKGROUND_COLOR), Some("yellow".into()));
    // The video and the alert are the blocks they were.
    assert_eq!(
        value(video, "url"),
        Some(Value::from("https://lotype.dev/b.mp4"))
    );
    assert_eq!(
        value(video, "poster"),
        Some("https://lotype.dev/a.png".into())
    );
    assert_eq!(value(alert, attr::KIND), Some("warning".into()));
    assert_eq!(doc.children(Some(alert)).unwrap(), [inside]);
}

#[test]
fn a_model_is_told_of_display_and_inline_math_and_edits_equations() {
    fn math_schema() -> Schema {
        fn inline_plain(atom: &Atom) -> String {
            let tex = match atom.attrs.get("tex") {
                Some(Value::Str(tex)) => tex.as_str(),
                _ => "",
            };
            format!("${tex}$")
        }

        let math_block = BlockSpec::new("math", Content::Inline)
            .with_role(Role::Code)
            .without_style()
            .written_as_fence("math")
            .as_display_math()
            .with_hint("display LaTeX equations");
        let math_inline = InlineSpec::new("inline_math", inline_plain)
            .as_inline_math("tex")
            .with_hint("inline LaTeX equations (`$...$`)");
        Schema::builtin().block(math_block).inline(math_inline)
    }

    let make_editor = || {
        let mut doc = Document::new(math_schema());
        let made = doc.insert_markdown(None, 0, "Intro.").unwrap();
        (Editor::new(doc), made)
    };

    let (mut main, made) = make_editor();
    let mut session = AiSession::new(&main).unwrap();
    let request = session.prompt("Add math").unwrap();
    assert!(
        request.system.contains(
            "display LaTeX equations (`$$\\n...\\n$$`, or a fenced block tagged `math`, and its text inside as it is)"
        ),
        "{}",
        request.system
    );
    assert!(
        request
            .system
            .contains("Inline text may also include inline LaTeX equations (`$...$`)."),
        "{}",
        request.system
    );

    // Repeating schema construction yields identical prompt wording and ordering.
    let (other, _) = make_editor();
    let mut other_session = AiSession::new(&other).unwrap();
    let request2 = other_session.prompt("Add math").unwrap();
    assert_eq!(request2.system, request.system);

    call(
        &mut session,
        "1",
        "update",
        update(made[0], "Energy $E = mc^2$ holds."),
    );
    call(
        &mut session,
        "2",
        "add",
        json!({
            "referenceId": made[0].to_string(),
            "position": "after",
            "blocks": [
                "$$\n\\int_0^1 x\\,dx = \\frac{1}{2}\n$$",
                "```math\nx = 1\n\ny = 2\n```"
            ],
        }),
    );
    session.feed(LlmEvent::Done).unwrap();
    assert_eq!(session.state(), &AiState::Reviewing);
    session.accept(&mut main).unwrap();

    let doc = main.document();
    let blocks = doc.blocks();
    let kinds: Vec<_> = blocks
        .iter()
        .map(|block| doc.block_type(*block).unwrap())
        .collect();
    assert_eq!(kinds, ["paragraph", "math", "math"]);
    let inline_atom = doc
        .spans(blocks[0])
        .unwrap()
        .into_iter()
        .find_map(|span| span.atom)
        .expect("inline math atom inserted in paragraph");
    assert_eq!(inline_atom.name, "inline_math");
    assert_eq!(inline_atom.attrs.get("tex"), Some(&Value::from("E = mc^2")));
    assert_eq!(
        doc.plain_text(blocks[1]).unwrap(),
        r"\int_0^1 x\,dx = \frac{1}{2}"
    );
    assert_eq!(doc.plain_text(blocks[2]).unwrap(), "x = 1\n\ny = 2");
}
