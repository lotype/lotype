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
fn a_click_on_the_handle_opens_the_block_menu_only(cx: &mut TestAppContext) {
    let mut ed = EditorTestContext::new(cx, blocks![p!["one<a>"], p!["two"]]);
    hover(&mut ed, 1);
    let handle = find(&mut ed, "side-handle").unwrap();
    ed.cx.simulate_click(handle, Modifiers::none());
    ed.settle();
    assert!(find(&mut ed, "block-menu-delete").is_some());
    // The selection stays where it was.
    ed.assert(blocks![p!["one<a>"], p!["two"]]);
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

/// Opens the block menu of the block at `index` with a click on its handle.
fn open_menu(ed: &mut EditorTestContext, index: usize) {
    hover(ed, index);
    let handle = find(ed, "side-handle").unwrap();
    ed.cx.simulate_click(handle, Modifiers::none());
    ed.settle();
}

fn tap(ed: &mut EditorTestContext, selector: &'static str) {
    let at = find(ed, selector).unwrap_or_else(|| panic!("{selector} shows"));
    ed.cx.simulate_click(at, Modifiers::none());
    ed.settle();
}

#[gpui::test]
fn the_block_menu_deletes_the_block(cx: &mut TestAppContext) {
    let mut ed = EditorTestContext::new(cx, blocks![p!["one"], p!["two"], p!["three<a>"]]);
    open_menu(&mut ed, 1);
    tap(&mut ed, "block-menu-delete");
    ed.assert(blocks![p!["one<a>"], p!["three"]]);
    assert!(find(&mut ed, "block-menu-delete").is_none());
}

#[gpui::test]
fn the_block_menu_colors_the_block(cx: &mut TestAppContext) {
    let mut ed = EditorTestContext::new(cx, blocks![p!["one<a>"], p!["two"]]);
    open_menu(&mut ed, 1);
    tap(&mut ed, "text_color-red");
    open_menu(&mut ed, 1);
    tap(&mut ed, "background_color-blue");
    assert_eq!(
        shown(&mut ed),
        "paragraph \"one<a>\"\nparagraph background_color=blue text_color=red \"two\"\n"
    );
    // Default takes a color off, and Escape closes the menu.
    open_menu(&mut ed, 1);
    tap(&mut ed, "text_color-default");
    open_menu(&mut ed, 1);
    ed.keys("escape");
    assert!(find(&mut ed, "block-menu-delete").is_none());
    assert_eq!(
        shown(&mut ed),
        "paragraph \"one<a>\"\nparagraph background_color=blue \"two\"\n"
    );
}

#[gpui::test]
fn a_key_closes_the_block_menu_and_a_deleted_block_takes_it(cx: &mut TestAppContext) {
    let mut ed = EditorTestContext::new(cx, blocks![p!["one"], p!["two"], p!["three<a>"]]);
    open_menu(&mut ed, 1);
    // The key does what it does, and the menu goes.
    ed.keys("left");
    assert!(find(&mut ed, "block-menu-delete").is_none());
    assert!(shown(&mut ed).ends_with("paragraph \"thre<a>e\"\n"));

    open_menu(&mut ed, 1);
    // A peer deletes the block: its menu goes, and other rows get their side menu.
    ed.view.update(ed.cx, |view, cx| {
        view.update_editor(cx, |editor| {
            let block = editor.document().blocks()[1];
            let mut peer = lotype_core::Document::new(lotype_core::Schema::builtin());
            peer.import(&editor.document().export_snapshot().unwrap())
                .unwrap();
            peer.remove_block(block).unwrap();
            let update = peer.export_updates(&editor.document().version()).unwrap();
            editor.import(&update).unwrap();
        });
    });
    ed.settle();
    assert!(find(&mut ed, "block-menu-delete").is_none());
    hover(&mut ed, 0);
    assert!(find(&mut ed, "side-handle").is_some());
}

#[gpui::test]
fn a_code_block_has_no_colors_in_its_menu(cx: &mut TestAppContext) {
    let code = lotype_core::test_support::block("code_block", vec!["fn main() {}".into()]);
    let mut ed = EditorTestContext::new(cx, blocks![p!["one<a>"], code]);
    open_menu(&mut ed, 1);
    assert!(find(&mut ed, "block-menu-delete").is_some());
    assert!(find(&mut ed, "text_color-red").is_none());
    assert!(find(&mut ed, "background_color-default").is_none());
}

#[gpui::test]
fn the_side_menu_is_centred_on_the_first_line_of_any_block(cx: &mut TestAppContext) {
    let code = lotype_core::test_support::block("code_block", vec!["fn a() {}".into()]);
    let quote = lotype_core::test_support::block("quote", vec!["said".into()]);
    let mut ed = EditorTestContext::new(cx, blocks![p!["text"], h1!["Title"], code, quote]);
    for index in 0..4 {
        hover(&mut ed, index);
        let handle = find(&mut ed, "side-handle").unwrap();
        let block = block_at(&mut ed, index);
        let line = ed.view.update(ed.cx, |view, _| {
            view.caret_bounds(lotype_core::Pos::new(block, 0)).unwrap()
        });
        let off = (handle.y - line.center().y).abs();
        assert!(off < px(1.5), "block {index}: {handle:?} against {line:?}");
    }
}

#[gpui::test]
fn the_side_menu_stays_a_moment_after_the_mouse_leaves(cx: &mut TestAppContext) {
    let mut ed = EditorTestContext::new(cx, blocks![p!["one"], p!["two"]]);
    hover(&mut ed, 0);
    let away = over(&mut ed, 1, true) + point(px(0.), px(300.));
    ed.cx.simulate_mouse_move(away, None, Modifiers::none());
    ed.settle();
    assert!(find(&mut ed, "side-handle").is_some());
    // Back over a row in time, it stays.
    hover(&mut ed, 1);
    ed.cx
        .executor()
        .advance_clock(std::time::Duration::from_millis(500));
    assert!(find(&mut ed, "side-handle").is_some());
    // Away for longer, it goes.
    ed.cx.simulate_mouse_move(away, None, Modifiers::none());
    ed.cx
        .executor()
        .advance_clock(std::time::Duration::from_millis(500));
    assert!(find(&mut ed, "side-handle").is_none());
}

#[gpui::test]
fn a_heading_dragged_among_nested_blocks_lands_after_what_they_are_in(cx: &mut TestAppContext) {
    let mut ed = EditorTestContext::new(
        cx,
        blocks![h1!["title<a>"], p!["one", p!["child"]], p!["two"]],
    );
    hover(&mut ed, 0);
    let handle = find(&mut ed, "side-handle").unwrap();
    ed.cx
        .simulate_mouse_down(handle, MouseButton::Left, Modifiers::none());
    let over_child = over(&mut ed, 2, false);
    ed.cx
        .simulate_mouse_move(over_child, Some(MouseButton::Left), Modifiers::none());
    // The line is under the last block in it, where the heading lands.
    let line = find(&mut ed, "drop-line").expect("a place for it");
    assert!(line.y > over_child.y, "{line:?} {over_child:?}");
    ed.cx
        .simulate_mouse_up(over_child, MouseButton::Left, Modifiers::none());
    ed.settle();
    ed.assert(blocks![p!["one", p!["child"]], h1!["title<a>"], p!["two"]]);
}

#[gpui::test]
fn a_second_click_on_the_handle_closes_the_block_menu(cx: &mut TestAppContext) {
    let mut ed = EditorTestContext::new(cx, blocks![p!["one<a>"], p!["two"]]);
    hover(&mut ed, 1);
    let handle = find(&mut ed, "side-handle").unwrap();
    ed.cx.simulate_click(handle, Modifiers::none());
    assert!(find(&mut ed, "block-menu-delete").is_some());
    ed.cx.simulate_click(handle, Modifiers::none());
    assert!(find(&mut ed, "block-menu-delete").is_none());
    ed.cx.simulate_click(handle, Modifiers::none());
    assert!(find(&mut ed, "block-menu-delete").is_some());
}
