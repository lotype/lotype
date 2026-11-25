//! Moving the cursor and selecting, with keys and the mouse.

use std::collections::HashMap;

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
    /// The selection from `anchor` to `head`: text, or, from one cell into
    /// another of its table, the cells between them whole.
    fn between(&self, anchor: Pos, head: Pos) -> Selection {
        let doc = self.editor.document();
        let cells =
            head.block != anchor.block && doc.cells_between(anchor.block, head.block).is_ok();
        match cells {
            true => Selection::Cells {
                anchor: anchor.block,
                head: head.block,
            },
            false => Selection::Text { anchor, head },
        }
    }

    fn layout(&self, block: BlockId) -> Option<&TextLayout> {
        self.layouts.get(&block)
    }

    fn text_len(&self, block: BlockId) -> usize {
        self.editor.document().text_len(block).unwrap_or(0)
    }

    /// The place in the text under a point, or nearest to it, and the side
    /// of a soft wrap it is on.
    pub(crate) fn hit(&self, at: Point<Pixels>) -> Option<(Pos, Affinity)> {
        // Up and down first, then, for the cells of a row, left and right.
        let bands = self.bands();
        let distance = |block: &BlockId| {
            let bounds = self.layout(*block)?.bounds()?;
            let (top, bottom) = self.extent(&bands, *block)?;
            let gap =
                |before: Pixels, after: Pixels| f32::from(before.max(after).max(Pixels::ZERO));
            let y = gap(top - at.y, at.y - bottom);
            let x = gap(bounds.left() - at.x, at.x - bounds.right());
            Some((y, x))
        };
        let nearest = self
            .order
            .iter()
            .filter_map(|block| Some((*block, distance(block)?)));
        let closer = |a: &(BlockId, (f32, f32)), b: &(BlockId, (f32, f32))| {
            a.1.0.total_cmp(&b.1.0).then(a.1.1.total_cmp(&b.1.1))
        };
        let (block, _) = nearest.min_by(closer)?;
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

    /// How far up and down each row of a table goes: its cells are as tall
    /// as the tallest of them, whatever text each holds.
    fn bands(&self) -> HashMap<BlockId, (Pixels, Pixels)> {
        let doc = self.editor.document();
        let mut bands: HashMap<BlockId, (Pixels, Pixels)> = HashMap::new();
        for cell in self.order.iter().filter(|block| block.column().is_some()) {
            let bounds = self.layout(*cell).and_then(|layout| layout.bounds());
            let (Some(bounds), Ok(Some(row))) = (bounds, doc.parent(*cell)) else {
                continue;
            };
            let band = bands.entry(row).or_insert((bounds.top(), bounds.bottom()));
            *band = (band.0.min(bounds.top()), band.1.max(bounds.bottom()));
        }
        bands
    }

    /// From where to where down the page `block` is: its text, or for a
    /// cell its row.
    fn extent(
        &self,
        bands: &HashMap<BlockId, (Pixels, Pixels)>,
        block: BlockId,
    ) -> Option<(Pixels, Pixels)> {
        let row = block
            .column()
            .and(self.editor.document().parent(block).ok()?);
        if let Some(band) = row.and_then(|row| bands.get(&row)) {
            return Some(*band);
        }
        let bounds = self.layout(block)?.bounds()?;
        Some((bounds.top(), bounds.bottom()))
    }

    /// Where to aim to go a line up or down out of `block`: just inside the
    /// nearest block wholly above or below it, which for a cell is no cell
    /// of its own row.
    fn beyond(&self, block: BlockId, down: bool) -> Option<Pixels> {
        let bands = self.bands();
        let (top, bottom) = self.extent(&bands, block)?;
        let at = self.order.iter().position(|known| *known == block)?;
        let extent = |other: &BlockId| self.extent(&bands, *other);
        if down {
            let mut below = self.order[at + 1..].iter().filter_map(extent);
            below
                .find(|other| other.0 >= bottom)
                .map(|other| other.0 + px(1.))
        } else {
            let mut above = self.order[..at].iter().rev().filter_map(extent);
            above
                .find(|other| other.1 <= top)
                .map(|other| other.1 - px(1.))
        }
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
            Motion::Up => match self.beyond(head.block, false) {
                Some(y) => return at(goal, y),
                None => start(head.block),
            },
            Motion::Down if line.bottom() < bounds.bottom() - px(1.) => {
                return at(goal, line.bottom() + px(1.));
            }
            Motion::Down => match self.beyond(head.block, true) {
                Some(y) => return at(goal, y),
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
        // Past the edge of a cell, Shift and an arrow select cells.
        let selection = self.between(anchor, target);
        self.editor.set_selection(Some(selection)).ok();
        self.goal = vertical.then_some((target, goal));
    }

    /// Selects the text of every block.
    pub(crate) fn select_all(&mut self) {
        let (Some(first), Some(last)) = (self.order.first(), self.order.last()) else {
            return;
        };
        // No text selection ends in a cell, so there the blocks are selected.
        if first.column().is_some() || last.column().is_some() {
            if let (Some(first), Some(last)) = (self.rows.first(), self.rows.last()) {
                self.select_rows(*first, *last);
            }
            return;
        }
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
        if self.ai.session.is_some() {
            return;
        }
        window.focus(&self.focus, cx);
        self.toolbar.menu = None;
        // A click ends a composition with what was composed.
        if let Some(composition) = self.composition.take() {
            self.commit(&composition.text);
        }
        if self.start_writing() {
            self.selection_changed(window, cx);
            return;
        }
        let Some((pos, affinity)) = self.hit(event.position) else {
            return;
        };
        self.side = Some((pos, affinity));
        let text = self.editor.document().text(pos.block).unwrap_or_default();
        let (anchor, head) = match event.click_count {
            1 => match self.editor.selection() {
                Some(Selection::Text { anchor, .. }) if event.modifiers.shift => (*anchor, pos),
                Some(Selection::Cells { anchor, .. }) if event.modifiers.shift => {
                    (Pos::new(*anchor, 0), pos)
                }
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
        let selection = self.between(anchor, head);
        self.editor.set_selection(Some(selection)).ok();
        self.drag_anchor = Some(anchor);
        self.dragging = true;
        self.selection_changed(window, cx);
    }

    pub(super) fn mouse_move(
        &mut self,
        event: &MouseMoveEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.ai.session.is_some() {
            return;
        }
        let pressed = event.pressed_button == Some(MouseButton::Left);
        if self.side_menu_move(event.position, pressed, cx) {
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
        // The editor keeps a text selection's anchor in place through a
        // peer's edits; the one noted at the press only says where cells
        // selected since started.
        let anchor = match self.editor.selection() {
            Some(Selection::Text { anchor, .. }) => Some(*anchor),
            _ => self.drag_anchor,
        };
        let (Some(anchor), Some((head, affinity))) = (anchor, self.hit(event.position)) else {
            return;
        };
        self.side = Some((head, affinity));
        let selection = self.between(anchor, head);
        self.editor.set_selection(Some(selection)).ok();
        self.selection_changed(window, cx);
    }

    pub(super) fn mouse_up(
        &mut self,
        event: &MouseUpEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if event.button != MouseButton::Left {
            return;
        }
        // A drop leaves the view scrolled where it is.
        if self.side_menu_release() {
            cx.notify();
        }
        if self.dragging {
            self.dragging = false;
            // The toolbar waits for the selection to be made.
            cx.notify();
        }
    }

    pub(super) fn selection_changed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.reveal.set(true);
        self.restart_blink(window, cx);
        window.notify_text_selection_changed(&self.focus);
        window.invalidate_character_coordinates();
        cx.notify();
    }
}
