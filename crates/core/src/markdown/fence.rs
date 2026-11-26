//! Blocks of the types a schema gives a fence: written as a fenced block
//! tagged with the type's tag, as GitHub writes a diagram, with the
//! attributes of its own after the tag and its text inside.
//!
//! ````text
//! ```callout tone=warning title="Read this first"
//! Back up **before** you upgrade.
//! ```
//! ````

use std::collections::BTreeMap;

use super::read::{Node, Reader};
use super::write::{inline, line_start, longest_run};
use crate::{BlockId, Content, Document, Role, Span, Value, attr};

impl Document {
    /// `block` as the fence its type is written as, where it is.
    pub(super) fn fence(&self, block: BlockId) -> Option<String> {
        let spec = self.spec(block).ok()?;
        let mut info = spec.fence()?.to_owned();
        for key in spec.markdown_attrs() {
            if let Ok(Some(value)) = self.attr(block, key) {
                info.push_str(&format!(" {key}={}", written(&value)));
            }
        }
        let text = match (spec.content(), spec.role()) {
            (Content::None, _) => String::new(),
            // Text as typed, as code is.
            (_, Role::Code) => self.plain_text(block).ok()?,
            // No line of it may read as a heading, a list item or a quote.
            _ => {
                let text = inline(&self.plain_spans(block).ok()?);
                let lines: Vec<_> = text.split('\n').map(line_start).collect();
                lines.join("\n")
            }
        };
        let fence = "`".repeat(longest_run(&text, '`').max(2) + 1);
        Some(match text.is_empty() {
            true => format!("{fence}{info}\n{fence}"),
            false => format!("{fence}{info}\n{text}\n{fence}"),
        })
    }

    /// Makes of a fenced block the block whose type the schema writes as a
    /// fence with its tag. It gets the attributes the type says its
    /// Markdown has and no others, which only a model or a hand made up,
    /// and what is inside as its text. A tag no type has stays code.
    pub(super) fn unfence(&self, node: &mut Node) {
        let Some(info) = node.info.take() else {
            return;
        };
        let mut words = words(&info).into_iter();
        let Some((tag, None)) = words.next() else {
            return;
        };
        let Some(spec) = self.schema().fenced(&tag) else {
            return;
        };
        node.block_type = spec.name().to_owned().into();
        let said = words.filter_map(|(key, value)| Some((key, value?)));
        let own = said.filter(|(key, _)| spec.markdown_attrs().contains(key));
        node.attrs = own.map(|(key, value)| (key.into(), value)).collect();
        match (spec.content(), spec.role()) {
            (Content::None, _) => node.spans.clear(),
            (_, Role::Code) => {}
            _ => {
                let text: String = node.spans.drain(..).map(|span| span.text).collect();
                node.spans = lines(&text);
            }
        }
    }
}

/// The text of `markdown` with its marks, a line for each block of it,
/// those nested in others too: a fence holds text, and none of it is lost
/// for looking like a list.
fn lines(markdown: &str) -> Vec<Span> {
    let mut spans = Vec::new();
    let mut nodes = Reader::default().read(markdown);
    nodes.reverse();
    while let Some(mut node) = nodes.pop() {
        nodes.extend(node.children.drain(..).rev());
        if node.spans.is_empty() {
            continue;
        }
        if !spans.is_empty() {
            spans.push(Span::new("\n", BTreeMap::new()));
        }
        spans.extend(node.spans);
    }
    spans
}

/// The signs a value is written without, as `\u0022` is a quote: those
/// that would end it, and those Markdown reads otherwise after a fence's
/// tag. Other signs stay as they are, a `%` in an address among them.
fn escaped(c: char) -> bool {
    matches!(c, '"' | '\'' | '\\' | '`' | '&') || c.is_control()
}

/// A value after a fence's tag: bare where it reads back as it is, and in
/// quotes otherwise.
fn written(value: &Value) -> String {
    let text = match value {
        Value::Bool(on) => return on.to_string(),
        Value::Int(number) => return number.to_string(),
        Value::Str(text) => text,
    };
    let kept = |c: char| !c.is_whitespace() && c != '=' && !escaped(c);
    if !text.is_empty() && text.chars().all(kept) && bare(text) == *value {
        return text.clone();
    }
    let mut quoted = String::from("\"");
    for c in text.chars() {
        match escaped(c) {
            true => quoted.push_str(&format!("\\u{:04X}", u32::from(c))),
            false => quoted.push(c),
        }
    }
    quoted + "\""
}

