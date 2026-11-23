//! The selection behind the text, and the caret in front of it.

use gpui::{
    App, Bounds, Entity, Hsla, IntoElement, Pixels, Window, canvas, fill, hsla, prelude::*,
};
use lotype_core::Selection;

use super::EditorView;
use crate::text_el::Affinity;

fn selection_color() -> Hsla {
    hsla(0.58, 0.9, 0.6, 0.3)
}

impl EditorView {
    fn selection_rectangles(&self) -> Vec<Bounds<Pixels>> {
        let Some(Selection::Text { anchor, head }) = self.editor.selection() else {
            return Vec::new();
        };
        // Text being composed is underlined instead.
        if anchor == head || self.composition.is_some() {
            return Vec::new();
        }
        let Ok((from, to)) = self.editor.document().ordered(*anchor, *head) else {
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

/// Paints the selection, before the text so that it stays readable.
pub(super) fn selection(view: Entity<EditorView>) -> impl IntoElement {
    let paint = move |_: Bounds<Pixels>, _: (), window: &mut Window, cx: &mut App| {
        for bounds in view.read(cx).selection_rectangles() {
            window.paint_quad(fill(bounds, selection_color()));
        }
    };
    canvas(|_, _, _| (), paint).absolute().size_full()
}

/// Paints the caret, after the text, while the editor has focus. Laid out
/// first, it scrolls the editor to bring the caret into sight when asked to.
pub(super) fn caret(view: Entity<EditorView>) -> impl IntoElement {
    let revealed = view.clone();
    let prepaint = move |_: Bounds<Pixels>, window: &mut Window, cx: &mut App| {
        let view = revealed.read(cx);
        if !view.reveal.replace(false) {
            return;
        }
        // The head of a range, which is where the selection grows.
        let head = match view.editor.selection() {
            Some(Selection::Text { head, .. }) => view.caret_bounds(*head),
            _ => None,
        };
        let (Some(caret), scroll) = (view.caret().or(head), &view.scroll) else {
            return;
        };
        let (shown, mut offset) = (scroll.bounds(), scroll.offset());
        if caret.top() < shown.top() {
            offset.y += shown.top() - caret.top();
        } else if caret.bottom() > shown.bottom() {
            offset.y -= caret.bottom() - shown.bottom();
        } else {
            return;
        }
        offset.y = offset.y.min(Pixels::ZERO).max(-scroll.max_offset().y);
        scroll.set_offset(offset);
        // The text is placed already: draw again, as a refresh while
        // drawing is dropped.
        window.defer(cx, |window, _| window.refresh());
    };
    let paint = move |_: Bounds<Pixels>, _: (), window: &mut Window, cx: &mut App| {
        let view = view.read(cx);
        if !view.focus.is_focused(window) {
            return;
        }
        let color = window.text_style().color;
        if let Some(bounds) = view.caret() {
            window.paint_quad(fill(bounds, color));
        }
    };
    canvas(prepaint, paint).absolute().size_full()
}
