//! AI in the editor: a prompt, the model's edits shown as they come, and
//! their review.
//!
//! While a session is open the view shows the session's copy of the
//! document, in an editor of its own that nothing writes in but the model:
//! the document's editor is put aside, and goes on taking the changes of
//! other peers. Accepting makes the edits in it and shows it again.

use std::sync::Arc;
use std::time::{Duration, Instant};

use gpui::{Context, Entity, Subscription, Task, Window, prelude::*};
use gpui_component::input::{InputEvent, InputState};
use lotype_ai::{
    Accepted, AiSession, AiState, AiTranscript, Change, LlmError, LlmEvent, LlmProvider,
};
use lotype_core::{BlockKind, Document, Editor, Pos, Selection, SlashItem};

use super::EditorView;

/// How long what comes of an answer waits for more, to be taken together.
const FRAME: Duration = Duration::from_millis(16);

/// The AI's state between frames.
#[derive(Default)]
pub(crate) struct Ai {
    provider: Option<Arc<dyn LlmProvider>>,
    /// What was said, in the sessions before this one.
    transcript: AiTranscript,
    pub(super) session: Option<AiSession>,
    /// The document's editor, while the view shows a session's copy.
    pub(super) main: Option<Editor>,
    /// What came of the answer and is not taken yet, and when the last
    /// was: text is revealed a frame's worth at a time.
    arrived: Vec<Result<LlmEvent, LlmError>>,
    taken: Option<Instant>,
    flush: Option<Task<()>>,
    /// Reads the answer. Dropping it cancels the request.
    reading: Option<Task<()>>,
    /// The field to ask in, while the bar is open.
    pub(super) field: Option<(Entity<InputState>, Subscription)>,
    /// The blocks the session changed, as of the last frame.
    pub(super) changes: Vec<Change>,
    /// The slash menu's entry asked for the bar, which the next frame opens.
    pub(super) wanted: bool,
}

/// The slash menu's entry for the prompt.
pub(super) fn slash_item() -> SlashItem {
    SlashItem {
        key: "ai".to_owned(),
        title: "Ask AI".to_owned(),
        subtext: "Write or edit with a prompt".to_owned(),
        group: "AI".to_owned(),
        aliases: vec!["ask".to_owned(), "write".to_owned()],
        block_type: String::new(),
        attrs: Vec::new(),
    }
}

/// An editor of a document of its own that starts as `doc` is.
fn copy_of(doc: &Document) -> Option<Editor> {
    let mut copy = Document::new(doc.schema().clone());
    copy.import(&doc.export_snapshot().ok()?).ok()?;
    Some(Editor::new(copy))
}

impl EditorView {
    /// Gives the editor a model to ask: a button in the formatting toolbar,
    /// an entry in the slash menu and Space in an empty paragraph open a
    /// prompt, and the model's edits are shown for review before they are
    /// the document's.
    ///
    /// The answer is read where the view runs. A provider whose stream
    /// needs a runtime of its own, as Tokio's, reads it there and passes
    /// the events on through a channel.
    pub fn with_ai(mut self, provider: Arc<dyn LlmProvider>) -> Self {
        self.ai.provider = Some(provider);
        self
    }

    /// Whether the editor has a model to ask.
    pub fn has_ai(&self) -> bool {
        self.ai.provider.is_some()
    }

    /// Where the open session is, if one is.
    pub fn ai_state(&self) -> Option<&AiState> {
        self.ai.session.as_ref().map(AiSession::state)
    }

    /// What was said to and by the model, in this session and those before.
    pub fn ai_transcript(&self) -> &AiTranscript {
        let session = self.ai.session.as_ref();
        session.map_or(&self.ai.transcript, AiSession::transcript)
    }

    /// The session's copy of the document, which the view shows while a
    /// session is open.
    pub fn ai_document(&self) -> Option<&Document> {
        self.ai.session.as_ref().map(AiSession::document)
    }

