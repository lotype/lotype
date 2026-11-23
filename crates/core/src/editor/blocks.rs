use std::collections::HashSet;

use super::{Editor, text_selection};
use crate::{Atom, BlockId, Content, Document, Error, OBJECT, Pos, Result, Selection, Span, Value};

impl Editor {
    /// Turns the blocks of the selection into `block_type`, with these
    /// attributes and no others. A block stays as it is when it cannot
    /// become that: when one of the two holds text and the other does not,
    /// when it has children and the type takes none, or when its own type
    /// is unknown here.
    pub fn set_block(&mut self, block_type: &str, attrs: &[(String, Value)]) -> Result<()> {
        let blocks = self.selected_blocks()?;
        self.apply(|doc, selection| {
            let (holds_text, nests) = {
                let spec = doc.schema().block_spec(block_type);
                let spec = spec.ok_or_else(|| Error::UnknownBlockType(block_type.to_owned()))?;
                (spec.content() == Content::Inline, spec.allows_children())
            };
            for block in blocks {
                let fits = doc.spec(block).is_ok()
                    && !doc.is_table_part(block)
                    && doc.text_len(block).is_ok() == holds_text
                    && (nests || doc.children(Some(block))?.is_empty());
                if !fits {
                    continue;
                }
                // Only what changes, so that asking for what a block already
                // is makes no undo step.
                for key in doc.attrs(block)?.into_keys() {
                    if !attrs.iter().any(|(kept, _)| *kept == key) {
                        doc.remove_attr(block, &key)?;
                    }
                }
                doc.set_block_type(block, block_type)?;
                for (key, value) in attrs {
                    doc.set_attr(block, key, value.clone())?;
                }
            }
            Ok(selection.cloned())
        })
    }

    /// Nests the selected blocks under the block before them, as Tab does.
    /// Blocks that cannot go there stay.
    pub fn indent(&mut self) -> Result<()> {
        self.shift(false, Document::indent_block)
    }

    /// Moves the selected blocks out of their parent, as Shift-Tab does.
    /// Blocks that cannot leave stay.
    pub fn outdent(&mut self) -> Result<()> {
        self.shift(true, Document::outdent_block)
    }

    fn shift(
        &mut self,
        backward: bool,
        shift: fn(&mut Document, BlockId) -> Result<bool>,
    ) -> Result<()> {
        let blocks = self.selected_blocks()?;
        self.apply(|doc, selection| {
            // Each block once, and not one inside another selected block,
            // which moves with that one.
            let chosen: HashSet<_> = blocks.into_iter().collect();
            let mut outermost = Vec::new();
            for block in &chosen {
                let mut above = doc.parent(*block)?;
                while let Some(parent) = above.filter(|parent| !chosen.contains(parent)) {
                    above = doc.parent(parent)?;
                }
                if above.is_none() {
                    outermost.push((doc.path(*block)?, *block));
                }
            }
            outermost.sort_by(|a, b| a.0.cmp(&b.0));
            // Moving out starts at the end: a block that takes no children
            // cannot leave while a sibling still follows it.
            if backward {
                outermost.reverse();
            }
            for (_, block) in outermost {
                shift(doc, block)?;
            }
            Ok(selection.cloned())
        })
    }

    /// Inserts an atom in place of the selection, as a step of its own,
    /// with the marks typed text would get there.
    pub fn insert_atom(&mut self, atom: Atom) -> Result<()> {
        let (anchor, head) = text_selection(self.selection.as_ref())?;
        let marks = self.active_marks()?;
        let span = Span::atom(atom, marks);
        self.apply(|doc, _| {
            let caret = doc.delete_range(anchor, head)?;
            doc.insert_spans(caret.block, caret.offset, std::slice::from_ref(&span))?;
            let end = Pos::new(caret.block, caret.offset + OBJECT.len_utf8());
            Ok(Some(Selection::caret(end)))
        })
    }

    /// Inserts pasted plain text, as a step of its own and as it is: no
    /// input rule reads it. A web or mail address becomes a link: on the
    /// selected text when there is some, and else as text of its own.
    pub fn paste_text(&mut self, text: &str) -> Result<()> {
        let (anchor, head) = text_selection(self.selection.as_ref())?;
        let address = text.trim();
        let links = is_address(address) && self.doc.mark_spec("link").is_ok();
        let selected = !self.doc.segments(anchor, head)?.is_empty();
        let mut marks = self.active_marks()?;
        if links {
            marks.insert("link".to_owned(), Value::from(address));
        }
        self.apply(|doc, selection| {
            if links && selected {
                doc.mark_range(anchor, head, "link", address)?;
                return Ok(selection.cloned());
            }
            let text = if links { address } else { text }.to_owned();
            let caret = doc.delete_range(anchor, head)?;
            let end = Pos::new(caret.block, caret.offset + text.len());
            doc.insert_spans(caret.block, caret.offset, &[Span::new(text, marks)])?;
            Ok(Some(Selection::caret(end)))
        })
    }

