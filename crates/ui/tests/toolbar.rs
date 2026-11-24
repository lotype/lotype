use gpui::{Modifiers, MouseButton, TestAppContext};
use lotype_core::{b, blocks, p};
use lotype_ui::test_support::EditorTestContext;

fn button(ed: &mut EditorTestContext, name: &'static str) -> Option<gpui::Point<gpui::Pixels>> {
    ed.settle();
    ed.cx.debug_bounds(name).map(|bounds| bounds.center())
}

#[gpui::test]
fn the_toolbar_toggles_marks_on_the_selection(cx: &mut TestAppContext) {
    let mut ed = EditorTestContext::new(cx, blocks![p!["one <a>two"]]);
    assert!(button(&mut ed, "toolbar-bold").is_none());
    ed.keys("shift-right shift-right shift-right");
    let bold = button(&mut ed, "toolbar-bold").expect("the toolbar shows");
    ed.cx.simulate_click(bold, Modifiers::none());
    // The selection stays, and so does the toolbar.
    ed.assert(blocks![p!["one ", b!["<a>two<b>"]]]);
    let bold = button(&mut ed, "toolbar-bold").expect("the toolbar stays");
    ed.cx.simulate_click(bold, Modifiers::none());
    ed.assert(blocks![p!["one <a>two<b>"]]);
}

#[gpui::test]
fn the_toolbar_waits_for_a_drag_to_end(cx: &mut TestAppContext) {
    let mut ed = EditorTestContext::new(cx, blocks![p!["<a>one two"]]);
    let block = ed.editor(|editor| editor.document().blocks()[0]);
    let (start, end) = ed.view.update(ed.cx, |view, _| {
        let at = |offset| {
            let caret = view
                .caret_bounds(lotype_core::Pos::new(block, offset))
                .unwrap();
            caret.center()
        };
        (at(0), at(3))
    });
    ed.cx
        .simulate_mouse_down(start, MouseButton::Left, Modifiers::none());
    ed.cx
        .simulate_mouse_move(end, Some(MouseButton::Left), Modifiers::none());
    assert!(button(&mut ed, "toolbar-bold").is_none());
    ed.cx
        .simulate_mouse_up(end, MouseButton::Left, Modifiers::none());
    assert!(button(&mut ed, "toolbar-bold").is_some());
}

#[gpui::test]
fn a_selection_in_a_table_cell_asks_for_no_frames(cx: &mut TestAppContext) {
    let mut doc = lotype_core::Document::new(lotype_core::Schema::builtin());
    let table = doc.insert_table(None, 0, 1, 1).unwrap();
    let row = doc.children(Some(table)).unwrap()[0];
    let cell = doc.cell(row, doc.columns(table).unwrap()[0]).unwrap();
    doc.insert_text(cell, 0, "abc").unwrap();
    let mut editor = lotype_core::Editor::new(doc);
    let (anchor, head) = (
        lotype_core::Pos::new(cell, 0),
        lotype_core::Pos::new(cell, 2),
    );
    editor
        .set_selection(Some(lotype_core::Selection::Text { anchor, head }))
        .unwrap();
    let mut ed = EditorTestContext::with_view(cx, |cx| lotype_ui::EditorView::new(editor, cx));
    // Settling would panic on a view that asks for every frame.
    assert!(button(&mut ed, "toolbar-bold").is_none());
}

fn press(ed: &mut EditorTestContext, selector: &'static str) {
    let at = button(ed, selector).unwrap_or_else(|| panic!("{selector} shows"));
    ed.cx.simulate_click(at, Modifiers::none());
    ed.settle();
}

#[gpui::test]
fn the_block_type_select_turns_the_selected_blocks(cx: &mut TestAppContext) {
    let mut ed = EditorTestContext::new(cx, blocks![p!["one <a>two<b>"], p!["three"]]);
    press(&mut ed, "toolbar-block-type");
    // Heading 2 is the third entry, after paragraph and heading 1.
    press(&mut ed, "block-type-2");
    let heading = lotype_core::test_support::block("heading", vec!["one <a>two<b>".into()]);
    ed.assert(blocks![heading.attr("level", 2), p!["three"]]);
    assert!(button(&mut ed, "block-type-2").is_none());
}

