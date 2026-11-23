use std::collections::BTreeMap;

use loro::cursor::PosType;
use loro::{LoroText, LoroValue, TextDelta};

use crate::atom::ATOM;
use crate::text::unicode_offset;
use crate::{Document, Expand, Pos, Result, Value};

pub(crate) type TypingMarks = BTreeMap<String, LoroValue>;

pub(crate) fn visible_marks(
    marks: impl IntoIterator<Item = (String, LoroValue)>,
) -> BTreeMap<String, Value> {
    // Names starting with `$` are the core's own, as the atom mark is,
    // whatever value a peer gave them.
    marks
        .into_iter()
        .filter(|(name, _)| !name.starts_with('$'))
        .filter_map(|(name, value)| Some((name, Value::from_loro(&value)?)))
        .collect()
}

impl Document {
    /// The marks that [`Editor::insert_text`](crate::Editor::insert_text) uses at `pos`.
    ///
    /// A mark carries over when both sides have the same value or its expand
    /// rule reaches `pos`. When both sides expand, the preceding value wins.
    pub fn marks_at(&self, pos: Pos) -> Result<BTreeMap<String, Value>> {
        self.typing_marks(pos).map(visible_marks)
    }

    /// Keeps raw mark values so they survive typing and pending mark changes.
    pub(crate) fn typing_marks(&self, pos: Pos) -> Result<TypingMarks> {
        let text = self.inline(pos.block)?;
        let offset = unicode_offset(&text, pos.offset)?;
        self.typing_marks_in(&text, offset)
    }

    pub(crate) fn typing_marks_in(&self, text: &LoroText, offset: usize) -> Result<TypingMarks> {
        let before = match offset.checked_sub(1) {
            Some(index) => self.typing_marks_of(text, index)?,
            None => BTreeMap::new(),
        };
        let after = if offset == text.len_unicode() {
            BTreeMap::new()
        } else {
            self.typing_marks_of(text, offset)?
        };

        let expands = |name: &str, wanted: Expand| {
            let expand = self.schema().mark_spec(name).map(|spec| spec.expand());
            matches!(expand, Some(e) if e == wanted || e == Expand::Both)
        };
        let mut marks = BTreeMap::new();
        for (name, value) in before {
            if after.get(&name) == Some(&value) || expands(&name, Expand::After) {
                marks.insert(name, value);
            }
        }
        for (name, value) in after {
            if expands(&name, Expand::Before) {
                marks.entry(name).or_insert(value);
            }
        }
        Ok(marks)
    }

    fn typing_marks_of(&self, text: &LoroText, offset: usize) -> Result<TypingMarks> {
        let delta = text.slice_delta(offset, offset + 1, PosType::Unicode)?;
        let marks = match delta.into_iter().next() {
            Some(TextDelta::Insert { attributes, .. }) => attributes,
            _ => None,
        };
        // Old documents may already contain a boolean on a valued mark.
        // Typed text is never an atom, even between two of the same.
        Ok(marks
            .into_iter()
            .flatten()
            .filter(|(name, _)| name != ATOM)
            .filter(|(name, value)| {
                !matches!(value, LoroValue::Bool(_))
                    || self
                        .schema()
                        .mark_spec(name)
                        .is_none_or(|spec| spec.allows_boolean())
            })
            .collect())
    }

    /// The marks that all of the text between two positions has, with the same value.
    pub fn marks_in(&self, a: Pos, b: Pos) -> Result<BTreeMap<String, Value>> {
        let mut common: Option<BTreeMap<String, Value>> = None;
        for (block, range) in self.segments(a, b)? {
            for span in self.spans_in(block, range)? {
                common = Some(match common {
                    Some(mut marks) => {
                        marks.retain(|name, value| span.marks.get(name) == Some(value));
                        marks
                    }
                    None => span.marks,
                });
            }
        }
        Ok(common.unwrap_or_default())
    }
}
