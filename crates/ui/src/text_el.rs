//! The text of one block: styled runs and inline atoms, wrapped to the
//! width it is given. GPUI's `StyledText` holds no elements inside its
//! text, and puts a caret at a soft wrap only at the end of the upper line.

mod geometry;
mod wrap;

use std::cell::RefCell;
use std::ops::Range;
use std::rc::Rc;

use gpui::{
    AnyElement, App, AvailableSpace, Bounds, Element, ElementId, GlobalElementId, HighlightStyle,
    Hsla, InspectorElementId, IntoElement, LayoutId, Pixels, Point, ShapedLine, SharedString, Size,
    TextAlign, TextRun, Window, fill, point, px, size,
};

const LINE_BADGE_MARGIN_LEFT: Pixels = px(4.);

pub(crate) use geometry::Affinity;

/// Visual styling applied to one `\n`-delimited source line in a text block.
#[derive(Clone)]
pub struct LineDecoration {
    /// Zero-based hard line index (`0` is the first line).
    pub line: usize,
    /// Full-width background fill behind the line.
    pub background: Option<Hsla>,
    /// Accent bar color drawn at the left edge of the line.
    pub bar: Option<Hsla>,
    /// Short gutter marker (such as `"+"` or `"-"`) and its ink color.
    pub badge: Option<(SharedString, Hsla)>,
}

/// A non-editable element placed in its own row right below a source line,
/// such as a Twoslash `^?` callout or diagnostic banner.
pub struct Interline {
    /// Zero-based hard line index after which this element is placed.
    pub after_line: usize,
    /// Element laid out below that source line.
    pub element: AnyElement,
}

/// Visual-only styling for a block's text, computed at render time without
/// storing marks in the document.
#[derive(Default)]
pub struct TextDecoration {
    /// Byte-range styles (syntax tokens, wavy diagnostic underlines).
    pub highlights: Vec<(Range<usize>, HighlightStyle)>,
    /// Per-line background tints and diff/focus bars.
    pub lines: Vec<LineDecoration>,
    /// Elements inserted between source lines without shifting document byte offsets.
    pub interlines: Vec<Interline>,
    /// Zero-based hard lines collapsed in preview mode (such as Twoslash directives and cut sections).
    pub hidden_lines: Vec<usize>,
    /// Visible end-byte overrides `(hard_line, visible_end_byte)` for stripping trailing inline directive comments.
    pub trimmed_lines: Vec<(usize, usize)>,
}

/// Where the text was laid out, shared by the element and the view.
#[derive(Clone, Default)]
pub(crate) struct TextLayout(Rc<RefCell<Option<Laid>>>);

pub(crate) struct Laid {
    text: SharedString,
    /// The height of a line of text without atoms.
    line_height: Pixels,
    lines: Vec<Line>,
    interlines: Vec<(usize, Pixels)>,
    size: Size<Pixels>,
    wrap_width: Option<Pixels>,
    origin: Point<Pixels>,
}

/// One line on screen.
struct Line {
    /// Zero-based `\n`-delimited source line index.
    hard_line: usize,
    /// Whether this is the first visual row of `hard_line`.
    first_of_hard: bool,
    /// What it shows of the text, without a line break that ends it.
    range: Range<usize>,
    /// Whether the text goes on in the next line without a line break.
    wraps: bool,
    /// Where the line starts, right of the left edge when it is aligned.
    left: Pixels,
    top: Pixels,
    height: Pixels,
    /// Where text is painted in the line, below an atom taller than it.
    text_top: Pixels,
    pieces: Vec<Piece>,
}

struct Piece {
    range: Range<usize>,
    x: Pixels,
    width: Pixels,
    kind: Kind,
}

enum Kind {
    Text(Box<ShapedLine>),
    /// The atom's index, and its top in the line.
    Atom(usize, Pixels),
}

