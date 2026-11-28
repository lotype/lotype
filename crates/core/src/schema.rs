use std::collections::HashMap;

use loro::{ExpandType, StyleConfig, StyleConfigMap};

use crate::{Atom, BlockId, Document, Error, Result, Value, attr, block, mark};

/// What a block holds besides child blocks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Content {
    /// Rich text.
    Inline,
    /// No text: only child blocks, or nothing.
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
/// A type of block: what it holds and how editing treats it.
pub struct BlockSpec {
    name: String,
    content: Content,
    children: bool,
    style: bool,
    top_level: bool,
    role: Role,
    attrs: Vec<String>,
    hint: Option<String>,
    fence: Option<String>,
    display_math: bool,
}

impl BlockSpec {
    /// A type named `name` holding `content`, that takes no children, takes colors and
    /// alignment, and has the role of text.
    pub fn new(name: impl Into<String>, content: Content) -> Self {
        Self {
            name: name.into(),
            content,
            children: false,
            style: true,
            top_level: false,
            role: Role::Text,
            attrs: Vec::new(),
            hint: None,
            fence: None,
            display_math: false,
        }
    }

    /// The attributes of its own that a block of this type has in
    /// Markdown, as a heading its level. Reading Markdown sets no others,
    /// and what replaces a block's Markdown leaves the others as they are.
    pub fn with_attrs<A: Into<String>>(mut self, attrs: impl IntoIterator<Item = A>) -> Self {
        self.attrs = attrs.into_iter().map(Into::into).collect();
        self
    }

    /// What blocks of this type are and how they are written, in a few
    /// words, for whoever is told what a document takes: a model above all.
    pub fn with_hint(mut self, hint: impl Into<String>) -> Self {
        self.hint = Some(hint.into());
        self
    }

    /// A block of this type is written in Markdown as a fenced block
    /// tagged `tag`, as GitHub writes a diagram: its attributes after the
    /// tag as `key=value`, and its text inside. A type without this is
    /// written as its text alone, and Markdown makes none of it.
    pub fn written_as_fence(mut self, tag: impl Into<String>) -> Self {
        self.fence = Some(tag.into());
        self
    }

    /// Marks this block type as the schema's display math block, enabling
    /// Markdown `$$...$$` round-tripping and the `$$ ` block input rule.
    pub fn as_display_math(mut self) -> Self {
        self.display_math = true;
        self
    }

    /// What a block of this type is to editing.
    pub fn with_role(mut self, role: Role) -> Self {
        self.role = role;
        self
    }

    /// A block of this type takes child blocks.
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

    /// The type's name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// What a block of this type holds besides child blocks.
    pub fn content(&self) -> Content {
        self.content
    }

    /// Whether a block of this type takes child blocks.
    pub fn allows_children(&self) -> bool {
        self.children
    }

    /// Whether a block of this type takes `text_color`, `background_color`
    /// and `text_alignment`.
    pub fn takes_style(&self) -> bool {
        self.style
    }

    /// Whether a block of this type is never in another block.
    pub fn is_top_level(&self) -> bool {
        self.top_level
    }

    /// What a block of this type is to editing.
    pub fn role(&self) -> Role {
        self.role
    }

    /// The attributes of its own that a block of this type has in Markdown.
    pub fn markdown_attrs(&self) -> &[String] {
        &self.attrs
    }

    /// What blocks of this type are and how they are written, where said.
    pub fn hint(&self) -> Option<&str> {
        self.hint.as_deref()
    }

    /// The tag of the fenced block this type is written as in Markdown.
    pub fn fence(&self) -> Option<&str> {
        self.fence.as_deref()
    }

    /// Whether this block type represents display math (`$$...$$`).
    pub fn is_display_math(&self) -> bool {
        self.display_math
    }
}

/// The deepest heading: a level beyond it, from Markdown or a peer, is read
/// as this one.
pub const MAX_HEADING_LEVEL: i64 = 5;

/// Whether a mark grows when text is typed at its edges.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Expand {
    /// Text typed right before the mark gets it.
    Before,
    /// Text typed right after the mark gets it.
    After,
    /// Text typed on either side of the mark gets it.
    Both,
    /// Text typed next to the mark does not get it.
    None,
}

#[derive(Debug, Clone)]
/// A mark: what text can carry, as bold or a link.
pub struct MarkSpec {
    name: String,
    expand: Expand,
    requires_value: bool,
}

impl MarkSpec {
    /// A mark named `name`, on or off, growing over typed text as `expand` says.
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

    /// The mark's name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Whether text typed at the mark's edges gets it.
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
    math_attr: Option<String>,
    hint: Option<String>,
}

