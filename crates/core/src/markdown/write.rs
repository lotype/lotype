mod math;
mod text;

use super::{FILE_TITLE, PAGE_BREAK, UNNAMED_FILE_TITLE};
use crate::{BlockId, Document, MAX_HEADING_LEVEL, Value, attr, block};
use math::is_display_math_markdown_safe;
use text::{address, escape};

pub(super) use text::{inline, line_start, longest_run};

/// A block with what the line before it, its first line and its later lines
/// start with.
type Item = (BlockId, String, String, String);

impl Document {
    /// The document as Markdown: CommonMark with GitHub's alerts, task lists
    /// and strikethrough.
    ///
    /// Markdown cannot say everything a document holds. Underline and colors
    /// are left off, a toggle list item reads as a bullet, and
    /// blocks nested in a block that is no list item, quote or alert
    /// follow it instead. Space at the ends of a block's text, empty
    /// paragraphs, and the difference between a list item without text and
    /// one whose text is its first paragraph are not kept either.
    /// A file is a link with a title that Lotype uses to restore its block type.
    pub fn to_markdown(&self) -> String {
        let mut out = String::new();
        let mut stack = self.items(None, "");
        stack.reverse();
        // The block that follows a marker written on a line of its own.
        let mut under = None;
        while let Some((block, gap, first, rest)) = stack.pop() {
            let body = self.body(block).unwrap_or_default();
            let mut children = self.items(Some(block), &rest);
            let marker = first != rest;
            // A task's box is only read as one when it ends its line or text
            // follows it, so it keeps a line to itself where a block follows.
            // So does a dash: `- ---` or `- - -` would read as a divider.
            let boxed = first.ends_with("] ") || first.ends_with("- ");
            if !body.is_empty() || marker && (boxed || children.is_empty()) {
                if !out.is_empty() && under != Some(block) {
                    // Stays inside the quote the block is in, if any.
                    out.push_str(gap.trim_end());
                    out.push('\n');
                }
                for (number, line) in body.split('\n').enumerate() {
                    let start = if number == 0 { &first } else { &rest };
                    out.push_str(if line.is_empty() {
                        start.trim_end()
                    } else {
                        start
                    });
                    out.push_str(line);
                    out.push('\n');
                }
                // No blank line may come between a marker and what is in its item.
                under = children
                    .first()
                    .filter(|_| body.is_empty())
                    .map(|child| child.0);
            } else if marker && let Some(child) = children.first_mut() {
                // A list item may not start with a blank line, so what is in
                // one without text starts on the line of its marker.
                child.2 = format!("{first}{}", &child.2[rest.len()..]);
                child.1 = gap;
            }
            stack.extend(children.into_iter().rev());
        }
        out
    }

    /// Every block in reading order, with how deep it is nested and its own
    /// Markdown: its lines of [`Document::to_markdown`] without what is
    /// nested in it. A table is one block here, with its rows.
    pub fn markdown_blocks(&self) -> Vec<(BlockId, usize, String)> {
        let mut out = Vec::new();
        let level = |parent, depth| {
            let items = self.items(parent, "").into_iter().rev();
            items.map(move |item| (item, depth))
        };
        let mut stack: Vec<(Item, usize)> = level(None, 0).collect();
        while let Some(((block, _, first, rest), depth)) = stack.pop() {
            let body = self.body(block).unwrap_or_default();
            let lines = body.split('\n').enumerate();
            let lines =
                lines.map(|(at, line)| [if at == 0 { &first } else { &rest }, line].concat());
            out.push((block, depth, lines.collect::<Vec<_>>().join("\n")));
            if self
                .block_type(block)
                .is_ok_and(|kind| kind != block::TABLE)
            {
                stack.extend(level(Some(block), depth + 1));
            }
        }
        out
    }

    /// The children of `parent`, each with its list marker or quote sign.
    fn items(&self, parent: Option<BlockId>, prefix: &str) -> Vec<Item> {
        let mut number = None;
        let children = self.children(parent).unwrap_or_default();
        let items = children.into_iter().map(|block| {
            let block_type = self.block_type(block).unwrap_or_default();
            number = self.list_number(block, number);
            let marker = match block_type.as_str() {
                block::BULLET_LIST_ITEM | block::TOGGLE_LIST_ITEM => "- ".to_owned(),
                block::NUMBERED_LIST_ITEM => format!("{}. ", number.unwrap_or(1)),
                block::CHECK_LIST_ITEM => match self.attr(block, attr::CHECKED) {
                    Ok(Some(Value::Bool(true))) => "- [x] ".to_owned(),
                    _ => "- [ ] ".to_owned(),
                },
                block::QUOTE | block::ALERT => {
                    let inside = format!("{prefix}> ");
                    return (block, prefix.to_owned(), inside.clone(), inside);
                }
                _ => String::new(),
            };
            // What is in a task item lines up with its dash, not its box.
            let width = if marker.starts_with("- ") {
                2
            } else {
                marker.len()
            };
            let (first, indent) = (format!("{prefix}{marker}"), " ".repeat(width));
            (block, prefix.to_owned(), first, format!("{prefix}{indent}"))
        });
        items.collect()
    }