    /// Asks the model, about the document and what is selected in it. The
    /// first question opens a session; one asked while edits wait for
    /// review goes on from them.
    pub fn ai_ask(&mut self, text: &str, cx: &mut Context<Self>) {
        if self.ai.provider.is_none() || text.trim().is_empty() {
            return;
        }
        if self.ai.session.is_none() {
            if let Some(composition) = self.composition.take() {
                self.commit(&composition.text);
            }
            let Ok(session) = AiSession::new(&self.editor) else {
                return;
            };
            let Some(copy) = copy_of(session.document()) else {
                return;
            };
            self.dismiss_edit_controls();
            let transcript = std::mem::take(&mut self.ai.transcript);
            self.ai.session = Some(session.with_transcript(transcript));
            self.ai.main = Some(std::mem::replace(&mut self.editor, copy));
        }
        let asked = self.ai.session.as_mut().map(|session| session.prompt(text));
        if let Some(Ok(request)) = asked {
            self.ai_send(request, cx);
        }
        cx.notify();
    }

    /// Drops editing popups, delayed callbacks and gestures before showing the draft.
    fn dismiss_edit_controls(&mut self) {
        self.side_menu = Default::default();
        self.slash = Default::default();
        self.toolbar = Default::default();
        self.links = Default::default();
        self.alert_menu = None;
        self.table_menu = None;
        self.table_hover = None;
        self.block_range = None;
        self.drag_anchor = None;
        self.dragging = false;
    }

    /// Sends a request and reads its answer, an event at a time.
    fn ai_send(&mut self, request: lotype_ai::LlmRequest, cx: &mut Context<Self>) {
        let Some(provider) = self.ai.provider.clone() else {
            return;
        };
        self.ai.arrived.clear();
        self.ai.taken = Some(Instant::now());
        self.ai.reading = Some(cx.spawn(async move |view, cx| {
            let mut stream = provider.stream(request);
            loop {
                let next = std::future::poll_fn(|cx| stream.as_mut().poll_next(cx)).await;
                // An answer that ends without saying so has ended.
                let event = next.unwrap_or(Ok(LlmEvent::Done));
                let last = !matches!(&event, Ok(event) if *event != LlmEvent::Done);
                let told = view.update(cx, |view, cx| {
                    view.ai.arrived.push(event);
                    let wait = view.ai.taken.map_or(Duration::ZERO, |taken| {
                        FRAME.saturating_sub(taken.elapsed())
                    });
                    if last || wait.is_zero() {
                        view.ai_take();
                        cx.notify();
                    } else if view.ai.flush.is_none() {
                        view.ai.flush = Some(cx.spawn(async move |view, cx| {
                            cx.background_executor().timer(wait).await;
                            let _ = view.update(cx, |view, cx| {
                                view.ai_take();
                                cx.notify();
                            });
                        }));
                    }
                });
                if last || told.is_err() {
                    return;
                }
            }
        }));
    }

    /// Takes what came of the answer since the last time, in one go.
    fn ai_take(&mut self) {
        self.ai.flush = None;
        self.ai.taken = Some(Instant::now());
        let arrived = std::mem::take(&mut self.ai.arrived);
        let Some(session) = &mut self.ai.session else {
            return;
        };
        if arrived.is_empty() {
            return;
        }
        // More of one call is one event.
        let mut events: Vec<Result<LlmEvent, LlmError>> = Vec::new();
        for event in arrived {
            match (events.last_mut(), event) {
                (
                    Some(Ok(LlmEvent::ToolCallDelta { id, args_delta, .. })),
                    Ok(LlmEvent::ToolCallDelta {
                        id: next,
                        args_delta: more,
                        ..
                    }),
                ) if *id == next => args_delta.push_str(&more),
                (_, event) => events.push(event),
            }
        }
        for event in events {
            let taken = match event {
                Ok(event) => session.feed(event),
                Err(error) => session.fail(&error),
            };
            if taken.is_err() {
                break;
            }
        }
        self.ai_show();
    }

    /// Brings the view up to the session: its copy's changes into the
    /// editor shown, and an answer that left nothing to review closes it.
    fn ai_show(&mut self) {
        let Some(session) = &self.ai.session else {
            return;
        };
        let shown = self.editor.document().version();
        if let Ok(updates) = session.document().export_updates(&shown) {
            self.editor.import(&updates).ok();
        }
        self.ai.changes = session.changes();
        if *session.state() == AiState::UserInput {
            self.ai_close();
        }
    }

