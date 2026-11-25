use lotype_core::{BlockId, BlockSpec, Content, Document, Role, Schema, Value, attr, block};

/// The built-in schema with a callout of rich text, a diagram of source
/// text and a video that holds no text.
fn schema() -> Schema {
    let callout = BlockSpec::new("callout", Content::Inline)
        .written_as_fence("callout")
        .with_attrs(["tone", "title"])
        .with_hint("callouts");
    let diagram = BlockSpec::new("diagram", Content::Inline)
        .with_role(Role::Code)
        .written_as_fence("mermaid");
    let video = BlockSpec::new("video", Content::None)
        .written_as_fence("video")
        .with_attrs(["url", "autoplay", "start"]);
    Schema::builtin().block(callout).block(diagram).block(video)
}

fn read(markdown: &str) -> (Document, Vec<BlockId>) {
    let mut doc = Document::new(schema());
    let made = doc.insert_markdown(None, 0, markdown).unwrap();
    (doc, made)
}

fn attr_of(doc: &Document, block: BlockId, key: &str) -> Option<Value> {
    doc.attr(block, key).unwrap()
}

#[test]
fn a_fence_with_a_type_s_tag_is_a_block_of_that_type() {
    let (doc, made) = read(
        "```callout tone=warning title=\"Read this first\"\nBack up **before** you upgrade.\n```",
    );
    let [callout] = made[..] else {
        panic!("one block")
    };
    assert_eq!(doc.block_type(callout).unwrap(), "callout");
    assert_eq!(attr_of(&doc, callout, "tone"), Some("warning".into()));
    assert_eq!(
        attr_of(&doc, callout, "title"),
        Some("Read this first".into())
    );
    // What is inside is its text, with the marks Markdown gives it.
    assert_eq!(
        doc.plain_text(callout).unwrap(),
        "Back up before you upgrade."
    );
    let bold = doc.spans(callout).unwrap();
    assert!(
        bold.iter()
            .any(|span| span.text == "before" && span.marks.contains_key("bold"))
    );
}

#[test]
fn text_as_typed_stays_as_typed_and_a_block_without_text_takes_none() {
    let (doc, made) = read(
        "```mermaid\ngraph TD\n  A --> **B**\n```\n\n```video url=https://lotype.dev/a.mp4 autoplay=true start=12\nignored\n```",
    );
    let [diagram, video] = made[..] else {
        panic!("two blocks")
    };
    assert_eq!(doc.block_type(diagram).unwrap(), "diagram");
    assert_eq!(doc.plain_text(diagram).unwrap(), "graph TD\n  A --> **B**");
    assert_eq!(doc.block_type(video).unwrap(), "video");
    assert_eq!(attr_of(&doc, video, "autoplay"), Some(true.into()));
    assert_eq!(attr_of(&doc, video, "start"), Some(12.into()));
    assert!(doc.children(Some(video)).unwrap().is_empty());
}

#[test]
fn only_the_attributes_a_type_names_are_read_and_a_tag_no_type_has_stays_code() {
    let (doc, made) = read(
        "```callout tone=info text_color=red onclick=\"x\"\nHi\n```\n\n```rust\nfn main() {}\n```",
    );
    let [callout, code] = made[..] else {
        panic!("two blocks")
    };
    assert_eq!(
        doc.attrs(callout).unwrap().into_keys().collect::<Vec<_>>(),
        ["tone"]
    );
    assert_eq!(doc.block_type(code).unwrap(), block::CODE_BLOCK);
    assert_eq!(attr_of(&doc, code, attr::LANGUAGE), Some("rust".into()));
    // Without the type in the schema, the same fence is code too.
    let mut plain = Document::new(Schema::builtin());
    let made = plain
        .insert_markdown(None, 0, "```callout tone=info\nHi\n```")
        .unwrap();
    assert_eq!(plain.block_type(made[0]).unwrap(), block::CODE_BLOCK);
}

#[test]
fn what_is_written_reads_back_the_same() {
    let values = [
        "plain",
        "two words",
        "",
        "true",
        "42",
        "a=b",
        "say \"hi\" \\ `tick` & 100%",
        "línea\nnueva",
    ];
    for value in values {
        let mut doc = Document::new(schema());
        let callout = doc.insert_block(None, 0, "callout").unwrap();
        doc.set_attr(callout, "title", value).unwrap();
        doc.set_attr(callout, attr::TEXT_ALIGNMENT, "center")
            .unwrap();
        doc.insert_text(callout, 0, "Some ``` text").unwrap();
        let markdown = doc.to_markdown();
        // Alignment is none of the type's own in Markdown.
        assert!(!markdown.contains("center"), "{markdown}");
        let (back, made) = read(&markdown);
        assert_eq!(made.len(), 1, "{markdown}");
        assert_eq!(back.block_type(made[0]).unwrap(), "callout", "{markdown}");
        assert_eq!(
            attr_of(&back, made[0], "title"),
            Some(value.into()),
            "{markdown}"
        );
        assert_eq!(
            back.plain_text(made[0]).unwrap(),
            "Some ``` text",
            "{markdown}"
        );
    }
}

#[test]
fn a_value_is_read_as_a_model_writes_it() {
    let (doc, made) = read(
        "```video url=\"https://x.dev/a%20b.mp4?q=1&r=2\"\n```\n\n```callout title='Say \"hi\"'\nHi\n```",
    );
    // A percent sign in an address is the address's own.
    let url = "https://x.dev/a%20b.mp4?q=1&r=2";
    assert_eq!(attr_of(&doc, made[0], "url"), Some(url.into()));
    assert_eq!(attr_of(&doc, made[1], "title"), Some("Say \"hi\"".into()));
    let (back, made) = read(&doc.to_markdown());
    assert_eq!(attr_of(&back, made[0], "url"), Some(url.into()));
    assert_eq!(attr_of(&back, made[1], "title"), Some("Say \"hi\"".into()));
}

#[test]
fn text_that_looks_like_other_blocks_stays_the_block_s_text() {
    for text in [
        "- not a list",
        "# not a heading",
        "> not a quote",
        "1. not numbered",
        "one\n- two",
    ] {
        let mut doc = Document::new(schema());
        let callout = doc.insert_block(None, 0, "callout").unwrap();
        doc.insert_text(callout, 0, text).unwrap();
        let (back, made) = read(&doc.to_markdown());
        assert_eq!(made.len(), 1, "{text}");
        assert_eq!(back.plain_text(made[0]).unwrap(), text);
    }
    // What a model nests in a fence is all text of the one block.
    let (doc, made) = read("```callout\n- a\n  - nested\n- b\n```");
    assert_eq!(doc.plain_text(made[0]).unwrap(), "a\nnested\nb");
}

#[test]
fn the_types_are_listed_in_the_order_they_were_added() {
    let schema = schema();
    let names: Vec<_> = schema.block_specs().map(BlockSpec::name).collect();
    assert_eq!(names[0], block::PARAGRAPH);
    assert_eq!(names[names.len() - 3..], ["callout", "diagram", "video"]);
    assert_eq!(
        schema.fenced("mermaid").map(BlockSpec::name),
        Some("diagram")
    );
    let hints: Vec<_> = schema.block_specs().filter_map(BlockSpec::hint).collect();
    assert!(hints.contains(&"headings (`# Title`)") && hints.contains(&"callouts"));
}
