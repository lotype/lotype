use lotype_core::{BlockId, Document, Error, Schema, Value};

fn doc() -> Document {
    Document::new(Schema::builtin())
}

fn paragraph(doc: &mut Document, parent: Option<BlockId>, index: usize) -> BlockId {
    doc.insert_block(parent, index, "paragraph").unwrap()
}

#[test]
fn new_document_is_empty() {
    let doc = doc();
    assert!(doc.children(None).unwrap().is_empty());
    assert!(doc.blocks().is_empty());
}

#[test]
fn inserts_blocks_at_index() {
    let mut doc = doc();
    let a = paragraph(&mut doc, None, 0);
    let c = paragraph(&mut doc, None, 1);
    let b = paragraph(&mut doc, None, 1);
    assert_eq!(doc.children(None).unwrap(), [a, b, c]);
    assert_eq!(doc.block_type(b).unwrap(), "paragraph");
    assert_eq!(doc.parent(b).unwrap(), None);
    assert_eq!(doc.index_of(c).unwrap(), 2);
}

#[test]
fn rejects_invalid_inserts() {
    let mut doc = doc();
    assert!(matches!(
        doc.insert_block(None, 0, "video"),
        Err(Error::UnknownBlockType(name)) if name == "video"
    ));
    assert!(matches!(
        doc.insert_block(None, 1, "paragraph"),
        Err(Error::OutOfBounds { index: 1, len: 0 })
    ));
    let heading = doc.insert_block(None, 0, "heading").unwrap();
    assert!(matches!(
        doc.insert_block(Some(heading), 0, "paragraph"),
        Err(Error::ChildrenNotAllowed(name)) if name == "heading"
    ));
    assert!(doc.children(Some(heading)).unwrap().is_empty());
}

#[test]
fn nests_blocks() {
    let mut doc = doc();
    let a = paragraph(&mut doc, None, 0);
    let a1 = paragraph(&mut doc, Some(a), 0);
    assert_eq!(doc.children(Some(a)).unwrap(), [a1]);
    assert_eq!(doc.parent(a1).unwrap(), Some(a));
}

#[test]
fn remove_takes_descendants() {
    let mut doc = doc();
    let a = paragraph(&mut doc, None, 0);
    let a1 = paragraph(&mut doc, Some(a), 0);
    let b = paragraph(&mut doc, None, 1);
    doc.remove_block(a).unwrap();
    assert_eq!(doc.children(None).unwrap(), [b]);
    assert!(!doc.contains(a));
    assert!(!doc.contains(a1));
    assert!(matches!(doc.block_type(a1), Err(Error::BlockNotFound)));
    assert!(matches!(doc.remove_block(a), Err(Error::BlockNotFound)));
    assert!(matches!(
        doc.insert_block(Some(a), 0, "paragraph"),
        Err(Error::BlockNotFound)
    ));
}

#[test]
fn moves_within_and_across_parents() {
    let mut doc = doc();
    let a = paragraph(&mut doc, None, 0);
    let b = paragraph(&mut doc, None, 1);
    let c = paragraph(&mut doc, None, 2);

    doc.move_block(a, None, 2).unwrap();
    assert_eq!(doc.children(None).unwrap(), [b, c, a]);
    assert!(matches!(
        doc.move_block(a, None, 3),
        Err(Error::OutOfBounds { index: 3, len: 2 })
    ));

    doc.move_block(a, Some(b), 0).unwrap();
    assert_eq!(doc.children(None).unwrap(), [b, c]);
    assert_eq!(doc.children(Some(b)).unwrap(), [a]);
}

#[test]
fn rejects_invalid_moves() {
    let mut doc = doc();
    let a = paragraph(&mut doc, None, 0);
    let a1 = paragraph(&mut doc, Some(a), 0);
    let heading = doc.insert_block(None, 1, "heading").unwrap();

    assert!(matches!(
        doc.move_block(a, Some(a1), 0),
        Err(Error::CyclicMove)
    ));
    assert!(matches!(
        doc.move_block(a, Some(a), 0),
        Err(Error::CyclicMove)
    ));
    assert!(matches!(
        doc.move_block(a1, Some(heading), 0),
        Err(Error::ChildrenNotAllowed(_))
    ));
    assert_eq!(doc.children(Some(a)).unwrap(), [a1]);
}

