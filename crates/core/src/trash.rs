use loro::{CommitOptions, LoroText, TreeID, TreeParentId};

use crate::document::{TEXT, TYPE};
use crate::table::{COLUMNS, ColumnId, TABLE};
use crate::{Document, Error, Result};

/// Marks a trash node in its metadata.
pub(crate) const TRASH: &str = "$trash";

/// The origin of changes that are not part of any undo step.
pub(crate) const SYSTEM_ORIGIN: &str = "lotype:system";

/// Loro never revives a node it deleted: undoing the deletion makes a copy,
/// which loses what other peers did to the original. So no block is ever
/// deleted. A removed block moves under a trash node, which no method shows,
/// and undo moves that same block back. A new block starts out there too, so
/// that undoing its insertion puts it away instead of deleting it.
impl Document {
    pub(crate) fn is_trash(&self, node: TreeID) -> bool {
        self.trash.contains(&node)
    }

    /// Keeps the list of trash nodes in step with a marker that changed.
    pub(crate) fn note_trash(&mut self, node: TreeID, marked: bool) {
        if marked {
            self.trash.insert(node);
        } else {
            self.trash.remove(&node);
        }
    }

    /// A trash node at the top level, where no block can contain it.
    pub(crate) fn top_trash(&self) -> Option<TreeID> {
        let at_top = |trash: &&TreeID| self.tree.parent(**trash) == Some(TreeParentId::Root);
        self.trash.iter().filter(at_top).min().copied()
    }

    /// The node removed and unused blocks go under. Peers that each made one
    /// before they met keep them all.
    pub(crate) fn trash_node(&mut self) -> Result<TreeID> {
        if let Some(trash) = self.top_trash() {
            return Ok(trash);
        }
        let trash = self.tree.create(TreeParentId::Root)?;
        self.places = self.places.take().map(|_| Default::default());
        self.tree.get_meta(trash)?.insert(TRASH, true)?;
        self.trash.insert(trash);
        Ok(trash)
    }

    /// Runs changes that no undo step holds. An editor's history skips them,
    /// so they must come before the step that needs them, not inside it.
    pub(crate) fn system<T>(&mut self, edit: impl FnOnce(&mut Self) -> Result<T>) -> Result<T> {
        self.doc.commit();
        let result = edit(self);
        self.doc
            .commit_with(CommitOptions::new().origin(SYSTEM_ORIGIN));
        result
    }

    /// Makes a block in the trash. Moving it out is what inserts it.
    pub(crate) fn mint(&mut self, block_type: &str) -> Result<TreeID> {
        let trash = self.trash_node()?;
        // A draft makes blocks under the peer of the document it is a draft
        // of, and nothing else, so that document can make the same blocks
        // with the same ids before it replays the draft.
        if let Some(draft) = &mut self.draft {
            self.doc.commit();
            if draft.tip.as_ref() != Some(&self.doc.state_frontiers()) {
                draft.tip = None;
            }
            self.doc.set_peer_id(draft.local)?;
        }
        let made = self.mint_under(trash, block_type);
        if let Some(draft) = &mut self.draft {
            self.doc.commit();
            if draft.tip.is_some() {
                draft.tip = Some(self.doc.state_frontiers());
            }
            self.doc.set_peer_id(draft.scratch)?;
            draft.minted(made.as_ref().ok().copied(), block_type);
        }
        made
    }

    fn mint_under(&self, trash: TreeID, block_type: &str) -> Result<TreeID> {
        let id = self.tree.create(TreeParentId::Node(trash))?;
        let meta = self.tree.get_meta(id)?;
        meta.insert(TYPE, block_type)?;
        // Created up front for every block: creating it later from two peers
        // at once would leave two containers and lose one side's text.
        meta.insert_container(TEXT, LoroText::new())?;
        if block_type == TABLE {
            // With the table, and with one identity for every peer: its
            // columns are listed before anything else can happen to it.
            meta.ensure_mergeable_movable_list(COLUMNS)?;
        }
        Ok(id)
    }

    /// Makes the blocks a draft made, under the same ids, and gives the
    /// cells it wrote in their text.
    pub(crate) fn mint_all(
        &mut self,
        minted: &[(Option<TreeID>, String)],
        cells: &[(TreeID, ColumnId)],
    ) -> Result<()> {
        self.system(|doc| {
            for (id, block_type) in minted {
                if Some(doc.mint(block_type)?) != *id {
                    return Err(Error::Internal("a draft block got another id".into()));
                }
            }
            let mut cells = cells.iter();
            cells.try_for_each(|(row, column)| doc.make_cell(*row, *column).map(drop))
        })
    }

    /// Moves a node and everything in it to the trash.
    pub(crate) fn discard(&mut self, node: TreeID) -> Result<()> {
        let trash = self.trash_node()?;
        // Where it leaves, the children after it are one place up.
        let from = self.tree.parent(node);
        if let Some((places, from)) = self.places.as_mut().zip(from) {
            places.remove(&from);
        }
        self.tree.mov(node, TreeParentId::Node(trash))?;
        self.record(|| Box::new(move |doc| doc.discard(node)));
        Ok(())
    }

    /// How many trash nodes are among the children of `parent`.
    pub(crate) fn trash_under(&self, parent: TreeParentId) -> usize {
        let here = |trash: &&TreeID| self.tree.parent(**trash) == Some(parent);
        self.trash.iter().filter(here).count()
    }

    /// Loro's index for the place before the child at `index`. It counts the
    /// trash nodes among the children, and `moving` leaves its place first.
    pub(crate) fn raw_index(
        &self,
        parent: TreeParentId,
        index: usize,
        moving: Option<TreeID>,
    ) -> usize {
        let hidden = self.trash_under(parent);
        // Before everything is before the trash nodes too.
        if hidden == 0 || index == 0 {
            return index;
        }
        // After everything as well. Loading a document appends block after
        // block, which must not read all the siblings each time.
        let here = moving.is_some_and(|node| self.tree.parent(node) == Some(parent));
        let len = self.tree.children_num(parent).unwrap_or(0) - usize::from(here);
        if index + hidden == len {
            return len;
        }
        // Before the block last placed here, or right after it, with
        // nothing between them but what is no block.
        let placed = self.places.as_ref().and_then(|places| places.get(&parent));
        if let Some((at, raw)) = placed.filter(|(at, _)| !here && (index == *at || index == at + 1))
        {
            return raw + (index - at);
        }
        let mut raw = self.tree.children(parent).unwrap_or_default();
        raw.retain(|id| Some(*id) != moving);
        let mut shown = raw
            .iter()
            .enumerate()
            .filter(|(_, id)| !self.is_trash(**id));
        shown.nth(index).map_or(raw.len(), |(at, _)| at)
    }
}
