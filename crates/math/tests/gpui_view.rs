#![cfg(feature = "test-support")]

use gpui::{Focusable as _, Modifiers, ScrollDelta, ScrollEvent, TestAppContext, point, px};
use lotype_core::{Document, Editor, OBJECT, Pos, Schema, Selection, block};
use lotype_math::{BLOCK_TYPE, INLINE_TYPE, block_tex, extend_schema, inline_tex};
use lotype_ui::EditorView;
use lotype_ui::test_support::EditorTestContext;

fn editor_is_focused(ed: &mut EditorTestContext) -> bool {
    let focus = ed.view.read_with(ed.cx, |view, cx| view.focus_handle(cx));
    ed.cx.update(|window, _| focus.is_focused(window))
}

#[gpui::test]
fn block_equation_toggles_between_preview_and_live_edit(cx: &mut TestAppContext) {
    let schema = extend_schema(Schema::builtin());
    let mut doc = Document::new(schema);
    let ids = doc
        .insert_markdown(None, 0, "Above\n\n$$\nE = mc^2\n$$\n\nBelow")
        .unwrap();
    let math_id = ids[1];
    assert_eq!(doc.block_type(math_id).unwrap(), BLOCK_TYPE);

    let mut editor = Editor::new(doc);
    editor
        .set_selection(Some(Selection::caret(Pos::new(ids[0], 0))))
        .unwrap();

    let mut ed = EditorTestContext::with_view(cx, |cx| {
        let view = EditorView::new(editor, cx);
        lotype_math::install(view, cx)
    });

    // Initially caret is in "Above", so math block is folded in preview mode.
    assert!(ed.cx.debug_bounds("math-block").is_some());
    assert!(ed.cx.debug_bounds("math-block-editing").is_none());

    // Clicking the rendered equation enters live-edit mode.
    let bounds = ed.cx.debug_bounds("math-block").unwrap();
    ed.cx.simulate_click(bounds.center(), Modifiers::none());
    ed.settle();
    assert!(ed.cx.debug_bounds("math-block-editing").is_some());
    assert!(ed.cx.debug_bounds("math-source").is_some());

    // Typing invalid LaTeX shows an inline error banner without losing editing focus.
    ed.type_text(r" + \frac{1}");
    ed.settle();
    assert!(ed.cx.debug_bounds("math-error").is_some());

    // Completing the fraction clears the error and updates the live preview.
    ed.type_text("{2}");
    ed.settle();
    assert!(ed.cx.debug_bounds("math-error").is_none());
    assert_eq!(
        ed.editor(|editor| block_tex(editor.document(), math_id)),
        Some(r"E = mc^2 + \frac{1}{2}".to_owned())
    );

    // Clicking Done in the header folds back to pure rendered display math.
    let done = ed
        .cx
        .debug_bounds("math-done")
        .expect("Done button visible");
    ed.cx.simulate_click(done.center(), Modifiers::none());
    ed.settle();
    assert!(ed.cx.debug_bounds("math-block").is_some());
    assert!(ed.cx.debug_bounds("math-block-editing").is_none());

    // Re-enter edit mode and verify Escape also folds back cleanly.
    let bounds = ed.cx.debug_bounds("math-block").unwrap();
    ed.cx.simulate_click(bounds.center(), Modifiers::none());
    ed.settle();
    assert!(ed.cx.debug_bounds("math-block-editing").is_some());
    ed.keys("escape");
    ed.settle();
    assert!(ed.cx.debug_bounds("math-block").is_some());
    assert!(ed.cx.debug_bounds("math-block-editing").is_none());
}

