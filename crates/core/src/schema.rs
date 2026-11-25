use std::collections::HashMap;

use loro::{ExpandType, StyleConfig, StyleConfigMap};

use crate::{Atom, BlockId, Document, Error, Result, Value, mark};

/// What a block holds besides child blocks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Content {
    /// Rich text.
    Inline,
    None,
}

/// What a block of a type is to editing: what Enter, Backspace and typed
/// Markdown do in it. A schema gives its own types these as the built-in
/// one gives its own.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Role {
    /// Text, as a paragraph or a heading.
    #[default]
    Text,
    /// Enter goes on with the next item, and leaves the list from an empty
    /// one, as a bullet, numbered or check list item.
    ListItem,
    /// A list item whose blocks a view shows or hides: Enter in an open one
    /// writes in it.
    Toggle,
    /// Holds blocks that Enter leaves for a text block after it, as a quote
    /// or an alert.
    Wrapper,
    /// Text as typed: no Markdown turns into anything, no slash opens a
    /// menu, and a paste is plain text, as in a code block.
    Code,
}

impl Role {
    /// Enter goes on with the next item, as in a toggle too.
    pub fn is_list_item(self) -> bool {
        matches!(self, Self::ListItem | Self::Toggle)
    }
}

#[derive(Debug, Clone)]
pub struct BlockSpec {
    name: String,
    content: Content,
    children: bool,
    style: bool,
    top_level: bool,
    role: Role,
}

impl BlockSpec {
    pub fn new(name: impl Into<String>, content: Content) -> Self {
        Self {
            name: name.into(),
            content,
            children: false,
            style: true,
            top_level: false,
            role: Role::Text,
        }
    }

    /// What a block of this type is to editing.
    pub fn with_role(mut self, role: Role) -> Self {
        self.role = role;
        self
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

    pub fn role(&self) -> Role {
        self.role
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
    text_block: Option<String>,
}

impl Schema {
    pub fn new() -> Self {
        Self::default()
    }

    /// The default block and mark set.
    pub fn builtin() -> Self {
        use Content::{Inline, None};

        let mut schema = Self::new().with_text_block("paragraph");
        let roles = [
            ("paragraph", Role::Text),
            ("quote", Role::Wrapper),
            ("bullet_list_item", Role::ListItem),
            ("numbered_list_item", Role::ListItem),
            ("check_list_item", Role::ListItem),
            ("toggle_list_item", Role::Toggle),
        ];
        for (name, role) in roles {
            let spec = BlockSpec::new(name, Inline).with_children().with_role(role);
            schema = schema.block(spec);
        }
        let alert = BlockSpec::new("alert", None).with_children();
        schema = schema.block(alert.with_role(Role::Wrapper));
        schema = schema.block(BlockSpec::new("heading", Inline).top_level());
        let code = BlockSpec::new("code_block", Inline).without_style();
        schema = schema.block(code.with_role(Role::Code));
        for name in ["image", "file", "divider", "page_break", "table_row"] {
            schema = schema.block(BlockSpec::new(name, None));
        }
        schema = schema.block(BlockSpec::new("table", None).with_children());
        schema = schema.block(BlockSpec::new("table_cell", Inline));
        for name in [mark::BOLD, mark::ITALIC, mark::UNDERLINE, mark::STRIKE] {
            schema = schema.mark(MarkSpec::new(name, Expand::After));
        }
        for name in [mark::TEXT_COLOR, mark::BACKGROUND_COLOR] {
            schema = schema.mark(MarkSpec::new(name, Expand::After).with_value());
        }
        schema
            .mark(MarkSpec::new(mark::CODE, Expand::None))
            .mark(MarkSpec::new(mark::LINK, Expand::None).with_value())
    }

    /// Names the block typing starts in, and that what leaves another block
    /// becomes, as a paragraph. It must be a block type of the schema that
    /// holds text; without one, commands that would make one do nothing.
    pub fn with_text_block(mut self, name: impl Into<String>) -> Self {
        self.text_block = Some(name.into());
        self
    }

    /// The block for text, where it is one of the schema's types with text.
    pub fn text_block(&self) -> Option<&str> {
        let name = self.text_block.as_deref()?;
        let spec = self.blocks.get(name)?;
        (spec.content == Content::Inline).then_some(name)
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

impl Document {
    /// What `block` is to editing; text for a type this schema lacks.
    pub fn role(&self, block: BlockId) -> Role {
        self.spec(block).map_or(Role::Text, BlockSpec::role)
    }

    /// The schema's block for text, to make one of.
    pub fn text_block(&self) -> Result<String> {
        let name = self.schema().text_block().ok_or(Error::NoTextBlock)?;
        Ok(name.to_owned())
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
