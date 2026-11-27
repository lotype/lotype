//! Low-level directive and token parsers for Twoslash and Shiki comment notations.

use std::ops::Range;

use crate::analyzer::{CompletionItem, CompletionKind, Severity};
use crate::twoslash::{CutKind, LineTag};

pub(super) struct SourceLine<'a> {
    pub(super) start: usize,
    pub(super) text: &'a str,
    pub(super) directive_only: bool,
}

/// Parsed Shiki `[!code ...]` notation on a source line.
pub(super) enum CodeNotation {
    Line {
        tag: LineTag,
        count: usize,
        standalone: bool,
        trimmed_end: usize,
    },
    Word {
        word: String,
        standalone: bool,
        trimmed_end: usize,
    },
}

pub(super) fn nearest_code_line(lines: &[SourceLine<'_>], above: usize) -> Option<usize> {
    (0..above)
        .rev()
        .find(|&idx| !lines[idx].directive_only && !lines[idx].text.trim().is_empty())
}

pub(super) fn comment_body(line: &str) -> Option<(usize, &str)> {
    let trimmed = line.trim_start();
    let indent_bytes = line.len() - trimmed.len();
    let indent_chars = line[..indent_bytes].chars().count();
    for marker in ["//", "#", "--"] {
        if let Some(rest) = trimmed.strip_prefix(marker) {
            if marker == "#" && rest.starts_with('[') {
                // Rust attribute `#[derive(...)]`, not a comment.
                continue;
            }
            if marker == "//" && (rest.starts_with('/') || rest.starts_with('!')) {
                // Rust doc comment `///` or `//!` is documentation, not a Twoslash directive.
                continue;
            }
            return Some((indent_chars + marker.chars().count(), rest));
        }
    }
    None
}

pub(super) fn parse_cut(trimmed: &str) -> Option<CutKind> {
    match trimmed {
        "---cut---" | "---cut-before---" => Some(CutKind::Before),
        "---cut-after---" => Some(CutKind::After),
        "---cut-start---" => Some(CutKind::Start),
        "---cut-end---" => Some(CutKind::End),
        _ => None,
    }
}

pub(super) fn parse_compiler_flag(
    trimmed: &str,
    no_errors: &mut bool,
    expected: &mut Vec<String>,
) -> bool {
    if matches!(trimmed, "@noErrors" | "@noErrorValidation") || trimmed.starts_with("@noErrors:") {
        *no_errors = true;
        return true;
    }
    if let Some(rest) = trimmed.strip_prefix("@errors:") {
        for token in rest.split(|ch: char| ch.is_whitespace() || ch == ',') {
            let clean = token.trim();
            if !clean.is_empty() && !expected.iter().any(|c| c == clean) {
                expected.push(clean.to_owned());
            }
        }
        return true;
    }
    // Common TypeScript Twoslash compiler options that hide the comment line in output.
    for flag in [
        "@target:",
        "@module:",
        "@strict",
        "@lib:",
        "@jsx:",
        "@allowJs",
        "@checkJs",
        "@noImplicitAny",
        "@exactOptionalPropertyTypes",
    ] {
        if trimmed == flag.trim_end_matches(':') || trimmed.starts_with(flag) {
            return true;
        }
    }
    false
}

pub(super) fn parse_filename_directive(trimmed: &str) -> Option<String> {
    let rest = trimmed.strip_prefix("@filename:")?.trim();
    (!rest.is_empty()).then(|| rest.to_owned())
}

pub(super) fn parse_caret_prefix(body: &str) -> Option<(usize, usize, &str)> {
    let spaces_bytes = body.len() - body.trim_start_matches(' ').len();
    let after_spaces = &body[spaces_bytes..];
    let carets_bytes = after_spaces.len() - after_spaces.trim_start_matches('^').len();
    if carets_bytes == 0 {
        return None;
    }
    Some((spaces_bytes, carets_bytes, &after_spaces[carets_bytes..]))
}

pub(super) fn parse_diagnostic_directive(text: &str) -> Option<(Severity, Option<String>, String)> {
    let prefixes = [
        ("@error", Severity::Error),
        ("@warn", Severity::Warning),
        ("@warning", Severity::Warning),
        ("@log", Severity::Info),
        ("@info", Severity::Info),
        ("@annotate", Severity::Hint),
        ("@hint", Severity::Hint),
    ];
    for (prefix, severity) in prefixes {
        if let Some(rest) = text.strip_prefix(prefix) {
            if prefix == "@error" && rest.starts_with('s') {
                // `@errors: 2322` is handled by `parse_compiler_flag`.
                continue;
            }
            let rest = rest.trim_start();
            let (code, message) = if let Some(inside) = rest.strip_prefix('(')
                && let Some((code, after)) = inside.split_once(')')
            {
                (
                    (!code.trim().is_empty()).then(|| code.trim().to_owned()),
                    after.trim_start_matches(':').trim(),
                )
            } else if let Some(after_colon) = rest.strip_prefix(':') {
                (None, after_colon.trim())
            } else {
                continue;
            };
            if !message.is_empty() {
                return Some((severity, code, message.to_owned()));
            }
        }
    }
    None
}

pub(super) fn parse_hover_directive(trimmed: &str) -> Option<(String, String, Option<String>)> {
    let rest = trimmed.strip_prefix("@hover")?.trim_start();
    let (symbol, detail) = rest.split_once(':')?;
    let symbol = symbol.trim();
    let detail = detail.trim();
    if symbol.is_empty() || detail.is_empty() {
        return None;
    }
    let (signature, docs) = match detail.split_once(" - ") {
        Some((sig, doc)) => (sig.trim().to_owned(), Some(doc.trim().to_owned())),
        None => (detail.to_owned(), None),
    };
    Some((symbol.to_owned(), signature, docs))
}

pub(super) fn parse_inline_completions(raw: &str) -> Vec<CompletionItem> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }
    trimmed
        .split(['|', ','])
        .filter_map(|entry| {
            let entry = entry.trim();
            if entry.is_empty() {
                return None;
            }
            if let Some(paren) = entry.find('(') {
                let label = entry[..paren].trim().to_owned();
                return Some(CompletionItem {
                    label,
                    kind: CompletionKind::Function,
                    detail: Some(entry[paren..].trim().to_owned()),
                });
            }
            if let Some((name, ty)) = entry.split_once(':') {
                return Some(CompletionItem {
                    label: name.trim().to_owned(),
                    kind: CompletionKind::Property,
                    detail: Some(ty.trim().to_owned()),
                });
            }
            Some(CompletionItem {
                label: entry.to_owned(),
                kind: CompletionKind::Property,
                detail: None,
            })
        })
        .collect()
}