/// A value written bare: a truth, a whole number, or text.
fn bare(word: &str) -> Value {
    match word {
        "true" => Value::Bool(true),
        "false" => Value::Bool(false),
        word => match word.parse::<i64>() {
            Ok(number) => Value::Int(number),
            Err(_) => Value::Str(word.to_owned()),
        },
    }
}

/// The text of a quoted value, with each sign written as its code back.
fn unquoted(text: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(at) = rest.find("\\u") {
        out.push_str(&rest[..at]);
        let code = rest.get(at + 2..at + 6);
        let sign = code.and_then(|code| u32::from_str_radix(code, 16).ok());
        match sign.and_then(char::from_u32) {
            Some(sign) => {
                out.push(sign);
                rest = &rest[at + 6..];
            }
            None => {
                out.push_str("\\u");
                rest = &rest[at + 2..];
            }
        }
    }
    out + rest
}

/// The words after the signs that open a fence: its tag, then `key=value`,
/// a value with spaces in quotes of either kind.
fn words(info: &str) -> Vec<(String, Option<Value>)> {
    let mut words = Vec::new();
    let mut rest = info.trim_start();
    while !rest.is_empty() {
        let end = rest.find(|c: char| c == '=' || c.is_whitespace());
        let (key, after) = rest.split_at(end.unwrap_or(rest.len()));
        let Some(after) = after.strip_prefix('=') else {
            words.push((key.to_owned(), None));
            rest = after.trim_start();
            continue;
        };
        let quote = after.chars().next().filter(|c| matches!(c, '"' | '\''));
        let (value, after) = match quote {
            Some(quote) => {
                let quoted = &after[1..];
                let (text, after) = quoted.split_once(quote).unwrap_or((quoted, ""));
                (Value::Str(unquoted(text)), after)
            }
            None => {
                let end = after.find(char::is_whitespace).unwrap_or(after.len());
                (bare(&after[..end]), &after[end..])
            }
        };
        words.push((key.to_owned(), Some(value)));
        rest = after.trim_start();
    }
    words
}

/// Canonicalizes 1-based line ranges such as `1, 3-5` into `1,3-5`.
pub(crate) fn normalize_line_ranges(spec: &str) -> Option<String> {
    let mut parts = Vec::new();
    for raw in spec.trim().trim_matches(['{', '}']).split(',') {
        let raw = raw.trim();
        if raw.is_empty() {
            continue;
        }
        let range = match raw.split_once('-') {
            Some((start, end)) => {
                let start: u32 = start.trim().parse().ok()?;
                let end: u32 = end.trim().parse().ok()?;
                if start == 0 || start > end || end > 1_000_000 {
                    return None;
                }
                if start == end {
                    start.to_string()
                } else {
                    format!("{start}-{end}")
                }
            }
            None => {
                let line: u32 = raw.parse().ok()?;
                if line == 0 || line > 1_000_000 {
                    return None;
                }
                line.to_string()
            }
        };
        parts.push(range);
    }
    (!parts.is_empty()).then(|| parts.join(","))
}

