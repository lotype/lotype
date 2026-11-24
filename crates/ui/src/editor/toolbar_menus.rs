//! The formatting toolbar's dropdowns: the type of the selected blocks, and
//! the colors of the selected text. A dropdown hangs under the bar, in
//! front of it and the text, and closes on a choice, a click elsewhere or
//! a key.

use std::collections::BTreeMap;

use gpui::{
    AnyElement, ClickEvent, Context, MouseButton, MouseDownEvent, div, prelude::*, px, rgb,
};
use gpui_component::button::{Button, ButtonVariants};
use gpui_component::list::ListItem;
use gpui_component::{Sizable, ThemeStyled, h_flex, v_flex};
use lotype_core::{BlockId, MAX_HEADING_LEVEL, Value};

use super::EditorView;
use super::block_menu::swatch_row;
use super::toolbar::{Dropdown, tagged};
use crate::icons::{self, icon};

/// The types the select offers: a block type, a heading's level, an icon
/// and a title, as BlockNote's block type select has them.
const BLOCK_TYPES: [(&str, Option<i64>, &[u8], &str); 12] = [
    ("paragraph", None, icons::PARAGRAPH, "Paragraph"),
    ("heading", Some(1), icons::HEADING_1, "Heading 1"),
    ("heading", Some(2), icons::HEADING_2, "Heading 2"),
    ("heading", Some(3), icons::HEADING_3, "Heading 3"),
    ("heading", Some(4), icons::HEADING_4, "Heading 4"),
    ("heading", Some(5), icons::HEADING_5, "Heading 5"),
    ("quote", None, icons::QUOTE, "Quote"),
    ("toggle_list_item", None, icons::TOGGLE, "Toggle List"),
    ("bullet_list_item", None, icons::BULLET, "Bullet List"),
    ("numbered_list_item", None, icons::NUMBERED, "Numbered List"),
    ("check_list_item", None, icons::CHECK_LIST, "Check List"),
    // Last, so that it is found only for a code block.
    ("code_block", None, icons::CODE, "Code Block"),
];

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
        let kind = doc.block_type(first).ok()?;
        let level = match doc.attr(first, "level") {
            Ok(Some(Value::Int(level))) => Some(level.clamp(1, MAX_HEADING_LEVEL)),
            _ => Some(1),
        };
        let schema = doc.schema();
        let offered =
            |(name, ..): &(&str, Option<i64>, &[u8], &str)| schema.block_spec(name).is_some();
        let current = BLOCK_TYPES
            .iter()
            .position(|(name, at, ..)| *name == kind && (at.is_none() || *at == level))?;
        let (_, _, svg, title) = BLOCK_TYPES[current];
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
            .on_click(toggle);
        let items: Vec<_> = BLOCK_TYPES
            .iter()
            .enumerate()
            .filter(|(_, item)| offered(item) && item.0 != "code_block")
            .map(|(index, &(name, level, svg, title))| {
                let choose = cx.listener(move |view: &mut Self, _: &ClickEvent, window, cx| {
                    let mut attrs = Vec::new();
                    if let Some(level) = level {
                        attrs.push(("level".to_owned(), Value::Int(level)));
                    }
                    view.toolbar.menu = None;
                    view.editor.set_block(name, &attrs).ok();
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
                .mt_1()
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
        let marks = ["text_color", "background_color"].map(|name| schema.mark_spec(name).is_some());
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
            .on_click(toggle);
        let current = |name: &str| match active.get(name) {
            Some(Value::Str(color)) => Some(color.clone()),
            _ => None,
        };
        let title = |text: &'static str| {
            div()
                .px_2()
                .pt_1()
                .text_size(px(12.))
                .text_color(rgb(0x8a8f98))
                .child(text)
        };
        let mut rows = Vec::new();
        for (mark, shown, label) in [
            ("text_color", marks[0], "Text"),
            ("background_color", marks[1], "Background"),
        ] {
            if !shown {
                continue;
            }
            let pick = move |view: &mut Self, name: Option<&'static str>| {
                view.editor.set_mark(mark, name.map(Value::from)).ok();
                view.toolbar.menu = None;
            };
            let selector = if mark == "text_color" {
                "mark-text_color"
            } else {
                "mark-background_color"
            };
            rows.push(title(label).into_any_element());
            let current = current(mark);
            rows.push(
                swatch_row(
                    current.as_deref(),
                    mark == "background_color",
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
                .mt_1()
                .p_1()
                .on_mouse_down(MouseButton::Left, stop)
                .children(rows)
                .into_any_element()
        };
        let trigger = div().on_mouse_down(MouseButton::Left, stop).child(trigger);
        let menu = self.dropdown(Dropdown::Color, menu);
        Some((tagged("color", trigger).into_any_element(), menu))
    }
}
