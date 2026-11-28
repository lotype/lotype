//! Native GPU vector renderer for [`katex_native::Layout`] in GPUI.
//!
//! Draws text runs using KaTeX's loaded fonts, rules/rectangles via `paint_quad`,
//! and stretchy radicals/delimiters via `PathBuilder` + `paint_path` clipped by
//! `with_content_mask`.

use std::sync::Arc;

use gpui::{
    AnyElement, App, Bounds, ContentMask, Font as GpuiFont, FontStyle, FontWeight, Hsla,
    PathBuilder, Pixels, Point, Rgba, Size, TextAlign, TextRun, Window, canvas, fill, point,
    prelude::*, px, size,
};
use katex_native::{Color, Font, Item, Layout, PathCommand};

/// Minimum thickness in logical pixels so hairline fraction bars remain crisp.
const MIN_RULE_PX: f32 = 0.75;

/// Computes the pixel size and top-to-baseline distance for an inline formula.
pub(crate) fn inline_metrics(layout: &Layout, px_per_em: Pixels) -> (Size<Pixels>, Pixels) {
    let em = f32::from(px_per_em);
    let tag_width = layout.tag.as_ref().map_or(0., |tag| 1.0 + tag.width);
    let width = px(((layout.width + tag_width) as f32 * em).ceil().max(1.));
    let height = layout
        .height
        .max(layout.tag.as_ref().map_or(0., |tag| tag.height));
    let depth = layout
        .depth
        .max(layout.tag.as_ref().map_or(0., |tag| tag.depth));
    let ascent = px((height as f32 * em).ceil());
    let total_height = px((((height + depth) as f32) * em).ceil().max(1.));
    (size(width, total_height), ascent)
}

/// Renders an inline formula as a fixed-size vector GPUI canvas element.
pub(crate) fn inline_element(
    layout: Arc<Layout>,
    px_per_em: Pixels,
    color: Hsla,
) -> (AnyElement, Pixels) {
    let (dim, baseline) = inline_metrics(&layout, px_per_em);
    let em = f32::from(px_per_em);
    let ascent = layout
        .height
        .max(layout.tag.as_ref().map_or(0., |tag| tag.height)) as f32
        * em;
    let element = canvas(
        |_, _, _| (),
        move |bounds, _, window, cx| {
            let origin = point(bounds.origin.x, bounds.origin.y + px(ascent));
            paint_items(&layout.items, origin, em, color, window, cx);
            if let Some(tag) = &layout.tag {
                let tag_origin = point(origin.x + px(((layout.width + 1.0) as f32) * em), origin.y);
                paint_items(&tag.items, tag_origin, em, color, window, cx);
            }
        },
    )
    .w(dim.width)
    .h(dim.height)
    .flex_none()
    .into_any_element();
    (element, baseline)
}

/// Renders a display equation as a full-width vector GPUI canvas element,
/// centered (or flush-left when `fleqn`) with optional `\tag{...}` aligned to
/// the right (or left when `leqno`).
pub(crate) fn display_element(
    layout: Arc<Layout>,
    px_per_em: Pixels,
    color: Hsla,
    fleqn: bool,
    leqno: bool,
) -> AnyElement {
    let em = f32::from(px_per_em);
    let height = layout
        .height
        .max(layout.tag.as_ref().map_or(0., |tag| tag.height)) as f32;
    let depth = layout
        .depth
        .max(layout.tag.as_ref().map_or(0., |tag| tag.depth)) as f32;
    let pad_y = px(4.);
    let min_width = px(
        ((layout.width + layout.tag.as_ref().map_or(0., |tag| tag.width + 1.5)) as f32 * em)
            .ceil()
            .max(1.),
    ) + px(4.);
    let total_height = px(((height + depth) * em).ceil().max(1.)) + pad_y * 2.;
    let ascent = px(height * em);

    canvas(
        |_, _, _| (),
        move |bounds, _, window, cx| {
            let body_w = px((layout.width as f32) * em);
            let tag_w = layout
                .tag
                .as_ref()
                .map_or(Pixels::ZERO, |tag| px((tag.width as f32) * em));
            let gap = px(em * 1.0);
            let baseline_y = bounds.origin.y + pad_y + ascent;

            let body_x = if fleqn {
                let left_margin = if leqno && tag_w > Pixels::ZERO {
                    tag_w + gap
                } else {
                    Pixels::ZERO
                };
                bounds.origin.x + left_margin
            } else {
                let centered = bounds.origin.x + (bounds.size.width - body_w) / 2.;
                if leqno && tag_w > Pixels::ZERO {
                    centered.max(bounds.origin.x + tag_w + gap)
                } else if !leqno && tag_w > Pixels::ZERO {
                    let max_right = bounds.right() - tag_w - gap - body_w;
                    centered.min(max_right).max(bounds.origin.x)
                } else {
                    centered.max(bounds.origin.x)
                }
            };

            paint_items(
                &layout.items,
                point(body_x, baseline_y),
                em,
                color,
                window,
                cx,
            );

            if let Some(tag) = &layout.tag {
                let tag_x = if leqno {
                    bounds.origin.x
                } else {
                    (bounds.right() - tag_w).max(body_x + body_w + gap)
                };
                paint_items(&tag.items, point(tag_x, baseline_y), em, color, window, cx);
            }
        },
    )
    .w_full()
    .min_w(min_width)
    .flex_shrink_0()
    .h(total_height)
    .into_any_element()
}