#[gpui::test]
fn alignment_moves_the_lines_and_stays_with_the_block(cx: &mut TestAppContext) {
    let mut ed = EditorTestContext::new(cx, blocks![p!["<a>one<b>"]]);
    let block = ed.editor(|editor| editor.document().blocks()[0]);
    let start = |ed: &mut EditorTestContext| {
        ed.view.update(ed.cx, |view, _| {
            view.caret_bounds(lotype_core::Pos::new(block, 0))
                .unwrap()
                .left()
        })
    };
    let left = start(&mut ed);
    press(&mut ed, "toolbar-center");
    let center = start(&mut ed);
    press(&mut ed, "toolbar-right");
    let right = start(&mut ed);
    assert!(
        left < center && center < right,
        "{left:?} {center:?} {right:?}"
    );
    let value = ed.editor(|editor| editor.document().attr(block, "text_alignment").unwrap());
    assert_eq!(value, Some(lotype_core::Value::from("right")));
    press(&mut ed, "toolbar-left");
    assert_eq!(start(&mut ed), left);
}

#[gpui::test]
fn the_color_dropdown_marks_the_selection(cx: &mut TestAppContext) {
    let mut ed = EditorTestContext::new(cx, blocks![p!["one <a>two<b>"]]);
    press(&mut ed, "toolbar-color");
    press(&mut ed, "mark-text_color-red");
    let red = lotype_core::test_support::marked("text_color", "red", vec!["<a>two<b>".into()]);
    ed.assert(blocks![p!["one ", red]]);
    // A key closes a dropdown.
    press(&mut ed, "toolbar-color");
    assert!(button(&mut ed, "mark-text_color-red").is_some());
    ed.keys("shift-left");
    assert!(button(&mut ed, "mark-text_color-red").is_none());
}

#[gpui::test]
fn nest_and_unnest_move_the_selected_block(cx: &mut TestAppContext) {
    let mut ed = EditorTestContext::new(cx, blocks![p!["one"], p!["<a>two<b>"]]);
    press(&mut ed, "toolbar-nest");
    ed.assert(blocks![p!["one", p!["<a>two<b>"]]]);
    press(&mut ed, "toolbar-unnest");
    ed.assert(blocks![p!["one"], p!["<a>two<b>"]]);
}

#[gpui::test]
fn a_dropdown_near_the_bottom_takes_its_clicks_alone(cx: &mut TestAppContext) {
    let mut many: Vec<_> = (0..59).map(|_| p!["line"]).collect();
    many.push(p!["one <a>two<b>"]);
    let mut ed = EditorTestContext::new(cx, many);
    ed.keys("shift-right shift-left");
    press(&mut ed, "toolbar-block-type");
    // Check List, the last entry, is where the dropdown would cover the bar.
    press(&mut ed, "block-type-10");
    let last = ed.editor(|editor| {
        let doc = editor.document();
        let block = *doc.blocks().last().unwrap();
        (doc.block_type(block).unwrap(), doc.spans(block).unwrap())
    });
    assert_eq!(last.0, "check_list_item");
    assert!(
        last.1.iter().all(|span| span.marks.is_empty()),
        "{:?}",
        last.1
    );
}

#[gpui::test]
fn near_the_bottom_a_dropdown_opens_over_the_bar(cx: &mut TestAppContext) {
    let mut many: Vec<_> = (0..59).map(|_| p!["line"]).collect();
    many.push(p!["one <a>two<b>"]);
    let mut ed = EditorTestContext::new(cx, many);
    ed.keys("shift-right shift-left");
    press(&mut ed, "toolbar-color");
    let bar = ed.cx.debug_bounds("toolbar-bold").unwrap();
    let swatch = ed.cx.debug_bounds("mark-background_color-yellow").unwrap();
    // The whole dropdown is over the bar, which it leaves in sight.
    assert!(swatch.bottom() < bar.top(), "{swatch:?} over {bar:?}");
    press(&mut ed, "mark-background_color-yellow");
    let marked = ed.editor(|editor| {
        let doc = editor.document();
        let block = *doc.blocks().last().unwrap();
        doc.spans(block).unwrap()
    });
    assert!(
        marked
            .iter()
            .any(|span| span.marks.contains_key("background_color"))
    );
    assert!(marked.iter().all(|span| !span.marks.contains_key("bold")));
}
