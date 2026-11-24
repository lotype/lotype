//! The side menu: left of the block the mouse is over, centred on its first
//! line, a button that adds a block below it through the slash menu, and a
//! handle that opens the block menu when clicked and moves the block when
//! dragged, with a line where it lands. It stays a moment after the mouse
//! leaves the rows.

use std::time::Duration;

use gpui::{
    AnyElement, Bounds, ClickEvent, Context, CursorStyle, MouseButton, MouseDownEvent, Pixels,
    Point, Task, Window, div, prelude::*, px, rgb,
};
use gpui_component::button::{Button, ButtonVariants};
use gpui_component::{Sizable, h_flex};
use lotype_core::{BlockId, Pos, Selection};

use super::EditorView;
use crate::icons::{self, icon};

/// The room left of the blocks that the side menu of a top-level block takes.
pub(crate) const GUTTER: f32 = 48.;
/// How far the mouse goes with the handle held before it drags the block.
const DRAG_START: f32 = 4.;
/// How tall the side menu is: its buttons, the extra small ones.
const MENU_HEIGHT: Pixels = px(20.);
/// How long the side menu stays once the mouse has left the rows, so that
/// it does not flicker away on the way to it or between blocks.
const HIDE_DELAY: Duration = Duration::from_millis(300);

#[derive(Default)]
pub(crate) struct SideMenu {
    /// The row the mouse is over.
    hovered: Option<BlockId>,
    dragged: Option<Dragged>,
    /// The block whose block menu is open.
    pub(super) menu: Option<BlockId>,
    /// How far down its row the side menu goes, as the last frame laid it out.
    top: Option<Pixels>,
    /// Hides the side menu after `HIDE_DELAY`, unless the mouse comes back.
    leaving: Option<Task<()>>,
}

/// A block held by its handle.
struct Dragged {
    block: BlockId,
    from: Point<Pixels>,
    /// The row it would land next to, and whether after it.
    target: Option<(BlockId, bool)>,
    /// It went far enough to be a drag, not a click.
    moved: bool,
}

impl EditorView {
    /// The rows' bounds in the last frame, where they are on the screen.
    fn row_bounds(&self, at: usize) -> Option<Bounds<Pixels>> {
        // The scroll handle has them before scrolling: the rows come first.
        let row = self.scroll.bounds_for_item(at)?;
        Some(Bounds::new(row.origin + self.scroll.offset(), row.size))
    }

    /// The row under `position`, and whether `position` is in its lower
    /// half. The lower half of a row whose children show is the upper half
    /// of its first child: a block dropped there goes before that child.
    fn row_at(&self, position: Point<Pixels>) -> Option<(BlockId, bool)> {
        if !self.scroll.bounds().contains(&position) {
            return None;
        }
        // The rows go down the page in order.
        let (mut low, mut high) = (0, self.rows.len());
        while low < high {
            let middle = (low + high) / 2;
            match self.row_bounds(middle) {
                Some(bounds) if bounds.bottom() <= position.y => low = middle + 1,
                _ => high = middle,
            }
        }
        let block = *self.rows.get(low)?;
        let bounds = self.row_bounds(low)?;
        if position.y < bounds.top() {
            return None;
        }
        let lower = position.y > bounds.center().y;
        let next = self.rows.get(low + 1).copied();
        let doc = self.editor.document();
        if let Some(child) =
            next.filter(|next| lower && doc.parent(*next).ok() == Some(Some(block)))
        {
            return Some((child, false));
        }
        Some((block, lower))
    }

    /// Follows the mouse: the row it is over, and where a dragged block goes.
    /// Returns whether anything changed.
    pub(super) fn side_menu_move(
        &mut self,
        position: Point<Pixels>,
        pressed: bool,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.side_menu.dragged.is_some() {
            if !pressed {
                // A release the window never heard of.
                self.side_menu.dragged = None;
                return true;
            }
            let target = self.row_at(position);
            let Some(dragged) = &mut self.side_menu.dragged else {
                return false;
            };
            // A heading goes in no block, so it has no place among nested ones.
            let doc = self.editor.document();
            let top_level = doc
                .block_type(dragged.block)
                .ok()
                .and_then(|kind| doc.schema().block_spec(&kind))
                .is_some_and(|spec| spec.is_top_level());
            let target =
                target.filter(|(at, _)| !top_level || doc.parent(*at).is_ok_and(|p| p.is_none()));
            dragged.moved |= (position - dragged.from).magnitude() > f64::from(DRAG_START);
            if !dragged.moved {
                return false;
            }
            let changed = dragged.target != target;
            dragged.target = target;
            return changed;
        }
        let Some((hovered, _)) = self.row_at(position) else {
            self.leave_later(cx);
            return false;
        };
        self.side_menu.leaving = None;
        let changed = Some(hovered) != self.side_menu.hovered;
        self.side_menu.hovered = Some(hovered);
        changed
    }

    /// Hides the side menu after a moment, unless the mouse is over a row again by then.
    fn leave_later(&mut self, cx: &mut Context<Self>) {
        if self.side_menu.hovered.is_none() || self.side_menu.leaving.is_some() {
            return;
        }
        let leave = cx.spawn(async move |view, cx| {
            cx.background_executor().timer(HIDE_DELAY).await;
            view.update(cx, |view, cx| {
                view.side_menu.leaving = None;
                if view.side_menu.hovered.take().is_some() {
                    cx.notify();
                }
            })
            .ok();
        });
        self.side_menu.leaving = Some(leave);
    }

