//! What is drawn of a session: the bar to ask in and to act on the model's
//! edits, the marks on the blocks they changed, and where the model writes.

use gpui::{
    Anchor, AnyElement, App, Bounds, ClickEvent, Context, Div, Entity, Hsla, IntoElement,
    KeyDownEvent, MouseButton, Pixels, Window, anchored, canvas, deferred, div, fill, point,
    prelude::*, px,
};
use gpui_component::button::{Button, ButtonVariants};
use gpui_component::input::Input;
use gpui_component::{ActiveTheme, Sizable, ThemeStyled, h_flex, v_flex};
use lotype_ai::{AiState, Change, EditLocation, Role};
use lotype_core::BlockId;

use super::EditorView;
use crate::text_el::Affinity;

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
            let button = Button::new((prefix, at))
                .small()
                .tab_stop(false)
                .label(*label);
            let button = match at + 1 == offered.len() && offered.len() > 1 {
                true => button.primary(),
                false => button.ghost(),
            };
            let tagged = div().debug_selector(move || format!("{prefix}-{}", what.name()));
            tagged.child(button.on_click(click)).into_any_element()
        });
        buttons.collect()
    }
}

impl EditorView {
    /// Finds the visible row for the cursor, using an ancestor when the target is hidden.
    pub(super) fn ai_cursor_row(&self, rows: &[BlockId]) -> Option<EditLocation> {
        let location = self.ai.session.as_ref()?.edit_location()?;
        let (block, removal) = match location {
            EditLocation::Block(block) => (block, false),
            EditLocation::Removal(Some(block)) => (block, true),
            EditLocation::Removal(None) => return Some(location),
        };
        let block = self.visible_cursor_row(block, rows)?;
        Some(if removal {
            EditLocation::Removal(Some(block))
        } else {
            EditLocation::Block(block)
        })
    }

    /// Finds the closest row in the view, including a collapsed block's ancestor.
    pub(super) fn visible_cursor_row(
        &self,
        mut block: BlockId,
        rows: &[BlockId],
    ) -> Option<BlockId> {
        while !rows.contains(&block) {
            block = self.editor.document().parent(block).ok().flatten()?;
        }
        Some(block)
    }

    /// Does `act` with the session.
    pub(crate) fn ai_act(&mut self, act: Act, window: &mut Window, cx: &mut Context<Self>) {
        match act {
            Act::Stop => self.ai_stop(cx),
            Act::Retry => self.ai_retry(cx),
            Act::Accept => drop(self.ai_accept(window, cx)),
            Act::Reject => self.ai_reject(window, cx),
        }
    }

    /// What the model is doing, in a line: or what it said of what it did,
    /// and how many blocks it removed, which the view does not show.
    pub(crate) fn ai_status(&self) -> Option<String> {
        match self.ai_state()? {
            AiState::Thinking => Some("Thinking…".to_owned()),
            AiState::Writing => Some("Writing…".to_owned()),
            AiState::Error(error) => Some(error.clone()),
            AiState::Reviewing => {
                let removed = self.ai.changes.iter();
                let removed = removed.filter(|change| matches!(change, Change::Removed(_)));
                let gone = match removed.count() {
                    0 => String::new(),
                    1 => " 1 block removed.".to_owned(),
                    more => format!(" {more} blocks removed."),
                };
                let said = self.ai_transcript().messages.last();
                let said = said.filter(|message| message.role == Role::Assistant);
                let said = said
                    .map(|message| message.text.as_str())
                    .unwrap_or_default();
                Some(format!("{said}{gone}").trim().to_owned()).filter(|text| !text.is_empty())
            }
            _ => None,
        }
    }

    /// The bar, at the foot of the editor: what the model is doing or
    /// said, the field to ask in, and what can be done with its edits.
    pub(super) fn ai_bar(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let input = self.ai.field.as_ref()?.0.clone();
        let view = self.scroll.bounds();
        if view.size.width <= Pixels::ZERO {
            window.request_animation_frame();
            return None;
        }
        let state = self.ai_state().cloned();
        let buttons = Act::buttons(state.as_ref(), "ai", cx, Self::ai_act);
        let escape = cx.listener(|view: &mut Self, event: &KeyDownEvent, window, cx| {
            // With edits to review, leaving them is said with a button.
            if event.keystroke.key == "escape" && view.ai.session.is_none() {
                cx.stop_propagation();
                view.ai_close_bar(window, cx);
            }
        });
        let muted = cx.theme().muted_foreground;
        let status = self.ai_status().map(|status| {
            let line = div().px_1().text_sm().text_color(muted);
            line.debug_selector(|| "ai-status".to_owned()).child(status)
        });
        let width = (view.size.width - px(48.)).min(px(560.)).max(px(240.));
        let bar = v_flex()
            .popover_style(cx)
            .occlude()
            .p_1()
            .gap_1()
            .w(width)
            .debug_selector(|| "ai-bar".to_owned())
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_key_down(escape)
            .children(status)
            .child(
                h_flex()
                    .gap_1()
                    .child(div().flex_1().child(Input::new(&input).small()))
                    .children(buttons),
            );
        let left = view.left() + (view.size.width - width) / 2.;
        let at = point(left, view.bottom() - px(12.));
        let placed = anchored()
            .anchor(Anchor::BottomLeft)
            .position(at)
            .child(bar);
        Some(deferred(placed).with_priority(1).into_any_element())
    }
}

/// Labels the AI cursor in the editor gutter, alongside its row.
pub(super) fn cursor(cx: &App) -> Div {
    div()
        .absolute()
        .left(px(-super::side_menu::GUTTER))
        .top_0()
        .ml_2()
        .px_1()
        .rounded_sm()
        .bg(cx.theme().primary)
        .text_color(cx.theme().primary_foreground)
        .text_xs()
        .debug_selector(|| "ai-cursor".to_owned())
        .child("AI")
}

/// Marks the blocks a session changed, and shows where the model is
/// writing: over the rows, as the selection is.
pub(super) fn marks(view: Entity<EditorView>) -> impl IntoElement {
    let paint = move |_: Bounds<Pixels>, _: (), window: &mut Window, cx: &mut App| {
        let editor = view.read(cx);
        if editor.ai.session.is_none() {
            return;
        }
        let (added, updated) = (cx.theme().green, cx.theme().blue);
        let tint = |color: Hsla| color.opacity(0.12);
        // shortcut: only blocks with text are marked, and removed blocks
        // are counted in the bar; draw both when reviews ask for more.
        for change in &editor.ai.changes {
            let (block, color) = match change {
                Change::Added(block) => (*block, added),
                Change::Updated(block) => (*block, updated),
                Change::Removed(_) => continue,
            };
            if let Some(bounds) = editor.text_bounds(block) {
                window.paint_quad(fill(bounds, tint(color)));
            }
        }
        let caret = editor.ai.session.as_ref().and_then(|session| {
            let EditLocation::Block(block) = session.edit_location()? else {
                return None;
            };
            if session.state() != &AiState::Writing {
                return None;
            }
            let end = editor.editor.document().text_len(block).ok()?;
            editor.layouts.get(&block)?.caret(end, Affinity::default())
        });
        if let Some(caret) = caret {
            window.paint_quad(fill(caret, cx.theme().primary));
        }
    };
    canvas(|_, _, _| (), paint).absolute().size_full()
}
