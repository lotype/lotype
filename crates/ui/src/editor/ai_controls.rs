//! The controls the AI's surfaces share: what can be done with a session,
//! the button that sends, what a session says, and the row they sit in. Their colors
//! are the application's theme's, named here once.

use gpui::{AnyElement, App, ClickEvent, Context, Div, Hsla, Window, div, prelude::*, px};
use gpui_component::button::{Button, ButtonCustomVariant, ButtonVariants};
use gpui_component::{ActiveTheme, Disableable, Sizable, h_flex};
use lotype_ai::AiState;

use crate::icons::{self, icon};

/// The color of the action to take: accepting, retrying, sending.
fn accent(cx: &App) -> Hsla {
    cx.theme().blue
}

/// What is written on the accent.
fn on_accent(cx: &App) -> Hsla {
    cx.theme().background
}

/// `button` filled with the accent, for the action a session waits for.
fn solid(button: Button, cx: &App) -> Button {
    let accent = accent(cx);
    let style = ButtonCustomVariant::new(cx)
        .color(accent)
        .foreground(on_accent(cx))
        .hover(accent.opacity(0.9))
        .active(accent.opacity(0.8));
    // A custom variant only tints at rest: the fill is the button's own.
    button.custom(style).bg(accent)
}

/// The button that sends a field's text, as Enter does: tinted with the
/// accent once there is something to send.
pub(crate) fn send(id: &'static str, ready: bool, cx: &App) -> Button {
    let accent = accent(cx);
    let tint = ButtonCustomVariant::new(cx)
        .color(accent)
        .foreground(accent)
        .hover(accent.opacity(0.3))
        .active(accent.opacity(0.4));
    Button::new(id)
        .small()
        .when(ready, |send| send.custom(tint))
        .label("Send")
        .child(icon(icons::RETURN).size_3())
        .disabled(!ready)
}

/// What a session in `state` says, in a line: an answer in the text's
/// color, a failure in the danger's, and the rest under the AI's mark.
pub(crate) fn status(text: String, state: Option<&AiState>, cx: &App) -> Div {
    let line = h_flex().min_w_0().gap_1p5().text_sm();
    let said = div().min_w_0().child(text);
    match state {
        None => line.child(said),
        Some(AiState::Error(_)) => line.text_color(cx.theme().danger).child(said),
        Some(_) => line
            .text_color(cx.theme().muted_foreground)
            .child(icon(icons::SPARKLES).size_3p5())
            .child(said),
    }
}

/// What a session says while the model answers, whatever it is doing:
/// not every model thinks before it writes.
pub(crate) const WORKING: &str = "Working…";

/// The AI's mark and [`WORKING`], with a light on the letters `step`
/// steps along: it passes over the text, three letters wide, and comes
/// again from before it.
pub(crate) fn working(step: usize, cx: &App) -> Div {
    let (ink, lit) = (cx.theme().muted_foreground, cx.theme().foreground);
    let head = (step % (WORKING.chars().count() + 6)) as f32 - 3.;
    let letters = WORKING.chars().enumerate().map(|(index, letter)| {
        let near = (1. - (index as f32 - head).abs() / 3.).max(0.);
        div()
            .text_color(ink.blend(lit.opacity(near)))
            .child(letter.to_string())
    });
    h_flex()
        .gap_1p5()
        .text_sm()
        .text_color(ink)
        .child(icon(icons::SPARKLES).size_3p5())
        .child(h_flex().children(letters))
}

/// A row of a card: what the session says, and what can be done with it.
/// `None` where there is neither.
pub(crate) fn tier(status: Option<AnyElement>, buttons: Vec<AnyElement>) -> Option<Div> {
    if status.is_none() && buttons.is_empty() {
        return None;
    }
    let tier = h_flex()
        .items_center()
        .gap_1()
        .px_2()
        .py_1()
        .min_h(px(34.))
        .child(div().flex_1().min_w_0().children(status))
        .children(buttons);
    Some(tier)
}

/// What can be done with a session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Act {
    Stop,
    Retry,
    Accept,
    Reject,
}

impl Act {
    /// What a session in `state` offers, the last of them the one to press.
    pub(crate) fn offered(state: Option<&AiState>) -> &'static [(Act, &'static str)] {
        match state {
            Some(AiState::Thinking | AiState::Writing) => &[(Act::Stop, "Stop")],
            Some(AiState::Reviewing) => &[(Act::Reject, "Reject"), (Act::Accept, "Accept")],
            Some(AiState::Error(_)) => &[(Act::Reject, "Close"), (Act::Retry, "Retry")],
            _ => &[],
        }
    }

    /// The name a test finds its button by, after `prefix`.
    fn name(self) -> &'static str {
        match self {
            Act::Stop => "stop",
            Act::Retry => "retry",
            Act::Accept => "accept",
            Act::Reject => "reject",
        }
    }

    /// The buttons for what a session in `state` offers, each doing `act`
    /// of its own, and found by tests as `{prefix}-{name}`.
    pub(crate) fn buttons<V: 'static>(
        state: Option<&AiState>,
        prefix: &'static str,
        cx: &mut Context<V>,
        act: impl Fn(&mut V, Act, &mut Window, &mut Context<V>) + Clone + 'static,
    ) -> Vec<AnyElement> {
        let offered = Self::offered(state);
        let buttons = offered.iter().enumerate().map(|(at, (what, label))| {
            let (what, act) = (*what, act.clone());
            let click = cx.listener(move |view: &mut V, _: &ClickEvent, window, cx| {
                act(view, what, window, cx);
            });
            let button = Button::new((prefix, at)).small().label(*label);
            let button = match at + 1 == offered.len() && offered.len() > 1 {
                true => solid(button, cx),
                false => button,
            };
            let tagged = div().debug_selector(move || format!("{prefix}-{}", what.name()));
            tagged.child(button.on_click(click)).into_any_element()
        });
        buttons.collect()
    }
}
