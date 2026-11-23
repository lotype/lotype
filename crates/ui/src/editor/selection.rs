//! Whole blocks as the selection. Escape selects the blocks a text
//! selection is in; arrows then move from block to block and Shift extends;
//! Enter goes back into the text and Backspace or Delete removes them.

use std::collections::HashSet;

use lotype_core::{BlockId, Pos, Selection};

use super::EditorView;

impl EditorView {
    /// Handles a key on whole blocks. Returns whether it did.
    pub(super) fn block_key(&mut self, name: &str) -> bool {
        match self.editor.selection().cloned() {
            Some(Selection::Text { anchor, head }) if name == "Escape" => {
                let Ok((from, to)) = self.editor.document().ordered(anchor, head) else {
                    return false;
                };
                // Escape goes on to the application where no row is selected.
                self.select_rows(from.block, to.block)
            }
            Some(Selection::Blocks(blocks)) => {
                let range = self
                    .block_range
                    .filter(|(anchor, head)| blocks.contains(anchor) && blocks.contains(head))
                    .or_else(|| Some((*blocks.first()?, *blocks.last()?)));
                // An application can select no blocks at all.
                let Some((anchor, head)) = range else {
                    return false;
                };
                match name {
                    "ArrowUp" | "ArrowDown" => self.step(head, name == "ArrowDown", None),
                    "Shift-ArrowUp" | "Shift-ArrowDown" => {
                        self.step(head, name == "Shift-ArrowDown", Some(anchor));
                    }
                    "Enter" => return self.enter_text(head),
                    "Backspace" | "Delete" => self.delete_selection(),
                    "Escape" => {}
                    _ => return false,
                }
                true
            }
            _ => false,
        }
    }

    /// Selects the rows from `anchor` to `head`, whole. Returns whether it did.
    pub(super) fn select_rows(&mut self, anchor: BlockId, head: BlockId) -> bool {
        let at = |block| self.rows.iter().position(|row| *row == block);
        let (Some(from), Some(to)) = (at(anchor), at(head)) else {
            return false;
        };
        let rows = &self.rows[from.min(to)..=from.max(to)];
        let blocks = Selection::Blocks(rows.to_vec());
        let selected = self.editor.set_selection(Some(blocks)).is_ok();
        if selected {
            self.block_range = Some((anchor, head));
        }
        selected
    }

    /// Puts the caret at the end of the text of `head`, or of the nearest
    /// block with text after it or else before it. Returns whether it did.
    fn enter_text(&mut self, head: BlockId) -> bool {
        let doc = self.editor.document();
        let Some(at) = self.rows.iter().position(|row| *row == head) else {
            return false;
        };
        let (before, after) = self.rows.split_at(at);
        let text = |block: &&BlockId| doc.text_len(**block).is_ok();
        let Some(block) = after
            .iter()
            .find(text)
            .or_else(|| before.iter().rev().find(text))
        else {
            return false;
        };
        let end = Pos::new(*block, doc.text_len(*block).unwrap_or(0));
        self.editor
            .set_selection(Some(Selection::caret(end)))
            .is_ok()
    }

    /// Moves the head of a block selection to the next or previous row,
    /// keeping `anchor` where Shift extends.
    fn step(&mut self, head: BlockId, forward: bool, anchor: Option<BlockId>) {
        let Some(at) = self.rows.iter().position(|row| *row == head) else {
            return;
        };
        let to = if forward { at + 1 } else { at.wrapping_sub(1) };
        let Some(next) = self.rows.get(to).copied() else {
            return;
        };
        self.select_rows(anchor.unwrap_or(next), next);
    }

    /// Deletes the selection. Blocks deleted whole leave the caret at the end
    /// of the text before them, or the start of the text after them.
    pub(super) fn delete_selection(&mut self) {
        let Some(Selection::Blocks(blocks)) = self.editor.selection().cloned() else {
            self.editor.delete_selection().ok();
            return;
        };
        let doc = self.editor.document();
        let chosen: HashSet<_> = blocks.iter().copied().collect();
        let inside = |block: BlockId| {
            let mut at = Some(block);
            while let Some(block) = at {
                if chosen.contains(&block) {
                    return true;
                }
                at = doc.parent(block).ok().flatten();
            }
            false
        };
        // The rows are those of the last frame: with none selected, the caret
        // is left to the core.
        let first = self.rows.iter().position(|row| chosen.contains(row));
        let (before, after) = self.rows.split_at(first.unwrap_or(self.rows.len()));
        let text = |block: &&BlockId| doc.text_len(**block).is_ok();
        let caret = match before.iter().rev().find(text).filter(|_| first.is_some()) {
            Some(block) => doc.text_len(*block).ok().map(|len| Pos::new(*block, len)),
            None => after
                .iter()
                .filter(|block| !inside(**block))
                .find(text)
                .map(|block| Pos::new(*block, 0)),
        };
        if self.editor.delete_selection().is_ok()
            && let Some(caret) = caret
        {
            self.editor
                .set_selection(Some(Selection::caret(caret)))
                .ok();
        }
    }
}
