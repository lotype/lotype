use gpui::{IntoElement, Modifiers, ParentElement, Styled, TestAppContext, div, point, px};
use lotype_core::{
    Atom, Document, Editor, InlineSpec, OBJECT, Pos, Schema, Selection, Value, blocks, p,
};
use lotype_ui::EditorView;
use lotype_ui::test_support::EditorTestContext;

fn head(ed: &mut EditorTestContext) -> Pos {
    ed.editor(|editor| match editor.selection() {
        Some(Selection::Text { head, .. }) => *head,
        _ => panic!("no text selection"),
    })
}

fn caret_top(ed: &mut EditorTestContext) -> gpui::Pixels {
    let head = head(ed);
    ed.view
        .update(ed.cx, |view, _| view.caret_bounds(head).unwrap().top())
}

#[gpui::test]
fn the_caret_can_stand_at_either_end_of_a_soft_wrap(cx: &mut TestAppContext) {
    // Far wider than the test window, so that it wraps.
    let words = "word ".repeat(150);
    let mut ed = EditorTestContext::new(cx, blocks![p!["<a>", words.as_str()]]);
    let first = caret_top(&mut ed);
    ed.keys("cmd-right");
    let wrap = head(&mut ed).offset;
    assert!(wrap > 0 && wrap < words.len());
    // The end of the first line is drawn there, and stays put.
    assert_eq!(caret_top(&mut ed), first);
    ed.keys("cmd-right");
    assert_eq!(head(&mut ed).offset, wrap);
    // The same place, at the start of the next line.
    ed.keys("down cmd-left");
    assert_eq!(head(&mut ed).offset, wrap);
    assert!(caret_top(&mut ed) > first);
    ed.keys("cmd-left");
    assert_eq!(head(&mut ed).offset, wrap);
    // A click at the start of the second line puts the caret there.
    let start = caret_top(&mut ed);
    ed.cx
        .simulate_click(point(px(1.), start + px(4.)), Modifiers::none());
    assert_eq!(head(&mut ed).offset, wrap);
    assert_eq!(caret_top(&mut ed), start);
}

fn mention(atom: &Atom) -> String {
    match atom.attrs.get("label") {
        Some(Value::Str(label)) => format!("@{label}"),
        _ => "@".into(),
    }
}

/// An editor of "a", a mention of Ann, and "b", with the caret at the end.
fn with_mention(tall: bool) -> impl FnOnce(&mut gpui::Context<EditorView>) -> EditorView {
    move |cx| {
        let schema = Schema::builtin().inline(InlineSpec::new("mention", mention));
        let mut doc = Document::new(schema);
        let block = doc.insert_block(None, 0, "paragraph").unwrap();
        doc.insert_text(block, 0, "ab").unwrap();
        let mut editor = Editor::new(doc);
        let caret = Selection::caret(Pos::new(block, 1));
        editor.set_selection(Some(caret)).unwrap();
        editor
            .insert_atom(Atom::new("mention").attr("label", "Ann"))
            .unwrap();
        let view = EditorView::new(editor, cx);
        if !tall {
            return view;
        }
        view.with_atom_view("mention", |_, _, _| {
            div().w(px(50.)).h(px(80.)).child("x").into_any_element()
        })
    }
}

#[gpui::test]
fn an_atom_takes_its_own_width_and_one_step(cx: &mut TestAppContext) {
    let mut ed = EditorTestContext::with_view(cx, with_mention(false));
    let end = head(&mut ed);
    assert_eq!(end.offset, 1 + OBJECT.len_utf8());
    let after = ed
        .view
        .update(ed.cx, |view, _| view.caret_bounds(end).unwrap());
    ed.keys("left");
    let before = head(&mut ed);
    assert_eq!(before.offset, 1);
    let before = ed
        .view
        .update(ed.cx, |view, _| view.caret_bounds(before).unwrap());
    // As wide as the chip with `@Ann` in it, more than one character.
    assert!(after.left() - before.left() > px(30.));
    // A click on the right half of the atom lands after it.
    let right = point(after.left() - px(2.), after.top() + px(4.));
    ed.cx.simulate_click(right, Modifiers::none());
    assert_eq!(head(&mut ed), end);
}