#[gpui::test]
fn inline_equation_popover_edits_and_deletes_atom(cx: &mut TestAppContext) {
    let schema = extend_schema(Schema::builtin());
    let mut doc = Document::new(schema);
    let ids = doc.insert_markdown(None, 0, "Formula $x^2$ here.").unwrap();
    let para = ids[0];

    let mut editor = Editor::new(doc);
    editor
        .set_selection(Some(Selection::caret(Pos::new(para, 0))))
        .unwrap();

    let mut ed = EditorTestContext::with_view(cx, |cx| {
        let view = EditorView::new(editor, cx);
        lotype_math::install(view, cx)
    });

    let inline_bounds = ed
        .cx
        .debug_bounds("inline-math")
        .expect("inline math atom rendered");
    ed.cx
        .simulate_click(inline_bounds.center(), Modifiers::none());
    ed.settle();
    assert!(ed.cx.debug_bounds("inline-math-popover").is_some());
    assert!(ed.cx.debug_bounds("math-copy").is_some());
    assert!(ed.cx.debug_bounds("math-source").is_some());

    // Append `+1` inside the popover input and click Done.
    ed.cx.simulate_keystrokes("+ 1");
    ed.settle();
    let done = ed.cx.debug_bounds("inline-math-done").unwrap();
    ed.cx.simulate_click(done.center(), Modifiers::none());
    ed.settle();
    assert!(ed.cx.debug_bounds("inline-math-popover").is_none());

    let tex = ed.editor(|editor| {
        editor
            .document()
            .spans(para)
            .unwrap()
            .into_iter()
            .find_map(|s| s.atom.filter(|a| a.name == INLINE_TYPE))
            .map(|a| inline_tex(&a).to_owned())
    });
    assert_eq!(tex.as_deref(), Some("x^2+1"));

    // Click again and delete the inline equation via the Delete button.
    let inline_bounds = ed.cx.debug_bounds("inline-math").unwrap();
    ed.cx
        .simulate_click(inline_bounds.center(), Modifiers::none());
    ed.settle();
    let delete = ed.cx.debug_bounds("inline-math-delete").unwrap();
    ed.cx.simulate_click(delete.center(), Modifiers::none());
    ed.settle();
    assert!(ed.cx.debug_bounds("inline-math").is_none());
    assert_eq!(
        ed.editor(|editor| editor.document().plain_text(para).unwrap()),
        "Formula  here."
    );
}

#[gpui::test]
fn slash_menu_inserts_equation_block(cx: &mut TestAppContext) {
    let schema = extend_schema(Schema::builtin());
    let mut doc = Document::new(schema);
    let para = doc.insert_block(None, 0, block::PARAGRAPH).unwrap();
    let mut editor = Editor::new(doc);
    editor
        .set_selection(Some(Selection::caret(Pos::new(para, 0))))
        .unwrap();

    let mut ed = EditorTestContext::with_view(cx, |cx| {
        let view = EditorView::new(editor, cx);
        lotype_math::install(view, cx)
    });

    ed.type_text("/");
    ed.settle();
    // Equation is grouped directly under Advanced alongside Table, before Media.
    let table_bounds = ed.cx.debug_bounds("slash-table").unwrap();
    let math_bounds = ed.cx.debug_bounds("slash-math").unwrap();
    let image_bounds = ed.cx.debug_bounds("slash-image").unwrap();
    assert!(
        table_bounds.bottom() <= math_bounds.top() && math_bounds.bottom() <= image_bounds.top(),
        "expected Equation between Table and Image in Advanced group: table={table_bounds:?}, math={math_bounds:?}, image={image_bounds:?}"
    );

    ed.type_text("math");
    ed.settle();
    assert!(ed.cx.debug_bounds("slash-math").is_some());
    ed.keys("enter");
    ed.settle();
    assert_eq!(
        ed.editor(|editor| editor.document().block_type(para).unwrap()),
        BLOCK_TYPE
    );
    assert!(ed.cx.debug_bounds("math-block-editing").is_some());
}

#[gpui::test]
fn wide_equation_renders_inside_horizontal_scroll_container(cx: &mut TestAppContext) {
    let schema = extend_schema(Schema::builtin());
    let mut doc = Document::new(schema);
    let wide = r"a_1 + a_2 + a_3 + a_4 + a_5 + a_6 + a_7 + a_8 + a_9 + a_{10} + a_{11} + a_{12} + a_{13} + a_{14} = \sum_{k=1}^{14} a_k";
    let ids = doc
        .insert_markdown(None, 0, &format!("Above\n\n$$\n{wide}\n$$"))
        .unwrap();
    let mut editor = Editor::new(doc);
    editor
        .set_selection(Some(Selection::caret(Pos::new(ids[0], 0))))
        .unwrap();

    let mut ed = EditorTestContext::with_view(cx, |cx| {
        let view = EditorView::new(editor, cx);
        lotype_math::install(view, cx)
    });
    ed.cx
        .simulate_resize(gpui::size(gpui::px(320.), gpui::px(400.)));
    ed.settle();

    let block_bounds = ed.cx.debug_bounds("math-block").unwrap();
    assert!(block_bounds.size.width <= gpui::px(320.));

    // Click to enter live edit mode and verify the preview still fits inside the container width.
    ed.cx
        .simulate_click(block_bounds.center(), Modifiers::none());
    ed.settle();
    let editing_bounds = ed.cx.debug_bounds("math-block-editing").unwrap();
    assert!(editing_bounds.size.width <= gpui::px(320.));
}

