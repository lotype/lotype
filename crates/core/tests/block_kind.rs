use lotype_core::test_support::{editor, outline};
use lotype_core::{AlertKind, BlockKind, Document, Schema, blocks, p};

#[test]
fn a_block_turns_into_a_kind_and_reads_back_as_it() {
    let mut ed = editor(blocks![p!["<a>a"]]);
    let block = ed.document().blocks()[0];
    for kind in [
        BlockKind::Heading(3),
        BlockKind::Quote,
        BlockKind::CheckListItem { checked: true },
        BlockKind::CodeBlock {
            language: Some("rust".into()),
        },
        BlockKind::NumberedListItem,
        BlockKind::Paragraph,
    ] {
        ed.turn_into(&kind).unwrap();
        assert_eq!(BlockKind::of(ed.document(), block), Some(kind));
    }
    // A level out of range is brought into it.
    ed.turn_into(&BlockKind::Heading(9)).unwrap();
    assert_eq!(
        BlockKind::of(ed.document(), block),
        Some(BlockKind::Heading(5))
    );
}

#[test]
fn blocks_of_a_kind_are_made_with_their_attributes() {
    let mut doc = Document::new(Schema::builtin());
    let image = BlockKind::Image {
        url: "https://x.y/a.png".into(),
        caption: String::new(),
    };
    let made = doc.insert_kind(None, 0, &image).unwrap();
    doc.insert_kind(None, 1, &BlockKind::Alert(AlertKind::Warning))
        .unwrap();
    assert_eq!(
        outline(&doc, None),
        "image url=https://x.y/a.png\nalert kind=warning\n"
    );
    assert_eq!(BlockKind::of(&doc, made), Some(image));
}
