use gpui::{Modifiers, TestAppContext, point, px};
use lotype_core::test_support::outline;
use lotype_core::{BlockSpec, Content, Document, Editor, Pos, Role, Schema, Selection};
use lotype_ui::EditorView;
use lotype_ui::test_support::EditorTestContext;

#[gpui::test]
fn a_toggle_of_a_schema_of_its_own_opens_closes_and_writes_in_it(cx: &mut TestAppContext) {
    let build = |cx: &mut gpui::Context<EditorView>| {
        let text = |name: &str, role| BlockSpec::new(name, Content::Inline).with_role(role);
        let schema = Schema::new()
            .with_text_block("line")
            .block(text("line", Role::Text).with_children())
            .block(text("fold", Role::Toggle).with_children());
        let mut doc = Document::new(schema);
        let fold = doc.insert_block(None, 0, "fold").unwrap();
        doc.insert_text(fold, 0, "title").unwrap();
        let mut editor = Editor::new(doc);
        let caret = Selection::caret(Pos::new(fold, 5));
        editor.set_selection(Some(caret)).unwrap();
        EditorView::new(editor, cx)
    };
    let mut ed = EditorTestContext::with_view(cx, build);
    ed.keys("enter");
    ed.type_text("inside");
    let shown = |ed: &mut EditorTestContext| ed.editor(|e| outline(e.document(), e.selection()));
    assert_eq!(shown(&mut ed), "fold \"title\"\n  line \"inside<a>\"\n");
    // Its arrow closes it, which hides what is in it.
    let fold = ed.editor(|e| e.document().blocks()[0]);
    let inside = ed.editor(|e| e.document().blocks()[1]);
    let arrow = ed.view.update(ed.cx, |view, _| {
        let text = view.text_bounds(fold).unwrap();
        point(text.left() - px(12.), text.top() + px(11.))
    });
    ed.cx.simulate_click(arrow, Modifiers::none());
    ed.settle();
    assert!(
        ed.view
            .update(ed.cx, |view, _| view.text_bounds(inside))
            .is_none()
    );
}