#[gpui::test]
fn long_inline_equation_wraps_source_and_commits_on_enter(cx: &mut TestAppContext) {
    let schema = extend_schema(Schema::builtin());
    let mut doc = Document::new(schema);
    let long_tex = r"\frac{1}{\sqrt{2\pi\sigma^2}} \exp\left(-\frac{(x-\mu)^2}{2\sigma^2}\right) + \sum_{k=1}^{n} \frac{\alpha_k}{k^2}";
    let ids = doc
        .insert_markdown(None, 0, &format!("Density ${long_tex}$ here."))
        .unwrap();
    let para = ids[0];

    let mut editor = Editor::new(doc);
    editor
        .set_selection(Some(Selection::caret(Pos::new(para, 0))))
        .unwrap();

    let mut ed = EditorTestContext::with_view(cx, |cx| {
        let view = EditorView::new(editor, cx);
        lotype_math::install(view, cx)
    });

    let inline_bounds = ed.cx.debug_bounds("inline-math").unwrap();
    ed.cx
        .simulate_click(inline_bounds.center(), Modifiers::none());
    ed.settle();

    // Long LaTeX wraps onto multiple rows inside the auto-growing textarea.
    let input_bounds = ed.cx.debug_bounds("inline-math-input").unwrap();
    assert!(
        input_bounds.size.height > gpui::px(30.),
        "expected wrapped multi-line height, got {:?}",
        input_bounds.size.height
    );

    // Pressing Enter submits and closes the popover.
    ed.cx.simulate_keystrokes("+ 1 enter");
    ed.settle();
    assert!(ed.cx.debug_bounds("inline-math-popover").is_none());
    let updated = ed.editor(|editor| {
        editor
            .document()
            .spans(para)
            .unwrap()
            .into_iter()
            .find_map(|s| s.atom.filter(|a| a.name == INLINE_TYPE))
            .map(|a| inline_tex(&a).to_owned())
    });
    assert_eq!(updated.as_deref(), Some(format!("{long_tex}+1").as_str()));
}

#[gpui::test]
fn selection_toolbar_is_hidden_in_block_and_inline_equations(cx: &mut TestAppContext) {
    let schema = extend_schema(Schema::builtin());
    let mut doc = Document::new(schema);
    let ids = doc
        .insert_markdown(None, 0, "Hi $x^2$ end.\n\n$$\nE = mc^2\n$$")
        .unwrap();
    let para = ids[0];
    let math_block = ids[1];

    let mut editor = Editor::new(doc);
    editor
        .set_selection(Some(Selection::caret(Pos::new(para, 0))))
        .unwrap();

    let mut ed = EditorTestContext::with_view(cx, |cx| {
        let view = EditorView::new(editor, cx);
        lotype_math::install(view, cx)
    });

    // Selecting plain prose ("Hi") shows the formatting toolbar.
    ed.keys("shift-right shift-right");
    ed.settle();
    assert!(ed.cx.debug_bounds("toolbar-bold").is_some());

    // Selecting only the inline equation atom hides the formatting toolbar.
    let atom_start = "Hi ".len();
    let atom_end = atom_start + lotype_core::OBJECT.len_utf8();
    ed.view.update(ed.cx, |view, cx| {
        view.update_editor(cx, |editor| {
            editor
                .set_selection(Some(Selection::Text {
                    anchor: Pos::new(para, atom_start),
                    head: Pos::new(para, atom_end),
                }))
                .unwrap();
        });
    });
    ed.settle();
    assert!(ed.cx.debug_bounds("toolbar-bold").is_none());

    // Selecting text inside a block equation hides the formatting toolbar.
    ed.view.update(ed.cx, |view, cx| {
        view.update_editor(cx, |editor| {
            editor
                .set_selection(Some(Selection::Text {
                    anchor: Pos::new(math_block, 0),
                    head: Pos::new(math_block, 4),
                }))
                .unwrap();
        });
    });
    ed.settle();
    assert!(ed.cx.debug_bounds("math-block-editing").is_some());
    assert!(ed.cx.debug_bounds("toolbar-bold").is_none());

    // Selecting a paragraph down to offset 0 of the following math block keeps the toolbar visible.
    ed.view.update(ed.cx, |view, cx| {
        view.update_editor(cx, |editor| {
            editor
                .set_selection(Some(Selection::Text {
                    anchor: Pos::new(para, 0),
                    head: Pos::new(math_block, 0),
                }))
                .unwrap();
        });
    });
    ed.settle();
    assert!(ed.cx.debug_bounds("toolbar-bold").is_some());
}

