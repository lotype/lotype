//! How marks look: text runs for GPUI from a block's spans.

use gpui::{
    FontStyle, FontWeight, HighlightStyle, Hsla, Rgba, StrikethroughStyle, TextRun, TextStyle,
    UnderlineStyle, px, rgb,
};
use lotype_core::{Span, Value, mark};

pub(crate) const CODE_FONT: &str = "Menlo";

/// How the core's named colors look, as BlockNote draws them
/// (`defaultColors.ts`), so that documents written with it look the same:
/// text then background, in the light and in the dark color scheme.
fn named(name: &str, dark: bool) -> Option<(u32, u32)> {
    let (light, night) = match name {
        "gray" => ((0x676767, 0xe8e8e8), (0xa3a3a3, 0x383838)),
        "brown" => ((0x7b5540, 0xf1e5df), (0xc79d86, 0x473328)),
        "red" => ((0x994a37, 0xffdfd6), (0xe69783, 0x5a2619)),
        "orange" => ((0x8f5509, 0xfbe3cd), (0xdaa168, 0x532d00)),
        "yellow" => ((0x796300, 0xf0e8cb), (0xc2ad61, 0x453700)),
        "green" => ((0x48722d, 0xddeed4), (0x92bc7c, 0x254112)),
        "blue" => ((0x006d9a, 0xcfedff), (0x6ab8e4, 0x003d5b)),
        "purple" => ((0x6f549a, 0xece2ff), (0xb7a0e4, 0x3f2d5b)),
        "pink" => ((0x904873, 0xfddeee), (0xdc94bc, 0x542541)),
        _ => return None,
    };
    Some(if dark { night } else { light })
}

/// The colors of links and of inline code, from the theme.
#[derive(Clone, Copy, Default)]
pub(crate) struct Inks {
    pub link: Hsla,
    pub code: Hsla,
    /// The theme is dark, for the named colors.
    pub dark: bool,
}

/// One run per span, styled by its marks on top of `base`.
pub(crate) fn text_runs(spans: &[Span], base: &TextStyle, inks: Inks) -> Vec<TextRun> {
    let spans = spans.iter().filter(|span| !span.text.is_empty());
    spans.map(|span| text_run(span, base, inks)).collect()
}

/// Effective [`TextStyle`] for `span` with its marks applied on top of `base`.
pub(crate) fn span_text_style(span: &Span, base: &TextStyle, inks: Inks) -> TextStyle {
    let mut style = base.clone().highlight(span_highlight(span, inks));
    if span.marks.contains_key(mark::CODE) {
        style.font_family = CODE_FONT.into();
    }
    style
}

fn text_run(span: &Span, base: &TextStyle, inks: Inks) -> TextRun {
    let mut run = base
        .clone()
        .highlight(span_highlight(span, inks))
        .to_run(span.text.len());
    if span.marks.contains_key(mark::CODE) {
        run.font.family = CODE_FONT.into();
    }
    run
}

fn span_highlight(span: &Span, inks: Inks) -> HighlightStyle {
    let mut style = HighlightStyle::default();
    let line = |color| UnderlineStyle {
        color,
        thickness: px(1.),
        wavy: false,
    };
    let link = inks.link;
    // Marks come sorted by name, so an explicit color wins over the link's.
    for (name, value) in &span.marks {
        match name.as_str() {
            mark::BACKGROUND_COLOR => style.background_color = color(value, true, inks.dark),
            mark::BOLD => style.font_weight = Some(FontWeight::BOLD),
            mark::CODE => {
                style.background_color.get_or_insert(inks.code);
            }
            mark::ITALIC => style.font_style = Some(FontStyle::Italic),
            mark::LINK => {
                style.color = Some(link);
                style.underline = Some(line(Some(link)));
            }
            mark::STRIKE => {
                let (thickness, color) = (px(1.), None);
                style.strikethrough = Some(StrikethroughStyle { thickness, color });
            }
            mark::TEXT_COLOR => style.color = color(value, false, inks.dark).or(style.color),
            mark::UNDERLINE => style.underline = Some(line(None)),
            _ => {}
        }
    }
    style
}

