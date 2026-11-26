//! GPUI interline elements for Twoslash `^?` popovers, `^|` completions,
//! diagnostics, `@filename:` tabs, and `---cut---` dividers.

use gpui::{AnyElement, App, FontWeight, Hsla, div, prelude::*, px};
use gpui_component::{ActiveTheme as _, h_flex, v_flex};

use super::{CODE_FONT, HighlightCache, severity_ink};
use crate::analyzer::{CompletionKind, DiagnosticAnnotation, Severity};
use crate::twoslash::{CompletionCallout, CutKind, QueryCallout};

const APPROX_CH_WIDTH: f32 = 7.4;

fn column_indent(col: usize, max_cols: usize) -> gpui::Pixels {
    px((col.min(max_cols) as f32) * APPROX_CH_WIDTH)
}

pub(super) fn render_query_callout(
    cache: &mut HighlightCache,
    language: Option<&str>,
    query: &QueryCallout,
    folded: bool,
    cx: &App,
) -> AnyElement {
    let theme = cx.theme();
    let label = match &query.text {
        Some(text) => cache
            .highlight_snippet(language, text, cx)
            .into_any_element(),
        None => div()
            .text_color(theme.muted_foreground)
            .child(format!("{} : analyzing…", query.symbol))
            .into_any_element(),
    };
    let pointer = if folded { "▲" } else { "↳" };
    let card = h_flex()
        .gap_1p5()
        .px_2()
        .py(px(3.))
        .rounded_sm()
        .border_1()
        .border_color(theme.border)
        .bg(theme.popover)
        .text_xs()
        .font_family(CODE_FONT)
        .max_w(px(520.))
        .debug_selector(|| "twoslash-query".to_owned())
        .child(
            div()
                .text_color(theme.primary)
                .font_weight(FontWeight::SEMIBOLD)
                .child(pointer),
        )
        .child(label);
    let indent = if folded {
        column_indent(query.character, 44)
    } else {
        px(0.)
    };
    div().pl(indent).py(px(2.)).child(card).into_any_element()
}

pub(super) fn render_completion_dropdown(
    comp: &CompletionCallout,
    folded: bool,
    cx: &App,
) -> AnyElement {
    let theme = cx.theme();
    let border = theme.border;
    let active_bg = theme.list_active;
    let fg = theme.foreground;
    let muted = theme.muted_foreground;
    let primary = theme.primary;
    let prefix = comp.prefix.clone();

    let rows: Vec<AnyElement> = if comp.items.is_empty() {
        vec![
            div()
                .px_2()
                .py(px(2.))
                .text_xs()
                .text_color(muted)
                .child("No completions")
                .into_any_element(),
        ]
    } else {
        comp.items
            .iter()
            .enumerate()
            .map(|(idx, item)| {
                let badge = match item.kind {
                    CompletionKind::Function => "fn",
                    CompletionKind::Property => "prop",
                    CompletionKind::Variable => "var",
                    CompletionKind::Type => "type",
                    CompletionKind::Keyword => "kw",
                };
                let (matched, rest) = if !prefix.is_empty() && item.label.starts_with(&prefix) {
                    (prefix.clone(), item.label[prefix.len()..].to_owned())
                } else {
                    (String::new(), item.label.clone())
                };
                let label_el = h_flex()
                    .items_center()
                    .when(!matched.is_empty(), |row| {
                        row.child(
                            div()
                                .font_weight(FontWeight::BOLD)
                                .text_color(primary)
                                .child(matched),
                        )
                    })
                    .child(div().text_color(fg).child(rest));

                let mut row = h_flex()
                    .w_full()
                    .justify_between()
                    .items_center()
                    .gap_3()
                    .px_2()
                    .py(px(2.))
                    .rounded(px(3.));
                if idx == 0 {
                    row = row.bg(active_bg);
                }
                row.child(
                    h_flex()
                        .gap_1p5()
                        .items_center()
                        .child(
                            div()
                                .px_1()
                                .rounded(px(2.))
                                .bg(theme.secondary)
                                .text_color(muted)
                                .child(badge),
                        )
                        .child(label_el),
                )
                .children(item.detail.as_ref().map(|detail| {
                    div()
                        .text_color(muted)
                        .truncate()
                        .max_w(px(240.))
                        .child(detail.clone())
                }))
                .into_any_element()
            })
            .collect()
    };

    let card = v_flex()
        .p_1()
        .min_w(px(210.))
        .max_w(px(460.))
        .rounded_sm()
        .border_1()
        .border_color(border)
        .bg(theme.popover)
        .text_xs()
        .font_family(CODE_FONT)
        .debug_selector(|| "twoslash-completion".to_owned())
        .children(rows);

    let indent = if folded {
        column_indent(
            comp.character.saturating_sub(comp.prefix.chars().count()),
            40,
        )
    } else {
        px(0.)
    };
    div().pl(indent).py(px(2.)).child(card).into_any_element()
}