#[gpui::test]
fn inline_popover_commits_to_shifted_atom_after_remote_edit(cx: &mut TestAppContext) {
    let schema = extend_schema(Schema::builtin());
    let mut doc = Document::new(schema.clone());
    let ids = doc.insert_markdown(None, 0, "$a$ $b$").unwrap();
    let para = ids[0];
    let b_offset = OBJECT.len_utf8() + 1;

    // Peer B starts from a snapshot before the popover opens.
    let mut peer = Document::new(schema);
    peer.import(&doc.export_snapshot().unwrap()).unwrap();
    let since = peer.version();

    let mut editor = Editor::new(doc);
    editor
        .set_selection(Some(Selection::caret(Pos::new(para, 0))))
        .unwrap();
    let mut ed = EditorTestContext::with_view(cx, |cx| {
        let view = EditorView::new(editor, cx);
        lotype_math::install(view, cx)
    });

    // Click the second inline equation (`$b$`) at `b_offset`.
    let b_bounds = ed
        .view
        .read_with(ed.cx, |view, _| {
            view.range_bounds(para, b_offset..b_offset + OBJECT.len_utf8())
        })
        .expect("bounds for `$b$`");
    ed.cx.simulate_click(b_bounds.center(), Modifiers::none());
    ed.settle();
    assert!(ed.cx.debug_bounds("inline-math-popover").is_some());

    // Remote peer prepends "See " (4 bytes), shifting `$a$` into `$b$`'s old offset!
    peer.insert_text(para, 0, "See ").unwrap();
    let update = peer.export_updates(&since).unwrap();
    ed.view.update(ed.cx, |view, cx| {
        view.update_editor(cx, |editor| {
            editor.import(&update).unwrap();
        });
    });
    ed.settle();
    assert!(ed.cx.debug_bounds("inline-math-popover").is_some());

    // Committing must update `$b$` without corrupting `$a$`.
    ed.cx.simulate_keystrokes("+ 1 enter");
    ed.settle();
    assert!(ed.cx.debug_bounds("inline-math-popover").is_none());

    let equations: Vec<String> = ed.editor(|editor| {
        editor
            .document()
            .spans(para)
            .unwrap()
            .into_iter()
            .filter_map(|span| span.atom.filter(|a| a.name == INLINE_TYPE))
            .map(|atom| inline_tex(&atom).to_owned())
            .collect()
    });
    assert_eq!(equations, vec!["a".to_owned(), "b+1".to_owned()]);
}

#[gpui::test]
fn inline_popover_follows_scroll_and_closes_when_scrolled_out_of_view(cx: &mut TestAppContext) {
    let schema = extend_schema(Schema::builtin());
    let mut doc = Document::new(schema);
    let ids = doc.insert_markdown(None, 0, "Eq $x^2$ here.").unwrap();
    let top_para = ids[0];
    let mut last_para = top_para;
    for i in 1..=80 {
        let p = doc.insert_block(None, i, block::PARAGRAPH).unwrap();
        doc.insert_text(p, 0, &format!("Paragraph line {i}"))
            .unwrap();
        last_para = p;
    }

    // Keep caret at bottom so scrolling down lets `top_para` virtualize out.
    let mut editor = Editor::new(doc);
    editor
        .set_selection(Some(Selection::caret(Pos::new(last_para, 0))))
        .unwrap();
    let mut ed = EditorTestContext::with_view(cx, |cx| {
        let view = EditorView::new(editor, cx);
        lotype_math::install(view, cx)
    });

    // Scroll back to the top so `top_para` is visible and open its inline popover.
    for _ in 0..10 {
        ed.cx.simulate_event(ScrollEvent {
            position: point(px(300.), px(300.)),
            delta: ScrollDelta::Pixels(point(px(0.), px(400.))),
            ..Default::default()
        });
        ed.settle();
    }
    let inline_bounds = ed
        .cx
        .debug_bounds("inline-math")
        .expect("top inline math visible");
    ed.cx
        .simulate_click(inline_bounds.center(), Modifiers::none());
    ed.settle();
    let initial_popover = ed
        .cx
        .debug_bounds("inline-math-popover")
        .expect("popover open");
    assert!(!editor_is_focused(&mut ed));

    // A small downward scroll moves the popover upward with its equation instead of freezing.
    ed.cx.simulate_event(ScrollEvent {
        position: point(px(300.), px(300.)),
        delta: ScrollDelta::Pixels(point(px(0.), px(-40.))),
        ..Default::default()
    });
    ed.settle();
    let moved_popover = ed
        .cx
        .debug_bounds("inline-math-popover")
        .expect("popover stays open while equation is visible");
    assert!(
        moved_popover.top() < initial_popover.top(),
        "expected popover to follow scroll: before={initial_popover:?}, after={moved_popover:?}"
    );

    // Scrolling far downward virtualizes `top_para`, closes the popover, and restores editor focus.
    for _ in 0..8 {
        ed.cx.simulate_event(ScrollEvent {
            position: point(px(300.), px(300.)),
            delta: ScrollDelta::Pixels(point(px(0.), px(-400.))),
            ..Default::default()
        });
        ed.settle();
    }
    assert!(ed.cx.debug_bounds("inline-math-popover").is_none());
    assert!(editor_is_focused(&mut ed));
}

