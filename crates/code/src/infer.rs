//! Built-in zero-dependency semantic analyzer for Twoslash blocks.
//!
//! Extracts declarations, JSDoc/rustdoc comments, inferred types, identifier hovers,
//! `^?` type queries, `^|` completions, and basic assignment mismatch diagnostics.

mod builtins;
mod decls;

use std::collections::HashMap;
use std::ops::Range;

use crate::analyzer::{
    AnalysisFuture, AnalysisRequest, CodeAnalysis, CodeAnalyzer, CompletionItem, CompletionKind,
    DiagnosticAnnotation, HoverAnnotation,
};
use crate::languages::canonical_language;
use builtins::{
    LangFamily, builtin_members_for_type, infer_expr_type, language_keywords, lookup_member,
    rust_char_literal_end,
};
use decls::{
    is_ident_name, parse_fn_decl, parse_type_decl, parse_var_decl, strip_trailing_comment,
    strip_trailing_comment_preserving_len,
};

#[derive(Clone, Debug)]
pub(crate) struct SymbolInfo {
    pub(crate) kind: CompletionKind,
    pub(crate) signature: String,
    pub(crate) ty: Option<String>,
    pub(crate) return_type: Option<String>,
    pub(crate) docs: Option<String>,
}

/// Fast in-memory [`CodeAnalyzer`] enabled by default on [`CodeBlocks`](crate::CodeBlocks).
#[derive(Debug, Clone, Copy, Default)]
pub struct BuiltInAnalyzer;

