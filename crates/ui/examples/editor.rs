//! An editor window, for trying the editor by hand: a tour of the blocks.

use gpui::{
    App, Bounds, Entity, Focusable, Window, WindowBounds, WindowOptions, div, prelude::*, px, size,
    white,
};
use lotype_core::{Atom, BlockId, Document, Editor, InlineSpec, Pos, Schema, Selection, Value};
use lotype_ui::EditorView;

/// The blocks, each under a heading of its own with how to make it and what
/// it does.
const TEXT: &str = r#"# Lotype

A tour of the blocks. Type `/` at the start of a line, or after a space, to pick any of them from the slash menu; select text for the formatting toolbar; hover a block for its side menu.

### Paragraph

Just type. Enter starts the next paragraph, and Shift+Enter a new line,\
like this one.

### Text styles

Some **bold** (`⌘B`), *italic* (`⌘I`), ~~struck~~ (`⌘⇧S`) and `code` (`⌘E`) text, typed between their Markdown signs or with the keys. Select text for underline, colors and [links](https://lotype.dev).

### Headings

Type `#` to `#####` and a space, press `⌥⌘1` to `⌥⌘5`, or pick one from the slash menu. A heading is never inside another block.

### Mentions

An application adds atoms of its own to the text, one step for the arrows, as this mention:

### Bullet list

- Type `-` and a space, or press `⌘⇧8`
  - Tab nests an item, Shift+Tab takes it out
- Enter in an empty item leaves the list

### Numbered list

1. Type `1.` and a space, or press `⌘⇧7`
2. Items count on by themselves

### Check list

- [ ] Type `[]` and a space, or press `⌘⇧9`
- [x] Click the box to check an item

### Toggle list

- Click the arrow to hide what is in it, or press `⌘⇧6` to make one
  - Enter in an open toggle writes in here

### Quote

> Type `>` and a space. Enter leaves the quote for a new paragraph, and Shift+Enter starts a new line in it.

### Alert

> [!TIP]
> Pick Alert in the slash menu, and click its title for another kind: note, tip, important, warning or caution. Enter leaves it, Shift+Enter writes on inside.

### Code block

```
// Type ``` and a space, or press ⌥⌘C.
// No Markdown turns into anything here.
let tour = "of the blocks";
```

### Table

| key | what it does |
| --- | --- |
| Tab | the next cell, or a new row from the last |
| Enter | the cell below, or out of the table from the last row |
| Shift+Enter | a new line in the cell |

Pick Table in the slash menu for a 2 × 2 one. Hover it for the bars that add a row and a column; the cell with the caret has a menu for the rows and columns around it. Drag across cells, or use Shift and a click or the arrows, to select them whole.

### Image

![The Lotype logo](https://raw.githubusercontent.com/lotype/lotype/main/assets/logo.png)

### Divider

Type `---`, or pick Divider in the slash menu.

---

### Page break

Where printing and export start a new page. Pick Page Break in the slash menu.

<!-- page-break -->

### Moving blocks

Drag a block by its handle, or press Escape to select whole blocks, then `⌘⇧↑` and `⌘⇧↓` to move them. A click on the handle opens the block menu, to color or delete a block.
"#;

/// The second line of the last cell: Markdown puts a cell on one line.
const CELL_LINE: &str = "\nlike this one";

/// A mention, written as `@name`.
fn mention(atom: &Atom) -> String {
    match atom.attrs.get("name") {
        Some(Value::Str(name)) => format!("@{name}"),
        _ => "@".to_owned(),
    }
}

/// The first block whose text starts with `start`.
fn block_starting(doc: &Document, start: &str) -> BlockId {
    let found = doc.blocks().into_iter().find(|block| {
        doc.plain_text(*block)
            .is_ok_and(|text| text.starts_with(start))
    });
    found.unwrap_or_else(|| panic!("the tour has a block starting {start:?}"))
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
            .expect("the tour is valid Markdown");
        // What Markdown cannot say: a toggle reads as a bullet, and a cell
        // is one line.
        let toggle = block_starting(&doc, "Click the arrow");
        doc.set_block_type(toggle, "toggle_list_item")
            .expect("a bullet becomes a toggle");
        let table = doc.blocks().into_iter().find(|block| {
            doc.block_type(*block)
                .is_ok_and(|block_type| block_type == "table")
        });
        let table = table.expect("the tour has a table");
        let last_row = *doc.children(Some(table)).unwrap().last().unwrap();
        let last_column = *doc.columns(table).unwrap().last().unwrap();
        let cell = doc.cell(last_row, last_column).unwrap();
        let end = doc.text_len(cell).unwrap();
        doc.insert_text(cell, end, CELL_LINE).unwrap();
        // A mention at the end of its paragraph, and the caret after it.
        let mentions = block_starting(&doc, "An application adds atoms");
        let end = doc.text_len(mentions).expect("it holds text");
        let mut editor = Editor::new(doc);
        let caret = Selection::caret(Pos::new(mentions, end));
        editor
            .set_selection(Some(caret))
            .expect("the block has text");
        editor.insert_text(" ").expect("the block has text");
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
