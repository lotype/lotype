//! A chat with the model about an editor's document, for an application to
//! put next to the editor.

use gpui::{
    App, Context, Div, Entity, Focusable, IntoElement, Render, ScrollHandle, Subscription, Window,
    div, prelude::*, px,
};
use gpui_component::input::{InputEvent, Textarea, TextareaState};
use gpui_component::{ActiveTheme, Sizable, StyledExt, h_flex, v_flex};
use lotype_ai::Role;

use crate::EditorView;
use crate::editor::{Act, send, status, tier};

/// A chat panel on an [`EditorView`] that has a model
/// ([`EditorView::with_ai`]): the shared history from all AI entry points,
/// a composer for the whole document, and review controls for its requests.
pub struct AiPanel {
    editor: Entity<EditorView>,
    input: Entity<TextareaState>,
    thread: ScrollHandle,
    _watches: [Subscription; 2],
}

impl AiPanel {
    /// A panel on `editor`.
    pub fn new(editor: Entity<EditorView>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let input = cx.new(|cx| {
            TextareaState::new(window, cx)
                .placeholder("Ask or edit the whole document…")
                .auto_grow(2, 6)
                .submit_on_enter(true)
        });
        let asked =
            cx.subscribe_in(
                &input,
                window,
                |panel: &mut Self, _, event, window, cx| match event {
                    InputEvent::PressEnter { shift: false, .. } => panel.send(window, cx),
                    InputEvent::Change => cx.notify(),
                    _ => {}
                },
            );
        let drawn = cx.observe(&editor, |panel, _, cx| {
            if -panel.thread.offset().y >= panel.thread.max_offset().y - px(8.) {
                panel.thread.scroll_to_bottom();
            }
            cx.notify();
        });
        Self {
            editor,
            input,
            thread: ScrollHandle::new(),
            _watches: [asked, drawn],
        }
    }

    /// Sends the composer text, retaining it if another turn owns the editor.
    fn send(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let text = self.input.read(cx).value().to_string();
        let sent = self
            .editor
            .update(cx, |editor, cx| editor.ai_ask_document(&text, cx));
        if sent {
            self.input
                .update(cx, |input, cx| input.set_value("", window, cx));
            self.thread.scroll_to_bottom();
            cx.notify();
        }
    }
}

/// The panel's focus is its field's.
impl Focusable for AiPanel {
    fn focus_handle(&self, cx: &gpui::App) -> gpui::FocusHandle {
        self.input.read(cx).focus_handle(cx)
    }
}

impl Render for AiPanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let editor = self.editor.read(cx);
        let muted = cx.theme().muted_foreground;
        let said_so_far: Vec<_> = editor
            .ai_transcript()
            .messages
            .iter()
            .enumerate()
            .map(|(index, message)| {
                chat_message(message.role, message.text.clone(), cx)
                    .debug_selector(move || format!("ai-panel-message-{index}"))
            })
            .chain(
                editor
                    .ai_streaming_reply()
                    .map(|reply| chat_message(Role::Assistant, reply.to_owned(), cx)),
            )
            .collect();
        let state = editor.ai_chat_state().cloned();
        // What its own session says goes on the card; the rest is a note.
        let said = state.as_ref().and_then(|_| editor.ai_status()).map(|text| {
            let said = status(text, state.as_ref(), cx);
            said.debug_selector(|| "ai-panel-status".to_owned())
                .into_any_element()
        });
        let note = match state {
            Some(_) => None,
            None if editor.ai_state().is_some() => {
                Some("Finish the contextual suggestion before sending.")
            }
            None if !editor.has_ai() => Some("No AI model configured."),
            None => None,
        };
        let note = note.map(|note| {
            let line = div().text_sm().text_color(muted);
            line.debug_selector(|| "ai-panel-status".to_owned())
                .child(note)
        });
        let can_send = editor.ai_chat_can_send() && !self.input.read(cx).value().trim().is_empty();
        let empty = said_so_far.is_empty().then(|| {
            div()
                .text_sm()
                .text_color(muted)
                .child("Ask a question, summarize the page, or request an edit.")
        });
        let buttons = Act::buttons(
            state.as_ref(),
            "ai-panel",
            cx,
            |panel: &mut Self, act, window, cx| {
                panel.editor.update(cx, |editor, cx| {
                    if editor.ai_chat_state().is_some() {
                        editor.ai_act(act, window, cx);
                    }
                });
            },
        );
        let strip = cx.theme().muted.opacity(0.5);
        let top = tier(said, buttons).map(|top| top.bg(strip));
        let send = send("document-chat-send", can_send, cx)
            .on_click(cx.listener(|panel, _, window, cx| panel.send(window, cx)));
        let border = cx.theme().border;
        let composer = v_flex()
            .p_1()
            .gap_1()
            .border_color(border)
            .when(top.is_some(), |composer| composer.border_t_1())
            .child(Textarea::new(&self.input).small().appearance(false))
            .child(
                h_flex()
                    .justify_between()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .px_1()
                            .text_xs()
                            .text_color(muted)
                            .child("Shift+Enter for a new line"),
                    )
                    .child(
                        div()
                            .debug_selector(|| "ai-panel-send".to_owned())
                            .child(send),
                    ),
            );
        v_flex()
            .size_full()
            .p_2()
            .gap_2()
            .debug_selector(|| "ai-panel".to_owned())
            .child(
                v_flex()
                    .gap_0p5()
                    .child(div().font_semibold().child("Document chat"))
                    .child(div().text_xs().text_color(muted).child("Whole document")),
            )
            .child(
                v_flex()
                    .id("ai-panel-said")
                    .flex_1()
                    .min_h_0()
                    .gap_2()
                    .overflow_y_scroll()
                    .track_scroll(&self.thread)
                    .children(empty)
                    .children(said_so_far),
            )
            .children(note)
            .child(
                v_flex()
                    .border_1()
                    .border_color(border)
                    .rounded_lg()
                    .overflow_hidden()
                    .children(top)
                    .child(composer),
            )
    }
}

/// Renders one entry in the document-chat thread.
fn chat_message(role: Role, text: String, cx: &App) -> Div {
    let who = match role {
        Role::User => "You",
        Role::Assistant => "AI",
    };
    v_flex()
        .gap_0p5()
        .rounded_md()
        .p_2()
        .when(role == Role::User, |message| message.bg(cx.theme().muted))
        .child(
            div()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(who),
        )
        .child(div().text_sm().child(text))
}
