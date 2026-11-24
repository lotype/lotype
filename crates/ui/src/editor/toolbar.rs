//! The formatting toolbar over a text selection, as BlockNote's: the type
//! of the blocks, the marks, the alignment, the colors, nesting and the
//! link. Its buttons never take the focus, so the selection stays as it is.

use gpui::{
    Anchor, AnyElement, ClickEvent, Context, Div, MouseButton, Window, anchored, div, prelude::*,
    px, rgb,
};
use gpui_component::button::{Button, ButtonVariants};
use gpui_component::{Selectable, Sizable, ThemeStyled, h_flex};
use lotype_core::{BlockId, Pos, Selection, Value};

use super::EditorView;
use super::popup::{Placement, popup};
use crate::icons::{self, icon};

/// The marks on the toolbar, with their icons and names.
const MARKS: [(&str, &[u8], &str); 5] = [
    ("bold", icons::BOLD, "Bold"),
    ("italic", icons::ITALIC, "Italic"),
    ("underline", icons::UNDERLINE, "Underline"),
    ("strike", icons::STRIKE, "Strikethrough"),
    ("code", icons::CODE, "Code"),
];

/// The alignments, as `text_alignment` names them; left is none at all.
const ALIGNMENTS: [(&str, &[u8], &str); 3] = [
    ("left", icons::ALIGN_LEFT, "Align left"),
    ("center", icons::ALIGN_CENTER, "Align center"),
    ("right", icons::ALIGN_RIGHT, "Align right"),
];

/// The toolbar's state between frames.
#[derive(Default)]
pub(crate) struct Toolbar {
    /// The start of the selection, which the toolbar goes over.
    pub(super) place: Placement,
    /// The dropdown open from it.
    pub(super) menu: Option<Dropdown>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Dropdown {
    BlockType,
    Color,
}

/// A button of the toolbar: an icon, a tooltip, and a debug selector
/// `toolbar-{id}` for tests.
pub(super) fn button(id: &'static str, svg: &[u8], tip: &'static str) -> Button {
    Button::new(id)
        .ghost()
        .small()
        .tab_stop(false)
        .icon(icon(svg))
        .tooltip(tip)
}

pub(super) fn tagged(id: &'static str, button: impl IntoElement) -> Div {
    div()
        .debug_selector(move || format!("toolbar-{id}"))
        .child(button)
}

fn separator() -> Div {
    div().w(px(1.)).h(px(16.)).mx_1().bg(rgb(0xe1e4e8))
}

impl EditorView {
    /// The selected text, when it is some and not being dragged out.
    fn selected_text(&self) -> Option<(Pos, Pos)> {
        let Some(Selection::Text { anchor, head }) = self.editor.selection() else {
            return None;
        };
        // Marks go across no edge of a cell.
        let cell = anchor.block.column().or(head.block.column()).is_some();
        let shown = anchor != head && !cell && !self.dragging && self.composition.is_none();
        shown.then(|| self.editor.document().ordered(*anchor, *head).ok())?
    }

    /// The block the selection starts in, which the toolbar shows the type,
    /// the alignment and the style of.
    pub(super) fn first_selected_block(&self) -> Option<BlockId> {
        self.selected_text().map(|(from, _)| from.block)
    }

    /// The start of the selection, as last laid out.
    pub(super) fn toolbar_target(&self) -> Option<gpui::Bounds<gpui::Pixels>> {
        let (from, _) = self.selected_text()?;
        self.caret_bounds(from)
    }

    /// Notes where the selection starts, at the start of a frame.
    pub(super) fn place_toolbar(&mut self) {
        let at = self.toolbar_target();
        self.toolbar.place.update(at);
        if at.is_none() {
            self.toolbar.menu = None;
        }
    }

