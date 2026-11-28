use lotype::core::{Document, Editor, Schema, block};

#[test]
fn core_reexport_works() {
    let mut doc = Document::new(Schema::builtin());
    let id = doc.insert_block(None, 0, block::PARAGRAPH).unwrap();
    doc.insert_text(id, 0, "hello").unwrap();

    let editor = Editor::new(doc);
    assert_eq!(editor.document().plain_text(id).unwrap(), "hello");
}

#[cfg(feature = "test-support")]
#[test]
fn core_test_support_reexport_works() {
    use lotype::core::p;
    use lotype::core::test_support::{assert_editor, editor};

    let ed = editor(vec![p!("hello<a>")]);
    assert_editor(&ed, vec![p!("hello<a>")]);
}

#[cfg(feature = "storage")]
#[test]
fn storage_reexport_works() {
    use lotype::storage::{DocId, merge, restore_with_schema};

    let id = DocId::new("doc-1");
    assert_eq!(id.as_str(), "doc-1");

    let mut doc = Document::new(Schema::builtin());
    let block_id = doc.insert_block(None, 0, block::PARAGRAPH).unwrap();
    doc.insert_text(block_id, 0, "stored").unwrap();
    let snap = doc.export_snapshot().unwrap();

    let merged = merge([&snap]).unwrap();
    let restored = restore_with_schema(Schema::builtin(), merged).unwrap();
    assert_eq!(restored.plain_text(block_id).unwrap(), "stored");
}

#[cfg(feature = "sqlite")]
#[test]
fn sqlite_reexport_works() {
    let _store = lotype::storage::SqliteStore::open(":memory:").unwrap();
}

#[cfg(feature = "server")]
#[test]
fn server_reexport_works() {
    fn _assert_router<A: lotype::server::Authorizer, S: lotype::server::DocStore>(
        auth: A,
        store: S,
    ) {
        let _ = lotype::server::router(auth, store);
    }

    let _ = lotype::server::Access::Write;
    let id = lotype::server::DocId::new("room-1");
    assert_eq!(id.as_str(), "room-1");
}

#[cfg(feature = "ai")]
#[test]
fn ai_reexport_works() {
    use lotype::ai::{AiState, Diff, word_diff};

    let new = "hello lotype";
    let diffs = word_diff("hello world", new);
    assert!(
        diffs
            .iter()
            .any(|d| matches!(d, Diff::Added(range) if &new[range.clone()] == "lotype"))
    );
    assert_eq!(AiState::UserInput, AiState::UserInput);
}

#[cfg(all(feature = "ai", feature = "test-support"))]
#[test]
fn ai_test_support_reexport_works() {
    let provider = lotype::ai::test_support::MockProvider::new();
    assert!(provider.requests().is_empty());
}

#[cfg(feature = "code")]
#[test]
fn code_reexport_works() {
    use lotype::code::{CodeBlockMeta, language_label};
    use lotype::core::attr;

    assert_eq!(language_label(Some("rs")), "Rust");

    let mut doc = Document::new(Schema::builtin());
    let block_id = doc.insert_block(None, 0, block::CODE_BLOCK).unwrap();
    doc.set_attr(block_id, attr::LANGUAGE, "ts").unwrap();
    doc.insert_text(block_id, 0, "const x = 1;").unwrap();

    let meta = CodeBlockMeta::of(&doc, block_id).unwrap();
    assert_eq!(meta.label, "TypeScript");
}

#[cfg(feature = "ui")]
#[test]
fn ui_reexport_works() {
    let _ = std::any::TypeId::of::<lotype::ui::EditorView>();
    let _ = std::any::TypeId::of::<lotype::ui::AiPanel>();
}

#[cfg(all(feature = "ui", feature = "test-support"))]
#[test]
fn ui_test_support_reexport_works() {
    fn _assert_type(_: lotype::ui::test_support::EditorTestContext<'_>) {}
}

#[cfg(all(feature = "code", feature = "ui"))]
#[test]
fn code_gpui_reexport_works() {
    let _ = std::any::TypeId::of::<lotype::code::CodeBlocks>();
    let _install = lotype::code::install;
}

#[cfg(feature = "math")]
#[test]
fn math_reexport_works() {
    let layout = lotype::math::typeset("E = mc^2", false).unwrap();
    assert!(layout.width > 0.);
    let atom = lotype::math::inline_atom("x^2");
    assert_eq!(lotype::math::inline_tex(&atom), "x^2");
}

#[cfg(all(feature = "math", feature = "ui"))]
#[test]
fn math_gpui_reexport_works() {
    let _ = std::any::TypeId::of::<lotype::math::MathBlocks>();
    let _install = lotype::math::install;
}
