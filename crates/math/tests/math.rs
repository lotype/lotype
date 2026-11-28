use lotype_core::{Document, Editor, OBJECT, Pos, Schema, Selection, block};
use lotype_math::{
    BLOCK_TYPE, INLINE_TYPE, MathOptions, allows_selection_toolbar, block_tex, extend_schema,
    inline_atom, inline_tex, is_equation_selection, slash_item, to_svg, to_svg_display, typeset,
    typeset_with, validate,
};

#[test]
fn typesets_inline_display_tags_and_custom_macros() {
    let inline = typeset(r"E = mc^2", false).unwrap();
    assert!(inline.width > 0. && inline.height > 0.);
    assert!(!inline.items.is_empty());

    let display = typeset(
        r"\int_{-\infty}^{\infty} e^{-x^2}\,dx = \sqrt{\pi} \tag{1}",
        true,
    )
    .unwrap();
    assert!(display.width > 0.);
    assert!(display.tag.is_some());

    let opts = MathOptions::new().with_macro(r"\R", r"\mathbb{R}");
    let custom = typeset_with(r"x \in \R^n", false, &opts).unwrap();
    assert!(custom.width > 0.);

    let err = validate(r"\frac{1}", false).unwrap_err();
    assert!(!err.message.is_empty());

    let svg = to_svg(r"\frac{a}{b}", false, 16., "#000").unwrap();
    assert!(svg.starts_with("<svg ") && svg.ends_with("</svg>"));

    let display_svg = to_svg_display(r"x^2 \tag{2}", 16., "#000", 20.).unwrap();
    assert!(display_svg.starts_with("<svg ") && display_svg.ends_with("</svg>"));
}

#[test]
fn markdown_round_trips_inline_and_block_equations() {
    let schema = extend_schema(Schema::builtin());
    let md = "\
Euler's identity $e^{i\\pi} + 1 = 0$ holds.\n\
\n\
$$\n\
\\int_0^1 x^2\\,dx = \\frac{1}{3}\n\
$$\n\
\n\
```math\n\
a^2 + b^2 = c^2\n\
```\n";
    let mut doc = Document::new(schema.clone());
    let ids = doc.insert_markdown(None, 0, md).unwrap();
    assert_eq!(ids.len(), 3);

    // First block: paragraph containing inline_math atom surrounded by text.
    let spans = doc.spans(ids[0]).unwrap();
    let atom = spans
        .iter()
        .find_map(|span| span.atom.as_ref())
        .expect("paragraph has an inline_math atom");
    assert_eq!(atom.name, INLINE_TYPE);
    assert_eq!(inline_tex(atom), r"e^{i\pi} + 1 = 0");
    assert_eq!(
        doc.plain_text(ids[0]).unwrap(),
        r"Euler's identity $e^{i\pi} + 1 = 0$ holds."
    );

    // Second and third blocks: display math blocks from $$...$$ and ```math.
    assert_eq!(doc.block_type(ids[1]).unwrap(), BLOCK_TYPE);
    assert_eq!(
        block_tex(&doc, ids[1]).as_deref(),
        Some(r"\int_0^1 x^2\,dx = \frac{1}{3}")
    );
    assert_eq!(doc.block_type(ids[2]).unwrap(), BLOCK_TYPE);
    assert_eq!(block_tex(&doc, ids[2]).as_deref(), Some("a^2 + b^2 = c^2"));

    // Round-trip through to_markdown and back.
    let written = doc.to_markdown();
    assert!(written.contains(r"$e^{i\pi} + 1 = 0$"), "{written}");
    assert!(
        written.contains("$$\n\\int_0^1 x^2\\,dx = \\frac{1}{3}\n$$"),
        "{written}"
    );
    let mut reread = Document::new(schema);
    let ids2 = reread.insert_markdown(None, 0, &written).unwrap();
    assert_eq!(ids2.len(), 3);
    assert_eq!(block_tex(&reread, ids2[1]), block_tex(&doc, ids[1]));
    assert_eq!(block_tex(&reread, ids2[2]), block_tex(&doc, ids[2]));
}