    /// Inserts pasted Markdown, as [`Self::paste_document`] does.
    pub fn paste_markdown(&mut self, markdown: &str) -> Result<()> {
        let mut pasted = Document::new(self.doc.schema().clone());
        pasted.insert_markdown(None, 0, markdown)?;
        self.paste_document(&pasted)
    }

    /// Inserts the blocks of a pasted document, as a step of its own, as
    /// ProseMirror pastes a slice: a single paragraph goes into the text at
    /// the caret, and blocks go between the two halves of the block at the
    /// caret, with the first joining the text before the caret when it is a
    /// paragraph and the text after the caret joining the last. Code and
    /// table cells take the pasted text as plain text. What the schema lacks
    /// is left out, as [`Document::insert_document`] leaves it.
    pub fn paste_document(&mut self, pasted: &Document) -> Result<()> {
        let (anchor, head) = text_selection(self.selection.as_ref())?;
        let top = pasted.children(None)?;
        let (start, _) = self.doc.ordered(anchor, head)?;
        let plain =
            start.block.column().is_some() || self.doc.block_type(start.block)? == "code_block";
        let paragraph =
            |block| -> Result<bool> {
                Ok(pasted.block_type(block)? == "paragraph"
                    && pasted.children(Some(block))?.is_empty())
            };
        let mut spans = match top[..] {
            _ if plain => {
                // Blocks without text, such as a divider, are left out.
                let texts = pasted.blocks().into_iter();
                let texts: Vec<_> = texts
                    .filter_map(|block| pasted.plain_text(block).ok())
                    .collect();
                let marks = self.active_marks()?;
                vec![Span::new(texts.join("\n"), marks)]
            }
            [only] if paragraph(only)? => pasted.spans(only)?,
            _ => Vec::new(),
        };
        self.doc.fit_marks(&mut spans);
        if plain || paragraph_only(&top, &spans) {
            return self.apply(|doc, _| {
                let caret = doc.delete_range(anchor, head)?;
                doc.insert_spans(caret.block, caret.offset, &spans)?;
                let len: usize = spans.iter().map(|span| span.text.len()).sum();
                Ok(Some(Selection::caret(Pos::new(
                    caret.block,
                    caret.offset + len,
                ))))
            });
        }
        let first_joins = paragraph(top[0])?;
        self.apply(|doc, _| {
            let caret = doc.delete_range(anchor, head)?;
            let tail = doc.split_block(caret)?.block;
            let parent = doc.parent(caret.block)?;
            let inserted = doc.insert_document(parent, doc.index_of(caret.block)? + 1, pasted)?;
            // Nothing the schema takes: the block is whole again.
            if inserted.is_empty() {
                let caret = doc.merge_backward(tail)?.unwrap_or(caret);
                return Ok(Some(Selection::caret(caret)));
            }
            if first_joins {
                doc.merge_backward(inserted[0])?;
            } else if doc.text_len(caret.block)? == 0 {
                doc.remove_block(caret.block)?;
            }
            // The text after the caret joins the last pasted block, unless
            // that holds no text, as a divider does.
            let last = doc.prev_block(tail)?;
            let joins = last.is_some_and(|last| doc.text_len(last).is_ok());
            let caret = match joins {
                true => doc.merge_backward(tail)?.unwrap_or(Pos::new(tail, 0)),
                false => Pos::new(tail, 0),
            };
            Ok(Some(Selection::caret(caret)))
        })
    }

    /// The blocks the selection touches, in document order for a text selection.
    fn selected_blocks(&self) -> Result<Vec<BlockId>> {
        match &self.selection {
            Some(Selection::Text { anchor, head }) => {
                let (start, end) = self.doc.ordered(*anchor, *head)?;
                self.doc.span(start.block, end.block)
            }
            Some(Selection::Blocks(blocks)) => Ok(blocks.clone()),
            // Cells are no blocks to retype or move.
            Some(Selection::Cells { .. }) | None => Err(Error::NoSelection),
        }
    }
}

fn is_address(text: &str) -> bool {
    let schemes = ["https://", "http://", "mailto:"];
    let named = |scheme: &&str| text.len() > scheme.len() && text.starts_with(*scheme);
    schemes.iter().any(named) && !text.contains(char::is_whitespace)
}

/// Whether a paste is one paragraph's text, or nothing.
fn paragraph_only(top: &[BlockId], spans: &[Span]) -> bool {
    top.is_empty() || top.len() == 1 && !spans.is_empty()
}
