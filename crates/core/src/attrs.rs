//! The attributes of the built-in blocks, and their values as a view reads
//! them, so that no view spells a key or a value out itself.

use crate::schema::MAX_HEADING_LEVEL;
use crate::{BlockId, Document, Value};

/// The names of block attributes the built-in blocks carry.
pub mod attr {
    /// A heading's level, 1 to [`MAX_HEADING_LEVEL`](crate::MAX_HEADING_LEVEL).
    pub const LEVEL: &str = "level";
    /// Whether a check list item is checked.
    pub const CHECKED: &str = "checked";
    /// The number a numbered list starts at, on its first item.
    pub const START: &str = "start";
    /// An alert's kind, one of [`alert::KINDS`](crate::alert::KINDS).
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
    pub const NOTE: &str = "note";
    pub const TIP: &str = "tip";
    pub const IMPORTANT: &str = "important";
    pub const WARNING: &str = "warning";
    pub const CAUTION: &str = "caution";
    /// All of them, the note first, which a kind no one knows is read as.
    pub const KINDS: [&str; 5] = [NOTE, TIP, IMPORTANT, WARNING, CAUTION];

    /// A color GitHub draws alerts in: a Primer token, with its value in
    /// the light and in the dark color scheme, as hex.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct Color {
        pub token: &'static str,
        pub light: &'static str,
        pub dark: &'static str,
    }

    /// How GitHub draws an alert of a kind: its title, the color of the
    /// bar on its left, and that of its title and icon.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct Style {
        pub kind: &'static str,
        pub title: &'static str,
        pub border: Color,
        pub color: Color,
    }

    const fn color(token: &'static str, light: &'static str, dark: &'static str) -> Color {
        Color { token, light, dark }
    }

    /// GitHub's styles of the kinds, in the order of [`KINDS`], from
    /// `github-markdown-css`.
    pub const STYLES: [Style; 5] = [
        Style {
            kind: NOTE,
            title: "Note",
            border: color("--borderColor-accent-emphasis", "#0969da", "#1f6feb"),
            color: color("--fgColor-accent", "#0969da", "#4493f8"),
        },
        Style {
            kind: TIP,
            title: "Tip",
            border: color("--borderColor-success-emphasis", "#1a7f37", "#238636"),
            color: color("--fgColor-success", "#1a7f37", "#3fb950"),
        },
        Style {
            kind: IMPORTANT,
            title: "Important",
            border: color("--borderColor-done-emphasis", "#8250df", "#8957e5"),
            color: color("--fgColor-done", "#8250df", "#ab7df8"),
        },
        Style {
            kind: WARNING,
            title: "Warning",
            border: color("--borderColor-attention-emphasis", "#9a6700", "#9e6a03"),
            color: color("--fgColor-attention", "#9a6700", "#d29922"),
        },
        Style {
            kind: CAUTION,
            title: "Caution",
            border: color("--borderColor-danger-emphasis", "#cf222e", "#da3633"),
            color: color("--fgColor-danger", "#d1242f", "#f85149"),
        },
    ];

    /// The style of an alert of `kind`, a note's for a kind no one knows.
    pub fn style(kind: &str) -> &'static Style {
        let known = STYLES
            .iter()
            .find(|style| style.kind.eq_ignore_ascii_case(kind));
        known.unwrap_or(&STYLES[0])
    }
}

/// The named colors of a block and of text, BlockNote's, so that documents
/// written with it look the same. A view picks how each looks.
pub const COLOR_NAMES: [&str; 9] = [
    "gray", "brown", "red", "orange", "yellow", "green", "blue", "purple", "pink",
];

/// The names of the built-in marks.
pub mod mark {
    pub const BOLD: &str = "bold";
    pub const ITALIC: &str = "italic";
    pub const UNDERLINE: &str = "underline";
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
    Left,
    Center,
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
    Text,
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
    pub fn alert_kind(&self, block: BlockId) -> &'static str {
        let kind = self.text_attr(block, attr::KIND).unwrap_or_default();
        alert::style(&kind).kind
    }

    /// An attribute that is text, as an image's address or a file's name,
    /// or the empty text.
    pub fn text_of(&self, block: BlockId, key: &str) -> String {
        self.text_attr(block, key).unwrap_or_default()
    }
}
