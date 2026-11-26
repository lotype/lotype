//! What is drawn of a session: the card to ask in and to act on the model's
//! edits, the toolbar's menu for selected text, the marks on the blocks they changed, and where the model writes.

use gpui::{
    AnyElement, App, Bounds, ClickEvent, Context, Div, Entity, Hsla, IntoElement, KeyDownEvent,
    MouseButton, Pixels, TextAlign, TextRun, TransformationMatrix, Window, canvas, div, fill,
    point, prelude::*, px, size,
};
use gpui_component::input::Textarea;
use gpui_component::list::ListItem;
use gpui_component::{ActiveTheme, Sizable, ThemeStyled, h_flex, v_flex};
use lotype_ai::{AiState, Change, EditLocation, Role};
use lotype_core::{BlockId, Pos};

use super::EditorView;
use super::ai::{BarLayout, PromptScope};
use super::ai_controls::{Act, WORKING, send, status, tier, working};
use crate::icons::{self, icon};
use crate::text_el::Affinity;

/// Quick edits offered for selected text, with their request text.
const SUGGESTIONS: [(&str, &str, &str); 3] = [
    (
        "spelling",
        "Fix spelling & grammar",
        "Fix spelling and grammar in the selected text.",
    ),
    (
        "clarity",
        "Improve clarity",
        "Rewrite the selected text for clarity without changing its meaning.",
    ),
    (
        "shorten",
        "Shorten",
        "Shorten the selected text without losing its meaning.",
    ),
];

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

    /// Where the live text caret sits in the shown document while the model writes.
    pub(super) fn ai_writing_pos(&self) -> Option<Pos> {
        let pos = self.ai.session.as_ref()?.writing_pos()?;
        let len = self.editor.document().text_len(pos.block).ok()?;
        (pos.offset <= len).then_some(pos)
    }

    /// The text block the model is writing, where its live cursor is drawn.
    pub(super) fn ai_writing_block(&self) -> Option<BlockId> {
        self.ai_writing_pos().map(|pos| pos.block)
    }

    pub(super) fn ai_writing_caret(&self) -> Option<Bounds<Pixels>> {
        let pos = self.ai_writing_pos()?;
        self.layouts
            .get(&pos.block)?
            .caret(pos.offset, Affinity::default())
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

    /// What the session says, in a line: what the model is doing, that
    /// its edits wait for review and how many blocks they removed that
    /// the view does not show, or its answer where it made no edits.
    pub(crate) fn ai_status(&self) -> Option<String> {
        match self.ai_state() {
            None => self
                .ai_transcript()
                .messages
                .get(self.ai.field.as_ref()?.history_start..)?
                .last()
                .filter(|message| message.role == Role::Assistant)
                .map(|message| message.text.clone()),
            Some(AiState::Thinking | AiState::Writing) => Some(WORKING.to_owned()),
            Some(AiState::Error(error)) => Some(error.clone()),
            Some(AiState::Reviewing) => {
                // Removed blocks with text are shown struck through.
                let removed = self.ai.changes.iter();
                let removed = removed.filter(|change| matches!(change, Change::Removed(_)));
                Some(match removed.count() - self.ai_struck().len() {
                    0 => "Review the changes".to_owned(),
                    1 => "Review the changes · 1 block removed".to_owned(),
                    more => format!("Review the changes · {more} blocks removed"),
                })
            }
            _ => None,
        }
    }

    /// Whether the toolbar's menu for the selected text is open: its field
    /// asks, and no session has started.
    pub(super) fn ai_asks_selection(&self) -> bool {
        let field = self.ai.field.as_ref();
        self.ai.session.is_none()
            && field.is_some_and(|field| field.scope == PromptScope::Selection)
            && self.selected_text().is_some()
    }

    /// Closes a prompt that has no edits to review, on Escape: leaving
    /// those is said with a button.
    fn ai_escape(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if event.keystroke.key == "escape" && self.ai.session.is_none() {
            cx.stop_propagation();
            self.ai_close_bar(window, cx);
        }
    }

    /// The field to ask in under the AI's mark, which grows with what is
    /// written, and the button that sends it: next to the field while the
    /// text is one line, and on a row of its own below once it is more,
    /// where it takes none of the field's width.
    fn ai_field_row(&self, cx: &mut Context<Self>) -> Option<Div> {
        let field = self.ai.field.as_ref()?;
        let text = field.input.read(cx).value();
        let ready = !text.trim().is_empty() && self.ai_can_submit(field.scope);
        let click = cx.listener(|view: &mut Self, _: &ClickEvent, window, cx| {
            view.ai_submit_field(window, cx);
        });
        let send = send("ai-send", ready, cx).on_click(click);
        let sent = div().debug_selector(|| "ai-send".to_owned()).child(send);
        // Text that fit before it grew past one line fits again once it
        // is shorter than it was then: otherwise the two layouts, of
        // different widths, would take turns.
        let (height, line) = (field.height.get(), field.line.get());
        let line = line.into_iter().chain(height).reduce(Pixels::min);
        field.line.set(line);
        let tall = height
            .zip(line)
            .is_some_and(|(height, line)| height > line + px(1.));
        let broken = text.contains('\n');
        match field.wrapped_at.get() {
            None if tall || broken => field.wrapped_at.set(Some(text.len())),
            Some(len) if text.len() < len && !broken => field.wrapped_at.set(None),
            _ => {}
        }
        let below = field.wrapped_at.get().is_some();
        // How tall the field is decides the layout of the next frame.
        let (heights, view) = (field.height.clone(), cx.entity().entity_id());
        let measured = canvas(
            move |bounds: Bounds<Pixels>, window: &mut Window, _: &mut App| {
                if heights.replace(Some(bounds.size.height)) != Some(bounds.size.height) {
                    window.on_next_frame(move |_, cx| cx.notify(view));
                }
            },
            |_, _, _, _| {},
        );
        let input = Textarea::new(&field.input).small().appearance(false);
        let input = div().relative().flex_1().min_w_0().child(input);
        let input = input.child(measured.absolute().top_0().left_0().size_full());
        let mark = div().pl_1().text_color(cx.theme().muted_foreground);
        let mark = mark.child(icon(icons::SPARKLES));
        let row = h_flex().gap_1();
        Some(match below {
            true => v_flex()
                .p_1()
                .gap_1()
                .child(row.items_start().child(mark.pt(px(5.))).child(input))
                .child(h_flex().justify_end().child(sent)),
            false => row
                .p_1()
                .items_center()
                .child(mark)
                .child(input)
                .child(sent),
        })
    }

    /// What an open session says in the card's row.
    fn ai_line(&self, cx: &App) -> Option<AnyElement> {
        let state = self.ai_state()?;
        let said = match state {
            AiState::Thinking | AiState::Writing => working(self.ai.glow, cx).into_any_element(),
            _ => status(self.ai_status()?, Some(state), cx).into_any_element(),
        };
        let line = div().debug_selector(|| "ai-status".to_owned());
        Some(line.child(said).into_any_element())
    }

    /// The answer of a session that made no edits, over the field to ask
    /// more in.
    fn ai_answer(&self, cx: &App) -> Option<Div> {
        if self.ai.session.is_some() {
            return None;
        }
        let said = div().px_2().pt_2().pb_1();
        let said = said.debug_selector(|| "ai-status".to_owned());
        Some(said.child(status(self.ai_status()?, None, cx)))
    }

    /// The toolbar's menu for the selected text: the quick edits, and a
    /// field to ask for another.
    pub(super) fn ai_selection_menu(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !self.ai_asks_selection() {
            return None;
        }
        let focus = self.ai.field.as_ref()?.focus.clone();
        let items = SUGGESTIONS.map(|(id, title, prompt)| {
            let choose = cx.listener(move |view: &mut Self, _: &ClickEvent, window, cx| {
                if view.ai_submit(prompt, PromptScope::Selection, cx) {
                    view.ai_hold_focus(window, cx);
                }
            });
            let row = h_flex()
                .gap_2()
                .text_sm()
                .items_center()
                .child(icon(icons::SPARKLES))
                .child(title);
            div()
                .debug_selector(move || format!("ai-suggestion-{id}"))
                .child(ListItem::new(id).child(row).on_click(choose))
        });
        let border = cx.theme().border;
        let said = self.ai_answer(cx);
        let field = self.ai_field_row(cx);
        let menu = v_flex()
            .popover_style(cx)
            .occlude()
            .overflow_hidden()
            .w(px(300.))
            .debug_selector(|| "ai-menu".to_owned())
            .track_focus(&focus)
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_key_down(cx.listener(Self::ai_escape))
            .children(said.map(|said| said.pb_2().border_b_1().border_color(border)))
            .child(v_flex().p_1().children(items))
            .children(field.map(|field| field.border_t_1().border_color(border)));
        Some(menu.into_any_element())
    }

    pub(super) fn shows_ai_bar(&self) -> bool {
        self.ai.scope != Some(PromptScope::Document)
            && (self.ai.field.is_some() || self.ai.session.is_some())
            && !self.ai_asks_selection()
    }

    /// The card next to the row a prompt was opened at: what the session
    /// says and offers, over the field to ask in.
    pub(super) fn ai_bar(&self, follow_write: bool, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !self.shows_ai_bar() {
            self.ai.bar_height.set(None);
            self.ai.bar_layout.set(None);
            return None;
        }
        let focus = self.ai.field.as_ref().map(|field| field.focus.clone());
        let state = self.ai_state().cloned();
        let review = matches!(state, Some(AiState::Reviewing | AiState::Error(_)));
        let buttons = Act::buttons(state.as_ref(), "ai", cx, Self::ai_act);
        // One row: what the session says and offers, or the field to ask in.
        let row = tier(self.ai_line(cx), buttons);
        let field = match row {
            Some(_) => None,
            None => self.ai_field_row(cx),
        };
        let (scroll, height, measured, entity) = (
            self.scroll.clone(),
            self.ai.bar_height.clone(),
            self.ai.bar_layout.clone(),
            cx.entity(),
        );
        let reveal = canvas(
            move |bounds: Bounds<Pixels>, window: &mut Window, cx: &mut App| {
                height.set(Some(bounds.size.height));
                let layout = Some(BarLayout {
                    height: bounds.size.height,
                    viewport: scroll.bounds().size.height,
                    review,
                });
                if measured.get() != layout {
                    let view = entity.entity_id();
                    if follow_write {
                        window.on_next_frame(move |_, cx| cx.notify(view));
                    } else {
                        measured.set(layout);
                        let keep = entity.read(cx).ai_writing_caret();
                        if super::paint::reveal_bounds_preserving(&scroll, bounds, keep) {
                            window.on_next_frame(move |_, cx| cx.notify(view));
                        }
                    }
                }
            },
            |_, _, _, _| {},
        );
        let theme = cx.theme();
        let bar = v_flex()
            .border_1()
            .border_color(theme.border)
            .rounded(theme.radius)
            .bg(theme.background)
            .overflow_hidden()
            .w_full()
            .max_w(px(560.))
            .debug_selector(|| "ai-bar".to_owned())
            .when_some(focus, |bar, focus| bar.track_focus(&focus))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_key_down(cx.listener(Self::ai_escape))
            .children(self.ai_answer(cx))
            .children(row)
            .children(field);
        Some(
            div()
                .relative()
                .pt(px(4.))
                .pb(px(6.))
                .pr(px(12.))
                .flex_none()
                .child(bar)
                .child(reveal.absolute().top_0().left_0().size_full())
                .into_any_element(),
        )
    }
}

