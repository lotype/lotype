use std::collections::BTreeMap;
use std::fmt;
use std::str::FromStr;
use std::sync::Mutex;

use loro::{ExpandType, LoroDoc, LoroMap, LoroText, LoroTree, StyleConfig, TreeID, TreeParentId};

use crate::draft::Draft;
use crate::ids::Restored;
use crate::order::OrderCache;
use crate::{BlockSpec, Content, Error, Result, Schema, Value};

/// The deepest nesting level a document has. Top-level blocks are at level 1.
///
/// Blocks that another peer nests deeper stay in the shared data but are not
/// part of the document: no method returns them, and their ids are not found.
pub const MAX_DEPTH: usize = 128;

const TREE: &str = "blocks";
pub(crate) const TYPE: &str = "$type";
pub(crate) const TEXT: &str = "$text";
pub(crate) const ID: &str = "$id";

/// The identity of a block. It stays the same when undo or redo brings a
/// removed block back.
///
/// To keep a reference outside the document, store the id as text with
/// `to_string` and read it back with `parse`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BlockId(pub(crate) TreeID);

impl fmt::Display for BlockId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl FromStr for BlockId {
    type Err = Error;

    fn from_str(text: &str) -> Result<Self> {
        let id = TreeID::try_from(text).map_err(|_| Error::InvalidBlockId(text.to_owned()))?;
        Ok(Self(id))
    }
}

/// A tree of blocks.
pub struct Document {
    pub(crate) doc: LoroDoc,
    pub(crate) tree: LoroTree,
    pub(crate) order: Mutex<OrderCache>,
    pub(crate) draft: Option<Draft>,
    pub(crate) restored: Restored,
    schema: Schema,
}

