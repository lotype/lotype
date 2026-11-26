use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt;
use std::str::FromStr;
use std::sync::Mutex;

use loro::{ExpandType, LoroDoc, LoroMap, LoroText, LoroTree, StyleConfig, TreeID, TreeParentId};

use crate::draft::Draft;
use crate::order::OrderCache;
use crate::table::{CELL, ColumnId, ROW, TABLE};
use crate::{BlockSpec, Content, Error, Result, Schema, Value};

/// The deepest nesting level a document has. Top-level blocks are at level 1.
///
/// Blocks that another peer nests deeper stay in the shared data but are not
/// part of the document: no method returns them, and their ids are not found.
/// The same holds for removed blocks, which the shared data keeps as well.
pub const MAX_DEPTH: usize = 128;

const TREE: &str = "blocks";
pub(crate) const TYPE: &str = "$type";
pub(crate) const TEXT: &str = "$text";

/// The identity of a block. It stays the same when undo or redo brings a
/// removed block back.
///
/// A cell of a table has one as well: its row together with a column.
///
/// To keep a reference outside the document, store the id as text with
/// `to_string` and read it back with `parse`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BlockId(pub(crate) TreeID, pub(crate) Option<ColumnId>);

impl BlockId {
    /// The column, when this is a cell of a table.
    pub fn column(self) -> Option<ColumnId> {
        self.1
    }
}

impl fmt::Display for BlockId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)?;
        self.1.map_or(Ok(()), |column| write!(f, "/{column}"))
    }
}

impl FromStr for BlockId {
    type Err = Error;

    fn from_str(text: &str) -> Result<Self> {
        let invalid = || Error::InvalidBlockId(text.to_owned());
        let (node, column) = match text.split_once('/') {
            Some((node, column)) => (node, Some(column.parse().map_err(|_| invalid())?)),
            None => (text, None),
        };
        Ok(Self(TreeID::try_from(node).map_err(|_| invalid())?, column))
    }
}

/// A tree of blocks.
pub struct Document {
    pub(crate) doc: LoroDoc,
    pub(crate) tree: LoroTree,
    pub(crate) order: Mutex<OrderCache>,
    pub(crate) draft: Option<Draft>,
    /// The nodes marked as trash, wherever a peer has put them.
    pub(crate) trash: HashSet<TreeID>,
    /// While a run of inserts is made ([`Document::batch`]): for a parent,
    /// the child last placed in it, as its index among the blocks and
    /// Loro's among all the children.
    pub(crate) places: Option<HashMap<TreeParentId, (usize, usize)>>,
    schema: Schema,
}

impl Document {
    /// An empty document of `schema`'s blocks, marks and atoms.
    pub fn new(schema: Schema) -> Self {
        Self::from_loro(LoroDoc::new(), schema)
    }

    pub(crate) fn from_loro(doc: LoroDoc, schema: Schema) -> Self {
        doc.config_text_style(schema.style_config());
        // Marks from a peer with a different schema must stay readable and
        // copyable; without a default, Loro rejects every edit that meets one.
        let expand = ExpandType::None;
        doc.config_default_text_style(Some(StyleConfig { expand }));
        let tree = doc.get_tree(TREE);
        Self {
            doc,
            tree,
            order: Mutex::default(),
            draft: None,
            trash: HashSet::new(),
            places: None,
            schema,
        }
    }

    /// The block types, marks and atoms the document takes.
    pub fn schema(&self) -> &Schema {
        &self.schema
    }

    /// Ends the current change. Each commit is one undo step.
    pub fn commit(&mut self) {
        self.commit_change();
    }

    pub(crate) fn commit_change(&self) {
        self.doc.commit();
        self.record(|| {
            Box::new(|doc| {
                doc.commit();
                Ok(())
            })
        });
    }

