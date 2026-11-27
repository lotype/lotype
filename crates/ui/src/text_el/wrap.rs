//! Breaking a block's text into lines, around atoms of their own size.

use std::ops::Range;

use gpui::{Pixels, ShapedLine, SharedString, Size, TextRun, Window, size};
use unicode_segmentation::UnicodeSegmentation;

use super::{Kind, Laid, Line, Piece};

/// What a block's text is laid out from, measured before layout begins.
pub(super) struct Input {
    pub(super) text: SharedString,
    pub(super) runs: Vec<TextRun>,
    /// Where each run begins.
    pub(super) starts: Vec<usize>,
    pub(super) atoms: Vec<(Range<usize>, Size<Pixels>)>,
    pub(super) interlines: Vec<(usize, Size<Pixels>)>,
    pub(super) hidden_lines: Vec<usize>,
    pub(super) trimmed_lines: Vec<(usize, usize)>,
    pub(super) font_size: Pixels,
    pub(super) line_height: Pixels,
    /// How far right each line goes in the room it leaves: 0 left, 1 right.
    pub(super) align: f32,
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

/// The text runs of `range`, cut from runs that cover the whole text, of
/// which `starts` holds where each begins.
fn runs_in(runs: &[TextRun], starts: &[usize], range: &Range<usize>) -> Vec<TextRun> {
    // Runs are found by search: a text with many atoms has many runs.
    let first = starts
        .partition_point(|start| *start <= range.start)
        .saturating_sub(1);
    let mut cut = Vec::new();
    for (run, start) in runs[first..].iter().zip(&starts[first..]) {
        if *start >= range.end {
            break;
        }
        let len = (start + run.len)
            .min(range.end)
            .saturating_sub((*start).max(range.start));
        if len > 0 {
            cut.push(TextRun { len, ..run.clone() });
        }
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
        let runs = runs_in(&self.runs, &self.starts, range);
        window
            .text_system()
            .shape_line(text, self.font_size, &runs, None)
    }

    /// Text between atoms is shaped whole, and atoms take their width.
    fn hard(&self, range: Range<usize>, window: &Window) -> Hard {
        let mut items = Vec::new();
        let mut x = Pixels::ZERO;
        let mut at = range.start;
        // Atoms are in order: those of this line follow each other.
        let first = self
            .atoms
            .partition_point(|(atom, _)| atom.start < range.start);
        let atoms = self.atoms.iter().enumerate().skip(first);
        let atoms = atoms.take_while(|(_, (atom, _))| atom.end <= range.end);
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
        let mut placed_interlines = Vec::new();
        let mut top = Pixels::ZERO;
        let mut widest = Pixels::ZERO;
        let mut start = 0;
        for (hard_line, hard) in self.text.split('\n').enumerate() {
            let full_range = start..start + hard.len();
            start = full_range.end + 1;
            if !self.hidden_lines.contains(&hard_line) {
                let visible_end = self
                    .trimmed_lines
                    .iter()
                    .find_map(|&(line_idx, end)| (line_idx == hard_line).then_some(end))
                    .unwrap_or(full_range.end)
                    .clamp(full_range.start, full_range.end);
                let visible_end = self
                    .text
                    .floor_char_boundary(visible_end)
                    .max(full_range.start);
                let range = full_range.start..visible_end;
                let line = self.hard(range.clone(), window);
                for (sub_idx, range) in self
                    .breaks(&line, range, wrap_width)
                    .into_iter()
                    .enumerate()
                {
                    // Space hanging past the edge takes no width.
                    let content = range.start + self.text[range.clone()].trim_end().len();
                    let width = line.x(content) - line.x(range.start);
                    widest = widest.max(width);
                    let mut laid = self.line(&line, hard_line, sub_idx == 0, range, top, window);
                    if let Some(room) = wrap_width.map(|wrap| wrap - width) {
                        let left = (room * self.align).max(Pixels::ZERO);
                        for piece in &mut laid.pieces {
                            piece.x += left;
                        }
                        laid.left = left;
                    }
                    top += laid.height;
                    lines.push(laid);
                }
                if let Some(last) = lines.last_mut() {
                    last.wraps = false;
                }
            }
            for (index, &(after_line, measured)) in self.interlines.iter().enumerate() {
                if after_line == hard_line {
                    widest = widest.max(measured.width);
                    placed_interlines.push((index, top));
                    top += measured.height;
                }
            }
        }
        Laid {
            text: self.text.clone(),
            line_height: self.line_height,
            lines,
            interlines: placed_interlines,
            size: size(wrap_width.unwrap_or(widest), top),
            wrap_width,
            origin: Default::default(),
        }
    }

    /// Greedy line breaks, where space at the end of a line may hang past
    /// the edge, and a word wider than the line breaks between graphemes.
    /// Each line walks its graphemes once, and stops at the first that
    /// does not fit, so a paragraph lays out in near-linear time.
    fn breaks(&self, line: &Hard, range: Range<usize>, width: Option<Pixels>) -> Vec<Range<usize>> {
        let text = &self.text;
        let mut lines = Vec::new();
        let mut start = range.start;
        // Space at the end hangs past the edge of the last line.
        let rest = range.start + text[range.clone()].trim_end().len();
        loop {
            let limit = width.map(|width| line.x(start) + width);
            let fits = |end: usize| limit.is_none_or(|limit| line.x(end) <= limit);
            if fits(rest.max(start)) {
                lines.push(start..range.end);
                return lines;
            }
            // The last place a line may break, the last grapheme boundary,
            // and the first, all where the text before them fits.
            let (mut wrap, mut grapheme, mut first) = (None, None, None);
            // Where the text before a place ends, without trailing space.
            let mut content = start;
            let graphemes = text[start..range.end].grapheme_indices(true);
            for (at, part) in graphemes.map(|(offset, part)| (start + offset, part)) {
                if at > start {
                    first.get_or_insert(at);
                    if !fits(content) {
                        break;
                    }
                    if breaks_before(text, at) {
                        wrap = Some(at);
                    }
                    grapheme = Some(at);
                }
                if !part.chars().all(char::is_whitespace) {
                    content = at + part.len();
                }
            }
            let end = wrap.or(grapheme).or(first).unwrap_or(range.end);
            // Nothing but space is left: it hangs on this line, and no
            // empty line follows a last grapheme wider than the line.
            if end >= rest {
                lines.push(start..range.end);
                return lines;
            }
            lines.push(start..end);
            start = end;
        }
    }

    fn line(
        &self,
        hard: &Hard,
        hard_line: usize,
        first_of_hard: bool,
        range: Range<usize>,
        top: Pixels,
        window: &Window,
    ) -> Line {
        let left = hard.x(range.start);
        let mut pieces = Vec::new();
        let mut text_top = Pixels::ZERO;
        // Items are in order: those of this line follow each other.
        let first = hard
            .items
            .partition_point(|item| item.range.end <= range.start);
        let items = hard.items[first..]
            .iter()
            .take_while(|item| item.range.start < range.end);
        for item in items {
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
            hard_line,
            first_of_hard,
            range,
            wraps: true,
            left: Pixels::ZERO,
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
    fn runs_are_cut_to_a_range() {
        let run = |len| TextRun {
            len,
            ..TextRun::default()
        };
        let runs = [run(3), run(2), run(4)];
        let lens = |range| {
            runs_in(&runs, &[0, 3, 5], &range)
                .iter()
                .map(|run| run.len)
                .collect::<Vec<_>>()
        };
        assert_eq!(lens(2..6), [1, 2, 1]);
        assert_eq!(lens(3..5), [2]);
        assert_eq!(lens(0..9), [3, 2, 4]);
        assert_eq!(lens(9..9), Vec::<usize>::new());
    }

    #[test]
    fn lines_break_after_spaces_and_around_cjk_but_not_at_atoms() {
        let text = "ab cd\u{FFFC}e 日本";
        let breaks: Vec<_> = (1..text.len())
            .filter(|at| text.is_char_boundary(*at) && breaks_before(text, *at))
            .collect();
        // Before `c`, before `日`, and between `日` and `本`.
        assert_eq!(breaks, [3, 10, 13]);
    }
}
