//! An editor window, for trying the editor by hand.

use gpui::{
    App, Bounds, Entity, Focusable, Window, WindowBounds, WindowOptions, div, prelude::*, px, size,
    white,
};
use lotype_core::{Atom, Document, Editor, InlineSpec, Pos, Schema, Selection, Value};
use lotype_ui::EditorView;

const TEXT: &str = "# Lotype\n\nSome **bold**, *italic*, ~~struck~~ and `code` text, \
with a [link](https://lotype.dev).\n\n## Wrapping\n\nA long paragraph that wraps \
over several lines when the window is narrow enough, to see how lines break. \
日本語のテキストも。\n\n```\nfn main() {}\n```\n\n\
- A bullet\n  - nested under it\n    1. numbered\n    2. and counted\n\
- [ ] a task\n- [x] a task done\n\n> A quote\n>\n> - with a list in it\n\n\
---\n\nAfter a divider.\n\n> [!TIP]\n> An alert.\n\n\
| title | description |\n| --- | --- |\n| tony stark | a senior software engineer |\n\
| gitfromwildan | IT support that keeps your business online. |\n\n\
<!-- page-break -->\n\nThe end.\n";

/// The second line of the last cell: Markdown puts a cell on one line.
const CELL_LINE: &str = "\nAnd Dev open-source tools.";

/// A mention, written as `@name`.
fn mention(atom: &Atom) -> String {
    match atom.attrs.get("name") {
        Some(Value::Str(name)) => format!("@{name}"),
        _ => "@".to_owned(),
    }
}

/// The view draws no background of its own: that is the application's.
struct Page(Entity<EditorView>);

impl Render for Page {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full().p_4().bg(white()).child(self.0.clone())
    }
}

fn main() {
    gpui_platform::application().run(|cx: &mut App| {
        let schema = Schema::builtin().inline(InlineSpec::new("mention", mention));
        let mut doc = Document::new(schema);
        doc.insert_markdown(None, 0, TEXT)
            .expect("the sample is valid Markdown");
        let table = doc.blocks().into_iter().find(|block| {
            doc.block_type(*block)
                .is_ok_and(|block_type| block_type == "table")
        });
        let table = table.expect("the sample has a table");
        let last_row = *doc.children(Some(table)).unwrap().last().unwrap();
        let last_column = *doc.columns(table).unwrap().last().unwrap();
        let cell = doc.cell(last_row, last_column).unwrap();
        let end = doc.text_len(cell).unwrap();
        doc.insert_text(cell, end, CELL_LINE).unwrap();
        let mut editor = Editor::new(doc);
        // An atom at the end of the second paragraph, and the caret after it.
        let second = editor.document().blocks()[1];
        let end = editor.document().text_len(second).expect("it holds text");
        let caret = Selection::caret(Pos::new(second, end));
        editor
            .set_selection(Some(caret))
            .expect("the block has text");
        let ann = Atom::new("mention").attr("name", "Ann");
        editor.insert_atom(ann).expect("the schema has mentions");
        let bounds = Bounds::centered(None, size(px(640.), px(960.)), cx);
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            ..Default::default()
        };
        cx.open_window(options, |window, cx| {
            let editor = cx.new(|cx| EditorView::new(editor, cx));
            window.focus(&editor.focus_handle(cx), cx);
            cx.new(|_| Page(editor))
        })
        .expect("the window opens");
        cx.activate(true);
    });
}
