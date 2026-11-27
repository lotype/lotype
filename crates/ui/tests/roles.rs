use gpui::{Modifiers, TestAppContext, point, px};
use lotype_core::test_support::outline;
use lotype_core::{
    Atom, BlockSpec, Command, Content, Document, Editor, InlineSpec, KeyBinding, OBJECT, Pos, Role,
    Schema, Selection, block,
};
use lotype_ui::EditorView;
use lotype_ui::test_support::EditorTestContext;

#[gpui::test]
fn a_toggle_of_a_schema_of_its_own_opens_closes_and_writes_in_it(cx: &mut TestAppContext) {
    let build = |cx: &mut gpui::Context<EditorView>| {
        let text = |name: &str, role| BlockSpec::new(name, Content::Inline).with_role(role);
        let schema = Schema::new()
            .with_text_block("line")
            .block(text("line", Role::Text).with_children())
            .block(text("fold", Role::Toggle).with_children());
        let mut doc = Document::new(schema);
        let fold = doc.insert_block(None, 0, "fold").unwrap();
        doc.insert_text(fold, 0, "title").unwrap();
        let mut editor = Editor::new(doc);
        let caret = Selection::caret(Pos::new(fold, 5));
        editor.set_selection(Some(caret)).unwrap();
        EditorView::new(editor, cx)
    };
    let mut ed = EditorTestContext::with_view(cx, build);
    ed.keys("enter");
    ed.type_text("inside");
    let shown = |ed: &mut EditorTestContext| ed.editor(|e| outline(e.document(), e.selection()));
    assert_eq!(shown(&mut ed), "fold \"title\"\n  line \"inside<a>\"\n");
    // Its arrow closes it, which hides what is in it.
    let fold = ed.editor(|e| e.document().blocks()[0]);
    let inside = ed.editor(|e| e.document().blocks()[1]);
    let arrow = ed.view.update(ed.cx, |view, _| {
        let text = view.text_bounds(fold).unwrap();
        point(text.left() - px(12.), text.top() + px(11.))
    });
    ed.cx.simulate_click(arrow, Modifiers::none());
    ed.settle();
    assert!(
        ed.view
            .update(ed.cx, |view, _| view.text_bounds(inside))
            .is_none()
    );
}

#[gpui::test]
fn a_block_of_an_applications_own_type_is_drawn_by_its_view(cx: &mut TestAppContext) {
    use gpui::{InteractiveElement, IntoElement, ParentElement, Styled, div};
    use lotype_ui::BlockContent;
    let build = |cx: &mut gpui::Context<EditorView>| {
        let schema = Schema::builtin()
            .block(BlockSpec::new("callout", Content::Inline))
            .block(BlockSpec::new("embed", Content::None));
        let mut doc = Document::new(schema);
        let callout = doc.insert_block(None, 0, "callout").unwrap();
        doc.insert_text(callout, 0, "hi").unwrap();
        let embed = doc.insert_block(None, 1, "embed").unwrap();
        doc.set_attr(embed, "src", "https://x.y").unwrap();
        let mut editor = Editor::new(doc);
        editor
            .set_selection(Some(Selection::caret(Pos::new(callout, 2))))
            .unwrap();
        EditorView::new(editor, cx)
            .with_block_view("callout", |content: BlockContent, _, _| {
                let text = content.text.expect("a callout holds text");
                div()
                    .debug_selector(|| "callout-view".to_owned())
                    .flex()
                    .child("💡")
                    .child(text)
                    .into_any_element()
            })
            .with_block_view("embed", |content: BlockContent, _, _| {
                assert!(content.text.is_none());
                let src = content.document.text_of(content.block, "src");
                div()
                    .debug_selector(|| "embed-view".to_owned())
                    .child(src)
                    .into_any_element()
            })
    };
    let mut ed = EditorTestContext::with_view(cx, build);
    ed.settle();
    assert!(ed.cx.debug_bounds("callout-view").is_some());
    assert!(ed.cx.debug_bounds("embed-view").is_some());
    // The text in it is the editor's: typed into, with the caret in it.
    ed.type_text("!");
    let shown = ed.editor(|e| outline(e.document(), e.selection()));
    assert!(shown.starts_with("callout \"hi!<a>\""), "{shown}");
}

