//! A session's edits shown where they are: the words a change took out
//! struck through next to those it put in, and the blocks it removed
//! struck through where the document has them.

use std::ops::Range;

use gpui::{App, Hsla, StrikethroughStyle, TextRun, px};
use gpui_component::ActiveTheme;
use lotype_ai::{AiState, Change, Diff, word_diff};
use lotype_core::{BlockId, Document, Span};

use super::EditorView;
use super::canvas::Row;

/// The color of what a session takes out.
pub(super) fn removed_ink(cx: &App) -> Hsla {
    cx.theme().red
}

/// The color of what a session puts in.
pub(super) fn added_ink(cx: &App) -> Hsla {
    cx.theme().green
}

/// Whether a stretch of the text shown went or came.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Mark {
    Removed,
    Added,
}

/// The stretches of a block's text shown that went or came, in its bytes.
pub(super) type Marks = Vec<(Range<usize>, Mark)>;

/// Strikes through the runs of what went, and tints those of what came.
pub(super) fn mark_runs(runs: &mut [TextRun], marks: &Marks, cx: &App) {
    let (gone, come) = (removed_ink(cx), added_ink(cx));
    let mut at = 0;
    for run in runs {
        let mark = marks.iter().find(|(range, _)| range.contains(&at));
        match mark.map(|(_, mark)| mark) {
            Some(Mark::Removed) => {
                run.color = cx.theme().muted_foreground;
                run.background_color = Some(gone.opacity(0.15));
                let (thickness, color) = (px(1.), Some(gone));
                run.strikethrough = Some(StrikethroughStyle { thickness, color });
            }
            Some(Mark::Added) => run.background_color = Some(come.opacity(0.2)),
            None => {}
        }
        at += run.len;
    }
}

/// Puts the `struck` blocks back among `rows`, where `main` has them:
/// after the sibling before each that is shown, or first in its parent.
// shortcut: what is in a removed block is not shown with it; draw its
// children when reviews of removed lists and toggles ask for more.
pub(super) fn restore_removed(rows: &mut Vec<Row>, main: &Document, struck: &[BlockId]) {
    for &block in struck {
        let at = |block: BlockId, rows: &[Row]| rows.iter().position(|row| row.block == block);
        let Ok(parent) = main.parent(block) else {
            continue;
        };
        if at(block, rows).is_some() {
            continue;
        }
        let siblings = main.children(parent).unwrap_or_default();
        let own = siblings.iter().position(|sibling| *sibling == block);
        let mut before = siblings[..own.unwrap_or(0)].iter().rev();
        let before = before.find_map(|sibling| at(*sibling, rows));
        let around = parent.and_then(|parent| at(parent, rows));
        let (index, depth) = match (before, around) {
            (Some(before), _) => {
                let depth = rows[before].depth;
                let inside = rows[before + 1..]
                    .iter()
                    .take_while(|row| row.depth > depth);
                (before + 1 + inside.count(), depth)
            }
            (None, Some(around)) => (around + 1, rows[around].depth + 1),
            (None, None) if parent.is_none() => (0, 0),
            // Its parent is not shown, so neither is it.
            (None, None) => continue,
        };
        let row = Row {
            block,
            block_type: main.block_type(block).unwrap_or_default(),
            depth,
            number: None,
            bars: around.map_or_else(Vec::new, |around| rows[around].bars.clone()),
            selected: None,
            quoted: around.is_some_and(|around| rows[around].quoted),
        };
        rows.insert(index, row);
    }
}

impl EditorView {
    /// The blocks a session removed that are shown struck through: those
    /// with text, which the document still has.
    pub(super) fn ai_struck(&self) -> Vec<BlockId> {
        let Some(main) = &self.ai.main else {
            return Vec::new();
        };
        let removed = self.ai.changes.iter().filter_map(|change| match change {
            Change::Removed(block) => Some(*block),
            _ => None,
        });
        let texts = removed.filter(|block| main.document().text_len(*block).is_ok());
        texts.collect()
    }

    /// Whether `block` is one of [`Self::ai_struck`], asked of every row
    /// a frame lays out.
    fn ai_is_struck(&self, block: BlockId) -> bool {
        let removed = || self.ai.changes.contains(&Change::Removed(block));
        let main = self.ai.main.as_ref();
        main.is_some_and(|main| removed() && main.document().text_len(block).is_ok())
    }

    /// The document that has `block` to show: the one a session works on
    /// for a block it removed, and the one shown for the others.
    pub(super) fn document_of(&self, block: BlockId) -> &Document {
        match &self.ai.main {
            Some(main) if self.ai_is_struck(block) => main.document(),
            _ => self.editor.document(),
        }
    }

    /// The text of `block` as a review shows it, where a session changed
    /// it: a removed block's whole text as gone, and a changed block's
    /// with what went before what came. What went is the text the session
    /// found there, not what others have made of it in the document
    /// since. `None` for the others, and while the model still writes.
    pub(super) fn ai_review_spans(&self, block: BlockId) -> Option<(Vec<Span>, Marks)> {
        let was = self.ai.session.as_ref()?.original_spans(block)?;
        let text = |spans: &[Span]| spans.iter().map(|span| span.text.as_str()).collect();
        let old: String = text(was);
        if self.ai_is_struck(block) {
            return Some((was.to_vec(), vec![(0..old.len(), Mark::Removed)]));
        }
        let reviewing = self.ai_state() == Some(&AiState::Reviewing);
        if !reviewing || !self.ai.changes.contains(&Change::Updated(block)) {
            return None;
        }
        let is = self.editor.document().spans(block).ok()?;
        let new: String = text(&is);
        let (mut spans, mut marks, mut at) = (Vec::new(), Marks::new(), 0);
        for part in word_diff(&old, &new) {
            let (from, range, mark) = match part {
                Diff::Kept(range) => (&is[..], range, None),
                Diff::Added(range) => (&is[..], range, Some(Mark::Added)),
                Diff::Removed(range) => (was, range, Some(Mark::Removed)),
            };
            marks.extend(mark.map(|mark| (at..at + range.len(), mark)));
            at += range.len();
            spans.extend(spans_in(from, range));
        }
        // A change of the block's type or style alone has no words to mark.
        (!marks.is_empty()).then_some((spans, marks))
    }
}

/// The part of `spans` in `range` of their text, in bytes that end
/// characters, as a diff of that text gives them.
fn spans_in(spans: &[Span], range: Range<usize>) -> Vec<Span> {
    let mut at = 0;
    let mut part = Vec::new();
    for span in spans {
        let end = at + span.text.len();
        let (from, to) = (range.start.max(at), range.end.min(end));
        if from < to {
            let mut cut = span.clone();
            cut.text = span.text[from - at..to - at].to_owned();
            part.push(cut);
        }
        at = end;
    }
    part
}
