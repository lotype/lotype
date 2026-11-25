//! The formatting toolbar's dropdowns: the type of the selected blocks, and
//! the colors of the selected text. A dropdown hangs under the bar, in
//! front of it and the text, and closes on a choice, a click elsewhere or
//! a key.

use std::collections::BTreeMap;

use gpui::{AnyElement, ClickEvent, Context, MouseButton, MouseDownEvent, div, prelude::*, px};
use gpui_component::button::{Button, ButtonVariants};
use gpui_component::list::ListItem;
use gpui_component::{ActiveTheme, Selectable, Sizable, ThemeStyled, h_flex, v_flex};
use lotype_core::{BlockId, Role, SlashItem, Value, default_block_types, mark};

use super::EditorView;
use super::block_menu::swatch_row;
use super::toolbar::{Dropdown, measure, tagged};
use crate::icons::{self, icon};

fn stop(_: &MouseDownEvent, _: &mut gpui::Window, cx: &mut gpui::App) {
    cx.stop_propagation();
}

impl EditorView {
    fn toggle_dropdown(&mut self, dropdown: Dropdown) {
        let open = self.toolbar.menu != Some(dropdown);
        self.toolbar.menu = open.then_some(dropdown);
    }

    /// The open dropdown's menu, made by `menu` where it is `dropdown`.
    fn dropdown(
        &self,
        dropdown: Dropdown,
        menu: impl FnOnce() -> AnyElement,
    ) -> Option<AnyElement> {
        (self.toolbar.menu == Some(dropdown)).then(menu)
    }

    /// The select of the block type, when the block the selection starts in
    /// has a type it offers, and its menu while open.
    pub(super) fn block_type_select(
        &self,
        first: BlockId,
        cx: &mut Context<Self>,
    ) -> Option<(AnyElement, Option<AnyElement>)> {
        let doc = self.editor.document();
        let types: Vec<_> = default_block_types()
            .into_iter()
            .filter(|item| doc.schema().block_spec(&item.block_type).is_some())
            .collect();
        let current = types.iter().position(|item| item.describes(doc, first))?;
        let svg = icons::entry(&types[current].key).unwrap_or(icons::PARAGRAPH);
        let title = types[current].title.clone();
        let toggle = cx.listener(|view: &mut Self, _: &ClickEvent, _, cx| {
            view.toggle_dropdown(Dropdown::BlockType);
            cx.notify();
        });
        let trigger = Button::new("block-type")
            .ghost()
            .small()
            .tab_stop(false)
            .icon(icon(svg))
            .label(title)
            // gpui-component's own caret is an asset the application may lack.
            .child(icon(icons::CHEVRON_DOWN).size_3())
            .selected(self.toolbar.menu == Some(Dropdown::BlockType))
            .on_click(toggle);
        // Code is no text to turn into another, nor text into code here.
        let schema = doc.schema();
        let code = |item: &SlashItem| {
            schema
                .block_spec(&item.block_type)
                .is_some_and(|spec| spec.role() == Role::Code)
        };
        let items: Vec<_> = types
            .into_iter()
            .enumerate()
            .filter(|(_, item)| !code(item))
            .map(|(index, item)| {
                let svg = icons::entry(&item.key).unwrap_or(icons::PARAGRAPH);
                let title = item.title.clone();
                let choose = cx.listener(move |view: &mut Self, _: &ClickEvent, window, cx| {
                    if view.ai.session.is_some() {
                        return;
                    }
                    view.toolbar.menu = None;
                    view.editor.set_block(&item.block_type, &item.attrs).ok();
                    view.selection_changed(window, cx);
                });
                let chosen = index == current;
                let row = h_flex()
                    .w_full()
                    .gap_2()
                    .text_sm()
                    .items_center()
                    .child(icon(svg))
                    .child(div().flex_1().child(title))
                    .children(chosen.then(|| icon(icons::CHECK)));
                let item = ListItem::new(("block-type", index))
                    .child(row)
                    .on_click(choose);
                div()
                    .debug_selector(move || format!("block-type-{index}"))
                    .child(item)
            })
            .collect();
        let menu = || {
            v_flex()
                .popover_style(cx)
                .occlude()
                .p_1()
                .w(px(180.))
                .on_mouse_down(MouseButton::Left, stop)
                .children(items)
                .into_any_element()
        };
        let trigger = div().on_mouse_down(MouseButton::Left, stop).child(trigger);
        let menu = self.dropdown(Dropdown::BlockType, menu);
        Some((tagged("block-type", trigger).into_any_element(), menu))
    }

    /// The button of the text and background colors of the selected text,
    /// where the schema has those marks, and its menu while open.
    pub(super) fn color_button(
        &self,
        active: &BTreeMap<String, Value>,
        cx: &mut Context<Self>,
    ) -> Option<(AnyElement, Option<AnyElement>)> {
        let schema = self.editor.document().schema();
        let marks =
            [mark::TEXT_COLOR, mark::BACKGROUND_COLOR].map(|name| schema.mark_spec(name).is_some());
        if marks == [false, false] {
            return None;
        }
        let toggle = cx.listener(|view: &mut Self, _: &ClickEvent, _, cx| {
            view.toggle_dropdown(Dropdown::Color);
            cx.notify();
        });
        let trigger = Button::new("color")
            .ghost()
            .small()
            .tab_stop(false)
            .icon(icon(icons::COLOR))
            .child(icon(icons::CHEVRON_DOWN).size_3())
            .tooltip("Colors")
            .selected(self.toolbar.menu == Some(Dropdown::Color))
            .on_click(toggle);
        let current = |name: &str| match active.get(name) {
            Some(Value::Str(color)) => Some(color.clone()),
            _ => None,
        };
        let muted = cx.theme().muted_foreground;
        let title = move |text: &'static str| {
            div()
                .px_2()
                .pt_1()
                .text_size(px(12.))
                .text_color(muted)
                .child(text)
        };
        let mut rows = Vec::new();
        for (mark, shown, label) in [
            (mark::TEXT_COLOR, marks[0], "Text"),
            (mark::BACKGROUND_COLOR, marks[1], "Background"),
        ] {
            if !shown {
                continue;
            }
            let pick = move |view: &mut Self, name: Option<&'static str>| {
                view.editor.set_mark(mark, name.map(Value::from)).ok();
                view.toolbar.menu = None;
            };
            let selector = if mark == mark::TEXT_COLOR {
                "mark-text_color"
            } else {
                "mark-background_color"
            };
            rows.push(title(label).into_any_element());
            let current = current(mark);
            rows.push(
                swatch_row(
                    current.as_deref(),
                    mark == mark::BACKGROUND_COLOR,
                    selector,
                    pick,
                    cx,
                )
                .into_any_element(),
            );
        }
        let menu = move || {
            v_flex()
                .popover_style(cx)
                .occlude()
                .p_1()
                .on_mouse_down(MouseButton::Left, stop)
                .children(rows)
                .into_any_element()
        };
        // Its dropdown hangs from it: where it is, the bar learns from this.
        let trigger = div()
            .relative()
            .on_mouse_down(MouseButton::Left, stop)
            .child(trigger)
            .child(measure(self.toolbar.color_button.clone()));
        let menu = self.dropdown(Dropdown::Color, menu);
        Some((tagged("color", trigger).into_any_element(), menu))
    }
}
