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
        let current = match self.editor.document().attr(block, key) {
            Ok(Some(Value::Str(name))) => Some(name),
            _ => None,
        };
        let pick = move |view: &mut Self, name: Option<&'static str>| {
            view.color_block(block, key, name);
            view.side_menu.menu = None;
        };
        swatch_row(current.as_deref(), key == BACKGROUND_COLOR, key, pick, cx)
    }

    /// The block menu for `block`, placed where it is rendered, when open.
    pub(super) fn block_menu(&self, block: BlockId, cx: &mut Context<Self>) -> Option<AnyElement> {
        if self.side_menu.menu != Some(block) {
            return None;
        }
        // Code, math and diagrams have colors of their own.
        let doc = self.editor.document();
        let spec = doc
            .block_type(block)
            .ok()
            .and_then(|kind| doc.schema().block_spec(&kind).cloned());
        let colored = spec.is_some_and(|spec| spec.takes_style());
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
            .when(colored, |menu| {
                menu.child(title("Text"))
                    .child(self.swatches(block, TEXT_COLOR, cx))
                    .child(title("Background"))
                    .child(self.swatches(block, BACKGROUND_COLOR, cx))
            });
        Some(
            deferred(anchored().child(menu))
                .with_priority(1)
                .into_any_element(),
        )
    }
}

/// A row of swatches for BlockNote's colors, the first one the default, with
/// `current` marked; `pick` gets the name clicked, or `None` for the default.
/// Each swatch's debug selector is `{selector}-{name}`.
pub(super) fn swatch_row(
    current: Option<&str>,
    background: bool,
    selector: &'static str,
    pick: impl Fn(&mut EditorView, Option<&'static str>) + Clone + 'static,
    cx: &mut Context<EditorView>,
) -> Div {
    let names = std::iter::once(None).chain(COLORS.iter().map(|(name, ..)| Some(*name)));
    let swatches = names.map(|name| {
        let pick = pick.clone();
        let click = cx.listener(
            move |view: &mut EditorView, _: &MouseDownEvent, window, cx| {
                cx.stop_propagation();
                pick(view, name);
                view.selection_changed(window, cx);
            },
        );
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
            .debug_selector(move || format!("{selector}-{}", name.unwrap_or("default")))
            .on_mouse_down(MouseButton::Left, click);
        swatch = match fill {
            Some(fill) if background => swatch.bg(fill),
            Some(fill) => swatch.text_color(fill).child("A"),
            None => swatch.child("A"),
        };
        if current == name {
            swatch = swatch.border_2().border_color(rgb(0x0969da));
        }
        swatch
    });
    div().flex().gap_1().px_2().pb_1().children(swatches)
}
