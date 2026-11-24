use loro::TreeParentId;
use unicode_segmentation::GraphemeCursor;

use super::{Editor, text_selection};
use crate::editor::BLOCK_STYLE;
use crate::{BlockId, Document, Error, Pos, Result, Selection};

const PARAGRAPH: &str = "paragraph";
const LIST_ITEMS: [&str; 4] = [
    "bullet_list_item",
    "numbered_list_item",
    "check_list_item",
    "toggle_list_item",
];

/// Makes `block` a paragraph, keeping its text, its children and its style.
fn make_paragraph(doc: &mut Document, block: BlockId) -> Result<()> {
    let paragraph = doc.schema().block_spec(PARAGRAPH);
    let style = paragraph.is_some_and(|spec| spec.takes_style());
    for key in doc.attrs(block)?.into_keys() {
        if !(style && BLOCK_STYLE.contains(&key.as_str())) {
            doc.remove_attr(block, &key)?;
        }
    }
    doc.set_block_type(block, PARAGRAPH)
}

/// The first block after `block` in document order that holds text.
pub(super) fn next_text_block(doc: &Document, block: BlockId) -> Result<Option<BlockId>> {
    let mut at = doc.next_block(block)?;
    while let Some(next) = at {
        if doc.text_len(next).is_ok() {
            return Ok(Some(next));
        }
        at = doc.next_block(next)?;
    }
    Ok(None)
}

/// Consecutive typing or deleting that undoes as one step.
pub(super) struct Run {
    kind: Kind,
    /// Where the run left the caret. The next keystroke extends the run only from here.
    caret: Pos,
    ends_with_space: bool,
}

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Insert,
    DeleteBackward,
    DeleteForward,
}

impl Editor {
    /// Replaces the selected text with `text`.
    ///
    /// Text typed in sequence undoes one word at a time: a run of calls is one
    /// undo step until a word starts after whitespace, or anything else happens.
    ///
    /// Markdown typed at the start of a paragraph turns it into that block:
    /// `# ` to `##### `, `- `, `1. `, `[] `, `> `, `` ``` `` with or without
    /// a language and then a space, and `---`. Text typed between `**`, `*`,
    /// `_`, `~~` or backticks gets that mark, except in code. Each is a step
    /// of its own, so undo gives back what was typed.
    pub fn insert_text(&mut self, text: &str) -> Result<()> {
        let (anchor, head) = text_selection(self.selection.as_ref())?;
        let starts_word = text.chars().next().is_some_and(|c| !c.is_whitespace());
        let extends = anchor == head
            && self.extends(Kind::Insert, head)
            && !(starts_word && self.run.as_ref().is_some_and(|run| run.ends_with_space));
        let marks = self.pending_marks.clone();
        self.run_step(Kind::Insert, extends, text, |doc| {
            let caret = doc.delete_range(anchor, head)?;
            doc.insert_typed(caret, text, marks)?;
            Ok(Pos::new(caret.block, caret.offset + text.len()))
        })?;
        // Every input rule ends in one of these, and most keystrokes do not.
        if text.ends_with([' ', '-', '`', '*', '_', '~']) {
            self.input_rule()?;
        }
        Ok(())
    }

    /// Deletes the selection, or the character before the caret, as Backspace
    /// does. At the start of a block it joins the block onto the one before it.
    pub fn delete_backward(&mut self) -> Result<()> {
        self.delete_character(Kind::DeleteBackward)
    }

    /// Deletes the selection, or the character after the caret, as Delete
    /// does. At the end of a block it joins the next block onto this one.
    pub fn delete_forward(&mut self) -> Result<()> {
        self.delete_character(Kind::DeleteForward)
    }

