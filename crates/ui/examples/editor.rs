//! An editor window, for trying the editor by hand.

use gpui::{
    App, Bounds, Entity, Focusable, Window, WindowBounds, WindowOptions, div, prelude::*, px, size,
    white,
};
use lotype_core::{Document, Editor, Pos, Schema, Selection};
use lotype_ui::EditorView;

const TEXT: &str = "# Lotype\n\nSome **bold**, *italic*, ~~struck~~ and `code` text, \
with a [link](https://lotype.dev).\n\n## Wrapping\n\nA long paragraph that wraps \
over several lines when the window is narrow enough, to see how lines break. \
日本語のテキストも。\n\n```\nfn main() {}\n```\n";

/// The view draws no background of its own: that is the application's.
struct Page(Entity<EditorView>);

impl Render for Page {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full().p_4().bg(white()).child(self.0.clone())
    }
}

fn main() {
    gpui_platform::application().run(|cx: &mut App| {
        let mut doc = Document::new(Schema::builtin());
        doc.insert_markdown(None, 0, TEXT)
            .expect("the sample is valid Markdown");
        let mut editor = Editor::new(doc);
        let first = editor.document().blocks()[0];
        let caret = Selection::caret(Pos::new(first, 0));
        editor
            .set_selection(Some(caret))
            .expect("the block has text");
        let bounds = Bounds::centered(None, size(px(640.), px(480.)), cx);
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
