//! The slash menu: the blocks a `/` typed at the start of a word can turn
//! into, filtered by what follows it. It stays open while text is typed, so
//! the editor keeps the focus: the arrows, Enter, Tab and Escape are the
//! menu's while it shows. gpui-component's popover would take the focus, so
//! the menu is its list items in GPUI's own `anchored` place.

use gpui::{
    Anchor, AnyElement, Bounds, Context, MouseButton, Pixels, ScrollHandle, Window, anchored,
    deferred, div, point, prelude::*, px,
};
use gpui_component::list::ListItem;
use gpui_component::{ThemeStyled, v_flex};
use lotype_core::{BlockId, SlashItem, SlashQuery, filter_slash_items};

use super::EditorView;

/// The menu's state between frames.
#[derive(Default)]
pub(crate) struct SlashMenu {
    /// The entry Enter takes, among those that match.
    highlighted: usize,
    /// The block and query the highlight was chosen for.
    query: Option<(BlockId, String)>,
    /// The block whose slash Escape closed the menu for, until the query ends.
    dismissed: Option<BlockId>,
    /// The slash, as it was last laid out: the menu goes under it, or over
    /// it where there is no room below.
    at: Option<Bounds<Pixels>>,
    /// The slash moved in the last layout, so the menu needs another frame.
    moved: bool,
    scroll: ScrollHandle,
}

impl EditorView {
    /// The query and the entries that match it, while the menu shows.
    fn slash_matches(&self) -> Option<(SlashQuery, Vec<SlashItem>)> {
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
        self.slash.scroll.scroll_to_item(at);
    }

    fn accept_slash(&mut self, item: &SlashItem) {
        self.editor.accept_slash_item(item).ok();
        self.slash.highlighted = 0;
    }

    /// Notes where the slash is, from the layout of the frame before, which
    /// is still there when the frame starts. The slash stays where it is
    /// while the query is typed after it.
    pub(super) fn place_slash_menu(&mut self) {
        let query = self.editor.slash_query();
        let slash = query
            .as_ref()
            .map(|query| lotype_core::Pos::new(query.block, query.range.start));
        let at = slash.and_then(|slash| self.caret_bounds(slash));
        // A scroll or another width moves the slash after the menu is placed.
        self.slash.moved = at != self.slash.at;
        self.slash.at = at;
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
        let Some(at) = self.slash.at.filter(|_| !self.slash.moved) else {
            // The slash is laid out in this frame: the menu goes there in the next.
            window.request_animation_frame();
            return None;
        };
        let highlighted = self.slash.highlighted.min(items.len() - 1);
        let entries = items.into_iter().enumerate().map(|(index, item)| {
            let title = div().child(item.title.clone());
            let click = cx.listener(move |view: &mut Self, _: &gpui::ClickEvent, window, cx| {
                view.accept_slash(&item);
                view.selection_changed(window, cx);
            });
            ListItem::new(("slash-item", index))
                .selected(index == highlighted)
                .child(title)
                .on_click(click)
        });
        let list = div()
            .id("slash-menu")
            .track_scroll(&self.slash.scroll)
            .overflow_y_scroll()
            .max_h(px(320.))
            .children(entries);
        let menu = v_flex()
            .popover_style(cx)
            .occlude()
            .w(px(240.))
            .p_1()
            // A click on the menu is no click in the text.
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(list);
        let placed = anchored()
            .anchor(Anchor::TopLeft)
            .attach(at, Anchor::BottomLeft)
            .offset(point(px(0.), px(4.)))
            .snap_to_window_with_margin(px(8.))
            .child(menu);
        Some(deferred(placed).with_priority(1).into_any_element())
    }
}
