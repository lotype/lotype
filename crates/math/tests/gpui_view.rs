#![cfg(feature = "test-support")]

use gpui::{Modifiers, TestAppContext};
use lotype_core::{Document, Editor, Pos, Schema, Selection, block};
use lotype_math::{BLOCK_TYPE, INLINE_TYPE, block_tex, extend_schema, inline_tex};
use lotype_ui::EditorView;
use lotype_ui::test_support::EditorTestContext;

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
}
