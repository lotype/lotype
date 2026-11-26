use std::path::PathBuf;

use gpui::{Modifiers, Task, TestAppContext};
use lotype_core::test_support::{block, editor};
use lotype_core::{BlockKind, Document, Schema, attr, blocks, p};
use lotype_ui::test_support::EditorTestContext;
use lotype_ui::{EditorView, MediaSource};

fn click(editor: &mut EditorTestContext, selector: &'static str) {
    editor.settle();
    let bounds = editor
        .cx
        .debug_bounds(selector)
        .unwrap_or_else(|| panic!("{selector} shows"));
    editor.cx.simulate_click(bounds.center(), Modifiers::none());
    editor.settle();
}

#[gpui::test]
fn a_file_from_the_slash_menu_can_be_edited_opened_and_undone(cx: &mut TestAppContext) {
    let mut editor = EditorTestContext::new(cx, blocks![p!["<a>"]]);
    editor.type_text("/file");
    editor.keys("enter");
    let file = editor.editor(|editor| editor.document().blocks()[0]);
    click(&mut editor, "media-edit");
    editor.type_text("https://example.com/report.pdf");
    click(&mut editor, "media-label");
    editor.type_text("Quarterly report");
    click(&mut editor, "media-save");
    let stored = editor.editor(|editor| BlockKind::of(editor.document(), file));
    assert_eq!(
        stored,
        Some(BlockKind::File {
            url: "https://example.com/report.pdf".into(),
            name: "Quarterly report".into(),
        })
    );
    click(&mut editor, "media-open");
    assert_eq!(
        editor.cx.opened_url().as_deref(),
        Some("https://example.com/report.pdf")
    );
    editor.view.update(editor.cx, |view, cx| {
        assert!(view.update_editor(cx, |editor| editor.undo().unwrap()));
    });
    assert!(editor.editor(|editor| matches!(
        BlockKind::of(editor.document(), file),
        Some(BlockKind::File { url, name }) if url.is_empty() && name.is_empty()
    )));
}

#[gpui::test]
fn image_picker_saves_the_hosts_reference_and_cancel_leaves_it_alone(cx: &mut TestAppContext) {
    let mut editor = EditorTestContext::with_view(cx, |cx| {
        EditorView::new(editor(blocks![p!["<a>"]]), cx).with_media_importer(|path, _| {
            assert_eq!(path, PathBuf::from("/test/logo.png"));
            Task::ready(Ok("assets:logo".into()))
        })
    });
    editor.type_text("/image");
    editor.keys("enter");
    let image = editor.editor(|editor| editor.document().blocks()[0]);
    click(&mut editor, "media-edit");
    click(&mut editor, "media-browse");
    assert!(editor.cx.did_prompt_for_paths());
    editor.cx.simulate_path_prompt_response(|options| {
        assert!(options.files && !options.directories && !options.multiple);
        Some(vec![PathBuf::from("/test/logo.png")])
    });
    click(&mut editor, "media-label");
    editor.type_text("Company logo");
    editor.keys("enter");
    let stored = Some(BlockKind::Image {
        url: "assets:logo".into(),
        caption: "Company logo".into(),
    });
    assert_eq!(
        editor.editor(|editor| BlockKind::of(editor.document(), image)),
        stored
    );

    click(&mut editor, "media-edit");
    editor.keys("cmd-a");
    editor.type_text("file:///some/other/file");
    editor.keys("escape");
    assert!(editor.cx.debug_bounds("media-reference").is_none());
    assert_eq!(
        editor.editor(|editor| BlockKind::of(editor.document(), image)),
        stored
    );
}

#[gpui::test]
fn a_document_reference_does_not_open_an_arbitrary_local_file(cx: &mut TestAppContext) {
    let mut editor = EditorTestContext::new(cx, blocks![p!["<a>"]]);
    editor.type_text("/file");
    editor.keys("enter");
    click(&mut editor, "media-edit");
    editor.type_text("file:///some/local/file");
    editor.keys("enter");
    assert!(editor.cx.debug_bounds("media-open").is_none());
}

#[gpui::test]
fn the_host_can_resolve_an_image_to_a_local_preview(cx: &mut TestAppContext) {
    let mut editor = EditorTestContext::with_view(cx, |cx| {
        let model = editor(blocks![block("image", vec![]).attr("url", "assets:logo")]);
        EditorView::new(model, cx).with_media_resolver(|reference| {
            assert_eq!(reference, "assets:logo");
            let logo = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets/logo.png");
            Some(MediaSource::Path(logo))
        })
    });
    editor.settle();
    let preview = editor
        .cx
        .debug_bounds("media-preview")
        .expect("image preview");
    assert!(preview.size.height > gpui::px(100.));
    assert!(preview.size.height <= gpui::px(420.));
}

#[gpui::test]
fn saving_a_stale_form_leaves_the_peers_attachment_alone(cx: &mut TestAppContext) {
    let mut editor = EditorTestContext::new(
        cx,
        blocks![block("file", vec![]).attr("url", "https://example.com/old.pdf")],
    );
    let file = editor.editor(|editor| editor.document().blocks()[0]);
    click(&mut editor, "media-edit");
    editor.keys("cmd-a");
    editor.type_text("https://example.com/local.pdf");
    editor.view.update(editor.cx, |view, cx| {
        view.update_editor(cx, |editor| {
            let mut peer = Document::new(Schema::builtin());
            peer.import(&editor.document().export_snapshot().unwrap())
                .unwrap();
            peer.set_attr(file, attr::URL, "https://example.com/peer.pdf")
                .unwrap();
            editor.import(&peer.export_snapshot().unwrap()).unwrap();
        });
    });
    editor.keys("enter");
    assert!(editor.cx.debug_bounds("media-error").is_some());
    assert_eq!(
        editor.editor(|editor| editor.document().text_of(file, attr::URL)),
        "https://example.com/peer.pdf"
    );
}
