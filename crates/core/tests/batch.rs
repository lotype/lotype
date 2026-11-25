use lotype_core::test_support::editor;
use lotype_core::{Document, Schema, blocks, p};

/// A document of `count` paragraphs with a removed block among its top
/// blocks: what is no block is then among the children Loro counts.
fn long(count: usize) -> Document {
    let mut doc = Document::new(Schema::builtin());
    let gone = doc.insert_block(None, 0, "paragraph").unwrap();
    doc.remove_block(gone).unwrap();
    let text: String = (0..count).map(|at| format!("old {at}\n\n")).collect();
    doc.insert_markdown(None, 0, &text).unwrap();
    doc
}

fn texts(doc: &Document) -> Vec<String> {
    let blocks = doc.blocks().into_iter();
    blocks.map(|block| doc.plain_text(block).unwrap()).collect()
}

#[test]
fn blocks_read_into_the_middle_are_in_order_among_those_there() {
    let mut doc = long(6);
    let made = doc
        .insert_markdown(None, 3, "new a\n\n- new b\n  - inside\n- new c\n\nnew d\n")
        .unwrap();
    assert_eq!(made.len(), 4);
    assert_eq!(
        texts(&doc),
        [
            "old 0", "old 1", "old 2", "new a", "new b", "inside", "new c", "new d", "old 3",
            "old 4", "old 5"
        ]
    );
    // A copy of those, put before the one they follow.
    let copy = doc.slice_blocks(&made).unwrap();
    doc.insert_document(None, 2, &copy).unwrap();
    assert_eq!(
        texts(&doc)[..8],
        [
            "old 0", "old 1", "new a", "new b", "inside", "new c", "new d", "old 2"
        ]
    );
}

#[test]
fn an_editor_s_paste_into_the_middle_is_in_order_and_one_step() {
    let mut ed = editor(blocks![p!["first"], p!["gone"], p!["mid<a>"], p!["last"]]);
    let gone = ed.document().blocks()[1];
    ed.apply(|doc, selection| {
        doc.remove_block(gone)?;
        Ok(selection.cloned())
    })
    .unwrap();
    let text: String = (0..40).map(|at| format!("pasted {at}\n\n")).collect();
    ed.paste_markdown(&text).unwrap();
    let all = texts(ed.document());
    // The first of them goes on in the block the caret is in.
    assert_eq!(all.len(), 42);
    assert_eq!(all[0], "first");
    assert!(all[1].starts_with("mid"));
    assert_eq!(all[41], "last");
    let pasted: Vec<_> = all.iter().filter(|text| text.contains("pasted")).collect();
    assert!(pasted.windows(2).all(|pair| {
        let number = |text: &String| text.rsplit(' ').next().unwrap().parse::<usize>().unwrap();
        number(pair[0]) < number(pair[1])
    }));
    ed.undo().unwrap();
    assert_eq!(texts(ed.document()), ["first", "mid", "last"]);
}