impl BuiltInAnalyzer {
    /// Synchronously analyzes `request`.
    pub fn analyze_sync(request: &AnalysisRequest) -> CodeAnalysis {
        let canonical = request
            .language
            .as_deref()
            .map(canonical_language)
            .unwrap_or_else(|| "typescript".to_owned());
        let family = LangFamily::of(&canonical);

        let mut symbols: HashMap<String, SymbolInfo> = HashMap::new();
        let mut type_members: HashMap<String, Vec<CompletionItem>> = HashMap::new();
        let mut diagnostics: Vec<DiagnosticAnnotation> = Vec::new();

        let mut lines: Vec<&str> = Vec::new();
        let mut line_starts = Vec::new();
        let mut byte_cursor = 0;
        for raw in request.code.split('\n') {
            lines.push(raw.strip_suffix('\r').unwrap_or(raw));
            line_starts.push(byte_cursor);
            byte_cursor += raw.len() + 1;
        }

        // Pass 1: declarations, doc comments, interface/struct fields, and assignment checks.
        let mut pending_doc: Vec<String> = Vec::new();
        let mut in_jsdoc = false;
        let mut active_type_body: Option<String> = None;

        for (line_idx, &raw_line) in lines.iter().enumerate() {
            let trimmed = raw_line.trim();
            if in_jsdoc {
                if let Some(end_idx) = trimmed.find("*/") {
                    let text = trimmed[..end_idx].trim().trim_start_matches('*').trim();
                    if !text.is_empty() {
                        pending_doc.push(text.to_owned());
                    }
                    in_jsdoc = false;
                } else {
                    let text = trimmed.trim_start_matches('*').trim();
                    if !text.is_empty() {
                        pending_doc.push(text.to_owned());
                    }
                }
                continue;
            }
            let trimmed = if let Some(after_empty) = trimmed.strip_prefix("/**/") {
                pending_doc.clear();
                let rest = after_empty.trim();
                if rest.is_empty() {
                    continue;
                }
                rest
            } else {
                trimmed
            };
            if let Some(after_open) = trimmed.strip_prefix("/**") {
                pending_doc.clear();
                if let Some(end_idx) = after_open.find("*/") {
                    let text = after_open[..end_idx].trim();
                    if !text.is_empty() {
                        pending_doc.push(text.to_owned());
                    }
                } else {
                    let text = after_open.trim();
                    if !text.is_empty() {
                        pending_doc.push(text.to_owned());
                    }
                    in_jsdoc = true;
                }
                continue;
            }
            if let Some(rust_doc) = trimmed.strip_prefix("///") {
                pending_doc.push(rust_doc.trim().to_owned());
                continue;
            }
            if trimmed.starts_with("//") || trimmed.starts_with('#') {
                continue;
            }
            if trimmed.is_empty() {
                pending_doc.clear();
                continue;
            }

            // Collect fields inside an open `interface Foo { ... }` or `struct Foo { ... }`.
            if let Some(type_name) = active_type_body.clone() {
                if trimmed.starts_with('}') {
                    active_type_body = None;
                    continue;
                }
                let clean = strip_trailing_comment(family, trimmed)
                    .trim_end_matches([',', ';'])
                    .trim();
                let clean = clean.strip_prefix("pub ").unwrap_or(clean);
                if let Some((field, fty)) = clean.split_once(':') {
                    let fname = field.trim().trim_end_matches('?');
                    if is_ident_name(fname) {
                        type_members
                            .entry(type_name)
                            .or_default()
                            .push(CompletionItem {
                                label: fname.to_owned(),
                                kind: CompletionKind::Property,
                                detail: Some(format!(": {}", fty.trim())),
                            });
                    }
                }
                continue;
            }

            let docs = (!pending_doc.is_empty()).then(|| pending_doc.join(" "));
            pending_doc.clear();
            let code_only = strip_trailing_comment(family, trimmed);

            if let Some(name) = parse_type_decl(family, code_only, &docs, &mut symbols) {
                if code_only.ends_with('{') {
                    active_type_body = Some(name);
                }
                continue;
            }
            if parse_fn_decl(family, code_only, &lines, line_idx, &docs, &mut symbols) {
                continue;
            }
            parse_var_decl(
                family,
                code_only,
                raw_line,
                line_starts[line_idx],
                line_idx,
                &docs,
                &mut symbols,
                &mut diagnostics,
            );
        }

        // Pass 2: resolve identifier and member hovers across non-comment lines.
        let mut hovers = Vec::new();
        for (line_idx, &raw_line) in lines.iter().enumerate() {
            let trimmed_start = raw_line.trim_start();
            if trimmed_start.starts_with("//")
                || (trimmed_start.starts_with('#') && !trimmed_start.starts_with("#["))
            {
                continue;
            }
            let code_slice = strip_trailing_comment_preserving_len(family, raw_line);
            collect_line_hovers(
                family,
                code_slice,
                line_starts[line_idx],
                &symbols,
                &type_members,
                &mut hovers,
            );
        }

        // Pass 3: resolve `^?` queries.
        let mut queries = Vec::new();
        for q in &request.queries {
            if let Some(found) = hovers.iter().find(|h| h.range == q.target_range) {
                queries.push((q.target_range.clone(), found.signature.clone()));
            } else if let Some(sym) = symbols.get(&q.symbol) {
                queries.push((q.target_range.clone(), sym.signature.clone()));
            }
        }

        // Pass 4: resolve `^|` completions.
        let mut completions = Vec::new();
        for site in &request.completions {
            let mut candidates: Vec<CompletionItem> = if let Some(recv) = &site.receiver {
                if let Some(recv_ty) = infer_expr_type(family, recv, false, &symbols) {
                    let mut list = builtin_members_for_type(family, &recv_ty);
                    if let Some(custom) = type_members.get(recv_ty.trim()) {
                        list.extend(custom.iter().cloned());
                    }
                    list
                } else {
                    Vec::new()
                }
            } else {
                let mut list: Vec<CompletionItem> = symbols
                    .iter()
                    .map(|(name, info)| CompletionItem {
                        label: name.clone(),
                        kind: info.kind,
                        detail: info.ty.as_ref().map(|t| format!(": {t}")),
                    })
                    .collect();
                for &kw in language_keywords(family) {
                    list.push(CompletionItem {
                        label: kw.to_owned(),
                        kind: CompletionKind::Keyword,
                        detail: None,
                    });
                }
                list
            };
            candidates.retain(|item| item.label.starts_with(&site.prefix));
            candidates.sort_by(|a, b| a.label.cmp(&b.label));
            candidates.truncate(8);
            if !candidates.is_empty() {
                completions.push((site.offset, candidates));
            }
        }

        CodeAnalysis {
            queries,
            completions,
            hovers,
            diagnostics,
        }
    }
}

