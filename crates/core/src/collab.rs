use std::collections::HashSet;

use loro::cursor::{Cursor, PosType, Side};
use loro::{ContainerID, ContainerTrait, ExportMode, VersionVector};

use crate::{BlockId, Document, Error, Pos, Result, Selection};

/// How much of a document's history a peer has. The default is none of it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Version(VersionVector);

/// A selection that follows its text through changes from other peers.
pub(crate) enum Anchored {
    Text { anchor: Anchor, head: Anchor },
    Blocks(Vec<BlockId>),
}

pub(crate) struct Anchor {
    pos: Pos,
    cursor: Option<Cursor>,
    /// The cursor is on the character before the position.
    after: bool,
    /// Keep the exact offset until a remote edit changes this text.
    moved: bool,
}

impl Anchored {
    pub(crate) fn record_import(&mut self, changed_texts: &HashSet<ContainerID>) {
        if let Self::Text { anchor, head } = self {
            for endpoint in [anchor, head] {
                endpoint.moved |= endpoint
                    .cursor
                    .as_ref()
                    .is_some_and(|cursor| changed_texts.contains(&cursor.container));
            }
        }
    }
}

/// Exchanging changes with other peers, as bytes. How they travel is up to the application.
impl Document {
    pub fn version(&self) -> Version {
        self.commit_change();
        Version(self.doc.oplog_vv())
    }

    /// The whole document with its history, to store it or to start another peer from.
    pub fn export_snapshot(&self) -> Result<Vec<u8>> {
        self.export(ExportMode::Snapshot)
    }

    /// The changes that a peer at `since` lacks.
    pub fn export_updates(&self, since: &Version) -> Result<Vec<u8>> {
        self.export(ExportMode::updates(&since.0))
    }

    fn export(&self, mode: ExportMode) -> Result<Vec<u8>> {
        if self.draft.is_some() {
            return Err(Error::ExportDuringEdit);
        }
        let bytes = self.doc.export(mode);
        bytes.map_err(|error| Error::Internal(Box::new(error)))
    }

    /// Merges a snapshot or updates from another peer. If Loro can apply
    /// only part of an update against shallow history, those changes are
    /// retained and this method returns an error. Updates with dependencies
    /// that arrive out of order take effect when those dependencies arrive.
    ///
    /// Use [`Editor::import`](crate::Editor::import) for a document in an editor.
    pub fn import(&mut self, update: &[u8]) -> Result<()> {
        if self.draft.is_some() {
            return Err(Error::ImportDuringEdit);
        }
        let before = self.doc.state_frontiers();
        let imported = self.doc.import(update);
        if self.doc.state_frontiers() != before {
            self.refresh_ids();
        }
        imported
            .map(drop)
            .map_err(|error| Error::InvalidUpdate(Box::new(error)))
    }

    pub(crate) fn anchored(&self, selection: &Selection) -> Anchored {
        match selection {
            Selection::Text { anchor, head } => {
                let start = match self.ordered(*anchor, *head) {
                    Ok((start, _)) if anchor != head => Some(start),
                    _ => None,
                };
                Anchored::Text {
                    anchor: self.anchor(*anchor, start == Some(*anchor)),
                    head: self.anchor(*head, start == Some(*head)),
                }
            }
            Selection::Blocks(blocks) => Anchored::Blocks(blocks.clone()),
        }
    }

    /// Where the selection is now. A caret whose block is gone has no place,
    /// and neither has a block selection that lost all of its blocks.
    pub(crate) fn resolve(&self, anchored: &Anchored) -> Option<Selection> {
        match anchored {
            Anchored::Text { anchor, head } => Some(Selection::Text {
                anchor: self.resolve_anchor(anchor)?,
                head: self.resolve_anchor(head)?,
            }),
            Anchored::Blocks(blocks) => {
                let blocks: Vec<_> = blocks
                    .iter()
                    .copied()
                    .filter(|b| self.contains(*b))
                    .collect();
                (!blocks.is_empty()).then_some(Selection::Blocks(blocks))
            }
        }
    }

    /// The start of a range holds on to the character before it and its end
    /// to the one after, so the range still covers its text when that is
    /// deleted and comes back, as undo does.
    fn anchor(&self, pos: Pos, range_start: bool) -> Anchor {
        let after = range_start && pos.offset > 0;
        let cursor = self.inline(pos.block).ok().and_then(|text| {
            let offset = text.convert_pos(pos.offset, PosType::Bytes, PosType::Unicode)?;
            text.get_cursor(offset - usize::from(after), Side::Middle)
        });
        Anchor {
            pos,
            cursor,
            after,
            moved: false,
        }
    }

    fn resolve_anchor(&self, anchor: &Anchor) -> Option<Pos> {
        let text = self.inline(anchor.pos.block).ok()?;
        // Undo brings a removed block back with new text, which the cursor
        // does not know. The offset it was made at is the best place then.
        let cursor = anchor
            .cursor
            .as_ref()
            .filter(|cursor| anchor.moved && cursor.container == text.id());
        let moved = cursor.and_then(|cursor| {
            let position = self.doc.get_cursor_pos(cursor).ok()?;
            // A deleted character resolves to its boundary, so only step
            // past a character if it is still there.
            let unicode =
                position.current.pos + usize::from(anchor.after && position.update.is_none());
            text.convert_pos(unicode, PosType::Unicode, PosType::Bytes)
        });
        let pos = Pos::new(anchor.pos.block, moved.unwrap_or(anchor.pos.offset));
        self.check_pos(pos).ok().map(|()| pos)
    }
}