    /// Child blocks of `parent`, or the top-level blocks when `parent` is `None`.
    pub fn children(&self, parent: Option<BlockId>) -> Result<Vec<BlockId>> {
        // A cell holds text and nothing else.
        if let Some(cell) = parent.filter(|parent| parent.1.is_some()) {
            return self.depth(cell).map(|_| Vec::new());
        }
        let (parent, depth) = self.slot(parent)?;
        if depth == MAX_DEPTH {
            return Ok(Vec::new());
        }
        let children = self.child_ids(parent).into_iter();
        Ok(children.map(|id| BlockId(id, None)).collect())
    }

    /// The parent of `block`, or `None` for a top-level block.
    pub fn parent(&self, block: BlockId) -> Result<Option<BlockId>> {
        if block.1.is_some() {
            // The row.
            return self.depth(block).map(|_| Some(BlockId(block.0, None)));
        }
        match self.tree.parent(self.live(block)?) {
            Some(TreeParentId::Node(id)) => Ok(Some(BlockId(id, None))),
            Some(TreeParentId::Root) => Ok(None),
            _ => Err(Error::BlockNotFound),
        }
    }

    /// Whether `block` is in the document: not removed, and not nested deeper than
    /// [`MAX_DEPTH`], where a merge can leave one.
    pub fn contains(&self, block: BlockId) -> bool {
        self.depth(block).is_ok()
    }

    /// Inserts an empty block of `block_type` at `index` among the children of `parent`, or at
    /// the top level for `None`. The parent must take children, a heading goes in no block, and
    /// the block must not end up deeper than [`MAX_DEPTH`].
    pub fn insert_block(
        &mut self,
        parent: Option<BlockId>,
        index: usize,
        block_type: &str,
    ) -> Result<BlockId> {
        self.check_allows_children(parent)?;
        self.check_place(parent, block_type)?;
        if self.slot(parent)?.1 == MAX_DEPTH {
            return Err(Error::TooDeep);
        }
        self.create_block(parent, index, block_type)
    }

    /// Rows go in tables and nothing else does. A cell is no block to
    /// place, and a block of a top-level type, as a heading, goes in none.
    fn check_place(&self, parent: Option<BlockId>, block_type: &str) -> Result<()> {
        let table = |parent| self.block_type(parent).is_ok_and(|parent| parent == TABLE);
        let in_table = parent.is_some_and(table);
        if block_type == CELL || in_table != (block_type == ROW) {
            return Err(Error::TablePart);
        }
        let spec = self.schema.block_spec(block_type);
        if parent.is_some() && spec.is_some_and(BlockSpec::is_top_level) {
            return Err(Error::TopLevelOnly(block_type.to_owned()));
        }
        Ok(())
    }

    /// [`Self::insert_block`] without the rules on the parent's type and on
    /// depth. Commands use it next to blocks that are already there, so they
    /// still work inside nesting that a merge left invalid.
    pub(crate) fn create_block(
        &mut self,
        parent: Option<BlockId>,
        index: usize,
        block_type: &str,
    ) -> Result<BlockId> {
        self.spec_named(block_type)?;
        self.check_index(parent, index, None)?;
        let id = self.mint(block_type)?;
        self.relocate_node(id, parent, index)?;
        Ok(BlockId(id, None))
    }

    /// Removes `block` and all of its descendants. The shared data keeps
    /// them, out of sight, which is where an editor's undo finds them.
    pub fn remove_block(&mut self, block: BlockId) -> Result<()> {
        self.discard(self.live(block)?)
    }

    /// Removes the blocks that are still in the document, in the given order.
    pub(crate) fn remove_blocks(
        &mut self,
        blocks: impl IntoIterator<Item = BlockId>,
    ) -> Result<()> {
        blocks.into_iter().try_for_each(|block| {
            // One of them can be inside another that has just gone.
            if block.1.is_none() && self.contains(block) {
                self.discard(block.0)?;
            }
            Ok(())
        })
    }

