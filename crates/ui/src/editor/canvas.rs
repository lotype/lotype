//! The blocks as a column of rows: indented by how deep they are nested,
//! with list markers, and with the bars of the quotes and alerts they are in.

use std::collections::HashSet;

use gpui::{
    AnyElement, App, Bounds, Context, FontStyle, Hsla, MouseButton, Pixels, SharedString,
    TextStyle, Window, canvas, div, img, point, prelude::*, px, rgb, size,
};
use lotype_core::{BlockColor, BlockId, Document, Pos, Role, Selection, Value, attr};

use super::EditorView;
use super::alert::alert_color;
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
    /// How deep the selected block is that this one is, or is in, where
    /// one is: the selection is painted from there.
    pub selected: Option<usize>,
    /// The block is a quote or in one, so its text is italic.
    pub quoted: bool,
}

/// A block around the next one in document order.
struct Around {
    block: BlockId,
    /// It, or one around it, hides what is in it.
    hides: bool,
    selected: Option<usize>,
    quoted: bool,
    bar: Option<Bar>,
}

/// The rows of the blocks shown, in document order: every block but those
/// in a closed toggle or in a table.
pub(crate) fn rows(
    doc: &Document,
    closed: &HashSet<BlockId>,
    selected: &HashSet<BlockId>,
) -> Vec<Row> {
    let mut rows = Vec::new();
    // The blocks around the next one, outermost first.
    let mut around: Vec<Around> = Vec::new();
    // The number of the last sibling at each depth, while siblings are numbered.
    let mut numbers: Vec<Option<i64>> = Vec::new();
    for block in doc.blocks() {
        let parent = doc.parent(block).ok().flatten();
        while around
            .last()
            .is_some_and(|outer| Some(outer.block) != parent)
        {
            around.pop();
        }
        let depth = around.len();
        let block_type = doc.block_type(block).unwrap_or_default();
        numbers.resize(depth + 1, None);
        let number = (block_type == "numbered_list_item").then(|| match numbers[depth] {
            Some(before) => before.saturating_add(1),
            None => match doc.attr(block, attr::START) {
                Ok(Some(Value::Int(start))) => start,
                _ => 1,
            },
        });
        numbers[depth] = number;
        let bar = match block_type.as_str() {
            "quote" => Some((depth, quote_color())),
            "alert" => Some((depth, alert_color(doc, block))),
            _ => None,
        };
        let outer = around.last();
        let hidden = outer.is_some_and(|outer| outer.hides);
        let chosen = outer
            .and_then(|outer| outer.selected)
            .or(selected.contains(&block).then_some(depth));
        let quoted = block_type == "quote" || outer.is_some_and(|outer| outer.quoted);
        if !hidden {
            let outer = around.iter().filter_map(|outer| outer.bar);
            rows.push(Row {
                block,
                block_type: block_type.clone(),
                depth,
                number,
                bars: outer.chain(bar).collect(),
                selected: chosen,
                quoted,
            });
        }
        let toggle = doc.role(block) == Role::Toggle;
        let hides = block_type == "table" || toggle && closed.contains(&block);
        around.push(Around {
            block,
            hides: hidden || hides,
            selected: chosen,
            quoted,
            bar,
        });
    }
    rows
}

