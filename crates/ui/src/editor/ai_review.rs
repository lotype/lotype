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

    /// The document that has `block` to show: the one a session works on
    /// for a block it removed, and the one shown for the others.
    pub(super) fn document_of(&self, block: BlockId) -> &Document {
        match &self.ai.main {
            Some(main) if self.ai_struck().contains(&block) => main.document(),
            _ => self.editor.document(),
        }
    }

    /// The text of `block` as a review shows it, where a session changed
    /// it: a removed block's whole text as gone, and a changed block's
    /// with what went before what came. `None` for the others, and while
    /// the model still writes.
    pub(super) fn ai_review_spans(&self, block: BlockId) -> Option<(Vec<Span>, Marks)> {
        let main = self.ai.main.as_ref()?.document();
        if self.ai_struck().contains(&block) {
            let spans = main.spans(block).ok()?;
            let len = spans.iter().map(|span| span.text.len()).sum();
            return Some((spans, vec![(0..len, Mark::Removed)]));
        }
        let reviewing = self.ai_state() == Some(&AiState::Reviewing);
        if !reviewing || !self.ai.changes.contains(&Change::Updated(block)) {
            return None;
        }
        let copy = self.editor.document();
        let text = |doc: &Document| {
            let spans = doc.spans(block).ok()?;
            Some(
                spans
                    .iter()
                    .map(|span| span.text.as_str())
                    .collect::<String>(),
            )
        };
        let (old, new) = (text(main)?, text(copy)?);
        let (mut spans, mut marks, mut at) = (Vec::new(), Marks::new(), 0);
        for part in word_diff(&old, &new) {
            let (from, range, mark) = match part {
                Diff::Kept(range) => (copy, range, None),
                Diff::Added(range) => (copy, range, Some(Mark::Added)),
                Diff::Removed(range) => (main, range, Some(Mark::Removed)),
            };
            let len = range.len();
            spans.extend(from.spans_in(block, range).ok()?);
            marks.extend(mark.map(|mark| (at..at + len, mark)));
            at += len;
        }
        // A change of the block's type or style alone has no words to mark.
        (!marks.is_empty()).then_some((spans, marks))
    }
}