    /// Where the side menu goes down the row of `block`: centred on its
    /// first line of text, whatever its type makes of that line's height or
    /// of the room above it, or on a short row without text, or at the top
    /// of a tall one, as an image's.
    fn side_menu_top(&self, block: BlockId) -> Option<Pixels> {
        let at = self.rows.iter().position(|row| *row == block)?;
        let row = self.row_bounds(at)?;
        Some(match self.caret_bounds(Pos::new(block, 0)) {
            Some(line) => line.top() - row.top() + (line.size.height - MENU_HEIGHT) / 2.,
            None if row.size.height <= MENU_HEIGHT * 2. => (row.size.height - MENU_HEIGHT) / 2.,
            None => px(2.),
        })
    }

    /// Notes where the side menu goes, at the start of a frame, from the
    /// layout of the frame before.
    pub(super) fn place_side_menu(&mut self) {
        let held = self.side_menu.dragged.as_ref().map(|dragged| dragged.block);
        let block = held.or(self.side_menu.menu).or(self.side_menu.hovered);
        self.side_menu.top = block.and_then(|block| self.side_menu_top(block));
    }

    /// Drops a dragged block, or opens its block menu where the handle was
    /// only clicked, leaving the selection as it is. Returns whether a block
    /// was held.
    pub(super) fn side_menu_release(&mut self) -> bool {
        let Some(dragged) = self.side_menu.dragged.take() else {
            return false;
        };
        let block = dragged.block;
        match dragged.target {
            Some((target, after)) if target != block => self.drop_block(block, target, after),
            None if !dragged.moved => self.side_menu.menu = Some(block),
            _ => {}
        }
        true
    }

    /// The mouse left the window.
    pub(super) fn side_menu_leave(&mut self, cx: &mut Context<Self>) {
        self.leave_later(cx);
    }

    /// Ends text being composed, as a click anywhere does, and takes the focus.
    fn press(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(composition) = self.composition.take() {
            self.commit(&composition.text);
        }
        window.focus(&self.focus, cx);
    }

    /// Moves `block` next to `target`, among `target`'s siblings, as one
    /// undo step. A block cannot go inside itself, and does not.
    fn drop_block(&mut self, block: BlockId, target: BlockId, after: bool) {
        self.editor
            .apply(|doc, selection| {
                doc.move_block_next_to(block, target, after)?;
                Ok(selection.cloned())
            })
            .ok();
    }

    /// Adds an empty paragraph below `block`, or takes `block` where it is
    /// one, and opens the slash menu in it.
    fn add_below(&mut self, block: BlockId) {
        let doc = self.editor.document();
        let empty = doc.block_type(block).is_ok_and(|kind| kind == "paragraph")
            && doc.text_len(block).is_ok_and(|len| len == 0)
            && doc
                .children(Some(block))
                .is_ok_and(|children| children.is_empty());
        let placed = if empty {
            self.editor
                .set_selection(Some(Selection::caret(Pos::new(block, 0))))
        } else {
            self.editor.apply(|doc, _| {
                let index = doc.index_of(block)? + 1;
                let paragraph = doc.insert_block(doc.parent(block)?, index, "paragraph")?;
                Ok(Some(Selection::caret(Pos::new(paragraph, 0))))
            })
        };
        if placed.is_ok() {
            self.editor.insert_text("/").ok();
        }
    }

    /// The side menu of the row of `block`, `indent` from the left, when the
    /// mouse is over it and no selection is being dragged out.
    pub(super) fn side_menu(
        &self,
        block: BlockId,
        indent: f32,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        // The block held, or whose menu is open, keeps its side menu.
        let held = self.side_menu.dragged.as_ref().map(|dragged| dragged.block);
        let kept = held.or(self.side_menu.menu);
        if self.dragging
            || kept.unwrap_or(block) != block
            || self.side_menu.hovered != Some(block) && kept.is_none()
        {
            return None;
        }
        let add = cx.listener(move |view: &mut Self, _: &ClickEvent, window, cx| {
            view.add_below(block);
            window.focus(&view.focus, cx);
            view.selection_changed(window, cx);
        });
        let hold = cx.listener(move |view: &mut Self, event: &MouseDownEvent, window, cx| {
            cx.stop_propagation();
            view.press(window, cx);
            view.side_menu.dragged = Some(Dragged {
                block,
                from: event.position,
                target: None,
                moved: false,
            });
        });
        let add = Button::new("side-add")
            .ghost()
            .xsmall()
            .tab_stop(false)
            .icon(icon(icons::PLUS))
            .tooltip("Add a block below")
            .on_click(add);
        let handle = div()
            .px_1()
            .text_color(rgb(0x8a8f98))
            .cursor(CursorStyle::OpenHand)
            .debug_selector(|| "side-handle".to_owned())
            .on_mouse_down(MouseButton::Left, hold)
            .child(icon(icons::GRIP).size_4())
            .children(self.block_menu(block, cx));
        let menu = h_flex()
            .absolute()
            .top(self.side_menu.top.unwrap_or(px(2.)))
            .left(px(indent - GUTTER + 4.))
            .gap_1()
            .items_center()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|view: &mut Self, _: &MouseDownEvent, window, cx| {
                    cx.stop_propagation();
                    view.press(window, cx);
                }),
            )
            .child(div().debug_selector(|| "side-add".to_owned()).child(add))
            .child(handle);
        Some(menu.into_any_element())
    }

    /// The line where a dragged block lands, on the row of `block`.
    pub(super) fn drop_line(&self, block: BlockId, indent: f32) -> Option<AnyElement> {
        let (target, after) = self.side_menu.dragged.as_ref()?.target?;
        if target != block {
            return None;
        }
        let line = div()
            .absolute()
            .left(px(indent))
            .right_0()
            .h(px(2.))
            .bg(rgb(0x0969da));
        let line = if after {
            line.bottom(px(-1.))
        } else {
            line.top(px(-1.))
        };
        Some(
            line.debug_selector(|| "drop-line".to_owned())
                .into_any_element(),
        )
    }
}
