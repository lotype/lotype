use std::collections::BTreeMap;

use lotype_core::test_support::{assert_editor, block, build, editor, marked};
use lotype_core::{
    Document, Editor, Error, Expand, MarkSpec, Pos, Schema, Selection, Value, b, blocks, divider,
    i, p,
};

fn type_each(ed: &mut Editor, text: &str) {
    for c in text.chars() {
        ed.insert_text(&c.to_string()).unwrap();
    }
}

fn marks(pairs: &[(&str, Value)]) -> BTreeMap<String, Value> {
    pairs
        .iter()
        .map(|(k, v)| ((*k).to_owned(), v.clone()))
        .collect()
}

fn caret_at(ed: &mut Editor, block: usize, offset: usize) {
    let block = ed.document().blocks()[block];
    ed.set_selection(Some(Selection::caret(Pos::new(block, offset))))
        .unwrap();
}

#[test]
fn typing_undoes_a_word_at_a_time() {
    let mut ed = editor(blocks![p!["<a>"]]);
    type_each(&mut ed, "hi  yo there");
    assert_editor(&ed, blocks![p!["hi  yo there<a>"]]);

    assert!(ed.undo().unwrap());
    assert_editor(&ed, blocks![p!["hi  yo <a>"]]);
    assert!(ed.undo().unwrap());
    assert_editor(&ed, blocks![p!["hi  <a>"]]);
    assert!(ed.undo().unwrap());
    assert_editor(&ed, blocks![p!["<a>"]]);
    assert!(!ed.undo().unwrap());

    assert!(ed.redo().unwrap());
    assert_editor(&ed, blocks![p!["hi  <a>"]]);
    assert!(ed.redo().unwrap());
    assert!(ed.redo().unwrap());
    assert_editor(&ed, blocks![p!["hi  yo there<a>"]]);
    assert!(!ed.redo().unwrap());
}

#[test]
fn moving_the_caret_or_another_command_ends_a_run() {
    let mut ed = editor(blocks![p!["<a>"]]);
    type_each(&mut ed, "ab");
    caret_at(&mut ed, 0, 2);
    type_each(&mut ed, "cd");
    ed.split_block().unwrap();
    type_each(&mut ed, "ef");
    assert_editor(&ed, blocks![p!["abcd"], p!["ef<a>"]]);

    ed.undo().unwrap();
    assert_editor(&ed, blocks![p!["abcd"], p!["<a>"]]);
    ed.undo().unwrap();
    assert_editor(&ed, blocks![p!["abcd<a>"]]);
    ed.undo().unwrap();
    assert_editor(&ed, blocks![p!["ab<a>"]]);
    ed.undo().unwrap();
    assert_editor(&ed, blocks![p!["<a>"]]);
}

#[test]
fn typing_over_a_selection_is_its_own_step_and_undo_restores_the_selection() {
    let mut ed = editor(blocks![p!["a<a>bc"], p!["d<b>e"]]);
    type_each(&mut ed, "XY");
    assert_editor(&ed, blocks![p!["aXY<a>e"]]);
    ed.undo().unwrap();
    assert_editor(&ed, blocks![p!["a<a>bc"], p!["d<b>e"]]);
    assert!(!ed.can_undo());
}

#[test]
fn an_empty_insert_changes_nothing() {
    let mut ed = editor(blocks![p!["a<a>"]]);
    ed.insert_text("").unwrap();
    assert!(!ed.can_undo());
    type_each(&mut ed, "b");
    ed.insert_text("").unwrap();
    type_each(&mut ed, "c");
    ed.undo().unwrap();
    assert_editor(&ed, blocks![p!["ab<a>"]]);
}

#[test]
fn a_mark_toggled_on_a_caret_applies_to_what_is_typed_next() {
    let mut ed = editor(blocks![p!["x<a>y"]]);
    ed.toggle_mark("bold").unwrap();
    assert_eq!(ed.active_marks().unwrap(), marks(&[("bold", true.into())]));
    assert!(!ed.can_undo());
    type_each(&mut ed, "ab");
    assert_editor(&ed, blocks![p!["x", b!["ab<a>"], "y"]]);

    ed.toggle_mark("italic").unwrap();
    type_each(&mut ed, "c");
    ed.toggle_mark("bold").unwrap();
    type_each(&mut ed, "d");
    assert_editor(&ed, blocks![p!["x", b!["ab", i!["c"]], i!["d<a>"], "y"]]);
    assert_eq!(
        ed.active_marks().unwrap(),
        marks(&[("italic", true.into())])
    );
}

