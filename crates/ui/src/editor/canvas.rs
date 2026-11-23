//! The blocks as a column of rows: indented by how deep they are nested,
//! with list markers, and with the bars of the quotes and alerts they are in.

use std::collections::HashSet;

use gpui::{
    AnyElement, Context, FontWeight, Hsla, MouseButton, SharedString, TextStyle, Window, div, img,
    prelude::*, px, rgb,
};
use lotype_core::{BlockId, Document, Pos, Selection, Value};

use super::EditorView;
use super::input::Shown;

/// How far each level of nesting moves a block to the right.
const INDENT: f32 = 24.;
/// Deeper blocks go no further right, so that a peer's deep nesting
/// leaves their text room.
const DEEPEST_INDENT: usize = 16;

/// How far right a block `depth` deep is.
fn indent(depth: usize) -> f32 {
    INDENT * depth.min(DEEPEST_INDENT) as f32
}

/// A quote or an alert around a block: how deep it is, and its colour.
type Bar = (usize, Hsla);

/// A block as it is shown.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Row {
    pub block: BlockId,
    pub block_type: String,
    pub depth: usize,
    /// The number of a numbered list item, counted along its siblings.
    pub number: Option<i64>,
    /// The quotes and alerts around the block, itself included.
    pub bars: Vec<Bar>,
}

/// The rows of the blocks shown, in document order: every block but those
/// in a closed toggle or in a table.
pub(crate) fn rows(doc: &Document, closed: &HashSet<BlockId>) -> Vec<Row> {
    let mut rows = Vec::new();
    // The blocks around the next one, outermost first: whether each hides
    // what is in it, and its bar.
    let mut around: Vec<(BlockId, bool, Option<Bar>)> = Vec::new();
    // The number of the last sibling at each depth, while siblings are numbered.
    let mut numbers: Vec<Option<i64>> = Vec::new();
    for block in doc.blocks() {
        let parent = doc.parent(block).ok().flatten();
        while around.last().is_some_and(|(at, ..)| Some(*at) != parent) {
            around.pop();
        }
        let depth = around.len();
        let block_type = doc.block_type(block).unwrap_or_default();
        numbers.resize(depth + 1, None);
        let number = (block_type == "numbered_list_item").then(|| match numbers[depth] {
            Some(before) => before.saturating_add(1),
            None => match doc.attr(block, "start") {
                Ok(Some(Value::Int(start))) => start,
                _ => 1,
            },
        });
        numbers[depth] = number;
        let bar = match block_type.as_str() {
            "quote" => Some((depth, quote_color())),
            "alert" => Some((depth, alert_style(&text_attr(doc, block, "kind")).1)),
            _ => None,
        };
        let hidden = around.last().is_some_and(|(_, hides, _)| *hides);
        if !hidden {
            let outer = around.iter().filter_map(|(.., bar)| *bar);
            rows.push(Row {
                block,
                block_type: block_type.clone(),
                depth,
                number,
                bars: outer.chain(bar).collect(),
            });
        }
        let hides =
            block_type == "table" || block_type == "toggle_list_item" && closed.contains(&block);
        around.push((block, hidden || hides, bar));
    }
    rows
}

fn text_attr(doc: &Document, block: BlockId, key: &str) -> String {
    match doc.attr(block, key) {
        Ok(Some(Value::Str(text))) => text,
        _ => String::new(),
    }
}

fn muted() -> Hsla {
    rgb(0x8a8f98).into()
}

/// The title and colour of an alert of `kind`, as GitHub draws them; a
/// kind it does not know is a note.
fn alert_style(kind: &str) -> (&'static str, Hsla) {
    let kinds = [
        ("Tip", 0x1a7f37),
        ("Important", 0x8250df),
        ("Warning", 0x9a6700),
        ("Caution", 0xd1242f),
    ];
    let known = kinds
        .into_iter()
        .find(|(name, _)| name.eq_ignore_ascii_case(kind));
    let (title, color) = known.unwrap_or(("Note", 0x0969da));
    (title, rgb(color).into())
}

