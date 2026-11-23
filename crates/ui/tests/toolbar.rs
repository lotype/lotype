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
