//! Incremental Tree-sitter syntax highlighting and Twoslash text decorations.

mod cards;

use std::collections::HashMap;
use std::ops::Range;
use std::sync::Arc;

use gpui::{
    App, FontWeight, HighlightStyle, Hsla, SharedString, StyledText, TextRun, UnderlineStyle, px,
};
use gpui_component::ActiveTheme as _;
use gpui_component::highlighter::{HighlightTheme, SyntaxHighlighter};
use gpui_component::input::Rope;
use lotype_core::BlockId;
use lotype_ui::{Interline, LineDecoration, TextDecoration};
use tree_sitter::{InputEdit, Point};

use crate::analyzer::Severity;
use crate::languages::canonical_language;
use crate::twoslash::{LineTag, ParsedCode};
use cards::{
    render_completion_dropdown, render_cut_divider, render_diagnostic_banner, render_file_tab,
    render_query_callout,
};

pub(crate) const CODE_FONT: &str = "Menlo";

struct CachedHighlight {
    language: String,
    text: String,
    theme: Arc<HighlightTheme>,
    highlighter: SyntaxHighlighter,
    styles: Vec<(Range<usize>, HighlightStyle)>,
}

/// Per-view cache of Tree-sitter highlighters keyed by [`BlockId`].
#[derive(Default)]
pub(crate) struct HighlightCache {
    blocks: HashMap<BlockId, CachedHighlight>,
    snippets: HashMap<String, (String, SyntaxHighlighter)>,
}

impl HighlightCache {
    /// Drops cached highlighters for blocks that no longer exist in the document.
    pub(crate) fn retain_blocks(&mut self, keep: impl Fn(&BlockId) -> bool) {
        self.blocks.retain(|id, _| keep(id));
    }

    /// Highlights `text` in `language`, reusing the incremental syntax tree when edited.
    pub(crate) fn highlight_block(
        &mut self,
        block: BlockId,
        language: Option<&str>,
        text: &str,
        theme: &Arc<HighlightTheme>,
    ) -> Vec<(Range<usize>, HighlightStyle)> {
        let canonical = language
            .map(canonical_language)
            .unwrap_or_else(|| "text".to_owned());
        if canonical == "text" || text.is_empty() {
            self.blocks.remove(&block);
            return Vec::new();
        }
        if let Some(entry) = self.blocks.get_mut(&block)
            && entry.language == canonical
        {
            if entry.text == text && Arc::ptr_eq(&entry.theme, theme) {
                return entry.styles.clone();
            }
            if entry.text != text {
                let edit = compute_input_edit(&entry.text, text);
                let next_rope = Rope::from_str(text);
                entry.highlighter.update(Some(edit), &next_rope, None);
                entry.text.clear();
                entry.text.push_str(text);
            }
            entry.theme = Arc::clone(theme);
            entry.styles = entry.highlighter.styles(&(0..text.len()), theme.as_ref());
            return entry.styles.clone();
        }

        let rope = Rope::from_str(text);
        let mut highlighter = SyntaxHighlighter::new(&canonical);
        highlighter.update(None, &rope, None);
        let styles = highlighter.styles(&(0..text.len()), theme.as_ref());
        self.blocks.insert(
            block,
            CachedHighlight {
                language: canonical,
                text: text.to_owned(),
                theme: Arc::clone(theme),
                highlighter,
                styles: styles.clone(),
            },
        );
        styles
    }

    /// Highlights a short one-line or multi-line type signature for Twoslash callouts and hover cards.
    pub(crate) fn highlight_snippet(
        &mut self,
        language: Option<&str>,
        snippet: &str,
        cx: &App,
    ) -> StyledText {
        let base_color = cx.theme().foreground;
        let canonical = language
            .map(canonical_language)
            .unwrap_or_else(|| "text".to_owned());
        if canonical == "text" || snippet.is_empty() {
            return StyledText::new(SharedString::from(snippet.to_owned()));
        }
        let rope = Rope::from_str(snippet);
        let highlighter = match self.snippets.get_mut(&canonical) {
            Some((prev, highlighter)) => {
                if prev != snippet {
                    let edit = compute_input_edit(prev, snippet);
                    highlighter.update(Some(edit), &rope, None);
                    prev.clear();
                    prev.push_str(snippet);
                }
                highlighter
            }
            None => {
                let mut highlighter = SyntaxHighlighter::new(&canonical);
                highlighter.update(None, &rope, None);
                &mut self
                    .snippets
                    .entry(canonical)
                    .or_insert((snippet.to_owned(), highlighter))
                    .1
            }
        };
        let styles = highlighter.styles(&(0..snippet.len()), cx.theme().highlight_theme.as_ref());
        let runs = styles_to_runs(snippet.len(), base_color, &styles);
        StyledText::new(SharedString::from(snippet.to_owned())).with_runs(runs)
    }
}