/// Labels the AI cursor in the editor gutter, alongside its row.
pub(super) fn cursor(cx: &App) -> Div {
    h_flex()
        .absolute()
        .left(px(-super::side_menu::GUTTER))
        .top_0()
        .ml_2()
        .px_1()
        .rounded_sm()
        .bg(cx.theme().primary)
        .text_color(cx.theme().primary_foreground)
        .text_xs()
        .gap_0p5()
        .debug_selector(|| "ai-cursor".to_owned())
        .child(icon(icons::SPARKLES).size_3())
        .child("AI")
}

/// Paints the agent's name tag over `caret`, as a peer's name is over theirs.
fn caret_tag(caret: Bounds<Pixels>, window: &mut Window, cx: &mut App) {
    let (ink, paper) = (cx.theme().primary_foreground, cx.theme().primary);
    let (font_size, height, icon) = (px(11.), px(16.), px(10.));
    let run = TextRun {
        len: 2,
        font: window.text_style().font(),
        color: ink,
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    let line = window
        .text_system()
        .shape_line("AI".into(), font_size, &[run], None);
    let origin = point(caret.left(), caret.top() - height);
    let tag = Bounds::new(origin, size(icon + line.width + px(9.), height));
    window.paint_quad(fill(tag, paper));
    let sparkles = Bounds::new(
        origin + point(px(3.), (height - icon) / 2.),
        size(icon, icon),
    );
    let unit = TransformationMatrix::unit();
    let svg = Some(icons::SPARKLES);
    window
        .paint_svg(sparkles, "lotype/sparkles".into(), svg, unit, ink, cx)
        .ok();
    let text = origin + point(icon + px(6.), Pixels::ZERO);
    line.paint(text, height, TextAlign::Left, None, window, cx)
        .ok();
}

/// Marks the blocks a session changed, and shows where the model is
/// writing: over the rows, as the selection is.
pub(super) fn marks(view: Entity<EditorView>) -> impl IntoElement {
    let paint = move |_: Bounds<Pixels>, _: (), window: &mut Window, cx: &mut App| {
        let editor = view.read(cx);
        if editor.ai.session.is_none() {
            return;
        }
        let (added, updated) = (super::ai_review::added_ink(cx), cx.theme().blue);
        let tint = |color: Hsla| color.opacity(0.12);
        // shortcut: only blocks with text are marked, and removed blocks
        // without text are counted in the card; draw both when reviews ask
        // for more.
        for change in &editor.ai.changes {
            let (block, color) = match change {
                Change::Added(block) => (*block, added),
                Change::Updated(block) => (*block, updated),
                Change::Removed(_) => continue,
            };
            // Where the words that changed are marked, the block is not.
            if editor.ai.diffed.contains(&block) {
                continue;
            }
            if let Some(bounds) = editor.text_bounds(block) {
                window.paint_quad(fill(bounds, tint(color)));
            }
        }
        if let Some(caret) = editor.ai_writing_caret() {
            window.paint_quad(fill(caret, cx.theme().primary));
            caret_tag(caret, window, cx);
        }
    };
    canvas(|_, _, _| (), paint).absolute().size_full()
}
