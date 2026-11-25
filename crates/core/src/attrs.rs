//! The attributes of the built-in blocks, and their values as a view reads
//! them, so that no view spells a key or a value out itself.

use crate::schema::MAX_HEADING_LEVEL;
use crate::{BlockId, Document, Value};
use alert::AlertKind;

/// The names of the built-in block types, as [`Schema::builtin`](crate::Schema::builtin)
/// has them. [`BlockKind`](crate::BlockKind) names them with the attributes
/// that make each what it is.
pub mod block {
    /// Text, where typing starts: the built-in schema's text block.
    pub const PARAGRAPH: &str = "paragraph";
    /// A heading, of a level in [`attr::LEVEL`](crate::attr::LEVEL).
    pub const HEADING: &str = "heading";
    /// A quote, its own text and the blocks in it.
    pub const QUOTE: &str = "quote";
    /// One of GitHub's alerts, of a kind in [`attr::KIND`](crate::attr::KIND).
    pub const ALERT: &str = "alert";
    /// An item of a bulleted list.
    pub const BULLET_LIST_ITEM: &str = "bullet_list_item";
    /// An item of a numbered list, the first with a start in
    /// [`attr::START`](crate::attr::START) where it is no 1.
    pub const NUMBERED_LIST_ITEM: &str = "numbered_list_item";
    /// An item of a check list, checked in [`attr::CHECKED`](crate::attr::CHECKED).
    pub const CHECK_LIST_ITEM: &str = "check_list_item";
    /// A list item whose blocks a view shows or hides.
    pub const TOGGLE_LIST_ITEM: &str = "toggle_list_item";
    /// Code, of a language in [`attr::LANGUAGE`](crate::attr::LANGUAGE).
    pub const CODE_BLOCK: &str = "code_block";
    /// A horizontal rule.
    pub const DIVIDER: &str = "divider";
    /// Where printing and export start a new page.
    pub const PAGE_BREAK: &str = "page_break";
    /// An image at [`attr::URL`](crate::attr::URL), with a caption.
    pub const IMAGE: &str = "image";
    /// A file at [`attr::URL`](crate::attr::URL), with a name.
    pub const FILE: &str = "file";
    /// A table: its children are its rows, and it lists its columns.
    pub const TABLE: &str = "table";
    /// A row of a table.
    pub const TABLE_ROW: &str = "table_row";
    /// A cell of a table: a row and a column, no block of its own.
    pub const TABLE_CELL: &str = "table_cell";
}

/// The names of block attributes the built-in blocks carry.
pub mod attr {
    /// A heading's level, 1 to [`MAX_HEADING_LEVEL`](crate::MAX_HEADING_LEVEL).
    pub const LEVEL: &str = "level";
    /// Whether a check list item is checked.
    pub const CHECKED: &str = "checked";
    /// The number a numbered list starts at, on its first item.
    pub const START: &str = "start";
    /// An alert's kind, the name of an [`AlertKind`](crate::AlertKind).
    pub const KIND: &str = "kind";
    /// A code block's language.
    pub const LANGUAGE: &str = "language";
    /// The address of an image or a file.
    pub const URL: &str = "url";
    /// An image's caption.
    pub const CAPTION: &str = "caption";
    /// A file's name.
    pub const NAME: &str = "name";
    /// A block's text color, one of the named colors.
    pub const TEXT_COLOR: &str = "text_color";
    /// A block's background color, one of the named colors.
    pub const BACKGROUND_COLOR: &str = "background_color";
    /// Where a block's lines go: `center` or `right`, or left without it.
    pub const TEXT_ALIGNMENT: &str = "text_alignment";
}

/// The kinds of an alert, and how GitHub draws them, as GitHub Flavored
/// Markdown's stylesheet has them.
pub mod alert {
    /// The kind of an alert, as GitHub Flavored Markdown has them.
    #[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
    pub enum AlertKind {
        /// Information the reader should notice. A kind no one knows reads
        /// as this.
        #[default]
        Note,
        /// Advice to do something better or more easily.
        Tip,
        /// Information the reader needs to succeed.
        Important,
        /// Something that needs attention now, to avoid a problem.
        Warning,
        /// A risk or a bad outcome of an action.
        Caution,
    }

