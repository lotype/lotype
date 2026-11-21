use std::collections::BTreeMap;

use loro::{LoroDoc, LoroMap, LoroText, LoroTree, TreeID, TreeParentId};

use crate::{BlockSpec, Error, Result, Schema, Value};

const TREE: &str = "blocks";
pub(crate) const TYPE: &str = "$type";
pub(crate) const TEXT: &str = "$text";

/// Identifies a block.
///
/// Undoing the removal of a block restores it under a new id.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BlockId(pub(crate) TreeID);

/// A tree of blocks.
pub struct Document {
    pub(crate) doc: LoroDoc,
    pub(crate) tree: LoroTree,
    schema: Schema,
}

impl Document {
    pub fn new(schema: Schema) -> Self {
        let doc = LoroDoc::new();
        doc.config_text_style(schema.style_config());
        let tree = doc.get_tree(TREE);
        Self { doc, tree, schema }
    }

    pub fn schema(&self) -> &Schema {
        &self.schema
    }

    /// Ends the current change. Each commit is one undo step.
    pub fn commit(&mut self) {
        self.doc.commit();
    }

    /// Child blocks of `parent`, or the top-level blocks when `parent` is `None`.
    pub fn children(&self, parent: Option<BlockId>) -> Result<Vec<BlockId>> {
        let children = self.child_ids(self.parent_id(parent)?);
        Ok(children.into_iter().map(BlockId).collect())
    }

    /// The parent of `block`, or `None` for a top-level block.
    pub fn parent(&self, block: BlockId) -> Result<Option<BlockId>> {
        match self.tree.parent(self.live(block)?) {
            Some(TreeParentId::Node(id)) => Ok(Some(BlockId(id))),
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
        self.spec_named(block_type)?;
        self.check_child_slot(parent, index, None)?;
        let id = self.tree.create_at(self.parent_id(parent)?, index)?;
        let meta = self.tree.get_meta(id)?;
        meta.insert(TYPE, block_type)?;
        // Created up front for every block: creating it later from two peers
        // at once would leave two containers and lose one side's text.
        meta.insert_container(TEXT, LoroText::new())?;
        Ok(BlockId(id))
    }

    /// Removes `block` and all of its descendants.
    pub fn remove_block(&mut self, block: BlockId) -> Result<()> {
        self.tree.delete(self.live(block)?)?;
        Ok(())
    }

    /// Moves `block` so that it ends up at `index` among the children of `parent`.
    pub fn move_block(
        &mut self,
        block: BlockId,
        parent: Option<BlockId>,
        index: usize,
    ) -> Result<()> {
        let id = self.live(block)?;
        self.check_child_slot(parent, index, Some(block))?;
        self.tree.mov_to(id, self.parent_id(parent)?, index)?;
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
        if !spec.allows_children() && !self.child_ids(self.parent_id(Some(block))?).is_empty() {
            return Err(Error::ChildrenNotAllowed(block_type.to_owned()));
        }
        if spec.content() == crate::Content::None && !self.raw_text(block)?.is_empty() {
            return Err(Error::NoInlineContent);
        }
        self.meta(block)?.insert(TYPE, block_type)?;
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
        self.meta(block)?
            .insert(attr_key(key)?, value.into().to_loro())?;
        Ok(())
    }

    pub fn remove_attr(&mut self, block: BlockId, key: &str) -> Result<()> {
        self.meta(block)?.delete(attr_key(key)?)?;
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

    /// Children of a parent already known to be live. Skips the liveness
    /// check, which walks every ancestor.
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

    // `LoroTree::contains` stays true for removed blocks.
    pub(crate) fn live(&self, block: BlockId) -> Result<TreeID> {
        match self.tree.is_node_deleted(&block.0) {
            Ok(false) => Ok(block.0),
            _ => Err(Error::BlockNotFound),
        }
    }

    fn parent_id(&self, parent: Option<BlockId>) -> Result<TreeParentId> {
        match parent {
            Some(block) => Ok(TreeParentId::Node(self.live(block)?)),
            None => Ok(TreeParentId::Root),
        }
    }

    /// Checks that `parent` can take a child at `index`. `moving` is a block
    /// that leaves its current slot first, which shifts its later siblings.
    fn check_child_slot(
        &self,
        parent: Option<BlockId>,
        index: usize,
        moving: Option<BlockId>,
    ) -> Result<()> {
        if let Some(parent) = parent {
            let spec = self.spec(parent)?;
            if !spec.allows_children() {
                return Err(Error::ChildrenNotAllowed(spec.name().to_owned()));
            }
        }
        let parent_id = self.parent_id(parent)?;
        let already_here = moving.is_some_and(|block| self.tree.parent(block.0) == Some(parent_id));
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