#[gpui::test]
fn code_block_enter_handles_atoms_cross_block_selections_menus_and_keymap(cx: &mut TestAppContext) {
    // 1. Inline atom whose plain_text ("@") is shorter than U+FFFC (3 bytes):
    //    Enter must slice Document::text without panicking, and still exit on
    //    a trailing empty line.
    let build_with_atom = |cx: &mut gpui::Context<EditorView>| {
        let schema = Schema::builtin().inline(InlineSpec::new("mention", |_| "@".to_owned()));
        let mut doc = Document::new(schema);
        let code = doc.insert_block(None, 0, block::CODE_BLOCK).unwrap();
        doc.insert_text(code, 0, "ab").unwrap();
        let mut editor = Editor::new(doc);
        editor
            .set_selection(Some(Selection::caret(Pos::new(code, 1))))
            .unwrap();
        editor.insert_atom(Atom::new("mention")).unwrap();
        EditorView::new(editor, cx)
    };
    let mut ed = EditorTestContext::with_view(cx, build_with_atom);
    let code = ed.editor(|e| e.document().blocks()[0]);
    // Caret is at byte 4 ("a\u{FFFC}|b"), past plain_text ("a@b").len() == 3.
    ed.keys("enter");
    assert_eq!(
        ed.editor(|e| e.document().text(code).unwrap()),
        format!("a{OBJECT}\nb")
    );
    let end = ed.editor(|e| e.document().text_len(code).unwrap());
    ed.view.update(ed.cx, |view, cx| {
        let _ = view.update_editor(cx, |editor| {
            editor.set_selection(Some(Selection::caret(Pos::new(code, end))))
        });
    });
    ed.keys("enter");
    assert_eq!(ed.editor(|e| e.document().blocks().len()), 1);
    ed.keys("enter");
    assert_eq!(ed.editor(|e| e.document().blocks().len()), 2);

    // 2. Cross-block selection starting in a paragraph and ending in a code
    //    block splits the surviving paragraph instead of inserting '\n' in it.
    let build_cross = |cx: &mut gpui::Context<EditorView>| {
        let mut doc = Document::new(Schema::builtin());
        let para = doc.insert_block(None, 0, block::PARAGRAPH).unwrap();
        doc.insert_text(para, 0, "abcd").unwrap();
        let code = doc.insert_block(None, 1, block::CODE_BLOCK).unwrap();
        doc.insert_text(code, 0, "efgh").unwrap();
        let mut editor = Editor::new(doc);
        editor
            .set_selection(Some(Selection::Text {
                anchor: Pos::new(para, 2),
                head: Pos::new(code, 2),
            }))
            .unwrap();
        EditorView::new(editor, cx)
    };
    let mut cross_ed = EditorTestContext::with_view(cx, build_cross);
    cross_ed.keys("enter");
    let cross_blocks = cross_ed.editor(|e| {
        e.document()
            .blocks()
            .into_iter()
            .map(|id| {
                (
                    e.document().block_type(id).unwrap(),
                    e.document().text(id).unwrap(),
                )
            })
            .collect::<Vec<_>>()
    });
    assert_eq!(
        cross_blocks,
        vec![
            (block::PARAGRAPH.to_owned(), "ab".to_owned()),
            (block::PARAGRAPH.to_owned(), "gh".to_owned()),
        ]
    );

    // 3. Opening the block menu and pressing Enter in a code block closes the
    //    menu, and custom keymaps govern Command::SplitBlock in code blocks.
    let build_keymap = |cx: &mut gpui::Context<EditorView>| {
        let mut doc = Document::new(Schema::builtin());
        let code = doc.insert_block(None, 0, block::CODE_BLOCK).unwrap();
        doc.insert_text(code, 0, "hi").unwrap();
        let mut editor = Editor::new(doc);
        editor
            .set_selection(Some(Selection::caret(Pos::new(code, 2))))
            .unwrap();
        EditorView::new(editor, cx).with_keymap(vec![
            KeyBinding {
                key: "Enter".to_owned(),
                command: Command::DeleteBackward,
            },
            KeyBinding {
                key: "Shift-Enter".to_owned(),
                command: Command::SplitBlock,
            },
        ])
    };
    let mut keymap_ed = EditorTestContext::with_view(cx, build_keymap);
    let code = keymap_ed.editor(|e| e.document().blocks()[0]);
    let over = keymap_ed.view.update(keymap_ed.cx, |view, _| {
        let bounds = view.text_bounds(code).unwrap();
        point(bounds.left() + px(4.), bounds.top() + px(2.))
    });
    keymap_ed
        .cx
        .simulate_mouse_move(over, None, Modifiers::none());
    keymap_ed.settle();
    let handle = keymap_ed
        .cx
        .debug_bounds("side-handle")
        .expect("side handle visible")
        .center();
    keymap_ed.cx.simulate_click(handle, Modifiers::none());
    keymap_ed.settle();
    assert!(keymap_ed.cx.debug_bounds("block-menu-delete").is_some());

    // Enter is rebound to DeleteBackward: it closes the menu and runs the custom binding.
    keymap_ed.keys("enter");
    keymap_ed.settle();
    assert!(keymap_ed.cx.debug_bounds("block-menu-delete").is_none());
    assert_eq!(keymap_ed.editor(|e| e.document().text(code).unwrap()), "h");

    // Shift-Enter is bound to Command::SplitBlock: it inserts '\n', then exits on empty line.
    keymap_ed.keys("shift-enter");
    assert_eq!(
        keymap_ed.editor(|e| e.document().text(code).unwrap()),
        "h\n"
    );
    assert_eq!(keymap_ed.editor(|e| e.document().blocks().len()), 1);
    keymap_ed.keys("shift-enter");
    assert_eq!(keymap_ed.editor(|e| e.document().blocks().len()), 2);
}
