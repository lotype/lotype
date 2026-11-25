use super::PAGE_BREAK;
use crate::{BlockId, Document, MAX_HEADING_LEVEL, Span, Value, attr, block};

/// A block with what the line before it, its first line and its later lines
/// start with.
type Item = (BlockId, String, String, String);

/// The largest number CommonMark reads at the start of a list item.
const LARGEST_NUMBER: i64 = 999_999_999;

impl Document {
    /// The document as Markdown: CommonMark with GitHub's alerts, task lists
    /// and strikethrough.
    ///
    /// Markdown cannot say everything a document holds. Underline and colors
    /// are left off, a toggle list item reads as a bullet, a file as a link,
    /// and blocks nested in a block that is no list item, quote or alert
    /// follow it instead. Space at the ends of a block's text, empty
    /// paragraphs, and the difference between a list item without text and
    /// one whose text is its first paragraph are not kept either.
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

    /// The children of `parent`, each with its list marker or quote sign.
    fn items(&self, parent: Option<BlockId>, prefix: &str) -> Vec<Item> {
        let mut number = None;
        let children = self.children(parent).unwrap_or_default();
        let items = children.into_iter().map(|block| {
            let block_type = self.block_type(block).unwrap_or_default();
            let numbered = block_type == block::NUMBERED_LIST_ITEM;
            number = numbered.then(|| match (number, self.attr(block, attr::START)) {
                (Some(before), _) => LARGEST_NUMBER.min(before + 1),
                (None, Ok(Some(Value::Int(start)))) => start.clamp(0, LARGEST_NUMBER),
                (None, _) => 1,
            });
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
            block::ALERT => format!("[!{}]", self.alert_kind(block).to_uppercase()),
            block::IMAGE => format!(
                "![{}]({})",
                escape(&attr(attr::CAPTION)),
                address(&attr(attr::URL))
            ),
            block::FILE => {
                let (name, url) = (attr(attr::NAME), attr(attr::URL));
                let shown = if name.is_empty() { &url } else { &name };
                format!("[{}]({})", escape(shown), address(&url))
            }
            block::CODE_BLOCK => {
                let code = self.plain_text(block).ok()?;
                let fence = "`".repeat(longest_run(&code, '`').max(2) + 1);
                let language =
                    attr(attr::LANGUAGE).replace(|c: char| c == '`' || c.is_whitespace(), "");
                format!("{fence}{language}\n{code}\n{fence}")
            }
            block::TABLE => {
                let (rows, columns) = (self.children(Some(block)).ok()?, self.columns(block).ok()?);
                let line = |row: &BlockId| {
                    let cells = columns.iter().map(|column| {
                        let cell = self.cell(*row, *column);
                        let spans = cell
                            .and_then(|cell| self.plain_spans(cell))
                            .unwrap_or_default();
                        // A cell is one line, and a bar ends it even in code.
                        inline(&spans).replace("\\\n", " ").replace('|', "\\|")
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
                let mut title = inline(&self.plain_spans(block).ok()?).replace("\\\n", " ");
                // Signs at the end of a heading would read as part of its frame.
                if let Some(last) = title.rfind('#').filter(|at| at + 1 == title.len()) {
                    title.insert(last, '\\');
                }
                format!("{} {title}", "#".repeat(level))
            }
            _ => {
                // A block of a type unknown here still has its text.
                let lines = match self.plain_spans(block) {
                    Ok(spans) => inline(&spans),
                    Err(_) => escape(self.raw_text(block).ok()?.to_string().trim()),
                };
                let lines: Vec<_> = lines.split('\n').map(line_start).collect();
                lines.join("\n")
            }
        })
    }
}

fn longest_run(text: &str, of: char) -> usize {
    let runs = text.split(|c| c != of);
    runs.map(str::len).max().unwrap_or(0)
}

/// A mark that is open, with what tells it from another and what closes it.
struct Open {
    key: String,
    close: String,
}

/// Rich text as Markdown. A line break in it is a hard break.
fn inline(spans: &[Span]) -> String {
    let mut out = String::new();
    let mut open: Vec<Open> = Vec::new();
    // Space between two spans goes outside the marks that end or start there.
    let mut space = String::new();
    // A sign only ends emphasis before a character that is no part of a
    // word. Where one follows, it is written as a character reference.
    let mut guard = false;
    // The sign that ended emphasis last, with nothing written since. A sign
    // that starts emphasis right there must be the other one, or the two
    // would read as one run.
    let mut closed = None;
    // The sign that ended emphasis inside emphasis still open. One that
    // starts emphasis again in there must be the other one too: with text
    // between, a lone sign there can both open and close, and Markdown
    // pairs it with the wrong one.
    let mut inner = None;
    // Where in `out` emphasis signs start, and where the text after them does.
    let mut started: Vec<(usize, usize)> = Vec::new();
    for span in spans {
        let core = span.text.trim();
        if core.is_empty() {
            space.push_str(&span.text);
            continue;
        }
        let lead = span.text.len() - span.text.trim_start().len();
        let on = |name: &str| {
            let value = span.marks.get(name);
            value.is_some_and(|value| *value != Value::Bool(false))
        };
        let mut wanted = Vec::new();
        if let Some(Value::Str(url)) = span.marks.get("link") {
            wanted.push(format!("link {url}"));
        }
        let names = ["bold", "italic", "strike"];
        wanted.extend(names.into_iter().filter(|name| on(name)).map(str::to_owned));

        let kept = open.iter().zip(&wanted);
        let kept = kept
            .take_while(|(open, wanted)| open.key == **wanted)
            .count();
        while open.len() > kept {
            let Some(mark) = open.pop() else { break };
            let sign = mark.close.chars().next();
            if matches!(sign, Some('*' | '_' | '~')) {
                let inside = out.chars().next_back();
                guard = sign == Some('_') || inside.is_none_or(|c| !c.is_alphanumeric());
                closed = sign.filter(|sign| *sign != '~');
                if open.iter().any(|outer| outer.close.starts_with(['*', '_'])) {
                    inner = inner.or(closed);
                }
            } else {
                (guard, closed) = (false, None);
            }
            out.push_str(&mark.close);
        }
        if open.is_empty() {
            inner = None;
        }

        space.push_str(&span.text[..lead]);
        // A line cannot start with a break, nor can the text.
        if out.is_empty() && space.contains(['\n', '\r']) {
            space.clear();
        }
        if !space.is_empty() {
            out.push_str(&escape(&space));
            space.clear();
            (guard, closed) = (false, None);
        }

        let text = if on("code") { code(core) } else { escape(core) };
        let underscores = closed == Some('*') || inner == Some('*');
        let new = &wanted[kept..];
        let emphasis = new.first().is_some_and(|first| !first.starts_with("link "));
        if emphasis {
            // A sign only starts emphasis after a character that is no part
            // of a word, unless a word follows it. An underscore never
            // starts emphasis inside a word.
            let after = if new.len() > 1 {
                None
            } else {
                text.chars().next()
            };
            if underscores || !after.is_some_and(char::is_alphanumeric) {
                guard_before(&mut out, &mut started);
            }
        }
        let start = out.len();
        for key in new {
            let (sign, close) = match (key.as_str(), underscores) {
                ("bold", false) => ("**", "**".to_owned()),
                ("bold", true) => ("__", "__".to_owned()),
                ("italic", false) => ("*", "*".to_owned()),
                ("italic", true) => ("_", "_".to_owned()),
                ("strike", _) => ("~~", "~~".to_owned()),
                (link, _) => ("[", format!("]({})", address(&link["link ".len()..]))),
            };
            out.push_str(sign);
            open.push(Open {
                key: key.clone(),
                close,
            });
            guard = false;
        }
        if emphasis {
            started.push((start, out.len()));
        }
        match text.chars().next().filter(|c| guard && part_of_word(*c)) {
            Some(first) => {
                out.push_str(&reference(first));
                out.push_str(&text[first.len_utf8()..]);
            }
            None => out.push_str(&text),
        }
        (guard, closed) = (false, None);
        space.push_str(&span.text[lead + core.len()..]);
    }
    while let Some(mark) = open.pop() {
        out.push_str(&mark.close);
    }
    out
}

/// Writes the last character of `out` as a reference, if it can count as
/// part of a word, so that a sign after it can start emphasis. The signs
/// right before that character then have a sign after them, and need the
/// same of the character before them.
fn guard_before(out: &mut String, started: &mut [(usize, usize)]) {
    let mut end = out.len();
    while let Some(c) = out[..end].chars().next_back().filter(|c| part_of_word(*c)) {
        let (at, written) = (end - c.len_utf8(), reference(c));
        let grown = written.len() - c.len_utf8();
        out.replace_range(at..end, &written);
        for (start, text) in started.iter_mut().filter(|(start, _)| *start >= end) {
            (*start, *text) = (*start + grown, *text + grown);
        }
        match started.iter().find(|(_, text)| *text == at) {
            Some((start, _)) => end = *start,
            None => return,
        }
    }
}

/// Whether a character next to an emphasis sign can count as part of a word.
/// Only space and ASCII signs surely do not.
fn part_of_word(c: char) -> bool {
    !c.is_whitespace() && !c.is_ascii_punctuation()
}

fn reference(c: char) -> String {
    format!("&#{};", c as u32)
}

/// Code, a span a line: a code span reads a line break in it as a space,
/// so the breaks go between the spans.
fn code(text: &str) -> String {
    let span = |line: &str| {
        if line.is_empty() {
            return String::new();
        }
        let fence = "`".repeat(longest_run(line, '`') + 1);
        let pad = if line.starts_with('`') || line.ends_with('`') {
            " "
        } else {
            ""
        };
        format!("{fence}{pad}{line}{pad}{fence}")
    };
    let lines: Vec<_> = text.split(['\n', '\r']).map(span).collect();
    lines.join("\\\n")
}

/// Text with every sign Markdown would read as markup escaped.
fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '\\' | '`' | '*' | '_' | '[' | ']' | '<' | '&' | '~' | '$' => {
                out.push('\\');
                out.push(c);
            }
            '\n' => out.push_str("\\\n"),
            // Markdown ends a line at a carriage return as well.
            '\r' => out.push_str("&#13;"),
            _ => out.push(c),
        }
    }
    out
}