    /// The lines of a block itself, without what is nested in it.
    fn body(&self, block: BlockId) -> Option<String> {
        let block_type = self.block_type(block).ok()?;
        let attr = |key| match self.attr(block, key) {
            Ok(Some(Value::Str(text))) => text.replace(['\n', '\r'], " "),
            _ => String::new(),
        };
        Some(match block_type.as_str() {
            block::DIVIDER => "---".to_owned(),
            block::PAGE_BREAK => PAGE_BREAK.to_owned(),
            block::ALERT => format!("[!{}]", self.alert_kind(block).name().to_uppercase()),
            block::IMAGE => format!(
                "![{}]({})",
                escape(&attr(attr::CAPTION)),
                address(&attr(attr::URL))
            ),
            block::FILE => {
                let (name, url) = (attr(attr::NAME), attr(attr::URL));
                let (label, title) = match (name.is_empty(), url.is_empty()) {
                    (false, _) => (name.as_str(), FILE_TITLE),
                    (true, false) => (url.as_str(), UNNAMED_FILE_TITLE),
                    (true, true) => ("File", UNNAMED_FILE_TITLE),
                };
                format!("[{}]({} \"{title}\")", escape(label), address(&url))
            }
            block::CODE_BLOCK => {
                let code = self.plain_text(block).ok()?;
                let fence = "`".repeat(longest_run(&code, '`').max(2) + 1);
                let info = self.code_fence_info(block);
                format!("{fence}{info}\n{code}\n{fence}")
            }
            block::TABLE => {
                let (rows, columns) = (self.children(Some(block)).ok()?, self.columns(block).ok()?);
                let line = |row: &BlockId| {
                    let cells = columns.iter().map(|column| {
                        let cell = self.cell(*row, *column);
                        let spans = cell.and_then(|cell| self.spans(cell)).unwrap_or_default();
                        // A cell is one line, and an unescaped bar ends it even in code.
                        escape_table_pipes(&inline(&spans, self.schema()).replace("\\\n", " "))
                    });
                    format!("| {} |", cells.collect::<Vec<_>>().join(" | "))
                };
                // The first row is the one Markdown wants headings in.
                let (head, body) = rows.split_first().filter(|_| !columns.is_empty())?;
                let mut lines = vec![line(head), format!("|{}", " --- |".repeat(columns.len()))];
                lines.extend(body.iter().map(line));
                lines.join("\n")
            }
            block::HEADING => {
                let level = match self.attr(block, attr::LEVEL) {
                    Ok(Some(Value::Int(level))) => level.clamp(1, MAX_HEADING_LEVEL) as usize,
                    _ => 1,
                };
                let mut title =
                    inline(&self.spans(block).ok()?, self.schema()).replace("\\\n", " ");
                // Signs at the end of a heading would read as part of its frame.
                if let Some(last) = title.rfind('#').filter(|at| at + 1 == title.len()) {
                    title.insert(last, '\\');
                }
                format!("{} {title}", "#".repeat(level))
            }
            _ => {
                if let Ok(spec) = self.spec(block)
                    && spec.is_display_math()
                {
                    let tex = self.plain_text(block).ok()?;
                    if tex.is_empty() {
                        if spec.fence().is_none() {
                            return Some("$$\n$$".to_owned());
                        }
                    } else if is_display_math_markdown_safe(&tex) {
                        return Some(format!("$$\n{tex}\n$$"));
                    }
                }
                match self.fence(block) {
                    Some(fence) => fence,
                    None => {
                        // A block of a type unknown here still has its text.
                        let lines = match self.spans(block) {
                            Ok(spans) => inline(&spans, self.schema()),
                            Err(_) => escape(self.raw_text(block).ok()?.to_string().trim()),
                        };
                        let lines: Vec<_> = lines.split('\n').map(line_start).collect();
                        lines.join("\n")
                    }
                }
            }
        })
    }
}

/// Escapes any `|` in a GFM table cell that is not already preceded by an odd
/// number of backslashes (so LaTeX norms like `$\|x\|$` do not become `$\\|x\\|$`
/// and split the table cell).
fn escape_table_pipes(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut backslashes = 0usize;
    for c in text.chars() {
        match c {
            '\\' => {
                backslashes += 1;
                out.push(c);
            }
            '|' => {
                if backslashes.is_multiple_of(2) {
                    out.push('\\');
                }
                out.push('|');
                backslashes = 0;
            }
            _ => {
                backslashes = 0;
                out.push(c);
            }
        }
    }
    out
}
