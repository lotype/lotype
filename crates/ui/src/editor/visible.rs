//! Which rows a frame lays out: those near the view, and those the
//! selection is in and the nearest with text on each side of it, where keys
//! take the caret. The others keep the height they had when last laid out,
//! or an estimate until they are, so that the column scrolls as before while
//! a frame costs what is near the view rather than the whole document. The
//! first row in sight keeps its place as rows above it are measured.

use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use gpui::{Pixels, Window};
use lotype_core::{BlockId, Content, Document, Selection, Version};

use super::EditorView;
use super::canvas::{self, Row};

#[derive(Default)]
pub(crate) struct Heights {
    /// How tall each row was when last laid out.
    known: HashMap<BlockId, Pixels>,
    /// The rows the last frame laid out, whose heights it measured.
    laid_out: HashSet<BlockId>,
    /// The width they were laid out at: another wraps their text anew.
    width: Option<Pixels>,
    /// The first row in sight in the last frame, and how far below the top
    /// of the view its own top was.
    anchor: Option<(BlockId, Pixels)>,
    /// Rows above it the last frame laid out apart, out of the column, only
    /// to measure them, and where each was among the scroll's children.
    measured: Vec<(BlockId, usize)>,
    /// How many more frames bring the caret into sight again, after one
    /// that did while rows near it were first measured, which moves it.
    follow: u8,
    /// How many rows in sight frames laid out apart, where no one sees
    /// them, for tests: none should be.
    unseen: usize,
}

/// What a frame does with a row.
pub(crate) enum Plan {
    /// Lays it out in the column.
    Lay,
    /// Leaves room as tall as it is, or as it is thought to be.
    Space(Pixels),
    /// Leaves room as tall as it is thought to be, and lays it out apart to
    /// measure it: it is above the view, where a row of another height
    /// would move the rows in sight.
    Measure(Pixels),
}

impl Heights {
    #[cfg(any(test, feature = "test-support"))]
    pub(super) fn laid_out_count(&self) -> usize {
        self.laid_out.len()
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(super) fn unseen_count(&self) -> usize {
        self.unseen
    }
}

/// The blocks with text in reading order: those of `rows`, and the cells of
/// their tables, row by row. Keys go through these whether or not a frame
/// laid them out.
fn reading_order(doc: &Document, rows: &[Row]) -> Vec<BlockId> {
    let mut order = Vec::with_capacity(rows.len());
    for row in rows {
        if holds_text(doc, row) {
            order.push(row.block);
            continue;
        }
        let Ok(columns) = doc.columns(row.block) else {
            continue;
        };
        for line in doc.children(Some(row.block)).unwrap_or_default() {
            let cells = columns
                .iter()
                .filter_map(|column| doc.cell(line, *column).ok());
            order.extend(cells);
        }
    }
    order
}

/// Whether the row's block holds text, read from the schema, which costs
/// nothing next to reading the text.
fn holds_text(doc: &Document, row: &Row) -> bool {
    let spec = doc.schema().block_spec(&row.block_type);
    spec.is_some_and(|spec| spec.content() == Content::Inline)
}

/// What the rows are made of: the document as it was, the toggles closed,
/// the blocks selected, and those a session removed that are still shown.
type MadeOf = (Version, HashSet<BlockId>, HashSet<BlockId>, Vec<BlockId>);

/// The rows and the reading order of the document as it was, the toggles
/// closed and the blocks selected: a frame where none of them changed, as
/// most are, reads none of the blocks again.
#[derive(Default)]
pub(crate) struct Shape {
    made_of: Option<MadeOf>,
    rows: Rc<Vec<Row>>,
    order: Rc<Vec<BlockId>>,
    /// Whether keys put the caret in each row: it holds text, or is a table.
    texty: Rc<Vec<bool>>,
}

impl EditorView {
    /// The rows of the blocks shown, and the blocks with text in reading
    /// order, made again only where the document, the closed toggles or the
    /// selected blocks changed.
    pub(super) fn shape(&mut self, selected: HashSet<BlockId>) -> Rc<Vec<Row>> {
        let doc = self.editor.document();
        let now = (
            doc.version(),
            self.closed.clone(),
            selected,
            self.ai_struck(),
        );
        if self.shape.made_of.as_ref() != Some(&now) {
            let mut rows = canvas::rows(doc, &self.closed, &now.2);
            if let Some(main) = &self.ai.main {
                super::ai_review::restore_removed(&mut rows, main.document(), &now.3);
            }
            self.shape.order = Rc::new(reading_order(doc, &rows));
            let texty = rows
                .iter()
                .map(|row| holds_text(doc, row) || doc.columns(row.block).is_ok());
            self.shape.texty = Rc::new(texty.collect());
            self.shape.rows = Rc::new(rows);
            self.shape.made_of = Some(now);
        }
        self.order = Rc::clone(&self.shape.order);
        Rc::clone(&self.shape.rows)
    }

    /// Notes the heights the last frame laid its rows out at, and the first
    /// row in sight, at the start of a frame. A change of width forgets the
    /// heights.
    pub(super) fn measure_rows(&mut self) {
        let view = self.scroll.bounds();
        if self.heights.width != Some(view.size.width) {
            self.heights.known.clear();
            self.heights.width = Some(view.size.width);
        }
        // The scroll handle has the rows where they are before scrolling.
        let top = view.top() - self.scroll.offset().y;
        self.heights.anchor = None;
        for (at, block) in self.rows.iter().enumerate() {
            if !self.heights.laid_out.contains(block) {
                continue;
            }
            let item = self.row_item(at);
            let Some(bounds) = item.and_then(|item| self.scroll.bounds_for_item(item)) else {
                continue;
            };
            self.heights.known.insert(*block, bounds.size.height);
            if self.heights.anchor.is_none() && bounds.bottom() > top {
                self.heights.anchor = Some((*block, bounds.top() - top));
            }
        }
        self.heights.laid_out.clear();
        for (block, item) in std::mem::take(&mut self.heights.measured) {
            if let Some(bounds) = self.scroll.bounds_for_item(item) {
                self.heights.known.insert(block, bounds.size.height);
            }
        }
    }