    fn delete_character(&mut self, kind: Kind) -> Result<()> {
        if self
            .selection
            .as_ref()
            .is_some_and(|sel| !sel.is_collapsed())
        {
            return self.delete_selection();
        }
        let (_, head) = text_selection(self.selection.as_ref())?;
        self.pending_marks = None;
        // Backspace right after an input rule takes the rule back, and
        // leaves what was typed.
        let rule = self.rule.take();
        let unchanged = rule == Some((self.doc.version(), self.selection.clone()));
        if kind == Kind::DeleteBackward && unchanged && self.can_undo() {
            return self.undo().map(drop);
        }
        let len = self.doc.text_len(head.block)?;
        // A block that is no paragraph becomes one first, keeping its text,
        // where Backspace is at its start or Delete in it once it is empty
        // and the last; another press then joins it, as for any paragraph.
        let edge = match kind {
            Kind::DeleteBackward => head.offset == 0,
            _ => len == 0 && next_text_block(&self.doc, head.block)?.is_none(),
        };
        if edge && self.becomes_paragraph(head.block)? {
            return self.apply(|doc, selection| {
                make_paragraph(doc, head.block)?;
                Ok(selection.cloned())
            });
        }
        match kind {
            Kind::DeleteBackward if head.offset == 0 => self.join_or_drop(head),
            Kind::DeleteForward if head.offset == len => {
                self.apply(|doc, _| Ok(Some(Selection::caret(join_forward(doc, head)?))))
            }
            _ => self.delete_grapheme(head, kind),
        }
    }

    /// Enter in an empty list item with nothing in it leaves the list, as in
    /// word processors: one in another list item moves out a level, and any
    /// other becomes a paragraph. Returns whether it did.
    pub(super) fn leave_list(&mut self) -> Result<bool> {
        let Some(Selection::Text { anchor, head }) = self.selection else {
            return Ok(false);
        };
        let list_item = |kind: &str| LIST_ITEMS.contains(&kind);
        let block = head.block;
        let empty = anchor == head
            && block.column().is_none()
            && list_item(&self.doc.block_type(block)?)
            && self.doc.text_len(block)? == 0
            && self.doc.children(Some(block))?.is_empty();
        if !empty || !self.becomes_paragraph(block)? {
            return Ok(false);
        }
        let parent = self.doc.parent(block)?;
        let nested = parent.is_some_and(|parent| {
            self.doc
                .block_type(parent)
                .is_ok_and(|kind| list_item(&kind))
        });
        self.apply(|doc, selection| {
            if !(nested && doc.outdent_block(block)?) {
                make_paragraph(doc, block)?;
            }
            Ok(selection.cloned())
        })?;
        Ok(true)
    }

    /// Enter in `cell`: the caret goes to the end of the cell below it, or
    /// from the last row into a new paragraph after the table, as Enter at
    /// the end of a block starts the next one.
    pub(super) fn enter_in_cell(&mut self, cell: BlockId) -> Result<()> {
        let row = self.doc.parent(cell)?.ok_or(Error::NotATable)?;
        let table = self.doc.parent(row)?.ok_or(Error::NotATable)?;
        let rows = self.doc.children(Some(table))?;
        let next = rows
            .iter()
            .position(|at| *at == row)
            .and_then(|at| rows.get(at + 1));
        let column = cell.column().ok_or(Error::NotATable)?;
        if let Some(next) = next {
            let below = self.doc.cell(*next, column)?;
            let caret = Pos::new(below, self.doc.text_len(below)?);
            return self.set_selection(Some(Selection::caret(caret)));
        }
        self.apply(|doc, _| {
            let after = doc.index_of(table)? + 1;
            let paragraph = doc.create_block(doc.parent(table)?, after, PARAGRAPH)?;
            Ok(Some(Selection::caret(Pos::new(paragraph, 0))))
        })
    }

    /// Whether `block` is no paragraph and can become one where it is: a
    /// cell cannot, nor a block in a schema without paragraphs.
    fn becomes_paragraph(&self, block: BlockId) -> Result<bool> {
        let kind = self.doc.block_type(block)?;
        let paragraph = self.doc.schema().block_spec(PARAGRAPH);
        Ok(kind != PARAGRAPH && block.column().is_none() && paragraph.is_some())
    }

