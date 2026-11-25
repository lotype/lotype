use std::collections::HashSet;
use std::sync::{Arc, Mutex};

use loro::cursor::{Cursor, PosType, Side};
use loro::event::Diff;
use loro::undo::CursorWithPos;
use loro::{
    ContainerID, ContainerTrait, ContainerType, ExportMode, TreeID, UndoItemMeta, VersionVector,
};

use crate::trash::TRASH;
use crate::{BlockId, Document, Error, Pos, Result, Selection};

/// How much of a document's history a peer has. The default is none of it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Version(VersionVector);

impl Version {
    /// The version as bytes, to send to a peer.
    pub fn encode(&self) -> Vec<u8> {
        self.0.encode()
    }

    /// A version from the bytes of [`Version::encode`].
    pub fn decode(bytes: &[u8]) -> Result<Self> {
        let decoded = VersionVector::decode(bytes);
        decoded
            .map(Self)
            .map_err(|error| Error::InvalidUpdate(Box::new(error)))
    }
}

/// A selection that follows its text through changes from other peers.
pub(crate) enum Anchored {
    Text { anchor: Anchor, head: Anchor },
    Blocks(Vec<BlockId>),
    Cells(BlockId, BlockId),
}

pub(crate) struct Anchor {
    block: BlockId,
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
    /// How much of its history the document has, for a peer to ask for what it lacks with
    /// [`Document::export_updates`].
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

    /// The changes imported before those they come after, which wait for
    /// them and are in no snapshot or update: what to keep with a snapshot
    /// so that it holds all the document was given. `None` when none wait.
    pub fn export_waiting(&self) -> Result<Option<Vec<u8>>> {
        let waiting = self.doc.export_pending_updates();
        waiting.map_err(|error| Error::Internal(Box::new(error)))
    }

    /// Whether the document came of a snapshot without its early history.
    /// Its updates are of no use to a peer that has not the same start.
    pub fn is_shallow(&self) -> bool {
        self.doc.is_shallow()
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
    ///
    /// # Panics
    ///
    /// Loro checks the bytes, but an update can be made that it panics on,
    /// and a document it panicked in panics again when used or dropped. A
    /// peer that is not trusted is best met through a relay that imports
    /// first, as `lotype-server` does, and passes on only what it exported.
    pub fn import(&mut self, update: &[u8]) -> Result<()> {
        if self.draft.is_some() {
            return Err(Error::ImportDuringEdit);
        }
        // A peer can make a trash node, or take the marker off one.
        let marked = Arc::new(Mutex::new(Vec::new()));
        let seen = Arc::clone(&marked);
        let subscription = self.doc.subscribe_root(Arc::new(move |event| {
            for change in event.events {
                // A node's metadata has the id of the node.
                if let Diff::Map(map) = &change.diff
                    && let Some(marker) = map.updated.get(TRASH)
                    && let ContainerID::Normal {
                        peer,
                        counter,
                        container_type: ContainerType::Map,
                    } = change.target
                {
                    let node = TreeID::new(*peer, *counter);
                    seen.lock().unwrap().push((node, marker.is_some()));
                }
            }
        }));
        let imported = self.doc.import(update);
        drop(subscription);
        for (node, marker) in std::mem::take(&mut *marked.lock().unwrap()) {
            self.note_trash(node, marker);
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
            Selection::Cells { anchor, head } => Anchored::Cells(*anchor, *head),
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
                let blocks: Vec<_> = blocks
                    .iter()
                    .copied()
                    .filter(|block| self.contains(*block))
                    .collect();
                (!blocks.is_empty()).then_some(Selection::Blocks(blocks))
            }
            // While both corners are there. A row or column that goes or
            // comes between them only changes what lies between.
            Anchored::Cells(anchor, head) => {
                let (anchor, head) = (*anchor, *head);
                let cells = self.cells_between(anchor, head).ok();
                cells.map(|_| Selection::Cells { anchor, head })
            }
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
            block: pos.block,
            offset: pos.offset,
            cursor,
            after,
            moved: false,
            cut: false,
        }
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
        let block = anchor.block;
        let text = self.inline(block).ok()?;
        // The offset the anchor was made at is exact until a peer changes
        // the text. A cursor is also no use once a peer has put another text
        // in the block.
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
