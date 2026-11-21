use lotype_core::{BlockId, Document, Error, MAX_DEPTH, Schema, Value};

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
fn sibling_lookups_see_uncommitted_tree_edits() {
    let mut doc = doc();
    let a = paragraph(&mut doc, None, 0);
    let b = paragraph(&mut doc, None, 1);
    assert_eq!(doc.next_block(a).unwrap(), Some(b));

    let c = paragraph(&mut doc, None, 1);
    assert_eq!(doc.next_block(a).unwrap(), Some(c));
    assert_eq!(doc.prev_block(b).unwrap(), Some(c));
    doc.move_block(b, Some(a), 0).unwrap();
    assert_eq!(doc.next_block(a).unwrap(), Some(b));
    assert_eq!(doc.prev_block(c).unwrap(), Some(b));
    assert_eq!(doc.path(b).unwrap(), [0, 0]);
    doc.remove_block(b).unwrap();
    assert_eq!(doc.next_block(a).unwrap(), Some(c));
    assert_eq!(doc.index_of(c).unwrap(), 1);
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

    let text = paragraph(&mut doc, None, 2);
    doc.insert_text(text, 0, "words").unwrap();
    assert!(matches!(
        doc.set_block_type(text, "divider"),
        Err(Error::TextNotAllowed(name)) if name == "divider"
    ));
    doc.delete_text(text, 0..5).unwrap();
    doc.set_block_type(text, "divider").unwrap();
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
fn nesting_stops_at_max_depth() {
    let mut doc = doc();
    let mut chain = Vec::new();
    for _ in 0..MAX_DEPTH {
        chain.push(paragraph(&mut doc, chain.last().copied(), 0));
    }
    let deepest = *chain.last().unwrap();
    assert!(matches!(
        doc.insert_block(Some(deepest), 0, "paragraph"),
        Err(Error::TooDeep)
    ));
    assert_eq!(doc.blocks().len(), MAX_DEPTH);
    assert_eq!(doc.path(deepest).unwrap().len(), MAX_DEPTH);

    let pair = paragraph(&mut doc, None, 1);
    let child = paragraph(&mut doc, Some(pair), 0);
    assert!(matches!(
        doc.move_block(pair, Some(chain[MAX_DEPTH - 2]), 0),
        Err(Error::TooDeep)
    ));
    assert!(matches!(
        doc.move_block(child, Some(deepest), 0),
        Err(Error::TooDeep)
    ));
    doc.move_block(child, Some(chain[MAX_DEPTH - 2]), 1)
        .unwrap();
    doc.move_block(pair, Some(chain[MAX_DEPTH - 3]), 1).unwrap();
    assert_eq!(doc.children(None).unwrap(), [chain[0]]);
}

#[test]
fn a_cyclic_move_is_reported_as_such_even_in_a_tall_tree() {
    let mut doc = doc();
    let mut chain = Vec::new();
    for _ in 0..MAX_DEPTH {
        chain.push(paragraph(&mut doc, chain.last().copied(), 0));
    }
    for target in [chain[0], chain[1], chain[MAX_DEPTH - 2]] {
        assert!(matches!(
            doc.move_block(chain[0], Some(target), 0),
            Err(Error::CyclicMove)
        ));
    }
    assert_eq!(doc.blocks().len(), MAX_DEPTH);
}

#[test]
fn a_block_id_round_trips_through_text() {
    let mut doc = doc();
    let block = paragraph(&mut doc, None, 0);
    let parsed: BlockId = block.to_string().parse().unwrap();
    assert_eq!(parsed, block);
    assert!(doc.contains(parsed));

    for text in ["", "block", "1@", "1@2@3"] {
        let error = text.parse::<BlockId>().unwrap_err();
        assert!(matches!(error, Error::InvalidBlockId(found) if found == text));
    }
}
