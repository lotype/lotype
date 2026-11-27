//! Parser for JavaScript/TypeScript Twoslash directives (`^?`, `^|`, `@errors`,
//! `@noErrors`, `@filename`, `---cut---`, `@hover`, `@log`, `@annotate`) and Shiki
//! notation transformers (`[!code highlight]`, `[!code ++]`, `[!code --]`, `[!code word:...]`).

mod directives;

use std::ops::Range;

use crate::analyzer::{
    CodeAnalysis, CompletionItem, CompletionSite, DiagnosticAnnotation, HoverAnnotation, QuerySite,
    VirtualFile,
};
use crate::lines::parse_line_ranges;
use directives::{
    CodeNotation, SourceLine, comment_body, completion_context_at, find_symbol_in_line,
    find_word_ranges, nearest_code_line, parse_caret_prefix, parse_code_notation,
    parse_compiler_flag, parse_cut, parse_diagnostic_directive, parse_filename_directive,
    parse_hover_directive, parse_inline_completions, strip_inline_code_tag, token_range_at_column,
    trimmed_line_range,
};

/// Visual tag applied to a source line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineTag {
    /// Neutral emphasis (`{1,3-5}` or `[!code highlight]`).
    Highlight,
    /// Diff insertion (`[!code ++]`).
    Added,
    /// Diff removal (`[!code --]`).
    Removed,
    /// Error line emphasis (`[!code error]`).
    Error,
    /// Warning line emphasis (`[!code warning]`).
    Warning,
}

/// Kind of Twoslash cut divider comment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CutKind {
    /// Hides setup lines above in preview and copy (`---cut---` / `---cut-before---`).
    Before,
    /// Hides trailing lines below in preview and copy (`---cut-after---`).
    After,
    /// Starts a hidden region ended by [`CutKind::End`] (`---cut-start---`).
    Start,
    /// Ends a hidden region started by [`CutKind::Start`] (`---cut-end---`).
    End,
}

/// Per-line marker produced by fence ranges or inline directives.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LineMarker {
    /// Zero-based line index.
    pub line: usize,
    /// Visual tag for this line.
    pub tag: LineTag,
}

/// A `^?` Twoslash callout pinned to a token on an earlier code line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueryCallout {
    /// Zero-based line index of the `^?` comment itself (callout renders below it).
    pub comment_line: usize,
    /// Zero-based line index of the inspected code line.
    pub target_line: usize,
    /// Zero-based character column of the `^` pointer.
    pub character: usize,
    /// UTF-8 byte range of the inspected token in the block's text.
    pub target_range: Range<usize>,
    /// Inspected token text.
    pub symbol: String,
    /// Type or value label to display (`None` while waiting on an async analyzer).
    pub text: Option<String>,
}

/// A `^|` Twoslash completion dropdown pinned to a cursor column on an earlier code line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompletionCallout {
    /// Zero-based line index of the `^|` comment itself.
    pub comment_line: usize,
    /// Zero-based line index of the code line above `^|`.
    pub target_line: usize,
    /// Zero-based character column of the `^` pointer.
    pub character: usize,
    /// UTF-8 byte offset in the block's text at `(target_line, character)`.
    pub offset: usize,
    /// Identifier prefix typed immediately before `offset`.
    pub prefix: String,
    /// Receiver expression before `.` or `::`, if any.
    pub receiver: Option<String>,
    /// Completion candidates to display.
    pub items: Vec<CompletionItem>,
}

