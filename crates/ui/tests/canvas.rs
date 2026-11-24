use gpui::{Modifiers, Pixels, Point, TestAppContext, point, px};
use lotype_core::test_support::block;
use lotype_core::{BlockId, Pos, Value, blocks, bullet, p};
use lotype_ui::test_support::EditorTestContext;

fn block_at(ed: &mut EditorTestContext, index: usize) -> BlockId {
    ed.editor(|editor| editor.document().blocks()[index])
}

/// Where the marker before the text of the block at `index` is.
fn marker(ed: &mut EditorTestContext, index: usize) -> Point<Pixels> {
    let block = block_at(ed, index);
    ed.view.update(ed.cx, |view, _| {
        let text = view.text_bounds(block).unwrap();
        point(text.left() - px(12.), text.top() + px(11.))
    })
}

#[gpui::test]
fn nested_blocks_are_indented_under_their_parent(cx: &mut TestAppContext) {
    let mut ed = EditorTestContext::new(cx, blocks![p!["parent", bullet!["child"]], p!["next"]]);
    let (parent, child, next) = (
        block_at(&mut ed, 0),
        block_at(&mut ed, 1),
        block_at(&mut ed, 2),
    );
    ed.view.update(ed.cx, |view, _| {
        let left = |block| view.text_bounds(block).unwrap().left();
        assert!(left(child) > left(parent));
        assert_eq!(left(next), left(parent));
    });
}

#[gpui::test]
fn a_click_on_a_box_checks_it_and_undo_unchecks_it(cx: &mut TestAppContext) {
    let task = block("check_list_item", vec!["task<a>".into()]).attr("checked", false);
    let mut ed = EditorTestContext::new(cx, blocks![task]);
    let checked = |ed: &mut EditorTestContext| {
        ed.editor(|editor| {
            let block = editor.document().blocks()[0];
            editor.document().attr(block, "checked").unwrap()
        })
    };
    let at = marker(&mut ed, 0);
    ed.cx.simulate_click(at, Modifiers::none());
    assert_eq!(checked(&mut ed), Some(Value::Bool(true)));
    // The click on the box leaves the caret where it was.
    let task = block("check_list_item", vec!["task<a>".into()]).attr("checked", true);
    ed.assert(blocks![task]);
    ed.keys("cmd-z");
    assert_eq!(checked(&mut ed), Some(Value::Bool(false)));
}

#[gpui::test]
fn a_toggle_hides_its_children_until_the_caret_goes_in(cx: &mut TestAppContext) {
    let toggle = block(
        "toggle_list_item",
        vec!["toggle<a>".into(), p!["inside"].into()],
    );
    let mut ed = EditorTestContext::new(cx, blocks![toggle, p!["after"]]);
    let inside = block_at(&mut ed, 1);
    let shown = |ed: &mut EditorTestContext| {
        ed.view
            .update(ed.cx, |view, _| view.text_bounds(inside).is_some())
    };
    assert!(shown(&mut ed));
    let at = marker(&mut ed, 0);
    ed.cx.simulate_click(at, Modifiers::none());
    assert!(!shown(&mut ed));
    // Arrows go past what is hidden.
    ed.keys("down");
    let after = block_at(&mut ed, 2);
    ed.editor(|editor| match editor.selection() {
        Some(lotype_core::Selection::Text { head, .. }) => assert_eq!(head.block, after),
        other => panic!("{other:?}"),
    });
    // A selection put inside opens the toggle again.
    ed.view.update(ed.cx, |view, cx| {
        view.update_editor(cx, |editor| {
            let inside = lotype_core::Selection::caret(Pos::new(inside, 0));
            editor.set_selection(Some(inside)).unwrap();
        });
    });
    ed.settle();
    assert!(shown(&mut ed));
}

#[gpui::test]
fn the_caret_is_scrolled_into_sight(cx: &mut TestAppContext) {
    let many: Vec<_> = (0..200).map(|_| p!["line"]).collect();
    let mut ed = EditorTestContext::new(cx, many);
    let first = block_at(&mut ed, 0);
    ed.view.update(ed.cx, |view, cx| {
        view.update_editor(cx, |editor| {
            editor
                .set_selection(Some(lotype_core::Selection::caret(Pos::new(first, 0))))
                .unwrap();
        });
    });
    ed.settle();
    let height = ed.cx.update(|window, _| window.viewport_size().height);
    ed.keys("cmd-down");
    ed.settle();
    let last = block_at(&mut ed, 199);
    let caret = ed.view.update(ed.cx, |view, _| {
        view.caret_bounds(Pos::new(last, 4)).unwrap()
    });
    assert!(caret.bottom() <= height, "{caret:?} is below {height:?}");
    assert!(caret.top() >= px(0.));
    ed.keys("cmd-up");
    let caret = ed.view.update(ed.cx, |view, _| {
        view.caret_bounds(Pos::new(first, 0)).unwrap()
    });
    assert!(
        caret.top() >= px(0.) && caret.bottom() <= height,
        "{caret:?} {height:?}"
    );
}