    /// All of them, the note first.
    pub const KINDS: [AlertKind; 5] = [
        AlertKind::Note,
        AlertKind::Tip,
        AlertKind::Important,
        AlertKind::Warning,
        AlertKind::Caution,
    ];

    impl AlertKind {
        /// The name it is stored under, as `tip`.
        pub fn name(self) -> &'static str {
            self.style().name
        }

        /// The kind `name` names, in any case, as `TIP` or `tip`.
        pub fn parse(name: &str) -> Option<Self> {
            let style = STYLES
                .iter()
                .find(|style| style.name.eq_ignore_ascii_case(name));
            style.map(|style| style.kind)
        }

        /// How GitHub draws an alert of the kind.
        pub fn style(self) -> &'static Style {
            &STYLES[self as usize]
        }
    }

    /// A color GitHub draws alerts in: a Primer token, with its value in
    /// the light and in the dark color scheme, as hex.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct Color {
        /// The Primer token's name, as GitHub's stylesheet has it.
        pub token: &'static str,
        /// Its value in the light color scheme, as `#rrggbb`.
        pub light: &'static str,
        /// Its value in the dark color scheme, as `#rrggbb`.
        pub dark: &'static str,
    }

    /// How GitHub draws an alert of a kind: its title, the color of the
    /// bar on its left, and that of its title and icon.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct Style {
        /// The kind.
        pub kind: AlertKind,
        /// The name the kind is stored under.
        pub name: &'static str,
        /// The title GitHub gives an alert of the kind.
        pub title: &'static str,
        /// The color of the bar on the alert's left.
        pub border: Color,
        /// The color of the alert's title and icon.
        pub color: Color,
    }

    const fn color(token: &'static str, light: &'static str, dark: &'static str) -> Color {
        Color { token, light, dark }
    }

    /// GitHub's styles of the kinds, in the order of [`KINDS`], from
    /// `github-markdown-css`.
    pub const STYLES: [Style; 5] = [
        Style {
            kind: AlertKind::Note,
            name: "note",
            title: "Note",
            border: color("--borderColor-accent-emphasis", "#0969da", "#1f6feb"),
            color: color("--fgColor-accent", "#0969da", "#4493f8"),
        },
        Style {
            kind: AlertKind::Tip,
            name: "tip",
            title: "Tip",
            border: color("--borderColor-success-emphasis", "#1a7f37", "#238636"),
            color: color("--fgColor-success", "#1a7f37", "#3fb950"),
        },
        Style {
            kind: AlertKind::Important,
            name: "important",
            title: "Important",
            border: color("--borderColor-done-emphasis", "#8250df", "#8957e5"),
            color: color("--fgColor-done", "#8250df", "#ab7df8"),
        },
        Style {
            kind: AlertKind::Warning,
            name: "warning",
            title: "Warning",
            border: color("--borderColor-attention-emphasis", "#9a6700", "#9e6a03"),
            color: color("--fgColor-attention", "#9a6700", "#d29922"),
        },
        Style {
            kind: AlertKind::Caution,
            name: "caution",
            title: "Caution",
            border: color("--borderColor-danger-emphasis", "#cf222e", "#da3633"),
            color: color("--fgColor-danger", "#d1242f", "#f85149"),
        },
    ];
}

/// The largest number a numbered list item shows, CommonMark's.
pub(crate) const LARGEST_LIST_NUMBER: i64 = 999_999_999;

/// The named colors of a block and of text, BlockNote's, so that documents
/// written with it look the same. A view picks how each looks.
pub const COLOR_NAMES: [&str; 9] = [
    "gray", "brown", "red", "orange", "yellow", "green", "blue", "purple", "pink",
];

/// The names of the built-in marks.
pub mod mark {
    /// Bold text, on or off.
    pub const BOLD: &str = "bold";
    /// Italic text, on or off.
    pub const ITALIC: &str = "italic";
    /// Underlined text, on or off.
    pub const UNDERLINE: &str = "underline";
    /// Struck-through text, on or off.
    pub const STRIKE: &str = "strike";
    /// Inline code: text as typed, as in a code block.
    pub const CODE: &str = "code";
    /// A link, its address the value.
    pub const LINK: &str = "link";
    /// The text's color, one of the named colors, or a hex color a peer wrote.
    pub const TEXT_COLOR: &str = "text_color";
    /// The text's background color, as [`TEXT_COLOR`].
    pub const BACKGROUND_COLOR: &str = "background_color";
}

