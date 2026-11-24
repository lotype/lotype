use gpui::{Modifiers, TestAppContext};
use lotype_core::test_support::block;
use lotype_core::{Value, blocks, p};
use lotype_ui::test_support::EditorTestContext;

fn kind(ed: &mut EditorTestContext) -> Option<Value> {
    ed.editor(|editor| {
        let doc = editor.document();
        doc.attr(doc.blocks()[1], "kind").unwrap()
    })
}

#[gpui::test]
fn a_click_on_an_alerts_title_picks_its_kind(cx: &mut TestAppContext) {
    let note = block("alert", vec![p!["inside"].into()]).attr("kind", "note");
    let mut ed = EditorTestContext::new(cx, blocks![p!["<a>a"], note]);
    ed.settle();
    assert!(ed.cx.debug_bounds("alert-kind-warning").is_none());
    let title = ed.cx.debug_bounds("alert-kind").unwrap().center();
    ed.cx.simulate_click(title, Modifiers::none());
    ed.settle();
    let warning = ed.cx.debug_bounds("alert-kind-warning").unwrap().center();
    ed.cx.simulate_click(warning, Modifiers::none());
    ed.settle();
    assert_eq!(kind(&mut ed), Some(Value::Str("warning".into())));
    assert!(ed.cx.debug_bounds("alert-kind-warning").is_none());
    // Undo takes the kind back.
    ed.keys("cmd-z");
    assert_eq!(kind(&mut ed), Some(Value::Str("note".into())));
}
