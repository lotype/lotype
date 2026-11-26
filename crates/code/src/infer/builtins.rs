//! Built-in language member tables and expression type inference helpers.

use std::collections::HashMap;

use crate::analyzer::{CompletionItem, CompletionKind};
use crate::infer::SymbolInfo;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum LangFamily {
    TsJs,
    Rust,
    Python,
    Go,
    Other,
}

impl LangFamily {
    pub(super) fn of(canonical: &str) -> Self {
        match canonical {
            "typescript" | "tsx" | "javascript" => Self::TsJs,
            "rust" => Self::Rust,
            "python" => Self::Python,
            "go" => Self::Go,
            _ => Self::Other,
        }
    }
}

/// Infers the type of an expression `expr` using known `symbols` and language rules.
pub(super) fn infer_expr_type(
    family: LangFamily,
    expr: &str,
    is_const: bool,
    symbols: &HashMap<String, SymbolInfo>,
) -> Option<String> {
    let expr = expr.trim().trim_end_matches(';').trim();
    if expr.is_empty() {
        return None;
    }
    if let Some(awaited) = expr.strip_prefix("await ") {
        let inner = infer_expr_type(family, awaited, false, symbols)?;
        if let Some(unwrapped) = inner
            .strip_prefix("Promise<")
            .and_then(|s| s.strip_suffix('>'))
        {
            return Some(unwrapped.to_owned());
        }
        return Some(inner);
    }
    // String literals.
    if (expr.starts_with('"') && expr.ends_with('"') && expr.len() >= 2)
        || (expr.starts_with('\'') && expr.ends_with('\'') && expr.len() >= 2)
        || (expr.starts_with('`') && expr.ends_with('`') && expr.len() >= 2)
    {
        return Some(match family {
            LangFamily::TsJs if is_const && !expr.starts_with('`') => expr.to_owned(),
            LangFamily::TsJs => "string".to_owned(),
            LangFamily::Rust => "&'static str".to_owned(),
            LangFamily::Python => "str".to_owned(),
            LangFamily::Go => "string".to_owned(),
            LangFamily::Other => "string".to_owned(),
        });
    }
    // Booleans.
    if matches!(expr, "true" | "false") {
        return Some(match family {
            LangFamily::TsJs if is_const => expr.to_owned(),
            LangFamily::TsJs => "boolean".to_owned(),
            LangFamily::Rust | LangFamily::Go => "bool".to_owned(),
            LangFamily::Python => "bool".to_owned(),
            LangFamily::Other => "bool".to_owned(),
        });
    }
    if matches!(expr, "True" | "False") && family == LangFamily::Python {
        return Some("bool".to_owned());
    }
    // Numeric literals.
    if let Some(num_ty) = infer_number_literal(family, expr, is_const) {
        return Some(num_ty);
    }
    // Rust `vec![...]` macro.
    if family == LangFamily::Rust
        && let Some(inner) = expr.strip_prefix("vec![").and_then(|s| s.strip_suffix(']'))
    {
        let elem = inner
            .split(',')
            .next()
            .and_then(|first| infer_expr_type(family, first, false, symbols))
            .unwrap_or_else(|| "_".to_owned());
        return Some(format!("Vec<{elem}>"));
    }
    // Array / list literals `[a, b]`.
    if let Some(inner) = expr.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
        let items: Vec<&str> = split_top_level(inner, ',');
        let elem = items
            .first()
            .and_then(|first| infer_expr_type(family, first, false, symbols))
            .map(|ty| widen_literal(family, &ty))
            .unwrap_or_else(|| "unknown".to_owned());
        return Some(match family {
            LangFamily::TsJs => format!("{elem}[]"),
            LangFamily::Rust => format!("[{elem}; {}]", items.len()),
            LangFamily::Python => format!("list[{elem}]"),
            _ => format!("{elem}[]"),
        });
    }
    // Object literal `{ a: 1, b: "x" }` in TS/JS.
    if family == LangFamily::TsJs
        && let Some(inner) = expr.strip_prefix('{').and_then(|s| s.strip_suffix('}'))
    {
        let mut fields = Vec::new();
        for part in split_top_level(inner, ',') {
            let part = part.trim();
            if part.is_empty() {
                continue;
            }
            if let Some((k, v)) = part.split_once(':') {
                let fty = infer_expr_type(family, v, false, symbols)
                    .map(|ty| widen_literal(family, &ty))
                    .unwrap_or_else(|| "unknown".to_owned());
                fields.push(format!("{}: {fty}", k.trim()));
            }
        }
        if !fields.is_empty() {
            return Some(format!("{{ {} }}", fields.join("; ")));
        }
    }
    // Arrow function `(a: number) => expr` in TS/JS.
    if family == LangFamily::TsJs
        && let Some((params, body)) = expr.split_once("=>")
    {
        let params = params.trim();
        let ret = infer_expr_type(family, body, false, symbols)
            .map(|ty| widen_literal(family, &ty))
            .unwrap_or_else(|| "void".to_owned());
        let formatted_params = if params.starts_with('(') {
            params.to_owned()
        } else {
            format!("({params}: unknown)")
        };
        return Some(format!("{formatted_params} => {ret}"));
    }
    // Direct symbol lookup.
    if let Some(sym) = symbols.get(expr) {
        return sym.ty.clone();
    }
    // Function or method call `callee(...)`.
    if let Some(open) = expr.rfind('(')
        && expr.ends_with(')')
    {
        let callee = expr[..open].trim();
        if let Some(sym) = symbols.get(callee) {
            return sym.return_type.clone().or_else(|| sym.ty.clone());
        }
        if let Some((recv, method)) = callee.rsplit_once('.')
            && let Some(recv_ty) = infer_expr_type(family, recv, false, symbols)
            && let Some((_, ret)) = lookup_member(family, &recv_ty, method.trim())
        {
            return Some(ret);
        }
        if family == LangFamily::Rust && matches!(callee, "String::from" | "format!") {
            return Some("String".to_owned());
        }
    }
    // Property access `recv.prop`.
    if let Some((recv, prop)) = expr.rsplit_once('.') {
        let prop = prop.trim();
        if let Some(recv_ty) = infer_expr_type(family, recv, false, symbols)
            && let Some((_, ty)) = lookup_member(family, &recv_ty, prop)
        {
            return Some(ty);
        }
    }
    None
}

