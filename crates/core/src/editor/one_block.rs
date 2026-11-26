//! What a view does to one block from a control on it: a check box, a
//! swatch, an alert's title, a side menu's add button or handle. Each is one
//! undo step, so that no view writes a block's attributes itself.

use super::Editor;
use crate::attrs::{Align, BlockColor, LARGEST_LIST_NUMBER, attr, block as types};
use crate::{AlertKind, BlockId, Document, Error, Pos, Result, Selection};

/// Fails unless `block` is of the type `name`.
fn check_type(doc: &Document, block: BlockId, name: &'static str) -> Result<()> {
    if doc.block_type(block)? == name {
        Ok(())
    } else {
        Err(Error::NotOfType(name))
    }
}

impl Editor {
    /// Checks or unchecks a check list item; fails for another block.
    pub fn set_checked(&mut self, block: BlockId, checked: bool) -> Result<()> {
        check_type(&self.doc, block, types::CHECK_LIST_ITEM)?;
        self.apply(|doc, selection| {
            doc.set_attr(block, attr::CHECKED, checked)?;
            Ok(selection.cloned())
        })
    }

    /// Gives a block one of its colors by name, or takes it off for `None`;
    /// a type that takes no style gets none, nor does a row or a cell of a
    /// table, which no view draws a color of: the table takes it for them.
    pub fn set_block_color(
        &mut self,
        block: BlockId,
        which: BlockColor,
        name: Option<&str>,
    ) -> Result<()> {
        self.apply(|doc, selection| {
            let inside_table = doc.is_table_part(block) && doc.block_type(block)? != types::TABLE;
            if !doc.spec(block)?.takes_style() || inside_table {
                return Ok(selection.cloned());
            }
            match name {
                Some(name) => doc.set_attr(block, which.key(), name)?,
                None => doc.remove_attr(block, which.key())?,
            }
            Ok(selection.cloned())
        })
    }

    /// Sets where the lines of the selected blocks go.
    pub fn set_alignment(&mut self, align: Align) -> Result<()> {
        self.set_block_attr(attr::TEXT_ALIGNMENT, align.value())
    }

    /// Sets an alert's kind; fails for a block that is no alert.
    pub fn set_alert_kind(&mut self, block: BlockId, kind: AlertKind) -> Result<()> {
        check_type(&self.doc, block, types::ALERT)?;
        self.apply(|doc, selection| {
            doc.set_attr(block, attr::KIND, kind.name())?;
            Ok(selection.cloned())
        })
    }

    /// Sets an image's reference and caption in one undo step. An empty value clears it.
    pub fn set_image(&mut self, block: BlockId, url: &str, caption: &str) -> Result<()> {
        self.set_media(
            block,
            types::IMAGE,
            [(attr::URL, url), (attr::CAPTION, caption)],
        )
    }

    /// Sets a file's reference and name in one undo step. An empty value clears it.
    pub fn set_file(&mut self, block: BlockId, url: &str, name: &str) -> Result<()> {
        self.set_media(block, types::FILE, [(attr::URL, url), (attr::NAME, name)])
    }

    /// Sets or clears a code block's language in one undo step.
    pub fn set_code_language(&mut self, block: BlockId, language: Option<&str>) -> Result<()> {
        self.set_code_text_attr(block, attr::LANGUAGE, language.unwrap_or_default())
    }

    /// Sets or clears a code block's title in one undo step.
    pub fn set_code_title(&mut self, block: BlockId, title: Option<&str>) -> Result<()> {
        self.set_code_text_attr(block, attr::TITLE, title.unwrap_or_default())
    }

    /// Sets or clears a code block's highlighted line ranges in one undo step.
    pub fn set_code_highlights(&mut self, block: BlockId, lines: Option<&str>) -> Result<()> {
        let normalized = lines
            .and_then(crate::markdown::normalize_line_ranges)
            .unwrap_or_default();
        self.set_code_text_attr(block, attr::HIGHLIGHT_LINES, &normalized)
    }

    /// Enables or disables Twoslash on a code block in one undo step.
    pub fn set_twoslash(&mut self, block: BlockId, twoslash: bool) -> Result<()> {
        check_type(&self.doc, block, types::CODE_BLOCK)?;
        let value = crate::Value::Bool(twoslash);
        if self.doc.attr(block, attr::TWOSLASH)?.as_ref() == Some(&value) {
            return Ok(());
        }
        self.apply(|doc, selection| {
            doc.set_attr(block, attr::TWOSLASH, twoslash)?;
            Ok(selection.cloned())
        })
    }

    fn set_code_text_attr(&mut self, block: BlockId, key: &'static str, text: &str) -> Result<()> {
        check_type(&self.doc, block, types::CODE_BLOCK)?;
        let value = if text.is_empty() {
            if !self.doc.has_attr(block, key)? {
                return Ok(());
            }
            None
        } else {
            let value = crate::Value::from(text);
            if self.doc.attr(block, key)?.as_ref() == Some(&value) {
                return Ok(());
            }
            Some(value)
        };
        self.apply(|doc, selection| {
            match value {
                Some(value) => doc.set_attr(block, key, value)?,
                None => doc.remove_attr(block, key)?,
            }
            Ok(selection.cloned())
        })
    }