fn quote_color() -> Hsla {
    rgb(0xd0d4d9).into()
}

impl EditorView {
    /// A row's element, or `None` for a block this view does not draw.
    pub(super) fn row_element(
        &mut self,
        row: &Row,
        base: &TextStyle,
        shown: Option<&Shown>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let block = row.block;
        let doc = self.editor.document();
        let content = match row.block_type.as_str() {
            "divider" | "page_break" => {
                let line = div().h(px(1.)).w_full().bg(rgb(0xe1e4e8));
                div().h(px(17.)).w_full().flex().items_center().child(line)
            }
            "image" => {
                let (url, caption) = (
                    text_attr(doc, block, "url"),
                    text_attr(doc, block, "caption"),
                );
                let label = if caption.is_empty() {
                    url.clone()
                } else {
                    caption.clone()
                };
                let shown = SharedString::from(label);
                let fallback = move || {
                    let missing = div().p_2().rounded_md().bg(rgb(0xf3f4f6));
                    missing
                        .text_color(muted())
                        .child(shown.clone())
                        .into_any_element()
                };
                let image = img(url).max_w_full().with_fallback(fallback);
                let caption =
                    (!caption.is_empty()).then(|| div().text_color(muted()).child(caption));
                div().flex().flex_col().child(image).children(caption)
            }
            "file" => {
                let (url, name) = (text_attr(doc, block, "url"), text_attr(doc, block, "name"));
                let name = if name.is_empty() { url } else { name };
                div().text_color(rgb(0x0969da)).child(format!("📎 {name}"))
            }
            "alert" => {
                let (title, color) = alert_style(&text_attr(doc, block, "kind"));
                div()
                    .pl(px(INDENT))
                    .text_color(color)
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(title)
            }
            "quote" => div()
                .pl(px(INDENT))
                .child(self.block_text(block, base, shown, window, cx)?),
            "code_block" => div()
                .w_full()
                .p_2()
                .rounded_md()
                .bg(rgb(0xf3f4f6))
                .child(self.block_text(block, base, shown, window, cx)?),
            _ => self.block_text(block, base, shown, window, cx)?,
        };
        let bars = row.bars.iter().map(|(depth, color)| {
            let left = px(indent(*depth) + 2.);
            let bar = div().absolute().top_0().bottom_0().left(left).w(px(3.));
            bar.bg(*color)
        });
        let row_element = div()
            .relative()
            .flex()
            .items_start()
            .py(px(2.))
            .pl(px(indent(row.depth)))
            .children(bars)
            .children(self.marker(row, cx))
            .child(div().flex_1().min_w_0().child(content));
        Some(row_element.into_any_element())
    }

    /// The bullet, number, box or arrow before a list item.
    fn marker(&self, row: &Row, cx: &mut Context<Self>) -> Option<AnyElement> {
        let block = row.block;
        let slot = div().w(px(INDENT)).flex_none().flex().justify_center();
        let marker = match row.block_type.as_str() {
            "bullet_list_item" => slot.child("•"),
            "numbered_list_item" => slot.child(format!("{}.", row.number.unwrap_or(1))),
            "check_list_item" => {
                let doc = self.editor.document();
                let checked = matches!(doc.attr(block, "checked"), Ok(Some(Value::Bool(true))));
                let mut check = div()
                    .mt(px(4.))
                    .size(px(14.))
                    .rounded_sm()
                    .border_1()
                    .border_color(muted())
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_size(px(10.))
                    .cursor_pointer();
                if checked {
                    check = check.bg(rgb(0x0969da)).text_color(rgb(0xffffff)).child("✓");
                }
                let click = cx.listener(move |view: &mut Self, _: &gpui::MouseDownEvent, _, cx| {
                    cx.stop_propagation();
                    view.set_checked(block, !checked);
                    cx.notify();
                });
                slot.child(check.on_mouse_down(MouseButton::Left, click))
            }
            "toggle_list_item" => {
                let open = !self.closed.contains(&block);
                let click = cx.listener(move |view: &mut Self, _: &gpui::MouseDownEvent, _, cx| {
                    cx.stop_propagation();
                    if !view.closed.remove(&block) {
                        view.close(block);
                    }
                    cx.notify();
                });
                let arrow = div().cursor_pointer().child(if open { "▾" } else { "▸" });
                slot.text_color(muted())
                    .child(arrow.on_mouse_down(MouseButton::Left, click))
            }
            _ => return None,
        };
        Some(marker.into_any_element())
    }