#[test]
fn a_mark_toggled_off_inside_marked_text_stays_off_while_typing() {
    let mut ed = editor(blocks![p![b!["ab<a>cd"]]]);
    assert_eq!(ed.active_marks().unwrap(), marks(&[("bold", true.into())]));
    ed.toggle_mark("bold").unwrap();
    assert!(ed.active_marks().unwrap().is_empty());
    type_each(&mut ed, "XY");
    assert_editor(&ed, blocks![p![b!["ab"], "XY<a>", b!["cd"]]]);
}

#[test]
fn a_pending_mark_that_does_not_expand_still_covers_every_typed_character() {
    let mut ed = editor(blocks![p!["<a>"]]);
    ed.toggle_mark("code").unwrap();
    type_each(&mut ed, "let x");
    let code = marked("code", true, vec!["let x<a>".into()]);
    assert_editor(&ed, blocks![p![code]]);
}

#[test]
fn valued_marks_require_an_explicit_value_at_a_caret() {
    let link = |text: &str| marked("link", "https://lotype.dev", vec![text.into()]);
    let mut ed = editor(blocks![p![link("a<a>b")]]);
    let value = Value::from("https://lotype.dev");
    assert!(matches!(
        ed.toggle_mark("link"),
        Err(Error::MarkValueRequired(_))
    ));
    assert!(matches!(
        ed.set_mark("link", Some(true.into())),
        Err(Error::MarkValueRequired(_))
    ));
    assert_eq!(ed.active_marks().unwrap()["link"], value);

    ed.set_mark("link", None).unwrap();
    ed.insert_text("X").unwrap();
    ed.set_mark("link", Some(value)).unwrap();
    ed.insert_text("Y").unwrap();
    assert_editor(&ed, blocks![p![link("a"), "X", link("Y<a>b")]]);
    assert!(ed.undo().unwrap());
    assert_editor(&ed, blocks![p![link("a"), "X<a>", link("b")]]);
}

#[test]
fn setting_a_valued_mark_on_a_range_is_undoable() {
    let mut ed = editor(blocks![p!["<a>ab<b>"]]);
    let color = Value::from("blue");
    assert!(matches!(
        ed.toggle_mark("text_color"),
        Err(Error::MarkValueRequired(_))
    ));
    ed.set_mark("text_color", Some(color.clone())).unwrap();
    assert_eq!(ed.active_marks().unwrap()["text_color"], color);
    ed.set_mark("text_color", None).unwrap();
    assert!(ed.active_marks().unwrap().is_empty());
    assert!(ed.undo().unwrap());
    assert_eq!(ed.active_marks().unwrap()["text_color"], color);
}

#[test]
fn setting_the_same_selection_ends_the_run_but_keeps_pending_marks() {
    let code = |text: &str| marked("code", true, vec![text.into()]);
    let mut ed = editor(blocks![p!["<a>"]]);
    ed.toggle_mark("code").unwrap();
    ed.insert_text("a").unwrap();

    ed.set_selection(ed.selection().cloned()).unwrap();
    ed.insert_text("b").unwrap();
    assert_editor(&ed, blocks![p![code("ab<a>")]]);

    assert!(ed.undo().unwrap());
    assert_editor(&ed, blocks![p![code("a<a>")]]);
    assert!(ed.undo().unwrap());
    assert_editor(&ed, blocks![p!["<a>"]]);
}

#[test]
fn moving_the_caret_drops_pending_marks() {
    let mut ed = editor(blocks![p!["ab<a>"]]);
    ed.toggle_mark("bold").unwrap();
    caret_at(&mut ed, 0, 1);
    assert!(ed.active_marks().unwrap().is_empty());
    type_each(&mut ed, "X");
    assert_editor(&ed, blocks![p!["aX<a>b"]]);

    ed.toggle_mark("bold").unwrap();
    ed.undo().unwrap();
    type_each(&mut ed, "Z");
    assert_editor(&ed, blocks![p!["aZ<a>b"]]);
    assert!(ed.toggle_mark("blink").is_err());
}

