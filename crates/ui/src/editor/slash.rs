//! The slash menu: the blocks a `/` typed at the start of a word can turn
//! into, filtered by what follows it. It stays open while text is typed, so
//! the editor keeps the focus: the arrows, Enter, Tab and Escape are the
//! menu's while it shows. gpui-component's popover would take the focus, so
//! the menu is its list items, placed as `popup` places them.

use gpui::{AnyElement, Context, MouseButton, ScrollHandle, Window, div, prelude::*, px};
use gpui_component::list::ListItem;
use gpui_component::{ActiveTheme, ThemeStyled, h_flex, v_flex};
use lotype_core::{BlockId, Command, SlashItem, SlashQuery, filter_slash_items};

use super::EditorView;
use super::keys::shortcut_label;
use super::popup::{Placement, popup};
use crate::icons::{self, icon};

/// The menu's state between frames.
#[derive(Default)]
pub(crate) struct SlashMenu {
    /// The entry Enter takes, among those that match.
    highlighted: usize,
    /// The block and query the highlight was chosen for.
    query: Option<(BlockId, String)>,
    /// The block whose slash Escape closed the menu for, until the query ends.
    dismissed: Option<BlockId>,
    /// The slash: the menu goes under it, or over it where there is no room below.
    pub(super) place: Placement,
    scroll: ScrollHandle,
}

impl EditorView {
    /// The query and the entries that match it, while the menu shows.
    pub(super) fn slash_matches(&self) -> Option<(SlashQuery, Vec<SlashItem>)> {
        if self.composition.is_some() {
            return None;
        }
        let query = self.editor.slash_query()?;
        if self.slash.dismissed == Some(query.block) {
            return None;
        }
        let items = filter_slash_items(&self.slash_items, &query.query);
        let items: Vec<_> = items.into_iter().cloned().collect();
        (!items.is_empty()).then_some((query, items))
    }

    /// Handles a key while the menu shows. Returns whether it did.
    pub(super) fn slash_key(&mut self, name: &str) -> bool {
        let Some((query, items)) = self.slash_matches() else {
            return false;
        };
        let count = items.len();
        let highlighted = self.slash.highlighted.min(count - 1);
        match name {
            "ArrowDown" => self.highlight((highlighted + 1) % count),
            "ArrowUp" => self.highlight((highlighted + count - 1) % count),
            "Enter" | "Tab" => self.accept_slash(&items[highlighted]),
            "Escape" => self.slash.dismissed = Some(query.block),
            _ => return false,
        }
        true
    }

    fn highlight(&mut self, at: usize) {
        self.slash.highlighted = at;
        // The list has a heading before each group's first entry.
        let Some((_, items)) = self.slash_matches() else {
            return;
        };
        let shown = &items[..=at.min(items.len() - 1)];
        let headings = 1 + shown
            .windows(2)
            .filter(|pair| pair[0].group != pair[1].group)
            .count();
        self.slash.scroll.scroll_to_item(at + headings);
    }

    fn accept_slash(&mut self, item: &SlashItem) {
        self.editor.accept_slash_item(item).ok();
        self.slash.highlighted = 0;
    }

    /// The slash, as last laid out.
    pub(super) fn slash_target(&self) -> Option<gpui::Bounds<gpui::Pixels>> {
        let query = self.editor.slash_query()?;
        self.caret_bounds(lotype_core::Pos::new(query.block, query.range.start))
    }

    /// Notes where the slash is, at the start of a frame. The slash stays
    /// where it is while the query is typed after it.
    pub(super) fn place_slash_menu(&mut self) {
        let query = self.editor.slash_query();
        self.slash.place.update(self.slash_target());
        // The menu closed by Escape opens for the next query.
        let block = query.as_ref().map(|query| query.block);
        if self.slash.dismissed != block {
            self.slash.dismissed = None;
        }
        // A new query starts at the first entry, at the top of the list.
        let query = query.map(|query| (query.block, query.query));
        if self.slash.query != query {
            self.slash.query = query;
            self.slash.highlighted = 0;
            self.slash.scroll.set_offset(Default::default());
        }
    }

    /// The menu, when it shows.
    pub(super) fn slash_menu(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if !self.focus.is_focused(window) {
            return None;
        }
        let (_, items) = self.slash_matches()?;
        let at = self.slash.place.get(window)?;
        let highlighted = self.slash.highlighted.min(items.len() - 1);
        let muted = cx.theme().muted_foreground;
        let mut rows = Vec::new();
        let mut group = None;
        for (index, item) in items.into_iter().enumerate() {
            // Each group is headed once: the items come grouped.
            if group.as_ref() != Some(&item.group) {
                group = Some(item.group.clone());
                let heading = div().px_2().pt_2().pb_1().text_xs().text_color(muted);
                rows.push(heading.child(item.group.clone()).into_any_element());
            }
            let shortcut = self.shortcut_for(&item).map(|keys| {
                let badge = div()
                    .px_1()
                    .rounded_sm()
                    .bg(cx.theme().list_active)
                    .text_xs()
                    .text_color(muted);
                badge.child(keys)
            });
            let tile = div()
                .size(px(32.))
                .flex_none()
                .rounded_md()
                .bg(cx.theme().muted)
                .flex()
                .items_center()
                .justify_center()
                .children(icons::entry(&item.key).map(icon));
            let words = div()
                .flex_1()
                .min_w_0()
                .child(div().text_sm().child(item.title.clone()))
                .child(
                    div()
                        .text_xs()
                        .text_color(muted)
                        .child(item.subtext.clone()),
                );
            let row = h_flex()
                .w_full()
                .gap_2()
                .items_center()
                .child(tile)
                .child(words)
                .children(shortcut);
            let key = item.key.clone();
            let click = cx.listener(move |view: &mut Self, _: &gpui::ClickEvent, window, cx| {
                view.accept_slash(&item);
                view.selection_changed(window, cx);
            });
            let entry = ListItem::new(("slash-item", index))
                .selected(index == highlighted)
                .child(row)
                .on_click(click);
            let entry = div()
                .debug_selector(move || format!("slash-{key}"))
                .child(entry);
            rows.push(entry.into_any_element());
        }
        let list = div()
            .id("slash-menu")
            .debug_selector(|| "slash-list".to_owned())
            .track_scroll(&self.slash.scroll)
            .overflow_y_scroll()
            .max_h(px(360.))
            .children(rows);
        let menu = v_flex()
            .popover_style(cx)
            .occlude()
            .w(px(320.))
            .p_1()
            // A click on the menu is no click in the text.
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(list);
        Some(popup(at, false, menu))
    }

    /// The keys bound to what `item` makes, as the keymap has them.
    fn shortcut_for(&self, item: &SlashItem) -> Option<String> {
        let binding = self.keymap.iter().find(|binding| {
            matches!(&binding.command, Command::SetBlock { block_type, attrs }
                if *block_type == item.block_type && *attrs == item.attrs)
        })?;
        Some(shortcut_label(&binding.key))
    }
}
