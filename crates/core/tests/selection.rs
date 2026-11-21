use lotype_core::{BlockId, Document, Error, Pos, Schema, Selection};

/// a("héllo")(a1("x")), divider, b("world")
fn doc() -> (Document, [BlockId; 4]) {
    let mut doc = Document::new(Schema::builtin());
    let a = doc.insert_block(None, 0, "paragraph").unwrap();
    let a1 = doc.insert_block(Some(a), 0, "paragraph").unwrap();
    let divider = doc.insert_block(None, 1, "divider").unwrap();
    let b = doc.insert_block(None, 2, "paragraph").unwrap();
    doc.insert_text(a, 0, "héllo").unwrap();
    doc.insert_text(a1, 0, "x").unwrap();
    doc.insert_text(b, 0, "world").unwrap();
    (doc, [a, a1, divider, b])
}

#[test]
fn caret_is_collapsed() {
    let (_, [a, _, _, b]) = doc();
    assert!(Selection::caret(Pos::new(a, 0)).is_collapsed());
    let range = Selection::Text {
        anchor: Pos::new(a, 0),
        head: Pos::new(b, 0),
    };
    assert!(!range.is_collapsed());
    assert!(!Selection::Blocks(vec![a]).is_collapsed());
}

#[test]
fn checks_positions() {
    let (mut doc, [a, _, divider, b]) = doc();
    assert!(doc.check_pos(Pos::new(a, 0)).is_ok());
    assert!(doc.check_pos(Pos::new(a, 6)).is_ok());
    assert!(matches!(
        doc.check_pos(Pos::new(a, 7)),
        Err(Error::OutOfBounds { index: 7, len: 6 })
    ));
    assert!(matches!(
        doc.check_pos(Pos::new(a, 2)),
        Err(Error::NotCharBoundary(2))
    ));
    assert!(matches!(
        doc.check_pos(Pos::new(divider, 0)),
        Err(Error::NoInlineContent)
    ));
    doc.remove_block(b).unwrap();
    assert!(matches!(
        doc.check_pos(Pos::new(b, 0)),
        Err(Error::BlockNotFound)
    ));
}

#[test]
fn checks_selections() {
    let (mut doc, [a, a1, divider, b]) = doc();
    let text = Selection::Text {
        anchor: Pos::new(b, 5),
        head: Pos::new(a, 1),
    };
    assert!(doc.check_selection(&text).is_ok());
    assert!(
        doc.check_selection(&Selection::Blocks(vec![a1, divider]))
            .is_ok()
    );

    let bad_head = Selection::Text {
        anchor: Pos::new(a, 0),
        head: Pos::new(a, 99),
    };
    assert!(doc.check_selection(&bad_head).is_err());

    doc.remove_block(a).unwrap();
    assert!(matches!(
        doc.check_selection(&Selection::Blocks(vec![b, a1])),
        Err(Error::BlockNotFound)
    ));
}

#[test]
fn orders_positions_in_document_order() {
    let (doc, [a, a1, _, b]) = doc();
    let in_order = [
        Pos::new(a, 0),
        Pos::new(a, 6),
        Pos::new(a1, 0),
        Pos::new(a1, 1),
        Pos::new(b, 0),
    ];
    for (i, first) in in_order.iter().enumerate() {
        for second in &in_order[i..] {
            assert_eq!(doc.ordered(*first, *second).unwrap(), (*first, *second));
            assert_eq!(doc.ordered(*second, *first).unwrap(), (*first, *second));
        }
    }
    assert!(doc.ordered(Pos::new(a, 2), Pos::new(b, 0)).is_err());
}