/// A block's own style, which goes with it whatever its type becomes, where
/// that type takes a style.
pub(crate) const BLOCK_STYLE: [&str; 3] = [
    attr::TEXT_COLOR,
    attr::BACKGROUND_COLOR,
    attr::TEXT_ALIGNMENT,
];

/// Where the lines of a block go.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Align {
    #[default]
    /// Lines start at the left edge, where they go anyway.
    Left,
    /// Lines are centred.
    Center,
    /// Lines end at the right edge.
    Right,
}

impl Align {
    /// The value stored, none for the left, which is where lines go anyway.
    pub(crate) fn value(self) -> Option<Value> {
        match self {
            Self::Left => None,
            Self::Center => Some("center".into()),
            Self::Right => Some("right".into()),
        }
    }
}

/// Which of a block's two colors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockColor {
    /// The color of the block's text.
    Text,
    /// The color behind the block's text.
    Background,
}

impl BlockColor {
    /// The attribute it is stored under.
    pub fn key(self) -> &'static str {
        match self {
            Self::Text => attr::TEXT_COLOR,
            Self::Background => attr::BACKGROUND_COLOR,
        }
    }
}

impl Document {
    fn text_attr(&self, block: BlockId, key: &str) -> Option<String> {
        match self.attr(block, key) {
            Ok(Some(Value::Str(text))) => Some(text),
            _ => None,
        }
    }

    /// A heading's level, read within 1 to [`MAX_HEADING_LEVEL`] whatever a
    /// peer wrote, and 1 where none is.
    pub fn heading_level(&self, block: BlockId) -> i64 {
        match self.attr(block, attr::LEVEL) {
            Ok(Some(Value::Int(level))) => level.clamp(1, MAX_HEADING_LEVEL),
            _ => 1,
        }
    }

    /// Whether a check list item is checked.
    pub fn is_checked(&self, block: BlockId) -> bool {
        matches!(self.attr(block, attr::CHECKED), Ok(Some(Value::Bool(true))))
    }

    /// Where a block's lines go, left for a type that takes no style.
    pub fn alignment(&self, block: BlockId) -> Align {
        if !self.spec(block).is_ok_and(|spec| spec.takes_style()) {
            return Align::Left;
        }
        match self.text_attr(block, attr::TEXT_ALIGNMENT).as_deref() {
            Some("center") => Align::Center,
            Some("right") => Align::Right,
            _ => Align::Left,
        }
    }

    /// The name of one of a block's colors, none for a type that takes no
    /// style.
    pub fn block_color(&self, block: BlockId, which: BlockColor) -> Option<String> {
        let styled = self.spec(block).is_ok_and(|spec| spec.takes_style());
        styled.then(|| self.text_attr(block, which.key()))?
    }

    /// An alert's kind, a note for a kind no one knows.
    pub fn alert_kind(&self, block: BlockId) -> AlertKind {
        let kind = self.text_attr(block, attr::KIND).unwrap_or_default();
        AlertKind::parse(&kind).unwrap_or_default()
    }

    /// The number a numbered list item shows, given `before`, that of the
    /// sibling right before it where that is one too: one more than that,
    /// or else the number its list starts at, [`attr::START`] or 1, within
    /// 0 to CommonMark's largest. `None` for a block that is no numbered
    /// list item.
    pub fn list_number(&self, block: BlockId, before: Option<i64>) -> Option<i64> {
        if self.block_type(block).ok()? != block::NUMBERED_LIST_ITEM {
            return None;
        }
        Some(match (before, self.attr(block, attr::START)) {
            (Some(before), _) => before.saturating_add(1).min(LARGEST_LIST_NUMBER),
            (None, Ok(Some(Value::Int(start)))) => start.clamp(0, LARGEST_LIST_NUMBER),
            (None, _) => 1,
        })
    }

    /// An attribute that is text, as an image's address or a file's name,
    /// or the empty text.
    pub fn text_of(&self, block: BlockId, key: &str) -> String {
        self.text_attr(block, key).unwrap_or_default()
    }
}