    fn set_checked(&mut self, block: BlockId, checked: bool) {
        self.editor
            .apply(|doc, selection| {
                doc.set_attr(block, "checked", checked)?;
                Ok(selection.cloned())
            })
            .ok();
    }

    /// Closes a toggle, taking the selection out of what it hides.
    fn close(&mut self, toggle: BlockId) {
        self.closed.insert(toggle);
        let doc = self.editor.document();
        let Some(Selection::Text { anchor, head }) = self.editor.selection() else {
            return;
        };
        let hidden = |block: BlockId| {
            let mut at = doc.parent(block).ok().flatten();
            while let Some(parent) = at {
                if parent == toggle {
                    return true;
                }
                at = doc.parent(parent).ok().flatten();
            }
            false
        };
        if hidden(anchor.block) || hidden(head.block) {
            let end = Pos::new(toggle, doc.text_len(toggle).unwrap_or(0));
            self.editor.set_selection(Some(Selection::caret(end))).ok();
        }
    }

    /// Opens the toggles around the selection, so that it stays in sight.
    pub(super) fn open_around_selection(&mut self) {
        let doc = self.editor.document();
        let ends = match self.editor.selection() {
            Some(Selection::Text { anchor, head }) => vec![anchor.block, head.block],
            _ => Vec::new(),
        };
        for end in ends {
            let mut at = doc.parent(end).ok().flatten();
            while let Some(block) = at {
                self.closed.remove(&block);
                at = doc.parent(block).ok().flatten();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use lotype_core::test_support::build;
    use lotype_core::{blocks, bullet, p};

    use super::*;

    #[test]
    fn rows_are_nested_numbered_and_hidden_in_closed_toggles() {
        let (mut doc, _) = build(blocks![p!["one", bullet!["child"]], p!["two"]]);
        let blocks = doc.blocks();
        let (one, child, two) = (blocks[0], blocks[1], blocks[2]);
        let quote = doc.insert_block(None, 1, "quote").unwrap();
        let first = doc
            .insert_block(Some(quote), 0, "numbered_list_item")
            .unwrap();
        let second = doc
            .insert_block(Some(quote), 1, "numbered_list_item")
            .unwrap();
        doc.set_attr(first, "start", 3).unwrap();
        let toggle = doc.insert_block(None, 2, "toggle_list_item").unwrap();
        let inside = doc.insert_block(Some(toggle), 0, "paragraph").unwrap();

        let shown = |closed: &HashSet<BlockId>| {
            let rows = rows(&doc, closed);
            rows.iter()
                .map(|row| (row.block, row.depth, row.number, row.bars.len()))
                .collect::<Vec<_>>()
        };
        assert_eq!(
            shown(&HashSet::new()),
            [
                (one, 0, None, 0),
                (child, 1, None, 0),
                (quote, 0, None, 1),
                (first, 1, Some(3), 1),
                (second, 1, Some(4), 1),
                (toggle, 0, None, 0),
                (inside, 1, None, 0),
                (two, 0, None, 0),
            ]
        );
        let closed = HashSet::from([toggle]);
        assert!(!shown(&closed).iter().any(|row| row.0 == inside));
    }
}