    /// The toolbar, when it shows.
    pub(super) fn toolbar(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if !self.focus.is_focused(window) || self.selected_text().is_none() {
            self.toolbar.menu = None;
            return None;
        }
        let at = self.toolbar.place.get(window)?;
        let doc = self.editor.document();
        let first = self.first_selected_block()?;
        let spec = doc.block_type(first).ok();
        let spec = spec.and_then(|kind| doc.schema().block_spec(&kind));
        let styled = spec.is_some_and(|spec| spec.takes_style());
        let active = self.editor.active_marks().unwrap_or_default();
        let schema = doc.schema();
        let marks = MARKS
            .into_iter()
            .filter(|(name, ..)| schema.mark_spec(name).is_some());
        let marks: Vec<_> = marks
            .map(|(name, svg, tip)| {
                let click = cx.listener(move |view: &mut Self, _: &ClickEvent, window, cx| {
                    view.editor.toggle_mark(name).ok();
                    view.selection_changed(window, cx);
                });
                let on = active.contains_key(name);
                let button = button(name, svg, tip).selected(on).toggled(on);
                tagged(name, button.on_click(click))
            })
            .collect();
        let alignment = match doc.attr(first, "text_alignment") {
            Ok(Some(Value::Str(alignment))) => alignment,
            _ => "left".to_owned(),
        };
        let alignments = ALIGNMENTS.into_iter().map(|(name, svg, tip)| {
            let click = cx.listener(move |view: &mut Self, _: &ClickEvent, window, cx| {
                let value = (name != "left").then(|| Value::from(name));
                view.editor.set_block_attr("text_alignment", value).ok();
                view.selection_changed(window, cx);
            });
            let on = alignment == name;
            tagged(name, button(name, svg, tip).selected(on).on_click(click))
        });
        let alignments: Vec<_> = if styled {
            alignments.collect()
        } else {
            Vec::new()
        };
        let nest = cx.listener(|view: &mut Self, _: &ClickEvent, window, cx| {
            view.editor.indent().ok();
            view.selection_changed(window, cx);
        });
        let unnest = cx.listener(|view: &mut Self, _: &ClickEvent, window, cx| {
            view.editor.outdent().ok();
            view.selection_changed(window, cx);
        });
        let link = schema.mark_spec("link").is_some().then(|| {
            let url = match active.get("link") {
                Some(Value::Str(url)) => url.clone(),
                _ => String::new(),
            };
            let linked = !url.is_empty();
            let click = cx.listener(move |view: &mut Self, _: &ClickEvent, window, cx| {
                view.open_link_field(&url, window, cx);
            });
            tagged(
                "link",
                button("link", icons::LINK, "Link")
                    .selected(linked)
                    .on_click(click),
            )
        });
        let (block_type, type_menu) = self.block_type_select(first, cx).unzip();
        let (color, color_menu) = self.color_button(&active, cx).unzip();
        // The open dropdown comes last, so that it is in front of the bar's
        // buttons and takes the clicks on itself: under the select from the
        // left, under the color button from the right.
        let left = type_menu.flatten().map(|menu| (menu, false));
        let dropdown = left.or(color_menu.flatten().map(|menu| (menu, true)));
        let dropdown = dropdown.map(|(menu, right)| {
            let hanging = div().absolute().top_full();
            let (hanging, anchor) = if right {
                (hanging.right_0(), Anchor::TopRight)
            } else {
                (hanging.left_0(), Anchor::TopLeft)
            };
            let placed = anchored().anchor(anchor).snap_to_window_with_margin(px(8.));
            hanging.child(placed.child(menu))
        });
        let bar = h_flex()
            .relative()
            .popover_style(cx)
            .occlude()
            .p_1()
            .gap_0p5()
            .items_center()
            // A click on the toolbar is no click in the text, and closes a
            // dropdown, which takes clicks on itself and its button first.
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|view: &mut Self, _: &gpui::MouseDownEvent, _, cx| {
                    cx.stop_propagation();
                    if view.toolbar.menu.take().is_some() {
                        cx.notify();
                    }
                }),
            )
            .children(
                block_type
                    .map(|select| div().flex().items_center().child(select).child(separator())),
            )
            .children(marks)
            .when(!alignments.is_empty(), |bar| {
                bar.child(separator()).children(alignments)
            })
            .children(
                color.map(|color| div().flex().items_center().child(separator()).child(color)),
            )
            .child(separator())
            .child(tagged(
                "nest",
                button("nest", icons::NEST, "Nest block").on_click(nest),
            ))
            .child(tagged(
                "unnest",
                button("unnest", icons::UNNEST, "Unnest block").on_click(unnest),
            ))
            .children(link)
            .children(dropdown);
        Some(popup(at, true, bar))
    }
}
