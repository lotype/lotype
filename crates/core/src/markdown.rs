//! Markdown in and out: CommonMark with GitHub's alerts, task lists and
//! strikethrough.

mod fence;
mod read;
mod write;

pub(crate) use fence::normalize_line_ranges;

/// What a page break is written as, since Markdown has none.
const PAGE_BREAK: &str = "<!-- page-break -->";

/// Link titles that preserve a standalone file block in CommonMark.
const FILE_TITLE: &str = "lotype-file";
const UNNAMED_FILE_TITLE: &str = "lotype-file-unnamed";