    /// Moves `block` so that it ends up at `index` among the children of `parent`.
    pub fn move_block(
        &mut self,
        block: BlockId,
        parent: Option<BlockId>,
        index: usize,
    ) -> Result<()> {
        self.check_allows_children(parent)?;
        let block_type = self.block_type(block).unwrap_or_default();
        self.check_place(parent, &block_type)?;
        // A row keeps to its table: its cells are those of that table's columns.
        if block_type == ROW && self.parent(block)? != parent {
            return Err(Error::TablePart);
        }
        // Before the depth rule: a block is always "too deep" for its own subtree.
        if parent.is_some_and(|parent| self.is_within(parent, block)) {
            return Err(Error::CyclicMove);
        }
        if self.slot(parent)?.1 + self.height(block)? > MAX_DEPTH {
            return Err(Error::TooDeep);
        }
        self.relocate_block(block, parent, index)
    }

    /// [`Self::move_block`] without the rules on the parent's type and on
    /// depth, like [`Self::create_block`].
    pub(crate) fn relocate_block(
        &mut self,
        block: BlockId,
        parent: Option<BlockId>,
        index: usize,
    ) -> Result<()> {
        let id = self.live(block)?;
        self.relocate_node(id, parent, index)
    }

    /// Moves an existing raw child, including a child hidden by the depth cap.
    pub(crate) fn relocate_node(
        &mut self,
        node: TreeID,
        parent: Option<BlockId>,
        index: usize,
    ) -> Result<()> {
        self.check_index(parent, index, Some(node))?;
        let parent_id = self.slot(parent)?.0;
        let from = self.tree.parent(node);
        let raw = self.raw_index(parent_id, index, Some(node));
        self.tree.mov_to(node, parent_id, raw)?;
        if let Some(places) = &mut self.places {
            // Where it left, the children after it are one place up.
            if let Some(from) = from.filter(|from| *from != parent_id) {
                places.remove(&from);
            }
            places.insert(parent_id, (index, raw));
        }
        self.record(|| Box::new(move |doc| doc.relocate_node(node, parent, index)));
        Ok(())
    }

    /// Runs `work`, which places many blocks. Finding Loro's index for a
    /// block's reads all its siblings where a trash node is among them, as
    /// at the top level; inside, a block placed next to the last one placed
    /// in the same parent is put there without that, so a run of blocks
    /// costs its length and not that times the siblings. Only the
    /// document's own methods may change it meanwhile, which is why this is
    /// not kept between calls: an import or an undo moves blocks unseen.
    pub(crate) fn batch<T>(&mut self, work: impl FnOnce(&mut Self) -> T) -> T {
        let outer = self.places.is_some();
        self.places.get_or_insert_default();
        let done = work(self);
        if !outer {
            self.places = None;
        }
        done
    }

    /// The name of `block`'s type, `table_cell` for a cell.
    pub fn block_type(&self, block: BlockId) -> Result<String> {
        if block.1.is_some() {
            return self.depth(block).map(|_| CELL.to_owned());
        }
        let value = self.meta(block)?.get(TYPE);
        let value = value.and_then(|v| v.into_value().ok());
        match value.as_ref().and_then(Value::from_loro) {
            Some(Value::Str(name)) => Ok(name),
            _ => Err(Error::UnknownBlockType(String::new())),
        }
    }

    /// Changes the block's type. A block that becomes a type that goes only
    /// at the top level, as a heading, comes out of the blocks it is in
    /// where it reads, and so does what follows it in them.
    pub fn set_block_type(&mut self, block: BlockId, block_type: &str) -> Result<()> {
        let spec = self.spec_named(block_type)?.clone();
        if self.is_table_part(block) || [TABLE, ROW, CELL].contains(&block_type) {
            return Err(Error::TablePart);
        }
        if !spec.allows_children() && !self.children(Some(block))?.is_empty() {
            return Err(Error::ChildrenNotAllowed(block_type.to_owned()));
        }
        let has_text = self.raw_text(block).is_ok_and(|text| !text.is_empty());
        if spec.content() == Content::None && has_text {
            return Err(Error::TextNotAllowed(block_type.to_owned()));
        }
        if spec.is_top_level() {
            self.lift_to_top(block)?;
        }
        self.meta(block)?.insert(TYPE, block_type)?;
        self.record(|| {
            let block_type = block_type.to_owned();
            Box::new(move |doc| doc.set_block_type(block, &block_type))
        });
        Ok(())
    }

