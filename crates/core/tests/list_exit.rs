use lotype_core::test_support::{assert_editor, block, editor};
use lotype_core::{blocks, bullet, p};

#[test]
fn enter_in_an_empty_item_leaves_the_list() {
    let mut ed = editor(blocks![bullet!["a"], bullet!["<a>"]]);
    ed.split_block().unwrap();
    assert_editor(&ed, blocks![bullet!["a"], p!["<a>"]]);

    // One in another item moves out a level first.
    let mut ed = editor(blocks![bullet!["a", bullet!["<a>"]]]);
    ed.split_block().unwrap();
    assert_editor(&ed, blocks![bullet!["a"], bullet!["<a>"]]);
    ed.split_block().unwrap();
    assert_editor(&ed, blocks![bullet!["a"], p!["<a>"]]);

    // A checked item and an empty toggle do too; one with text splits.
    let checked = block("check_list_item", vec!["<a>".into()]).attr("checked", true);
    let mut ed = editor(blocks![checked]);
    ed.split_block().unwrap();
    assert_editor(&ed, blocks![p!["<a>"]]);
    let mut ed = editor(blocks![block("toggle_list_item", vec!["<a>".into()])]);
    ed.split_toggle(true).unwrap();
    assert_editor(&ed, blocks![p!["<a>"]]);
    let mut ed = editor(blocks![bullet!["a<a>"]]);
    ed.split_block().unwrap();
    assert_editor(&ed, blocks![bullet!["a"], bullet!["<a>"]]);
}
