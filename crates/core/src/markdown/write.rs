use super::PAGE_BREAK;
use crate::{BlockId, Document, Span, Value};

/// A block with what the line before it, its first line and its later lines
/// start with.
type Item = (BlockId, String, String, String);

impl Document {
    /// The document as Markdown: CommonMark with GitHub's alerts, task lists
    /// and strikethrough.
    ///
    /// Markdown cannot say everything a document holds. Underline and colors
    /// are left off, a toggle list item reads as a bullet, and blocks nested
    /// in a block that is no list item, quote or alert follow it instead.
    pub fn to_markdown(&self) -> String {
        let mut out = String::new();
        let mut stack = self.items(None, "");
        stack.reverse();
        while let Some((block, gap, first, rest)) = stack.pop() {
            if let Some(body) = self.body(block) {
                if !out.is_empty() {
                    // Stays inside the quote the block is in, if any.
                    out.push_str(gap.trim_end());
                    out.push('\n');
                }
                for (number, line) in body.split('\n').enumerate() {
                    out.push_str(if number == 0 { &first } else { &rest });
                    out.push_str(line);
                    out.push('\n');
                }
            }
            let children = self.items(Some(block), &rest);
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
            let numbered = block_type == "numbered_list_item";
            number = numbered.then(|| match (number, self.attr(block, "start")) {
                (Some(before), _) => before + 1,
                (None, Ok(Some(Value::Int(start)))) => start,
                (None, _) => 1,
            });
            let marker = match block_type.as_str() {
                "bullet_list_item" | "toggle_list_item" => "- ".to_owned(),
                "numbered_list_item" => format!("{}. ", number.unwrap_or(1)),
                "check_list_item" => match self.attr(block, "checked") {
                    Ok(Some(Value::Bool(true))) => "- [x] ".to_owned(),
                    _ => "- [ ] ".to_owned(),
                },
                "quote" | "alert" => {
                    let inside = format!("{prefix}> ");
                    return (block, prefix.to_owned(), inside.clone(), inside);
                }
                _ => String::new(),
            };
            let (first, indent) = (format!("{prefix}{marker}"), " ".repeat(marker.len()));
            (block, prefix.to_owned(), first, format!("{prefix}{indent}"))
        });
        items.collect()
    }

    /// The lines of a block itself, without what is nested in it.
    fn body(&self, block: BlockId) -> Option<String> {
        let block_type = self.block_type(block).ok()?;
        let text = |key| match self.attr(block, key) {
            Ok(Some(Value::Str(text))) => text,
            _ => String::new(),
        };
        Some(match block_type.as_str() {
            "divider" => "---".to_owned(),
            "page_break" => PAGE_BREAK.to_owned(),
            "alert" => format!("[!{}]", text("kind").to_uppercase()),
            "image" => format!("![{}]({})", escape(&text("caption")), address(&text("url"))),
            "file" => {
                let (name, url) = (text("name"), text("url"));
                let shown = if name.is_empty() { &url } else { &name };
                format!("[{}]({})", escape(shown), address(&url))
            }
            "code_block" => {
                let code = self.text(block).ok()?;
                let fence = "`".repeat(longest_run(&code, '`').max(2) + 1);
                let language =
                    text("language").replace(|c: char| c == '`' || c.is_whitespace(), "");
                format!("{fence}{language}\n{code}\n{fence}")
            }
            "heading" => {
                let level = match self.attr(block, "level") {
                    Ok(Some(Value::Int(level))) => level.clamp(1, 6) as usize,
                    _ => 1,
                };
                let title = inline(&self.spans(block).ok()?).replace("\\\n", " ");
                format!("{} {title}", "#".repeat(level))
            }
            _ => {
                let lines = inline(&self.spans(block).ok()?);
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

/// Rich text as Markdown. A line break in it is a hard break.
fn inline(spans: &[Span]) -> String {
    let mut out = String::new();
    // The marks open now, outermost first, each with what closes it.
    let mut open: Vec<(String, String)> = Vec::new();
    // Space between two spans goes outside the marks that end or start there.
    let mut space = "";
    for span in spans {
        let core = span.text.trim();
        let start = span.text.len() - span.text.trim_start().len();
        let mut wanted = Vec::new();
        if !core.is_empty() {
            if let Some(Value::Str(url)) = span.marks.get("link") {
                wanted.push((format!("]({})", address(url)), "["));
            }
            for (name, sign) in [("bold", "**"), ("italic", "*"), ("strike", "~~")] {
                if span.marks.contains_key(name) {
                    wanted.push((sign.to_owned(), sign));
                }
            }
        }
        let kept = open.iter().zip(&wanted);
        let kept = kept.take_while(|(open, wanted)| open.1 == wanted.0).count();
        while open.len() > kept {
            out.push_str(&open.pop().map(|(_, close)| close).unwrap_or_default());
        }
        out.push_str(space);
        out.push_str(&span.text[..start]);
        for (close, sign) in wanted.into_iter().skip(kept) {
            out.push_str(sign);
            open.push((sign.to_owned(), close));
        }
        if span.marks.contains_key("code") {
            out.push_str(&code(core));
        } else {
            out.push_str(&escape(core));
        }
        space = &span.text[start + core.len()..];
    }
    while let Some((_, close)) = open.pop() {
        out.push_str(&close);
    }
    out.push_str(space);
    out
}

fn code(text: &str) -> String {
    let text = text.replace('\n', " ");
    let fence = "`".repeat(longest_run(&text, '`') + 1);
    let pad = if text.starts_with('`') || text.ends_with('`') {
        " "
    } else {
        ""
    };
    format!("{fence}{pad}{text}{pad}{fence}")
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
            _ => out.push(c),
        }
    }
    out
}

/// A line of a paragraph, kept from reading as a heading, a list item, a
/// quote, a rule or code.
fn line_start(line: &str) -> String {
    let text = line.trim_start_matches(' ');
    // A space written as a character reference is no indentation.
    let mut out = "&#32;".repeat(line.len() - text.len());
    let digits = text.len() - text.trim_start_matches(|c: char| c.is_ascii_digit()).len();
    let numbered = digits > 0 && text[digits..].starts_with(['.', ')']);
    if text.starts_with(['#', '>', '-', '+', '=']) {
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
    let plain = !url.is_empty() && !url.contains(|c: char| c.is_whitespace() || "()<>".contains(c));
    if plain {
        return url.to_owned();
    }
    let url = url.replace('<', "%3C").replace('>', "%3E");
    format!("<{}>", url.replace(['\n', '\r'], ""))
}