#[gpui::test]
fn clearing_block_equation_drops_stale_preview(cx: &mut TestAppContext) {
    let schema = extend_schema(Schema::builtin());
    let mut doc = Document::new(schema);
    let ids = doc
        .insert_markdown(None, 0, "Above\n\n$$\nE = mc^2\n$$")
        .unwrap();
    let math_id = ids[1];
    let mut editor = Editor::new(doc);
    editor
        .set_selection(Some(Selection::caret(Pos::new(ids[0], 0))))
        .unwrap();

    let mut ed = EditorTestContext::with_view(cx, |cx| {
        let view = EditorView::new(editor, cx);
        lotype_math::install(view, cx)
    });

    // Click into edit mode and break the formula: stale preview of `E = mc^2` is shown.
    let bounds = ed.cx.debug_bounds("math-block").unwrap();
    ed.cx.simulate_click(bounds.center(), Modifiers::none());
    ed.settle();
    ed.type_text(r" + \frac{1}");
    ed.settle();
    assert!(ed.cx.debug_bounds("math-error").is_some());
    assert!(ed.cx.debug_bounds("math-stale-preview").is_some());

    // Empty the block completely, then type a new incomplete formula: old formula must not reappear.
    ed.view.update(ed.cx, |view, cx| {
        view.update_editor(cx, |editor| {
            let len = editor.document().text_len(math_id).unwrap();
            editor
                .set_selection(Some(Selection::Text {
                    anchor: Pos::new(math_id, 0),
                    head: Pos::new(math_id, len),
                }))
                .unwrap();
            editor.delete_backward().unwrap();
        });
    });
    ed.settle();
    assert!(ed.cx.debug_bounds("math-error").is_none());

    ed.type_text(r"\frac{1}");
    ed.settle();
    assert!(ed.cx.debug_bounds("math-error").is_some());
    assert!(ed.cx.debug_bounds("math-stale-preview").is_none());
}

#[gpui::test]
fn inline_equation_scales_to_heading_font_size(cx: &mut TestAppContext) {
    let schema = extend_schema(Schema::builtin());

    let mut body_doc = Document::new(schema.clone());
    body_doc
        .insert_markdown(None, 0, "Body $E = mc^2$")
        .unwrap();
    let body_ed = EditorTestContext::with_view(cx, |cx| {
        lotype_math::install(EditorView::new(Editor::new(body_doc), cx), cx)
    });
    let body_bounds = body_ed.cx.debug_bounds("inline-math").unwrap();

    let mut h1_doc = Document::new(schema);
    h1_doc
        .insert_markdown(None, 0, "# Heading $E = mc^2$")
        .unwrap();
    let h1_ed = EditorTestContext::with_view(cx, |cx| {
        lotype_math::install(EditorView::new(Editor::new(h1_doc), cx), cx)
    });
    let h1_bounds = h1_ed.cx.debug_bounds("inline-math").unwrap();

    assert!(
        h1_bounds.size.width > body_bounds.size.width * 1.5
            && h1_bounds.size.height > body_bounds.size.height * 1.5,
        "expected H1 inline math ({h1_bounds:?}) to scale above body ({body_bounds:?})"
    );
}
