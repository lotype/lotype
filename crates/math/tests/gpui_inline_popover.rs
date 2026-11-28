#![cfg(feature = "test-support")]

use gpui::{Focusable as _, Modifiers, TestAppContext, point};
use lotype_core::{Document, Editor, OBJECT, Pos, Schema, Selection};
use lotype_math::{INLINE_TYPE, extend_schema, inline_tex};
use lotype_ui::EditorView;
use lotype_ui::test_support::EditorTestContext;

fn editor_is_focused(ed: &mut EditorTestContext) -> bool {
    let focus = ed.view.read_with(ed.cx, |view, cx| view.focus_handle(cx));
    ed.cx.update(|window, _| focus.is_focused(window))
}

#[gpui::test]
fn clicking_second_inline_equation_commits_first_popover_edits(cx: &mut TestAppContext) {
    let schema = extend_schema(Schema::builtin());
    let mut doc = Document::new(schema);
    let ids = doc
        .insert_markdown(None, 0, "First $a^2$ and second $b^2$ end.")
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

    let offsets = |ed: &mut EditorTestContext| -> Vec<(usize, String)> {
        ed.editor(|editor| {
            let mut offset = 0usize;
            editor
                .document()
                .spans(para)
                .unwrap()
                .into_iter()
                .filter_map(|span| {
                    let start = offset;
                    offset += span.text.len();
                    span.atom
                        .filter(|atom| atom.name == INLINE_TYPE)
                        .map(|atom| (start, inline_tex(&atom).to_owned()))
                })
                .collect()
        })
    };

    let click_atom = |ed: &mut EditorTestContext, offset: usize| {
        let bounds = ed
            .view
            .read_with(ed.cx, |view, _| {
                view.range_bounds(para, offset..offset + OBJECT.len_utf8())
            })
            .expect("inline atom bounds");
        let atom_y = ed.cx.debug_bounds("inline-math").unwrap().center().y;
        ed.cx
            .simulate_click(point(bounds.center().x, atom_y), Modifiers::none());
        ed.settle();
    };

    let initial_atoms = offsets(&mut ed);
    assert_eq!(initial_atoms.len(), 2);
    let first_offset = initial_atoms[0].0;
    let second_offset = initial_atoms[1].0;

    click_atom(&mut ed, first_offset);
    assert!(ed.cx.debug_bounds("inline-math-popover").is_some());

    // Edit the first equation without pressing Enter, then click directly on the second equation.
    ed.cx.simulate_keystrokes("+ 1");
    ed.settle();

    click_atom(&mut ed, second_offset);

    // First atom edits must be committed while the popover is now open on the second atom.
    assert!(ed.cx.debug_bounds("inline-math-popover").is_some());
    assert_eq!(
        offsets(&mut ed)
            .into_iter()
            .map(|(_, tex)| tex)
            .collect::<Vec<_>>(),
        vec!["a^2+1".to_owned(), "b^2".to_owned()]
    );

    // Edit the second equation and press Enter to commit and close cleanly.
    ed.cx.simulate_keystrokes("+ 2 enter");
    ed.settle();
    assert!(ed.cx.debug_bounds("inline-math-popover").is_none());
    assert!(editor_is_focused(&mut ed));
    assert_eq!(
        offsets(&mut ed)
            .into_iter()
            .map(|(_, tex)| tex)
            .collect::<Vec<_>>(),
        vec!["a^2+1".to_owned(), "b^2+2".to_owned()]
    );

    // Clear the first equation and click the second equation directly: emptying the first
    // deletes it cleanly while keeping the popover open and anchored on the second atom.
    let updated_atoms = offsets(&mut ed);
    let first_offset = updated_atoms[0].0;
    let second_offset = updated_atoms[1].0;
    click_atom(&mut ed, first_offset);
    for _ in 0..6 {
        ed.keys("backspace");
    }
    ed.settle();

    click_atom(&mut ed, second_offset);

    assert!(ed.cx.debug_bounds("inline-math-popover").is_some());
    assert_eq!(
        offsets(&mut ed)
            .into_iter()
            .map(|(_, tex)| tex)
            .collect::<Vec<_>>(),
        vec!["b^2+2".to_owned()]
    );

    // Typing into the surviving second equation commits to that atom at its shifted offset.
    ed.cx.simulate_keystrokes("+ 3 enter");
    ed.settle();
    assert!(ed.cx.debug_bounds("inline-math-popover").is_none());
    assert_eq!(
        offsets(&mut ed)
            .into_iter()
            .map(|(_, tex)| tex)
            .collect::<Vec<_>>(),
        vec!["b^2+2+3".to_owned()]
    );
}