fn muted() -> Hsla {
    rgb(0x8a8f98).into()
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
        // A block's own colors, which its marks go on top of, where its
        // type takes colors: code, math and diagrams have their own.
        let painted = |which| {
            let name = doc.block_color(block, which)?;
            crate::marks::color(&Value::Str(name), which == BlockColor::Background)
        };
        let (ink, fill) = (painted(BlockColor::Text), painted(BlockColor::Background));
        let mut colored = base.clone();
        if let Some(ink) = ink {
            colored.color = ink;
        }
        // What a quote says, its own text and what is in it, is italic.
        if row.quoted {
            colored.font_style = FontStyle::Italic;
        }
        let base = &colored;
        let content = match row.block_type.as_str() {
            "divider" => {
                let line = div().h(px(1.)).w_full().bg(rgb(0xe1e4e8));
                div().h(px(17.)).w_full().flex().items_center().child(line)
            }
            // Where printing starts a new page: dashed, and named, as word
            // processors draw it, so it is not taken for a divider.
            "page_break" => {
                // GPUI draws no dashed border on one side alone.
                let dashes = || {
                    let paint =
                        |bounds: Bounds<Pixels>, _: (), window: &mut Window, _: &mut App| {
                            let y = bounds.center().y;
                            let mut x = bounds.left();
                            while x < bounds.right() {
                                let dash = Bounds::new(point(x, y), size(px(4.), px(1.)));
                                window.paint_quad(gpui::fill(dash, muted()));
                                x += px(8.);
                            }
                        };
                    canvas(|_, _, _| (), paint).flex_1().h(px(1.))
                };
                let label = div()
                    .px_2()
                    .text_size(px(11.))
                    .text_color(muted())
                    .child("Page break");
                let row = div().h(px(17.)).w_full().flex().items_center();
                row.child(dashes()).child(label).child(dashes())
            }
            "image" => {
                let (url, caption) = (
                    doc.text_of(block, attr::URL),
                    doc.text_of(block, attr::CAPTION),
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
                let (url, name) = (
                    doc.text_of(block, attr::URL),
                    doc.text_of(block, attr::NAME),
                );
                let name = if name.is_empty() { url } else { name };
                div().text_color(rgb(0x0969da)).child(format!("📎 {name}"))
            }
            "alert" => div().pl(px(INDENT)).child(self.alert_title(block, cx)),
            "table" => div().child(self.table_grid(block, base, shown, window, cx)?),
            "quote" => div()
                .pl(px(INDENT))
                .child(self.block_text(block, base, shown, window, cx)?),
            _ if doc.role(block) == Role::Code => div()
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
        // Over the block's own colors, as the text selection is.
        // From the selected block's indent, so that it and what is in it
        // read as one, with an edge on the left that marks it.
        let selected = row.selected.map(|depth| {
            let overlay = div().absolute().top_0().bottom_0().right_0();
            let overlay = overlay.left(px(indent(depth)));
            let edge = rgb(0x0969da);
            overlay
                .bg(super::paint::selection_color())
                .border_l_2()
                .border_color(edge)
        });
        let row_element = div()
            .relative()
            .flex()
            .items_start()
            .py(px(2.))
            .pl(px(indent(row.depth)))
            .children(bars)
            .children(self.side_menu(row.block, indent(row.depth), cx))
            .children(self.drop_line(row.block, indent(row.depth)))
            .children(self.marker(row, cx))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .rounded_sm()
                    .when_some(fill, |content, fill| content.bg(fill))
                    .child(content),
            )
            .children(selected);
        Some(row_element.into_any_element())
    }

    /// The bullet, number, box or arrow before a list item.
    fn marker(&self, row: &Row, cx: &mut Context<Self>) -> Option<AnyElement> {
        let block = row.block;
        let slot = div().w(px(INDENT)).flex_none().flex().justify_center();
        let toggle = self.editor.document().role(block) == Role::Toggle;
        let marker = match row.block_type.as_str() {
            _ if toggle => {
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
            "bullet_list_item" => slot.child("•"),
            "numbered_list_item" => slot.child(format!("{}.", row.number.unwrap_or(1))),
            "check_list_item" => {
                let checked = self.editor.document().is_checked(block);
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
                    view.editor.set_checked(block, !checked).ok();
                    cx.notify();
                });
                slot.child(check.on_mouse_down(MouseButton::Left, click))
            }
            _ => return None,
        };
        Some(marker.into_any_element())
    }

    /// Closes a toggle, taking the selection out of what it hides.
    fn close(&mut self, toggle: BlockId) {
        self.closed.insert(toggle);
        let doc = self.editor.document();
        let hidden = |block: &BlockId| {
            let mut at = doc.parent(*block).ok().flatten();
            while let Some(parent) = at {
                if parent == toggle {
                    return true;
                }
                at = doc.parent(parent).ok().flatten();
            }
            false
        };
        let end = Pos::new(toggle, doc.text_len(toggle).unwrap_or(0));
        let taken = match self.editor.selection() {
            Some(Selection::Text { anchor, head })
                if hidden(&anchor.block) || hidden(&head.block) =>
            {
                Selection::caret(end)
            }
            // A cell is in its row, which is in the table the toggle hides.
            Some(Selection::Cells { anchor, head }) if hidden(anchor) || hidden(head) => {
                Selection::caret(end)
            }
            Some(Selection::Blocks(blocks)) if blocks.iter().any(hidden) => {
                self.block_range = Some((toggle, toggle));
                Selection::Blocks(vec![toggle])
            }
            _ => return,
        };
        self.editor.set_selection(Some(taken)).ok();
    }

    /// Opens the toggles around the selection, so that it stays in sight.
    pub(super) fn open_around_selection(&mut self) {
        let doc = self.editor.document();
        let inner = match self.editor.selection() {
            Some(Selection::Text { anchor, head }) => vec![anchor.block, head.block],
            Some(Selection::Blocks(blocks)) => blocks.clone(),
            _ => Vec::new(),
        };
        for block in inner {
            let mut at = doc.parent(block).ok().flatten();
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
        doc.set_attr(first, attr::START, 3).unwrap();
        let toggle = doc.insert_block(None, 2, "toggle_list_item").unwrap();
        let inside = doc.insert_block(Some(toggle), 0, "paragraph").unwrap();

        let shown = |closed: &HashSet<BlockId>| {
            let rows = rows(&doc, closed, &HashSet::new());
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
        // A quote's text and what is in it are italic, and nothing after it.
        let quoted: Vec<_> = rows(&doc, &HashSet::new(), &HashSet::new())
            .iter()
            .filter(|row| row.quoted)
            .map(|row| row.block)
            .collect();
        assert_eq!(quoted, [quote, first, second]);
        let closed = HashSet::from([toggle]);
        assert!(!shown(&closed).iter().any(|row| row.0 == inside));
    }
}
