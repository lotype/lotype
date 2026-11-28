use super::Editor;
use crate::rules::{self, BlockRule, InlineMathRule, MarkRule};
use crate::{Atom, BlockId, Content, OBJECT, Pos, Result, Role, Selection, Span, mark};

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
        if rules::display_math_rule(typed)
            && let Some(math_name) = self
                .doc
                .schema()
                .display_math_block()
                .map(|spec| spec.name().to_owned())
            && self.takes_type(block, &math_name, text.len() == typed.len())?
        {
            self.apply_block_rule_named(block, typed.len(), &math_name, Vec::new())?;
            self.note_rule();
            return Ok(());
        }
        if let Some(rule) = rules::block_rule(typed)
            && self.takes(block, &rule, text.len() == typed.len())?
        {
            self.apply_block_rule(block, typed.len(), rule)?;
            self.note_rule();
            return Ok(());
        }
        // Code is typed as it is, in a code block and in inline code alike.
        if self.doc.role(block) == Role::Code || self.doc.marks_at(head)?.contains_key(mark::CODE) {
            return Ok(());
        }
        if let Some(rule) = rules::inline_math_rule(typed)
            && !self
                .doc
                .spans_in(block, rule.replace.clone())?
                .iter()
                .any(|span| span.marks.contains_key(mark::CODE))
            && let Some((name, attr)) = self
                .doc
                .schema()
                .inline_math_spec()
                .map(|(spec, attr)| (spec.name().to_owned(), attr.to_owned()))
        {
            self.atom_typed(block, rule, &name, &attr)?;
            self.note_rule();
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
    /// One in another block stays one: what is typed there is text.
    fn takes(&self, block: BlockId, rule: &BlockRule, at_end: bool) -> Result<bool> {
        self.takes_type(block, rule.block_type, at_end)
    }

    fn takes_type(&self, block: BlockId, block_type: &str, at_end: bool) -> Result<bool> {
        let Some(spec) = self.doc.schema().block_spec(block_type) else {
            return Ok(false);
        };
        let has_children = !self.doc.children(Some(block))?.is_empty();
        let text = self.doc.schema().text_block();
        Ok(
            text.is_some_and(|text| self.doc.block_type(block).is_ok_and(|kind| kind == text))
                && self.doc.parent(block)?.is_none()
                && (spec.allows_children() || !has_children)
                && (spec.content() == Content::Inline || at_end),
        )
    }

    fn apply_block_rule(&mut self, block: BlockId, typed: usize, rule: BlockRule) -> Result<()> {
        self.apply_block_rule_named(block, typed, rule.block_type, rule.attrs)
    }

    fn apply_block_rule_named(
        &mut self,
        block: BlockId,
        typed: usize,
        block_type: &str,
        attrs: Vec<(&'static str, crate::Value)>,
    ) -> Result<()> {
        self.apply(|doc, _| {
            doc.delete_text(block, 0..typed)?;
            doc.set_block_type(block, block_type)?;
            for (key, value) in attrs {
                doc.set_attr(block, key, value)?;
            }
            if doc.text_len(block).is_ok() {
                return Ok(Some(Selection::caret(Pos::new(block, 0))));
            }
            // The block holds no text now, so typing goes on below it.
            let (parent, index) = (doc.parent(block)?, doc.index_of(block)? + 1);
            let below = doc.create_block(parent, index, &doc.text_block()?)?;
            Ok(Some(Selection::caret(Pos::new(below, 0))))
        })
    }

    fn atom_typed(
        &mut self,
        block: BlockId,
        rule: InlineMathRule,
        atom_name: &str,
        attr_key: &str,
    ) -> Result<()> {
        let start = rule.replace.start;
        let marks = self
            .doc
            .marks_in(Pos::new(block, start), Pos::new(block, rule.replace.end))?;
        let atom = Atom::new(atom_name).attr(attr_key, rule.tex);
        let span = Span::atom(atom, marks);
        let caret = Pos::new(block, start + OBJECT.len_utf8());
        self.apply(|doc, _| {
            doc.delete_text(block, rule.replace)?;
            doc.insert_spans(block, start, std::slice::from_ref(&span))?;
            Ok(Some(Selection::caret(caret)))
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