/// Complete set of annotations extracted from a code block's text and fence attributes.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ParsedCode {
    /// Highlighted, diffed, or flagged lines.
    pub lines: Vec<LineMarker>,
    /// Byte ranges highlighted via `[!code word:...]`.
    pub word_highlights: Vec<Range<usize>>,
    /// Twoslash `^?` query callouts.
    pub queries: Vec<QueryCallout>,
    /// Twoslash `^|` completion dropdowns.
    pub completions: Vec<CompletionCallout>,
    /// Inline and analyzer diagnostics.
    pub diagnostics: Vec<DiagnosticAnnotation>,
    /// Symbol hover entries.
    pub hovers: Vec<HoverAnnotation>,
    /// Twoslash cut divider lines (`(line_index, kind)`).
    pub cuts: Vec<(usize, CutKind)>,
    /// Virtual files declared with `// @filename: ...`.
    pub files: Vec<VirtualFile>,
    /// Whether `// @noErrors` was set on the block.
    pub no_errors: bool,
    /// Expected diagnostic codes declared via `// @errors: ...`.
    pub expected_errors: Vec<String>,
    /// Zero-based lines hidden when rendering in Shiki Twoslash preview mode.
    pub hidden_in_preview: Vec<usize>,
    /// Visible end-byte overrides `(line_index, end_byte)` for trailing `[!code ...]` tags.
    pub trimmed_in_preview: Vec<(usize, usize)>,
    cut_mask: Vec<bool>,
}

impl ParsedCode {
    /// Builds [`QuerySite`] entries for an [`AnalysisRequest`](crate::AnalysisRequest).
    pub fn query_sites(&self) -> Vec<QuerySite> {
        self.queries
            .iter()
            .map(|query| QuerySite {
                target_range: query.target_range.clone(),
                line: query.target_line,
                character: query.character,
                symbol: query.symbol.clone(),
            })
            .collect()
    }

    /// Builds [`CompletionSite`] entries for an [`AnalysisRequest`](crate::AnalysisRequest).
    pub fn completion_sites(&self) -> Vec<CompletionSite> {
        self.completions
            .iter()
            .map(|comp| CompletionSite {
                line: comp.target_line,
                character: comp.character,
                offset: comp.offset,
                prefix: comp.prefix.clone(),
                receiver: comp.receiver.clone(),
            })
            .collect()
    }

    /// Whether `line` lies inside a `---cut---` hidden section.
    pub fn is_cut_line(&self, line: usize) -> bool {
        self.cut_mask.get(line).copied().unwrap_or(false)
    }

    /// Merges asynchronous [`CodeAnalysis`] results while honoring `@noErrors`, `@errors`,
    /// and explicit inline overrides.
    pub fn merge_analysis(&mut self, analysis: &CodeAnalysis) {
        for (range, label) in &analysis.queries {
            for query in &mut self.queries {
                let overlaps = query.target_range == *range
                    || (query.target_range.start < range.end
                        && range.start < query.target_range.end);
                if overlaps && query.text.is_none() {
                    query.text = Some(label.clone());
                }
            }
        }
        for (offset, items) in &analysis.completions {
            for comp in &mut self.completions {
                if comp.offset == *offset && comp.items.is_empty() {
                    comp.items = items.clone();
                }
            }
        }
        // Explicit `@hover` and inline `^? text` hovers win; analyzer hovers fill every other
        // identifier, and come before hovers synthesized from analyzer query text so the richer
        // analyzer signature is kept for the same token.
        for hover in &analysis.hovers {
            let overlaps_existing = self.hovers.iter().any(|item| {
                item.range == hover.range
                    || (item.range.start < hover.range.end && hover.range.start < item.range.end)
            });
            if !overlaps_existing {
                self.hovers.push(hover.clone());
            }
        }
        for query in &self.queries {
            if let Some(signature) = &query.text
                && !self
                    .hovers
                    .iter()
                    .any(|hover| hover.range == query.target_range)
            {
                self.hovers.push(HoverAnnotation {
                    range: query.target_range.clone(),
                    symbol: query.symbol.clone(),
                    signature: signature.clone(),
                    docs: None,
                });
            }
        }
        if !self.no_errors {
            for diagnostic in &analysis.diagnostics {
                let on_completion_line = self
                    .completions
                    .iter()
                    .any(|comp| comp.target_line == diagnostic.after_line);
                if on_completion_line || !matches_expected(&self.expected_errors, diagnostic) {
                    continue;
                }
                let overlaps_existing = self.diagnostics.iter().any(|existing| {
                    existing == diagnostic
                        || existing.target_range == diagnostic.target_range
                        || (existing.target_range.start < diagnostic.target_range.end
                            && diagnostic.target_range.start < existing.target_range.end)
                });
                if !overlaps_existing {
                    self.diagnostics.push(diagnostic.clone());
                }
            }
        }
    }

