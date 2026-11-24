//! Lucide icons (ISC, `assets/icons/LICENSE-LUCIDE`), built into the crate so
//! that an application need not give GPUI an asset source for them.

use gpui_component::Icon;

macro_rules! icons {
    ($($name:ident = $file:literal),* $(,)?) => {
        $(pub(crate) const $name: &[u8] = include_bytes!(concat!("../assets/icons/", $file, ".svg"));)*
    };
}

icons! {
    PARAGRAPH = "type",
    HEADING_1 = "heading-1",
    HEADING_2 = "heading-2",
    HEADING_3 = "heading-3",
    HEADING_4 = "heading-4",
    HEADING_5 = "heading-5",
    QUOTE = "text-quote",
    TOGGLE = "list-collapse",
    BULLET = "list",
    NUMBERED = "list-ordered",
    CHECK_LIST = "list-todo",
    BOLD = "bold",
    ITALIC = "italic",
    UNDERLINE = "underline",
    STRIKE = "strikethrough",
    CODE = "code",
    ALIGN_LEFT = "text-align-start",
    ALIGN_CENTER = "text-align-center",
    ALIGN_RIGHT = "text-align-end",
    COLOR = "baseline",
    NEST = "list-indent-increase",
    UNNEST = "list-indent-decrease",
    LINK = "link",
    CHEVRON_DOWN = "chevron-down",
    CHECK = "check",
    ALERT = "info",
    DIVIDER = "minus",
    PAGE_BREAK = "scissors-line-dashed",
    TABLE = "table",
    IMAGE = "image",
    FILE = "file",
}

/// An icon drawn from one of the SVGs above, in the text's color.
pub(crate) fn icon(svg: &[u8]) -> Icon {
    Icon::default().data(svg)
}
