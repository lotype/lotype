//! An editor window, for trying the editor by hand: a tour of the blocks.

use std::{cell::RefCell, collections::HashMap, path::PathBuf, rc::Rc, sync::Arc, time::Duration};

use futures_util::stream;
use gpui::{
    App, BackgroundExecutor, Bounds, Entity, Focusable, Task, Window, WindowBounds, WindowOptions,
    div, prelude::*, px, size, white,
};
use lotype_ai::{LlmEvent, LlmProvider, LlmRequest, LlmStream};
use lotype_core::{
    Atom, BlockId, Document, Editor, InlineSpec, Pos, Schema, Selection, Value, block,
};
use lotype_ui::{EditorView, MediaSource};
use serde_json::{Value as JsonValue, json};

/// The blocks, each under a heading of its own with how to make it and what
/// it does.
const TEXT: &str = r#"# Lotype

A tour of the blocks. Type `/` at the start of a line or after a space to pick a block type. Select text for the formatting toolbar or hover a block for its side menu.

### Ask AI

Try the offline AI demo. Press Enter at the end of this paragraph, then press Space on the empty line. Send `sample` to preview a Heading 3 followed by a paragraph and a Tip alert. Accept or Reject the whole suggestion.

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

```ts twoslash title="src/blocks.ts"
// @noErrors
interface Block {
  id: string
  kind: "paragraph" | "code"
}
// ---cut---
/** Formats a block label for the outline. */
function describeBlock(block: Block) {
  return `${block.kind}:${block.id}` // [!code ++]
}

const label = describeBlock({ id: "b1", kind: "code" })
//    ^?
label.to
//      ^|
```

Hover any identifier for its inferred signature and JSDoc. Click inside the block to edit hidden `---cut---`, `^?`, and `^|` Twoslash directives, then press Escape or click outside the block to fold back.

### Table

| key | what it does |
| --- | --- |
| Tab | the next cell, or a new row from the last |
| Enter | the cell below, or out of the table from the last row |
| Shift+Enter | a new line in the cell |

Pick Table in the slash menu for a 2 × 2 one. Hover it for the bars that add a row and a column; the cell with the caret has a menu for the rows and columns around it. Drag across cells, or use Shift and a click or the arrows, to select them whole.

### Image

![The Lotype logo](demo:logo)

### File

[SDK Cargo manifest](demo:manifest "lotype-file")

Use `/image` or `/file`, then Choose file to attach a local file in this demo.

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

const DEMO_TEXT: &str = "This paragraph is a suggestion from the offline demo.";
const DEMO_HEADING: &str = "### A heading from the offline demo";
const DEMO_ALERT: &str = "> [!TIP]\n>\n> Review the suggested block before accepting it.";
const DEMO_EVENT_DELAY: Duration = Duration::from_millis(400);

/// Streams a scripted response so the demo can show both writing and review.
struct DemoProvider {
    executor: BackgroundExecutor,
}

const DEMO_NESTED_HEADING: &str = "Headings can only go at the top level. Try `sample` on a top-level empty line, or send `paragraph` or `alert` here.";

