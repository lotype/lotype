use std::collections::BTreeMap;
use std::ops::Range;

use loro::cursor::PosType;
use loro::{LoroText, TextDelta};

use crate::atom::ATOM;
use crate::marks::{TypingMarks, visible_marks};
use crate::{Atom, BlockId, Content, Document, Error, MarkSpec, OBJECT, Pos, Result, Value};

/// A run of text with the same marks, or an atom, whose text is [`OBJECT`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Span {
    pub text: String,
    pub marks: BTreeMap<String, Value>,
    pub atom: Option<Atom>,
}

impl Span {
    pub fn new(text: impl Into<String>, marks: BTreeMap<String, Value>) -> Self {
        let text = text.into();
        Self {
            text,
            marks,
            atom: None,
        }
    }

    /// An atom, with marks such as bold that it shares with text.
    pub fn atom(atom: Atom, marks: BTreeMap<String, Value>) -> Self {
        let text = OBJECT.to_string();
        Self {
            text,
            marks,
            atom: Some(atom),
        }
    }
}

/// Rich text. Offsets are UTF-8 bytes and must fall on character boundaries.
impl Document {
    pub fn text(&self, block: BlockId) -> Result<String> {
        Ok(self.inline(block)?.to_string())
    }

    pub fn text_len(&self, block: BlockId) -> Result<usize> {
        Ok(self.inline(block)?.len_utf8())
    }

    pub fn spans(&self, block: BlockId) -> Result<Vec<Span>> {
        Ok(to_spans(self.inline(block)?.to_delta()))
    }

    pub fn spans_in(&self, block: BlockId, range: Range<usize>) -> Result<Vec<Span>> {
        let text = self.inline(block)?;
        let range = unicode_range(&text, range)?;
        let delta = text.slice_delta(range.start, range.end, PosType::Unicode)?;
        Ok(to_spans(delta))
    }

    pub fn insert_text(&mut self, block: BlockId, offset: usize, content: &str) -> Result<()> {
        let text = self.editable(block)?;
        let index = unicode_offset(&text, offset)?;
        if !content.is_empty() {
            text.insert(index, content)?;
            self.record(|| {
                let content = content.to_owned();
                Box::new(move |doc| doc.insert_text(block, offset, &content))
            });
        }
        Ok(())
    }

    /// Inserts text carrying exactly the given marks, whatever the marks around `offset`.
    pub fn insert_spans(&mut self, block: BlockId, offset: usize, spans: &[Span]) -> Result<()> {
        let text = self.editable(block)?;
        let index = unicode_offset(&text, offset)?;
        for (name, value) in spans.iter().flat_map(|span| &span.marks) {
            self.mark_spec(name)?.check_value(value)?;
        }
        for atom in spans.iter().filter_map(|span| span.atom.as_ref()) {
            if self.schema().inline_spec(&atom.name).is_none() {
                return Err(Error::UnknownAtom(atom.name.clone()));
            }
        }
        let inserts = spans.iter().map(|span| {
            let marks = span.marks.iter().map(|(k, v)| (k.clone(), v.to_loro()));
            let atom = span
                .atom
                .iter()
                .map(|atom| (ATOM.to_owned(), atom.to_loro()));
            // An atom is one character, whatever text the span was given.
            let insert = match span.atom {
                Some(_) => OBJECT.to_string(),
                None => span.text.clone(),
            };
            TextDelta::Insert {
                insert,
                attributes: Some(marks.chain(atom).collect()),
            }
        });
        insert_delta(&text, index, inserts.collect())?;
        self.record(|| {
            let spans = spans.to_vec();
            Box::new(move |doc| doc.insert_spans(block, offset, &spans))
        });
        Ok(())
    }

    pub(crate) fn insert_typed(
        &mut self,
        pos: Pos,
        content: &str,
        pending: Option<TypingMarks>,
    ) -> Result<()> {
        if content.is_empty() {
            return Ok(());
        }
        let text = self.editable(pos.block)?;
        let offset = unicode_offset(&text, pos.offset)?;
        let marks = match pending {
            Some(marks) => marks,
            None => self.typing_marks_in(&text, offset)?,
        };
        let insert = TextDelta::Insert {
            insert: content.to_owned(),
            attributes: Some(marks.into_iter().collect()),
        };
        insert_delta(&text, offset, vec![insert])
    }

    /// Copies a range of text to another place as stored, including marks
    /// that are not in the schema and mark values [`Value`] cannot hold.
    pub(crate) fn copy_text(
        &mut self,
        from: BlockId,
        range: Range<usize>,
        to: BlockId,
        offset: usize,
    ) -> Result<()> {
        let source = self.inline(from)?;
        let indices = unicode_range(&source, range.clone())?;
        let target = self.editable(to)?;
        let index = unicode_offset(&target, offset)?;
        let mut delta = source.slice_delta(indices.start, indices.end, PosType::Unicode)?;
        // Without attributes an insert would take on the marks around it.
        for item in &mut delta {
            if let TextDelta::Insert { attributes, .. } = item {
                attributes.get_or_insert_default();
            }
        }
        insert_delta(&target, index, delta)?;
        self.record(|| Box::new(move |doc| doc.copy_text(from, range, to, offset)));
        Ok(())
    }

