//! What a view does to one block from a control on it: a check box, a
//! swatch, an alert's title, a side menu's add button or handle. Each is one
//! undo step, so that no view writes a block's attributes itself.

use super::Editor;
use crate::attrs::{Align, BlockColor, attr, block as types};
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
    /// a type that takes no style gets none, nor does a table or a part of
    /// one, as [`Editor::set_block_attr`] leaves them.
    pub fn set_block_color(
        &mut self,
        block: BlockId,
        which: BlockColor,
        name: Option<&str>,
    ) -> Result<()> {
        self.apply(|doc, selection| {
            if !doc.spec(block)?.takes_style() || doc.is_table_part(block) {
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
    use crate::Value;

    #[test]
    fn values_are_stored_as_peers_read_them() {
        assert_eq!(super::Align::Center.value(), Some(Value::from("center")));
        assert_eq!(super::Align::Left.value(), None);
    }
}
