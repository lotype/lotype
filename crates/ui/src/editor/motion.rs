//! Moving the cursor and selecting, with keys and the mouse.

use gpui::{
    Context, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels, Point, Window,
    point, px,
};
use lotype_core::{BlockId, Pos, Selection};
use unicode_segmentation::UnicodeSegmentation;

use super::EditorView;
use crate::text_el::{Affinity, TextLayout};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Motion {
    Left,
    Right,
    WordLeft,
    WordRight,
    LineStart,
    LineEnd,
    Up,
    Down,
    DocStart,
    DocEnd,
}

/// The motion a key name asks for, and whether it extends the selection.
/// The bindings are those of macOS text fields.
pub(crate) fn motion(name: &str) -> Option<(Motion, bool)> {
    let extend = name.contains("Shift-");
    let name = name.replacen("Shift-", "", 1);
    let motion = match name.as_str() {
        "ArrowLeft" => Motion::Left,
        "ArrowRight" => Motion::Right,
        "Alt-ArrowLeft" => Motion::WordLeft,
        "Alt-ArrowRight" => Motion::WordRight,
        "Mod-ArrowLeft" | "Home" => Motion::LineStart,
        "Mod-ArrowRight" | "End" => Motion::LineEnd,
        "ArrowUp" => Motion::Up,
        "ArrowDown" => Motion::Down,
        "Mod-ArrowUp" => Motion::DocStart,
        "Mod-ArrowDown" => Motion::DocEnd,
        _ => return None,
    };
    Some((motion, extend))
}

impl EditorView {
    fn layout(&self, block: BlockId) -> Option<&TextLayout> {
        self.layouts.get(&block)
    }

    fn text_len(&self, block: BlockId) -> usize {
        self.editor.document().text_len(block).unwrap_or(0)
    }

    /// The place in the text under a point, or nearest to it, and the side
    /// of a soft wrap it is on.
    pub(crate) fn hit(&self, at: Point<Pixels>) -> Option<(Pos, Affinity)> {
        let distance = |block: &BlockId| {
            let bounds = self.layout(*block)?.bounds()?;
            let above = bounds.top() - at.y;
            let below = at.y - bounds.bottom();
            Some(above.max(below).max(Pixels::ZERO))
        };
        let nearest = self
            .order
            .iter()
            .filter_map(|block| Some((*block, distance(block)?)));
        let (block, _) = nearest.min_by(|a, b| f32::from(a.1).total_cmp(&f32::from(b.1)))?;
        let layout = self.layout(block)?;
        let bounds = layout.bounds()?;
        let downstream = |offset| Some((Pos::new(block, offset), Affinity::Downstream));
        // Above all the text is its start and below it its end, as in macOS.
        if self.order.first() == Some(&block) && at.y < bounds.top() {
            return downstream(0);
        }
        if self.order.last() == Some(&block) && at.y >= bounds.bottom() {
            return downstream(self.text_len(block));
        }
        // Not `clamp`, which panics on a layout less than a pixel tall.
        let y = at.y.min(bounds.bottom() - px(1.)).max(bounds.top());
        let (index, affinity) = layout.hit(point(at.x, y))?;
        Some((Pos::new(block, index.min(self.text_len(block))), affinity))
    }

    fn neighbor(&self, block: BlockId, forward: bool) -> Option<BlockId> {
        let at = self.order.iter().position(|known| *known == block)?;
        let at = if forward { at + 1 } else { at.checked_sub(1)? };
        self.order.get(at).copied()
    }

    /// Where a motion takes the cursor from `head`, with `goal` the x
    /// position that moving up and down keeps to.
    fn target(&self, head: Pos, motion: Motion, goal: Pixels) -> Option<(Pos, Affinity)> {
        let doc = self.editor.document();
        let text = doc.text(head.block).ok()?;
        let (before, after) = text.split_at(head.offset);
        let start = |block| Pos::new(block, 0);
        let end = |block| Pos::new(block, self.text_len(block));
        let layout = self.layout(head.block)?;
        let bounds = layout.bounds()?;
        let (line, shown) = layout.line(head.offset, self.affinity(head))?;
        let at = |x, y| self.hit(point(x, y));
        let target = match motion {
            Motion::Left => match before.grapheme_indices(true).next_back() {
                Some((offset, _)) => Pos::new(head.block, offset),
                None => self.neighbor(head.block, false).map_or(head, end),
            },
            Motion::Right => match after.graphemes(true).next() {
                Some(grapheme) => Pos::new(head.block, head.offset + grapheme.len()),
                None => self.neighbor(head.block, true).map_or(head, start),
            },
            Motion::WordLeft => {
                let mut words = before.split_word_bound_indices().rev();
                match words.find(|(_, word)| !word.trim().is_empty()) {
                    Some((offset, _)) => Pos::new(head.block, offset),
                    None if before.is_empty() => self.neighbor(head.block, false).map_or(head, end),
                    None => start(head.block),
                }
            }
            Motion::WordRight => {
                let mut words = after.split_word_bound_indices();
                match words.find(|(_, word)| !word.trim().is_empty()) {
                    Some((offset, word)) => Pos::new(head.block, head.offset + offset + word.len()),
                    None if after.is_empty() => self.neighbor(head.block, true).map_or(head, start),
                    None => end(head.block),
                }
            }
            Motion::LineStart => Pos::new(head.block, shown.start),
            // The end of a line that wraps is the end of that line, not the
            // start of the next.
            Motion::LineEnd => return Some((Pos::new(head.block, shown.end), Affinity::Upstream)),
            Motion::Up if line.top() > bounds.top() + px(1.) => {
                return at(goal, line.top() - px(1.));
            }
            Motion::Up => match self.neighbor(head.block, false) {
                Some(block) => return at(goal, self.layout(block)?.bounds()?.bottom() - px(1.)),
                None => start(head.block),
            },
            Motion::Down if line.bottom() < bounds.bottom() - px(1.) => {
                return at(goal, line.bottom() + px(1.));
            }
            Motion::Down => match self.neighbor(head.block, true) {
                Some(block) => return at(goal, self.layout(block)?.bounds()?.top() + px(1.)),
                None => end(head.block),
            },
            Motion::DocStart => start(*self.order.first()?),
            Motion::DocEnd => end(*self.order.last()?),
        };
        Some((target, Affinity::Downstream))
    }

