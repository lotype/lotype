use std::collections::BTreeMap;

use lotype_core::{
    Atom, BlockId, Document, Editor, Error, InlineSpec, OBJECT, Pos, Schema, Selection, Span, Value,
};

fn mention_text(atom: &Atom) -> String {
    match atom.attrs.get("label") {
        Some(Value::Str(label)) => format!("@{label}"),
        _ => "@".to_owned(),
    }
}

fn schema() -> Schema {
    Schema::builtin().inline(InlineSpec::new("mention", mention_text))
}

fn ann() -> Atom {
    Atom::new("mention").attr("id", "u1").attr("label", "Ann")
}

fn bold() -> BTreeMap<String, Value> {
    BTreeMap::from([("bold".to_owned(), Value::from(true))])
}

/// A document with one paragraph of `text`, and an editor with the caret at `caret`.
fn editor(text: &str, caret: usize) -> (Editor, BlockId) {
    let mut doc = Document::new(schema());
    let block = doc.insert_block(None, 0, "paragraph").unwrap();
    doc.insert_text(block, 0, text).unwrap();
    doc.commit();
    let mut editor = Editor::new(doc);
    let caret = Selection::caret(Pos::new(block, caret));
    editor.set_selection(Some(caret)).unwrap();
    (editor, block)
}

#[test]
fn an_atom_is_one_character_with_its_type_attributes_and_marks() {
    let mut doc = Document::new(schema());
    let block = doc.insert_block(None, 0, "paragraph").unwrap();
    let spans = [
        Span::new("hi ", BTreeMap::new()),
        Span::atom(ann(), bold()),
        Span::new("!", BTreeMap::new()),
    ];
    doc.insert_spans(block, 0, &spans).unwrap();
    assert_eq!(doc.spans(block).unwrap(), spans);
    assert_eq!(doc.text(block).unwrap(), format!("hi {OBJECT}!"));
    assert_eq!(doc.plain_text(block).unwrap(), "hi @Ann!");
    // The bold mark is the atom's; the atom itself is no mark.
    let marks = doc
        .marks_at(Pos::new(block, 3 + OBJECT.len_utf8()))
        .unwrap();
    assert_eq!(marks, bold());
    assert_eq!(doc.to_markdown(), "hi **@Ann**!\n");
}

#[test]
fn only_known_atoms_go_in() {
    let mut doc = Document::new(schema());
    let block = doc.insert_block(None, 0, "paragraph").unwrap();
    let unknown = Span::atom(Atom::new("poll"), BTreeMap::new());
    let error = doc.insert_spans(block, 0, &[unknown]);
    assert!(matches!(error, Err(Error::UnknownAtom(name)) if name == "poll"));
}

#[test]
fn typing_between_two_like_atoms_types_text() {
    let (mut ed, block) = editor("", 0);
    ed.insert_atom(ann()).unwrap();
    ed.insert_atom(ann()).unwrap();
    let between = Pos::new(block, OBJECT.len_utf8());
    ed.set_selection(Some(Selection::caret(between))).unwrap();
    ed.insert_text("x").unwrap();
    let spans = ed.document().spans(block).unwrap();
    let atoms = spans.iter().filter(|span| span.atom.is_some()).count();
    assert_eq!(atoms, 2);
    assert_eq!(ed.document().plain_text(block).unwrap(), "@Annx@Ann");
}

#[test]
fn an_atom_is_inserted_undone_and_deleted_as_one() {
    let (mut ed, block) = editor("ab", 1);
    ed.insert_atom(ann()).unwrap();
    assert_eq!(ed.document().plain_text(block).unwrap(), "a@Annb");
    ed.delete_backward().unwrap();
    assert_eq!(ed.document().text(block).unwrap(), "ab");
    assert!(ed.undo().unwrap());
    assert_eq!(ed.document().plain_text(block).unwrap(), "a@Annb");
    assert!(ed.undo().unwrap());
    assert_eq!(ed.document().text(block).unwrap(), "ab");
}

#[test]
fn atoms_survive_splits_joins_and_slices() {
    let (mut ed, block) = editor("ab", 1);
    ed.insert_atom(ann()).unwrap();
    // Split right after the atom, and join again.
    ed.split_block().unwrap();
    let blocks = ed.document().blocks();
    assert_eq!(ed.document().plain_text(blocks[0]).unwrap(), "a@Ann");
    ed.join_backward().unwrap();
    assert_eq!(ed.document().plain_text(block).unwrap(), "a@Annb");
    let end = Pos::new(block, ed.document().text_len(block).unwrap());
    let slice = ed.document().slice(Pos::new(block, 0), end).unwrap();
    let copy = slice.blocks()[0];
    assert_eq!(slice.spans(copy).unwrap()[1].atom, Some(ann()));
}

#[test]
fn atoms_from_two_peers_at_one_place_both_stay() {
    let (mut a, block) = editor("ab", 1);
    let mut b = Document::new(schema());
    b.import(&a.document().export_snapshot().unwrap()).unwrap();
    a.insert_atom(ann()).unwrap();
    let bob = Span::atom(Atom::new("mention").attr("label", "Bob"), BTreeMap::new());
    b.insert_spans(block, 1, &[bob]).unwrap();
    b.commit();
    let from_b = b.export_updates(&a.document().version()).unwrap();
    let from_a = a.document().export_updates(&b.version()).unwrap();
    a.import(&from_b).unwrap();
    b.import(&from_a).unwrap();
    let text = a.document().plain_text(block).unwrap();
    assert_eq!(text, b.plain_text(block).unwrap());
    assert!(text == "a@Ann@Bobb" || text == "a@Bob@Annb", "{text}");
}