pub(super) fn widen_literal(family: LangFamily, ty: &str) -> String {
    if family != LangFamily::TsJs {
        return ty.to_owned();
    }
    if (ty.starts_with('"') && ty.ends_with('"')) || (ty.starts_with('\'') && ty.ends_with('\'')) {
        return "string".to_owned();
    }
    if matches!(ty, "true" | "false") {
        return "boolean".to_owned();
    }
    if ty.parse::<f64>().is_ok() {
        return "number".to_owned();
    }
    ty.to_owned()
}

fn infer_number_literal(family: LangFamily, expr: &str, is_const: bool) -> Option<String> {
    if family == LangFamily::Rust {
        for suffix in [
            "usize", "isize", "u64", "u32", "u16", "u8", "i64", "i32", "i16", "i8", "f64", "f32",
        ] {
            if let Some(num) = expr.strip_suffix(suffix) {
                let clean = num.trim_end_matches('_');
                if !clean.is_empty() && clean.chars().all(|c| c.is_ascii_digit() || c == '.') {
                    return Some(suffix.to_owned());
                }
            }
        }
    }
    let clean: String = expr.chars().filter(|&c| c != '_').collect();
    if clean.is_empty() {
        return None;
    }
    if clean.chars().all(|c| c.is_ascii_digit()) {
        return Some(match family {
            LangFamily::TsJs if is_const => expr.to_owned(),
            LangFamily::TsJs => "number".to_owned(),
            LangFamily::Rust => "i32".to_owned(),
            LangFamily::Python | LangFamily::Go => "int".to_owned(),
            LangFamily::Other => "number".to_owned(),
        });
    }
    if clean.contains('.') && clean.parse::<f64>().is_ok() {
        return Some(match family {
            LangFamily::TsJs if is_const => expr.to_owned(),
            LangFamily::TsJs => "number".to_owned(),
            LangFamily::Rust => "f64".to_owned(),
            LangFamily::Python => "float".to_owned(),
            LangFamily::Go => "float64".to_owned(),
            LangFamily::Other => "number".to_owned(),
        });
    }
    None
}

pub(super) fn split_top_level(input: &str, sep: char) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut depth = 0i32;
    let mut start = 0;
    let mut in_quote: Option<char> = None;
    for (idx, ch) in input.char_indices() {
        if let Some(q) = in_quote {
            if ch == q {
                in_quote = None;
            }
            continue;
        }
        match ch {
            '"' | '\'' | '`' => in_quote = Some(ch),
            '(' | '[' | '{' | '<' => depth += 1,
            ')' | ']' | '}' | '>' => depth = (depth - 1).max(0),
            c if c == sep && depth == 0 => {
                parts.push(input[start..idx].trim());
                start = idx + ch.len_utf8();
            }
            _ => {}
        }
    }
    let tail = input[start..].trim();
    if !tail.is_empty() {
        parts.push(tail);
    }
    parts
}

/// Returns `(full_signature, result_type)` for built-in members on `receiver_ty`.
pub(super) fn lookup_member(
    family: LangFamily,
    receiver_ty: &str,
    member: &str,
) -> Option<(String, String)> {
    for item in builtin_members_for_type(family, receiver_ty) {
        if item.label == member {
            let detail = item.detail.unwrap_or_else(|| "unknown".to_owned());
            let ret = detail
                .rsplit_once(':')
                .or_else(|| detail.rsplit_once("->"))
                .map_or_else(|| detail.clone(), |(_, r)| r.trim().to_owned());
            let prefix = match item.kind {
                CompletionKind::Function => "(method)",
                _ => "(property)",
            };
            return Some((format!("{prefix} {member}{detail}"), ret));
        }
    }
    None
}