    /// Moves the cursor, or the head of the selection when `extend`.
    pub(crate) fn move_cursor(&mut self, motion: Motion, extend: bool) {
        let Some(Selection::Text { anchor, head }) = self.editor.selection().cloned() else {
            return;
        };
        let doc = self.editor.document();
        let collapses = !extend && anchor != head && matches!(motion, Motion::Left | Motion::Right);
        if collapses && let Ok((from, to)) = doc.ordered(anchor, head) {
            let caret = if motion == Motion::Left { from } else { to };
            self.editor
                .set_selection(Some(Selection::caret(caret)))
                .ok();
            return;
        }
        let x = || Some(self.caret_bounds(head)?.left());
        let vertical = matches!(motion, Motion::Up | Motion::Down);
        let kept = self
            .goal
            .filter(|(at, _)| vertical && *at == head)
            .map(|(_, x)| x);
        let Some(goal) = kept.or_else(x) else { return };
        let Some((target, affinity)) = self.target(head, motion, goal) else {
            return;
        };
        self.side = Some((target, affinity));
        let anchor = if extend { anchor } else { target };
        let selection = Selection::Text {
            anchor,
            head: target,
        };
        self.editor.set_selection(Some(selection)).ok();
        self.goal = vertical.then_some((target, goal));
    }

    /// Selects the text of every block.
    pub(crate) fn select_all(&mut self) {
        let (Some(first), Some(last)) = (self.order.first(), self.order.last()) else {
            return;
        };
        let anchor = Pos::new(*first, 0);
        let head = Pos::new(*last, self.text_len(*last));
        self.editor
            .set_selection(Some(Selection::Text { anchor, head }))
            .ok();
    }

    pub(super) fn mouse_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        window.focus(&self.focus, cx);
        // A click ends a composition with what was composed.
        if let Some(composition) = self.composition.take() {
            self.commit(&composition.text);
        }
        let Some((pos, affinity)) = self.hit(event.position) else {
            return;
        };
        self.side = Some((pos, affinity));
        let text = self.editor.document().text(pos.block).unwrap_or_default();
        let (anchor, head) = match event.click_count {
            1 => match self.editor.selection() {
                Some(Selection::Text { anchor, .. }) if event.modifiers.shift => (*anchor, pos),
                _ => (pos, pos),
            },
            2 => {
                let mut words = text.split_word_bound_indices();
                let word = words.find(|(start, word)| pos.offset < start + word.len());
                let (start, word) = word.unwrap_or((pos.offset, ""));
                (
                    Pos::new(pos.block, start),
                    Pos::new(pos.block, start + word.len()),
                )
            }
            _ => (Pos::new(pos.block, 0), Pos::new(pos.block, text.len())),
        };
        self.editor
            .set_selection(Some(Selection::Text { anchor, head }))
            .ok();
        self.dragging = true;
        self.selection_changed(window, cx);
    }

    pub(super) fn mouse_move(
        &mut self,
        event: &MouseMoveEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let pressed = event.pressed_button == Some(MouseButton::Left);
        if self.side_menu_move(event.position, pressed) {
            cx.notify();
        }
        if !self.dragging {
            return;
        }
        // A release the window never heard of ends the drag too.
        if event.pressed_button != Some(MouseButton::Left) {
            self.dragging = false;
            cx.notify();
            return;
        }
        let (Some(Selection::Text { anchor, .. }), Some((head, affinity))) =
            (self.editor.selection().cloned(), self.hit(event.position))
        else {
            return;
        };
        self.side = Some((head, affinity));
        self.editor
            .set_selection(Some(Selection::Text { anchor, head }))
            .ok();
        self.selection_changed(window, cx);
    }

    pub(super) fn mouse_up(
        &mut self,
        event: &MouseUpEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if event.button != MouseButton::Left {
            return;
        }
        match self.side_menu_release() {
            Some(true) => self.selection_changed(window, cx),
            Some(false) => cx.notify(),
            None => {}
        }
        if self.dragging {
            self.dragging = false;
            // The toolbar waits for the selection to be made.
            cx.notify();
        }
    }

    pub(super) fn selection_changed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.reveal.set(true);
        window.notify_text_selection_changed(&self.focus);
        window.invalidate_character_coordinates();
        cx.notify();
    }
}
