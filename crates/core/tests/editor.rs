use lotype_core::test_support::{assert_editor, build, editor};
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
fn a_failed_edit_is_rolled_back() {
    let mut ed = editor(blocks![p!["one"], p!["a<a>b<b>c", p!["child"]]]);
    ed.insert_text("x").unwrap();
    let before = ed.document().blocks();

    let result = ed.apply(|doc, _| {
        doc.delete_text(before[0], 0..3)?;
        doc.set_attr(before[0], "align", "center")?;
        doc.remove_block(before[1])?;
        doc.insert_block(None, 0, "paragraph")?;
        Err(Error::NoSelection)
    });
    assert!(matches!(result, Err(Error::NoSelection)));
    assert_editor(&ed, blocks![p!["one"], p!["ax<a>c", p!["child"]]]);
    assert_eq!(ed.document().blocks(), before);
    assert_eq!(ed.document().attr(before[0], "align").unwrap(), None);
    assert!(!ed.can_redo());
    assert!(!ed.redo().unwrap());

    assert!(ed.undo().unwrap());
    assert_editor(&ed, blocks![p!["one"], p!["a<a>b<b>c", p!["child"]]]);
    assert!(!ed.can_undo());
}

#[test]
fn a_failed_edit_at_the_history_cap_costs_no_undo_step() {
    let mut ed = editor(blocks![p!["<a>"]]);
    for _ in 0..105 {
        ed.insert_text("x").unwrap();
        ed.insert_text(" ").unwrap();
    }
    let result = ed.apply(|doc, _| {
        doc.insert_block(None, 0, "paragraph")?;
        Err(Error::NoSelection)
    });
    assert!(result.is_err());
    let mut undone = 0;
    while ed.undo().unwrap() {
        undone += 1;
    }
    assert_eq!(undone, 100);
    assert_editor(&ed, blocks![p!["x x x x x <a>"]]);
    assert!(!ed.can_undo());
}

#[test]
fn a_failed_edit_cannot_be_redone() {
    let mut ed = editor(blocks![p!["<a>"]]);
    ed.insert_text("a").unwrap();
    ed.split_block().unwrap();
    ed.undo().unwrap();
    let result = ed.apply(|doc, _| {
        doc.insert_block(None, 0, "paragraph")?;
        Err(Error::NoSelection)
    });
    assert!(result.is_err());
    assert_editor(&ed, blocks![p!["a<a>"]]);
    assert!(!ed.can_redo());

    assert!(ed.undo().unwrap());
    assert!(ed.redo().unwrap());
    assert_editor(&ed, blocks![p!["a<a>"]]);
    assert!(!ed.can_redo());
    assert!(!ed.redo().unwrap());
    assert_editor(&ed, blocks![p!["a<a>"]]);
}

#[test]
fn a_failed_edit_that_committed_is_rolled_back() {
    let mut ed = editor(blocks![p!["a<a>"]]);
    ed.insert_text("b").unwrap();
    let result = ed.apply(|doc, _| {
        let block = doc.blocks()[0];
        doc.insert_text(block, 0, "x")?;
        doc.commit();
        doc.insert_text(block, 0, "y")?;
        Err(Error::NoSelection)
    });
    assert!(result.is_err());
    assert_editor(&ed, blocks![p!["ab<a>"]]);

    assert!(ed.undo().unwrap());
    assert_editor(&ed, blocks![p!["a<a>"]]);
    assert!(!ed.undo().unwrap());
    assert!(ed.redo().unwrap());
    assert_editor(&ed, blocks![p!["ab<a>"]]);
}

#[test]
fn a_failed_keystroke_keeps_the_typing_before_it() {
    let mut ed = editor(blocks![p!["<a>"]]);
    ed.insert_text("a").unwrap();
    ed.insert_text("b").unwrap();
    assert!(ed.apply(|_, _| Err(Error::NoSelection)).is_err());
    assert_editor(&ed, blocks![p!["ab<a>"]]);
    assert!(ed.undo().unwrap());
    assert_editor(&ed, blocks![p!["<a>"]]);
}

#[test]
fn block_ids_survive_undo_and_redo() {
    let mut ed = editor(blocks![p!["a"], p!["b<a>c", p!["child"]]]);
    let before = ed.document().blocks();
    ed.set_selection(Some(Selection::Blocks(vec![before[1]])))
        .unwrap();
    ed.delete_selection().unwrap();
    assert!(!ed.document().contains(before[1]));
    assert!(!ed.document().contains(before[2]));

    for _ in 0..2 {
        ed.undo().unwrap();
        assert_eq!(ed.document().blocks(), before);
        assert_eq!(ed.document().text(before[2]).unwrap(), "child");
        assert_eq!(ed.document().parent(before[2]).unwrap(), Some(before[1]));
        assert_eq!(ed.document().children(None).unwrap(), before[..2]);
        assert_eq!(
            ed.document().next_block(before[0]).unwrap(),
            Some(before[1])
        );
        assert_eq!(
            ed.document().prev_block(before[2]).unwrap(),
            Some(before[1])
        );
        ed.redo().unwrap();
        assert_eq!(ed.document().blocks(), before[..1]);
    }

    // A block that redo creates again is the one the step created first.
    let mut ed = editor(blocks![p!["a<a>b"]]);
    ed.split_block().unwrap();
    let split = ed.document().blocks();
    ed.undo().unwrap();
    ed.redo().unwrap();
    assert_eq!(ed.document().blocks(), split);
    ed.insert_text("x").unwrap();
    assert_editor(&ed, blocks![p!["a"], p!["x<a>b"]]);
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
        ed.insert_text(" ").unwrap();
    }
    let mut undone = 0;
    while ed.undo().unwrap() {
        undone += 1;
    }
    assert_eq!(undone, 100);
    assert_editor(&ed, blocks![p!["x x x x x <a>"]]);
}

