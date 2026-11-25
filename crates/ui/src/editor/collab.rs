//! The carets and selections of the other peers, over the text.

use gpui::{
    App, Bounds, Entity, Hsla, IntoElement, Pixels, TextAlign, TextRun, Window, canvas, fill,
    point, prelude::*, px, rgb, size, white,
};
use lotype_core::{Presence, Selection};

use super::EditorView;

/// A name tag shows this much of a name, which is the peer's to choose.
const NAME_CHARS: usize = 24;

impl EditorView {
    /// Draws the caret and the selected text of each peer in `presence`,
    /// in the peer's color and under its name. The application keeps a
    /// clone, which it feeds what the peers send, and redraws the view
    /// with `cx.notify()` when it has.
    pub fn with_presence(mut self, presence: Presence) -> Self {
        self.presence = Some(presence);
        self
    }
}

/// Paints the peers after the text, like the caret.
// shortcut: text selections only; a peer's selected blocks and cells are
// not drawn, to add when collaborators ask where the others are in a table.
pub(super) fn cursors(view: Entity<EditorView>) -> impl IntoElement {
    let paint = move |_: Bounds<Pixels>, _: (), window: &mut Window, cx: &mut App| {
        let editor = view.read(cx);
        let Some(presence) = &editor.presence else {
            return;
        };
        let peers = presence.collaborators(editor.editor.document());
        // Where each is, before the painting takes the application.
        let placed: Vec<_> = peers
            .into_iter()
            .filter_map(|peer| {
                let Some(Selection::Text { anchor, head }) = peer.selection else {
                    return None;
                };
                let selected = editor.rectangles_between(anchor, head);
                Some((peer.name, peer.color, selected, editor.caret_bounds(head)))
            })
            .collect();
        let font = window.text_style().font();
        let (font_size, height) = (px(11.), px(16.));
        for (name, color, selected, caret) in placed {
            let color = Hsla::from(rgb(color));
            for bounds in selected {
                window.paint_quad(fill(bounds, color.opacity(0.25)));
            }
            let Some(caret) = caret else {
                continue;
            };
            window.paint_quad(fill(caret, color));
            // A line is shaped, so the name is kept to one.
            let visible = name.chars().filter(|c| !c.is_control());
            let name: String = visible.take(NAME_CHARS).collect();
            if name.is_empty() {
                continue;
            }
            let run = TextRun {
                len: name.len(),
                font: font.clone(),
                color: white(),
                background_color: None,
                underline: None,
                strikethrough: None,
            };
            let text_system = window.text_system();
            let line = text_system.shape_line(name.into(), font_size, &[run], None);
            let origin = point(caret.left(), caret.top() - height);
            let tag = Bounds::new(origin, size(line.width + px(6.), height));
            window.paint_quad(fill(tag, color));
            let text = origin + point(px(3.), Pixels::ZERO);
            line.paint(text, height, TextAlign::Left, None, window, cx)
                .ok();
        }
    };
    canvas(|_, _, _| (), paint).absolute().size_full()
}