    /// `block`'s attribute `key`, where it has one.
    pub fn attr(&self, block: BlockId, key: &str) -> Result<Option<Value>> {
        let value = self.meta(block)?.get(attr_key(key)?);
        let value = value.and_then(|v| v.into_value().ok());
        Ok(value.as_ref().and_then(Value::from_loro))
    }

    /// Checks whether an attribute exists, including values the SDK cannot decode.
    pub(crate) fn has_attr(&self, block: BlockId, key: &str) -> Result<bool> {
        Ok(self.meta(block)?.get(attr_key(key)?).is_some())
    }

    /// All of `block`'s attributes, by key.
    pub fn attrs(&self, block: BlockId) -> Result<BTreeMap<String, Value>> {
        let mut attrs = BTreeMap::new();
        self.meta(block)?.for_each(|key, value| {
            let value = value.into_value().ok();
            if let Some(value) = value.as_ref().and_then(Value::from_loro)
                && !key.starts_with('$')
            {
                attrs.insert(key.to_owned(), value);
            }
        });
        Ok(attrs)
    }

    /// Sets `block`'s attribute `key` to `value`. Keys starting with `$` are the document's
    /// own.
    pub fn set_attr(&mut self, block: BlockId, key: &str, value: impl Into<Value>) -> Result<()> {
        let value = value.into();
        self.meta(block)?.insert(attr_key(key)?, value.to_loro())?;
        self.record(|| {
            let key = key.to_owned();
            Box::new(move |doc| doc.set_attr(block, &key, value))
        });
        Ok(())
    }

    /// Takes `block`'s attribute `key` off.
    pub fn remove_attr(&mut self, block: BlockId, key: &str) -> Result<()> {
        self.meta(block)?.delete(attr_key(key)?)?;
        self.record(|| {
            let key = key.to_owned();
            Box::new(move |doc| doc.remove_attr(block, &key))
        });
        Ok(())
    }

    /// Copies every attribute as stored, including values [`Value`] cannot hold.
    pub(crate) fn copy_attrs(&mut self, from: BlockId, to: BlockId) -> Result<()> {
        let target = self.meta(to)?;
        let mut result = Ok(());
        self.meta(from)?.for_each(|key, value| {
            if let Ok(value) = value.into_value()
                && !key.starts_with('$')
                && result.is_ok()
            {
                result = target.insert(key, value);
            }
        });
        result?;
        self.record(|| Box::new(move |doc| doc.copy_attrs(from, to)));
        Ok(())
    }

    /// Whether blocks of this type hold text.
    pub(crate) fn holds_text(&self, block_type: &str) -> Result<bool> {
        Ok(self.spec_named(block_type)?.content() == Content::Inline)
    }

    pub(crate) fn spec(&self, block: BlockId) -> Result<&BlockSpec> {
        self.spec_named(&self.block_type(block)?)
    }

    pub(crate) fn raw_text(&self, block: BlockId) -> Result<LoroText> {
        if let Some(column) = block.1 {
            self.depth(block)?;
            return self.cell_text(block.0, column);
        }
        let container = self.meta(block)?.get(TEXT);
        container
            .and_then(|v| v.into_container().ok())
            .and_then(|c| c.into_text().ok())
            .ok_or(Error::NoInlineContent)
    }

    /// Children of a parent already known to be in the document and above
    /// [`MAX_DEPTH`]. Skips the check for that, which walks the ancestors.
    pub(crate) fn child_ids(&self, parent: TreeParentId) -> Vec<TreeID> {
        let mut children = self.tree.children(parent).unwrap_or_default();
        children.retain(|child| !self.is_trash(*child));
        children
    }

    fn spec_named(&self, name: &str) -> Result<&BlockSpec> {
        self.schema
            .block_spec(name)
            .ok_or_else(|| Error::UnknownBlockType(name.to_owned()))
    }

    fn meta(&self, block: BlockId) -> Result<LoroMap> {
        Ok(self.tree.get_meta(self.live(block)?)?)
    }

