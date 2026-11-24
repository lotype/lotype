//! Where places in laid-out text are on screen, and which place is at a
//! point. Nothing here panics before layout: it answers `None` instead.

use std::ops::Range;

use gpui::{Bounds, Pixels, Point, ShapedLine, SharedString, point, px, size};
use unicode_segmentation::UnicodeSegmentation;

use super::{Kind, Laid, Line, TextLayout};

/// Which line a caret at a soft wrap is on: the end of the upper one, or
/// the start of the lower one.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum Affinity {
    Upstream,
    #[default]
    Downstream,
}

impl Line {
    /// The x position of a place in this line.
    fn x(&self, at: usize) -> Pixels {
        for piece in &self.pieces {
            if at <= piece.range.end {
                return match &piece.kind {
                    Kind::Text(shaped) => {
                        piece.x + shaped.x_for_index(at.saturating_sub(piece.range.start))
                    }
                    Kind::Atom(..) if at <= piece.range.start => piece.x,
                    Kind::Atom(..) => piece.x + piece.width,
                };
            }
        }
        self.pieces
            .last()
            .map_or(self.left, |piece| piece.x + piece.width)
    }

    /// The place nearest to `x`.
    fn index(&self, x: Pixels) -> usize {
        for piece in &self.pieces {
            if x < piece.x + piece.width {
                return match &piece.kind {
                    Kind::Text(shaped) => piece.range.start + closest(shaped, x - piece.x),
                    Kind::Atom(..) if x < piece.x + piece.width / 2. => piece.range.start,
                    Kind::Atom(..) => piece.range.end,
                };
            }
        }
        self.range.end
    }
}

/// The glyph boundary nearest to `x`. GPUI's `closest_index_for_x` gives
/// the end of the text for any `x` past the last glyph's start.
fn closest(shaped: &ShapedLine, x: Pixels) -> usize {
    let glyphs = shaped.runs.iter().flat_map(|run| &run.glyphs);
    let boundaries = glyphs.map(|glyph| (glyph.index, glyph.position.x));
    let boundaries = boundaries.chain([(shaped.len(), shaped.width())]);
    let distance = |(_, at): &(usize, Pixels)| f32::from((*at - x).abs());
    let nearest = boundaries.min_by(|a, b| distance(a).total_cmp(&distance(b)));
    nearest.map_or(0, |(index, _)| index)
}

impl Laid {
    /// The line a place is on.
    fn line(&self, at: usize, affinity: Affinity) -> Option<usize> {
        let found = self
            .lines
            .iter()
            .position(|line| line.range.contains(&at) || line.range.end == at)?;
        let line = &self.lines[found];
        let lower = line.wraps && at == line.range.end && affinity == Affinity::Downstream;
        Some(found + usize::from(lower))
    }
}

impl TextLayout {
    fn read<R>(&self, read: impl FnOnce(&Laid) -> Option<R>) -> Option<R> {
        self.0.borrow().as_ref().and_then(read)
    }

    pub(crate) fn text(&self) -> Option<SharedString> {
        self.read(|laid| Some(laid.text.clone()))
    }

    pub(crate) fn bounds(&self) -> Option<Bounds<Pixels>> {
        self.read(|laid| Some(Bounds::new(laid.origin, laid.size)))
    }

    /// Where a caret at `at` is drawn: as tall as the text, not as an atom
    /// that makes its line taller.
    pub(crate) fn caret(&self, at: usize, affinity: Affinity) -> Option<Bounds<Pixels>> {
        self.read(|laid| {
            let line = &laid.lines[laid.line(at, affinity)?];
            // Space hanging past the edge puts no caret beyond it.
            let x = line.x(at).min(laid.size.width);
            let origin = laid.origin + point(x, line.top + line.text_top);
            Some(Bounds::new(origin, size(px(1.5), laid.line_height)))
        })
    }

    /// The line a place is on, as wide as the text, and what it shows.
    pub(crate) fn line(
        &self,
        at: usize,
        affinity: Affinity,
    ) -> Option<(Bounds<Pixels>, Range<usize>)> {
        self.read(|laid| {
            let line = &laid.lines[laid.line(at, affinity)?];
            let origin = laid.origin + point(Pixels::ZERO, line.top);
            let bounds = Bounds::new(origin, size(laid.size.width, line.height));
            Some((bounds, line.range.clone()))
        })
    }

    /// The place nearest to a point, and the line it is on when that ends
    /// at a soft wrap.
    pub(crate) fn hit(&self, at: Point<Pixels>) -> Option<(usize, Affinity)> {
        self.read(|laid| {
            let local = at - laid.origin;
            let below = |line: &&Line| local.y < line.top + line.height;
            let line = laid.lines.iter().find(below).or(laid.lines.last())?;
            let index = line.index(local.x);
            // A glyph can start inside a grapheme, as a combining mark's
            // does: the caret goes before the grapheme, not into it.
            let shown = laid.text.get(line.range.clone()).unwrap_or_default();
            let within = index.saturating_sub(line.range.start);
            let starts = shown.grapheme_indices(true).map(|(at, _)| at);
            let starts = starts.chain([shown.len()]);
            let before = starts.take_while(|at| *at <= within).last().unwrap_or(0);
            let index = line.range.start + before;
            let at_wrap = line.wraps && index == line.range.end;
            let affinity = if at_wrap {
                Affinity::Upstream
            } else {
                Affinity::Downstream
            };
            Some((index, affinity))
        })
    }

    /// Rectangles covering `start..end`, one per line. A line the range
    /// goes on from reaches the right edge; `wide` gives an empty line some
    /// width, for an empty block in a selection that goes through it.
    pub(crate) fn rectangles(&self, start: usize, end: usize, wide: bool) -> Vec<Bounds<Pixels>> {
        let rectangles = self.read(|laid| {
            let mut all = Vec::new();
            for line in &laid.lines {
                let (from, to) = (start.max(line.range.start), end.min(line.range.end));
                let through = end > line.range.end;
                let empty = line.range.is_empty() && wide;
                if from > to || from == to && !(empty || through && from > start) {
                    continue;
                }
                let left = line.x(from).min(laid.size.width);
                let right = if through { laid.size.width } else { line.x(to) };
                let right = right.min(laid.size.width).max(left);
                let width = if empty {
                    (right - left).max(px(4.))
                } else {
                    right - left
                };
                let origin = laid.origin + point(left, line.top);
                all.push(Bounds::new(origin, size(width, line.height)));
            }
            Some(all)
        });
        rectangles.unwrap_or_default()
    }
}
