use lotype_core::test_support::{assert_editor, block, editor, outline};
use lotype_core::{Editor, blocks, bullet, h1, p};

fn shown(ed: &Editor) -> String {
    outline(ed.document(), ed.selection())
}

#[test]
fn backspace_at_the_start_of_a_block_makes_it_a_paragraph_first() {
    let mut ed = editor(blocks![p!["a"], h1!["<a>title"]]);
    ed.delete_backward().unwrap();
    assert_editor(&ed, blocks![p!["a"], p!["<a>title"]]);
    // Then it joins the block before it, as a paragraph does.
    ed.delete_backward().unwrap();
    assert_editor(&ed, blocks![p!["a<a>title"]]);

    // A list item keeps what is in it, and a block its colors.
    let item = bullet!["<a>x", p!["child"]].attr("text_color", "red");
    let mut ed = editor(blocks![item]);
    ed.delete_backward().unwrap();
    assert_eq!(
        shown(&ed),
        "paragraph text_color=red \"<a>x\"\n  paragraph \"child\"\n"
    );

    let code = block("code_block", vec!["<a>fn a()".into()]).attr("language", "rust");
    let mut ed = editor(blocks![code]);
    ed.delete_backward().unwrap();
    assert_editor(&ed, blocks![p!["<a>fn a()"]]);
}

#[test]
fn an_empty_first_block_goes_and_the_last_one_stays() {
    let mut ed = editor(blocks![h1!["<a>"], p!["next"]]);
    ed.delete_backward().unwrap();
    assert_editor(&ed, blocks![p!["<a>"], p!["next"]]);
    ed.delete_backward().unwrap();
    assert_editor(&ed, blocks![p!["<a>next"]]);

    // The only block becomes a paragraph, which stays to be written in.
    let mut ed = editor(blocks![h1!["<a>"]]);
    ed.delete_backward().unwrap();
    ed.delete_backward().unwrap();
    assert_editor(&ed, blocks![p!["<a>"]]);
}

#[test]
fn delete_in_an_empty_last_block_makes_it_a_paragraph() {
    let mut ed = editor(blocks![p!["a"], h1!["<a>"]]);
    ed.delete_forward().unwrap();
    assert_editor(&ed, blocks![p!["a"], p!["<a>"]]);
    // With a block after it, Delete joins that one on, as before.
    let mut ed = editor(blocks![h1!["<a>"], p!["b"]]);
    ed.delete_forward().unwrap();
    assert_editor(&ed, blocks![h1!["<a>b"]]);
}

#[test]
fn backspace_right_after_an_input_rule_takes_it_back() {
    let mut ed = editor(blocks![p!["<a>"]]);
    ed.insert_text("# ").unwrap();
    assert_editor(&ed, blocks![h1!["<a>"]]);
    ed.delete_backward().unwrap();
    assert_editor(&ed, blocks![p!["# <a>"]]);

    let mut ed = editor(blocks![p!["<a>"]]);
    ed.insert_text("**b**").unwrap();
    ed.delete_backward().unwrap();
    assert_editor(&ed, blocks![p!["**b**<a>"]]);

    // Once something else happened, Backspace is Backspace.
    let mut ed = editor(blocks![p!["<a>"]]);
    ed.insert_text("# ").unwrap();
    ed.insert_text("x").unwrap();
    ed.delete_backward().unwrap();
    assert_editor(&ed, blocks![h1!["<a>"]]);
}

#[test]
fn backspace_takes_the_first_block_out_of_an_alert() {
    let tip = |items| block("alert", items).attr("kind", "tip");
    let mut ed = editor(blocks![
        p!["a"],
        tip(vec![p!["<a>one"].into(), p!["two"].into()])
    ]);
    ed.delete_backward().unwrap();
    assert_editor(
        &ed,
        blocks![p!["a"], p!["<a>one"], tip(vec![p!["two"].into()])],
    );
    // Then it joins the block before it, as a paragraph does.
    ed.delete_backward().unwrap();
    assert_editor(&ed, blocks![p!["a<a>one"], tip(vec![p!["two"].into()])]);

    // An alert left empty goes, and its block stays to be written in.
    let mut ed = editor(blocks![tip(vec![h1!["<a>"].into()])]);
    ed.delete_backward().unwrap();
    ed.delete_backward().unwrap();
    assert_editor(&ed, blocks![p!["<a>"]]);
    ed.undo().unwrap();
    assert_editor(&ed, blocks![tip(vec![p!["<a>"].into()])]);
}

#[test]
fn delete_before_an_alert_joins_its_first_block_on() {
    let tip = |items| block("alert", items).attr("kind", "tip");
    let mut ed = editor(blocks![
        p!["a<a>"],
        tip(vec![p!["one"].into(), p!["two"].into()])
    ]);
    ed.delete_forward().unwrap();
    assert_editor(&ed, blocks![p!["a<a>one"], tip(vec![p!["two"].into()])]);
    // An alert left empty goes.
    let mut ed = editor(blocks![p!["a<a>"], tip(vec![p!["two"].into()])]);
    ed.delete_forward().unwrap();
    assert_editor(&ed, blocks![p!["a<a>two"]]);
}