impl Document {
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
            restored: Restored::default(),
            schema,
        }
    }

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
        let (parent, depth) = self.slot(parent)?;
        if depth == MAX_DEPTH {
            return Ok(Vec::new());
        }
        let children = self.child_ids(parent);
        Ok(children.into_iter().map(|id| self.id_of(id)).collect())
    }

    /// The parent of `block`, or `None` for a top-level block.
    pub fn parent(&self, block: BlockId) -> Result<Option<BlockId>> {
        match self.tree.parent(self.live(block)?) {
            Some(TreeParentId::Node(id)) => Ok(Some(self.id_of(id))),
            Some(TreeParentId::Root) => Ok(None),
            _ => Err(Error::BlockNotFound),
        }
    }

    pub fn contains(&self, block: BlockId) -> bool {
        self.live(block).is_ok()
    }

    pub fn insert_block(
        &mut self,
        parent: Option<BlockId>,
        index: usize,
        block_type: &str,
    ) -> Result<BlockId> {
        self.check_allows_children(parent)?;
        if self.slot(parent)?.1 == MAX_DEPTH {
            return Err(Error::TooDeep);
        }
        self.create_block(parent, index, block_type)
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
        let id = self.tree.create_at(self.slot(parent)?.0, index)?;
        let meta = self.tree.get_meta(id)?;
        meta.insert(TYPE, block_type)?;
        // Undo recreates a removed block as a new node; this tells which block it was.
        meta.insert(ID, id.to_string())?;
        // Created up front for every block: creating it later from two peers
        // at once would leave two containers and lose one side's text.
        meta.insert_container(TEXT, LoroText::new())?;
        self.record(|| {
            let block_type = block_type.to_owned();
            Box::new(move |doc| doc.create_block(parent, index, &block_type).map(drop))
        });
        Ok(BlockId(id))
    }

    /// Removes `block` and all of its descendants.
    pub fn remove_block(&mut self, block: BlockId) -> Result<()> {
        self.tree.delete(self.live(block)?)?;
        // Peers with the same blocks must agree on their ids, and removing a
        // block can settle who gets an id that several blocks claimed.
        if self.restored.claims || self.restored.hidden {
            self.refresh_ids();
        }
        self.record(|| Box::new(move |doc| doc.remove_block(block)));
        Ok(())
    }

    pub(crate) fn remove_blocks(
        &mut self,
        blocks: impl IntoIterator<Item = BlockId>,
    ) -> Result<()> {
        // Resolve the public ids up front: removing a claimant can change
        // the id of a block later in this same deletion.
        let nodes: Vec<_> = blocks
            .into_iter()
            .filter_map(|block| self.live(block).ok())
            .collect();
        self.remove_nodes(nodes)
    }

    fn remove_nodes(&mut self, nodes: Vec<TreeID>) -> Result<()> {
        let result = nodes.iter().copied().try_for_each(|node| {
            if self.node_depth(node).is_ok() {
                self.tree.delete(node)?;
                self.record(|| Box::new(move |doc| doc.tree.delete(node).map_err(Into::into)));
            }
            Ok(())
        });
        if self.restored.claims || self.restored.hidden {
            self.refresh_ids();
            self.record(|| {
                Box::new(|doc| {
                    doc.refresh_ids();
                    Ok(())
                })
            });
        }
        result
    }

    /// Moves `block` so that it ends up at `index` among the children of `parent`.
    pub fn move_block(
        &mut self,
        block: BlockId,
        parent: Option<BlockId>,
        index: usize,
    ) -> Result<()> {
        self.check_allows_children(parent)?;
        // Before the depth rule: a block is always "too deep" for its own subtree.
        if parent.is_some_and(|parent| self.is_within(parent, block)) {
            return Err(Error::CyclicMove);
        }
        let parent_depth = self.slot(parent)?.1;
        if parent_depth + self.height(block)? > MAX_DEPTH {
            return Err(Error::TooDeep);
        }
        let visibility_changed = self.restored.hidden && self.depth(block)? != parent_depth + 1;
        self.relocate_block(block, parent, index)?;
        if visibility_changed {
            self.refresh_ids();
            self.record(|| {
                Box::new(|doc| {
                    doc.refresh_ids();
                    Ok(())
                })
            });
        }
        Ok(())
    }

    /// [`Self::move_block`] without the rules on the parent's type and on
    /// depth, like [`Self::create_block`]. Commands that lift children
    /// refresh ids when they remove the source block at the end.
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
        self.tree.mov_to(node, self.slot(parent)?.0, index)?;
        self.record(|| Box::new(move |doc| doc.relocate_node(node, parent, index)));
        Ok(())
    }

    pub fn block_type(&self, block: BlockId) -> Result<String> {
        let value = self.meta(block)?.get(TYPE);
        let value = value.and_then(|v| v.into_value().ok());
        match value.as_ref().and_then(Value::from_loro) {
            Some(Value::Str(name)) => Ok(name),
            _ => Err(Error::UnknownBlockType(String::new())),
        }
    }

    pub fn set_block_type(&mut self, block: BlockId, block_type: &str) -> Result<()> {
        let spec = self.spec_named(block_type)?;
        if !spec.allows_children() && !self.children(Some(block))?.is_empty() {
            return Err(Error::ChildrenNotAllowed(block_type.to_owned()));
        }
        let has_text = self.raw_text(block).is_ok_and(|text| !text.is_empty());
        if spec.content() == Content::None && has_text {
            return Err(Error::TextNotAllowed(block_type.to_owned()));
        }
        self.meta(block)?.insert(TYPE, block_type)?;
        self.record(|| {
            let block_type = block_type.to_owned();
            Box::new(move |doc| doc.set_block_type(block, &block_type))
        });
        Ok(())
    }

    pub fn attr(&self, block: BlockId, key: &str) -> Result<Option<Value>> {
        let value = self.meta(block)?.get(attr_key(key)?);
        let value = value.and_then(|v| v.into_value().ok());
        Ok(value.as_ref().and_then(Value::from_loro))
    }

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

    pub fn set_attr(&mut self, block: BlockId, key: &str, value: impl Into<Value>) -> Result<()> {
        let value = value.into();
        self.meta(block)?.insert(attr_key(key)?, value.to_loro())?;
        self.record(|| {
            let key = key.to_owned();
            Box::new(move |doc| doc.set_attr(block, &key, value))
        });
        Ok(())
    }

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

    pub(crate) fn spec(&self, block: BlockId) -> Result<&BlockSpec> {
        self.spec_named(&self.block_type(block)?)
    }

    pub(crate) fn raw_text(&self, block: BlockId) -> Result<LoroText> {
        let container = self.meta(block)?.get(TEXT);
        container
            .and_then(|v| v.into_container().ok())
            .and_then(|c| c.into_text().ok())
            .ok_or(Error::NoInlineContent)
    }

    /// Children of a parent already known to be in the document and above
    /// [`MAX_DEPTH`]. Skips the check for that, which walks the ancestors.
    pub(crate) fn child_ids(&self, parent: TreeParentId) -> Vec<TreeID> {
        self.tree.children(parent).unwrap_or_default()
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
        // A block that came back answers only to its first id.
        if self.restored.is_restored(block.0) {
            return Err(Error::BlockNotFound);
        }
        self.node_depth(self.node(block))
    }

    fn node_depth(&self, mut current: TreeID) -> Result<usize> {
        for depth in 1..=MAX_DEPTH {
            match self.tree.parent(current) {
                Some(TreeParentId::Node(parent)) => current = parent,
                Some(TreeParentId::Root) => return Ok(depth),
                _ => break,
            }
        }
        Err(Error::BlockNotFound)
    }

    pub(crate) fn live(&self, block: BlockId) -> Result<TreeID> {
        self.depth(block).map(|_| self.node(block))
    }

    /// A parent as Loro names it, with its nesting level (0 for the top level).
    fn slot(&self, parent: Option<BlockId>) -> Result<(TreeParentId, usize)> {
        match parent {
            Some(block) => Ok((TreeParentId::Node(self.node(block)), self.depth(block)?)),
            None => Ok((TreeParentId::Root, 0)),
        }
    }

    /// Whether `block` is `ancestor` or lies inside it.
    fn is_within(&self, block: BlockId, ancestor: BlockId) -> bool {
        let ancestor = self.node(ancestor);
        let mut current = self.node(block);
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
        let mut stack = vec![(self.node(block), 1)];
        while let Some((id, level)) = stack.pop() {
            height = height.max(level);
            if base + level <= MAX_DEPTH {
                let children = self.child_ids(TreeParentId::Node(id));
                stack.extend(children.into_iter().map(|child| (child, level + 1)));
            }
        }
        Ok(height)
    }

    fn check_allows_children(&self, parent: Option<BlockId>) -> Result<()> {
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
        let len = self.tree.children_num(parent_id).unwrap_or(0) - usize::from(already_here);
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