#[test]
fn input_rules_create_inline_and_block_equations_and_ignore_currency() {
    let schema = extend_schema(Schema::builtin());
    let mut doc = Document::new(schema);
    let p = doc.insert_block(None, 0, block::PARAGRAPH).unwrap();
    let mut editor = Editor::new(doc);
    editor
        .set_selection(Some(Selection::caret(Pos::new(p, 0))))
        .unwrap();

    // Currency amounts must stay plain text.
    for ch in "Cost is $5 and $10. ".chars() {
        editor.insert_text(&ch.to_string()).unwrap();
    }
    assert!(
        editor
            .document()
            .spans(p)
            .unwrap()
            .iter()
            .all(|span| span.atom.is_none())
    );

    // Inline math rule: `$x^2$` becomes an inline_math atom.
    for ch in "$x^2$".chars() {
        editor.insert_text(&ch.to_string()).unwrap();
    }
    let spans = editor.document().spans(p).unwrap();
    let atom_span = spans
        .iter()
        .find(|span| span.atom.is_some())
        .expect("inline math atom created");
    assert_eq!(inline_tex(atom_span.atom.as_ref().unwrap()), "x^2");

    // Backspace right after the input rule reverts it back to `$x^2$`.
    editor.delete_backward().unwrap();
    assert_eq!(
        editor.document().plain_text(p).unwrap(),
        "Cost is $5 and $10. $x^2$"
    );

    // Block math input rule: `$$ ` on a new paragraph turns into a `math` block.
    let p2 = editor.document().blocks().len();
    assert_eq!(p2, 1);
    editor.split_block().unwrap();
    let second = editor.document().blocks()[1];
    for ch in "$$ ".chars() {
        editor.insert_text(&ch.to_string()).unwrap();
    }
    assert_eq!(editor.document().block_type(second).unwrap(), BLOCK_TYPE);
    assert_eq!(editor.document().plain_text(second).unwrap(), "");
}

#[test]
fn replace_and_remove_atom_preserve_marks_and_undo() {
    let schema = extend_schema(Schema::builtin());
    let mut doc = Document::new(schema);
    let p = doc.insert_block(None, 0, block::PARAGRAPH).unwrap();
    let mut editor = Editor::new(doc);
    editor
        .set_selection(Some(Selection::caret(Pos::new(p, 0))))
        .unwrap();
    editor.insert_text("a ").unwrap();
    editor.toggle_mark("bold").unwrap();
    editor.insert_atom(inline_atom("x")).unwrap();
    editor.toggle_mark("bold").unwrap();
    editor.insert_text(" b").unwrap();

    let atom_pos = Pos::new(p, 2);
    assert_eq!(
        &editor.document().text(p).unwrap()[2..2 + OBJECT.len_utf8()],
        OBJECT.to_string().as_str()
    );

    editor
        .replace_atom(atom_pos, inline_atom(r"\alpha + \beta"))
        .unwrap();
    let spans = editor.document().spans(p).unwrap();
    let replaced = spans.iter().find(|s| s.atom.is_some()).unwrap();
    assert_eq!(
        inline_tex(replaced.atom.as_ref().unwrap()),
        r"\alpha + \beta"
    );
    assert!(replaced.marks.contains_key("bold"));

    editor.undo().unwrap();
    let spans = editor.document().spans(p).unwrap();
    let restored = spans.iter().find(|s| s.atom.is_some()).unwrap();
    assert_eq!(inline_tex(restored.atom.as_ref().unwrap()), "x");

    editor.remove_atom(atom_pos).unwrap();
    assert_eq!(editor.document().plain_text(p).unwrap(), "a  b");
}

#[test]
fn slash_item_inserts_math_block() {
    let schema = extend_schema(Schema::builtin());
    let mut doc = Document::new(schema);
    let p = doc.insert_block(None, 0, block::PARAGRAPH).unwrap();
    let mut editor = Editor::new(doc);
    editor
        .set_selection(Some(Selection::caret(Pos::new(p, 0))))
        .unwrap();
    editor.insert_text("/math").unwrap();
    editor.accept_slash_item(&slash_item()).unwrap();
    assert_eq!(editor.document().block_type(p).unwrap(), BLOCK_TYPE);
}