/// Splits and styles `runs` with `highlights`, preserving existing run properties
/// where a highlight leaves a field unset. Offsets that fall inside a UTF-8
/// code point are snapped down with `floor_char_boundary`.
pub(crate) fn apply_highlights(
    text: &str,
    runs: Vec<TextRun>,
    highlights: &[(std::ops::Range<usize>, HighlightStyle)],
) -> Vec<TextRun> {
    let text_len = text.len();
    if highlights.is_empty() || runs.is_empty() || text_len == 0 {
        return runs;
    }
    let normalized: Vec<(std::ops::Range<usize>, &HighlightStyle)> = highlights
        .iter()
        .filter_map(|(range, style)| {
            let start = text.floor_char_boundary(range.start.min(text_len));
            let end = text.floor_char_boundary(range.end.min(text_len));
            (start < end).then_some((start..end, style))
        })
        .collect();
    if normalized.is_empty() {
        return runs;
    }
    let mut cuts = Vec::with_capacity(runs.len() * 2 + normalized.len() * 2 + 2);
    cuts.push(0);
    cuts.push(text_len);
    let mut pos = 0;
    for run in &runs {
        pos += run.len;
        cuts.push(pos.min(text_len));
    }
    for (range, _) in &normalized {
        cuts.push(range.start);
        cuts.push(range.end);
    }
    cuts.sort_unstable();
    cuts.dedup();

    let mut run_starts = Vec::with_capacity(runs.len());
    let mut at = 0;
    for run in &runs {
        run_starts.push(at);
        at += run.len;
    }

    let mut out: Vec<TextRun> = Vec::with_capacity(cuts.len());
    for pair in cuts.windows(2) {
        let (start, end) = (pair[0], pair[1]);
        if start >= end {
            continue;
        }
        let base_idx = run_starts
            .partition_point(|&run_start| run_start <= start)
            .saturating_sub(1);
        let mut run = TextRun {
            len: end - start,
            ..runs[base_idx].clone()
        };
        for (range, style) in &normalized {
            if range.start <= start && end <= range.end {
                if let Some(color) = style.color {
                    run.color = color;
                }
                if let Some(bg) = style.background_color {
                    run.background_color = Some(bg);
                }
                if let Some(weight) = style.font_weight {
                    run.font.weight = weight;
                }
                if let Some(font_style) = style.font_style {
                    run.font.style = font_style;
                }
                if let Some(underline) = style.underline {
                    run.underline = Some(underline);
                }
                if let Some(strike) = style.strikethrough {
                    run.strikethrough = Some(strike);
                }
            }
        }
        if let Some(last) = out.last_mut() {
            let merged = TextRun {
                len: last.len,
                ..run.clone()
            };
            if *last == merged {
                last.len += run.len;
                continue;
            }
        }
        out.push(run);
    }
    out
}

