use lotype_core::test_support::{assert_editor, editor};
use lotype_core::{Error, Pos, Selection, b, blocks, divider, i, p};

#[test]
fn typing_replaces_the_selection_and_moves_the_caret() {
    let mut ed = editor(blocks![p!["he<a>llo"], p!["wor<b>ld"]]);
    ed.insert_text("y é").unwrap();
    assert_editor(&ed, blocks![p!["hey é<a>ld"]]);
    ed.insert_text("!").unwrap();
    assert_editor(&ed, blocks![p!["hey é!<a>ld"]]);
}

#[test]
fn commands_need_a_text_selection() {
    let mut ed = editor(blocks![p!["a"]]);
    assert!(matches!(ed.insert_text("x"), Err(Error::NoSelection)));
    assert!(matches!(ed.split_block(), Err(Error::NoSelection)));
    assert!(matches!(ed.join_backward(), Err(Error::NoSelection)));
    assert!(matches!(ed.toggle_mark("bold"), Err(Error::NoSelection)));
    assert!(matches!(ed.delete_selection(), Err(Error::NoSelection)));
    assert_editor(&ed, blocks![p!["a"]]);
    assert!(!ed.can_undo());
}

#[test]
fn set_selection_rejects_positions_outside_the_document() {
    let mut ed = editor(blocks![p!["é"]]);
    let block = ed.document().blocks()[0];
    assert!(
        ed.set_selection(Some(Selection::caret(Pos::new(block, 1))))
            .is_err()
    );
    assert!(
        ed.set_selection(Some(Selection::caret(Pos::new(block, 9))))
            .is_err()
    );
    assert_eq!(ed.selection(), None);
    ed.set_selection(Some(Selection::caret(Pos::new(block, 2))))
        .unwrap();
    assert_editor(&ed, blocks![p!["é<a>"]]);
}

#[test]
fn enter_and_backspace_round_trip() {
    let mut ed = editor(blocks![p!["ab", b!["c<a>d"], "ef"]]);
    ed.split_block().unwrap();
    assert_editor(&ed, blocks![p!["ab", b!["c"]], p![b!["<a>d"], "ef"]]);
    ed.join_backward().unwrap();
    assert_editor(&ed, blocks![p!["ab", b!["c<a>d"], "ef"]]);
}

#[test]
fn enter_replaces_a_selection() {
    let mut ed = editor(blocks![p!["a<a>b"], p!["c<b>d"]]);
    ed.split_block().unwrap();
    assert_editor(&ed, blocks![p!["a"], p!["<a>d"]]);
}

#[test]
fn join_backward_only_acts_on_a_caret_at_the_start() {
    let mut ed = editor(blocks![p!["a"], p!["b<a>c"]]);
    ed.join_backward().unwrap();
    assert_editor(&ed, blocks![p!["a"], p!["b<a>c"]]);
    assert!(!ed.can_undo());

    let mut ed = editor(blocks![p!["<a>a"]]);
    ed.join_backward().unwrap();
    assert_editor(&ed, blocks![p!["<a>a"]]);
    assert!(!ed.can_undo());
}

#[test]
fn deletes_selected_text_and_blocks() {
    let mut ed = editor(blocks![p!["a<a>b"], divider!(), p!["c<b>d"]]);
    ed.delete_selection().unwrap();
    assert_editor(&ed, blocks![p!["a<a>d"]]);

    let mut ed = editor(blocks![p!["keep"], p!["parent", p!["child"]], p!["last"]]);
    let all = ed.document().blocks();
    ed.set_selection(Some(Selection::Blocks(vec![all[1], all[2], all[3]])))
        .unwrap();
    ed.delete_selection().unwrap();
    assert_editor(&ed, blocks![p!["keep"]]);
    assert_eq!(ed.selection(), None);
}

#[test]
fn toggle_mark_keeps_the_selection() {
    let mut ed = editor(blocks![p!["a<a>bc"], p!["d<b>e"]]);
    ed.toggle_mark("italic").unwrap();
    assert_editor(&ed, blocks![p!["a<a>", i!["bc"]], p![i!["d<b>"], "e"]]);
    assert!(matches!(
        ed.toggle_mark("blink"),
        Err(Error::UnknownMark(_))
    ));
}

