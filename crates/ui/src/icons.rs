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
    SPARKLES = "sparkles",
    RETURN = "corner-down-left",
    CHEVRON_DOWN = "chevron-down",
    CHECK = "check",
    ALERT = "info",
    TIP = "lightbulb",
    IMPORTANT = "message-square-warning",
    WARNING = "triangle-alert",
    CAUTION = "octagon-alert",
    DIVIDER = "minus",
    PAGE_BREAK = "scissors-line-dashed",
    TABLE = "table",
    IMAGE = "image",
    FILE = "file",
    PLUS = "plus",
    GRIP = "grip-vertical",
}

/// An icon drawn from one of the SVGs above, in the text's color.
pub(crate) fn icon(svg: &[u8]) -> Icon {
    Icon::default().data(svg)
}

/// The icon of a built-in slash menu or block type entry, by its key.
pub(crate) fn entry(key: &str) -> Option<&'static [u8]> {
    Some(match key {
        "paragraph" => PARAGRAPH,
        "heading_1" => HEADING_1,
        "heading_2" => HEADING_2,
        "heading_3" => HEADING_3,
        "heading_4" => HEADING_4,
        "heading_5" => HEADING_5,
        "quote" => QUOTE,
        "toggle_list" => TOGGLE,
        "bullet_list" => BULLET,
        "numbered_list" => NUMBERED,
        "check_list" => CHECK_LIST,
        "code_block" => CODE,
        "alert" => ALERT,
        "divider" => DIVIDER,
        "page_break" => PAGE_BREAK,
        "table" => TABLE,
        "image" => IMAGE,
        "file" => FILE,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn every_built_in_entry_of_the_core_has_an_icon() {
        let items = lotype_core::default_slash_items();
        for item in items.iter().chain(&lotype_core::default_block_types()) {
            assert!(super::entry(&item.key).is_some(), "{}", item.key);
        }
    }
}