    /// Changes the attachment metadata while preserving the block and the selection.
    fn set_media(
        &mut self,
        block: BlockId,
        kind: &'static str,
        attrs: [(&str, &str); 2],
    ) -> Result<()> {
        check_type(&self.doc, block, kind)?;
        let mut changes = Vec::with_capacity(attrs.len());
        for (key, text) in attrs {
            let value = if text.is_empty() {
                if !self.doc.has_attr(block, key)? {
                    continue;
                }
                None
            } else {
                let value = crate::Value::from(text);
                if self.doc.attr(block, key)?.as_ref() == Some(&value) {
                    continue;
                }
                Some(value)
            };
            changes.push((key, value));
        }
        if changes.is_empty() {
            return Ok(());
        }
        self.apply(|doc, selection| {
            for (key, value) in changes {
                match value {
                    Some(value) => doc.set_attr(block, key, value)?,
                    None => doc.remove_attr(block, key)?,
                }
            }
            Ok(selection.cloned())
        })
    }

    /// Makes the numbered list `block` is in start at `start`, within 0 to
    /// CommonMark's largest, or at 1 for `None`; fails for a block that is
    /// no numbered list item. The list's first item holds its start.
    pub fn set_list_start(&mut self, block: BlockId, start: Option<i64>) -> Result<()> {
        check_type(&self.doc, block, types::NUMBERED_LIST_ITEM)?;
        self.apply(|doc, selection| {
            let siblings = doc.children(doc.parent(block)?)?;
            let at = doc.index_of(block)?;
            let numbered = |sibling: &BlockId| {
                doc.block_type(*sibling)
                    .is_ok_and(|name| name == types::NUMBERED_LIST_ITEM)
            };
            let before = siblings[..at].iter().rev().take_while(|b| numbered(b));
            let first = before.last().copied().unwrap_or(block);
            match start {
                Some(start) => {
                    doc.set_attr(first, attr::START, start.clamp(0, LARGEST_LIST_NUMBER))?
                }
                None => doc.remove_attr(first, attr::START)?,
            }
            Ok(selection.cloned())
        })
    }

    /// Puts the caret in an empty text block after `block`: `block` itself
    /// where it is one with nothing in it, or a new one, as a side menu's
    /// add button does.
    pub fn add_text_block_after(&mut self, block: BlockId) -> Result<()> {
        let doc = &self.doc;
        let text = doc.schema().text_block();
        let empty = doc
            .block_type(block)
            .is_ok_and(|kind| text == Some(kind.as_str()))
            && doc.text_len(block)? == 0
            && doc.children(Some(block))?.is_empty();
        if empty {
            return self.set_selection(Some(Selection::caret(Pos::new(block, 0))));
        }
        self.apply(|doc, _| {
            let index = doc.index_of(block)? + 1;
            let text = doc.text_block()?;
            let made = doc.insert_block(doc.parent(block)?, index, &text)?;
            Ok(Some(Selection::caret(Pos::new(made, 0))))
        })
    }

    /// Moves `block` right before or after `target`, as a drop of its
    /// handle does. The selection stays.
    pub fn move_block_next_to(
        &mut self,
        block: BlockId,
        target: BlockId,
        after: bool,
    ) -> Result<()> {
        self.apply(|doc, selection| {
            doc.move_block_next_to(block, target, after)?;
            Ok(selection.cloned())
        })
    }

    /// Makes a text block to write in, with the caret in it, when the
    /// document has no blocks and nothing is selected. Returns whether it did.
    pub fn start_writing(&mut self) -> Result<bool> {
        if self.selection.is_some() || !self.doc.children(None)?.is_empty() {
            return Ok(false);
        }
        self.apply(|doc, _| {
            let made = doc.insert_block(None, 0, &doc.text_block()?)?;
            Ok(Some(Selection::caret(Pos::new(made, 0))))
        })?;
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use crate::{Document, Editor, Schema, Value, attr, block};

    #[test]
    fn values_are_stored_as_peers_read_them() {
        assert_eq!(super::Align::Center.value(), Some(Value::from("center")));
        assert_eq!(super::Align::Left.value(), None);
    }

    #[test]
    fn clearing_media_removes_raw_attributes_the_sdk_cannot_decode() {
        let mut doc = Document::new(Schema::builtin());
        let image = doc.insert_block(None, 0, block::IMAGE).unwrap();
        doc.tree
            .get_meta(image.0)
            .unwrap()
            .insert(attr::URL, 0.5_f64)
            .unwrap();
        assert_eq!(doc.attr(image, attr::URL).unwrap(), None);
        let mut editor = Editor::new(doc);
        editor.set_image(image, "", "").unwrap();
        assert!(!editor.document().has_attr(image, attr::URL).unwrap());
        assert!(editor.undo().unwrap());
        assert!(editor.document().has_attr(image, attr::URL).unwrap());
    }
}