/// A line of a paragraph, kept from reading as a heading, a list item, a
/// quote, a rule or code.
fn line_start(line: &str) -> String {
    let text = line.trim_start_matches([' ', '\t']);
    // Space written as character references is no indentation.
    let indent = line[..line.len() - text.len()].chars();
    let mut out: String = indent.map(reference).collect();
    let digits = text.len() - text.trim_start_matches(|c: char| c.is_ascii_digit()).len();
    let numbered = digits > 0 && text[digits..].starts_with(['.', ')']);
    // A line of bars and dashes under another would make the two a table.
    let ruled = text.contains('-') && text.chars().all(|c| "|:- \t".contains(c));
    if ruled || text.starts_with(['#', '>', '-', '+', '=']) {
        out.push('\\');
        out.push_str(text);
    } else if numbered {
        out.push_str(&text[..digits]);
        out.push('\\');
        out.push_str(&text[digits..]);
    } else {
        out.push_str(text);
    }
    out
}

/// A link's address, in angle brackets when it would not read as one otherwise.
fn address(url: &str) -> String {
    let url = url.replace('\\', "\\\\").replace('&', "&amp;");
    let plain = !url.is_empty() && !url.contains(|c: char| c.is_whitespace() || "()<>".contains(c));
    if plain {
        return url;
    }
    let url = url.replace('<', "%3C").replace('>', "%3E");
    format!("<{}>", url.replace(['\n', '\r'], ""))
}