/// A color mark's value: one of BlockNote's names, in the light or the
/// dark color scheme, or a CSS hex color.
pub(crate) fn color(value: &Value, background: bool, dark: bool) -> Option<Hsla> {
    let Value::Str(name) = value else { return None };
    // Longer than `#rrggbbaa` is no color, and a peer's long text is not parsed.
    if name.len() > 9 {
        return None;
    }
    let named = named(name, dark).map(|(text, fill)| rgb(if background { fill } else { text }));
    let hex = || Rgba::try_from(name.as_str()).ok();
    named.or_else(hex).map(Hsla::from)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;

    fn span(text: &str, marks: &[(&str, Value)]) -> Span {
        let marks = marks
            .iter()
            .map(|(name, value)| (name.to_string(), value.clone()));
        Span::new(text, marks.collect::<BTreeMap<_, _>>())
    }

    #[test]
    fn every_named_color_of_the_core_looks_like_something() {
        for name in lotype_core::COLOR_NAMES {
            assert!(
                named(name, false).is_some() && named(name, true).is_some(),
                "{name}"
            );
        }
    }

    #[test]
    fn marks_style_their_run_only() {
        let base = TextStyle::default();
        let spans = [
            span("plain ", &[]),
            span("", &[("bold", true.into())]),
            span("bold", &[("bold", true.into()), ("italic", true.into())]),
            span("x", &[("code", true.into())]),
        ];
        let runs = text_runs(&spans, &base, Inks::default());
        assert_eq!(
            runs.iter().map(|run| run.len).collect::<Vec<_>>(),
            [6, 4, 1]
        );
        assert_eq!(runs[0], base.to_run(6));
        assert_eq!(runs[1].font.weight, FontWeight::BOLD);
        assert_eq!(runs[1].font.style, FontStyle::Italic);
        assert_eq!(runs[2].font.family.as_ref(), CODE_FONT);
        assert!(runs[2].background_color.is_some());
    }

    #[test]
    fn colors_are_names_or_hex_and_win_over_links() {
        let base = TextStyle::default();
        let colored = span(
            "a",
            &[("link", "https://x".into()), ("text_color", "red".into())],
        );
        let named = Hsla::from(rgb(0x994a37));
        let runs = text_runs(std::slice::from_ref(&colored), &base, Inks::default());
        assert_eq!(runs[0].color, named);
        // BlockNote's red for a dark theme.
        let dark = Inks {
            dark: true,
            ..Inks::default()
        };
        assert_eq!(
            text_runs(&[colored], &base, dark)[0].color,
            Hsla::from(rgb(0xe69783))
        );
        let red = Hsla::from(rgb(0xe03e3e));
        let hex = span("a", &[("background_color", "#e03e3e".into())]);
        assert_eq!(
            text_runs(&[hex], &base, Inks::default())[0].background_color,
            Some(red)
        );
        for junk in [Value::from("default"), Value::from("#zz"), Value::from(3)] {
            let run = &text_runs(
                &[span("a", &[("text_color", junk)])],
                &base,
                Inks::default(),
            )[0];
            assert_eq!(run.color, base.color);
        }
    }

    #[test]
    fn apply_highlights_snaps_utf8_boundaries_and_preserves_composed_underline() {
        let base = TextStyle::default();
        let text = "a日本語b";
        let initial = vec![base.to_run(text.len())];
        let tint = Hsla::from(rgb(0x006d9a));
        let styled = apply_highlights(
            text,
            initial,
            &[
                (
                    1..5,
                    HighlightStyle {
                        color: Some(tint),
                        ..Default::default()
                    },
                ),
                (
                    2..3,
                    HighlightStyle {
                        font_weight: Some(FontWeight::BOLD),
                        ..Default::default()
                    },
                ),
                (
                    8..99,
                    HighlightStyle {
                        font_style: Some(FontStyle::Italic),
                        ..Default::default()
                    },
                ),
            ],
        );
        let mut at = 0;
        for run in &styled {
            at += run.len;
            assert!(text.is_char_boundary(at), "invalid boundary {at}");
        }
        assert_eq!(at, text.len());

        // A pre-marked IME underline run in the middle must stay distinct from
        // neighbors even when a single highlight colors the entire line uniformly.
        let comment = "// abcd";
        let mut ime_run = base.to_run(2);
        ime_run.underline = Some(UnderlineStyle {
            color: None,
            thickness: px(1.),
            wavy: false,
        });
        let runs = vec![base.to_run(3), ime_run, base.to_run(2)];
        let colored = apply_highlights(
            comment,
            runs,
            &[(
                0..comment.len(),
                HighlightStyle {
                    color: Some(tint),
                    ..Default::default()
                },
            )],
        );
        assert_eq!(
            colored
                .iter()
                .map(|r| (r.len, r.color == tint, r.underline.is_some()))
                .collect::<Vec<_>>(),
            vec![(3, true, false), (2, true, true), (2, true, false)]
        );
    }
}