#[gpui::test]
fn closing_a_toggle_takes_the_caret_out_of_it(cx: &mut TestAppContext) {
    let toggle = block(
        "toggle_list_item",
        vec!["toggle".into(), p!["in<a>side"].into()],
    );
    let mut ed = EditorTestContext::new(cx, blocks![toggle]);
    let inside = block_at(&mut ed, 1);
    let at = marker(&mut ed, 0);
    ed.cx.simulate_click(at, Modifiers::none());
    let shown = ed
        .view
        .update(ed.cx, |view, _| view.text_bounds(inside).is_some());
    assert!(!shown);
    let toggle = block(
        "toggle_list_item",
        vec!["toggle<a>".into(), p!["inside"].into()],
    );
    ed.assert(blocks![toggle]);
}

#[gpui::test]
fn a_selection_is_scrolled_after_its_head(cx: &mut TestAppContext) {
    let many: Vec<_> = (0..200).map(|_| p!["line"]).collect();
    let mut ed = EditorTestContext::new(cx, many);
    let (first, last) = (block_at(&mut ed, 0), block_at(&mut ed, 199));
    ed.view.update(ed.cx, |view, cx| {
        view.update_editor(cx, |editor| {
            let caret = lotype_core::Selection::caret(Pos::new(first, 0));
            editor.set_selection(Some(caret)).unwrap();
        });
    });
    ed.keys("cmd-shift-down");
    ed.settle();
    let height = ed.cx.update(|window, _| window.viewport_size().height);
    let head = ed.view.update(ed.cx, |view, _| {
        view.caret_bounds(Pos::new(last, 4)).unwrap()
    });
    assert!(head.bottom() <= height, "{head:?} is below {height:?}");
}

#[gpui::test]
fn an_empty_paragraph_with_the_caret_shows_the_placeholder(cx: &mut TestAppContext) {
    let shows = |ed: &mut EditorTestContext| {
        ed.settle();
        ed.cx.debug_bounds("placeholder").is_some()
    };
    let mut ed = EditorTestContext::new(cx, blocks![p!["text"], p!["<a>"]]);
    assert!(shows(&mut ed));
    ed.type_text("a");
    assert!(!shows(&mut ed));
    ed.keys("backspace");
    assert!(shows(&mut ed));
    // Not where the caret is not, nor in an empty block of another type.
    ed.keys("up");
    assert!(!shows(&mut ed));
    let heading = block("heading", vec!["<a>".into()]).attr("level", 1);
    let mut ed = EditorTestContext::new(cx, blocks![heading]);
    assert!(!shows(&mut ed));
}

#[gpui::test]
fn an_empty_document_is_where_writing_starts(cx: &mut TestAppContext) {
    let shows = |ed: &mut EditorTestContext, selector: &'static str| {
        ed.settle();
        ed.cx.debug_bounds(selector)
    };
    let mut ed = EditorTestContext::new(cx, blocks![]);
    assert!(shows(&mut ed, "empty-document").is_some());
    // Typing makes the first paragraph, and undo takes it all back.
    ed.type_text("hi");
    ed.assert(blocks![p!["hi<a>"]]);
    assert!(shows(&mut ed, "empty-document").is_none());
    ed.keys("cmd-z cmd-z");
    assert!(ed.editor(|editor| editor.document().blocks().is_empty()));

    // A click there makes it too, with the caret and its placeholder in it.
    let mut ed = EditorTestContext::new(cx, blocks![]);
    let at = shows(&mut ed, "empty-document").unwrap().center();
    ed.cx.simulate_click(at, Modifiers::none());
    ed.settle();
    ed.assert(blocks![p!["<a>"]]);
    assert!(shows(&mut ed, "placeholder").is_some());
}

#[gpui::test]
fn the_caret_blinks_once_it_rests_and_only_at_a_caret(cx: &mut TestAppContext) {
    use std::time::Duration;
    let lit = |ed: &mut EditorTestContext| ed.view.update(ed.cx, |view, _| view.caret_lit());
    let mut ed = EditorTestContext::new(cx, blocks![p!["one<a>"]]);
    ed.view.update(ed.cx, |view, _| view.take_frames_as_cheap());
    ed.cx.update(|window, _| window.activate_window());
    ed.type_text("x");
    assert!(lit(&mut ed));
    ed.cx.executor().advance_clock(Duration::from_millis(510));
    assert!(!lit(&mut ed), "dark once the caret rests");
    ed.cx.executor().advance_clock(Duration::from_millis(540));
    assert!(lit(&mut ed), "lit again");
    // Typing lights it, and it waits before it blinks again.
    ed.cx.executor().advance_clock(Duration::from_millis(540));
    ed.type_text("y");
    assert!(lit(&mut ed));
    ed.cx.executor().advance_clock(Duration::from_millis(300));
    assert!(lit(&mut ed));
    // A range does not blink.
    ed.keys("shift-left");
    ed.cx.executor().advance_clock(Duration::from_millis(2000));
    assert!(lit(&mut ed));
}

#[gpui::test]
fn the_caret_stays_lit_where_frames_are_dear(cx: &mut TestAppContext) {
    // Hundreds of blocks laid out each frame cost far more than the budget.
    let mut many: Vec<_> = (0..500).map(|_| p!["a line of text"]).collect();
    many[0] = p!["one<a>"];
    let mut ed = EditorTestContext::new(cx, many);
    ed.cx.update(|window, _| window.activate_window());
    ed.type_text("x");
    ed.cx
        .executor()
        .advance_clock(std::time::Duration::from_millis(510));
    assert!(ed.view.update(ed.cx, |view, _| view.caret_lit()));
}