impl InlineSpec {
    /// `text` gives an atom as plain text, for the clipboard and Markdown,
    /// such as `@name` for a mention.
    pub fn new(name: impl Into<String>, text: fn(&Atom) -> String) -> Self {
        let name = name.into();
        Self {
            name,
            text,
            math_attr: None,
            hint: None,
        }
    }

    /// Marks this inline atom type as the schema's inline math atom, storing
    /// its LaTeX source in attribute `attr`. Enables Markdown `$...$`
    /// round-tripping and the `$...$` inline input rule.
    pub fn as_inline_math(mut self, attr: impl Into<String>) -> Self {
        self.math_attr = Some(attr.into());
        self
    }

    /// A few words saying what this inline atom is and how it is written in
    /// Markdown, for a model that edits the document, such as
    /// `"inline LaTeX equations (`$...$`)"`.
    pub fn with_hint(mut self, hint: impl Into<String>) -> Self {
        self.hint = Some(hint.into());
        self
    }

    /// The atom type's name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// `atom` as plain text, for the clipboard and Markdown.
    pub fn text(&self, atom: &Atom) -> String {
        (self.text)(atom)
    }

    /// Attribute name storing LaTeX source when this spec is inline math.
    pub fn inline_math_attr(&self) -> Option<&str> {
        self.math_attr.as_deref()
    }

    /// What this inline atom is and how it is written in Markdown, where a
    /// description was given.
    pub fn hint(&self) -> Option<&str> {
        self.hint.as_deref()
    }
}

/// `spec` with `hint`, where there is one to give.
fn described(spec: BlockSpec, hint: &str) -> BlockSpec {
    match hint {
        "" => spec,
        hint => spec.with_hint(hint),
    }
}

/// The block types, marks and inline atoms a document accepts.
#[derive(Debug, Clone, Default)]
pub struct Schema {
    blocks: HashMap<String, BlockSpec>,
    /// The names of the block types, in the order they were added.
    order: Vec<String>,
    marks: HashMap<String, MarkSpec>,
    inlines: HashMap<String, InlineSpec>,
    /// The names of the inline atom types, in the order they were added.
    inline_order: Vec<String>,
    text_block: Option<String>,
}

impl Schema {
    /// A schema with no block types, marks or atoms.
    pub fn new() -> Self {
        Self::default()
    }

    /// The default block and mark set.
    pub fn builtin() -> Self {
        use Content::{Inline, None};

        let mut schema = Self::new().with_text_block(block::PARAGRAPH);
        // Each with how it is written and the attributes Markdown gives it.
        let texts: [(_, _, _, &[&str]); 7] = [
            (block::PARAGRAPH, Role::Text, "", &[]),
            (
                block::HEADING,
                Role::Text,
                "headings (`# Title`)",
                &[attr::LEVEL],
            ),
            (
                block::BULLET_LIST_ITEM,
                Role::ListItem,
                "bullet items (`- item`)",
                &[],
            ),
            (
                block::NUMBERED_LIST_ITEM,
                Role::ListItem,
                "numbered items (`1. item`)",
                &[attr::START],
            ),
            (
                block::CHECK_LIST_ITEM,
                Role::ListItem,
                "tasks (`- [ ] task`)",
                &[attr::CHECKED],
            ),
            (block::TOGGLE_LIST_ITEM, Role::Toggle, "", &[]),
            (block::QUOTE, Role::Wrapper, "quotes (`> text`)", &[]),
        ];
        for (name, role, hint, attrs) in texts {
            let spec = BlockSpec::new(name, Inline).with_role(role);
            let spec = match name {
                block::HEADING => spec.top_level(),
                _ => spec.with_children(),
            };
            schema = schema.block(described(spec, hint).with_attrs(attrs.iter().copied()));
        }
        let alert = BlockSpec::new(block::ALERT, None).with_children();
        let alert = alert.with_role(Role::Wrapper).with_attrs([attr::KIND]);
        let hint = "alerts (`> [!NOTE]`, with their text added after them)";
        schema = schema.block(alert.with_hint(hint));
        let code = BlockSpec::new(block::CODE_BLOCK, Inline).without_style();
        let code = code.with_role(Role::Code).with_attrs([
            attr::LANGUAGE,
            attr::TITLE,
            attr::TWOSLASH,
            attr::HIGHLIGHT_LINES,
        ]);
        let code_hint = "fenced code (` ```lang twoslash title=\"file\" {1,3-4} `)";
        schema = schema.block(code.with_hint(code_hint));
        schema = schema.block(
            BlockSpec::new(block::TABLE, None)
                .with_children()
                .with_hint("tables"),
        );
        let voids: [(_, _, &[&str]); 5] = [
            (block::DIVIDER, "dividers (`---`)", &[]),
            (
                block::IMAGE,
                "images (`![caption](url)`)",
                &[attr::URL, attr::CAPTION],
            ),
            (block::FILE, "", &[attr::NAME, attr::URL]),
            (block::PAGE_BREAK, "", &[]),
            (block::TABLE_ROW, "", &[]),
        ];
        for (name, hint, attrs) in voids {
            let spec = BlockSpec::new(name, None).with_attrs(attrs.iter().copied());
            schema = schema.block(described(spec, hint));
        }
        schema = schema.block(BlockSpec::new(block::TABLE_CELL, Inline));
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
        let name = spec.name.clone();
        if self.blocks.insert(name.clone(), spec).is_none() {
            self.order.push(name);
        }
        self
    }