#[gpui::test]
fn a_tall_atom_makes_its_line_taller_and_text_sits_on_its_baseline(cx: &mut TestAppContext) {
    let mut ed = EditorTestContext::with_view(cx, with_mention(true));
    let block = head(&mut ed).block;
    ed.view.update(ed.cx, |view, _| {
        let text = view.text_bounds(block).unwrap();
        assert!(text.size.height >= px(80.));
        // Text is pushed down to the atom's middle.
        let caret = view.caret_bounds(Pos::new(block, 0)).unwrap();
        let middle = caret.top() + caret.size.height / 2.;
        assert!(
            (middle - (text.top() + px(40.))).abs() < px(1.),
            "{middle:?}"
        );
    });
}

#[gpui::test]
fn a_long_paragraph_lays_out_in_time_and_within_the_window(cx: &mut TestAppContext) {
    // Words, then a single word longer than a line, then hanging space.
    let text = format!(
        "{}{} {}b",
        "abc ".repeat(16_000),
        "a".repeat(4_000),
        " ".repeat(3_000)
    );
    let started = std::time::Instant::now();
    let mut ed = EditorTestContext::new(cx, blocks![p!["<a>", text.as_str()]]);
    // Several seconds when wrapping was quadratic, in a debug build.
    assert!(
        started.elapsed() < std::time::Duration::from_secs(2),
        "{:?}",
        started.elapsed()
    );
    let block = head(&mut ed).block;
    let window = ed.cx.update(|window, _| window.viewport_size().width);
    ed.view.update(ed.cx, |view, _| {
        let bounds = view.text_bounds(block).unwrap();
        assert!(bounds.size.width <= window, "{:?}", bounds.size.width);
        let end = view.caret_bounds(Pos::new(block, text.len() - 1)).unwrap();
        assert!(end.left() <= window);
    });
}

#[gpui::test]
fn no_line_starts_inside_a_character_cluster(cx: &mut TestAppContext) {
    let text = "葛\u{E0100}".repeat(400);
    let mut ed = EditorTestContext::new(cx, blocks![p!["<a>", text.as_str()]]);
    // Each Cmd-Right goes to a line's end; every one is between clusters.
    for _ in 0..5 {
        ed.keys("cmd-right right");
        let at = head(&mut ed).offset;
        assert_eq!(at % "葛\u{E0100}".len(), 0, "{at}");
    }
}

#[gpui::test]
fn unbroken_text_lays_out_in_time(cx: &mut TestAppContext) {
    // A long address or base64, and flags, which pair regional indicators.
    let text = format!("{}{}", "a".repeat(80_000), "🇦🇧".repeat(2_000));
    let started = std::time::Instant::now();
    let mut ed = EditorTestContext::new(cx, blocks![p!["<a>", text.as_str()]]);
    // Over half a minute while each line scanned to the end of the text.
    assert!(
        started.elapsed() < std::time::Duration::from_secs(3),
        "{:?}",
        started.elapsed()
    );
    // Every line starts between flags, not inside one.
    for _ in 0..3 {
        ed.keys("cmd-down cmd-left");
        assert_eq!(head(&mut ed).offset % "🇦".len(), 0);
    }
}

#[gpui::test]
fn no_empty_line_follows_a_last_atom_wider_than_the_line(cx: &mut TestAppContext) {
    // "ab " and then an atom far wider than the window, at the very end.
    let build = |cx: &mut gpui::Context<EditorView>| {
        let schema = Schema::builtin().inline(InlineSpec::new("mention", mention));
        let mut doc = Document::new(schema);
        let block = doc.insert_block(None, 0, "paragraph").unwrap();
        doc.insert_text(block, 0, "ab ").unwrap();
        let mut editor = Editor::new(doc);
        let caret = Selection::caret(Pos::new(block, 3));
        editor.set_selection(Some(caret)).unwrap();
        editor.insert_atom(Atom::new("mention")).unwrap();
        let view = EditorView::new(editor, cx);
        view.with_atom_view("mention", |_, _, _| {
            div().w(px(5000.)).h(px(20.)).child("x").into_any_element()
        })
    };
    let mut ed = EditorTestContext::with_view(cx, build);
    // The caret after the atom is on the atom's line, not on an empty one
    // under it.
    let after = caret_top(&mut ed);
    ed.keys("left");
    assert_eq!(caret_top(&mut ed), after);
}