/// Parses a code fence's info string (`lang twoslash title="file" {1,3-5}` or `lang:file`).
pub(super) fn parse_code_info(info: &str) -> Vec<(std::borrow::Cow<'static, str>, Value)> {
    let mut rest = info.trim();
    if rest.is_empty() {
        return Vec::new();
    }
    let (mut language, mut title, mut twoslash) =
        (None::<String>, None::<String>, None::<bool>);
    let mut highlights = Vec::<String>::new();

    if !rest.starts_with(['{', '[', '"', '\'']) {
        let end = rest
            .find(|c: char| c.is_whitespace() || c == '{')
            .unwrap_or(rest.len());
        let first = &rest[..end];
        if !first.contains('=') {
            if first.eq_ignore_ascii_case("twoslash") {
                twoslash = Some(true);
            } else if let Some((lang, file)) = first.split_once(':') {
                if !lang.is_empty() {
                    language = Some(lang.to_owned());
                }
                if !file.is_empty() {
                    title = Some(file.to_owned());
                }
            } else if !first.is_empty() {
                language = Some(first.to_owned());
            }
            rest = rest[end..].trim_start();
        }
    }

    while !rest.is_empty() {
        if let Some(inside) = rest.strip_prefix('{') {
            let Some((spec, after)) = inside.split_once('}') else {
                break;
            };
            if let Some(spec) = normalize_line_ranges(spec) {
                highlights.push(spec);
            }
            rest = after.trim_start();
            continue;
        }
        if let Some(inside) = rest.strip_prefix('[') {
            let Some((file, after)) = inside.split_once(']') else {
                break;
            };
            let file = file.trim();
            if title.is_none() && !file.is_empty() {
                title = Some(file.to_owned());
            }
            rest = after.trim_start();
            continue;
        }
        let end = rest
            .find(|c: char| c == '=' || c.is_whitespace() || c == '{')
            .unwrap_or(rest.len());
        let (key, after) = rest.split_at(end);
        let Some(after) = after.strip_prefix('=') else {
            if key.eq_ignore_ascii_case("twoslash") {
                twoslash = Some(true);
            }
            rest = after.trim_start();
            continue;
        };
        let quote = after.chars().next().filter(|c| matches!(c, '"' | '\''));
        let (value, after) = match quote {
            Some(quote) => {
                let quoted = &after[1..];
                let (text, after) = quoted.split_once(quote).unwrap_or((quoted, ""));
                (Value::Str(unquoted(text)), after)
            }
            None => {
                let end = after
                    .find(|c: char| c.is_whitespace() || c == '{')
                    .unwrap_or(after.len());
                (bare(&after[..end]), &after[end..])
            }
        };
        match key.to_ascii_lowercase().as_str() {
            "title" | "file" | "filename" => match value {
                Value::Str(text) if !text.is_empty() => title = Some(text),
                Value::Int(num) => title = Some(num.to_string()),
                _ => {}
            },
            "twoslash" => match value {
                Value::Bool(on) => twoslash = Some(on),
                Value::Int(num) => twoslash = Some(num != 0),
                Value::Str(text) => {
                    twoslash = Some(!matches!(text.as_str(), "false" | "0" | "off"))
                }
            },
            "lang" | "language" => {
                if let Value::Str(text) = value
                    && !text.is_empty()
                {
                    language = Some(text);
                }
            }
            "highlight" | "lines" | "highlight_lines" => {
                let raw = match value {
                    Value::Str(text) => text,
                    Value::Int(num) => num.to_string(),
                    Value::Bool(_) => String::new(),
                };
                if let Some(spec) = normalize_line_ranges(&raw) {
                    highlights.push(spec);
                }
            }
            _ => {}
        }
        rest = after.trim_start();
    }

    let mut attrs = Vec::new();
    if let Some(language) = language {
        attrs.push((attr::LANGUAGE.into(), Value::Str(language)));
    }
    if let Some(title) = title {
        attrs.push((attr::TITLE.into(), Value::Str(title)));
    }
    if let Some(twoslash) = twoslash {
        attrs.push((attr::TWOSLASH.into(), Value::Bool(twoslash)));
    }
    if let Some(lines) = normalize_line_ranges(&highlights.join(",")) {
        attrs.push((attr::HIGHLIGHT_LINES.into(), Value::Str(lines)));
    }
    attrs
}

impl Document {
    /// Formats the info string after the opening backticks of a `code_block`.
    pub(super) fn code_fence_info(&self, block: BlockId) -> String {
        let mut parts = Vec::new();
        if let Some(language) = self.code_language(block) {
            let clean: String = language
                .chars()
                .filter(|c| {
                    !c.is_whitespace()
                        && !matches!(c, '`' | '{' | '}' | '[' | ']' | '=' | '"' | '\'' | ':')
                })
                .collect();
            if clean.eq_ignore_ascii_case("twoslash") {
                parts.push(format!("lang={}", written(&Value::Str(clean))));
            } else if !clean.is_empty() {
                parts.push(clean);
            }
        }
        match self.attr(block, attr::TWOSLASH).ok().flatten() {
            Some(Value::Bool(false)) => parts.push("twoslash=false".to_owned()),
            Some(Value::Bool(true)) => parts.push("twoslash".to_owned()),
            _ => {}
        }
        if let Some(title) = self.code_title(block) {
            parts.push(format!("title={}", written(&Value::Str(title))));
        }
        if let Some(lines) = self
            .code_highlights(block)
            .and_then(|spec| normalize_line_ranges(&spec))
        {
            parts.push(format!("{{{lines}}}"));
        }
        parts.join(" ")
    }
}