    /// The block types, in the order they were added.
    pub fn block_specs(&self) -> impl Iterator<Item = &BlockSpec> {
        self.order.iter().filter_map(|name| self.blocks.get(name))
    }

    /// The block type written in Markdown as a fenced block tagged `tag`.
    pub fn fenced(&self, tag: &str) -> Option<&BlockSpec> {
        self.block_specs().find(|spec| spec.fence() == Some(tag))
    }

    /// The block type registered for display math (`$$...$$`), if any.
    pub fn display_math_block(&self) -> Option<&BlockSpec> {
        self.block_specs().find(|spec| spec.is_display_math())
    }

    /// The inline atom types, in the order they were added.
    pub fn inline_specs(&self) -> impl Iterator<Item = &InlineSpec> {
        self.inline_order
            .iter()
            .filter_map(|name| self.inlines.get(name))
    }

    /// The inline atom type and attribute key registered for inline math (`$...$`), if any.
    pub fn inline_math_spec(&self) -> Option<(&InlineSpec, &str)> {
        self.inline_specs()
            .find_map(|spec| Some((spec, spec.inline_math_attr()?)))
    }

    /// Adds a mark, replacing any existing one with the same name.
    pub fn mark(mut self, spec: MarkSpec) -> Self {
        self.marks.insert(spec.name.clone(), spec);
        self
    }

    /// Adds a type of inline atom, replacing any existing one with the same name.
    pub fn inline(mut self, spec: InlineSpec) -> Self {
        let name = spec.name.clone();
        if self.inlines.insert(name.clone(), spec).is_none() {
            self.inline_order.push(name);
        }
        self
    }

    /// The atom type named `name`, where the schema has it.
    pub fn inline_spec(&self, name: &str) -> Option<&InlineSpec> {
        self.inlines.get(name)
    }

    /// The block type named `name`, where the schema has it.
    pub fn block_spec(&self, name: &str) -> Option<&BlockSpec> {
        self.blocks.get(name)
    }

    /// The mark named `name`, where the schema has it.
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
        let paragraph = schema.block_spec(block::PARAGRAPH).unwrap();
        assert_eq!(paragraph.content(), Content::Inline);
        assert!(paragraph.allows_children());

        let heading = schema.block_spec(block::HEADING).unwrap();
        assert!(!heading.allows_children());

        let alert = schema.block_spec(block::ALERT).unwrap();
        assert_eq!(alert.content(), Content::None);
        assert!(alert.allows_children());

        assert_eq!(schema.mark_spec("bold").unwrap().expand(), Expand::After);
        assert_eq!(schema.mark_spec("link").unwrap().expand(), Expand::None);
        assert!(schema.block_spec("video").is_none());
    }

    #[test]
    fn custom_spec_replaces_same_name() {
        let schema = Schema::builtin()
            .block(BlockSpec::new(block::HEADING, Content::Inline).with_children());
        assert!(schema.block_spec(block::HEADING).unwrap().allows_children());
    }

    #[test]
    fn inline_math_spec_follows_registration_order() {
        fn plain(_: &Atom) -> String {
            String::new()
        }
        let schema = Schema::builtin()
            .inline(
                InlineSpec::new("first_math", plain)
                    .as_inline_math("tex")
                    .with_hint("first math (`$...$`)"),
            )
            .inline(InlineSpec::new("second_math", plain).as_inline_math("src"))
            .inline(
                InlineSpec::new("first_math", plain)
                    .as_inline_math("latex")
                    .with_hint("updated math (`$...$`)"),
            );
        let names: Vec<_> = schema.inline_specs().map(InlineSpec::name).collect();
        assert_eq!(names, ["first_math", "second_math"]);
        let (spec, attr) = schema.inline_math_spec().unwrap();
        assert_eq!(spec.name(), "first_math");
        assert_eq!(attr, "latex");
        assert_eq!(spec.hint(), Some("updated math (`$...$`)"));
    }
}
