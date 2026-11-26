use lotype_core::{BlockKind, Document, Editor, Error, Pos, Schema, Selection, block, mark};

#[test]
fn attachment_edits_keep_identity_selection_and_history() {
    let mut doc = Document::new(Schema::builtin());
    let text = doc.insert_block(None, 0, block::PARAGRAPH).unwrap();
    let image = doc.insert_block(None, 1, block::IMAGE).unwrap();
    let file = doc.insert_block(None, 2, block::FILE).unwrap();
    let mut editor = Editor::new(doc);
    let selected = Selection::caret(Pos::new(text, 0));
    editor.set_selection(Some(selected.clone())).unwrap();
    let image_kind = BlockKind::Image {
        url: "assets:logo".into(),
        caption: "Company logo".into(),
    };
    let file_kind = BlockKind::File {
        url: "assets:report".into(),
        name: "Report.pdf".into(),
    };

    assert!(matches!(
        editor.set_file(image, "x", "y"),
        Err(Error::NotOfType(block::FILE))
    ));
    editor
        .set_image(image, "assets:logo", "Company logo")
        .unwrap();
    editor
        .set_file(file, "assets:report", "Report.pdf")
        .unwrap();
    assert_eq!(editor.selection(), Some(&selected));
    assert_eq!(
        BlockKind::of(editor.document(), image),
        Some(image_kind.clone())
    );
    assert_eq!(
        BlockKind::of(editor.document(), file),
        Some(file_kind.clone())
    );

    let mut peer = Document::new(Schema::builtin());
    peer.import(&editor.document().export_snapshot().unwrap())
        .unwrap();
    assert_eq!(BlockKind::of(&peer, image), Some(image_kind.clone()));
    assert_eq!(BlockKind::of(&peer, file), Some(file_kind.clone()));
    let copy = peer.slice_blocks(&[image, file]).unwrap();
    assert_eq!(copy.to_plain_text(), "assets:logo\nassets:report");
    assert_eq!(
        BlockKind::of(&copy, copy.blocks()[1]),
        Some(file_kind.clone())
    );

    assert!(editor.undo().unwrap());
    assert_eq!(BlockKind::of(editor.document(), image), Some(image_kind));
    assert!(
        matches!(BlockKind::of(editor.document(), file), Some(BlockKind::File { url, name }) if url.is_empty() && name.is_empty())
    );
    assert!(editor.redo().unwrap());
    assert_eq!(
        BlockKind::of(editor.document(), file),
        Some(file_kind.clone())
    );
    editor.set_file(file, "", "").unwrap();
    assert!(editor.undo().unwrap());
    assert_eq!(BlockKind::of(editor.document(), file), Some(file_kind));
}

#[test]
fn markdown_restores_media_without_promoting_ordinary_links() {
    let kinds = [
        BlockKind::Image {
            url: "assets:logo".into(),
            caption: "assets:logo".into(),
        },
        BlockKind::Image {
            url: "assets:photo".into(),
            caption: String::new(),
        },
        BlockKind::File {
            url: "https://example.com/a (b).pdf?x=1&y=2".into(),
            name: "Report [2026].pdf".into(),
        },
        BlockKind::File {
            url: "assets:unnamed".into(),
            name: String::new(),
        },
        BlockKind::File {
            url: String::new(),
            name: String::new(),
        },
    ];
    let mut doc = Document::new(Schema::builtin());
    for (index, kind) in kinds.iter().enumerate() {
        doc.insert_kind(None, index, kind).unwrap();
    }
    let markdown = doc.to_markdown();
    let mut copy = Document::new(Schema::builtin());
    let blocks = copy.insert_markdown(None, 0, &markdown).unwrap();
    assert_eq!(blocks.len(), kinds.len());
    for (block, kind) in blocks.into_iter().zip(kinds) {
        assert_eq!(BlockKind::of(&copy, block), Some(kind));
    }

    let ordinary =
        "[Web link](https://example.com)\n\nRead [Report](assets:report \"lotype-file\").";
    let links = copy.insert_markdown(None, 0, ordinary).unwrap();
    assert!(
        links
            .iter()
            .all(|block| copy.block_type(*block).unwrap() == block::PARAGRAPH)
    );
    let mut plain = Document::new(
        Schema::new()
            .block(lotype_core::BlockSpec::new(
                block::PARAGRAPH,
                lotype_core::Content::Inline,
            ))
            .mark(lotype_core::MarkSpec::new(mark::LINK, lotype_core::Expand::None).with_value()),
    );
    let made = plain
        .insert_markdown(None, 0, "[Report](assets:report \"lotype-file\")")
        .unwrap();
    assert_eq!(plain.plain_text(made[0]).unwrap(), "Report");
    assert_eq!(
        plain.spans(made[0]).unwrap()[0].marks[mark::LINK],
        "assets:report".into()
    );
}

