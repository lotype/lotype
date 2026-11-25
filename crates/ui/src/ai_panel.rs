//! A chat with the model about an editor's document, for an application to
//! put next to the editor.

use gpui::{
    Context, Entity, Focusable, IntoElement, Render, Subscription, Window, div, prelude::*,
};
use gpui_component::input::{Input, InputEvent, InputState};
use gpui_component::{ActiveTheme, Sizable, h_flex, v_flex};
use lotype_ai::Role;

use crate::EditorView;
use crate::editor::Act;

/// A chat panel on an [`EditorView`] that has a model
/// ([`EditorView::with_ai`]): what was said, a field to ask in about the
/// whole document, and the buttons to stop the model, try again, and accept
/// or reject its edits, which the editor shows meanwhile.
pub struct AiPanel {
    editor: Entity<EditorView>,
    input: Entity<InputState>,
    _watches: [Subscription; 2],
}

impl AiPanel {
    /// A panel on `editor`.
    pub fn new(editor: Entity<EditorView>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let input = cx.new(|cx| InputState::new(window, cx).placeholder("Ask about the document…"));
        let asked = cx.subscribe_in(
            &input,
            window,
            |panel: &mut Self, input, event, window, cx| {
                if let InputEvent::PressEnter { .. } = event {
                    let text = input.read(cx).value().to_string();
                    input.update(cx, |input, cx| input.set_value("", window, cx));
                    panel
                        .editor
                        .update(cx, |editor, cx| editor.ai_ask(&text, cx));
                }
            },
        );
        // What the model does shows in the editor first.
        let drawn = cx.observe(&editor, |_, _, cx| cx.notify());
        Self {
            editor,
            input,
            _watches: [asked, drawn],
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
        let said = editor.ai_transcript().messages.iter().map(|message| {
            let who = match message.role {
                Role::User => "You",
                Role::Assistant => "AI",
            };
            v_flex()
                .gap_0p5()
                .child(div().text_xs().text_color(muted).child(who))
                .child(div().text_sm().child(message.text.clone()))
        });
        let said: Vec<_> = said.collect();
        let status = editor.ai_status().map(|status| {
            let line = div().text_sm().text_color(muted);
            line.debug_selector(|| "ai-panel-status".to_owned())
                .child(status)
        });
        let state = editor.ai_state().cloned();
        let buttons = Act::buttons(
            state.as_ref(),
            "ai-panel",
            cx,
            |panel: &mut Self, act, window, cx| {
                panel
                    .editor
                    .update(cx, |editor, cx| editor.ai_act(act, window, cx));
            },
        );
        v_flex()
            .size_full()
            .p_2()
            .gap_2()
            .debug_selector(|| "ai-panel".to_owned())
            .child(
                v_flex()
                    .id("ai-panel-said")
                    .flex_1()
                    .min_h_0()
                    .gap_2()
                    .overflow_y_scroll()
                    .children(said),
            )
            .children(status)
            .child(h_flex().gap_1().justify_end().children(buttons))
            .child(Input::new(&self.input).small())
    }
}