    /// The nesting level of a block in the document. A removed block, or one
    /// below [`MAX_DEPTH`], is not found. The walk is bounded by `MAX_DEPTH`
    /// no matter how deep a peer has nested the block.
    pub(crate) fn depth(&self, block: BlockId) -> Result<usize> {
        let depth = self.node_depth(block.0)?;
        // A cell is there while its row is in a table that has its column.
        // It counts as no level of its own.
        match block.1.map(|column| self.table_of(block.0, column)) {
            Some(None) => Err(Error::BlockNotFound),
            _ => Ok(depth),
        }
    }

    pub(crate) fn node_depth(&self, mut current: TreeID) -> Result<usize> {
        for depth in 1..=MAX_DEPTH {
            if self.is_trash(current) {
                break;
            }
            match self.tree.parent(current) {
                Some(TreeParentId::Node(parent)) => current = parent,
                Some(TreeParentId::Root) => return Ok(depth),
                _ => break,
            }
        }
        Err(Error::BlockNotFound)
    }

    /// Whether the children of `parent` are blocks of the document.
    pub(crate) fn shows(&self, parent: TreeParentId) -> bool {
        match parent {
            TreeParentId::Root => true,
            TreeParentId::Node(node) => self.node_depth(node).is_ok(),
            _ => false,
        }
    }

    /// The node of a block in the document. A cell has none.
    pub(crate) fn live(&self, block: BlockId) -> Result<TreeID> {
        if block.1.is_some() {
            return Err(Error::TablePart);
        }
        self.depth(block).map(|_| block.0)
    }

    /// A parent as Loro names it, with its nesting level (0 for the top level).
    fn slot(&self, parent: Option<BlockId>) -> Result<(TreeParentId, usize)> {
        match parent {
            Some(block) => Ok((TreeParentId::Node(self.live(block)?), self.depth(block)?)),
            None => Ok((TreeParentId::Root, 0)),
        }
    }

    /// Whether `block` is `ancestor` or lies inside it.
    fn is_within(&self, block: BlockId, ancestor: BlockId) -> bool {
        let ancestor = ancestor.0;
        let mut current = block.0;
        for _ in 0..MAX_DEPTH {
            if current == ancestor {
                return true;
            }
            match self.tree.parent(current) {
                Some(TreeParentId::Node(parent)) => current = parent,
                _ => break,
            }
        }
        false
    }

    /// How many levels the subtree of `block` spans, counting the block itself.
    fn height(&self, block: BlockId) -> Result<usize> {
        let base = self.depth(block)?;
        let mut height = 1;
        let mut stack = vec![(block.0, 1)];
        while let Some((id, level)) = stack.pop() {
            height = height.max(level);
            if base + level <= MAX_DEPTH {
                let children = self.child_ids(TreeParentId::Node(id));
                stack.extend(children.into_iter().map(|child| (child, level + 1)));
            }
        }
        Ok(height)
    }

    pub(crate) fn check_allows_children(&self, parent: Option<BlockId>) -> Result<()> {
        if let Some(parent) = parent {
            let spec = self.spec(parent)?;
            if !spec.allows_children() {
                return Err(Error::ChildrenNotAllowed(spec.name().to_owned()));
            }
        }
        Ok(())
    }

    /// Checks that `index` is a valid slot under `parent`. `moving` is a block
    /// that leaves its current slot first, which shifts its later siblings.
    fn check_index(
        &self,
        parent: Option<BlockId>,
        index: usize,
        moving: Option<TreeID>,
    ) -> Result<()> {
        let parent_id = self.slot(parent)?.0;
        let already_here = moving.is_some_and(|node| self.tree.parent(node) == Some(parent_id));
        let hidden = self.trash_under(parent_id) + usize::from(already_here);
        let len = self.tree.children_num(parent_id).unwrap_or(0) - hidden;
        if index > len {
            return Err(Error::OutOfBounds { index, len });
        }
        Ok(())
    }
}

fn attr_key(key: &str) -> Result<&str> {
    if key.starts_with('$') {
        return Err(Error::ReservedAttr(key.to_owned()));
    }
    Ok(key)
}