pub(super) fn parse_code_notation(line: &str, line_start: usize) -> Option<CodeNotation> {
    let tag_pos = line.rfind("[!code ")?;
    let rest = &line[tag_pos + "[!code ".len()..];
    let (inner, after_bracket) = rest.split_once(']')?;
    let after_bracket = after_bracket.trim();
    let before = line[..tag_pos].trim_end();
    let (cleaned_before, opener) = strip_comment_marker(before)?;
    let closing_ok = match opener {
        "/*" => after_bracket == "*/",
        "<!--" => after_bracket == "-->",
        _ => after_bracket.is_empty(),
    };
    if !closing_ok {
        return None;
    }

    let inner = inner.trim();
    let stripped_len = cleaned_before.len();
    let standalone = stripped_len == 0;
    let trimmed_end = line_start + stripped_len;

    if let Some(word_spec) = inner.strip_prefix("word:") {
        let word = word_spec.split(':').next().unwrap_or("").trim();
        if !word.is_empty() {
            return Some(CodeNotation::Word {
                word: word.to_owned(),
                standalone,
                trimmed_end,
            });
        }
        return None;
    }

    let (kind_str, count) = match inner.split_once(':') {
        Some((k, n)) => (k.trim(), n.trim().parse::<usize>().ok().unwrap_or(1).max(1)),
        None => (inner, 1),
    };
    let tag = match kind_str {
        "highlight" | "hl" | "focus" => LineTag::Highlight,
        "++" | "add" => LineTag::Added,
        "--" | "remove" => LineTag::Removed,
        "error" => LineTag::Error,
        "warning" | "warn" => LineTag::Warning,
        _ => return None,
    };
    Some(CodeNotation::Line {
        tag,
        count,
        standalone,
        trimmed_end,
    })
}

pub(super) fn strip_inline_code_tag(line: &str) -> Option<(String, bool)> {
    let (standalone, trimmed_end) = match parse_code_notation(line, 0)? {
        CodeNotation::Line {
            standalone,
            trimmed_end,
            ..
        }
        | CodeNotation::Word {
            standalone,
            trimmed_end,
            ..
        } => (standalone, trimmed_end),
    };
    Some((line[..trimmed_end].to_owned(), standalone))
}