impl DemoProvider {
    /// Builds the scripted reply and edits for the caret in the request context.
    fn plan(request: &LlmRequest) -> (&'static str, Vec<LlmEvent>) {
        if !request.tools.iter().any(|tool| tool.name == "update") {
            return (
                "Press Space on an empty line to preview a sample edit.",
                Vec::new(),
            );
        }
        let Some(message) = request.messages.last() else {
            return (
                "Press Space on an empty line to preview a sample edit.",
                Vec::new(),
            );
        };
        let Some((document, prompt)) = message
            .text
            .strip_prefix("<document>\n")
            .and_then(|text| text.split_once("\n</document>\n\n"))
        else {
            return (
                "Press Space on an empty line to preview a sample edit.",
                Vec::new(),
            );
        };
        let Ok(document) = serde_json::from_str::<JsonValue>(document) else {
            return (
                "Press Space on an empty line to preview a sample edit.",
                Vec::new(),
            );
        };
        let Some(cursor) = document.get("cursor").and_then(JsonValue::as_str) else {
            return (
                "Press Space on an empty line to preview a sample edit.",
                Vec::new(),
            );
        };
        let depth = document
            .get("blocks")
            .and_then(JsonValue::as_array)
            .and_then(|blocks| {
                blocks.iter().find_map(|entry| {
                    (entry.get("id").and_then(JsonValue::as_str) == Some(cursor))
                        .then(|| entry.get("depth").and_then(JsonValue::as_u64))
                        .flatten()
                })
            })
            .unwrap_or(0);
        let blocks: &[&str] = match prompt.trim().to_ascii_lowercase().as_str() {
            "heading 3" | "heading3" => &[DEMO_HEADING],
            "paragraph" => &[DEMO_TEXT],
            "alert" => &[DEMO_ALERT],
            _ => &[DEMO_HEADING, DEMO_TEXT, DEMO_ALERT],
        };
        if depth > 0 && blocks.contains(&DEMO_HEADING) {
            return (DEMO_NESTED_HEADING, Vec::new());
        }
        let mut events = vec![LlmEvent::ToolCallDelta {
            id: "demo-write".into(),
            name: "update".into(),
            args_delta: json!({"id": cursor, "block": blocks[0]}).to_string(),
        }];
        if blocks.len() > 1 {
            events.push(LlmEvent::ToolCallDelta {
                id: "demo-add".into(),
                name: "add".into(),
                args_delta: json!({
                    "referenceId": cursor,
                    "position": "after",
                    "blocks": &blocks[1..],
                })
                .to_string(),
            });
        }
        ("Here's the sample.", events)
    }
}

