//! The text of one block: styled runs and inline atoms, wrapped to the
//! width it is given. GPUI's `StyledText` holds no elements inside its
//! text, and puts a caret at a soft wrap only at the end of the upper line.

mod geometry;
mod wrap;

use std::cell::RefCell;
use std::ops::Range;
use std::rc::Rc;

use gpui::{
    AnyElement, App, AvailableSpace, Bounds, Element, ElementId, GlobalElementId,
    InspectorElementId, IntoElement, LayoutId, Pixels, Point, ShapedLine, SharedString, Size,
    TextAlign, TextRun, Window, point,
};

pub(crate) use geometry::Affinity;

/// Where the text was laid out, shared by the element and the view.
#[derive(Clone, Default)]
pub(crate) struct TextLayout(Rc<RefCell<Option<Laid>>>);

pub(crate) struct Laid {
    text: SharedString,
    /// The height of a line of text without atoms.
    line_height: Pixels,
    lines: Vec<Line>,
    size: Size<Pixels>,
    wrap_width: Option<Pixels>,
    origin: Point<Pixels>,
}

/// One line on screen.
struct Line {
    /// What it shows of the text, without a line break that ends it.
    range: Range<usize>,
    /// Whether the text goes on in the next line without a line break.
    wraps: bool,
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
    atoms: Vec<(Range<usize>, AnyElement)>,
    layout: TextLayout,
}

impl TextElement {
    /// `runs` cover the text; `atoms` are in order and do not overlap.
    pub(crate) fn new(
        text: SharedString,
        runs: Vec<TextRun>,
        atoms: Vec<(Range<usize>, AnyElement)>,
    ) -> Self {
        let layout = TextLayout::default();
        Self {
            text,
            runs,
            atoms,
            layout,
        }
    }

    pub(crate) fn layout(&self) -> &TextLayout {
        &self.layout
    }
}

fn unbounded() -> Size<AvailableSpace> {
    Size::new(AvailableSpace::MinContent, AvailableSpace::MinContent)
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
        // Atoms are measured here: laying out an element inside the
        // measuring callback below would nest layouts.
        let atoms = self.atoms.iter_mut();
        let atoms: Vec<_> = atoms
            .map(|(range, element)| {
                (
                    range.clone(),
                    element.layout_as_root(unbounded(), window, cx),
                )
            })
            .collect();
        let input = wrap::Input {
            text: self.text.clone(),
            runs: self.runs.clone(),
            atoms,
            font_size,
            line_height,
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
            if let Some(laid) = shared.0.borrow().as_ref()
                && laid.wrap_width == wrap_width
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
        let mut laid = self.layout.0.borrow_mut();
        let Some(laid) = laid.as_mut() else { return };
        laid.origin = bounds.origin;
        for line in &laid.lines {
            for piece in &line.pieces {
                if let Kind::Atom(index, top) = piece.kind {
                    let origin = bounds.origin + point(piece.x, line.top + top);
                    let atom = &mut self.atoms[index].1;
                    atom.prepaint_as_root(origin, unbounded(), window, cx);
                }
            }
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
        for line in &laid.lines {
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
    }
}
