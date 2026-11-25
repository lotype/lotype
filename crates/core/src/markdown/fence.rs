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
use super::write::{inline, longest_run};
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
            _ => inline(&self.plain_spans(block).ok()?),
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

/// The text of `markdown` with its marks, a line for each block of it.
fn lines(markdown: &str) -> Vec<Span> {
    let mut spans = Vec::new();
    for node in Reader::default().read(markdown) {
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

/// What is written in place of a sign that would end a value or that
/// Markdown reads otherwise after a fence's tag: its code, in hex.
const SIGNS: [char; 5] = ['%', '"', '\\', '`', '&'];

/// A value after a fence's tag: bare where it reads back as it is, and in
/// quotes otherwise.
fn written(value: &Value) -> String {
    let text = match value {
        Value::Bool(on) => return on.to_string(),
        Value::Int(number) => return number.to_string(),
        Value::Str(text) => text,
    };
    let kept = |c: char| !c.is_whitespace() && !c.is_control() && c != '=' && !SIGNS.contains(&c);
    if !text.is_empty() && text.chars().all(kept) && bare(text) == *value {
        return text.clone();
    }
    let mut quoted = String::from("\"");
    for c in text.chars() {
        match c {
            c if SIGNS.contains(&c) || c.is_control() => {
                quoted.push_str(&format!("%{:02X}", u32::from(c).min(0xFF)));
            }
            c => quoted.push(c),
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
    while let Some(at) = rest.find('%') {
        out.push_str(&rest[..at]);
        let code = rest.get(at + 1..at + 3);
        match code.and_then(|code| u8::from_str_radix(code, 16).ok()) {
            Some(code) => {
                out.push(char::from(code));
                rest = &rest[at + 3..];
            }
            None => {
                out.push('%');
                rest = &rest[at + 1..];
            }
        }
    }
    out + rest
}

/// The words after the signs that open a fence: its tag, then `key=value`,
/// a value with spaces in quotes.
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
        let (value, after) = match after.strip_prefix('"') {
            Some(quoted) => {
                let (text, after) = quoted.split_once('"').unwrap_or((quoted, ""));
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