#[test]
fn equation_selection_helper_disables_toolbar_for_block_and_inline_math() {
    let schema = extend_schema(Schema::builtin());
    let mut doc = Document::new(schema);
    let ids = doc
        .insert_markdown(None, 0, "Text $x^2$ end.\n\n$$\nE = mc^2\n$$")
        .unwrap();
    let para = ids[0];
    let math_block = ids[1];

    // Regular text selection allows toolbar.
    assert!(!is_equation_selection(
        &doc,
        Pos::new(para, 0),
        Pos::new(para, 4)
    ));
    assert!(allows_selection_toolbar(
        &doc,
        Pos::new(para, 0),
        Pos::new(para, 4)
    ));

    // Inline math atom selection alone suppresses toolbar.
    let atom_start = "Text ".len();
    let atom_end = atom_start + OBJECT.len_utf8();
    assert!(is_equation_selection(
        &doc,
        Pos::new(para, atom_start),
        Pos::new(para, atom_end)
    ));
    assert!(!allows_selection_toolbar(
        &doc,
        Pos::new(para, atom_start),
        Pos::new(para, atom_end)
    ));

    // Selection mixing prose and inline math keeps toolbar available.
    assert!(allows_selection_toolbar(
        &doc,
        Pos::new(para, 0),
        Pos::new(para, atom_end)
    ));

    // Selection inside a block equation suppresses toolbar.
    assert!(is_equation_selection(
        &doc,
        Pos::new(math_block, 0),
        Pos::new(math_block, 4)
    ));
    assert!(!allows_selection_toolbar(
        &doc,
        Pos::new(math_block, 0),
        Pos::new(math_block, 4)
    ));

    // Selecting a paragraph down to offset 0 of the next math block (e.g. Shift+Down)
    // selects only prose, so the formatting toolbar must remain available.
    assert!(!is_equation_selection(
        &doc,
        Pos::new(para, 0),
        Pos::new(math_block, 0)
    ));
    assert!(allows_selection_toolbar(
        &doc,
        Pos::new(para, 0),
        Pos::new(math_block, 0)
    ));
}

#[test]
fn equation_selection_handles_multiblock_ranges_consistently() {
    let schema = extend_schema(Schema::builtin());
    let mut doc = Document::new(schema);
    let ids = doc
        .insert_markdown(None, 0, "Before\n\n$$\na = 1\n$$\n\n$$\nb = 2\n$$\n\nAfter")
        .unwrap();
    let before = ids[0];
    let math_a = ids[1];
    let math_b = ids[2];
    let after = ids[3];

    // Spanning prose -> math -> prose keeps toolbar available for the prose.
    assert!(allows_selection_toolbar(
        &doc,
        Pos::new(before, 0),
        Pos::new(after, 3)
    ));
    // Spanning prose into a math block also contains prose, so keep toolbar.
    assert!(allows_selection_toolbar(
        &doc,
        Pos::new(before, 0),
        Pos::new(math_a, 3)
    ));
    // Spanning exclusively across two consecutive math blocks hides toolbar.
    assert!(is_equation_selection(
        &doc,
        Pos::new(math_a, 0),
        Pos::new(math_b, 3)
    ));
    assert!(!allows_selection_toolbar(
        &doc,
        Pos::new(math_a, 0),
        Pos::new(math_b, 3)
    ));
}

