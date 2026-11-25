//! How marks look: text runs for GPUI from a block's spans.

use gpui::{
    FontStyle, FontWeight, HighlightStyle, Hsla, Rgba, StrikethroughStyle, TextRun, TextStyle,
    UnderlineStyle, px, rgb,
};
use lotype_core::{Span, Value, mark};

pub(crate) const CODE_FONT: &str = "Menlo";

/// BlockNote's named colors, text then background, so that documents
/// written with it look the same.
pub(crate) const COLORS: [(&str, u32, u32); 9] = [
    ("gray", 0x9b9a97, 0xebeced),
    ("brown", 0x64473a, 0xe9e5e3),
    ("red", 0xe03e3e, 0xfbe4e4),
    ("orange", 0xd9730d, 0xf6e9d9),
    ("yellow", 0xdfab01, 0xfbf3db),
    ("green", 0x4d6461, 0xddedea),
    ("blue", 0x0b6e99, 0xddebf1),
    ("purple", 0x6940a5, 0xeae4f2),
    ("pink", 0xad1a72, 0xf4dfeb),
];

/// One run per span, styled by its marks on top of `base`.
pub(crate) fn text_runs(spans: &[Span], base: &TextStyle) -> Vec<TextRun> {
    let spans = spans.iter().filter(|span| !span.text.is_empty());
    spans.map(|span| text_run(span, base)).collect()
}

fn text_run(span: &Span, base: &TextStyle) -> TextRun {
    let mut style = HighlightStyle::default();
    let line = |color| UnderlineStyle {
        color,
        thickness: px(1.),
        wavy: false,
    };
    let link = Hsla::from(rgb(0x0b6e99));
    // Marks come sorted by name, so an explicit color wins over the link's.
    for (name, value) in &span.marks {
        match name.as_str() {
            mark::BACKGROUND_COLOR => style.background_color = color(value, true),
            mark::BOLD => style.font_weight = Some(FontWeight::BOLD),
            mark::CODE => {
                let code = Hsla::from(rgb(0xebeced));
                style.background_color.get_or_insert(code);
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
            mark::TEXT_COLOR => style.color = color(value, false).or(style.color),
            mark::UNDERLINE => style.underline = Some(line(None)),
            _ => {}
        }
    }
    let mut run = base.clone().highlight(style).to_run(span.text.len());
    if span.marks.contains_key(mark::CODE) {
        run.font.family = CODE_FONT.into();
    }
    run
}

/// A color mark's value: one of BlockNote's names or a CSS hex color.
pub(crate) fn color(value: &Value, background: bool) -> Option<Hsla> {
    let Value::Str(name) = value else { return None };
    // Longer than `#rrggbbaa` is no color, and a peer's long text is not parsed.
    if name.len() > 9 {
        return None;
    }
    let named = COLORS.iter().find(|(known, ..)| known == name);
    let named = named.map(|&(_, text, fill)| rgb(if background { fill } else { text }));
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
    fn marks_style_their_run_only() {
        let base = TextStyle::default();
        let spans = [
            span("plain ", &[]),
            span("", &[("bold", true.into())]),
            span("bold", &[("bold", true.into()), ("italic", true.into())]),
            span("x", &[("code", true.into())]),
        ];
        let runs = text_runs(&spans, &base);
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
        let red = Hsla::from(rgb(0xe03e3e));
        let colored = span(
            "a",
            &[("link", "https://x".into()), ("text_color", "red".into())],
        );
        assert_eq!(text_runs(&[colored], &base)[0].color, red);
        let hex = span("a", &[("background_color", "#e03e3e".into())]);
        assert_eq!(text_runs(&[hex], &base)[0].background_color, Some(red));
        for junk in [Value::from("default"), Value::from("#zz"), Value::from(3)] {
            let run = &text_runs(&[span("a", &[("text_color", junk)])], &base)[0];
            assert_eq!(run.color, base.color);
        }
    }
}
