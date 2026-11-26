use lotype_core::{Document, Editor, Pos, Schema, Selection, attr, block};

fn read(markdown: &str) -> Document {
    let mut doc = Document::new(Schema::builtin());
    doc.insert_markdown(None, 0, markdown).unwrap();
    doc
}

#[test]
fn code_fence_parses_and_round_trips_language_title_twoslash_and_line_ranges() {
    let markdown = "```rust twoslash title=\"src/main.rs\" {1, 3-5}\nlet answer = 42;\n```\n";
    let doc = read(markdown);
    let code = doc.blocks()[0];
    assert_eq!(doc.block_type(code).unwrap(), block::CODE_BLOCK);
    assert_eq!(doc.code_language(code).as_deref(), Some("rust"));
    assert_eq!(doc.code_title(code).as_deref(), Some("src/main.rs"));
    assert!(doc.is_twoslash(code));
    assert_eq!(doc.code_highlights(code).as_deref(), Some("1,3-5"));

    let written = doc.to_markdown();
    assert_eq!(
        written,
        "```rust twoslash title=src/main.rs {1,3-5}\nlet answer = 42;\n```\n"
    );
    let round_tripped = read(&written);
    let copy = round_tripped.blocks()[0];
    assert_eq!(round_tripped.code_language(copy).as_deref(), Some("rust"));
    assert_eq!(
        round_tripped.code_title(copy).as_deref(),
        Some("src/main.rs")
    );
    assert!(round_tripped.is_twoslash(copy));
    assert_eq!(
        round_tripped.code_highlights(copy).as_deref(),
        Some("1,3-5")
    );
}

#[test]
fn code_fence_accepts_colon_and_bracket_title_shorthands_and_quoted_spaces() {
    let colon = read("```ts:packages/sdk/src/index.ts twoslash\nexport const x = 1;\n```\n");
    let first = colon.blocks()[0];
    assert_eq!(colon.code_language(first).as_deref(), Some("ts"));
    assert_eq!(
        colon.code_title(first).as_deref(),
        Some("packages/sdk/src/index.ts")
    );
    assert!(colon.is_twoslash(first));

    let bracket = read("```python [scripts/run build.py] {2}\nprint(1)\nprint(2)\n```\n");
    let second = bracket.blocks()[0];
    assert_eq!(bracket.code_language(second).as_deref(), Some("python"));
    assert_eq!(
        bracket.code_title(second).as_deref(),
        Some("scripts/run build.py")
    );
    assert_eq!(bracket.code_highlights(second).as_deref(), Some("2"));

    let back = read(&bracket.to_markdown());
    assert_eq!(
        back.code_title(back.blocks()[0]).as_deref(),
        Some("scripts/run build.py")
    );
}

#[test]
fn twoslash_is_on_by_default_and_can_be_disabled_in_markdown() {
    let default = read("```ts\nconst value = 1;\n```\n");
    assert!(default.is_twoslash(default.blocks()[0]));
    assert_eq!(default.to_markdown(), "```ts\nconst value = 1;\n```\n");

    let disabled = read("```ts twoslash=false\nconst value = 1;\n```\n");
    assert!(!disabled.is_twoslash(disabled.blocks()[0]));
    assert_eq!(
        disabled.to_markdown(),
        "```ts twoslash=false\nconst value = 1;\n```\n"
    );
}

#[test]
fn code_block_editor_commands_and_colon_input_rule_are_single_undo_steps() {
    let mut doc = Document::new(Schema::builtin());
    let p = doc.insert_block(None, 0, block::PARAGRAPH).unwrap();
    let mut editor = Editor::new(doc);
    editor
        .set_selection(Some(Selection::caret(Pos::new(p, 0))))
        .unwrap();
    for ch in "```rust:src/lib.rs ".chars() {
        editor.insert_text(&ch.to_string()).unwrap();
    }
    let code = editor.document().blocks()[0];
    assert_eq!(
        editor.document().block_type(code).unwrap(),
        block::CODE_BLOCK
    );
    assert_eq!(
        editor.document().code_language(code).as_deref(),
        Some("rust")
    );
    assert_eq!(
        editor.document().code_title(code).as_deref(),
        Some("src/lib.rs")
    );

    editor.set_twoslash(code, false).unwrap();
    assert!(!editor.document().is_twoslash(code));
    editor.set_code_highlights(code, Some("{2, 4-5}")).unwrap();
    assert_eq!(
        editor.document().code_highlights(code).as_deref(),
        Some("2,4-5")
    );
    editor.undo().unwrap();
    assert_eq!(editor.document().code_highlights(code), None);
    editor.undo().unwrap();
    assert!(editor.document().is_twoslash(code));
    assert_eq!(editor.document().attr(code, attr::TWOSLASH).unwrap(), None);
}
