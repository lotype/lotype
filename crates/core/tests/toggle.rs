use lotype_core::test_support::{assert_editor, block, editor};
use lotype_core::{blocks, p};

fn toggle(
    text: &str,
    children: Vec<lotype_core::test_support::BlockNode>,
) -> lotype_core::test_support::BlockNode {
    let mut items = vec![text.into()];
    items.extend(children.into_iter().map(Into::into));
    block("toggle_list_item", items)
}

#[test]
fn enter_in_an_open_toggle_writes_in_it() {
    let mut ed = editor(blocks![toggle("title<a>", vec![p!["child"]])]);
    ed.split_toggle(true).unwrap();
    assert_editor(&ed, blocks![toggle("title", vec![p!["<a>"], p!["child"]])]);
    // The text after the caret goes in with it, as one undo step.
    let mut ed = editor(blocks![toggle("ti<a>tle", vec![])]);
    ed.split_toggle(true).unwrap();
    assert_editor(&ed, blocks![toggle("ti", vec![p!["<a>tle"]])]);
    ed.undo().unwrap();
    assert_editor(&ed, blocks![toggle("ti<a>tle", vec![])]);
}

#[test]
fn enter_in_a_closed_toggle_starts_another_and_keeps_what_it_hides() {
    let mut ed = editor(blocks![toggle("title<a>", vec![p!["child"]])]);
    ed.split_toggle(false).unwrap();
    assert_editor(
        &ed,
        blocks![toggle("title", vec![p!["child"]]), toggle("<a>", vec![])],
    );
    // Anywhere else, it is Enter.
    let mut ed = editor(blocks![p!["a<a>b"]]);
    ed.split_toggle(true).unwrap();
    assert_editor(&ed, blocks![p!["a"], p!["<a>b"]]);
}