/// The text of a block, with `atoms` drawn in place of their characters.
pub(crate) struct TextElement {
    text: SharedString,
    runs: Vec<TextRun>,
    atoms: Vec<(Range<usize>, AnyElement, Option<Pixels>)>,
    line_decorations: Vec<LineDecoration>,
    interlines: Vec<Interline>,
    hidden_lines: Vec<usize>,
    trimmed_lines: Vec<(usize, usize)>,
    align: f32,
    layout: TextLayout,
}

impl TextElement {
    /// `runs` cover the text; `atoms` are in order and do not overlap.
    pub(crate) fn new(
        text: SharedString,
        runs: Vec<TextRun>,
        atoms: Vec<(Range<usize>, AnyElement, Option<Pixels>)>,
    ) -> Self {
        let layout = TextLayout::default();
        Self {
            text,
            runs,
            atoms,
            line_decorations: Vec::new(),
            interlines: Vec::new(),
            hidden_lines: Vec::new(),
            trimmed_lines: Vec::new(),
            align: 0.,
            layout,
        }
    }

    pub(crate) fn decorate(mut self, decoration: TextDecoration) -> Self {
        self.line_decorations = decoration.lines;
        self.interlines = decoration.interlines;
        self.hidden_lines = decoration.hidden_lines;
        self.trimmed_lines = decoration.trimmed_lines;
        self
    }

    /// Moves each line right in the room it leaves: by none of it for 0,
    /// half of it to centre the line, all of it for 1.
    pub(crate) fn align(mut self, align: f32) -> Self {
        self.align = align;
        self
    }

    pub(crate) fn layout(&self) -> &TextLayout {
        &self.layout
    }
}

fn unbounded() -> Size<AvailableSpace> {
    Size::new(AvailableSpace::MinContent, AvailableSpace::MinContent)
}

fn max_content() -> Size<AvailableSpace> {
    Size::new(AvailableSpace::MaxContent, AvailableSpace::MinContent)
}

impl IntoElement for TextElement {
    type Element = Self;

    fn into_element(self) -> Self {
        self
    }
}

