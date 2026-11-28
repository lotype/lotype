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
}