impl LlmProvider for DemoProvider {
    fn stream(&self, request: LlmRequest) -> LlmStream {
        let (reply, edits) = Self::plan(&request);
        let events = [LlmEvent::TextDelta(reply.into())]
            .into_iter()
            .chain(edits)
            .chain([LlmEvent::Done]);
        let executor = self.executor.clone();
        Box::pin(stream::unfold(
            (events, executor),
            |(mut events, executor)| async move {
                let event = events.next()?;
                executor.timer(DEMO_EVENT_DELAY).await;
                Some((Ok(event), (events, executor)))
            },
        ))
    }
}

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
        let crate_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let paths = Rc::new(RefCell::new(HashMap::from([
            ("demo:logo".to_owned(), crate_dir.join("assets/logo.png")),
            ("demo:manifest".to_owned(), crate_dir.join("Cargo.toml")),
        ])));
        let schema = Schema::builtin().inline(InlineSpec::new("mention", mention));
        let mut doc = Document::new(schema);
        doc.insert_markdown(None, 0, TEXT)
            .expect("the tour is valid Markdown");
        let provider = Arc::new(DemoProvider {
            executor: cx.background_executor().clone(),
        });
        // What Markdown cannot say: a toggle reads as a bullet, and a cell
        // is one line.
        let toggle = block_starting(&doc, "Click the arrow");
        doc.set_block_type(toggle, block::TOGGLE_LIST_ITEM)
            .expect("a bullet becomes a toggle");
        let table = doc.blocks().into_iter().find(|block| {
            doc.block_type(*block)
                .is_ok_and(|block_type| block_type == block::TABLE)
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
        cx.open_window(options, move |window, cx| {
            let editor = cx.new(|cx| {
                let imports = paths.clone();
                let view = EditorView::new(editor, cx)
                    .with_ai(provider)
                    .with_media_importer(move |path, _| {
                        // Keep the mapping for this demo session. Applications can
                        // persist the bytes and return a durable reference here.
                        let mut files = imports.borrow_mut();
                        let reference = format!("demo:attachment/{}", files.len());
                        files.insert(reference.clone(), path);
                        Task::ready(Ok(reference))
                    })
                    .with_media_resolver(move |reference| match paths.borrow().get(reference) {
                        Some(path) => Some(MediaSource::Path(path.clone())),
                        None => Some(MediaSource::Url(reference.to_owned())),
                    });
                lotype_code::install(view, cx)
            });
            window.focus(&editor.focus_handle(cx), cx);
            cx.new(|_| Page(editor))
        })
        .expect("the window opens");
        cx.activate(true);
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{Modifiers, TestAppContext};
    use lotype_core::test_support::editor;
    use lotype_core::{blocks, p};
    use lotype_ui::test_support::EditorTestContext;

    fn submit_prompt(ed: &mut EditorTestContext, prompt: &str) {
        ed.type_text(prompt);
        ed.keys("enter");
        for _ in 0..4 {
            ed.cx.executor().advance_clock(DEMO_EVENT_DELAY);
            ed.settle();
        }
    }

    fn click_review(ed: &mut EditorTestContext, action: &'static str) {
        let button = ed
            .cx
            .debug_bounds(action)
            .expect("the review action is visible");
        ed.cx.simulate_click(button.center(), Modifiers::none());
        ed.settle();
    }

    #[gpui::test]
    fn the_demo_prompt_and_review_make_room_between_paragraphs(cx: &mut TestAppContext) {
        let editor = editor(blocks![
            p!["First"],
            p!["Second"],
            p!["Above"],
            p!["<a>"],
            p!["Below"]
        ]);
        let ids = editor.document().blocks();
        let (above, target, below) = (ids[2], ids[3], ids[4]);
        let mut ed = EditorTestContext::with_view(cx, |cx| {
            let provider = Arc::new(DemoProvider {
                executor: cx.background_executor().clone(),
            });
            EditorView::new(editor, cx).with_ai(provider)
        });
        let height = ed
            .view
            .update(ed.cx, |view, _| view.text_bounds(target).unwrap().bottom())
            + px(8.);
        ed.cx.simulate_resize(size(px(400.), height));
        ed.settle();
        let assert_card_position = |ed: &mut EditorTestContext| {
            let [above, paragraph, below] = ed.view.update(ed.cx, |view, _| {
                [above, target, below].map(|block| view.text_bounds(block).unwrap())
            });
            let bar = ed
                .cx
                .debug_bounds("ai-bar")
                .expect("the AI card is visible");
            assert!(above.bottom() <= paragraph.top());
            assert!(paragraph.bottom() <= bar.top() && bar.bottom() <= below.top());
            assert!(bar.top() >= px(0.) && bar.bottom() <= height);
        };

        for action in ["ai-reject", "ai-accept"] {
            ed.type_text(" ");
            assert_card_position(&mut ed);
            submit_prompt(&mut ed, "paragraph");

            ed.view.update(ed.cx, |view, _| {
                let draft = view.ai_document().expect("the preview is open");
                assert_eq!(draft.plain_text(target).unwrap(), DEMO_TEXT);
                assert!(
                    view.editor()
                        .document()
                        .plain_text(target)
                        .unwrap()
                        .is_empty()
                );
            });
            assert_card_position(&mut ed);
            click_review(&mut ed, action);
        }

        assert_eq!(
            ed.editor(|editor| editor.document().plain_text(target).unwrap()),
            DEMO_TEXT
        );
    }

    #[gpui::test]
    fn the_demo_previews_and_accepts_the_blocks_in_order(cx: &mut TestAppContext) {
        let editor = editor(blocks![
            p!["First"],
            p!["Second"],
            p!["Above"],
            p!["<a>"],
            p!["Below"]
        ]);
        let ids = editor.document().blocks();
        let (above, target, below) = (ids[2], ids[3], ids[4]);
        let mut ed = EditorTestContext::with_view(cx, |cx| {
            let provider = Arc::new(DemoProvider {
                executor: cx.background_executor().clone(),
            });
            EditorView::new(editor, cx).with_ai(provider)
        });
        let height = ed
            .view
            .update(ed.cx, |view, _| view.text_bounds(target).unwrap().bottom())
            + px(8.);
        ed.cx.simulate_resize(size(px(400.), height));
        ed.settle();
        let check_blocks = |doc: &Document| {
            let top = doc.children(None).unwrap();
            let kinds: Vec<_> = top
                .iter()
                .map(|block| doc.block_type(*block).unwrap())
                .collect();
            assert_eq!(
                kinds,
                [
                    block::PARAGRAPH,
                    block::PARAGRAPH,
                    block::PARAGRAPH,
                    block::HEADING,
                    block::PARAGRAPH,
                    block::ALERT,
                    block::PARAGRAPH,
                ]
            );
            assert_eq!(top[2], above);
            assert_eq!(top[3], target);
            assert_eq!(top[6], below);
            doc.children(Some(top[5])).unwrap()[0]
        };
        for action in ["ai-reject", "ai-accept"] {
            ed.type_text(" ");
            submit_prompt(&mut ed, "sample");
            let alert_body = ed.view.update(ed.cx, |view, _| {
                check_blocks(view.ai_document().expect("the preview is open"))
            });
            let (alert_bounds, below_bounds) = ed.view.update(ed.cx, |view, _| {
                (
                    view.text_bounds(alert_body).unwrap(),
                    view.text_bounds(below).unwrap(),
                )
            });
            let bar = ed
                .cx
                .debug_bounds("ai-bar")
                .expect("the AI card is visible");
            assert!(alert_bounds.bottom() <= bar.top() && bar.bottom() <= below_bounds.top());
            assert!(bar.top() >= px(0.) && bar.bottom() <= height);
            ed.assert(blocks![
                p!["First"],
                p!["Second"],
                p!["Above"],
                p!["<a>"],
                p!["Below"]
            ]);
            click_review(&mut ed, action);
        }
        ed.editor(|editor| {
            check_blocks(editor.document());
        });
        ed.type_text("!");
        assert_eq!(
            ed.editor(|editor| editor.document().plain_text(target).unwrap()),
            "!A heading from the offline demo"
        );
    }

    #[gpui::test]
    fn nested_empty_paragraphs_explain_heading_limits_and_allow_nested_edits(
        cx: &mut TestAppContext,
    ) {
        let mut doc = Document::new(Schema::builtin());
        let top = doc.insert_markdown(None, 0, "- Open toggle").unwrap();
        doc.set_block_type(top[0], block::TOGGLE_LIST_ITEM).unwrap();
        let nested = doc.insert_block(Some(top[0]), 0, block::PARAGRAPH).unwrap();
        let mut editor = Editor::new(doc);
        editor
            .set_selection(Some(Selection::caret(Pos::new(nested, 0))))
            .unwrap();
        let mut ed = EditorTestContext::with_view(cx, |cx| {
            let provider = Arc::new(DemoProvider {
                executor: cx.background_executor().clone(),
            });
            EditorView::new(editor, cx).with_ai(provider)
        });

        ed.type_text(" ");
        submit_prompt(&mut ed, "sample");
        assert!(ed.cx.debug_bounds("ai-accept").is_none());
        assert!(ed.cx.debug_bounds("ai-status").is_some());
        let reply = ed.view.update(ed.cx, |view, _| {
            view.ai_transcript().messages[1].text.clone()
        });
        assert_eq!(reply, DEMO_NESTED_HEADING);

        submit_prompt(&mut ed, "paragraph");
        click_review(&mut ed, "ai-accept");
        assert_eq!(
            ed.editor(|editor| editor.document().plain_text(nested).unwrap()),
            DEMO_TEXT
        );
    }
}