#[test]
fn marks_at_predicts_what_typing_gets() {
    let link = |text: &str| marked("link", "https://lotype.dev", vec![text.into()]);
    let code = |text: &str| marked("code", true, vec![text.into()]);
    let content = || {
        blocks![p![
            "a",
            b!["bé"],
            link("cd"),
            b![i!["e"], "f"],
            code("g"),
            "h",
            link("i")
        ]]
    };
    let (doc, _) = build(content());
    let block = doc.blocks()[0];
    let text = doc.text(block).unwrap();

    for offset in (0..=text.len()).filter(|offset| text.is_char_boundary(*offset)) {
        let predicted = doc.marks_at(Pos::new(block, offset)).unwrap();
        let (mut typed, _) = build(content());
        let block = typed.blocks()[0];
        typed.insert_text(block, offset, "X").unwrap();
        let spans = typed.spans_in(block, offset..offset + 1).unwrap();
        assert_eq!(predicted, spans[0].marks, "at offset {offset}");
    }
}

#[test]
fn active_marks_of_a_range_are_those_all_of_it_has() {
    let link = |text: &str| marked("link", "https://lotype.dev", vec![text.into()]);
    let mut ed = editor(blocks![
        p!["x", b!["<a>ab", i!["c"]]],
        p![b![link("d"), i!["e<b>"]], "f"],
    ]);
    assert_eq!(ed.active_marks().unwrap(), marks(&[("bold", true.into())]));
    let blocks = ed.document().blocks();
    ed.set_selection(Some(Selection::Blocks(blocks))).unwrap();
    assert!(ed.active_marks().unwrap().is_empty());
    ed.set_selection(None).unwrap();
    assert!(ed.active_marks().unwrap().is_empty());
}

#[test]
fn editor_typing_uses_the_marks_reported_at_a_boundary() {
    let schema = Schema::builtin()
        .mark(MarkSpec::new("left", Expand::After))
        .mark(MarkSpec::new("right", Expand::Before));
    let mut doc = Document::new(schema);
    let block = doc.insert_block(None, 0, "paragraph").unwrap();
    doc.insert_text(block, 0, "ab").unwrap();
    doc.mark(block, 0..1, "left", true).unwrap();
    doc.mark(block, 0..1, "right", "old").unwrap();
    doc.mark(block, 1..2, "right", "new").unwrap();
    doc.mark(block, 0..1, "code", true).unwrap();
    doc.mark(block, 1..2, "code", true).unwrap();
    doc.commit();

    let mut ed = Editor::new(doc);
    ed.set_selection(Some(Selection::caret(Pos::new(block, 1))))
        .unwrap();
    let expected = marks(&[
        ("code", true.into()),
        ("left", true.into()),
        ("right", "new".into()),
    ]);
    assert_eq!(ed.active_marks().unwrap(), expected);
    ed.insert_text("X").unwrap();
    let spans = ed.document().spans_in(block, 1..2).unwrap();
    assert_eq!(spans[0].marks, expected);
}

#[test]
fn backspace_deletes_whole_characters_and_undoes_as_one_step() {
    let mut ed = editor(blocks![p!["a👨‍👩‍👧e\u{301}🇮🇩<a>z"]]);
    ed.delete_backward().unwrap();
    assert_editor(&ed, blocks![p!["a👨‍👩‍👧e\u{301}<a>z"]]);
    ed.delete_backward().unwrap();
    assert_editor(&ed, blocks![p!["a👨‍👩‍👧<a>z"]]);
    ed.delete_backward().unwrap();
    assert_editor(&ed, blocks![p!["a<a>z"]]);

    ed.undo().unwrap();
    assert_editor(&ed, blocks![p!["a👨‍👩‍👧e\u{301}🇮🇩<a>z"]]);
    assert!(!ed.can_undo());
}

#[test]
fn deleting_from_inside_a_grapheme_removes_it_whole() {
    for delete in [Editor::delete_backward, Editor::delete_forward] {
        for text in ["ae<a>\u{301}b", "a🇮<a>🇩b", "a👨<a>‍👩‍👧b", "a\r<a>\nb"] {
            let mut ed = editor(blocks![p![text]]);
            delete(&mut ed).unwrap();
            assert_editor(&ed, blocks![p!["a<a>b"]]);
            ed.undo().unwrap();
            assert_editor(&ed, blocks![p![text]]);
        }
    }
}

