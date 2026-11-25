//! The carets and selections of the other peers, over the text.

use gpui::{
    App, Bounds, Context, Entity, Hsla, IntoElement, Pixels, Subscription, Task, TextAlign,
    TextRun, Window, canvas, fill, point, prelude::*, px, rgb, size, white,
};
use lotype_ai::AiSession;
use lotype_core::{Collaborator, PRESENCE_TIMEOUT, Presence, Selection, SyncMessage};

use super::EditorView;
use crate::text_el::Affinity;

/// A name tag shows this much of a name, which is the peer's to choose.
const NAME_CHARS: usize = 24;

/// Publishes one person's presence, including when their AI starts or stops editing.
pub(super) struct PresencePublisher {
    name: String,
    color: u32,
    send: Box<dyn Fn(SyncMessage)>,
    last: Vec<u8>,
    _watch: Subscription,
    _heartbeat: Task<()>,
}

impl EditorView {
    /// Draws the caret and the selected text of each peer in `presence`,
    /// in the peer's color and under its name. The application keeps a
    /// clone, which it feeds what the peers send, and redraws the view
    /// with `cx.notify()` when it has, and when a peer it has not heard
    /// of for `PRESENCE_TIMEOUT` is to go.
    pub fn with_presence(mut self, presence: Presence) -> Self {
        self.presence = Some(presence);
        self
    }

    /// Sends this editor's human and AI presence over the application's transport.
    ///
    /// Supply a callback that queues messages in order for one connection.
    /// This replaces manual presence publication for that connection. Use
    /// [`Self::with_presence`] separately to display messages received from peers.
    pub fn with_presence_publisher(
        mut self,
        name: impl Into<String>,
        color: u32,
        send: impl Fn(SyncMessage) + 'static,
        cx: &mut Context<Self>,
    ) -> Self {
        let watch = cx.observe_self(|view, _| view.publish_presence(false));
        let heartbeat = cx.spawn(async move |view, cx| {
            let mut refresh = false;
            loop {
                if view
                    .update(cx, |view, _| view.publish_presence(refresh))
                    .is_err()
                {
                    return;
                }
                cx.background_executor().timer(PRESENCE_TIMEOUT / 3).await;
                refresh = true;
            }
        });
        if self.presence_publisher.is_none() {
            cx.on_release(|view, _| {
                if let Some(publisher) = &view.presence_publisher {
                    (publisher.send)(SyncMessage::Presence(Presence::leave(
                        view.editor().document(),
                    )));
                }
            })
            .detach();
        }
        self.presence_publisher = Some(PresencePublisher {
            name: name.into(),
            color,
            send: Box::new(send),
            last: Vec::new(),
            _watch: watch,
            _heartbeat: heartbeat,
        });
        self
    }

    /// Queues the new state when it changes, or periodically to keep it alive.
    fn publish_presence(&mut self, refresh: bool) {
        let Some(publisher) = &self.presence_publisher else {
            return;
        };
        let editor = self.editor();
        let block = self.ai.session.as_ref().and_then(AiSession::active_anchor);
        let bytes = Presence::publish_with_ai(
            editor.document(),
            &publisher.name,
            publisher.color,
            editor.selection(),
            block,
        );
        if refresh || bytes != publisher.last {
            let publisher = self.presence_publisher.as_mut().expect("publisher is set");
            publisher.last = bytes.clone();
            (publisher.send)(SyncMessage::Presence(bytes));
        }
    }
}

/// Paints the peers after the text, like the caret, and before it.
// shortcut: text selections only; a peer's selected blocks and cells are
// not drawn, to add when collaborators ask where the others are in a table.
pub(super) fn cursors(view: Entity<EditorView>, peers: Vec<Collaborator>) -> impl IntoElement {
    let paint = move |_: Bounds<Pixels>, _: (), window: &mut Window, cx: &mut App| {
        let editor = view.read(cx);
        // Text being composed is laid out in place of the document's, so
        // offsets into that block's text are not where they are drawn.
        let composing = editor.composition.as_ref().and(editor.shown());
        let composing = composing.map(|shown| shown.block);
        // Where each is, before the painting takes the application.
        let placed: Vec<_> = peers
            .iter()
            .filter_map(|peer| {
                let Some(Selection::Text { anchor, head }) = &peer.selection else {
                    return None;
                };
                if [anchor.block, head.block]
                    .iter()
                    .any(|b| Some(*b) == composing)
                {
                    return None;
                }
                let selected = editor.rectangles_between(*anchor, *head);
                // Not the side of a soft wrap the local caret is on.
                let layout = editor.layouts.get(&head.block);
                let caret = layout.and_then(|l| l.caret(head.offset, Affinity::default()));
                Some((&peer.name, peer.color, selected, caret))
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
