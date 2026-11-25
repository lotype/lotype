//! Markdown in and out: CommonMark with GitHub's alerts, task lists and
//! strikethrough.

mod fence;
mod read;
mod write;

/// What a page break is written as, since Markdown has none.
const PAGE_BREAK: &str = "<!-- page-break -->";
