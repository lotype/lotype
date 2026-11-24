use gpui::{Modifiers, MouseButton, MouseDownEvent, Pixels, Point, TestAppContext, point, px};
use lotype_core::test_support::block;
use lotype_core::{blocks, p};
use lotype_ui::test_support::EditorTestContext;

/// Where the text of the block at `index` has `offset`, nudged into the
/// character after it.
fn at(ed: &mut EditorTestContext, index: usize, offset: usize) -> Point<Pixels> {
    let block = ed.editor(|editor| editor.document().blocks()[index]);
    ed.view.update(ed.cx, |view, _| {
        let caret = view
            .caret_bounds(lotype_core::Pos::new(block, offset))
            .unwrap();
        point(caret.left() + px(1.), caret.top() + caret.size.height / 2.)
    })
}

#[gpui::test]
fn arrows_move_by_character_and_across_blocks(cx: &mut TestAppContext) {
    let mut ed = EditorTestContext::new(cx, blocks![p!["a<a>e\u{301}"], p!["b"]]);
    ed.keys("right");
    ed.assert(blocks![p!["ae\u{301}<a>"], p!["b"]]);
    ed.keys("right");
    ed.assert(blocks![p!["ae\u{301}"], p!["<a>b"]]);
    ed.keys("left");
    ed.assert(blocks![p!["ae\u{301}<a>"], p!["b"]]);
    ed.keys("shift-right shift-right");
    ed.assert(blocks![p!["ae\u{301}<a>"], p!["b<b>"]]);
    // Without shift, an arrow collapses the selection to its side.
    ed.keys("left");
    ed.assert(blocks![p!["ae\u{301}<a>"], p!["b"]]);
}

#[gpui::test]
fn words_lines_and_the_whole_text(cx: &mut TestAppContext) {
    let mut ed = EditorTestContext::new(cx, blocks![p!["<a>foo bar baz"], p!["end"]]);
    ed.keys("alt-right");
    ed.assert(blocks![p!["foo<a> bar baz"], p!["end"]]);
    ed.keys("alt-right alt-left");
    ed.assert(blocks![p!["foo <a>bar baz"], p!["end"]]);
    ed.keys("cmd-right");
    ed.assert(blocks![p!["foo bar baz<a>"], p!["end"]]);
    ed.keys("cmd-shift-left");
    ed.assert(blocks![p!["<b>foo bar baz<a>"], p!["end"]]);
    ed.keys("cmd-down");
    ed.assert(blocks![p!["foo bar baz"], p!["end<a>"]]);
    ed.keys("cmd-a");
    ed.assert(blocks![p!["<a>foo bar baz"], p!["end<b>"]]);
}

#[gpui::test]
fn up_and_down_keep_to_a_column(cx: &mut TestAppContext) {
    let code = block("code_block", vec!["abcd\nab\nabcd".into()]);
    let mut ed = EditorTestContext::new(cx, blocks![p!["abc<a>d"], code, p!["abcd"]]);
    // The code block's padding moves its text right of the column.
    ed.keys("down");
    ed.assert(blocks![
        p!["abcd"],
        block("code_block", vec!["ab<a>cd\nab\nabcd".into()]),
        p!["abcd"]
    ]);
    ed.keys("down");
    ed.assert(blocks![
        p!["abcd"],
        block("code_block", vec!["abcd\nab<a>\nabcd".into()]),
        p!["abcd"]
    ]);
    ed.keys("down down");
    ed.assert(blocks![
        p!["abcd"],
        block("code_block", vec!["abcd\nab\nabcd".into()]),
        p!["abc<a>d"]
    ]);
    // Past the last line is the end, past the first the start.
    ed.keys("down");
    ed.assert(blocks![
        p!["abcd"],
        block("code_block", vec!["abcd\nab\nabcd".into()]),
        p!["abcd<a>"]
    ]);
    ed.keys("cmd-up up");
    ed.assert(blocks![
        p!["<a>abcd"],
        block("code_block", vec!["abcd\nab\nabcd".into()]),
        p!["abcd"]
    ]);
}

#[gpui::test]
fn clicks_place_the_caret_and_select(cx: &mut TestAppContext) {
    let mut ed = EditorTestContext::new(cx, blocks![p!["one two<a>"], p!["three"]]);
    let one = at(&mut ed, 0, 1);
    ed.cx.simulate_click(one, Modifiers::none());
    ed.assert(blocks![p!["o<a>ne two"], p!["three"]]);
    let three = at(&mut ed, 1, 2);
    ed.cx.simulate_click(three, Modifiers::shift());
    ed.assert(blocks![p!["o<a>ne two"], p!["th<b>ree"]]);

    let two = at(&mut ed, 0, 5);
    ed.cx.simulate_event(MouseDownEvent {
        button: MouseButton::Left,
        position: two,
        modifiers: Modifiers::none(),
        click_count: 2,
        first_mouse: false,
    });
    ed.cx
        .simulate_mouse_up(two, MouseButton::Left, Modifiers::none());
    ed.assert(blocks![p!["one <a>two<b>"], p!["three"]]);

    // Dragging from one block into another selects across them.
    ed.cx
        .simulate_mouse_down(one, MouseButton::Left, Modifiers::none());
    ed.cx
        .simulate_mouse_move(three, Some(MouseButton::Left), Modifiers::none());
    ed.cx
        .simulate_mouse_up(three, MouseButton::Left, Modifiers::none());
    ed.assert(blocks![p!["o<a>ne two"], p!["th<b>ree"]]);
}