fn styles_to_runs(
    len: usize,
    base_color: Hsla,
    styles: &[(Range<usize>, HighlightStyle)],
) -> Vec<TextRun> {
    let base_font = gpui::Font {
        family: CODE_FONT.into(),
        features: Default::default(),
        fallbacks: None,
        weight: FontWeight::NORMAL,
        style: gpui::FontStyle::Normal,
    };
    let plain = |run_len| TextRun {
        len: run_len,
        font: base_font.clone(),
        color: base_color,
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    let mut runs = Vec::new();
    let mut cursor = 0;
    for (range, style) in styles {
        let start = range.start.min(len);
        let end = range.end.min(len);
        if start > cursor {
            runs.push(plain(start - cursor));
        }
        if end > start {
            let mut font = base_font.clone();
            if let Some(weight) = style.font_weight {
                font.weight = weight;
            }
            if let Some(font_style) = style.font_style {
                font.style = font_style;
            }
            runs.push(TextRun {
                len: end - start,
                font,
                color: style.color.unwrap_or(base_color),
                background_color: style.background_color,
                underline: style.underline,
                strikethrough: style.strikethrough,
            });
            cursor = end;
        }
    }
    if cursor < len {
        runs.push(plain(len - cursor));
    }
    if runs.is_empty() && len > 0 {
        runs.push(plain(len));
    }
    runs
}

fn byte_to_point(text: &str, offset: usize) -> Point {
    let slice = &text[..offset];
    let row = slice.bytes().filter(|&b| b == b'\n').count();
    let line_start = slice.rfind('\n').map_or(0, |idx| idx + 1);
    Point::new(row, offset - line_start)
}

fn compute_input_edit(old_text: &str, new_text: &str) -> InputEdit {
    let prefix = old_text
        .chars()
        .zip(new_text.chars())
        .take_while(|(a, b)| a == b)
        .map(|(ch, _)| ch.len_utf8())
        .sum::<usize>();
    let suffix = old_text[prefix..]
        .chars()
        .rev()
        .zip(new_text[prefix..].chars().rev())
        .take_while(|(a, b)| a == b)
        .map(|(ch, _)| ch.len_utf8())
        .sum::<usize>();
    let old_end = old_text.len() - suffix;
    let new_end = new_text.len() - suffix;
    InputEdit {
        start_byte: prefix,
        old_end_byte: old_end,
        new_end_byte: new_end,
        start_position: byte_to_point(old_text, prefix),
        old_end_position: byte_to_point(old_text, old_end),
        new_end_position: byte_to_point(new_text, new_end),
    }
}

/// Converts syntax highlights and [`ParsedCode`] into a [`TextDecoration`] for `lotype-ui`.
pub(crate) fn build_text_decoration(
    cache: &mut HighlightCache,
    block: BlockId,
    language: Option<&str>,
    text: &str,
    parsed: &ParsedCode,
    editing: bool,
    cx: &App,
) -> TextDecoration {
    let theme = cx.theme().highlight_theme.clone();
    let mut highlights = cache.highlight_block(block, language, text, &theme);

    let total_lines = text.split('\n').count();
    // Fold directive and cut lines when the caret is outside the block, keeping at least
    // one line clickable if the entire block currently consists of directives.
    let fold = !editing && parsed.hidden_in_preview.len() < total_lines;

    for range in &parsed.word_highlights {
        if range.start < range.end
            && text.is_char_boundary(range.start)
            && text.is_char_boundary(range.end)
        {
            highlights.push((
                range.clone(),
                HighlightStyle {
                    background_color: Some(cx.theme().primary.opacity(0.18)),
                    ..Default::default()
                },
            ));
        }
    }

    for diagnostic in &parsed.diagnostics {
        if fold && parsed.is_cut_line(diagnostic.after_line) {
            continue;
        }
        let range = &diagnostic.target_range;
        if range.start >= range.end
            || !text.is_char_boundary(range.start)
            || !text.is_char_boundary(range.end)
        {
            continue;
        }
        let color = severity_ink(diagnostic.severity, cx);
        highlights.push((
            range.clone(),
            HighlightStyle {
                underline: Some(UnderlineStyle {
                    color: Some(color),
                    thickness: px(1.5),
                    wavy: true,
                }),
                ..Default::default()
            },
        ));
    }

    let lines = parsed
        .lines
        .iter()
        .filter(|marker| !(fold && parsed.is_cut_line(marker.line)))
        .map(|marker| line_decoration(marker.line, marker.tag, cx))
        .collect();

    let mut interlines = Vec::new();
    if !fold {
        for &(line, kind) in &parsed.cuts {
            interlines.push(Interline {
                after_line: line,
                element: render_cut_divider(kind, cx),
            });
        }
    }
    let visible_files: Vec<_> = parsed
        .files
        .iter()
        .filter(|file| !(fold && parsed.is_cut_line(file.header_line + 1)))
        .collect();
    if !fold || visible_files.len() > 1 {
        for file in visible_files {
            interlines.push(Interline {
                after_line: file.header_line,
                element: render_file_tab(&file.name, cx),
            });
        }
    }
    for query in &parsed.queries {
        if fold && parsed.is_cut_line(query.target_line) {
            continue;
        }
        interlines.push(Interline {
            after_line: query.comment_line,
            element: render_query_callout(cache, language, query, fold, cx),
        });
    }
    for comp in &parsed.completions {
        if fold && parsed.is_cut_line(comp.target_line) {
            continue;
        }
        interlines.push(Interline {
            after_line: comp.comment_line,
            element: render_completion_dropdown(comp, fold, cx),
        });
    }
    for diagnostic in &parsed.diagnostics {
        if fold && parsed.is_cut_line(diagnostic.after_line) {
            continue;
        }
        interlines.push(Interline {
            after_line: diagnostic.after_line,
            element: render_diagnostic_banner(diagnostic, cx),
        });
    }

    let (hidden_lines, trimmed_lines) = if fold {
        (
            parsed.hidden_in_preview.clone(),
            parsed.trimmed_in_preview.clone(),
        )
    } else {
        (Vec::new(), Vec::new())
    };

    TextDecoration {
        highlights,
        lines,
        interlines,
        hidden_lines,
        trimmed_lines,
    }
}

fn line_decoration(line: usize, tag: LineTag, cx: &App) -> LineDecoration {
    let theme = cx.theme();
    match tag {
        LineTag::Highlight => LineDecoration {
            line,
            background: Some(theme.primary.opacity(0.10)),
            bar: Some(theme.primary),
            badge: None,
        },
        LineTag::Added => LineDecoration {
            line,
            background: Some(theme.green.opacity(0.14)),
            bar: Some(theme.green),
            badge: Some(("+".into(), theme.green)),
        },
        LineTag::Removed => LineDecoration {
            line,
            background: Some(theme.red.opacity(0.14)),
            bar: Some(theme.red),
            badge: Some(("-".into(), theme.red)),
        },
        LineTag::Error => {
            let color = severity_ink(Severity::Error, cx);
            LineDecoration {
                line,
                background: Some(color.opacity(0.14)),
                bar: Some(color),
                badge: Some(("!".into(), color)),
            }
        }
        LineTag::Warning => {
            let color = severity_ink(Severity::Warning, cx);
            LineDecoration {
                line,
                background: Some(color.opacity(0.14)),
                bar: Some(color),
                badge: None,
            }
        }
    }
}

pub(crate) fn severity_ink(severity: Severity, cx: &App) -> Hsla {
    let status = &cx.theme().highlight_theme.style.status;
    match severity {
        Severity::Error => status.error(cx),
        Severity::Warning => status.warning(cx),
        Severity::Info => status.info(cx),
        Severity::Hint => status.hint(cx),
    }
}
