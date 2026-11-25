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

#[gpui::test]
fn an_application_draws_an_alert_kind_its_own_way(cx: &mut TestAppContext) {
    use gpui::{Hsla, Rgba};
    use lotype_ui::{AlertLook, EditorView, Shade};
    let red = Hsla::from(Rgba::try_from("#ff0000").unwrap());
    let hint = AlertLook {
        title: "Hint".into(),
        bar: Shade::same(red),
        color: Shade::same(red),
    };
    let tip = block("alert", vec![p!["inside"].into()]).attr("kind", "tip");
    let editor = lotype_core::test_support::editor(blocks![p!["<a>a"], tip]);
    let mut ed = EditorTestContext::with_view(cx, |cx| {
        EditorView::new(editor, cx).with_alert_look(lotype_core::AlertKind::Tip, hint)
    });
    ed.settle();
    // Its title still opens the menu of kinds, and a pick still sets one.
    let title = ed.cx.debug_bounds("alert-kind").unwrap().center();
    ed.cx.simulate_click(title, Modifiers::none());
    ed.settle();
    let caution = ed.cx.debug_bounds("alert-kind-caution").unwrap().center();
    ed.cx.simulate_click(caution, Modifiers::none());
    assert_eq!(kind(&mut ed), Some(Value::Str("caution".into())));
}