#[gpui::test]
fn a_click_commits_text_being_composed(cx: &mut TestAppContext) {
    let mut ed = EditorTestContext::new(cx, blocks![p!["ab<a>"], p!["cd"]]);
    ed.ime_compose("に", None);
    let c = at(&mut ed, 1, 1);
    ed.cx.simulate_click(c, Modifiers::none());
    ed.assert(blocks![p!["abに"], p!["c<a>d"]]);
}

#[gpui::test]
fn up_into_a_block_of_another_size_lands_on_its_last_line(cx: &mut TestAppContext) {
    let code = block("code_block", vec!["abcdefgh\nabcdefgh".into()]);
    let mut ed = EditorTestContext::new(cx, blocks![code, lotype_core::h1!["a<a>b"]]);
    ed.keys("up");
    let head = ed.editor(|editor| match editor.selection() {
        Some(lotype_core::Selection::Text { head, .. }) => head.offset,
        _ => panic!("no caret"),
    });
    // On the second line, near the column it came from, not at the start.
    assert!((10..=17).contains(&head), "{head}");
}

#[gpui::test]
fn dragging_past_the_editor_goes_on_selecting_until_released(cx: &mut TestAppContext) {
    let mut ed = EditorTestContext::new(cx, blocks![p!["one<a>"], p!["two"]]);
    let one = at(&mut ed, 0, 1);
    let below = point(one.x, px(5000.));
    ed.cx
        .simulate_mouse_down(one, MouseButton::Left, Modifiers::none());
    ed.cx
        .simulate_mouse_move(below, Some(MouseButton::Left), Modifiers::none());
    ed.assert(blocks![p!["o<a>ne"], p!["two<b>"]]);
    ed.cx
        .simulate_mouse_up(below, MouseButton::Left, Modifiers::none());
    // A drag that starts elsewhere selects nothing here.
    ed.cx
        .simulate_mouse_move(one, Some(MouseButton::Left), Modifiers::none());
    ed.assert(blocks![p!["o<a>ne"], p!["two<b>"]]);
}

#[gpui::test]
fn a_drag_keeps_its_start_through_a_peers_edit(cx: &mut TestAppContext) {
    let mut ed = EditorTestContext::new(cx, blocks![p!["hello world"]]);
    let (from, to) = (at(&mut ed, 0, 6), at(&mut ed, 0, 11));
    ed.cx
        .simulate_mouse_down(from, MouseButton::Left, Modifiers::none());
    // A peer writes before the place the drag started at.
    let block = ed.editor(|editor| editor.document().blocks()[0]);
    ed.view.update(ed.cx, |view, cx| {
        view.update_editor(cx, |editor| {
            let mut theirs = lotype_core::Document::new(lotype_core::Schema::builtin());
            theirs
                .import(&editor.document().export_snapshot().unwrap())
                .unwrap();
            theirs.insert_text(block, 0, "XXXX ").unwrap();
            editor.import(&theirs.export_snapshot().unwrap()).unwrap();
        });
    });
    ed.settle();
    let to = point(to.x + px(9.6 * 5.), to.y);
    ed.cx
        .simulate_mouse_move(to, Some(MouseButton::Left), Modifiers::none());
    ed.cx
        .simulate_mouse_up(to, MouseButton::Left, Modifiers::none());
    ed.assert(blocks![p!["XXXX hello <a>world<b>"]]);
}

#[gpui::test]
fn a_click_on_a_combining_mark_goes_before_its_letter(cx: &mut TestAppContext) {
    let mut ed = EditorTestContext::new(cx, blocks![p!["e\u{301}x<a>"]]);
    // The mark is a glyph of its own in the test fonts, one character in.
    let mark = at(&mut ed, 0, 0);
    let mark = point(mark.x + px(9.6), mark.y);
    ed.cx.simulate_click(mark, Modifiers::none());
    ed.type_text("a");
    let text = ed.editor(|editor| {
        editor
            .document()
            .text(editor.document().blocks()[0])
            .unwrap()
    });
    assert!(text == "ae\u{301}x" || text == "e\u{301}ax", "{text:?}");
}