impl CodeAnalyzer for BuiltInAnalyzer {
    fn analyze(&self, request: AnalysisRequest) -> AnalysisFuture {
        let result = Self::analyze_sync(&request);
        Box::pin(async move { Ok(result) })
    }
}

fn collect_line_hovers(
    family: LangFamily,
    code_slice: &str,
    line_start: usize,
    symbols: &HashMap<String, SymbolInfo>,
    type_members: &HashMap<String, Vec<CompletionItem>>,
    hovers: &mut Vec<HoverAnnotation>,
) {
    let is_ident = |ch: char| ch.is_alphanumeric() || ch == '_' || ch == '$';
    let bytes = code_slice.as_bytes();
    let mut idx = 0;
    let mut in_quote: Option<u8> = None;

    while idx < bytes.len() {
        let b = bytes[idx];
        if let Some(q) = in_quote {
            if b == b'\\' {
                idx += 1;
                if let Some(escaped) = code_slice[idx..].chars().next() {
                    idx += escaped.len_utf8();
                }
                continue;
            }
            if b == q {
                in_quote = None;
            }
            idx += code_slice[idx..].chars().next().map_or(1, char::len_utf8);
            continue;
        }
        if b == b'\'' && family == LangFamily::Rust {
            if let Some(end) = rust_char_literal_end(code_slice, idx) {
                idx = end;
            } else {
                idx += 1;
            }
            continue;
        }
        if matches!(b, b'"' | b'\'' | b'`') {
            in_quote = Some(b);
            idx += 1;
            continue;
        }
        let Some(ch) = code_slice[idx..].chars().next() else {
            break;
        };
        if is_ident(ch) && !ch.is_ascii_digit() {
            let start = idx;
            let mut end = idx + ch.len_utf8();
            while let Some(next_ch) = code_slice[end..].chars().next() {
                if is_ident(next_ch) {
                    end += next_ch.len_utf8();
                } else {
                    break;
                }
            }
            let ident = &code_slice[start..end];
            let range: Range<usize> = line_start + start..line_start + end;
            // Check member access `recv.ident` first.
            let before = code_slice[..start].trim_end();
            if let Some(recv_part) = before.strip_suffix('.') {
                let recv: String = recv_part
                    .chars()
                    .rev()
                    .take_while(|&c| is_ident(c))
                    .collect::<Vec<_>>()
                    .into_iter()
                    .rev()
                    .collect();
                if let Some(recv_ty) = infer_expr_type(family, &recv, false, symbols) {
                    if let Some((sig, _)) = lookup_member(family, &recv_ty, ident) {
                        hovers.push(HoverAnnotation {
                            range,
                            symbol: ident.to_owned(),
                            signature: sig,
                            docs: None,
                        });
                        idx = end;
                        continue;
                    }
                    if let Some(members) = type_members.get(recv_ty.trim())
                        && let Some(item) = members.iter().find(|m| m.label == ident)
                    {
                        let detail = item.detail.as_deref().unwrap_or("");
                        hovers.push(HoverAnnotation {
                            range,
                            symbol: ident.to_owned(),
                            signature: format!("(property) {ident}{detail}"),
                            docs: None,
                        });
                        idx = end;
                        continue;
                    }
                }
            } else if let Some(sym) = symbols.get(ident) {
                hovers.push(HoverAnnotation {
                    range,
                    symbol: ident.to_owned(),
                    signature: sym.signature.clone(),
                    docs: sym.docs.clone(),
                });
            }
            idx = end;
            continue;
        }
        idx += ch.len_utf8();
    }
}