    /// Finds the hover annotation covering `offset`, if any.
    pub fn hover_at(&self, offset: usize) -> Option<&HoverAnnotation> {
        self.hovers
            .iter()
            .find(|hover| hover.range.contains(&offset))
    }
}

fn matches_expected(expected: &[String], diagnostic: &DiagnosticAnnotation) -> bool {
    if expected.is_empty() {
        return true;
    }
    let Some(code) = &diagnostic.code else {
        return false;
    };
    let norm = code.trim_start_matches("TS");
    expected
        .iter()
        .any(|exp| exp.trim_start_matches("TS") == norm)
}

/// Parses line highlights and (when `twoslash` is true) Twoslash directives from `code`.
pub fn parse_code_annotations(
    code: &str,
    fence_highlights: Option<&str>,
    twoslash: bool,
) -> ParsedCode {
    let mut parsed = ParsedCode::default();
    let fence_set = fence_highlights.map(parse_line_ranges).unwrap_or_default();

    let mut raw_lines: Vec<SourceLine<'_>> = Vec::new();
    let mut offset = 0;
    for raw in code.split('\n') {
        let text = raw.strip_suffix('\r').unwrap_or(raw);
        raw_lines.push(SourceLine {
            start: offset,
            text,
            directive_only: false,
        });
        offset += raw.len() + 1;
    }

    let mut pending_hovers: Vec<(Option<usize>, String, String, Option<String>)> = Vec::new();
    let mut words_to_highlight: Vec<String> = Vec::new();
    let mut deferred_line_tags: Vec<(usize, usize, LineTag)> = Vec::new();
    let mut file_headers: Vec<(usize, String, usize)> = Vec::new();

    for i in 0..raw_lines.len() {
        let line_text = raw_lines[i].text;
        let line_start = raw_lines[i].start;
        if fence_set.contains_zero_based(i) {
            upsert_line_tag(&mut parsed.lines, i, LineTag::Highlight);
        }
        if let Some(notation) = parse_code_notation(line_text, line_start) {
            match notation {
                CodeNotation::Line {
                    tag,
                    count,
                    standalone,
                    trimmed_end,
                } => {
                    if standalone {
                        raw_lines[i].directive_only = true;
                        deferred_line_tags.push((i + 1, count, tag));
                        continue;
                    }
                    parsed.trimmed_in_preview.push((i, trimmed_end));
                    deferred_line_tags.push((i, count, tag));
                }
                CodeNotation::Word {
                    word,
                    standalone,
                    trimmed_end,
                } => {
                    words_to_highlight.push(word);
                    if standalone {
                        raw_lines[i].directive_only = true;
                        continue;
                    }
                    parsed.trimmed_in_preview.push((i, trimmed_end));
                }
            }
        }
        if !twoslash {
            continue;
        }
        let Some((comment_char_col, body)) = comment_body(line_text) else {
            continue;
        };
        let trimmed = body.trim();
        if let Some(cut) = parse_cut(trimmed) {
            raw_lines[i].directive_only = true;
            parsed.cuts.push((i, cut));
            continue;
        }
        if parse_compiler_flag(trimmed, &mut parsed.no_errors, &mut parsed.expected_errors) {
            raw_lines[i].directive_only = true;
            continue;
        }
        if let Some(filename) = parse_filename_directive(trimmed) {
            raw_lines[i].directive_only = true;
            let body_start = raw_lines
                .get(i + 1)
                .map_or(code.len(), |next| next.start)
                .min(code.len());
            file_headers.push((i, filename, body_start));
            continue;
        }
        if let Some((symbol, signature, docs)) = parse_hover_directive(trimmed) {
            raw_lines[i].directive_only = true;
            let code_above = nearest_code_line(&raw_lines, i);
            pending_hovers.push((code_above, symbol, signature, docs));
            continue;
        }
        if let Some((caret_offset_chars, caret_len_chars, rest)) = parse_caret_prefix(body) {
            let pointer_col = comment_char_col + caret_offset_chars;
            if let Some(rest_query) = rest.strip_prefix('?') {
                raw_lines[i].directive_only = true;
                if let Some(target_idx) = nearest_code_line(&raw_lines, i) {
                    let target = &raw_lines[target_idx];
                    let (range, symbol) = token_range_at_column(
                        target.text,
                        target.start,
                        pointer_col,
                        caret_len_chars,
                    );
                    let inline = rest_query.trim();
                    let text = (!inline.is_empty()).then(|| inline.to_owned());
                    if let Some(signature) = &text {
                        parsed.hovers.push(HoverAnnotation {
                            range: range.clone(),
                            symbol: symbol.clone(),
                            signature: signature.clone(),
                            docs: None,
                        });
                    }
                    parsed.queries.push(QueryCallout {
                        comment_line: i,
                        target_line: target_idx,
                        character: pointer_col,
                        target_range: range,
                        symbol,
                        text,
                    });
                }
                continue;
            }
            if let Some(rest_comp) = rest.strip_prefix('|') {
                raw_lines[i].directive_only = true;
                if let Some(target_idx) = nearest_code_line(&raw_lines, i) {
                    let target = &raw_lines[target_idx];
                    let (comp_offset, prefix, receiver) =
                        completion_context_at(target.text, target.start, pointer_col);
                    parsed.completions.push(CompletionCallout {
                        comment_line: i,
                        target_line: target_idx,
                        character: pointer_col,
                        offset: comp_offset,
                        prefix,
                        receiver,
                        items: parse_inline_completions(rest_comp),
                    });
                }
                continue;
            }
            if let Some((severity, code_id, message)) = parse_diagnostic_directive(rest.trim()) {
                raw_lines[i].directive_only = true;
                if let Some(target_idx) = nearest_code_line(&raw_lines, i) {
                    let target = &raw_lines[target_idx];
                    let (range, _) = token_range_at_column(
                        target.text,
                        target.start,
                        pointer_col,
                        caret_len_chars,
                    );
                    parsed.diagnostics.push(DiagnosticAnnotation {
                        target_range: range,
                        after_line: i,
                        severity,
                        code: code_id,
                        message,
                    });
                }
                continue;
            }
        }
        if let Some((severity, code_id, message)) = parse_diagnostic_directive(trimmed) {
            raw_lines[i].directive_only = true;
            let target_idx = nearest_code_line(&raw_lines, i).unwrap_or(i);
            let target = &raw_lines[target_idx];
            let range = trimmed_line_range(target.text, target.start);
            parsed.diagnostics.push(DiagnosticAnnotation {
                target_range: range,
                after_line: i,
                severity,
                code: code_id,
                message,
            });
        }
    }

    // Apply deferred `[!code tag:count]` spans across non-directive code lines.
    for (start_line, mut remaining, tag) in deferred_line_tags {
        for (idx, line) in raw_lines.iter().enumerate().skip(start_line) {
            if remaining == 0 {
                break;
            }
            if !line.directive_only {
                upsert_line_tag(&mut parsed.lines, idx, tag);
                remaining -= 1;
            }
        }
    }

    // Highlight words requested via `[!code word:...]`.
    for word in &words_to_highlight {
        for line in &raw_lines {
            if !line.directive_only {
                parsed
                    .word_highlights
                    .extend(find_word_ranges(line.text, line.start, word));
            }
        }
    }

    // Build `@filename:` virtual file slices.
    for (pos, (header_line, name, body_start)) in file_headers.iter().enumerate() {
        let body_end = file_headers
            .get(pos + 1)
            .map_or(code.len(), |(next_header, _, _)| {
                let before_nl = raw_lines[*next_header].start.saturating_sub(1);
                if before_nl > 0 && code.as_bytes()[before_nl - 1] == b'\r' {
                    before_nl - 1
                } else {
                    before_nl
                }
            });
        parsed.files.push(VirtualFile {
            name: name.clone(),
            header_line: *header_line,
            range: (*body_start).min(body_end)..body_end,
        });
    }

    // Compute cut mask and preview hidden lines.
    parsed.cut_mask = build_cut_mask(raw_lines.len(), &parsed.cuts);
    for (idx, line) in raw_lines.iter().enumerate() {
        if line.directive_only || parsed.cut_mask[idx] {
            parsed.hidden_in_preview.push(idx);
        }
    }

    for (above, symbol, signature, docs) in pending_hovers {
        let search_lines: Vec<usize> = match above {
            Some(line_idx) => std::iter::once(line_idx)
                .chain((0..raw_lines.len()).filter(|idx| *idx != line_idx))
                .collect(),
            None => (0..raw_lines.len()).collect(),
        };
        for idx in search_lines {
            if raw_lines[idx].directive_only {
                continue;
            }
            if let Some(range) =
                find_symbol_in_line(raw_lines[idx].text, raw_lines[idx].start, &symbol)
            {
                let next = HoverAnnotation {
                    range: range.clone(),
                    symbol: symbol.clone(),
                    signature: signature.clone(),
                    docs: docs.clone(),
                };
                if let Some(existing) = parsed.hovers.iter_mut().find(|hover| hover.range == range)
                {
                    *existing = next;
                } else {
                    parsed.hovers.push(next);
                }
                break;
            }
        }
    }

    parsed
}

