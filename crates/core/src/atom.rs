//! Inline atoms: things in a line of text that are not text, such as a
//! mention or a formula. An atom is one U+FFFC character carrying a mark
//! whose value is a map of its type and attributes, so that it moves,
//! merges and deletes with the text around it as a single character.

use std::collections::BTreeMap;

use loro::{LoroMapValue, LoroValue};

use crate::{BlockId, Document, Result, Span, Value};

/// The character an atom stands on.
pub const OBJECT: char = '\u{FFFC}';

/// The mark that makes U+FFFC an atom. Its value is a map, which public
/// marks never show, as they only hold scalars.
pub(crate) const ATOM: &str = "$atom";

/// Where an atom's type is kept among its attributes in the mark.
const TYPE: &str = "$type";

/// An atom's type and attributes.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Atom {
    pub name: String,
    pub attrs: BTreeMap<String, Value>,
}

impl Atom {
    pub fn new(name: impl Into<String>) -> Self {
        let (name, attrs) = (name.into(), BTreeMap::new());
        Self { name, attrs }
    }

    pub fn attr(mut self, key: &str, value: impl Into<Value>) -> Self {
        self.attrs.insert(key.to_owned(), value.into());
        self
    }

    /// Attributes another peer wrote that are not scalars are left out.
    pub(crate) fn from_loro(value: &LoroValue) -> Option<Self> {
        let LoroValue::Map(map) = value else {
            return None;
        };
        let Some(LoroValue::String(name)) = map.get(TYPE) else {
            return None;
        };
        let attrs = map.iter().filter(|(key, _)| *key != TYPE);
        let attrs = attrs.filter_map(|(key, value)| Some((key.clone(), Value::from_loro(value)?)));
        Some(Self {
            name: name.to_string(),
            attrs: attrs.collect(),
        })
    }

    pub(crate) fn to_loro(&self) -> LoroValue {
        let attrs = self
            .attrs
            .iter()
            .map(|(key, value)| (key.clone(), value.to_loro()));
        let name = (TYPE.to_owned(), LoroValue::from(self.name.as_str()));
        LoroValue::Map(attrs.chain([name]).collect::<LoroMapValue>())
    }
}

impl Document {
    /// A block's spans with each atom as the text its type stands for, and
    /// an atom of a type the schema lacks left out.
    pub(crate) fn plain_spans(&self, block: BlockId) -> Result<Vec<Span>> {
        let mut spans = self.spans(block)?;
        spans.retain_mut(|span| {
            let Some(atom) = span.atom.take() else {
                return true;
            };
            let spec = self.schema().inline_spec(&atom.name);
            span.text = spec.map_or_else(String::new, |spec| spec.text(&atom));
            !span.text.is_empty()
        });
        Ok(spans)
    }

    /// A block's text as plain text, with each atom as the text its type
    /// stands for.
    pub fn plain_text(&self, block: BlockId) -> Result<String> {
        let spans = self.plain_spans(block)?;
        Ok(spans.into_iter().map(|span| span.text).collect())
    }
}

#[cfg(test)]
mod tests {
    use loro::{ExpandType, LoroDoc, StyleConfig};

    use super::*;

    /// What a peer could write: the atom mark over more than U+FFFC, and
    /// over U+FFFC as a plain value.
    #[test]
    fn only_marked_object_characters_are_atoms() {
        let doc = LoroDoc::new();
        doc.config_default_text_style(Some(StyleConfig {
            expand: ExpandType::None,
        }));
        let text = doc.get_text("t");
        text.insert(0, &format!("a{OBJECT}b{OBJECT}")).unwrap();
        text.mark(0..3, ATOM, Atom::new("x").to_loro()).unwrap();
        text.mark(3..4, ATOM, "not a map").unwrap();
        let spans = crate::text::to_spans(text.to_delta());
        let atoms: Vec<_> = spans.iter().map(|span| span.atom.is_some()).collect();
        assert_eq!(atoms, [false, true, false, false]);
        let texts: Vec<_> = spans.iter().map(|span| span.text.as_str()).collect();
        assert_eq!(texts.concat(), format!("a{OBJECT}b{OBJECT}"));
    }
}
