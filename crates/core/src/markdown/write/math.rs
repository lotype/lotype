//! Whether TeX can be written as Markdown math and read back unchanged.

/// Whether `tex` has no unescaped `$`, balanced `{`/`}`, and no trailing
/// unescaped `\`.
fn has_balanced_math_delimiters(tex: &str) -> bool {
    let mut escaped = false;
    let mut brace_depth = 0usize;
    for c in tex.chars() {
        if escaped {
            escaped = false;
            continue;
        }
        match c {
            '\\' => escaped = true,
            '$' => return false,
            '{' => brace_depth += 1,
            '}' => {
                let Some(next) = brace_depth.checked_sub(1) else {
                    return false;
                };
                brace_depth = next;
            }
            _ => {}
        }
    }
    !escaped && brace_depth == 0
}

/// Whether `tex` can be emitted verbatim as `$tex$` without breaking
/// `pulldown-cmark`'s `ENABLE_MATH` delimiter or brace-context matching.
pub(super) fn is_inline_math_markdown_safe(tex: &str) -> bool {
    if tex.is_empty()
        || tex.contains(['\n', '\r'])
        || tex.starts_with(|c: char| c.is_ascii_whitespace())
        || tex.ends_with(|c: char| c.is_ascii_whitespace())
    {
        return false;
    }
    has_balanced_math_delimiters(tex)
}

/// Whether `tex` can be emitted verbatim as `$$\n{tex}\n$$` without blank
/// lines, CommonMark paragraph-interrupting constructs, or math delimiter /
/// brace mismatches breaking the block on re-parse.
pub(super) fn is_display_math_markdown_safe(tex: &str) -> bool {
    if tex.is_empty() || tex.contains('\r') || tex.trim() != tex {
        return false;
    }
    if !has_balanced_math_delimiters(tex) {
        return false;
    }
    tex.split('\n').all(is_paragraph_continuation_line)
}

/// Whether `line` stays inside an open CommonMark / GFM paragraph between
/// opening and closing `$$` lines.
fn is_paragraph_continuation_line(line: &str) -> bool {
    let trimmed = line.trim_matches([' ', '\t']);
    if trimmed.is_empty() {
        return false;
    }
    let s = line.trim_start_matches([' ', '\t']);
    if s.starts_with('>')
        || s.starts_with("```")
        || s.starts_with("~~~")
        || (s.starts_with('<')
            && s.as_bytes()
                .get(1)
                .is_some_and(|b| b.is_ascii_alphabetic() || matches!(b, b'/' | b'!' | b'?')))
    {
        return false;
    }
    let hashes = s.bytes().take_while(|&b| b == b'#').count();
    if (1..=6).contains(&hashes)
        && s.as_bytes()
            .get(hashes)
            .is_none_or(|&b| matches!(b, b' ' | b'\t'))
    {
        return false;
    }
    if (s.starts_with('-') || s.starts_with('+') || s.starts_with('*'))
        && s.as_bytes()
            .get(1)
            .is_some_and(|&b| matches!(b, b' ' | b'\t'))
    {
        return false;
    }
    let digits = s.bytes().take_while(u8::is_ascii_digit).count();
    if (1..=9).contains(&digits)
        && s.as_bytes()
            .get(digits)
            .is_some_and(|&b| matches!(b, b'.' | b')'))
        && s.as_bytes()
            .get(digits + 1)
            .is_none_or(|&b| matches!(b, b' ' | b'\t'))
    {
        return false;
    }
    // Reject Setext underlines (`===`, `---`), thematic breaks (`***`, `___`, `- - -`),
    // and GFM table delimiter rows (`| --- |`).
    let mut eq_only = true;
    let mut dash_only = true;
    let mut star_only = true;
    let mut under_only = true;
    let mut table_delim = s.contains('|');
    let mut has_dash = false;
    let mut non_ws_count = 0usize;
    for b in trimmed.bytes() {
        if matches!(b, b' ' | b'\t') {
            continue;
        }
        non_ws_count += 1;
        eq_only &= b == b'=';
        dash_only &= b == b'-';
        star_only &= b == b'*';
        under_only &= b == b'_';
        table_delim &= matches!(b, b'|' | b'-' | b':');
        has_dash |= b == b'-';
    }
    !(eq_only
        || dash_only
        || (non_ws_count >= 3 && (star_only || under_only))
        || (table_delim && has_dash))
}