#[test]
fn deletion_keys_delete_selected_blocks() {
    for delete in [Editor::delete_backward, Editor::delete_forward] {
        let mut ed = editor(blocks![p!["a"], p!["b", p!["child"]]]);
        let block = ed.document().blocks()[1];
        ed.set_selection(Some(Selection::Blocks(vec![block])))
            .unwrap();
        delete(&mut ed).unwrap();
        assert_editor(&ed, blocks![p!["a"]]);
        assert_eq!(ed.selection(), None);
        ed.undo().unwrap();
        assert_editor(&ed, blocks![p!["a"], p!["b", p!["child"]]]);
        assert_eq!(
            ed.selection(),
            Some(&Selection::Blocks(vec![ed.document().blocks()[1]]))
        );
    }
}

#[test]
fn backspace_joins_blocks_and_deletes_selections() {
    let mut ed = editor(blocks![p!["ab"], p!["<a>cd"]]);
    ed.delete_backward().unwrap();
    assert_editor(&ed, blocks![p!["ab<a>cd"]]);
    ed.delete_backward().unwrap();
    assert_editor(&ed, blocks![p!["a<a>cd"]]);
    ed.undo().unwrap();
    ed.undo().unwrap();
    assert_editor(&ed, blocks![p!["ab"], p!["<a>cd"]]);

    let mut ed = editor(blocks![p!["a<a>b"], p!["c<b>d"]]);
    ed.delete_backward().unwrap();
    assert_editor(&ed, blocks![p!["a<a>d"]]);

    let mut ed = editor(blocks![p!["<a>only"]]);
    ed.delete_backward().unwrap();
    assert_editor(&ed, blocks![p!["<a>only"]]);
    assert!(!ed.can_undo());
}

#[test]
fn delete_removes_the_next_character_and_undoes_as_one_step() {
    let mut ed = editor(blocks![p!["a<a>🇮🇩e\u{301}z"]]);
    ed.delete_forward().unwrap();
    ed.delete_forward().unwrap();
    assert_editor(&ed, blocks![p!["a<a>z"]]);
    ed.undo().unwrap();
    assert_editor(&ed, blocks![p!["a<a>🇮🇩e\u{301}z"]]);

    let mut ed = editor(blocks![p!["a<a>bc<b>d"]]);
    ed.delete_forward().unwrap();
    assert_editor(&ed, blocks![p!["a<a>d"]]);
}

#[test]
fn delete_at_the_end_of_a_block_joins_the_next_one() {
    let mut ed = editor(blocks![
        p!["ab<a>", p!["child", p!["grandchild"]]],
        p!["cd"]
    ]);
    ed.delete_forward().unwrap();
    assert_editor(&ed, blocks![p!["ab<a>child", p!["grandchild"]], p!["cd"]]);

    let mut ed = editor(blocks![p!["ab<a>"], divider!(), p!["cd"]]);
    ed.delete_forward().unwrap();
    assert_editor(&ed, blocks![p!["ab<a>"], p!["cd"]]);
    ed.delete_forward().unwrap();
    assert_editor(&ed, blocks![p!["ab<a>cd"]]);
    ed.delete_forward().unwrap();
    ed.delete_forward().unwrap();
    assert_editor(&ed, blocks![p!["ab<a>"]]);
    ed.delete_forward().unwrap();
    assert_editor(&ed, blocks![p!["ab<a>"]]);
}

#[test]
fn delete_does_not_remove_a_textless_block_that_has_children() {
    let alert = || block("alert", vec![p!["inside"].into()]);
    let mut ed = editor(blocks![p!["ab<a>"], alert()]);
    ed.delete_forward().unwrap();
    assert_editor(&ed, blocks![p!["ab<a>"], alert()]);
    assert!(!ed.can_undo());
}

#[test]
fn typing_and_deleting_are_separate_runs() {
    let mut ed = editor(blocks![p!["<a>"]]);
    type_each(&mut ed, "abc");
    ed.delete_backward().unwrap();
    ed.delete_backward().unwrap();
    type_each(&mut ed, "x");
    assert_editor(&ed, blocks![p!["ax<a>"]]);
    ed.undo().unwrap();
    assert_editor(&ed, blocks![p!["a<a>"]]);
    ed.undo().unwrap();
    assert_editor(&ed, blocks![p!["abc<a>"]]);
    ed.undo().unwrap();
    assert_editor(&ed, blocks![p!["<a>"]]);
}