#[test]
fn changes_block_type() {
    let mut doc = doc();
    let a = paragraph(&mut doc, None, 0);
    doc.set_block_type(a, "heading").unwrap();
    assert_eq!(doc.block_type(a).unwrap(), "heading");
    assert!(matches!(
        doc.set_block_type(a, "video"),
        Err(Error::UnknownBlockType(_))
    ));

    let parent = paragraph(&mut doc, None, 1);
    paragraph(&mut doc, Some(parent), 0);
    assert!(matches!(
        doc.set_block_type(parent, "heading"),
        Err(Error::ChildrenNotAllowed(_))
    ));
    assert_eq!(doc.block_type(parent).unwrap(), "paragraph");
}

#[test]
fn attributes() {
    let mut doc = doc();
    let a = doc.insert_block(None, 0, "heading").unwrap();
    assert_eq!(doc.attr(a, "level").unwrap(), None);
    doc.set_attr(a, "level", 2).unwrap();
    doc.set_attr(a, "id", "intro").unwrap();
    assert_eq!(doc.attr(a, "level").unwrap(), Some(Value::Int(2)));
    assert_eq!(
        doc.attrs(a).unwrap().into_iter().collect::<Vec<_>>(),
        [
            ("id".to_owned(), Value::from("intro")),
            ("level".to_owned(), Value::Int(2))
        ]
    );
    doc.remove_attr(a, "level").unwrap();
    assert_eq!(doc.attr(a, "level").unwrap(), None);
}

#[test]
fn reserved_attribute_keys() {
    let mut doc = doc();
    let a = paragraph(&mut doc, None, 0);
    for key in ["$type", "$text", "$anything"] {
        assert!(matches!(
            doc.set_attr(a, key, "x"),
            Err(Error::ReservedAttr(_))
        ));
        assert!(matches!(doc.attr(a, key), Err(Error::ReservedAttr(_))));
        assert!(matches!(
            doc.remove_attr(a, key),
            Err(Error::ReservedAttr(_))
        ));
    }
    assert_eq!(doc.block_type(a).unwrap(), "paragraph");
    assert!(doc.attrs(a).unwrap().is_empty());
}

/// a(a1(a11), a2), b
fn nested() -> (Document, [BlockId; 5]) {
    let mut doc = doc();
    let a = paragraph(&mut doc, None, 0);
    let a1 = paragraph(&mut doc, Some(a), 0);
    let a11 = paragraph(&mut doc, Some(a1), 0);
    let a2 = paragraph(&mut doc, Some(a), 1);
    let b = paragraph(&mut doc, None, 1);
    (doc, [a, a1, a11, a2, b])
}

#[test]
fn document_order() {
    let (doc, [a, a1, a11, a2, b]) = nested();
    let order = [a, a1, a11, a2, b];
    assert_eq!(doc.blocks(), order);

    for pair in order.windows(2) {
        assert_eq!(doc.next_block(pair[0]).unwrap(), Some(pair[1]));
        assert_eq!(doc.prev_block(pair[1]).unwrap(), Some(pair[0]));
    }
    assert_eq!(doc.next_block(b).unwrap(), None);
    assert_eq!(doc.prev_block(a).unwrap(), None);
}

#[test]
fn paths_sort_in_document_order() {
    let (doc, order) = nested();
    let paths: Vec<_> = order.iter().map(|b| doc.path(*b).unwrap()).collect();
    assert_eq!(
        paths,
        [vec![0], vec![0, 0], vec![0, 0, 0], vec![0, 1], vec![1]]
    );
    assert!(paths.is_sorted());
}

#[test]
fn deep_nesting_does_not_overflow_the_stack() {
    let mut doc = doc();
    let mut parent = None;
    for _ in 0..1_000 {
        parent = Some(paragraph(&mut doc, parent, 0));
    }
    assert_eq!(doc.blocks().len(), 1_000);
    assert_eq!(doc.path(parent.unwrap()).unwrap().len(), 1_000);
}
