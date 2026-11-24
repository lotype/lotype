//! The block menu, opened by a click on the side menu's handle, as
//! BlockNote's drag handle menu: Delete, and the block's text and
//! background colors, BlockNote's named ones.

use gpui::{
    AnyElement, ClickEvent, Context, Div, MouseButton, MouseDownEvent, anchored, deferred, div,
    prelude::*, px, rgb,
};
use gpui_component::list::ListItem;
use gpui_component::{ThemeStyled, v_flex};
use lotype_core::{BlockId, Selection, Value};

use super::EditorView;
use crate::marks::{COLORS, color};

const TEXT_COLOR: &str = "text_color";
const BACKGROUND_COLOR: &str = "background_color";

impl EditorView {
    /// Deletes the selected blocks where `block` is one of them, or else
    /// `block` alone, as BlockNote does.
    fn delete_from_menu(&mut self, block: BlockId) {
        let selected = matches!(
            self.editor.selection(),
            Some(Selection::Blocks(blocks)) if blocks.contains(&block)
        );
        if selected
            || self
                .editor
                .set_selection(Some(Selection::Blocks(vec![block])))
                .is_ok()
        {
            self.delete_selection();
        }
    }

    /// Gives `block` the color named `name` under `key`, or none for `None`.
    fn color_block(&mut self, block: BlockId, key: &'static str, name: Option<&'static str>) {
        self.editor
            .apply(|doc, selection| {
                match name {
                    Some(name) => doc.set_attr(block, key, name)?,
                    None => doc.remove_attr(block, key)?,
                }
                Ok(selection.cloned())
            })
            .ok();
    }

    /// A row of swatches for the block's colors under `key`, the first one
    /// the default.
    fn swatches(&self, block: BlockId, key: &'static str, cx: &mut Context<Self>) -> Div {
        let doc = self.editor.document();
        let current = match doc.attr(block, key) {
            Ok(Some(Value::Str(name))) => Some(name),
            _ => None,
        };
        let background = key == BACKGROUND_COLOR;
        let names = std::iter::once(None).chain(COLORS.iter().map(|(name, ..)| Some(*name)));
        let swatches = names.map(|name| {
            let click = cx.listener(move |view: &mut Self, _: &MouseDownEvent, window, cx| {
                cx.stop_propagation();
                view.color_block(block, key, name);
                view.side_menu.menu = None;
                view.selection_changed(window, cx);
            });
            let fill = name.and_then(|name| color(&Value::Str(name.to_owned()), background));
            let mut swatch = div()
                .size(px(20.))
                .rounded_sm()
                .border_1()
                .border_color(rgb(0xd0d4d9))
                .flex()
                .items_center()
                .justify_center()
                .text_size(px(12.))
                .cursor_pointer()
                .debug_selector(move || format!("{key}-{}", name.unwrap_or("default")))
                .on_mouse_down(MouseButton::Left, click);
            swatch = match fill {
                Some(fill) if background => swatch.bg(fill),
                Some(fill) => swatch.text_color(fill).child("A"),
                None => swatch.child("A"),
            };
            if current.as_deref() == name {
                swatch = swatch.border_2().border_color(rgb(0x0969da));
            }
            swatch
        });
        div().flex().gap_1().px_2().pb_1().children(swatches)
    }

    /// The block menu for `block`, placed where it is rendered, when open.
    pub(super) fn block_menu(&self, block: BlockId, cx: &mut Context<Self>) -> Option<AnyElement> {
        if self.side_menu.menu != Some(block) {
            return None;
        }
        let delete = cx.listener(move |view: &mut Self, _: &ClickEvent, window, cx| {
            view.side_menu.menu = None;
            view.delete_from_menu(block);
            view.selection_changed(window, cx);
        });
        let close = cx.listener(|view: &mut Self, _: &MouseDownEvent, _, cx| {
            if view.side_menu.menu.take().is_some() {
                cx.notify();
            }
        });
        let title = |text: &'static str| {
            div()
                .px_2()
                .pt_1()
                .text_size(px(12.))
                .text_color(rgb(0x8a8f98))
                .child(text)
        };
        let menu = v_flex()
            .popover_style(cx)
            .occlude()
            .p_1()
            .on_mouse_down_out(close)
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(
                div()
                    .debug_selector(|| "block-menu-delete".to_owned())
                    .child(
                        ListItem::new("block-menu-delete")
                            .child("Delete")
                            .on_click(delete),
                    ),
            )
            .child(title("Text"))
            .child(self.swatches(block, TEXT_COLOR, cx))
            .child(title("Background"))
            .child(self.swatches(block, BACKGROUND_COLOR, cx));
        Some(
            deferred(anchored().child(menu))
                .with_priority(1)
                .into_any_element(),
        )
    }
}
