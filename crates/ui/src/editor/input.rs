//! Text from the system: typing, and IME composition.
//!
//! The system sees one block, the one the cursor is in, and counts in UTF-16.
//! Text being composed stays in the view, in place of the selection, until
//! it is committed: peers and the undo history only ever get finished text.

use std::ops::Range;

use gpui::{Bounds, Context, EntityInputHandler, Pixels, Point, UTF16Selection, Window, size};
use lotype_core::{BlockId, Pos, Selection};

use super::EditorView;
use crate::text_el::Affinity;

/// Text being composed, which replaces the selection on screen.
pub(crate) struct Composition {
    pub(crate) text: String,
    /// What the IME has selected in `text`, in bytes.
    pub(crate) selected: Range<usize>,
}

/// The block the system edits, as it is shown.
pub(crate) struct Shown {
    pub(crate) block: BlockId,
    pub(crate) text: String,
    /// The part of the block's text that the selection covers, in bytes.
    replaced: Range<usize>,
    /// Where the composed text is in `text`.
    pub(crate) marked: Option<Range<usize>>,
    /// The selection in `text`, and whether its head comes first.
    pub(crate) selected: (Range<usize>, bool),
}

impl Shown {
    /// The place in the block's text for a place in the shown text.
    pub(super) fn to_doc(&self, at: usize) -> usize {
        match &self.marked {
            Some(marked) if at >= marked.end => at - marked.len() + self.replaced.len(),
            Some(marked) if at > marked.start => self.replaced.start,
            _ => at,
        }
    }

    fn utf16(&self, range: &Range<usize>) -> Range<usize> {
        to_utf16(&self.text, range.start)..to_utf16(&self.text, range.end)
    }

    fn utf8(&self, range: &Range<usize>) -> Range<usize> {
        let start = from_utf16(&self.text, range.start);
        start..from_utf16(&self.text, range.end).max(start)
    }
}

fn to_utf16(text: &str, at: usize) -> usize {
    text[..at].encode_utf16().count()
}

/// The byte offset of a UTF-16 offset, rounded down to a character.
fn from_utf16(text: &str, at: usize) -> usize {
    let mut units = 0;
    for (offset, c) in text.char_indices() {
        units += c.len_utf16();
        if units > at {
            return offset;
        }
    }
    text.len()
}

impl EditorView {
    pub(crate) fn shown(&self) -> Option<Shown> {
        let doc = self.editor.document();
        let Some(Selection::Text { anchor, head }) = self.editor.selection() else {
            return None;
        };
        let text = doc.text(head.block).ok()?;
        let (from, to) = doc.ordered(*anchor, *head).ok()?;
        let replaced = if from.block == to.block {
            from.offset..to.offset
        } else if *head == to {
            0..head.offset
        } else {
            head.offset..text.len()
        };
        let backward = head.offset < anchor.offset && anchor.block == head.block;
        let Some(composition) = &self.composition else {
            let selected = (replaced.clone(), backward);
            let (block, marked) = (head.block, None);
            return Some(Shown {
                block,
                text,
                replaced,
                marked,
                selected,
            });
        };
        let start = replaced.start;
        let marked = start..start + composition.text.len();
        let selected = start + composition.selected.start..start + composition.selected.end;
        let text = [&text[..start], &composition.text, &text[replaced.end..]].concat();
        Some(Shown {
            block: head.block,
            text,
            replaced,
            marked: Some(marked),
            selected: (selected, false),
        })
    }

    /// Selects a range of the block's text, unless it is selected already:
    /// setting the selection drops the marks chosen for the next typed text.
    fn select_in(&mut self, block: BlockId, range: Range<usize>) {
        let selection = Selection::Text {
            anchor: Pos::new(block, range.start),
            head: Pos::new(block, range.end),
        };
        if self.editor.selection() != Some(&selection) {
            self.editor.set_selection(Some(selection)).ok();
        }
    }

    /// Ends the composition with its text in the document.
    /// Gives a document with no blocks a paragraph with the caret in it, on
    /// the first click, key or text: a document is changed only once
    /// someone writes in it. Returns whether it did.
    pub(crate) fn start_writing(&mut self) -> bool {
        self.editor.start_writing().unwrap_or(false)
    }

    pub(crate) fn commit(&mut self, text: &str) {
        self.composition = None;
        if !text.is_empty() {
            self.editor.insert_text(text).ok();
        }
    }

    fn changed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.reveal.set(true);
        self.restart_blink(window, cx);
        window.notify_text_input_changed(&self.focus);
        cx.notify();
    }
}