pub(super) fn render_diagnostic_banner(diagnostic: &DiagnosticAnnotation, cx: &App) -> AnyElement {
    let ink = severity_ink(diagnostic.severity, cx);
    let (bg, border) = severity_fill(diagnostic.severity, cx);
    let prefix = match (&diagnostic.severity, &diagnostic.code) {
        (Severity::Error, Some(code)) => format!("error[{code}]"),
        (Severity::Error, None) => "error".to_owned(),
        (Severity::Warning, Some(code)) => format!("warning[{code}]"),
        (Severity::Warning, None) => "warning".to_owned(),
        (Severity::Info, Some(code)) => format!("log[{code}]"),
        (Severity::Info, None) => "log".to_owned(),
        (Severity::Hint, Some(code)) => format!("note[{code}]"),
        (Severity::Hint, None) => "note".to_owned(),
    };
    let banner = v_flex()
        .px_2()
        .py(px(3.))
        .rounded_sm()
        .border_1()
        .border_color(border)
        .bg(bg)
        .text_xs()
        .max_w(px(520.))
        .debug_selector(|| "twoslash-diagnostic".to_owned())
        .child(
            h_flex()
                .gap_1p5()
                .flex_wrap()
                .child(
                    div()
                        .font_family(CODE_FONT)
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(ink)
                        .child(prefix),
                )
                .child(
                    div()
                        .text_color(cx.theme().foreground)
                        .child(diagnostic.message.clone()),
                ),
        );
    div().py(px(2.)).child(banner).into_any_element()
}

pub(super) fn render_file_tab(name: &str, cx: &App) -> AnyElement {
    let theme = cx.theme();
    h_flex()
        .gap_1p5()
        .py(px(3.))
        .items_center()
        .text_xs()
        .font_family(CODE_FONT)
        .text_color(theme.muted_foreground)
        .debug_selector(|| "twoslash-file".to_owned())
        .child(
            div()
                .px_1p5()
                .py(px(1.))
                .rounded_sm()
                .border_1()
                .border_color(theme.border)
                .bg(theme.background)
                .text_color(theme.foreground)
                .child(name.to_owned()),
        )
        .into_any_element()
}

pub(super) fn render_cut_divider(kind: CutKind, cx: &App) -> AnyElement {
    let label = match kind {
        CutKind::Before => "twoslash cut ↑ hidden outside edit mode",
        CutKind::After => "twoslash cut ↓ hidden outside edit mode",
        CutKind::Start => "twoslash cut-start ↓",
        CutKind::End => "twoslash cut-end ↑",
    };
    let line = || div().h(px(1.)).w(px(24.)).bg(cx.theme().border);
    h_flex()
        .gap_2()
        .py(px(2.))
        .items_center()
        .text_xs()
        .text_color(cx.theme().muted_foreground)
        .debug_selector(|| "twoslash-cut".to_owned())
        .child(line())
        .child(label)
        .child(line())
        .into_any_element()
}

fn severity_fill(severity: Severity, cx: &App) -> (Hsla, Hsla) {
    let status = &cx.theme().highlight_theme.style.status;
    match severity {
        Severity::Error => (status.error_background(cx), status.error_border(cx)),
        Severity::Warning => (status.warning_background(cx), status.warning_border(cx)),
        Severity::Info => (status.info_background(cx), status.info_border(cx)),
        Severity::Hint => (status.hint_background(cx), status.hint_border(cx)),
    }
}
