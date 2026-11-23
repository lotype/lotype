//! Where a menu or a toolbar goes: next to text, as the frame before laid
//! it out. That layout is still there when a frame starts and the popup is
//! placed; where a scroll or a resize moves the text, the next frame follows.

use gpui::{
    Anchor, AnyElement, Bounds, IntoElement, ParentElement, Pixels, Window, anchored, deferred,
    point, px,
};

/// The text a popup goes next to, as last laid out.
#[derive(Default)]
pub(crate) struct Placement {
    at: Option<Bounds<Pixels>>,
    /// The text moved in the last layout.
    moved: bool,
    /// A frame was asked for while the text was not laid out. Text that is
    /// never laid out, as in a table, would otherwise ask for every frame.
    asked: bool,
}

impl Placement {
    /// Notes where the text is now, at the start of a frame.
    pub(crate) fn update(&mut self, at: Option<Bounds<Pixels>>) {
        self.moved = at != self.at;
        self.at = at;
    }

    /// Where the popup goes. Text not laid out yet, or moved since, is laid
    /// out where it stays in this frame, so the view draws again in the next.
    pub(crate) fn get(&mut self, window: &mut Window) -> Option<Bounds<Pixels>> {
        if self.moved || self.at.is_none() && !self.asked {
            window.request_animation_frame();
        }
        self.asked = self.at.is_none();
        self.at
    }
}

/// `child` under `target`, or over it when `above`; on the other side where
/// there is no room, and in front of everything else.
pub(crate) fn popup(target: Bounds<Pixels>, above: bool, child: impl IntoElement) -> AnyElement {
    let (anchor, corner, gap) = if above {
        (Anchor::BottomLeft, Anchor::TopLeft, px(-4.))
    } else {
        (Anchor::TopLeft, Anchor::BottomLeft, px(4.))
    };
    let placed = anchored()
        .anchor(anchor)
        .attach(target, corner)
        .offset(point(px(0.), gap))
        .snap_to_window_with_margin(px(8.))
        .child(child);
    deferred(placed).with_priority(1).into_any_element()
}