    /// Backspace at the start of a paragraph: joins it onto the block before
    /// it, or, for an empty one at the very start, removes it and goes to
    /// the text after it, so that no empty block is left there for good.
    /// The first block in an alert, which has no text to join onto, goes out
    /// before it, and takes the alert away once it is left empty.
    fn join_or_drop(&mut self, head: Pos) -> Result<()> {
        let prev = self.doc.prev_block(head.block)?;
        let parent = self.doc.parent(head.block)?;
        if let Some(parent) = parent.filter(|parent| {
            prev == Some(*parent)
                && self.doc.text_len(*parent).is_err()
                && !self.doc.is_table_part(*parent)
        }) {
            return self.apply(|doc, selection| {
                doc.move_block_next_to(head.block, parent, false)?;
                doc.remove_if_emptied(Some(parent))?;
                Ok(selection.cloned())
            });
        }
        if prev.is_some() {
            return self.join_backward();
        }
        let empty =
            self.doc.text_len(head.block)? == 0 && self.doc.children(Some(head.block))?.is_empty();
        let next = next_text_block(&self.doc, head.block)?;
        let Some(next) = next.filter(|_| empty) else {
            return Ok(());
        };
        self.apply(|doc, _| {
            doc.remove_block(head.block)?;
            Ok(Some(Selection::caret(Pos::new(next, 0))))
        })
    }

    fn delete_grapheme(&mut self, head: Pos, kind: Kind) -> Result<()> {
        let extends = self.extends(kind, head);
        self.run_step(kind, extends, "", |doc| {
            doc.check_pos(head)?;
            let text = doc.text(head.block)?;
            let cursor = || GraphemeCursor::new(head.offset, text.len(), true);
            // A scalar boundary inside a grapheme is a valid caret position.
            // Use the whole text so either key can remove the whole grapheme.
            let at_boundary = cursor().is_boundary(&text, 0).expect("full text provided");
            let start = if at_boundary && kind == Kind::DeleteForward {
                head.offset
            } else {
                cursor()
                    .prev_boundary(&text, 0)
                    .expect("full text provided")
                    .unwrap_or(0)
            };
            let end = if at_boundary && kind == Kind::DeleteBackward {
                head.offset
            } else {
                cursor()
                    .next_boundary(&text, 0)
                    .expect("full text provided")
                    .unwrap_or(text.len())
            };
            doc.delete_text(head.block, start..end)?;
            Ok(Pos::new(head.block, start))
        })
    }

    /// Ends the current typing or deletion group so the next edit starts a new undo step.
    /// The selection and pending marks stay active.
    pub fn finish_edit_group(&mut self) {
        if self.run.take().is_some() {
            self.history.group_end();
        }
    }

    fn extends(&self, kind: Kind, caret: Pos) -> bool {
        matches!(&self.run, Some(run) if run.kind == kind && run.caret == caret)
    }

    /// Runs one keystroke's edit, which returns the new caret, and keeps its
    /// undo step open as a run. `typed` is the text the keystroke inserted.
    fn run_step(
        &mut self,
        kind: Kind,
        extends: bool,
        typed: &str,
        edit: impl FnOnce(&mut Document) -> Result<Pos>,
    ) -> Result<()> {
        let mut caret = None;
        let result = self.step(extends, |doc, _| {
            let pos = edit(doc)?;
            caret = Some(pos);
            Ok(Some(Selection::caret(pos)))
        });
        match (result, caret) {
            (Ok(true), Some(caret)) => {
                let ends_with_space = typed.chars().next_back().is_some_and(char::is_whitespace);
                self.run = Some(Run {
                    kind,
                    caret,
                    ends_with_space,
                });
                Ok(())
            }
            (result, _) => {
                // Nothing changed or the edit failed: nothing to extend later.
                self.run = None;
                self.history.group_end();
                result.map(drop)
            }
        }
    }
}

/// Joins the block after `block` onto it. A textless block there is removed
/// instead, unless it has children: then the first of those joins on, as
/// the first block in an alert, and an alert left empty goes.
fn join_forward(doc: &mut Document, caret: Pos) -> Result<Pos> {
    let Some(next) = doc.next_block(caret.block)? else {
        return Ok(caret);
    };
    if doc.text_len(next).is_ok() {
        return Ok(doc.merge_backward(next)?.unwrap_or(caret));
    }
    if doc
        .child_ids(TreeParentId::Node(doc.live(next)?))
        .is_empty()
    {
        doc.remove_block(next)?;
        return Ok(caret);
    }
    let first = doc.children(Some(next))?.first().copied();
    if let Some(first) = first.filter(|first| doc.text_len(*first).is_ok())
        && !doc.is_table_part(next)
    {
        doc.copy_text(first, 0..doc.text_len(first)?, caret.block, caret.offset)?;
        doc.dissolve_block(first)?;
        doc.remove_if_emptied(Some(next))?;
    }
    Ok(caret)
}