#[test]
fn char_anchor_tracks_inline_atom_across_remote_edits_and_detects_deletion() {
    let schema = extend_schema(Schema::builtin());
    let mut doc_a = Document::new(schema.clone());
    let ids = doc_a.insert_markdown(None, 0, "$a$ $b$").unwrap();
    let para = ids[0];
    let b_offset = OBJECT.len_utf8() + 1;

    // Peer B starts from a snapshot of Peer A.
    let mut doc_b = Document::new(schema);
    doc_b.import(&doc_a.export_snapshot().unwrap()).unwrap();
    let since = doc_b.version();

    // Anchor `$b$` on Peer A before remote edits arrive.
    let anchor = doc_a.anchor_char(Pos::new(para, b_offset)).unwrap();

    // Peer B prepends 4 ASCII bytes ("See "), shifting `$a$` into `$b$`'s old offset!
    doc_b.insert_text(para, 0, "See ").unwrap();
    let update = doc_b.export_updates(&since).unwrap();
    doc_a.import(&update).unwrap();

    let resolved = doc_a
        .resolve_char(&anchor)
        .expect("anchor still resolves to shifted `$b$`");
    assert_eq!(resolved, Pos::new(para, b_offset + 4));
    let spans = doc_a
        .spans_in(para, resolved.offset..resolved.offset + OBJECT.len_utf8())
        .unwrap();
    assert_eq!(
        inline_tex(spans[0].atom.as_ref().expect("inline_math atom")),
        "b"
    );

    // Once Peer B deletes `$b$`, the anchor resolves to `None` rather than matching `$a$`.
    let since_del = doc_b.version();
    doc_b
        .delete_text(para, resolved.offset..resolved.offset + OBJECT.len_utf8())
        .unwrap();
    doc_a
        .import(&doc_b.export_updates(&since_del).unwrap())
        .unwrap();
    assert!(doc_a.resolve_char(&anchor).is_none());
}

#[test]
fn inline_math_input_rule_preserves_marks_and_skips_code_spans() {
    let schema = extend_schema(Schema::builtin());
    let mut doc = Document::new(schema);
    let p1 = doc.insert_block(None, 0, block::PARAGRAPH).unwrap();
    let p2 = doc.insert_block(None, 1, block::PARAGRAPH).unwrap();
    let p3 = doc.insert_block(None, 2, block::PARAGRAPH).unwrap();
    let mut editor = Editor::new(doc);

    // Bold toggled on at offset 0 must survive `$x^2$` input-rule conversion.
    editor
        .set_selection(Some(Selection::caret(Pos::new(p1, 0))))
        .unwrap();
    editor.toggle_mark("bold").unwrap();
    for ch in "$x^2$".chars() {
        editor.insert_text(&ch.to_string()).unwrap();
    }
    let spans1 = editor.document().spans(p1).unwrap();
    assert_eq!(spans1.len(), 1);
    assert_eq!(inline_tex(spans1[0].atom.as_ref().unwrap()), "x^2");
    assert!(spans1[0].marks.contains_key("bold"));

    // Bold toggled on after unmarked prose must also survive `$y$`.
    editor
        .set_selection(Some(Selection::caret(Pos::new(p2, 0))))
        .unwrap();
    editor.insert_text("plain ").unwrap();
    editor.toggle_mark("bold").unwrap();
    for ch in "$y$".chars() {
        editor.insert_text(&ch.to_string()).unwrap();
    }
    let atom2 = editor
        .document()
        .spans(p2)
        .unwrap()
        .into_iter()
        .find(|span| span.atom.is_some())
        .expect("inline math atom");
    assert_eq!(inline_tex(atom2.atom.as_ref().unwrap()), "y");
    assert!(atom2.marks.contains_key("bold"));

    // A trailing `$` after an inline code span containing `$` must not swallow the code span.
    editor
        .set_selection(Some(Selection::caret(Pos::new(p3, 0))))
        .unwrap();
    for ch in "See `$foo` bar$".chars() {
        editor.insert_text(&ch.to_string()).unwrap();
    }
    let spans3 = editor.document().spans(p3).unwrap();
    assert!(spans3.iter().all(|span| span.atom.is_none()));
    assert!(
        spans3
            .iter()
            .any(|span| span.text == "$foo" && span.marks.contains_key("code")),
        "expected inline code span `$foo` to remain intact: {spans3:?}"
    );
}

