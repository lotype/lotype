//! Rich text and inline signs as Markdown.

use super::math::is_inline_math_markdown_safe;
use crate::{Schema, Span, Value};

pub(crate) fn longest_run(text: &str, of: char) -> usize {
    let runs = text.split(|c| c != of);
    runs.map(str::len).max().unwrap_or(0)
}

/// A mark that is open, with what tells it from another and what closes it.
struct Open {
    key: String,
    close: String,
}

/// Rich text as Markdown. A line break in it is a hard break.
pub(crate) fn inline(spans: &[Span], schema: &Schema) -> String {
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
    let inline_math = schema
        .inline_math_spec()
        .map(|(spec, attr)| (spec.name(), attr));
    for span in spans {
        let (owned, verbatim_math) = if let Some(atom) = &span.atom {
            if let Some((name, attr)) = inline_math
                && atom.name == name
                && let Some(Value::Str(tex)) = atom.attrs.get(attr)
                && is_inline_math_markdown_safe(tex)
            {
                (Some(format!("${tex}$")), true)
            } else {
                let plain = schema
                    .inline_spec(&atom.name)
                    .map(|spec| spec.text(atom))
                    .unwrap_or_default();
                if plain.is_empty() {
                    continue;
                }
                (Some(plain), false)
            }
        } else {
            (None, false)
        };
        let raw = owned.as_deref().unwrap_or(span.text.as_str());
        let core = raw.trim();
        if core.is_empty() {
            space.push_str(raw);
            continue;
        }
        let lead = raw.len() - raw.trim_start().len();
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

        space.push_str(&raw[..lead]);
        // A line cannot start with a break, nor can the text.
        if out.is_empty() && space.contains(['\n', '\r']) {
            space.clear();
        }
        if !space.is_empty() {
            out.push_str(&escape(&space));
            space.clear();
            (guard, closed) = (false, None);
        }

        let text = if verbatim_math {
            core.to_owned()
        } else if on("code") {
            code(core)
        } else {
            escape(core)
        };
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
        space.push_str(&raw[lead + core.len()..]);
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
pub(super) fn escape(text: &str) -> String {
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
pub(crate) fn line_start(line: &str) -> String {
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
pub(super) fn address(url: &str) -> String {
    let url = url.replace('\\', "\\\\").replace('&', "&amp;");
    let plain = !url.is_empty() && !url.contains(|c: char| c.is_whitespace() || "()<>".contains(c));
    if plain {
        return url;
    }
    let url = url.replace('<', "%3C").replace('>', "%3E");
    format!("<{}>", url.replace(['\n', '\r'], ""))
}
