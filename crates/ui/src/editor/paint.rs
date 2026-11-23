//! The selection behind the text, and the caret in front of it.

use gpui::{
    App, Bounds, Entity, Hsla, IntoElement, Pixels, TextLayout, Window, canvas, fill, hsla, point,
    prelude::*, px, size,
};
use lotype_core::Selection;

use super::EditorView;

fn selection_color() -> Hsla {
    hsla(0.58, 0.9, 0.6, 0.3)
}

/// Rectangles covering `start..end` of a block's text, one per line.
/// `wide` gives an empty range some width, for an empty block in a
/// selection that goes through it.
fn rectangles(layout: &TextLayout, start: usize, end: usize, wide: bool) -> Vec<Bounds<Pixels>> {
    let bounds = layout.bounds();
    let line = layout.line_height();
    let (Some(from), Some(to)) = (
        layout.position_for_index(start),
        layout.position_for_index(end),
    ) else {
        return Vec::new();
    };
    let row = |left: Pixels, right: Pixels, top: Pixels| {
        let width = if wide {
            (right - left).max(px(4.))
        } else {
            right - left
        };
        Bounds::new(point(left, top), size(width, line))
    };
    if from.y == to.y {
        return vec![row(from.x, to.x, from.y)];
    }
    let mut rows = vec![row(from.x, bounds.right(), from.y)];
    let mut top = from.y + line;
    while top < to.y {
        rows.push(row(bounds.left(), bounds.right(), top));
        top += line;
    }
    rows.push(row(bounds.left(), to.x, to.y));
    rows
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
            let end = if *block == to.block {
                to.offset
            } else {
                layout.len()
            };
            let wide = from.block != to.block;
            all.extend(rectangles(layout, start, end, wide));
        }
        all
    }

    fn caret(&self) -> Option<Bounds<Pixels>> {
        let shown = self.shown()?;
        let layout = self.layouts.get(&shown.block)?;
        let (selected, backward) = &shown.selected;
        let selecting = self
            .editor
            .selection()
            .is_some_and(|selection| !selection.is_collapsed());
        if selecting && self.composition.is_none() {
            return None;
        }
        let at = if *backward {
            selected.start
        } else {
            selected.end
        };
        let at = layout.position_for_index(at)?;
        Some(Bounds::new(at, size(px(1.5), layout.line_height())))
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

/// Paints the caret, after the text, while the editor has focus.
pub(super) fn caret(view: Entity<EditorView>) -> impl IntoElement {
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
    canvas(|_, _, _| (), paint).absolute().size_full()
}