    pub fn delete_text(&mut self, block: BlockId, range: Range<usize>) -> Result<()> {
        let text = self.inline(block)?;
        let indices = unicode_range(&text, range.clone())?;
        if !indices.is_empty() {
            text.delete(indices.start, indices.len())?;
            self.record(|| Box::new(move |doc| doc.delete_text(block, range)));
        }
        Ok(())
    }

    pub fn mark(
        &mut self,
        block: BlockId,
        range: Range<usize>,
        name: &str,
        value: impl Into<Value>,
    ) -> Result<()> {
        let value = value.into();
        self.mark_spec(name)?.check_value(&value)?;
        let text = self.inline(block)?;
        let indices = unicode_range(&text, range.clone())?;
        if !indices.is_empty() {
            text.mark(indices, name, value.to_loro())?;
            self.record(|| {
                let name = name.to_owned();
                Box::new(move |doc| doc.mark(block, range, &name, value))
            });
        }
        Ok(())
    }

    pub fn unmark(&mut self, block: BlockId, range: Range<usize>, name: &str) -> Result<()> {
        self.mark_spec(name)?;
        let text = self.inline(block)?;
        let indices = unicode_range(&text, range.clone())?;
        if !indices.is_empty() {
            text.unmark(indices, name)?;
            self.record(|| {
                let name = name.to_owned();
                Box::new(move |doc| doc.unmark(block, range, &name))
            });
        }
        Ok(())
    }

    pub(crate) fn check_offset(&self, block: BlockId, offset: usize) -> Result<()> {
        unicode_offset(&self.inline(block)?, offset).map(drop)
    }

    pub(crate) fn inline(&self, block: BlockId) -> Result<LoroText> {
        if self.spec(block)?.content() != Content::Inline {
            return Err(Error::NoInlineContent);
        }
        self.raw_text(block)
    }

    /// The text of a block, to insert into. A cell gets its text here: it
    /// has none of its own until something is written in it.
    pub(crate) fn editable(&self, block: BlockId) -> Result<LoroText> {
        let text = self.inline(block)?;
        match block.1.filter(|_| !text.is_attached()) {
            Some(column) => self.make_cell(block.0, column),
            None => Ok(text),
        }
    }

    pub(crate) fn mark_spec(&self, name: &str) -> Result<&MarkSpec> {
        self.schema()
            .mark_spec(name)
            .ok_or_else(|| Error::UnknownMark(name.to_owned()))
    }
}

fn insert_delta(text: &LoroText, offset: usize, inserts: Vec<TextDelta>) -> Result<()> {
    if inserts.is_empty() {
        return Ok(());
    }
    let mut delta = Vec::with_capacity(inserts.len() + 1);
    if offset > 0 {
        delta.push(TextDelta::Retain {
            retain: offset,
            attributes: None,
        });
    }
    delta.extend(inserts);
    Ok(text.apply_delta(&delta)?)
}

pub(crate) fn to_spans(delta: Vec<TextDelta>) -> Vec<Span> {
    let mut spans = Vec::with_capacity(delta.len());
    for item in delta {
        let TextDelta::Insert { insert, attributes } = item else {
            continue;
        };
        let attributes = attributes.unwrap_or_default();
        let atom = attributes.get(ATOM).and_then(Atom::from_loro);
        let marks = visible_marks(attributes);
        let Some(atom) = atom else {
            spans.push(Span::new(insert, marks));
            continue;
        };
        // A peer may have put the mark on more than U+FFFC: each of those
        // characters is an atom, and anything else is text.
        let mut text = String::new();
        for c in insert.chars() {
            if c != OBJECT {
                text.push(c);
                continue;
            }
            if !text.is_empty() {
                spans.push(Span::new(std::mem::take(&mut text), marks.clone()));
            }
            spans.push(Span::atom(atom.clone(), marks.clone()));
        }
        if !text.is_empty() {
            spans.push(Span::new(text, marks));
        }
    }
    spans
}

fn unicode_range(text: &LoroText, range: Range<usize>) -> Result<Range<usize>> {
    if range.start > range.end {
        return Err(Error::InvalidRange {
            start: range.start,
            end: range.end,
        });
    }
    Ok(unicode_offset(text, range.start)?..unicode_offset(text, range.end)?)
}

pub(crate) fn unicode_offset(text: &LoroText, offset: usize) -> Result<usize> {
    let unicode = text.convert_pos(offset, PosType::Bytes, PosType::Unicode);
    let unicode = unicode.ok_or_else(|| Error::OutOfBounds {
        index: offset,
        len: text.len_utf8(),
    })?;
    // Loro rounds an offset inside a character down instead of rejecting it.
    if text.convert_pos(unicode, PosType::Unicode, PosType::Bytes) != Some(offset) {
        return Err(Error::NotCharBoundary(offset));
    }
    Ok(unicode)
}