fn strip_comment_marker(before: &str) -> Option<(&str, &'static str)> {
    // Check `<!--` before `--` so HTML comments are not truncated to `<!`.
    for marker in ["<!--", "//", "#", "--", "/*"] {
        if let Some(stripped) = before.strip_suffix(marker) {
            return Some((stripped.trim_end(), marker));
        }
    }
    None
}

pub(super) fn completion_context_at(
    line: &str,
    line_start: usize,
    col_chars: usize,
) -> (usize, String, Option<String>) {
    let char_offsets: Vec<(usize, char)> = line.char_indices().collect();
    let clamped_bytes = if col_chars >= char_offsets.len() {
        line.len()
    } else {
        char_offsets[col_chars].0
    };
    let before = &line[..clamped_bytes];
    let is_ident = |ch: char| ch.is_alphanumeric() || ch == '_' || ch == '$';
    let prefix_chars: String = before
        .chars()
        .rev()
        .take_while(|&ch| is_ident(ch))
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    let before_prefix = &before[..before.len() - prefix_chars.len()];
    let receiver = before_prefix
        .strip_suffix('.')
        .or_else(|| before_prefix.strip_suffix("::"))
        .map(|left| {
            left.chars()
                .rev()
                .take_while(|&ch| is_ident(ch))
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect::<String>()
        })
        .filter(|recv| !recv.is_empty());
    (line_start + clamped_bytes, prefix_chars, receiver)
}

pub(super) fn token_range_at_column(
    line: &str,
    line_start: usize,
    col_chars: usize,
    caret_chars: usize,
) -> (Range<usize>, String) {
    let char_offsets: Vec<(usize, char)> = line.char_indices().collect();
    if char_offsets.is_empty() {
        return (line_start..line_start, String::new());
    }
    let clamped = col_chars.min(char_offsets.len() - 1);
    if caret_chars > 1 {
        let start_byte = char_offsets[clamped].0;
        let end_char = (clamped + caret_chars).min(char_offsets.len());
        let end_byte = char_offsets
            .get(end_char)
            .map_or(line.len(), |(offset, _)| *offset);
        return (
            line_start + start_byte..line_start + end_byte,
            line[start_byte..end_byte].to_owned(),
        );
    }
    let is_ident = |ch: char| ch.is_alphanumeric() || ch == '_' || ch == '$';
    if is_ident(char_offsets[clamped].1) {
        let mut left = clamped;
        while left > 0 && is_ident(char_offsets[left - 1].1) {
            left -= 1;
        }
        let mut right = clamped + 1;
        while right < char_offsets.len() && is_ident(char_offsets[right].1) {
            right += 1;
        }
        let start_byte = char_offsets[left].0;
        let end_byte = char_offsets
            .get(right)
            .map_or(line.len(), |(offset, _)| *offset);
        return (
            line_start + start_byte..line_start + end_byte,
            line[start_byte..end_byte].to_owned(),
        );
    }
    let start_byte = char_offsets[clamped].0;
    let end_byte = start_byte + char_offsets[clamped].1.len_utf8();
    (
        line_start + start_byte..line_start + end_byte,
        line[start_byte..end_byte].to_owned(),
    )
}

pub(super) fn trimmed_line_range(line: &str, line_start: usize) -> Range<usize> {
    let leading = line.len() - line.trim_start().len();
    let trailing = line.trim_end().len();
    let end = trailing.max(leading);
    line_start + leading..line_start + end
}

pub(super) fn find_symbol_in_line(
    line: &str,
    line_start: usize,
    symbol: &str,
) -> Option<Range<usize>> {
    let is_ident = |ch: char| ch.is_alphanumeric() || ch == '_' || ch == '$';
    let mut search_from = 0;
    while let Some(rel) = line[search_from..].find(symbol) {
        let start = search_from + rel;
        let end = start + symbol.len();
        let left_ok = line[..start]
            .chars()
            .next_back()
            .is_none_or(|ch| !is_ident(ch));
        let right_ok = line[end..].chars().next().is_none_or(|ch| !is_ident(ch));
        if left_ok && right_ok {
            return Some(line_start + start..line_start + end);
        }
        search_from = start + symbol.chars().next().map_or(1, char::len_utf8);
    }
    None
}

pub(super) fn find_word_ranges(line: &str, line_start: usize, word: &str) -> Vec<Range<usize>> {
    if word.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(rel) = line[from..].find(word) {
        let start = from + rel;
        let end = start + word.len();
        out.push(line_start + start..line_start + end);
        from = end;
    }
    out
}