#[test]
fn each_command_is_one_undo_step() {
    let mut ed = editor(blocks![p!["a<a>b"], p!["c<b>d"]]);
    assert!(!ed.can_undo());
    assert!(!ed.undo().unwrap());

    ed.insert_text("X").unwrap();
    ed.split_block().unwrap();
    assert_editor(&ed, blocks![p!["aX"], p!["<a>d"]]);

    assert!(ed.undo().unwrap());
    assert_editor(&ed, blocks![p!["aX<a>d"]]);
    assert!(ed.undo().unwrap());
    assert_editor(&ed, blocks![p!["a<a>b"], p!["c<b>d"]]);
    assert!(!ed.can_undo());

    assert!(ed.redo().unwrap());
    assert_editor(&ed, blocks![p!["aX<a>d"]]);
    assert!(ed.redo().unwrap());
    assert_editor(&ed, blocks![p!["aX"], p!["<a>d"]]);
    assert!(!ed.redo().unwrap());
}

#[test]
fn undo_restores_marks_children_and_the_selection() {
    let mut ed = editor(blocks![
        p!["a"],
        p!["<a>b", b!["old"], p!["child", p!["grandchild"]]],
    ]);
    ed.join_backward().unwrap();
    assert_editor(
        &ed,
        blocks![p!["a<a>b", b!["old"]], p!["child", p!["grandchild"]]],
    );
    ed.undo().unwrap();
    assert_editor(
        &ed,
        blocks![
            p!["a"],
            p!["<a>b", b!["old"], p!["child", p!["grandchild"]]],
        ],
    );
    ed.insert_text("!").unwrap();
    assert_editor(
        &ed,
        blocks![
            p!["a"],
            p!["!<a>b", b!["old"], p!["child", p!["grandchild"]]],
        ],
    );
}

#[test]
fn a_new_edit_clears_redo() {
    let mut ed = editor(blocks![p!["<a>"]]);
    ed.insert_text("a").unwrap();
    ed.undo().unwrap();
    assert!(ed.can_redo());
    ed.insert_text("b").unwrap();
    assert!(!ed.can_redo());
    assert!(!ed.redo().unwrap());
    assert_editor(&ed, blocks![p!["b<a>"]]);
}

#[test]
fn a_failed_command_leaves_a_valid_selection() {
    let mut ed = editor(blocks![p!["a<a>b<b>c"]]);
    let result = ed.apply(|doc, selection| {
        let Some(Selection::Text { anchor, .. }) = selection else {
            unreachable!()
        };
        doc.delete_text(anchor.block, 0..3)?;
        Err(Error::NoSelection)
    });
    assert!(matches!(result, Err(Error::NoSelection)));
    assert_eq!(ed.selection(), None);
    assert_editor(&ed, blocks![p![]]);
    ed.undo().unwrap();
    assert_editor(&ed, blocks![p!["a<a>b<b>c"]]);
}

#[test]
fn apply_rejects_an_invalid_resulting_selection() {
    let mut ed = editor(blocks![p!["a<a>"]]);
    let result = ed.apply(|_, selection| {
        let Some(Selection::Text { anchor, .. }) = selection else {
            unreachable!()
        };
        Ok(Some(Selection::caret(Pos::new(anchor.block, 99))))
    });
    assert!(matches!(result, Err(Error::OutOfBounds { .. })));
    assert_editor(&ed, blocks![p!["a<a>"]]);
    assert!(!ed.can_undo());
}

#[test]
fn history_is_capped() {
    let mut ed = editor(blocks![p!["<a>"]]);
    for _ in 0..105 {
        ed.insert_text("x").unwrap();
    }
    let mut undone = 0;
    while ed.undo().unwrap() {
        undone += 1;
    }
    assert_eq!(undone, 100);
    assert_editor(&ed, blocks![p!["xxxxx<a>"]]);
}
