use lotype_core::{Document, Schema};

#[test]
fn each_block_has_its_own_markdown_and_depth() {
    let text = "# Title\n\nSome **bold** text.\n\n- one\n  - nested\n- [x] done\n\n> quoted\n\n> [!TIP]\n> advice\n\n| a | b |\n| --- | --- |\n| 1 | 2 |\n\n```rust\nlet x = 1;\n```\n\n---\n";
    let mut doc = Document::new(Schema::builtin());
    doc.insert_markdown(None, 0, text).unwrap();
    let blocks = doc.markdown_blocks();
    let shown: Vec<(usize, &str)> = blocks
        .iter()
        .map(|(_, depth, markdown)| (*depth, markdown.as_str()))
        .collect();
    assert_eq!(
        shown,
        [
            (0, "# Title"),
            (0, "Some **bold** text."),
            (0, "- one"),
            (1, "- nested"),
            (0, "- [x] done"),
            (0, "> quoted"),
            (0, "> [!TIP]"),
            (1, "advice"),
            (0, "| a | b |\n| --- | --- |\n| 1 | 2 |"),
            (0, "```rust\nlet x = 1;\n```"),
            (0, "---"),
        ]
    );
    // In reading order, and no row or cell of the table among them.
    let ids: Vec<_> = blocks.iter().map(|(block, ..)| *block).collect();
    let order: Vec<_> = doc
        .blocks()
        .into_iter()
        .filter(|b| ids.contains(b))
        .collect();
    assert_eq!(ids, order);
    assert_eq!(ids.len(), 11);

    // A block's own Markdown makes that block again.
    for (block, _, markdown) in &blocks {
        let mut copy = Document::new(Schema::builtin());
        let made = copy.insert_markdown(None, 0, markdown).unwrap();
        assert_eq!(made.len(), 1, "{markdown}");
        assert_eq!(
            copy.block_type(made[0]).unwrap(),
            doc.block_type(*block).unwrap()
        );
    }
}
