use super::Editor;
use crate::rules::{self, BlockRule, MarkRule};
use crate::{BlockId, Content, Pos, Result, Selection};

impl Editor {
    /// Applies the input rule that the text before the caret asks for, as a
    /// step of its own, so that undo gives back what was typed.
    pub(super) fn input_rule(&mut self) -> Result<()> {
        let Some(Selection::Text { anchor, head }) = self.selection.clone() else {
            return Ok(());
        };
        if anchor != head {
            return Ok(());
        }
        let block = head.block;
        let text = self.doc.text(block)?;
        let typed = &text[..head.offset];
        if let Some(rule) = rules::block_rule(typed)
            && self.takes(block, &rule, text.len() == typed.len())?
        {
            self.turn_into(block, typed.len(), rule)?;
            self.note_rule();
            return Ok(());
        }
        // Code is typed as it is, in a code block and in inline code alike.
        if self.doc.block_type(block)? == "code_block"
            || self.doc.marks_at(head)?.contains_key("code")
        {
            return Ok(());
        }
        let on_off = |name| {
            self.doc
                .mark_spec(name)
                .is_ok_and(|spec| spec.allows_boolean())
        };
        match rules::mark_rule(typed) {
            Some(rule) if on_off(rule.mark) => {
                self.mark_typed(block, rule)?;
                self.note_rule();
                Ok(())
            }
            _ => Ok(()),
        }
    }

    /// Notes that an input rule was just applied, for Backspace to take back.
    fn note_rule(&mut self) {
        self.rule = Some((self.doc.version(), self.selection.clone()));
    }

    /// Whether `block` is a paragraph that can become what the rule says.
    fn takes(&self, block: BlockId, rule: &BlockRule, at_end: bool) -> Result<bool> {
        let Some(spec) = self.doc.schema().block_spec(rule.block_type) else {
            return Ok(false);
        };
        let has_children = !self.doc.children(Some(block))?.is_empty();
        Ok(self.doc.block_type(block)? == "paragraph"
            && (spec.allows_children() || !has_children)
            && (spec.content() == Content::Inline || at_end))
    }

    fn turn_into(&mut self, block: BlockId, typed: usize, rule: BlockRule) -> Result<()> {
        self.apply(|doc, _| {
            doc.delete_text(block, 0..typed)?;
            doc.set_block_type(block, rule.block_type)?;
            for (key, value) in rule.attrs {
                doc.set_attr(block, key, value)?;
            }
            if doc.text_len(block).is_ok() {
                return Ok(Some(Selection::caret(Pos::new(block, 0))));
            }
            // The block holds no text now, so typing goes on below it.
            let below = doc.create_block(doc.parent(block)?, doc.index_of(block)? + 1, "paragraph");
            Ok(Some(Selection::caret(Pos::new(below?, 0))))
        })
    }

    fn mark_typed(&mut self, block: BlockId, rule: MarkRule) -> Result<()> {
        let caret = Pos::new(block, rule.close.start - rule.open.len());
        self.apply(|doc, _| {
            doc.delete_text(block, rule.close.clone())?;
            doc.mark(block, rule.open.end..rule.close.start, rule.mark, true)?;
            doc.delete_text(block, rule.open.clone())?;
            Ok(Some(Selection::caret(caret)))
        })?;
        // What is typed next is outside the mark, even where the mark expands.
        self.edit_pending_marks(caret, |marks| {
            marks.remove(rule.mark);
        })
    }
}
