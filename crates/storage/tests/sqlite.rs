use lotype_core::test_support::{assert_document, build};
use lotype_core::{Document, Schema, blocks, p};
use lotype_storage::{DocId, DocStore, SqliteStore};

fn opened(parts: &[Vec<u8>]) -> Document {
    let mut doc = Document::new(Schema::builtin());
    for part in parts {
        doc.import(part).unwrap();
    }
    doc
}

#[tokio::test]
async fn a_document_is_what_was_appended_to_it() {
    let store = SqliteStore::open(":memory:").unwrap();
    let (id, other) = (DocId::new("notes"), DocId::new("other"));
    assert!(store.load(&id).await.unwrap().is_empty());

    let (mut doc, _) = build(blocks![p!["one"]]);
    store
        .append_update(&id, &doc.export_snapshot().unwrap())
        .await
        .unwrap();
    let before = doc.version();
    let block = doc.blocks()[0];
    doc.insert_text(block, 3, " two").unwrap();
    store
        .append_update(&id, &doc.export_updates(&before).unwrap())
        .await
        .unwrap();

    // Twice: the first load makes the rows one, which the second reads.
    for _ in 0..2 {
        let parts = store.load(&id).await.unwrap();
        assert_eq!(parts.len(), 1);
        let loaded = opened(&parts);
        assert_document(&loaded, None, blocks![p!["one two"]]);
        assert_eq!(loaded.version(), doc.version());
    }
    assert!(store.load(&other).await.unwrap().is_empty());

    // And what comes after a load is kept with it.
    let before = doc.version();
    doc.insert_text(block, 0, "zero ").unwrap();
    let update = doc.export_updates(&before).unwrap();
    store.clone().append_update(&id, &update).await.unwrap();
    let loaded = opened(&store.load(&id).await.unwrap());
    assert_document(&loaded, None, blocks![p!["zero one two"]]);
}

#[tokio::test]
async fn rows_that_are_no_document_fail_the_load_and_stay() {
    let store = SqliteStore::open(":memory:").unwrap();
    let id = DocId::new("broken");
    let (doc, _) = build(blocks![p!["kept"]]);
    let snapshot = doc.export_snapshot().unwrap();
    store.append_update(&id, &snapshot).await.unwrap();
    store.append_update(&id, b"not an update").await.unwrap();
    assert!(store.load(&id).await.is_err());
    assert!(store.load(&id).await.is_err());
}

#[tokio::test]
async fn an_update_stored_before_the_one_it_comes_after_waits_for_it() {
    let store = SqliteStore::open(":memory:").unwrap();
    let id = DocId::new("gap");
    let (mut doc, _) = build(blocks![p!["base"]]);
    store
        .append_update(&id, &doc.export_snapshot().unwrap())
        .await
        .unwrap();
    let block = doc.blocks()[0];
    let before = doc.version();
    doc.insert_text(block, 4, " one").unwrap();
    let first = doc.export_updates(&before).unwrap();
    let before = doc.version();
    doc.insert_text(block, 8, " two").unwrap();
    let second = doc.export_updates(&before).unwrap();

    // Loaded without the first, the second is in no snapshot yet, and is kept.
    store.append_update(&id, &second).await.unwrap();
    for _ in 0..2 {
        let loaded = opened(&store.load(&id).await.unwrap());
        assert_document(&loaded, None, blocks![p!["base"]]);
    }
    store.append_update(&id, &first).await.unwrap();
    let loaded = opened(&store.load(&id).await.unwrap());
    assert_document(&loaded, None, blocks![p!["base one two"]]);
}
