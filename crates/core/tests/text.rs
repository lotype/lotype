use std::collections::BTreeMap;

use lotype_core::{BlockId, Document, Error, Schema, Span, Value};

fn paragraph(content: &str) -> (Document, BlockId) {
    let mut doc = Document::new(Schema::builtin());
    let block = doc.insert_block(None, 0, "paragraph").unwrap();
    doc.insert_text(block, 0, content).unwrap();
    (doc, block)
}

fn span(text: &str, marks: &[(&str, Value)]) -> Span {
    let marks = marks.iter().map(|(k, v)| ((*k).to_owned(), v.clone()));
    Span {
        text: text.to_owned(),
        marks: marks.collect::<BTreeMap<_, _>>(),
    }
}

fn bold() -> (&'static str, Value) {
    ("bold", Value::Bool(true))
}

#[test]
fn boolean_values_for_links_are_rejected_before_writing() {
    let (mut doc, block) = paragraph("a");
    let inserts = [span("b", &[bold()]), span("c", &[("link", true.into())])];
    assert!(matches!(
        doc.insert_spans(block, 1, &inserts),
        Err(Error::MarkValueRequired(_))
    ));
    assert!(matches!(
        doc.mark(block, 0..1, "link", true),
        Err(Error::MarkValueRequired(_))
    ));
    assert_eq!(doc.spans(block).unwrap(), [span("a", &[])]);
}

#[test]
fn inserts_and_deletes_by_byte_offset() {
    let (mut doc, block) = paragraph("héllo 😀");
    assert_eq!(doc.text_len(block).unwrap(), 11);

    doc.insert_text(block, 3, "X").unwrap();
    assert_eq!(doc.text(block).unwrap(), "héXllo 😀");
    doc.delete_text(block, 8..12).unwrap();
    assert_eq!(doc.text(block).unwrap(), "héXllo ");
    doc.delete_text(block, 3..3).unwrap();
    doc.insert_text(block, 0, "").unwrap();
    assert_eq!(doc.text(block).unwrap(), "héXllo ");
}

#[test]
fn rejects_offsets_inside_a_character() {
    let (mut doc, block) = paragraph("é😀");
    for offset in [1, 3, 4, 5] {
        assert!(matches!(
            doc.insert_text(block, offset, "x"),
            Err(Error::NotCharBoundary(o)) if o == offset
        ));
        assert!(matches!(
            doc.delete_text(block, 0..offset),
            Err(Error::NotCharBoundary(_))
        ));
        assert!(matches!(
            doc.mark(block, 0..offset, "bold", true),
            Err(Error::NotCharBoundary(_))
        ));
        assert!(matches!(
            doc.unmark(block, 0..offset, "bold"),
            Err(Error::NotCharBoundary(_))
        ));
        assert!(matches!(
            doc.spans_in(block, 0..offset),
            Err(Error::NotCharBoundary(_))
        ));
    }
    assert_eq!(doc.text(block).unwrap(), "é😀");
    assert_eq!(doc.spans(block).unwrap(), [span("é😀", &[])]);
}

#[test]
#[expect(clippy::reversed_empty_ranges)]
fn rejects_out_of_bounds_and_inverted_ranges() {
    let (mut doc, block) = paragraph("abc");
    assert!(matches!(
        doc.insert_text(block, 4, "x"),
        Err(Error::OutOfBounds { index: 4, len: 3 })
    ));
    assert!(matches!(
        doc.delete_text(block, 1..9),
        Err(Error::OutOfBounds { index: 9, len: 3 })
    ));
    assert!(matches!(
        doc.delete_text(block, 2..1),
        Err(Error::InvalidRange { start: 2, end: 1 })
    ));
    assert!(matches!(
        doc.mark(block, 2..1, "bold", true),
        Err(Error::InvalidRange { .. })
    ));
    assert_eq!(doc.text(block).unwrap(), "abc");
}

#[test]
fn blocks_without_inline_content_have_no_text() {
    let mut doc = Document::new(Schema::builtin());
    let divider = doc.insert_block(None, 0, "divider").unwrap();
    assert!(matches!(doc.text(divider), Err(Error::NoInlineContent)));
    assert!(matches!(
        doc.insert_text(divider, 0, "x"),
        Err(Error::NoInlineContent)
    ));
    assert!(matches!(doc.spans(divider), Err(Error::NoInlineContent)));
}

#[test]
fn marks_and_unmarks() {
    let link = || ("link", Value::from("https://lotype.dev"));
    let (mut doc, block) = paragraph("héllo world");
    doc.mark(block, 0..6, "bold", true).unwrap();
    doc.mark(block, 3..12, "link", "https://lotype.dev")
        .unwrap();
    assert_eq!(
        doc.spans(block).unwrap(),
        [
            span("hé", &[bold()]),
            span("llo", &[bold(), link()]),
            span(" world", &[link()]),
        ]
    );

    doc.unmark(block, 0..3, "bold").unwrap();
    doc.unmark(block, 3..3, "link").unwrap();
    assert_eq!(
        doc.spans(block).unwrap(),
        [
            span("hé", &[]),
            span("llo", &[bold(), link()]),
            span(" world", &[link()]),
        ]
    );
    assert_eq!(
        doc.spans_in(block, 3..5).unwrap(),
        [span("ll", &[bold(), link()])]
    );
}

#[test]
fn rejects_unknown_marks() {
    let (mut doc, block) = paragraph("abc");
    assert!(matches!(
        doc.mark(block, 0..1, "blink", true),
        Err(Error::UnknownMark(name)) if name == "blink"
    ));
    assert!(matches!(
        doc.unmark(block, 0..1, "blink"),
        Err(Error::UnknownMark(_))
    ));
    assert!(matches!(
        doc.insert_spans(
            block,
            0,
            &[span("x", &[]), span("y", &[("blink", Value::Bool(true))])]
        ),
        Err(Error::UnknownMark(_))
    ));
    assert_eq!(doc.text(block).unwrap(), "abc");
}

#[test]
fn typing_at_a_mark_edge_follows_its_expand_rule() {
    let (mut doc, block) = paragraph("ab");
    doc.mark(block, 0..1, "bold", true).unwrap();
    doc.mark(block, 1..2, "link", "u").unwrap();
    doc.insert_text(block, 1, "X").unwrap();
    doc.insert_text(block, 3, "Y").unwrap();
    assert_eq!(
        doc.spans(block).unwrap(),
        [
            span("aX", &[bold()]),
            span("b", &[("link", Value::from("u"))]),
            span("Y", &[]),
        ]
    );
}

#[test]
fn insert_spans_keeps_exactly_the_given_marks() {
    let (mut doc, block) = paragraph("ab");
    doc.mark(block, 0..2, "bold", true).unwrap();
    doc.insert_spans(
        block,
        2,
        &[
            span("plain", &[]),
            span("ital", &[("italic", Value::Bool(true))]),
        ],
    )
    .unwrap();
    doc.insert_spans(block, 1, &[span("é", &[])]).unwrap();
    assert_eq!(
        doc.spans(block).unwrap(),
        [
            span("a", &[bold()]),
            span("é", &[]),
            span("b", &[bold()]),
            span("plain", &[]),
            span("ital", &[("italic", Value::Bool(true))]),
        ]
    );
}