#[gpui::test]
fn inline_popover_blur_preserves_selection_focus_and_skips_noop_undo(cx: &mut TestAppContext) {
    let schema = extend_schema(Schema::builtin());
    let mut doc = Document::new(schema);
    let ids = doc
        .insert_markdown(None, 0, "See $a^2$ here.\n\n$$\nE = mc^2\n$$")
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
    ed.cx.update(|window, _| window.activate_window());
    ed.settle();

    let inline_tex_at = |ed: &mut EditorTestContext| -> Option<String> {
        ed.editor(|editor| {
            editor
                .document()
                .spans(para)
                .unwrap()
                .into_iter()
                .find_map(|s| s.atom.filter(|a| a.name == INLINE_TYPE))
                .map(|a| inline_tex(&a).to_owned())
        })
    };

    // 1. Confirming an unmodified popover skips `replace_atom` (no empty undo step or CRDT rewrite).
    assert!(!ed.editor(|editor| editor.can_undo()));
    let anchor_before =
        ed.editor(|editor| editor.document().anchor_char(Pos::new(para, 4)).unwrap());
    let inline_bounds = ed.cx.debug_bounds("inline-math").unwrap();
    ed.cx
        .simulate_click(inline_bounds.center(), Modifiers::none());
    ed.settle();
    assert!(ed.cx.debug_bounds("inline-math-popover").is_some());
    ed.keys("enter");
    ed.settle();
    assert!(ed.cx.debug_bounds("inline-math-popover").is_none());
    assert!(editor_is_focused(&mut ed));
    assert!(!ed.editor(|editor| editor.can_undo()));
    assert_eq!(
        ed.editor(|editor| editor.document().resolve_char(&anchor_before)),
        Some(Pos::new(para, 4))
    );
    assert_eq!(
        ed.editor(|editor| editor.selection().cloned()),
        Some(Selection::caret(Pos::new(para, 4 + OBJECT.len_utf8())))
    );

    // 2. Blur onto an external FocusHandle commits edits without stealing focus back to EditorView.
    let inline_bounds = ed.cx.debug_bounds("inline-math").unwrap();
    ed.cx
        .simulate_click(inline_bounds.center(), Modifiers::none());
    ed.settle();
    ed.cx.simulate_keystrokes("+ 1");
    ed.settle();

    let external = ed.cx.update(|_, cx| cx.focus_handle());
    ed.cx.update(|window, cx| window.focus(&external, cx));
    ed.settle();
    assert!(ed.cx.debug_bounds("inline-math-popover").is_none());
    assert_eq!(inline_tex_at(&mut ed).as_deref(), Some("a^2+1"));
    assert!(ed.editor(|editor| editor.can_undo()));
    assert!(ed.cx.update(|window, _| external.is_focused(window)));
    assert!(!editor_is_focused(&mut ed));

    // 3. Clicking a display `math` block while inline popover is open commits without kicking `math` out of edit mode.
    let inline_bounds = ed.cx.debug_bounds("inline-math").unwrap();
    ed.cx
        .simulate_click(inline_bounds.center(), Modifiers::none());
    ed.settle();
    ed.cx.simulate_keystrokes("+ 2");
    ed.settle();

    let block_bounds = ed.cx.debug_bounds("math-block").unwrap();
    ed.cx
        .simulate_click(block_bounds.center(), Modifiers::none());
    ed.settle();
    assert!(ed.cx.debug_bounds("inline-math-popover").is_none());
    assert_eq!(inline_tex_at(&mut ed).as_deref(), Some("a^2+1+2"));
    assert!(ed.cx.debug_bounds("math-block-editing").is_some());
    assert!(matches!(
        ed.editor(|editor| editor.selection().cloned()),
        Some(Selection::Text { head, .. }) if head.block == math_block
    ));

    // 4. Emptying the inline equation and clicking trailing prose preserves the adjusted caret offset.
    let inline_bounds = ed.cx.debug_bounds("inline-math").unwrap();
    ed.cx
        .simulate_click(inline_bounds.center(), Modifiers::none());
    ed.settle();
    for _ in 0..8 {
        ed.keys("backspace");
    }
    ed.settle();

    let end_bounds = ed
        .view
        .read_with(ed.cx, |view, _| {
            let len = view.editor().document().text_len(para).unwrap();
            view.range_bounds(para, len - 2..len - 1)
        })
        .expect("trailing prose bounds");
    ed.cx.simulate_click(end_bounds.center(), Modifiers::none());
    ed.settle();

    assert!(ed.cx.debug_bounds("inline-math-popover").is_none());
    assert_eq!(
        ed.editor(|editor| editor.document().plain_text(para).unwrap()),
        "See  here."
    );
    assert!(matches!(
        ed.editor(|editor| editor.selection().cloned()),
        Some(Selection::Text { head, .. }) if head.block == para && head.offset > 4
    ));
}