impl EntityInputHandler for EditorView {
    fn text_for_range(
        &mut self,
        range: Range<usize>,
        actual: &mut Option<Range<usize>>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<String> {
        let shown = self.shown()?;
        let range = shown.utf8(&range);
        *actual = Some(shown.utf16(&range));
        Some(shown.text[range].to_owned())
    }

    fn selected_text_range(
        &mut self,
        _: bool,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        let shown = self.shown()?;
        let (range, reversed) = &shown.selected;
        let range = shown.utf16(range);
        let reversed = *reversed;
        Some(UTF16Selection { range, reversed })
    }

    fn set_selected_text_range(
        &mut self,
        range: Range<usize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(shown) = self.shown() else { return };
        let range = shown.utf8(&range);
        match (&mut self.composition, &shown.marked) {
            (Some(composition), Some(marked)) => {
                let len = composition.text.len();
                let inside = |at: usize| at.clamp(marked.start, marked.end) - marked.start;
                composition.selected = inside(range.start).min(len)..inside(range.end).min(len);
            }
            _ => self.select_in(shown.block, range),
        }
        window.notify_text_selection_changed(&self.focus);
        cx.notify();
    }

    fn text_length_utf16(&mut self, _: &mut Window, _: &mut Context<Self>) -> Option<usize> {
        self.shown()
            .map(|shown| to_utf16(&shown.text, shown.text.len()))
    }

    fn marked_text_range(&self, _: &mut Window, _: &mut Context<Self>) -> Option<Range<usize>> {
        let shown = self.shown()?;
        shown.marked.as_ref().map(|marked| shown.utf16(marked))
    }

    fn unmark_text(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(composition) = self.composition.take() {
            self.commit(&composition.text);
            self.changed(window, cx);
        }
    }

    fn replace_text_in_range(
        &mut self,
        range: Option<Range<usize>>,
        text: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.ai.session.is_some() {
            return;
        }
        self.start_writing();
        if range.is_none() && self.ai_space(text, window, cx) {
            return;
        }
        let Some(shown) = self.shown() else { return };
        // Typing over a selection that spans blocks replaces all of it, as
        // the editor does; any other range is in this block.
        let range = range
            .map(|range| shown.utf8(&range))
            .or(shown.marked.clone());
        let mut text = text.to_owned();
        if let Some(range) = range {
            let marked = shown.marked.as_ref();
            let touches =
                marked.filter(|marked| range.end >= marked.start && range.start <= marked.end);
            if let Some(marked) = touches {
                // What is replaced is part of the composed text, or next to
                // it, and the rest of that is committed with it.
                let (start, end) = (range.start.min(marked.start), range.end.max(marked.end));
                text = [
                    &shown.text[start..range.start],
                    &text,
                    &shown.text[range.end..end],
                ]
                .concat();
                self.select_in(shown.block, shown.to_doc(start)..shown.to_doc(end));
            } else if let Some(marked) = marked.cloned() {
                // Text apart from the composition: that goes in first, as
                // it is. Text after it is then found from the caret, which
                // ends what went in.
                if let Some(composition) = self.composition.take() {
                    self.commit(&composition.text);
                }
                let caret = match self.editor.selection() {
                    Some(Selection::Text { head, .. }) => *head,
                    _ => Pos::new(shown.block, marked.end),
                };
                let place = |at: usize| match at >= marked.end {
                    true => caret.offset + at - marked.end,
                    false => at,
                };
                self.select_in(caret.block, place(range.start)..place(range.end));
            } else {
                self.select_in(shown.block, range);
            }
        }
        let text = text.as_str();
        self.commit(text);
        if text.is_empty() {
            self.editor.delete_selection().ok();
        }
        self.changed(window, cx);
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range: Option<Range<usize>>,
        text: &str,
        selected: Option<Range<usize>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.ai.session.is_some() {
            return;
        }
        self.start_writing();
        let Some(shown) = self.shown() else { return };
        if let Some(range) = range {
            let range = shown.utf8(&range);
            self.select_in(
                shown.block,
                shown.to_doc(range.start)..shown.to_doc(range.end),
            );
        } else if self.composition.is_none()
            && matches!(self.editor.selection(), Some(Selection::Text { anchor, head }) if anchor.block != head.block)
        {
            // What is composed replaces one block's text only.
            self.editor.delete_selection().ok();
        }
        self.composition = (!text.is_empty()).then(|| {
            let selected = selected.map_or(text.len()..text.len(), |range| {
                let start = from_utf16(text, range.start);
                start..from_utf16(text, range.end).max(start)
            });
            Composition {
                text: text.to_owned(),
                selected,
            }
        });
        self.changed(window, cx);
    }

    fn bounds_for_range(
        &mut self,
        range: Range<usize>,
        _: Bounds<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let shown = self.shown()?;
        let layout = self.layouts.get(&shown.block)?;
        if layout.text()? != shown.text {
            return None;
        }
        let range = shown.utf8(&range);
        let start = layout.caret(range.start, Affinity::Downstream)?;
        // A range that wraps is given as its first line's part.
        let end = layout
            .caret(range.end, Affinity::Upstream)
            .filter(|end| end.top() == start.top());
        let width = end.map_or(Pixels::ZERO, |end| end.left() - start.left());
        Some(Bounds::new(start.origin, size(width, start.size.height)))
    }

    fn character_index_for_point(
        &mut self,
        at: Point<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<usize> {
        let shown = self.shown()?;
        let layout = self.layouts.get(&shown.block)?;
        if layout.text()? != shown.text {
            return None;
        }
        let (index, _) = layout.hit(at)?;
        Some(to_utf16(&shown.text, index.min(shown.text.len())))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utf16_offsets_round_down_to_characters() {
        let text = "e🙂漢";
        assert_eq!(to_utf16(text, 5), 3);
        assert_eq!(from_utf16(text, 1), 1);
        assert_eq!(from_utf16(text, 2), 1);
        assert_eq!(from_utf16(text, 3), 5);
        assert_eq!(from_utf16(text, 99), text.len());
    }
}
