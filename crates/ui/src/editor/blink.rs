//! The caret's blinking. Each blink draws the window again, and every frame
//! lays out every block until the rows are a virtual list, so the caret
//! blinks only while a frame is cheap: in a document too big for that it
//! stays lit, and costs nothing. It blinks only while the editor has the
//! focus in an active window with a caret, and stays lit while it moves.

use std::cell::Cell;
use std::rc::Rc;
use std::time::{Duration, Instant};

use gpui::{Context, Task, Window};
use lotype_core::Selection;

use super::EditorView;

/// How long the caret stays lit after it moves or text changes.
const RESUME: Duration = Duration::from_millis(500);
/// How long the caret is lit, and then dark, as macOS blinks it.
const PHASE: Duration = Duration::from_millis(530);
/// The most a frame may take for the caret to blink: two such frames a
/// second are under 2% of a core.
const BUDGET: Duration = Duration::from_millis(8);

#[derive(Default)]
pub(crate) struct Blink {
    /// The caret is in the dark half of a blink.
    pub(super) dark: bool,
    task: Option<Task<()>>,
    /// When the frame being drawn started, and how long the last one took.
    pub(super) started: Rc<Cell<Option<Instant>>>,
    pub(super) cost: Rc<Cell<Duration>>,
    /// Frames are taken as cheap, or as dear, whatever they cost, for tests.
    taken_as: Option<bool>,
}

impl EditorView {
    /// Lights the caret, and blinks it again after a while, as it moved or
    /// the text changed.
    pub(super) fn restart_blink(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.blink.dark = false;
        self.blink.task = Some(cx.spawn_in(window, async move |view, cx| {
            let mut wait = RESUME;
            loop {
                cx.background_executor().timer(wait).await;
                wait = PHASE;
                let blinking = view.update_in(cx, |view, window, cx| {
                    if !view.blinks(window) {
                        view.blink.dark = false;
                        cx.notify();
                        return false;
                    }
                    view.blink.dark = !view.blink.dark;
                    cx.notify();
                    true
                });
                if !matches!(blinking, Ok(true)) {
                    return;
                }
            }
        }));
    }

    /// Whether the caret blinks now: it is a caret, in the text this editor
    /// has the focus of, and a frame is cheap.
    fn blinks(&self, window: &Window) -> bool {
        let caret = matches!(self.editor.selection(), Some(Selection::Text { anchor, head }) if anchor == head);
        caret
            && self.composition.is_none()
            && window.is_window_active()
            && self.focus.is_focused(window)
            && self
                .blink
                .taken_as
                .unwrap_or(self.blink.cost.get() <= BUDGET)
    }

    /// Whether the caret is lit, for tests of blinking.
    #[cfg(any(test, feature = "test-support"))]
    pub fn caret_lit(&self) -> bool {
        !self.blink.dark
    }

    /// How many rows in sight any frame laid out where no one sees them,
    /// for tests: none should be.
    #[cfg(any(test, feature = "test-support"))]
    pub fn rows_measured_in_sight(&self) -> usize {
        self.heights.unseen_count()
    }

    /// How many rows the last frame laid out, for tests of a frame's cost.
    #[cfg(any(test, feature = "test-support"))]
    pub fn rows_laid_out(&self) -> usize {
        self.heights.laid_out_count()
    }

    /// Takes every frame as cheap, so that a test of blinking does not
    /// depend on how busy the machine running it is.
    #[cfg(any(test, feature = "test-support"))]
    pub fn take_frames_as_cheap(&mut self) {
        self.blink.taken_as = Some(true);
    }

    /// Takes every frame as costing more than the budget, for a test of
    /// what a dear frame does to the blinking.
    #[cfg(any(test, feature = "test-support"))]
    pub fn take_frames_as_dear(&mut self) {
        self.blink.taken_as = Some(false);
    }
}