    /// Notes that the row of `block` was laid out apart, as the scroll's
    /// child `item`, to be measured at the start of the next frame.
    pub(super) fn measuring(&mut self, block: BlockId, item: usize) {
        self.heights.measured.push((block, item));
    }

    /// Notes that the view draws nothing for `block`, which takes no room.
    pub(super) fn undrawn(&mut self, block: BlockId) {
        self.heights.known.insert(block, Pixels::ZERO);
        self.heights.laid_out.remove(&block);
    }

    /// Where the row at `at` is among the scroll's children, if the last
    /// frame laid it out.
    pub(super) fn row_item(&self, at: usize) -> Option<usize> {
        self.row_items.get(at).copied().flatten()
    }

    /// Which of `rows` this frame lays out, and the height of each of the
    /// others. Where rows above the first in sight changed height, the view
    /// scrolls by as much, so that it stays put. A frame that lays out a row
    /// of no known height asks for the next, which places the rows by what
    /// that one measured.
    pub(super) fn plan_rows(&mut self, rows: &[BlockId], window: &mut Window) -> Vec<Plan> {
        // Before the view is laid out, the window is as tall as it can be.
        let view = self.scroll.bounds().size.height;
        let view = if view > Pixels::ZERO {
            view
        } else {
            window.viewport_size().height
        };
        // As tall as the rows measured are on average, or one line with the
        // space around it before any is.
        let known = &self.heights.known;
        let estimate = match known.len() {
            0 => window.line_height() + gpui::px(4.),
            count => known.values().copied().sum::<Pixels>() / count as f32,
        };
        let heights: Vec<Option<Pixels>> =
            rows.iter().map(|block| known.get(block).copied()).collect();
        let height = |at: usize| heights[at].unwrap_or(estimate);
        let mut top = -self.scroll.offset().y;
        let mut first = 0;
        if let Some((anchor, below)) = self.heights.anchor.take()
            && let Some(at) = rows.iter().position(|block| *block == anchor)
        {
            first = at;
            let y: Pixels = (0..at).map(height).sum();
            let wanted = (y - below).max(Pixels::ZERO);
            if wanted != top {
                let mut offset = self.scroll.offset();
                offset.y = -wanted;
                self.scroll.set_offset(offset);
                top = wanted;
            }
        }
        let (from, to) = (top - view, top + view * 2.);
        let wanted = self.selection_rows(rows);
        let editing_media = self.media_form_block();
        let (mut y, mut unknown) = (Pixels::ZERO, false);
        let mut plan = Vec::with_capacity(rows.len());
        for (at, block) in rows.iter().enumerate() {
            let tall = height(at);
            let near = y + tall >= from && y <= to;
            let wanted = wanted.contains(&at) || Some(*block) == editing_media;
            y += tall;
            if !(near || wanted) {
                plan.push(Plan::Space(tall));
                continue;
            }
            unknown |= heights[at].is_none();
            // Apart only where no one looks: rows in sight are drawn, and
            // the selection and active controls keep their layout in the column.
            let apart = at < first && heights[at].is_none() && y <= top && !wanted;
            if apart {
                self.heights.unseen += usize::from(y > top && y - tall < top + view);
                plan.push(Plan::Measure(tall));
            } else {
                self.heights.laid_out.insert(*block);
                plan.push(Plan::Lay);
            }
        }
        // Rows first measured near a caret just brought into sight move it:
        // it is brought there again, for a frame or two, not for as long as
        // rows are measured, which a scroll away from it goes on doing.
        if self.reveal.get() {
            self.heights.follow = 2;
        } else if unknown && self.heights.follow > 0 {
            self.heights.follow -= 1;
            self.reveal.set(true);
        } else {
            self.heights.follow = 0;
        }
        if unknown {
            window.request_animation_frame();
        }
        plan
    }

    /// The rows the selection's ends are in, and on each side of them the
    /// rows up to the nearest that keys put the caret in.
    fn selection_rows(&self, rows: &[BlockId]) -> HashSet<usize> {
        let doc = self.editor.document();
        let ends = match self.editor.selection() {
            Some(Selection::Text { anchor, head }) => vec![anchor.block, head.block],
            Some(Selection::Cells { anchor, head }) => vec![*anchor, *head],
            Some(Selection::Blocks(blocks)) => {
                let ends = [blocks.first(), blocks.last()];
                let head = self.block_range.map(|(_, head)| head);
                ends.into_iter().flatten().copied().chain(head).collect()
            }
            None => Vec::new(),
        };
        let texty = |at: usize| self.shape.texty.get(at).copied().unwrap_or(true);
        let mut wanted = HashSet::new();
        for end in ends {
            // A cell's row is its table's; a block's is its own.
            let mut at = Some(end);
            let found = loop {
                let Some(block) = at else { break None };
                if let Some(index) = rows.iter().position(|row| *row == block) {
                    break Some(index);
                }
                at = doc.parent(block).ok().flatten();
            };
            let Some(index) = found else { continue };
            let up = (0..index).rev().find(|at| texty(*at)).unwrap_or(0);
            let last = rows.len().saturating_sub(1);
            let down = (index + 1..rows.len())
                .find(|at| texty(*at))
                .unwrap_or(last);
            wanted.extend(up..=down.max(index));
        }
        wanted
    }
}