impl Element for TextElement {
    type RequestLayoutState = ();
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        let style = window.text_style();
        let font_size = style.font_size.to_pixels(window.rem_size());
        let line_height = style
            .line_height
            .to_pixels(font_size.into(), window.rem_size());
        let line_height = window.pixel_snap(line_height);
        let sample_run = TextRun {
            len: 1,
            font: style.font(),
            color: style.color,
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        let sample = window
            .text_system()
            .shape_line("x".into(), font_size, &[sample_run], None);
        let text_baseline = (line_height - sample.ascent - sample.descent) / 2. + sample.ascent;
        // Atoms are measured here: laying out an element inside the
        // measuring callback below would nest layouts.
        let atoms = self.atoms.iter_mut();
        let atoms: Vec<_> = atoms
            .map(|(range, element, baseline)| {
                (
                    range.clone(),
                    element.layout_as_root(unbounded(), window, cx),
                    *baseline,
                )
            })
            .collect();
        let interlines: Vec<_> = self
            .interlines
            .iter_mut()
            .map(|interline| {
                (
                    interline.after_line,
                    interline.element.layout_as_root(max_content(), window, cx),
                )
            })
            .collect();
        let input = wrap::Input {
            text: self.text.clone(),
            starts: self
                .runs
                .iter()
                .scan(0, |at, run| Some(std::mem::replace(at, *at + run.len)))
                .collect(),
            runs: self.runs.clone(),
            atoms,
            interlines,
            hidden_lines: self.hidden_lines.clone(),
            trimmed_lines: self.trimmed_lines.clone(),
            font_size,
            line_height,
            text_baseline,
            align: self.align,
        };
        let shared = self.layout.clone();
        let measure = move |known: Size<Option<Pixels>>,
                            available: Size<AvailableSpace>,
                            window: &mut Window,
                            _: &mut App| {
            let wrap_width = known.width.or(match available.width {
                AvailableSpace::Definite(width) => Some(width),
                _ => None,
            });
            // A probe of the text's own size, without a width, gets the
            // size laid out last, as GPUI's own text answers it: laying out
            // again unwrapped would leave that layout to be painted.
            if let Some(laid) = shared.0.borrow().as_ref()
                && (wrap_width.is_none() || laid.wrap_width == wrap_width)
            {
                return laid.size;
            }
            let laid = input.lay_out(wrap_width, window);
            let size = laid.size;
            shared.0.replace(Some(laid));
            size
        };
        (
            window.request_measured_layout(Default::default(), measure),
            (),
        )
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        // Where atoms go, found before their views run, which must not
        // meet the layout borrowed.
        let mut places = Vec::new();
        let mut interline_places = Vec::new();
        if let Some(laid) = self.layout.0.borrow_mut().as_mut() {
            laid.origin = bounds.origin;
            for line in &laid.lines {
                for piece in &line.pieces {
                    if let Kind::Atom(index, top) = piece.kind {
                        places.push((index, bounds.origin + point(piece.x, line.top + top)));
                    }
                }
            }
            for &(index, top) in &laid.interlines {
                interline_places.push((index, bounds.origin + point(Pixels::ZERO, top)));
            }
        }
        for (index, origin) in places {
            self.atoms[index]
                .1
                .prepaint_as_root(origin, unbounded(), window, cx);
        }
        for (index, origin) in interline_places {
            self.interlines[index]
                .element
                .prepaint_as_root(origin, max_content(), window, cx);
        }
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        let laid = self.layout.0.borrow();
        let Some(laid) = laid.as_ref() else { return };
        let left = TextAlign::Left;
        let font = window.text_style().font();
        let font_size = window.text_style().font_size.to_pixels(window.rem_size());
        for line in &laid.lines {
            if let Some(deco) = self
                .line_decorations
                .iter()
                .find(|deco| deco.line == line.hard_line)
            {
                let row_origin = point(laid.origin.x - px(8.), laid.origin.y + line.top);
                let row_size = size(laid.size.width + px(16.), line.height);
                if let Some(bg) = deco.background {
                    window.paint_quad(fill(Bounds::new(row_origin, row_size), bg));
                }
                if let Some(bar) = deco.bar {
                    let bar_bounds = Bounds::new(
                        point(row_origin.x - px(4.), row_origin.y),
                        size(px(3.), line.height),
                    );
                    window.paint_quad(fill(bar_bounds, bar));
                }
                if line.first_of_hard
                    && let Some((badge, color)) = &deco.badge
                    && !badge.is_empty()
                {
                    let run = TextRun {
                        len: badge.len(),
                        font: font.clone(),
                        color: *color,
                        background_color: None,
                        underline: None,
                        strikethrough: None,
                    };
                    let shaped = window.text_system().shape_line(
                        badge.clone(),
                        font_size * 0.85,
                        &[run],
                        None,
                    );
                    let marker_pos = point(
                        laid.origin.x - px(4.) + LINE_BADGE_MARGIN_LEFT - shaped.width(),
                        laid.origin.y + line.top + line.text_top,
                    );
                    shaped
                        .paint(marker_pos, laid.line_height, left, None, window, cx)
                        .ok();
                }
            }
            for piece in &line.pieces {
                let origin = laid.origin + point(piece.x, line.top + line.text_top);
                match &piece.kind {
                    Kind::Text(shaped) => {
                        let height = laid.line_height;
                        shaped
                            .paint_background(origin, height, left, None, window, cx)
                            .ok();
                        shaped.paint(origin, height, left, None, window, cx).ok();
                    }
                    Kind::Atom(index, _) => self.atoms[*index].1.paint(window, cx),
                }
            }
        }
        for &(index, _) in &laid.interlines {
            self.interlines[index].element.paint(window, cx);
        }
    }
}