fn upsert_line_tag(lines: &mut Vec<LineMarker>, line: usize, tag: LineTag) {
    if let Some(existing) = lines.iter_mut().find(|m| m.line == line) {
        existing.tag = tag;
    } else {
        lines.push(LineMarker { line, tag });
    }
}

fn build_cut_mask(total_lines: usize, cuts: &[(usize, CutKind)]) -> Vec<bool> {
    let mut mask = vec![false; total_lines];
    let mut cut_before = None;
    let mut cut_after = None;
    let mut active_start: Option<usize> = None;
    for &(line, kind) in cuts {
        match kind {
            CutKind::Before => cut_before = Some(line),
            CutKind::After if cut_after.is_none() => cut_after = Some(line),
            CutKind::Start => active_start = Some(line),
            CutKind::End => {
                if let Some(start) = active_start.take() {
                    for flag in &mut mask[start..=line.min(total_lines.saturating_sub(1))] {
                        *flag = true;
                    }
                }
            }
            CutKind::After => {}
        }
    }
    if let Some(before) = cut_before {
        for flag in &mut mask[..=before.min(total_lines.saturating_sub(1))] {
            *flag = true;
        }
    }
    if let Some(after) = cut_after {
        for flag in &mut mask[after.min(total_lines)..] {
            *flag = true;
        }
    }
    mask
}

/// Produces clean clipboard text, stripping Twoslash directive-only lines,
/// cut regions, and `[!code ...]` tags.
pub fn clean_copy_text(code: &str, twoslash: bool) -> String {
    let parsed = parse_code_annotations(code, None, twoslash);
    let mut out = Vec::new();
    for (idx, raw) in code.split('\n').enumerate() {
        if parsed.hidden_in_preview.contains(&idx) {
            continue;
        }
        let line = raw.strip_suffix('\r').unwrap_or(raw);
        if let Some((cleaned, standalone)) = strip_inline_code_tag(line) {
            if standalone {
                continue;
            }
            out.push(cleaned);
        } else {
            out.push(line.to_owned());
        }
    }
    out.join("\n")
}
