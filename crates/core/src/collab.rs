use std::collections::HashSet;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use loro::cursor::{Cursor, PosType, Side};
use loro::event::Diff;
use loro::undo::CursorWithPos;
use loro::{ContainerID, ContainerTrait, ExportMode, TreeID, UndoItemMeta, VersionVector};

use crate::document::ID;
use crate::{BlockId, Document, Error, Pos, Result, Selection};

/// How much of a document's history a peer has. The default is none of it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Version(VersionVector);

/// A selection that follows its text through changes from other peers.
pub(crate) enum Anchored {
    Text { anchor: Anchor, head: Anchor },
    Blocks(Vec<BlockAnchor>),
}

pub(crate) struct BlockAnchor {
    id: BlockId,
    node: TreeID,
}

pub(crate) struct Anchor {
    block: BlockAnchor,
    offset: usize,
    cursor: Option<Cursor>,
    /// The cursor is on the character before the position.
    after: bool,
    /// Keep the exact offset until a remote edit changes this text.
    moved: bool,
    /// The step this is stored with removed the cursor's character, so
    /// undoing it puts a copy there.
    cut: bool,
}

impl Anchored {
    /// The endpoints for Loro's undo history, which moves them through the
    /// changes of other peers until the step is undone.
    pub(crate) fn history_meta(&self) -> UndoItemMeta {
        let mut meta = UndoItemMeta::new();
        if let Self::Text { anchor, head } = self
            && let (Some(anchor), Some(head)) = (&anchor.cursor, &head.cursor)
        {
            meta.add_cursor(anchor);
            meta.add_cursor(head);
        }
        meta
    }

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
        // Only the tree and `$id` decide which block has which id. Text,
        // the bulk of what peers send, leaves the ids as they are.
        let stale = Arc::new(AtomicBool::new(false));
        let seen = Arc::clone(&stale);
        let subscription = self.doc.subscribe_root(Arc::new(move |event| {
            let ids_changed = event.events.iter().any(|change| match &change.diff {
                Diff::Tree(_) => true,
                Diff::Map(map) => map.updated.contains_key(ID),
                _ => false,
            });
            if ids_changed {
                seen.store(true, Ordering::Relaxed);
            }
        }));
        let imported = self.doc.import(update);
        drop(subscription);
        if stale.load(Ordering::Relaxed) {
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
            Selection::Blocks(blocks) => Anchored::Blocks(
                blocks
                    .iter()
                    .map(|&block| self.anchor_block(block))
                    .collect(),
            ),
        }
    }

    /// Where the selection is now. A caret whose block is gone has no place,
    /// and neither has a block selection that lost all of its blocks.
    pub(crate) fn resolve(&self, anchored: &Anchored) -> Option<Selection> {
        self.resolve_history(anchored, &[])
    }

    pub(crate) fn resolve_history(
        &self,
        anchored: &Anchored,
        cursors: &[CursorWithPos],
    ) -> Option<Selection> {
        match anchored {
            Anchored::Text { anchor, head } => Some(Selection::Text {
                anchor: self.resolve_anchor(anchor, cursors.first())?,
                head: self.resolve_anchor(head, cursors.get(1))?,
            }),
            Anchored::Blocks(blocks) => {
                let mut seen = HashSet::new();
                let blocks: Vec<_> = blocks
                    .iter()
                    .filter_map(|block| self.resolve_block(block))
                    .filter(|block| seen.insert(*block))
                    .collect();
                (!blocks.is_empty()).then_some(Selection::Blocks(blocks))
            }
        }
    }

    fn anchor_block(&self, id: BlockId) -> BlockAnchor {
        BlockAnchor {
            id,
            node: self.node(id),
        }
    }

    fn resolve_block(&self, block: &BlockAnchor) -> Option<BlockId> {
        if self.node_depth(block.node).is_ok() {
            Some(self.id_of(block.node))
        } else {
            // A local undo can recreate the node under its original public ID.
            self.contains(block.id).then_some(block.id)
        }
    }

    /// The start of a range holds on to the character before it and its end
    /// to the one after, so the range still covers its text when that is
    /// deleted and comes back, as undo does.
    fn anchor(&self, pos: Pos, range_start: bool) -> Anchor {
        let after = range_start && pos.offset > 0;
        let cursor = self.inline(pos.block).ok().and_then(|text| {
            if range_start && pos.offset == 0 {
                return Some(Cursor::new(None, text.id(), Side::Left, 0));
            }
            let offset = text.convert_pos(pos.offset, PosType::Bytes, PosType::Unicode)?;
            text.get_cursor(offset - usize::from(after), Side::Middle)
        });
        Anchor {
            block: self.anchor_block(pos.block),
            offset: pos.offset,
            cursor,
            after,
            moved: false,
            cut: false,
        }
    }

    /// Whether a block of the selection came back with new text, which the
    /// cursors made in the old text do not follow.
    pub(crate) fn outdated(&self, anchored: &Anchored) -> bool {
        let Anchored::Text { anchor, head } = anchored else {
            return false;
        };
        [anchor, head].into_iter().any(|endpoint| {
            let block = self.resolve_block(&endpoint.block);
            let text = block.and_then(|block| self.inline(block).ok());
            let cursor = endpoint.cursor.as_ref();
            text.zip(cursor)
                .is_some_and(|(text, cursor)| cursor.container != text.id())
        })
    }

    /// Notes the endpoints whose character is gone. Call it right after the
    /// step the selection is stored with, before any other change.
    pub(crate) fn record_cut(&self, anchored: &mut Anchored) {
        if let Anchored::Text { anchor, head } = anchored {
            for endpoint in [anchor, head] {
                endpoint.cut |= endpoint.cursor.as_ref().is_some_and(|cursor| {
                    let position = self.doc.get_cursor_pos(cursor);
                    position.is_ok_and(|position| position.update.is_some())
                });
            }
        }
    }

    fn resolve_anchor(&self, anchor: &Anchor, restored: Option<&CursorWithPos>) -> Option<Pos> {
        let block = self.resolve_block(&anchor.block)?;
        let text = self.inline(block).ok()?;
        // Undo brings a removed block back with new text, which the cursor
        // does not know. The offset it was made at is the best place then.
        let cursor = anchor
            .cursor
            .as_ref()
            .filter(|cursor| anchor.moved && cursor.container == text.id());
        let relative = cursor.and_then(|cursor| {
            let position = self.doc.get_cursor_pos(cursor).ok()?;
            let present = position.update.is_none();
            Some((
                position.current.pos + usize::from(anchor.after && present),
                present,
            ))
        });
        let unicode = match relative {
            Some((pos, true)) => Some(pos),
            _ => restored
                // The character is gone. When the step removed it, undoing
                // the step put a copy back, which the cursor does not know.
                // When a peer removed the one before a range start, the
                // cursor resolves behind the text undo puts back at that
                // start. Either way Loro has moved the place the character
                // was at through what the other peers changed.
                .filter(|cursor| {
                    (anchor.cut || anchor.after)
                        && anchor.moved
                        && cursor.cursor.container == text.id()
                })
                .map(|cursor| cursor.pos.pos + usize::from(anchor.after && anchor.cut))
                .or_else(|| relative.map(|(pos, _)| pos)),
        };
        let offset = unicode
            .and_then(|pos| text.convert_pos(pos, PosType::Unicode, PosType::Bytes))
            .unwrap_or(anchor.offset);
        let pos = Pos::new(block, offset);
        self.check_pos(pos).ok().map(|()| pos)
    }
}