    /// Shows the document again, and keeps what was said.
    fn ai_close(&mut self) {
        self.ai.reading = None;
        self.ai.flush = None;
        self.ai.arrived.clear();
        self.ai.changes.clear();
        if let Some(session) = self.ai.session.take() {
            self.ai.transcript = session.transcript().clone();
        }
        if let Some(main) = self.ai.main.take() {
            self.editor = main;
        }
    }

    /// Stops the answer that is coming. The edits that were whole stay to
    /// be reviewed.
    pub fn ai_stop(&mut self, cx: &mut Context<Self>) {
        self.ai.reading = None;
        self.ai.flush = None;
        self.ai.arrived.clear();
        if let Some(session) = &mut self.ai.session {
            session.abort().ok();
        }
        self.ai_show();
        cx.notify();
    }

    /// Asks again what failed.
    pub fn ai_retry(&mut self, cx: &mut Context<Self>) {
        let again = self.ai.session.as_mut().map(AiSession::retry);
        let Some(Ok(request)) = again else {
            return;
        };
        // The copy is as it was when the user asked, which no update says.
        let copy = self.ai.session.as_ref().map(AiSession::document);
        if let Some(copy) = copy.and_then(copy_of) {
            self.editor = copy;
        }
        self.ai.changes.clear();
        self.ai_send(request, cx);
        cx.notify();
    }

    /// Makes the session's edits in the document, as one undo step, and
    /// shows the document again. Gives how many were made and how many
    /// left out, as others had changed their blocks; `None` when there is
    /// nothing to accept, or the document refused.
    pub fn ai_accept(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Option<Accepted> {
        let (session, main) = (self.ai.session.as_mut()?, self.ai.main.as_mut()?);
        let accepted = session.accept(main).ok()?;
        self.ai_close();
        self.ai_close_bar(window, cx);
        self.selection_changed(window, cx);
        Some(accepted)
    }

    /// Drops the session and its edits, and shows the document again.
    pub fn ai_reject(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.ai_close();
        self.ai_close_bar(window, cx);
        self.selection_changed(window, cx);
    }

    /// Opens the bar to ask in, at the foot of the editor.
    pub fn open_ai_prompt(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.ai.provider.is_none() || self.ai.field.is_some() {
            return;
        }
        let input =
            cx.new(|cx| InputState::new(window, cx).placeholder("Ask AI to write or edit…"));
        let events = cx.subscribe_in(&input, window, |view, input, event, window, cx| {
            match event {
                InputEvent::PressEnter { .. } => {
                    let text = input.read(cx).value().to_string();
                    input.update(cx, |input, cx| input.set_value("", window, cx));
                    view.ai_ask(&text, cx);
                }
                // The focus went elsewhere, where it stays, and with no
                // session there is nothing the bar is open for.
                InputEvent::Blur if window.is_window_active() && view.ai.session.is_none() => {
                    view.ai.field = None;
                    cx.notify();
                }
                _ => {}
            }
        });
        input.update(cx, |input, cx| input.focus(window, cx));
        self.ai.field = Some((input, events));
        cx.notify();
    }

    pub(super) fn ai_close_bar(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.ai.field.take().is_some() {
            window.focus(&self.focus, cx);
        }
        cx.notify();
    }

    /// Space in an empty paragraph opens the prompt instead of writing a
    /// space. Returns whether it did.
    pub(super) fn ai_space(
        &mut self,
        text: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let caret = match self.editor.selection() {
            Some(Selection::Text { anchor, head }) if anchor == head => Some(*head),
            _ => None,
        };
        let doc = self.editor.document();
        let empty = caret.filter(|caret| {
            BlockKind::of(doc, caret.block) == Some(BlockKind::Paragraph)
                && doc.text_len(caret.block).is_ok_and(|len| len == 0)
                && caret.block.column().is_none()
        });
        if text != " " || empty.is_none() || !self.has_ai() || self.composition.is_some() {
            return false;
        }
        self.open_ai_prompt(window, cx);
        true
    }

    /// The slash menu's entry: takes the query out and opens the prompt.
    pub(super) fn ai_from_slash(&mut self) {
        if let Some(query) = self.editor.slash_query() {
            let caret = Selection::caret(Pos::new(query.block, query.range.start));
            let cleared = self.editor.apply(|doc, _| {
                doc.delete_text(query.block, query.range)?;
                Ok(Some(caret))
            });
            cleared.ok();
        }
        self.ai.wanted = true;
    }
}