fn paint_items(
    items: &[Item],
    origin: Point<Pixels>,
    em: f32,
    default_color: Hsla,
    window: &mut Window,
    cx: &mut App,
) {
    for item in items {
        match item {
            Item::Text {
                x,
                y,
                text,
                font,
                size,
                color,
            } => {
                if text.is_empty() {
                    continue;
                }
                let font_px = px((*size as f32) * em).max(px(1.));
                let ink = resolve_color(*color, default_color);
                let run = TextRun {
                    len: text.len(),
                    font: to_gpui_font(*font),
                    color: ink,
                    background_color: None,
                    underline: None,
                    strikethrough: None,
                };
                let shaped =
                    window
                        .text_system()
                        .shape_line(text.clone().into(), font_px, &[run], None);
                let line_height = shaped.ascent + shaped.descent;
                let pos = point(
                    origin.x + px((*x as f32) * em),
                    origin.y + px((*y as f32) * em) - shaped.ascent,
                );
                let _ = shaped.paint(pos, line_height, TextAlign::Left, None, window, cx);
            }
            Item::Rect {
                x,
                y,
                width,
                height,
                color,
            } => {
                let w = px(((*width as f32) * em).max(MIN_RULE_PX));
                let h = px(((*height as f32) * em).max(MIN_RULE_PX));
                let bounds = Bounds::new(
                    point(
                        origin.x + px((*x as f32) * em),
                        origin.y + px((*y as f32) * em),
                    ),
                    size(w, h),
                );
                window.paint_quad(fill(bounds, resolve_color(*color, default_color)));
            }
            Item::Line {
                from,
                to,
                thickness,
                color,
            } => {
                let ink = resolve_color(*color, default_color);
                let stroke = px(((*thickness as f32) * em).max(MIN_RULE_PX));
                let p1 = point(
                    origin.x + px((from.0 as f32) * em),
                    origin.y + px((from.1 as f32) * em),
                );
                let p2 = point(
                    origin.x + px((to.0 as f32) * em),
                    origin.y + px((to.1 as f32) * em),
                );
                if (from.1 - to.1).abs() < 1e-6 {
                    let left = p1.x.min(p2.x);
                    let width = (p1.x - p2.x).abs().max(stroke);
                    let bounds = Bounds::new(point(left, p1.y - stroke / 2.), size(width, stroke));
                    window.paint_quad(fill(bounds, ink));
                } else if (from.0 - to.0).abs() < 1e-6 {
                    let top = p1.y.min(p2.y);
                    let height = (p1.y - p2.y).abs().max(stroke);
                    let bounds = Bounds::new(point(p1.x - stroke / 2., top), size(stroke, height));
                    window.paint_quad(fill(bounds, ink));
                } else {
                    let mut builder = PathBuilder::stroke(stroke);
                    builder.move_to(p1);
                    builder.line_to(p2);
                    if let Ok(path) = builder.build() {
                        window.paint_path(path, ink);
                    }
                }
            }
            Item::Path {
                commands,
                clip,
                color,
            } => {
                let ink = resolve_color(*color, default_color);
                let mut builder = PathBuilder::fill();
                let mut has_move = false;
                for cmd in commands {
                    match *cmd {
                        PathCommand::MoveTo(x, y) => {
                            builder.move_to(point(
                                origin.x + px((x as f32) * em),
                                origin.y + px((y as f32) * em),
                            ));
                            has_move = true;
                        }
                        PathCommand::LineTo(x, y) => {
                            if has_move {
                                builder.line_to(point(
                                    origin.x + px((x as f32) * em),
                                    origin.y + px((y as f32) * em),
                                ));
                            }
                        }
                        PathCommand::CubicTo(x1, y1, x2, y2, x, y) => {
                            if has_move {
                                builder.cubic_bezier_to(
                                    point(
                                        origin.x + px((x as f32) * em),
                                        origin.y + px((y as f32) * em),
                                    ),
                                    point(
                                        origin.x + px((x1 as f32) * em),
                                        origin.y + px((y1 as f32) * em),
                                    ),
                                    point(
                                        origin.x + px((x2 as f32) * em),
                                        origin.y + px((y2 as f32) * em),
                                    ),
                                );
                            }
                        }
                        PathCommand::Close => {
                            if has_move {
                                builder.close();
                            }
                        }
                    }
                }
                if !has_move {
                    continue;
                }
                let Ok(path) = builder.build() else {
                    continue;
                };
                if clip.width > 0. && clip.height > 0. {
                    let clip_bounds = Bounds::new(
                        point(
                            origin.x + px((clip.x as f32) * em),
                            origin.y + px((clip.y as f32) * em),
                        ),
                        size(px((clip.width as f32) * em), px((clip.height as f32) * em)),
                    );
                    window.with_content_mask(
                        Some(ContentMask {
                            bounds: clip_bounds,
                        }),
                        |window| {
                            window.paint_path(path, ink);
                        },
                    );
                } else {
                    window.paint_path(path, ink);
                }
            }
            _ => {}
        }
    }
}

fn to_gpui_font(font: Font) -> GpuiFont {
    GpuiFont {
        family: font.family.name().into(),
        weight: if font.bold {
            FontWeight::BOLD
        } else {
            FontWeight::NORMAL
        },
        style: if font.italic {
            FontStyle::Italic
        } else {
            FontStyle::Normal
        },
        ..Default::default()
    }
}

fn resolve_color(color: Option<Color>, default: Hsla) -> Hsla {
    match color {
        Some(c) => Hsla::from(Rgba {
            r: f32::from(c.r) / 255.,
            g: f32::from(c.g) / 255.,
            b: f32::from(c.b) / 255.,
            a: f32::from(c.a) / 255.,
        }),
        None => default,
    }
}
