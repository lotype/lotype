//! Links: a field for the address of the selected text, opened from the
//! formatting toolbar, and a toolbar under the link the caret is in, to
//! edit, remove or open it. The field takes the focus while it is open; the
//! editor keeps its selection meanwhile, following any peer's edits.

use gpui::{
    AnyElement, ClickEvent, Context, Entity, KeyDownEvent, MouseButton, Subscription, Window, div,
    prelude::*, px,
};
use gpui_component::button::{Button, ButtonVariants};
use gpui_component::input::{Input, InputEvent, InputState};
use gpui_component::{Sizable, ThemeStyled, h_flex};
use lotype_core::{Pos, Selection, Value};

use super::EditorView;
use super::popup::{Placement, popup};

const LINK: &str = "link";

/// The links' state between frames.
#[derive(Default)]
pub(crate) struct Links {
    /// The field for an address, while it is open, and what it listens to.
    field: Option<(Entity<InputState>, Subscription)>,
    /// The start of the text the field or the toolbar goes under.
    pub(super) place: Placement,
}

/// Whether opening `url` stays on the web or in mail: a peer's link must not
/// launch whatever other application a scheme names.
fn opens_safely(url: &str) -> bool {
    let url = url.to_ascii_lowercase();
    ["https://", "http://", "mailto:"]
        .iter()
        .any(|scheme| url.starts_with(scheme))
}

impl EditorView {
    /// The link at the caret: where it starts and ends, and its address.
    fn link_at_caret(&self) -> Option<(Pos, Pos, String)> {
        let Some(Selection::Text { anchor, head }) = self.editor.selection() else {
            return None;
        };
        if anchor != head {
            return None;
        }
        let mut runs: Vec<(usize, usize, String)> = Vec::new();
        let mut at = 0;
        for span in self.editor.document().spans(head.block).ok()? {
            let end = at + span.text.len();
            if let Some(Value::Str(url)) = span.marks.get(LINK) {
                match runs.last_mut() {
                    Some(run) if run.1 == at && run.2 == *url => run.1 = end,
                    _ => runs.push((at, end, url.clone())),
                }
            }
            at = end;
        }
        let around =
            |(start, end, _): &(usize, usize, String)| *start <= head.offset && head.offset <= *end;
        let (start, end, url) = runs.into_iter().find(around)?;
        Some((Pos::new(head.block, start), Pos::new(head.block, end), url))
    }

    /// Opens the field for the address of the selected text, holding `url`.
    pub(super) fn open_link_field(
        &mut self,
        url: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let input = cx.new(|cx| {
            let input = InputState::new(window, cx).placeholder("https://");
            input.default_value(url.to_owned())
        });
        let events = cx.subscribe_in(
            &input,
            window,
            |view, input, event, window, cx| match event {
                InputEvent::PressEnter { .. } => {
                    let url = input.read(cx).value().trim().to_owned();
                    view.close_link_field(Some(url), window, cx);
                    window.focus(&view.focus, cx);
                }
                // The focus went elsewhere, where it stays. A window that is
                // no longer active gives the field its focus back later.
                InputEvent::Blur if window.is_window_active() => {
                    view.close_link_field(None, window, cx);
                }
                _ => {}
            },
        );
        input.update(cx, |input, cx| input.focus(window, cx));
        self.links.field = Some((input, events));
        cx.notify();
    }

    /// Closes the field, linking the selected text to `url` where given, or
    /// unlinking it for an empty one.
    fn close_link_field(
        &mut self,
        url: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.links.field.take().is_none() {
            return;
        }
        if let Some(url) = url {
            let value = (!url.is_empty()).then_some(Value::Str(url));
            self.editor.set_mark(LINK, value).ok();
        }
        self.selection_changed(window, cx);
    }

    /// Notes where the field or the link toolbar goes, at the start of a frame.
    pub(super) fn place_links(&mut self) {
        self.links.place.update(self.links_target());
    }

