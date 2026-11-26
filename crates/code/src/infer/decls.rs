//! Declaration extractors (`interface`, `struct`, `type`, `function`/`fn`, `const`/`let`)
//! and primitive assignment compatibility checks for [`BuiltInAnalyzer`](super::BuiltInAnalyzer).

use std::collections::HashMap;

use super::SymbolInfo;
use super::builtins::{LangFamily, infer_expr_type, split_top_level, widen_literal};
use crate::analyzer::{CompletionKind, DiagnosticAnnotation, Severity};

pub(super) fn parse_type_decl(
    family: LangFamily,
    code: &str,
    docs: &Option<String>,
    symbols: &mut HashMap<String, SymbolInfo>,
) -> Option<String> {
    let clean = code
        .strip_prefix("export ")
        .or_else(|| code.strip_prefix("pub "))
        .unwrap_or(code);
    for kw in ["interface ", "struct ", "enum "] {
        if let Some(rest) = clean.strip_prefix(kw) {
            let name: String = rest
                .chars()
                .take_while(|&c| c.is_alphanumeric() || c == '_')
                .collect();
            if !name.is_empty() {
                let sig = format!("{}{name}", kw);
                symbols.insert(
                    name.clone(),
                    SymbolInfo {
                        kind: CompletionKind::Type,
                        signature: sig,
                        ty: Some(name.clone()),
                        return_type: None,
                        docs: docs.clone(),
                    },
                );
                return Some(name);
            }
        }
    }
    if let Some(rest) = clean.strip_prefix("type ")
        && let Some((lhs, rhs)) = rest.split_once('=')
    {
        let name = lhs.split('<').next().unwrap_or("").trim();
        let rhs_clean = rhs.trim().trim_end_matches(';').trim();
        if is_ident_name(name) {
            symbols.insert(
                name.to_owned(),
                SymbolInfo {
                    kind: CompletionKind::Type,
                    signature: format!("type {lhs}= {rhs_clean}"),
                    ty: Some(rhs_clean.to_owned()),
                    return_type: None,
                    docs: docs.clone(),
                },
            );
            if family == LangFamily::TsJs && rhs_clean.starts_with('{') {
                return Some(name.to_owned());
            }
        }
    }
    None
}

pub(super) fn parse_fn_decl(
    family: LangFamily,
    code: &str,
    lines: &[&str],
    line_idx: usize,
    docs: &Option<String>,
    symbols: &mut HashMap<String, SymbolInfo>,
) -> bool {
    let clean = code
        .strip_prefix("export ")
        .or_else(|| code.strip_prefix("pub "))
        .unwrap_or(code);
    let clean = clean.strip_prefix("async ").unwrap_or(clean);
    let (kw, rest) = if let Some(r) = clean.strip_prefix("function ") {
        ("function", r)
    } else if let Some(r) = clean.strip_prefix("fn ") {
        ("fn", r)
    } else if let Some(r) = clean.strip_prefix("def ") {
        ("def", r)
    } else if let Some(r) = clean.strip_prefix("func ") {
        ("func", r)
    } else {
        return false;
    };

    let Some(open) = rest.find('(') else {
        return false;
    };
    let raw_name = rest[..open].split('<').next().unwrap_or("").trim();
    if !is_ident_name(raw_name) {
        return false;
    }
    let Some(close_rel) = rest[open..].find(')') else {
        return false;
    };
    let close = open + close_rel;
    let params_str = &rest[open + 1..close];
    let mut local_params = symbols.clone();
    for param in split_top_level(params_str, ',') {
        let param = param.trim().strip_prefix("mut ").unwrap_or(param.trim());
        if let Some((pname, pty)) = param.split_once(':') {
            let pname = pname.trim().trim_end_matches('?');
            let pty = pty.trim();
            if is_ident_name(pname) {
                let info = SymbolInfo {
                    kind: CompletionKind::Variable,
                    signature: format!("(parameter) {pname}: {pty}"),
                    ty: Some(pty.to_owned()),
                    return_type: None,
                    docs: None,
                };
                local_params.insert(pname.to_owned(), info.clone());
                symbols.entry(pname.to_owned()).or_insert(info);
            }
        }
    }

    let after_parens = rest[close + 1..].trim().trim_end_matches('{').trim();
    let explicit_ret = after_parens
        .strip_prefix(':')
        .or_else(|| after_parens.strip_prefix("->"))
        .map(|s| s.trim().trim_end_matches(':').trim().to_owned())
        .filter(|s| !s.is_empty());

    let ret_ty = explicit_ret.or_else(|| {
        for body_line in lines.iter().skip(line_idx + 1).take(12) {
            let t = strip_trailing_comment(body_line.trim());
            if t.starts_with('}') {
                break;
            }
            if let Some(ret_expr) = t.strip_prefix("return ")
                && let Some(inferred) = infer_expr_type(family, ret_expr, false, &local_params)
            {
                return Some(widen_literal(family, &inferred));
            }
        }
        None
    });

    let ret_display = ret_ty.clone().unwrap_or_else(|| match family {
        LangFamily::Rust => "()".to_owned(),
        _ => "void".to_owned(),
    });
    let sig = match family {
        LangFamily::Rust => format!("fn {raw_name}({params_str}) -> {ret_display}"),
        LangFamily::Python => format!("def {raw_name}({params_str}) -> {ret_display}"),
        LangFamily::Go => format!("func {raw_name}({params_str}) {ret_display}"),
        _ => format!("{kw} {raw_name}({params_str}): {ret_display}"),
    };
    symbols.insert(
        raw_name.to_owned(),
        SymbolInfo {
            kind: CompletionKind::Function,
            signature: sig,
            ty: Some(format!("({params_str}) => {ret_display}")),
            return_type: Some(ret_display),
            docs: docs.clone(),
        },
    );
    true
}