#[test]
fn markdown_round_trips_math_in_fences_headings_tables_and_unsafe_tex() {
    use lotype_core::{BlockSpec, Content, Error};

    let schema = extend_schema(Schema::builtin())
        .block(BlockSpec::new("callout", Content::Inline).written_as_fence("callout"));

    // 1. Rich-text fenced block preserves inline math atoms on round-trip.
    let mut doc = Document::new(schema.clone());
    let callout = doc.insert_block(None, 0, "callout").unwrap();
    let mut editor = Editor::new(doc);
    editor
        .set_selection(Some(Selection::caret(Pos::new(callout, 0))))
        .unwrap();
    editor.insert_text("Check ").unwrap();
    editor.insert_atom(inline_atom("x^2")).unwrap();
    let md = editor.document().to_markdown();
    let mut reread = Document::new(schema.clone());
    let ids = reread.insert_markdown(None, 0, &md).unwrap();
    assert_eq!(reread.block_type(ids[0]).unwrap(), "callout");
    let fence_atom = reread
        .spans(ids[0])
        .unwrap()
        .into_iter()
        .find_map(|s| s.atom)
        .expect("inline math atom preserved inside rich-text fence");
    assert_eq!(inline_tex(&fence_atom), "x^2");

    // 2. Display math delimiters inside a heading stay inline inside the single heading block.
    let mut heading_doc = Document::new(schema.clone());
    let h_ids = heading_doc
        .insert_markdown(
            None,
            0,
            "# Theorem $$a^2 + b^2 = c^2$$ end\n\n# $$E = mc^2$$",
        )
        .unwrap();
    assert_eq!(h_ids.len(), 2);
    assert_eq!(heading_doc.block_type(h_ids[0]).unwrap(), block::HEADING);
    assert_eq!(
        heading_doc.plain_text(h_ids[0]).unwrap(),
        "Theorem $a^2 + b^2 = c^2$ end"
    );
    assert_eq!(heading_doc.block_type(h_ids[1]).unwrap(), block::HEADING);
    assert_eq!(heading_doc.plain_text(h_ids[1]).unwrap(), "$E = mc^2$");

    // 3. Inline math with `\|` norm inside a table cell must not split the table columns.
    let mut table_doc = Document::new(schema.clone());
    let table = table_doc.insert_table(None, 0, 2, 2).unwrap();
    let rows = table_doc.children(Some(table)).unwrap();
    let cols = table_doc.columns(table).unwrap();
    let cell = table_doc.cell(rows[0], cols[0]).unwrap();
    let mut t_ed = Editor::new(table_doc);
    t_ed.set_selection(Some(Selection::caret(Pos::new(cell, 0))))
        .unwrap();
    t_ed.insert_atom(inline_atom(r"\|x\|")).unwrap();
    let table_md = t_ed.document().to_markdown();
    assert!(
        !table_md.contains(r"\\|"),
        "must not turn `\\|` into unescaped `\\\\|` in table: {table_md}"
    );
    let mut table_reread = Document::new(schema.clone());
    let t_ids = table_reread.insert_markdown(None, 0, &table_md).unwrap();
    assert_eq!(table_reread.block_type(t_ids[0]).unwrap(), block::TABLE);
    assert_eq!(table_reread.columns(t_ids[0]).unwrap().len(), 2);

    // 4. Delimiter-unsafe inline math degrades cleanly without swallowing following prose.
    let mut unsafe_doc = Document::new(schema.clone());
    let p = unsafe_doc.insert_block(None, 0, block::PARAGRAPH).unwrap();
    let mut u_ed = Editor::new(unsafe_doc);
    u_ed.set_selection(Some(Selection::caret(Pos::new(p, 0))))
        .unwrap();
    for bad in [r"a \", " x ", r"\frac{1", "a $ b"] {
        u_ed.insert_atom(inline_atom(bad)).unwrap();
        u_ed.insert_text(" tail ").unwrap();
    }
    let unsafe_md = u_ed.document().to_markdown();
    let mut unsafe_reread = Document::new(schema);
    let u_ids = unsafe_reread.insert_markdown(None, 0, &unsafe_md).unwrap();
    let reread_text = unsafe_reread.plain_text(u_ids[0]).unwrap();
    assert_eq!(reread_text.matches("tail").count(), 4, "{reread_text}");

    // 5. Public `replace_atom` and `remove_atom` reject `usize::MAX` without overflowing.
    assert!(matches!(
        u_ed.replace_atom(Pos::new(p, usize::MAX), inline_atom("z")),
        Err(Error::OutOfBounds { .. })
    ));
    assert!(matches!(
        u_ed.remove_atom(Pos::new(p, usize::MAX)),
        Err(Error::OutOfBounds { .. })
    ));
}
