//! Breaking a block's text into lines, around atoms of their own size.

use std::ops::Range;

use gpui::{Pixels, ShapedLine, SharedString, Size, TextRun, Window, size};
use unicode_segmentation::{GraphemeCursor, UnicodeSegmentation};

use super::{Kind, Laid, Line, Piece};

/// What a block's text is laid out from, measured before layout begins.
pub(super) struct Input {
    pub(super) text: SharedString,
    pub(super) runs: Vec<TextRun>,
    pub(super) atoms: Vec<(Range<usize>, Size<Pixels>)>,
    pub(super) font_size: Pixels,
    pub(super) line_height: Pixels,
}

/// A run of text or an atom in a line between line breaks.
struct Item {
    range: Range<usize>,
    x: Pixels,
    width: Pixels,
    /// Text is shaped; an atom has its index.
    shaped: Result<ShapedLine, usize>,
}

/// Where each place in a line between line breaks is, along it.
struct Hard {
    items: Vec<Item>,
    /// Where each glyph and atom starts and ends, by byte, in order.
    stops: Vec<(usize, Pixels)>,
}

impl Hard {
    fn new(items: Vec<Item>) -> Self {
        let mut stops = Vec::new();
        for item in &items {
            if let Ok(shaped) = &item.shaped {
                let glyphs = shaped.runs.iter().flat_map(|run| &run.glyphs);
                let starts =
                    glyphs.map(|glyph| (item.range.start + glyph.index, item.x + glyph.position.x));
                stops.extend(starts);
            } else {
                stops.push((item.range.start, item.x));
            }
            stops.push((item.range.end, item.x + item.width));
        }
        stops.sort_by_key(|(at, _)| *at);
        Self { items, stops }
    }

    /// The x position of the place before `at`. Inside a glyph or an atom
    /// is its start, as no caret goes there.
    fn x(&self, at: usize) -> Pixels {
        let after = self.stops.partition_point(|(stop, _)| *stop <= at);
        after
            .checked_sub(1)
            .map_or(Pixels::ZERO, |stop| self.stops[stop].1)
    }
}

/// The text runs of `range`, cut from runs that cover the whole text.
fn runs_in(runs: &[TextRun], range: &Range<usize>) -> Vec<TextRun> {
    let mut start = 0;
    let mut cut = Vec::new();
    for run in runs {
        let end = start + run.len;
        let len = end.min(range.end).saturating_sub(start.max(range.start));
        if len > 0 {
            cut.push(TextRun { len, ..run.clone() });
        }
        start = end;
    }
    cut
}

/// Whether a line may break before the character at `at`: after a space,
/// or next to a CJK character, as UAX #14 lets ideographs break anywhere.
/// An atom sticks to the words around it.
pub(super) fn breaks_before(text: &str, at: usize) -> bool {
    let before = text[..at].chars().next_back();
    let after = text[at..].chars().next();
    let (Some(before), Some(after)) = (before, after) else {
        return false;
    };
    before.is_whitespace() && !after.is_whitespace() || is_cjk(before) || is_cjk(after)
}

/// A line never breaks inside a grapheme, such as a CJK character with a
/// variation selector or a combining mark.
fn is_grapheme_boundary(text: &str, at: usize) -> bool {
    let mut cursor = GraphemeCursor::new(at, text.len(), true);
    cursor.is_boundary(text, 0).unwrap_or(false)
}

fn is_cjk(c: char) -> bool {
    matches!(
        c,
        '\u{2E80}'..='\u{9FFF}'
            | '\u{AC00}'..='\u{D7AF}'
            | '\u{F900}'..='\u{FAFF}'
            | '\u{FF00}'..='\u{FFEF}'
            | '\u{20000}'..='\u{2FFFF}'
    )
}

impl Input {
    fn shape(&self, range: &Range<usize>, window: &Window) -> ShapedLine {
        let text = SharedString::from(self.text[range.clone()].to_owned());
        let runs = runs_in(&self.runs, range);
        window
            .text_system()
            .shape_line(text, self.font_size, &runs, None)
    }

    /// Text between atoms is shaped whole, and atoms take their width.
    fn hard(&self, range: Range<usize>, window: &Window) -> Hard {
        let mut items = Vec::new();
        let mut x = Pixels::ZERO;
        let mut at = range.start;
        let inside = |atom: &&(Range<usize>, Size<Pixels>)| {
            atom.0.start >= range.start && atom.0.end <= range.end
        };
        let atoms = self
            .atoms
            .iter()
            .enumerate()
            .filter(|(_, atom)| inside(atom));
        let end = (usize::MAX, &(range.end..range.end, Size::default()));
        for (index, (atom, measured)) in atoms.chain([end]) {
            if atom.start > at {
                let shaped = self.shape(&(at..atom.start), window);
                let width = shaped.width();
                let shaped = Ok(shaped);
                items.push(Item {
                    range: at..atom.start,
                    x,
                    width,
                    shaped,
                });
                x += width;
            }
            if !atom.is_empty() {
                let width = measured.width;
                items.push(Item {
                    range: atom.clone(),
                    x,
                    width,
                    shaped: Err(index),
                });
                x += width;
            }
            at = atom.end;
        }
        Hard::new(items)
    }

