//! The block menu, opened by a click on the side menu's handle, as
//! BlockNote's drag handle menu: Delete, and the block's text and
//! background colors, BlockNote's named ones.

use gpui::{
    AnyElement, ClickEvent, Context, Div, MouseButton, MouseDownEvent, anchored, deferred, div,
    prelude::*, px,
};
use gpui_component::list::ListItem;
use gpui_component::{ActiveTheme, ThemeStyled, v_flex};
use lotype_core::{BlockColor, BlockId, COLOR_NAMES, Selection, Value};

use super::EditorView;
use crate::marks::color;

impl EditorView {
    /// Deletes the selected blocks where `block` is one of them, or else
    /// `block` alone, as BlockNote does.
    fn delete_from_menu(&mut self, block: BlockId) {
        if self.ai.session.is_some() {
            return;
        }
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

    /// A row of swatches for one of the block's colors, the first one the
    /// default.
    fn swatches(&self, block: BlockId, which: BlockColor, cx: &mut Context<Self>) -> Div {
        let current = self.editor.document().block_color(block, which);
        let pick = move |view: &mut Self, name: Option<&'static str>| {
            view.editor.set_block_color(block, which, name).ok();
            view.side_menu.menu = None;
        };
        let background = which == BlockColor::Background;
        swatch_row(current.as_deref(), background, which.key(), pick, cx)
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
            if let Some(block) = view.side_menu.menu.take() {
                view.side_menu.closed = Some(block);
                cx.notify();
            }
        });
        let muted = cx.theme().muted_foreground;
        let title = move |text: &'static str| {
            div()
                .px_2()
                .pt_1()
                .text_size(px(12.))
                .text_color(muted)
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
                    .child(self.swatches(block, BlockColor::Text, cx))
                    .child(title("Background"))
                    .child(self.swatches(block, BlockColor::Background, cx))
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
    let accent = cx.theme().primary;
    let dark = cx.theme().mode.is_dark();
    let names = std::iter::once(None).chain(COLOR_NAMES.map(Some));
    let swatches = names.map(|name| {
        let pick = pick.clone();
        let click = cx.listener(
            move |view: &mut EditorView, _: &MouseDownEvent, window, cx| {
                if view.ai.session.is_some() {
                    return;
                }
                cx.stop_propagation();
                pick(view, name);
                view.selection_changed(window, cx);
            },
        );
        let fill = name.and_then(|name| color(&Value::Str(name.to_owned()), background, dark));
        let mut swatch = div()
            .size(px(20.))
            .rounded_sm()
            .border_1()
            .border_color(cx.theme().border)
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
            swatch = swatch.border_2().border_color(accent);
        }
        swatch
    });
    div().flex().gap_1().px_2().pb_1().children(swatches)
}