#[test]
fn undo_restores_a_block_selection() {
    let mut ed = editor(blocks![p!["a"], p!["b", p!["b1"]], p!["c"]]);
    let all = ed.document().blocks();
    ed.set_selection(Some(Selection::Blocks(vec![all[1], all[3]])))
        .unwrap();
    ed.delete_selection().unwrap();
    assert_editor(&ed, blocks![p!["a"]]);
    ed.undo().unwrap();
    assert_editor(&ed, blocks![p!["a"], p!["b", p!["b1"]], p!["c"]]);
    assert_eq!(
        ed.selection(),
        Some(&Selection::Blocks(vec![all[1], all[3]]))
    );
}

#[test]
fn a_step_with_no_net_effect_does_not_break_undo() {
    let mut ed = editor(blocks![p!["a<a>"]]);
    ed.insert_text("b").unwrap();
    ed.apply(|doc, selection| {
        let block = doc.blocks()[0];
        doc.insert_text(block, 0, "x")?;
        doc.delete_text(block, 0..1)?;
        Ok(selection.cloned())
    })
    .unwrap();
    assert_editor(&ed, blocks![p!["ab<a>"]]);

    assert!(ed.undo().unwrap());
    assert_editor(&ed, blocks![p!["a<a>"]]);
    assert!(!ed.undo().unwrap());
    assert!(ed.redo().unwrap());
    assert_editor(&ed, blocks![p!["ab<a>"]]);
    assert!(!ed.redo().unwrap());
}

#[test]
fn replacing_the_document_restarts_undo_and_redo() {
    let mut ed = editor(blocks![p!["old<a>"]]);
    ed.insert_text(" first ").unwrap();
    ed.insert_text("second").unwrap();
    assert!(ed.undo().unwrap());
    assert!(ed.can_undo());
    assert!(ed.can_redo());

    let (replacement, selection) = build(blocks![p!["new<a>"]]);
    ed.apply(|doc, _| {
        *doc = replacement;
        Ok(selection)
    })
    .unwrap();
    assert_editor(&ed, blocks![p!["new<a>"]]);
    assert!(!ed.can_undo());
    assert!(!ed.can_redo());

    ed.insert_text("!").unwrap();
    assert!(ed.undo().unwrap());
    assert_editor(&ed, blocks![p!["new<a>"]]);
    assert!(!ed.undo().unwrap());
    assert!(ed.redo().unwrap());
    assert_editor(&ed, blocks![p!["new!<a>"]]);
}

#[test]
fn replacing_the_document_before_a_panic_rebinds_undo() {
    let mut ed = editor(blocks![p!["old<a>"]]);
    let (replacement, selection) = build(blocks![p!["new<a>"]]);
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _ = ed.apply(|doc, _| {
            *doc = replacement;
            panic!("edit failed");
        });
    }));
    assert!(result.is_err());
    assert_eq!(ed.selection(), None);
    ed.set_selection(selection).unwrap();
    ed.insert_text("!").unwrap();
    assert!(ed.undo().unwrap());
    assert_editor(&ed, blocks![p!["new<a>"]]);
}

#[test]
fn neighboring_blocks_follow_undo_and_redo() {
    let mut ed = editor(blocks![p!["A"], p!["B"], p!["C"]]);
    let [a, b, c] = ed.document().blocks()[..] else {
        panic!();
    };
    assert_eq!(ed.document().next_block(a).unwrap(), Some(b));
    ed.apply(|doc, _| {
        doc.move_block(b, None, 2)?;
        Ok(None)
    })
    .unwrap();
    assert_eq!(ed.document().next_block(a).unwrap(), Some(c));
    assert!(ed.undo().unwrap());
    assert_eq!(ed.document().next_block(a).unwrap(), Some(b));
    assert!(ed.redo().unwrap());
    assert_eq!(ed.document().next_block(a).unwrap(), Some(c));
}

#[test]
fn a_panicking_edit_does_not_leave_the_editor_stuck() {
    let mut ed = editor(blocks![p!["a<a>"]]);
    let panicked = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _ = ed.apply(|doc, _| {
            let block = doc.blocks()[0];
            doc.insert_text(block, 0, "x")?;
            panic!("edit failed");
        });
    }));
    assert!(panicked.is_err());
    assert_eq!(ed.selection(), None);
    assert!(!ed.can_undo());
    assert_editor(&ed, blocks![p!["xa"]]);

    let block = ed.document().blocks()[0];
    ed.set_selection(Some(Selection::caret(Pos::new(block, 2))))
        .unwrap();
    ed.insert_text("b").unwrap();
    assert_editor(&ed, blocks![p!["xab<a>"]]);
    assert!(ed.undo().unwrap());
    assert_editor(&ed, blocks![p!["xa<a>"]]);
    assert!(!ed.undo().unwrap());
}