#[allow(clippy::too_many_arguments)]
pub(super) fn parse_var_decl(
    family: LangFamily,
    code: &str,
    raw_line: &str,
    line_start: usize,
    line_idx: usize,
    docs: &Option<String>,
    symbols: &mut HashMap<String, SymbolInfo>,
    diagnostics: &mut Vec<DiagnosticAnnotation>,
) {
    let clean = code.strip_prefix("export ").unwrap_or(code);
    let (kw, is_const, rest) = if let Some(r) = clean.strip_prefix("const ") {
        ("const", true, r)
    } else if let Some(r) = clean.strip_prefix("let mut ") {
        ("let mut", false, r)
    } else if let Some(r) = clean.strip_prefix("let ") {
        ("let", false, r)
    } else if let Some(r) = clean.strip_prefix("var ") {
        ("var", false, r)
    } else {
        return;
    };

    let Some((lhs, rhs)) = rest.split_once('=') else {
        return;
    };
    if rhs.starts_with('=') {
        return;
    }
    let rhs_expr = rhs.trim().trim_end_matches(';').trim();
    let (name, explicit_ty) = match lhs.split_once(':') {
        Some((n, t)) => (n.trim(), Some(t.trim().to_owned())),
        None => (lhs.trim(), None),
    };
    if !is_ident_name(name) {
        return;
    }

    let inferred_rhs =
        infer_expr_type(family, rhs_expr, is_const && explicit_ty.is_none(), symbols);
    if let (Some(exp), Some(actual)) = (&explicit_ty, &inferred_rhs)
        && is_incompatible_primitive(family, exp, actual)
    {
        let rhs_rel = raw_line.rfind(rhs_expr).unwrap_or(0);
        let range = line_start + rhs_rel..line_start + rhs_rel + rhs_expr.len();
        let (code_id, message) = match family {
            LangFamily::Rust => (
                Some("E0308".to_owned()),
                format!("mismatched types: expected `{exp}`, found `{actual}`"),
            ),
            _ => (
                Some("2322".to_owned()),
                format!("Type '{actual}' is not assignable to type '{exp}'."),
            ),
        };
        diagnostics.push(DiagnosticAnnotation {
            target_range: range,
            after_line: line_idx,
            severity: Severity::Error,
            code: code_id,
            message,
        });
    }

    let final_ty = explicit_ty
        .or(inferred_rhs)
        .unwrap_or_else(|| "unknown".to_owned());
    let return_type = final_ty.rsplit_once("=>").map(|(_, r)| r.trim().to_owned());
    let kind = if return_type.is_some() {
        CompletionKind::Function
    } else {
        CompletionKind::Variable
    };
    symbols.insert(
        name.to_owned(),
        SymbolInfo {
            kind,
            signature: format!("{kw} {name}: {final_ty}"),
            ty: Some(final_ty),
            return_type,
            docs: docs.clone(),
        },
    );
}

fn is_incompatible_primitive(family: LangFamily, expected: &str, actual: &str) -> bool {
    let wide = widen_literal(family, actual);
    match family {
        LangFamily::TsJs => matches!(
            (expected, wide.as_str()),
            ("number", "string" | "boolean")
                | ("string", "number" | "boolean")
                | ("boolean", "number" | "string")
        ),
        LangFamily::Rust => {
            let is_int = matches!(
                expected,
                "u8" | "u16" | "u32" | "u64" | "usize" | "i8" | "i16" | "i32" | "i64" | "isize"
            );
            (is_int && (wide.contains("str") || wide == "String" || wide == "bool"))
                || (matches!(expected, "&str" | "String") && (wide == "i32" || wide == "bool"))
        }
        _ => false,
    }
}

pub(super) fn is_ident_name(s: &str) -> bool {
    !s.is_empty()
        && s.chars().next().is_some_and(|c| !c.is_ascii_digit())
        && s.chars()
            .all(|c| c.is_alphanumeric() || c == '_' || c == '$')
}

pub(super) fn strip_trailing_comment(line: &str) -> &str {
    strip_trailing_comment_preserving_len(line).trim_end()
}

pub(super) fn strip_trailing_comment_preserving_len(line: &str) -> &str {
    let mut in_quote: Option<char> = None;
    let bytes = line.as_bytes();
    for (idx, ch) in line.char_indices() {
        if let Some(q) = in_quote {
            if ch == q && (idx == 0 || bytes[idx - 1] != b'\\') {
                in_quote = None;
            }
            continue;
        }
        match ch {
            '"' | '\'' | '`' => in_quote = Some(ch),
            '/' if line[idx..].starts_with("//") => return &line[..idx],
            _ => {}
        }
    }
    line
}