    pub(super) fn lay_out(&self, wrap_width: Option<Pixels>, window: &Window) -> Laid {
        let mut lines = Vec::new();
        let mut top = Pixels::ZERO;
        let mut widest = Pixels::ZERO;
        let mut start = 0;
        for hard in self.text.split('\n') {
            let range = start..start + hard.len();
            start = range.end + 1;
            let line = self.hard(range.clone(), window);
            for range in self.breaks(&line, range, wrap_width) {
                // Space hanging past the edge takes no width.
                let content = range.start + self.text[range.clone()].trim_end().len();
                widest = widest.max(line.x(content) - line.x(range.start));
                let laid = self.line(&line, range, top, window);
                top += laid.height;
                lines.push(laid);
            }
            if let Some(last) = lines.last_mut() {
                last.wraps = false;
            }
        }
        Laid {
            text: self.text.clone(),
            line_height: self.line_height,
            lines,
            size: size(wrap_width.unwrap_or(widest), top),
            wrap_width,
            origin: Default::default(),
        }
    }

    /// Greedy line breaks, where space at the end of a line may hang past
    /// the edge, and a word wider than the line breaks between graphemes.
    fn breaks(&self, line: &Hard, range: Range<usize>, width: Option<Pixels>) -> Vec<Range<usize>> {
        let text = &self.text;
        let mut lines = Vec::new();
        let mut start = range.start;
        loop {
            let limit = width.map(|width| line.x(start) + width);
            let fits = |at: usize| {
                let end = start + text[start..at].trim_end().len();
                limit.is_none_or(|limit| line.x(end) <= limit)
            };
            if fits(range.end) {
                lines.push(start..range.end);
                return lines;
            }
            // Each candidate is looked at once: the first that does not
            // fit ends the search.
            let breaks = (start + 1..range.end).filter(|at| {
                text.is_char_boundary(*at)
                    && is_grapheme_boundary(text, *at)
                    && breaks_before(text, *at)
            });
            let end = breaks.take_while(|at| fits(*at)).last().unwrap_or_else(|| {
                let mut graphemes = text[start..range.end].grapheme_indices(true);
                let ends = graphemes
                    .by_ref()
                    .map(|(offset, g)| start + offset + g.len());
                let mut ends = ends.peekable();
                let first = ends.peek().copied().unwrap_or(range.end);
                ends.take_while(|at| fits(*at)).last().unwrap_or(first)
            });
            lines.push(start..end);
            start = end;
        }
    }

    fn line(&self, hard: &Hard, range: Range<usize>, top: Pixels, window: &Window) -> Line {
        let left = hard.x(range.start);
        let mut pieces = Vec::new();
        let mut text_top = Pixels::ZERO;
        for item in &hard.items {
            let part = item.range.start.max(range.start)..item.range.end.min(range.end);
            if part.is_empty() {
                continue;
            }
            let x = hard.x(part.start) - left;
            match item.shaped {
                Ok(_) => {
                    let shaped = self.shape(&part, window);
                    let width = shaped.width();
                    pieces.push(Piece {
                        range: part,
                        x,
                        width,
                        kind: Kind::Text(Box::new(shaped)),
                    });
                }
                Err(index) => {
                    // An atom is centred on the text, as CSS's `middle`
                    // puts it, and pushes the text down when it is taller.
                    let over = (self.atoms[index].1.height - self.line_height) / 2.;
                    text_top = text_top.max(over);
                    let kind = Kind::Atom(index, -over);
                    pieces.push(Piece {
                        range: part,
                        x,
                        width: item.width,
                        kind,
                    });
                }
            }
        }
        let mut height = text_top + self.line_height;
        for piece in &mut pieces {
            if let Kind::Atom(index, offset) = &mut piece.kind {
                *offset += text_top;
                height = height.max(*offset + self.atoms[*index].1.height);
            }
        }
        Line {
            range,
            wraps: true,
            top,
            height,
            text_top,
            pieces,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines_break_after_spaces_and_around_cjk_but_not_at_atoms() {
        let text = "ab cd\u{FFFC}e 日本";
        let breaks: Vec<_> = (1..text.len())
            .filter(|at| text.is_char_boundary(*at) && breaks_before(text, *at))
            .collect();
        // Before `c`, before `日`, and between `日` and `本`.
        assert_eq!(breaks, [3, 10, 13]);
        let marked = "日\u{E0100}本";
        assert!(!is_grapheme_boundary(marked, 3));
        assert!(is_grapheme_boundary(marked, 7));
    }
}
