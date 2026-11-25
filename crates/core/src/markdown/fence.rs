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
use crate::{BlockId, Content, Document, Role, Span, Value};

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