#[test]
fn nested_markup_in_an_image_caption_keeps_the_outer_image_reference() {
    let mut doc = Document::new(Schema::builtin());
    let made = doc
        .insert_markdown(
            None,
            0,
            "![A ![small icon](inner.png) and [more](page)](outer.png)",
        )
        .unwrap();
    assert_eq!(
        BlockKind::of(&doc, made[0]),
        Some(BlockKind::Image {
            url: "outer.png".into(),
            caption: "A small icon and more".into(),
        })
    );
}

#[test]
fn an_unchanged_attachment_keeps_pending_typing_marks() {
    let mut doc = Document::new(Schema::builtin());
    let paragraph = doc.insert_block(None, 0, block::PARAGRAPH).unwrap();
    let image = doc.insert_block(None, 1, block::IMAGE).unwrap();
    let mut editor = Editor::new(doc);
    editor
        .set_selection(Some(Selection::caret(Pos::new(paragraph, 0))))
        .unwrap();
    editor.toggle_mark(mark::BOLD).unwrap();
    editor.set_image(image, "", "").unwrap();
    editor.insert_text("bold").unwrap();
    assert_eq!(
        editor.document().spans(paragraph).unwrap()[0].marks[mark::BOLD],
        true.into()
    );
}

#[test]
fn a_preview_can_label_a_file_without_changing_its_destination() {
    let mut doc = Document::new(Schema::builtin());
    let block = doc
        .insert_markdown(
            None,
            0,
            "[![Preview](preview.png)](report.pdf \"lotype-file\")",
        )
        .unwrap()[0];
    assert_eq!(
        BlockKind::of(&doc, block),
        Some(BlockKind::File {
            url: "report.pdf".into(),
            name: "Preview".into(),
        })
    );
}

#[test]
fn marked_links_keep_their_labels_in_headings_fences_and_schemas_without_files() {
    use lotype_core::{BlockSpec, Content, Expand, MarkSpec};
    let link = "[Download](report.pdf \"lotype-file-unnamed\")";
    let mut plain = Document::new(
        Schema::new()
            .block(BlockSpec::new(block::PARAGRAPH, Content::Inline))
            .mark(MarkSpec::new(mark::LINK, Expand::None).with_value()),
    );
    let blocks = plain
        .insert_markdown(
            None,
            0,
            &format!("{link}\n\n[File](<> \"lotype-file-unnamed\")"),
        )
        .unwrap();
    assert_eq!(plain.plain_text(blocks[0]).unwrap(), "Download");
    assert_eq!(plain.plain_text(blocks[1]).unwrap(), "File");
    let mut rich = Document::new(
        Schema::builtin()
            .block(BlockSpec::new("callout", Content::Inline).written_as_fence("callout")),
    );
    let blocks = rich
        .insert_markdown(None, 0, &format!("- # {link}\n\n```callout\n{link}\n```"))
        .unwrap();
    assert!(rich.children(Some(blocks[0])).unwrap().is_empty());
    for block in blocks {
        let spans = rich.spans(block).unwrap();
        assert_eq!(spans[0].text, "Download");
        assert_eq!(spans[0].marks[mark::LINK], "report.pdf".into());
    }
}