/// Built-in member completions for primitive and collection types.
pub(super) fn builtin_members_for_type(family: LangFamily, raw_ty: &str) -> Vec<CompletionItem> {
    let ty = widen_literal(family, raw_ty.trim());
    // Parse inline TS object type `{ id: number; name: string }`.
    if let Some(inner) = ty.strip_prefix('{').and_then(|s| s.strip_suffix('}')) {
        return inner
            .split(';')
            .filter_map(|field| {
                let (name, fty) = field.trim().split_once(':')?;
                Some(CompletionItem {
                    label: name.trim().to_owned(),
                    kind: CompletionKind::Property,
                    detail: Some(format!(": {}", fty.trim())),
                })
            })
            .collect();
    }
    match family {
        LangFamily::TsJs => {
            if ty == "string" {
                return vec![
                    prop("length", ": number"),
                    method("slice", "(start?: number, end?: number): string"),
                    method("split", "(separator: string): string[]"),
                    method("trim", "(): string"),
                    method("toUpperCase", "(): string"),
                    method("toLowerCase", "(): string"),
                    method("includes", "(searchString: string): boolean"),
                    method("startsWith", "(searchString: string): boolean"),
                    method(
                        "replace",
                        "(searchValue: string, replaceValue: string): string",
                    ),
                ];
            }
            if ty == "number" {
                return vec![
                    method("toFixed", "(fractionDigits?: number): string"),
                    method("toString", "(radix?: number): string"),
                    method("toPrecision", "(precision?: number): string"),
                    method("valueOf", "(): number"),
                ];
            }
            if let Some(elem) = ty.strip_suffix("[]") {
                return vec![
                    prop("length", ": number"),
                    method("map", &format!("<U>(fn: (item: {elem}) => U): U[]")),
                    method(
                        "filter",
                        &format!("(fn: (item: {elem}) => boolean): {elem}[]"),
                    ),
                    method(
                        "find",
                        &format!("(fn: (item: {elem}) => boolean): {elem} | undefined"),
                    ),
                    method("push", &format!("(...items: {elem}[]): number")),
                    method(
                        "slice",
                        &format!("(start?: number, end?: number): {elem}[]"),
                    ),
                    method("join", "(separator?: string): string"),
                    method("some", &format!("(fn: (item: {elem}) => boolean): boolean")),
                    method(
                        "every",
                        &format!("(fn: (item: {elem}) => boolean): boolean"),
                    ),
                ];
            }
            Vec::new()
        }
        LangFamily::Rust => {
            if matches!(ty.as_str(), "&str" | "&'static str" | "String") {
                return vec![
                    method("len", "(&self) -> usize"),
                    method("is_empty", "(&self) -> bool"),
                    method("trim", "(&self) -> &str"),
                    method("contains", "(&self, pat: &str) -> bool"),
                    method("starts_with", "(&self, pat: &str) -> bool"),
                    method("lines", "(&self) -> Lines<'_>"),
                    method("to_owned", "(&self) -> String"),
                ];
            }
            if ty.starts_with("Vec<") || ty.starts_with('[') {
                return vec![
                    method("len", "(&self) -> usize"),
                    method("is_empty", "(&self) -> bool"),
                    method("iter", "(&self) -> Iter<'_, T>"),
                    method("push", "(&mut self, value: T) -> ()"),
                    method("first", "(&self) -> Option<&T>"),
                    method("last", "(&self) -> Option<&T>"),
                ];
            }
            Vec::new()
        }
        _ => Vec::new(),
    }
}

pub(super) fn language_keywords(family: LangFamily) -> &'static [&'static str] {
    match family {
        LangFamily::TsJs => &[
            "const",
            "let",
            "function",
            "interface",
            "type",
            "return",
            "async",
            "await",
            "export",
            "import",
        ],
        LangFamily::Rust => &[
            "let", "fn", "pub", "struct", "enum", "impl", "match", "return", "async", "await",
        ],
        LangFamily::Python => &["def", "class", "return", "async", "await", "import", "from"],
        LangFamily::Go => &[
            "func",
            "type",
            "struct",
            "interface",
            "return",
            "defer",
            "go",
        ],
        LangFamily::Other => &[],
    }
}

fn prop(name: &str, detail: &str) -> CompletionItem {
    CompletionItem {
        label: name.to_owned(),
        kind: CompletionKind::Property,
        detail: Some(detail.to_owned()),
    }
}

fn method(name: &str, detail: &str) -> CompletionItem {
    CompletionItem {
        label: name.to_owned(),
        kind: CompletionKind::Function,
        detail: Some(detail.to_owned()),
    }
}