#[gpui::test]
fn byte_offset_at_hits_enclosing_glyph_and_utf8_boundaries(cx: &mut TestAppContext) {
    let text = "foo.日本語\ne\u{0301}x";
    let mut ed = EditorTestContext::new(cx, blocks![p!["<a>", text]]);
    let block = head(&mut ed).block;
    ed.view.update(ed.cx, |view, _| {
        // Just inside the right edge of the second 'o' in "foo." must stay on 'o' (byte 2),
        // not snap forward to '.' (byte 3).
        let dot = view.caret_bounds(Pos::new(block, 3)).unwrap();
        let mid_y = dot.top() + dot.size.height / 2.;
        assert_eq!(
            view.byte_offset_at(block, point(dot.left() - px(1.), mid_y)),
            Some(2)
        );
        assert_eq!(
            view.byte_offset_at(block, point(dot.left() + px(1.), mid_y)),
            Some(3)
        );

        // Right half of the final multi-byte UTF-8 character ('語', bytes 10..13)
        // must return its UTF-8 start byte (10), never a mid-character byte (12).
        let line1_end_byte = "foo.日本語".len();
        let last_char_start = "foo.日本".len();
        let line1_end = view.caret_bounds(Pos::new(block, line1_end_byte)).unwrap();
        let last_hit = view.byte_offset_at(block, point(line1_end.left() - px(1.), mid_y));
        assert_eq!(last_hit, Some(last_char_start));
        assert!(text.is_char_boundary(last_hit.unwrap()));
        assert_eq!(
            view.byte_offset_at(block, point(line1_end.left() + px(2.), mid_y)),
            None
        );

        // Combining mark cluster ("e\u{0301}") normalizes to the cluster start across its width.
        let line2_start = line1_end_byte + 1;
        let x_offset = line2_start + "e\u{0301}".len();
        let x_caret = view.caret_bounds(Pos::new(block, x_offset)).unwrap();
        let line2_y = x_caret.top() + x_caret.size.height / 2.;
        let cluster_hit = view.byte_offset_at(block, point(x_caret.left() - px(1.), line2_y));
        assert_eq!(cluster_hit, Some(line2_start));
        assert!(text.is_char_boundary(cluster_hit.unwrap()));
    });

    // Right half of an inline atom at the end of a line returns the start byte
    // of U+FFFC (1), not a mid-UTF-8 byte (3).
    let build = |cx: &mut gpui::Context<EditorView>| {
        let schema = Schema::builtin().inline(InlineSpec::new("mention", mention));
        let mut doc = Document::new(schema);
        let block = doc.insert_block(None, 0, "paragraph").unwrap();
        doc.insert_text(block, 0, "a").unwrap();
        let mut editor = Editor::new(doc);
        editor
            .set_selection(Some(Selection::caret(Pos::new(block, 1))))
            .unwrap();
        editor
            .insert_atom(Atom::new("mention").attr("label", "Ann"))
            .unwrap();
        EditorView::new(editor, cx)
    };
    let mut atom_ed = EditorTestContext::with_view(cx, build);
    let end = head(&mut atom_ed);
    assert_eq!(end.offset, 1 + OBJECT.len_utf8());
    atom_ed.view.update(atom_ed.cx, |view, _| {
        let after = view.caret_bounds(end).unwrap();
        let mid_y = after.top() + after.size.height / 2.;
        assert_eq!(
            view.byte_offset_at(end.block, point(after.left() - px(1.), mid_y)),
            Some(1)
        );
        assert_eq!(
            view.byte_offset_at(end.block, point(after.left() + px(2.), mid_y)),
            None
        );
    });
}

#[gpui::test]
fn text_decoration_with_mid_utf8_offsets_does_not_panic(cx: &mut TestAppContext) {
    use gpui::{HighlightStyle, Hsla, rgb};
    use lotype_core::block;
    use lotype_ui::TextDecoration;

    let build = |cx: &mut gpui::Context<EditorView>| {
        let mut doc = Document::new(Schema::builtin());
        let p = doc.insert_block(None, 0, block::PARAGRAPH).unwrap();
        doc.insert_text(p, 0, "日本語\nabc").unwrap();
        let mut editor = Editor::new(doc);
        editor
            .set_selection(Some(Selection::caret(Pos::new(p, 0))))
            .unwrap();
        EditorView::new(editor, cx).with_text_decorator(block::PARAGRAPH, |_, _, _, _, _, _| {
            TextDecoration {
                highlights: vec![(
                    1..5,
                    HighlightStyle {
                        color: Some(Hsla::from(rgb(0x006d9a))),
                        ..Default::default()
                    },
                )],
                trimmed_lines: vec![(0, 2)],
                ..TextDecoration::default()
            }
        })
    };
    let mut ed = EditorTestContext::with_view(cx, build);
    ed.settle();
    let block = head(&mut ed).block;
    assert!(
        ed.view
            .update(ed.cx, |view, _| view.text_bounds(block))
            .is_some()
    );
}