    /// The start of the text the field or the link toolbar goes under, as
    /// last laid out.
    pub(super) fn links_target(&self) -> Option<gpui::Bounds<gpui::Pixels>> {
        let start = match (&self.links.field, self.editor.selection()) {
            (Some(_), Some(Selection::Text { anchor, head })) => {
                let ordered = self.editor.document().ordered(*anchor, *head).ok();
                ordered.map(|(from, _)| from)
            }
            (Some(_), _) => None,
            (None, _) => self.link_at_caret().map(|(from, ..)| from),
        };
        start.and_then(|start| self.caret_bounds(start))
    }

    /// The field for an address, while it is open.
    pub(super) fn link_field(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let input = self.links.field.as_ref()?.0.clone();
        let at = self.links.place.get(window)?;
        let escape = cx.listener(|view: &mut Self, event: &KeyDownEvent, window, cx| {
            if event.keystroke.key == "escape" {
                cx.stop_propagation();
                view.close_link_field(None, window, cx);
                window.focus(&view.focus, cx);
            }
        });
        let field = h_flex()
            .popover_style(cx)
            .occlude()
            .p_1()
            .w(px(300.))
            .debug_selector(|| "link-field".to_owned())
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_key_down(escape)
            .child(Input::new(&input).small());
        Some(popup(at, false, field))
    }

    /// The toolbar under the link the caret is in.
    pub(super) fn link_toolbar(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let busy = self.links.field.is_some()
            || self.dragging
            || self.composition.is_some()
            || self.slash_matches().is_some();
        if busy || !self.focus.is_focused(window) {
            return None;
        }
        let (_, _, url) = self.link_at_caret()?;
        let at = self.links.place.get(window)?;
        // The link as it is when clicked: a peer may have moved it since.
        let edit = cx.listener(|view: &mut Self, _: &ClickEvent, window, cx| {
            let Some((from, to, url)) = view.link_at_caret() else {
                return;
            };
            let link = Selection::Text {
                anchor: from,
                head: to,
            };
            if view.editor.set_selection(Some(link)).is_ok() {
                view.open_link_field(&url, window, cx);
            }
        });
        let remove = cx.listener(|view: &mut Self, _: &ClickEvent, window, cx| {
            let Some((from, to, _)) = view.link_at_caret() else {
                return;
            };
            let caret = view.editor.selection().cloned();
            let link = Selection::Text {
                anchor: from,
                head: to,
            };
            if view.editor.set_selection(Some(link)).is_ok() {
                view.editor.set_mark(LINK, None).ok();
                view.editor.set_selection(caret).ok();
            }
            view.selection_changed(window, cx);
        });
        let button = |id: &'static str, label: &'static str| {
            Button::new(id).ghost().small().tab_stop(false).label(label)
        };
        let shown: String = match url.char_indices().nth(40) {
            Some((cut, _)) => format!("{}…", &url[..cut]),
            None => url.clone(),
        };
        let open = opens_safely(&url).then(|| {
            let open = move |_: &ClickEvent, _: &mut Window, cx: &mut gpui::App| cx.open_url(&url);
            div()
                .debug_selector(|| "link-open".to_owned())
                .child(button("link-open", "Open").on_click(open))
        });
        let bar = h_flex()
            .popover_style(cx)
            .occlude()
            .p_1()
            .gap_1()
            .items_center()
            .debug_selector(|| "link-toolbar".to_owned())
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(div().px_2().text_color(gpui::rgb(0x0969da)).child(shown))
            .child(
                div()
                    .debug_selector(|| "link-edit".to_owned())
                    .child(button("link-edit", "Edit").on_click(edit)),
            )
            .child(
                div()
                    .debug_selector(|| "link-remove".to_owned())
                    .child(button("link-remove", "Remove").on_click(remove)),
            )
            .children(open);
        Some(popup(at, false, bar))
    }
}

#[cfg(test)]
mod tests {
    use super::opens_safely;

    #[test]
    fn only_web_and_mail_addresses_open() {
        assert!(opens_safely("https://lotype.dev"));
        assert!(opens_safely("MAILTO:ann@example.com"));
        assert!(!opens_safely("javascript:alert(1)"));
        assert!(!opens_safely("file:///etc/passwd"));
        assert!(!opens_safely("ssh://host"));
    }
}
