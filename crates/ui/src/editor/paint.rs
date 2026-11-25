//! The selection behind the text, and the caret in front of it.

use gpui::{
    App, Bounds, Entity, Hsla, IntoElement, Pixels, Window, canvas, fill, point, prelude::*,
};
use gpui_component::ActiveTheme;
use lotype_core::{Pos, Selection};

use super::EditorView;
use crate::text_el::Affinity;

/// The theme's selection, which gpui-component keeps see-through enough to
/// read the text under it.
pub(super) fn selection_color(cx: &App) -> Hsla {
    cx.theme().selection
}

impl EditorView {
    fn selection_rectangles(&self) -> Vec<Bounds<Pixels>> {
        match self.editor.selection() {
            // Text being composed is underlined instead.
            Some(Selection::Text { anchor, head }) if self.composition.is_none() => {
                self.rectangles_between(*anchor, *head)
            }
            _ => Vec::new(),
        }
    }

    /// Where the text from one position to the other was drawn.
    pub(super) fn rectangles_between(&self, anchor: Pos, head: Pos) -> Vec<Bounds<Pixels>> {
        if anchor == head {
            return Vec::new();
        }
        let Ok((from, to)) = self.editor.document().ordered(anchor, head) else {
            return Vec::new();
        };
        let first = self.order.iter().position(|block| *block == from.block);
        let last = self.order.iter().position(|block| *block == to.block);
        let (Some(first), Some(last)) = (first, last) else {
            return Vec::new();
        };
        let mut all = Vec::new();
        for block in &self.order[first..=last] {
            let Some(layout) = self.layouts.get(block) else {
                continue;
            };
            let start = if *block == from.block { from.offset } else { 0 };
            // A block the selection goes on from is selected to the edge.
            let end = if *block == to.block {
                to.offset
            } else {
                usize::MAX
            };
            all.extend(layout.rectangles(start, end, from.block != to.block));
        }
        all
    }

    fn caret(&self) -> Option<Bounds<Pixels>> {
        let Some(Selection::Text { anchor, head }) = self.editor.selection() else {
            return None;
        };
        let Some(composition) = &self.composition else {
            return (anchor == head).then(|| self.caret_bounds(*head)).flatten();
        };
        // In text being composed, the caret is where the IME says.
        let shown = self.shown()?;
        let at = shown.marked?.start + composition.selected.end;
        self.layouts
            .get(&shown.block)?
            .caret(at, Affinity::Downstream)
    }
}

/// Paints the selection over the rows, whose colors would hide it, and
/// see-through, so that the text stays readable.
pub(super) fn selection(view: Entity<EditorView>) -> impl IntoElement {
    let paint = move |_: Bounds<Pixels>, _: (), window: &mut Window, cx: &mut App| {
        let color = selection_color(cx);
        for bounds in view.read(cx).selection_rectangles() {
            window.paint_quad(fill(bounds, color));
        }
    };
    canvas(|_, _, _| (), paint).absolute().size_full()
}

/// Scrolls the editor to bring the head of the selection into sight when
/// asked to, once the rows are laid out: the caret or a range's head, or the
/// row where a selection of blocks goes on.
fn reveal(view: &Entity<EditorView>, window: &mut Window, cx: &mut App) {
    let id = view.entity_id();
    let view = view.read(cx);
    if !view.reveal.replace(false) {
        return;
    }
    let scroll = &view.scroll;
    let target = match view.editor.selection() {
        Some(Selection::Text { head, .. }) => view.caret().or_else(|| view.caret_bounds(*head)),
        Some(Selection::Blocks(blocks)) => {
            let range = view.block_range.map(|(_, head)| head);
            let head = range
                .filter(|head| blocks.contains(head))
                .or(blocks.last().copied());
            let at = head.and_then(|head| view.rows.iter().position(|row| *row == head));
            // The scroll handle has the rows where they are before scrolling.
            let item = at.and_then(|at| view.row_item(at));
            let row = item.and_then(|item| scroll.bounds_for_item(item));
            row.map(|row| {
                Bounds::new(
                    row.origin + point(Pixels::ZERO, scroll.offset().y),
                    row.size,
                )
            })
        }
        _ => None,
    };
    let Some(target) = target else {
        return;
    };
    let (shown, mut offset) = (scroll.bounds(), scroll.offset());
    if target.top() < shown.top() {
        offset.y += shown.top() - target.top();
    } else if target.bottom() > shown.bottom() {
        offset.y -= target.bottom() - shown.bottom();
    } else {
        return;
    }
    offset.y = offset.y.min(Pixels::ZERO).max(-scroll.max_offset().y);
    scroll.set_offset(offset);
    // Everything is placed already: draw again in the next frame. A refresh
    // there is dropped, as the frame is drawing.
    window.on_next_frame(move |_, cx| cx.notify(id));
}

/// Paints the caret, after the text, while the editor has focus. Laid out
/// after the rows, it brings the selection into sight first.
pub(super) fn caret(view: Entity<EditorView>) -> impl IntoElement {
    let revealed = view.clone();
    let prepaint = move |_: Bounds<Pixels>, window: &mut Window, cx: &mut App| {
        reveal(&revealed, window, cx);
        // An edit that moves the text, as an alignment does, leaves the
        // popups placed by the last frame behind: they follow in the next.
        let view = revealed.read(cx);
        let behind = view.slash.place.left_behind(view.slash_target())
            || view.toolbar.place.left_behind(view.toolbar_target())
            || view.links.place.left_behind(view.links_target())
            || view.ai.place.left_behind(view.ai_prompt_target());
        if behind {
            let id = revealed.entity_id();
            window.on_next_frame(move |_, cx| cx.notify(id));
        }
    };
    let paint = move |_: Bounds<Pixels>, _: (), window: &mut Window, cx: &mut App| {
        let view = view.read(cx);
        if let Some(started) = view.blink.started.take() {
            view.blink.cost.set(started.elapsed());
        }
        if !view.focus.is_focused(window) || view.blink.dark {
            return;
        }
        // The theme's blue, as an accent-colored caret is on macOS, and as
        // the checked boxes are.
        let color = cx.theme().blue;
        if let Some(bounds) = view.caret() {
            window.paint_quad(fill(bounds, color));
        }
    };
    canvas(prepaint, paint).absolute().size_full()
}
