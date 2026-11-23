use gpui::{Modifiers, MouseButton, Pixels, Point, TestAppContext, point, px};
use lotype_core::test_support::outline;
use lotype_core::{BlockId, blocks, h1, p};
use lotype_ui::test_support::EditorTestContext;

fn block_at(ed: &mut EditorTestContext, index: usize) -> BlockId {
    ed.editor(|editor| editor.document().blocks()[index])
}

/// Over the text of the block at `index`, in its upper or lower half.
fn over(ed: &mut EditorTestContext, index: usize, lower: bool) -> Point<Pixels> {
    let block = block_at(ed, index);
    ed.view.update(ed.cx, |view, _| {
        let text = view.text_bounds(block).unwrap();
        let y = if lower {
            text.bottom() - px(2.)
        } else {
            text.top() + px(2.)
        };
        point(text.left() + px(4.), y)
    })
}

fn hover(ed: &mut EditorTestContext, index: usize) {
    let at = over(ed, index, false);
    ed.cx.simulate_mouse_move(at, None, Modifiers::none());
    ed.settle();
}

fn find(ed: &mut EditorTestContext, selector: &'static str) -> Option<Point<Pixels>> {
    ed.settle();
    ed.cx.debug_bounds(selector).map(|bounds| bounds.center())
}

fn shown(ed: &mut EditorTestContext) -> String {
    ed.editor(|editor| outline(editor.document(), editor.selection()))
}

#[gpui::test]
fn the_side_menu_shows_beside_the_block_under_the_mouse(cx: &mut TestAppContext) {
    let mut ed = EditorTestContext::new(cx, blocks![p!["one"], p!["two"]]);
    assert!(find(&mut ed, "side-handle").is_none());
    hover(&mut ed, 1);
    let handle = find(&mut ed, "side-handle").expect("the side menu shows");
    let text = ed.view.update(ed.cx, |view, _| {
        let blocks = view.editor().document().blocks();
        view.text_bounds(blocks[1]).unwrap()
    });
    assert!(handle.x < text.left());
    assert!(text.top() <= handle.y && handle.y <= text.bottom());
}

#[gpui::test]
fn the_add_button_opens_the_slash_menu_in_a_new_block(cx: &mut TestAppContext) {
    let mut ed = EditorTestContext::new(cx, blocks![p!["one"], p!["two"]]);
    hover(&mut ed, 0);
    let add = find(&mut ed, "side-add").unwrap();
    ed.cx.simulate_click(add, Modifiers::none());
    ed.settle();
    ed.assert(blocks![p!["one"], p!["/<a>"], p!["two"]]);
    // The slash menu has the keys: Enter takes its first entry.
    ed.keys("enter");
    ed.assert(blocks![p!["one"], h1!["<a>"], p!["two"]]);
}

#[gpui::test]
fn a_click_on_the_handle_selects_the_block(cx: &mut TestAppContext) {
    let mut ed = EditorTestContext::new(cx, blocks![p!["one<a>"], p!["two"]]);
    hover(&mut ed, 1);
    let handle = find(&mut ed, "side-handle").unwrap();
    ed.cx.simulate_click(handle, Modifiers::none());
    ed.settle();
    assert_eq!(shown(&mut ed), "paragraph \"one\"\n* paragraph \"two\"\n");
}

#[gpui::test]
fn dragging_the_handle_moves_the_block(cx: &mut TestAppContext) {
    let mut ed = EditorTestContext::new(cx, blocks![p!["one<a>"], p!["two"], p!["three"]]);
    hover(&mut ed, 0);
    let handle = find(&mut ed, "side-handle").unwrap();
    ed.cx
        .simulate_mouse_down(handle, MouseButton::Left, Modifiers::none());
    let below_three = over(&mut ed, 2, true);
    ed.cx
        .simulate_mouse_move(below_three, Some(MouseButton::Left), Modifiers::none());
    assert!(find(&mut ed, "drop-line").is_some());
    ed.cx
        .simulate_mouse_up(below_three, MouseButton::Left, Modifiers::none());
    ed.settle();
    ed.assert(blocks![p!["two"], p!["three"], p!["one<a>"]]);
    assert!(find(&mut ed, "drop-line").is_none());
    // One undo step puts it back.
    ed.keys("cmd-z");
    ed.assert(blocks![p!["one"], p!["two"], p!["three"]]);
}

#[gpui::test]
fn a_block_cannot_be_dropped_inside_itself(cx: &mut TestAppContext) {
    let mut ed = EditorTestContext::new(cx, blocks![p!["one", p!["child"]], p!["two"]]);
    hover(&mut ed, 0);
    let handle = find(&mut ed, "side-handle").unwrap();
    ed.cx
        .simulate_mouse_down(handle, MouseButton::Left, Modifiers::none());
    let below_child = over(&mut ed, 1, true);
    ed.cx
        .simulate_mouse_move(below_child, Some(MouseButton::Left), Modifiers::none());
    ed.cx
        .simulate_mouse_up(below_child, MouseButton::Left, Modifiers::none());
    ed.settle();
    assert_eq!(
        shown(&mut ed),
        "paragraph \"one\"\n  paragraph \"child\"\nparagraph \"two\"\n"
    );
}

#[gpui::test]
fn a_drag_that_ends_off_the_rows_changes_nothing(cx: &mut TestAppContext) {
    let mut ed = EditorTestContext::new(cx, blocks![p!["one<a>"], p!["two"]]);
    hover(&mut ed, 0);
    let handle = find(&mut ed, "side-handle").unwrap();
    ed.cx
        .simulate_mouse_down(handle, MouseButton::Left, Modifiers::none());
    let below = over(&mut ed, 1, true) + point(px(0.), px(200.));
    ed.cx
        .simulate_mouse_move(below, Some(MouseButton::Left), Modifiers::none());
    ed.cx
        .simulate_mouse_up(below, MouseButton::Left, Modifiers::none());
    ed.settle();
    ed.assert(blocks![p!["one<a>"], p!["two"]]);
}

#[gpui::test]
fn below_a_row_with_children_is_before_its_first_child(cx: &mut TestAppContext) {
    let mut ed = EditorTestContext::new(cx, blocks![p!["a", p!["child"]], p!["b<a>"]]);
    hover(&mut ed, 2);
    let handle = find(&mut ed, "side-handle").unwrap();
    ed.cx
        .simulate_mouse_down(handle, MouseButton::Left, Modifiers::none());
    let below_a = over(&mut ed, 0, true);
    ed.cx
        .simulate_mouse_move(below_a, Some(MouseButton::Left), Modifiers::none());
    ed.cx
        .simulate_mouse_up(below_a, MouseButton::Left, Modifiers::none());
    ed.settle();
    ed.assert(blocks![p!["a", p!["b<a>"], p!["child"]]]);
}
