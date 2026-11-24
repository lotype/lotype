use std::collections::HashMap;

use loro::{ExpandType, StyleConfig, StyleConfigMap};

use crate::{Atom, Error, Result, Value};

/// What a block holds besides child blocks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Content {
    /// Rich text.
    Inline,
    None,
}

#[derive(Debug, Clone)]
pub struct BlockSpec {
    name: String,
    content: Content,
    children: bool,
    style: bool,
    top_level: bool,
}

impl BlockSpec {
    pub fn new(name: impl Into<String>, content: Content) -> Self {
        Self {
            name: name.into(),
            content,
            children: false,
            style: true,
            top_level: false,
        }
    }

    pub fn with_children(mut self) -> Self {
        self.children = true;
        self
    }

    /// A block of this type has no colors or alignment of its own, as code,
    /// math and diagrams, which are drawn in a style of their own.
    pub fn without_style(mut self) -> Self {
        self.style = false;
        self
    }

    /// A block of this type is never in another block, as a heading, which
    /// starts a part of the page.
    pub fn top_level(mut self) -> Self {
        self.top_level = true;
        self
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn content(&self) -> Content {
        self.content
    }

    pub fn allows_children(&self) -> bool {
        self.children
    }

    /// Whether a block of this type takes `text_color`, `background_color`
    /// and `text_alignment`.
    pub fn takes_style(&self) -> bool {
        self.style
    }

    pub fn is_top_level(&self) -> bool {
        self.top_level
    }
}

/// The deepest heading: a level beyond it, from Markdown or a peer, is read
/// as this one.
pub const MAX_HEADING_LEVEL: i64 = 5;

/// Whether a mark grows when text is typed at its edges.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Expand {
    Before,
    After,
    Both,
    None,
}

#[derive(Debug, Clone)]
pub struct MarkSpec {
    name: String,
    expand: Expand,
    requires_value: bool,
}

impl MarkSpec {
    pub fn new(name: impl Into<String>, expand: Expand) -> Self {
        Self {
            name: name.into(),
            expand,
            requires_value: false,
        }
    }

    /// Requires a string or integer value, as for links and colors.
    pub fn with_value(mut self) -> Self {
        self.requires_value = true;
        self
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn expand(&self) -> Expand {
        self.expand
    }

    pub(crate) fn allows_boolean(&self) -> bool {
        !self.requires_value
    }

    pub(crate) fn check_value(&self, value: &Value) -> Result<()> {
        if !self.allows_boolean() && matches!(value, Value::Bool(_)) {
            return Err(Error::MarkValueRequired(self.name.clone()));
        }
        Ok(())
    }
}

/// A type of inline atom: something in a line of text that is not text.
#[derive(Debug, Clone)]
pub struct InlineSpec {
    name: String,
    text: fn(&Atom) -> String,
}

impl InlineSpec {
    /// `text` gives an atom as plain text, for the clipboard and Markdown,
    /// such as `@name` for a mention.
    pub fn new(name: impl Into<String>, text: fn(&Atom) -> String) -> Self {
        let name = name.into();
        Self { name, text }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn text(&self, atom: &Atom) -> String {
        (self.text)(atom)
    }
}

/// The block types, marks and inline atoms a document accepts.
#[derive(Debug, Clone, Default)]
pub struct Schema {
    blocks: HashMap<String, BlockSpec>,
    marks: HashMap<String, MarkSpec>,
    inlines: HashMap<String, InlineSpec>,
}

impl Schema {
    pub fn new() -> Self {
        Self::default()
    }

    /// The default block and mark set.
    pub fn builtin() -> Self {
        use Content::{Inline, None};

        let mut schema = Self::new();
        for name in [
            "paragraph",
            "quote",
            "bullet_list_item",
            "numbered_list_item",
            "check_list_item",
            "toggle_list_item",
        ] {
            schema = schema.block(BlockSpec::new(name, Inline).with_children());
        }
        schema = schema.block(BlockSpec::new("alert", None).with_children());
        schema = schema.block(BlockSpec::new("heading", Inline).top_level());
        schema = schema.block(BlockSpec::new("code_block", Inline).without_style());
        for name in ["image", "file", "divider", "page_break", "table_row"] {
            schema = schema.block(BlockSpec::new(name, None));
        }
        schema = schema.block(BlockSpec::new("table", None).with_children());
        schema = schema.block(BlockSpec::new("table_cell", Inline));
        for name in ["bold", "italic", "underline", "strike"] {
            schema = schema.mark(MarkSpec::new(name, Expand::After));
        }
        for name in ["text_color", "background_color"] {
            schema = schema.mark(MarkSpec::new(name, Expand::After).with_value());
        }
        schema
            .mark(MarkSpec::new("code", Expand::None))
            .mark(MarkSpec::new("link", Expand::None).with_value())
    }

    /// Adds a block type, replacing any existing one with the same name.
    pub fn block(mut self, spec: BlockSpec) -> Self {
        self.blocks.insert(spec.name.clone(), spec);
        self
    }

    /// Adds a mark, replacing any existing one with the same name.
    pub fn mark(mut self, spec: MarkSpec) -> Self {
        self.marks.insert(spec.name.clone(), spec);
        self
    }

    /// Adds a type of inline atom, replacing any existing one with the same name.
    pub fn inline(mut self, spec: InlineSpec) -> Self {
        self.inlines.insert(spec.name.clone(), spec);
        self
    }

    pub fn inline_spec(&self, name: &str) -> Option<&InlineSpec> {
        self.inlines.get(name)
    }

    pub fn block_spec(&self, name: &str) -> Option<&BlockSpec> {
        self.blocks.get(name)
    }

    pub fn mark_spec(&self, name: &str) -> Option<&MarkSpec> {
        self.marks.get(name)
    }

    pub(crate) fn style_config(&self) -> StyleConfigMap {
        let mut config = StyleConfigMap::new();
        for spec in self.marks.values() {
            let expand = match spec.expand {
                Expand::Before => ExpandType::Before,
                Expand::After => ExpandType::After,
                Expand::Both => ExpandType::Both,
                Expand::None => ExpandType::None,
            };
            config.insert(spec.name.as_str().into(), StyleConfig { expand });
        }
        config
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_specs() {
        let schema = Schema::builtin();
        let paragraph = schema.block_spec("paragraph").unwrap();
        assert_eq!(paragraph.content(), Content::Inline);
        assert!(paragraph.allows_children());

        let heading = schema.block_spec("heading").unwrap();
        assert!(!heading.allows_children());

        let alert = schema.block_spec("alert").unwrap();
        assert_eq!(alert.content(), Content::None);
        assert!(alert.allows_children());

        assert_eq!(schema.mark_spec("bold").unwrap().expand(), Expand::After);
        assert_eq!(schema.mark_spec("link").unwrap().expand(), Expand::None);
        assert!(schema.block_spec("video").is_none());
    }

    #[test]
    fn custom_spec_replaces_same_name() {
        let schema =
            Schema::builtin().block(BlockSpec::new("heading", Content::Inline).with_children());
        assert!(schema.block_spec("heading").unwrap().allows_children());
    }
}
