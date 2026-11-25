//! The built-in block types with the attributes that make each what it is,
//! to turn blocks into and to make them without spelling a name or a key.

use crate::attrs::{attr, block};
use crate::schema::MAX_HEADING_LEVEL;
use crate::{AlertKind, BlockId, Command, Document, Editor, Result, Value};

/// A built-in block type, with what makes a block of it what it is.
/// [`BlockKind::of`] gives a block's kind as the document reads it.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum BlockKind {
    /// Text, where typing starts.
    Paragraph,
    /// A heading of a level, 1 to [`MAX_HEADING_LEVEL`]; another is brought
    /// into that range.
    Heading(i64),
    /// A quote, its own text and the blocks in it.
    Quote,
    /// One of GitHub's alerts, of a kind.
    Alert(AlertKind),
    /// An item of a bulleted list.
    BulletListItem,
    /// An item of a numbered list, counted along its siblings.
    NumberedListItem,
    /// An item of a check list.
    CheckListItem {
        /// Whether it is checked.
        checked: bool,
    },
    /// A list item whose blocks a view shows or hides.
    ToggleListItem,
    /// Code, typed as it is.
    CodeBlock {
        /// The language, as Markdown names it after the fence.
        language: Option<String>,
    },
    /// A horizontal rule.
    Divider,
    /// Where printing and export start a new page.
    PageBreak,
    /// An image.
    Image {
        /// Where it is.
        url: String,
        /// What it shows, for a reader who cannot see it.
        caption: String,
    },
    /// A file, as a link to it.
    File {
        /// Where it is.
        url: String,
        /// The name shown for it.
        name: String,
    },
}

impl BlockKind {
    /// The name of the block type.
    pub fn name(&self) -> &'static str {
        match self {
            Self::Paragraph => block::PARAGRAPH,
            Self::Heading(_) => block::HEADING,
            Self::Quote => block::QUOTE,
            Self::Alert(_) => block::ALERT,
            Self::BulletListItem => block::BULLET_LIST_ITEM,
            Self::NumberedListItem => block::NUMBERED_LIST_ITEM,
            Self::CheckListItem { .. } => block::CHECK_LIST_ITEM,
            Self::ToggleListItem => block::TOGGLE_LIST_ITEM,
            Self::CodeBlock { .. } => block::CODE_BLOCK,
            Self::Divider => block::DIVIDER,
            Self::PageBreak => block::PAGE_BREAK,
            Self::Image { .. } => block::IMAGE,
            Self::File { .. } => block::FILE,
        }
    }

    /// The attributes that make a block of the type this one, as
    /// [`Editor::set_block`] and [`Document::set_attr`] take them. Empty
    /// text is no attribute.
    pub fn attrs(&self) -> Vec<(String, Value)> {
        let text = |key: &str, value: &str| {
            (!value.is_empty()).then(|| (key.to_owned(), Value::from(value)))
        };
        match self {
            Self::Heading(level) => {
                vec![(
                    attr::LEVEL.to_owned(),
                    (*level).clamp(1, MAX_HEADING_LEVEL).into(),
                )]
            }
            Self::Alert(kind) => vec![(attr::KIND.to_owned(), kind.name().into())],
            Self::CheckListItem { checked } => vec![(attr::CHECKED.to_owned(), (*checked).into())],
            Self::CodeBlock { language } => language
                .iter()
                .filter_map(|language| text(attr::LANGUAGE, language))
                .collect(),
            Self::Image { url, caption } => [text(attr::URL, url), text(attr::CAPTION, caption)]
                .into_iter()
                .flatten()
                .collect(),
            Self::File { url, name } => [text(attr::URL, url), text(attr::NAME, name)]
                .into_iter()
                .flatten()
                .collect(),
            _ => Vec::new(),
        }
    }

    /// What `block` is, where it is of a built-in type with text or no
    /// blocks of its own to make it from; `None` for a table, a cell or a
    /// type the built-in schema lacks.
    ///
    /// The kind is as the document reads it: a heading's level within its
    /// range, an alert's kind a note where no one knows it, no language for
    /// an empty one. What is no part of a kind is left out, as the number a
    /// numbered list starts at, so a block made of it starts without.
    pub fn of(doc: &Document, block: BlockId) -> Option<Self> {
        let text = |key| doc.text_of(block, key);
        Some(match doc.block_type(block).ok()?.as_str() {
            block::PARAGRAPH => Self::Paragraph,
            block::HEADING => Self::Heading(doc.heading_level(block)),
            block::QUOTE => Self::Quote,
            block::ALERT => Self::Alert(doc.alert_kind(block)),
            block::BULLET_LIST_ITEM => Self::BulletListItem,
            block::NUMBERED_LIST_ITEM => Self::NumberedListItem,
            block::CHECK_LIST_ITEM => Self::CheckListItem {
                checked: doc.is_checked(block),
            },
            block::TOGGLE_LIST_ITEM => Self::ToggleListItem,
            block::CODE_BLOCK => {
                let language = text(attr::LANGUAGE);
                Self::CodeBlock {
                    language: (!language.is_empty()).then_some(language),
                }
            }
            block::DIVIDER => Self::Divider,
            block::PAGE_BREAK => Self::PageBreak,
            block::IMAGE => Self::Image {
                url: text(attr::URL),
                caption: text(attr::CAPTION),
            },
            block::FILE => Self::File {
                url: text(attr::URL),
                name: text(attr::NAME),
            },
            _ => return None,
        })
    }
}

impl Command {
    /// Turns the selected blocks into `kind`, as [`Editor::turn_into`] does.
    pub fn turn_into(kind: &BlockKind) -> Self {
        Self::SetBlock {
            block_type: kind.name().to_owned(),
            attrs: kind.attrs(),
        }
    }
}

impl Editor {
    /// Turns the selected blocks into `kind`, as one undo step, as
    /// [`Editor::set_block`] does with its name and attributes, and with
    /// its limits: a block stays as it is where one of the two types holds
    /// text and the other does not, so text becomes no divider, image, file
    /// or alert; and a block already of the type keeps the attributes the
    /// kind leaves out, so `CodeBlock { language: None }` leaves a code
    /// block its language.
    pub fn turn_into(&mut self, kind: &BlockKind) -> Result<()> {
        self.set_block(kind.name(), &kind.attrs())
    }
}

impl Document {
    /// Inserts a block of `kind` at `index` among the children of
    /// `parent`, as [`Document::insert_block`] does, with its attributes.
    pub fn insert_kind(
        &mut self,
        parent: Option<BlockId>,
        index: usize,
        kind: &BlockKind,
    ) -> Result<BlockId> {
        let made = self.insert_block(parent, index, kind.name())?;
        for (key, value) in kind.attrs() {
            self.set_attr(made, &key, value)?;
        }
        Ok(made)
    }
}
