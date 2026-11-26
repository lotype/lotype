//! AI in the editor: a prompt, the model's edits shown as they come, and
//! their review.
//!
//! While a session is open the view shows the session's copy of the
//! document, in an editor of its own that nothing writes in but the model:
//! the document's editor is put aside, and goes on taking the changes of
//! other peers. Accepting makes the edits in it and shows it again.

use std::collections::HashSet;
use std::sync::Arc;
use std::time::{Duration, Instant};

use gpui::{Context, Task, Window};
use lotype_ai::{
    Accepted, AiSession, AiState, AiTranscript, Change, LlmError, LlmEvent, LlmProvider,
};
use lotype_core::{BlockId, Document, Editor, Selection};

use super::EditorView;
use super::ai_prompt::PromptField;
use super::popup::Placement;

/// How long what comes of an answer waits for more, to be taken together.
const FRAME: Duration = Duration::from_millis(16);

/// How long the light on "Working…" stays on a letter. Each step draws
/// the window again, so it takes them only while a frame is cheap: eight
/// frames a second within the caret's budget are under 7% of a core.
const GLOW: Duration = Duration::from_millis(120);

/// The surface and context that own an AI request.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum PromptScope {
    Inline,
    Selection,
    Document,
}

/// The AI's state between frames.
#[derive(Default)]
pub(crate) struct Ai {
    provider: Option<Arc<dyn LlmProvider>>,
    transcript: AiTranscript,
    pub(super) scope: Option<PromptScope>,
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
    /// Moves the light on "Working…" while the model answers, and how
    /// far it has moved.
    working: Option<Task<()>>,
    pub(super) glow: usize,
    /// The field to ask in, while the bar is open.
    pub(super) field: Option<PromptField>,
    pub(super) place: Placement,
    /// The blocks the session changed, as of the last frame.
    pub(super) changes: Vec<Change>,
    /// Those of them this frame shows the changed words of, which need
    /// no tint over the whole block.
    pub(super) diffed: HashSet<BlockId>,
    /// The slash menu's entry asked for the bar, which the next frame opens.
    pub(super) wanted: bool,
}

/// An editor of a document of its own that starts as `doc` is.
fn copy_of(doc: &Document) -> Option<Editor> {
    let mut copy = Document::new(doc.schema().clone());
    copy.import(&doc.export_snapshot().ok()?).ok()?;
    Some(Editor::new(copy))
}

impl EditorView {
    /// Enables inline writing, selection suggestions and whole-document chat.
    /// The model's edits stay in a private preview until accepted.
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

    /// The shared history of inline, selection and whole-document requests.
    pub fn ai_transcript(&self) -> &AiTranscript {
        let session = self.ai.session.as_ref();
        session.map_or(&self.ai.transcript, AiSession::transcript)
    }

    /// The state of the document chat's active request or review.
    pub fn ai_chat_state(&self) -> Option<&AiState> {
        if self.ai.scope == Some(PromptScope::Document) {
            self.ai_state()
        } else {
            None
        }
    }

    /// The text arriving in the shared thread.
    pub(crate) fn ai_streaming_reply(&self) -> Option<&str> {
        self.ai.session.as_ref()?.streaming_reply()
    }

    /// The session's copy of the document, which the view shows while a
    /// session is open.
    pub fn ai_document(&self) -> Option<&Document> {
        self.ai.session.as_ref().map(AiSession::document)
    }

    /// Asks about the current selection or caret in the contextual prompt.
    pub fn ai_ask(&mut self, text: &str, cx: &mut Context<Self>) {
        let scope = self
            .ai
            .field
            .as_ref()
            .map_or_else(|| self.context_scope(), |field| field.scope);
        self.ai_submit(text, scope, cx);
    }

    /// Sends a question to the whole-document chat, without the editor selection.
    /// Returns whether the request was queued.
    pub fn ai_ask_document(&mut self, text: &str, cx: &mut Context<Self>) -> bool {
        self.ai_submit(text, PromptScope::Document, cx)
    }

    /// Whether the document chat can send another question.
    pub(crate) fn ai_chat_can_send(&self) -> bool {
        self.ai_can_submit(PromptScope::Document)
    }

    /// Inferred context for a prompt opened by the application.
    pub(super) fn context_scope(&self) -> PromptScope {
        if matches!(self.editor().selection(), Some(Selection::Text { anchor, head }) if anchor != head)
        {
            PromptScope::Selection
        } else {
            PromptScope::Inline
        }
    }

    /// Whether this surface owns the current turn, and is allowed to ask.
    pub(super) fn ai_can_submit(&self, scope: PromptScope) -> bool {
        self.has_ai()
            && match self.ai_state() {
                None => scope != PromptScope::Selection || self.context_scope() == scope,
                Some(AiState::Reviewing) => self.ai.scope == Some(scope),
                _ => false,
            }
    }

    /// Starts or continues a session owned by one surface.
    pub(super) fn ai_submit(
        &mut self,
        text: &str,
        scope: PromptScope,
        cx: &mut Context<Self>,
    ) -> bool {
        let text = text.trim();
        if text.is_empty() || !self.ai_can_submit(scope) {
            return false;
        }
        if self.ai.session.is_none() {
            if let Some(composition) = self.composition.take() {
                self.commit(&composition.text);
            }
            let session = match scope {
                PromptScope::Document => AiSession::for_document(self.editor.document()),
                PromptScope::Selection => AiSession::for_selection(&self.editor),
                PromptScope::Inline => AiSession::new(&self.editor),
            };
            let Ok(session) = session else {
                return false;
            };
            let Some(copy) = copy_of(session.document()) else {
                return false;
            };
            let mut session = session.with_transcript(self.ai.transcript.clone());
            let Ok(request) = session.prompt(text) else {
                return false;
            };
            self.dismiss_edit_controls();
            if scope == PromptScope::Document {
                self.ai.field = None;
            }
            self.ai.scope = Some(scope);
            self.ai.session = Some(session);
            self.ai.main = Some(std::mem::replace(&mut self.editor, copy));
            self.ai_send(request, cx);
        } else {
            let Some(Ok(request)) = self.ai.session.as_mut().map(|session| session.prompt(text))
            else {
                return false;
            };
            self.ai_send(request, cx);
        }
        cx.notify();
        true
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
    fn ai_send(&mut self, mut request: lotype_ai::LlmRequest, cx: &mut Context<Self>) {
        let Some(provider) = self.ai.provider.clone() else {
            return;
        };
        if self.ai.scope == Some(PromptScope::Inline) {
            request.system.push_str(
                "\n\nThis is an inline writing prompt. When a cursor block is provided, use it as the place to write unless the user asks for another location.",
            );
        }
        self.ai.arrived.clear();
        self.ai.taken = Some(cx.background_executor().now());
        self.ai.working = Some(cx.spawn(async move |view, cx| {
            loop {
                cx.background_executor().timer(GLOW).await;
                let working = view.update(cx, |view, cx| {
                    let state = view.ai_state();
                    if !matches!(state, Some(AiState::Thinking | AiState::Writing)) {
                        return false;
                    }
                    if view.frame_is_cheap() {
                        view.ai.glow = view.ai.glow.wrapping_add(1);
                        cx.notify();
                    }
                    true
                });
                if !matches!(working, Ok(true)) {
                    return;
                }
            }
        }));
        self.ai.reading = Some(cx.spawn(async move |view, cx| {
            let mut stream = provider.stream(request);
            loop {
                let next = std::future::poll_fn(|cx| stream.as_mut().poll_next(cx)).await;
                // An answer that ends without saying so has ended.
                let event = next.unwrap_or(Ok(LlmEvent::Done));
                let last = !matches!(&event, Ok(event) if *event != LlmEvent::Done);
                let told = view.update(cx, |view, cx| {
                    view.ai.arrived.push(event);
                    let now = cx.background_executor().now();
                    let wait = view.ai.taken.map_or(Duration::ZERO, |taken| {
                        FRAME.saturating_sub(now.saturating_duration_since(taken))
                    });
                    if last || wait.is_zero() {
                        view.ai_take(now);
                        cx.notify();
                    } else if view.ai.flush.is_none() {
                        view.ai.flush = Some(cx.spawn(async move |view, cx| {
                            cx.background_executor().timer(wait).await;
                            let _ = view.update(cx, |view, cx| {
                                view.ai_take(cx.background_executor().now());
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
    fn ai_take(&mut self, now: Instant) {
        self.ai.flush = None;
        self.ai.taken = Some(now);
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
        self.ai.working = None;
        self.ai.flush = None;
        self.ai.arrived.clear();
        self.ai.changes.clear();
        if let Some(session) = self.ai.session.take() {
            self.ai.transcript = session.transcript().clone();
        }
        self.ai.scope = None;
        if let Some(main) = self.ai.main.take() {
            self.editor = main;
        }
    }

    /// Stops the answer that is coming. The edits that were whole stay to
    /// be reviewed.
    pub fn ai_stop(&mut self, cx: &mut Context<Self>) {
        self.ai.reading = None;
        self.ai.working = None;
        // The provider has already sent these events. A later call in the
        // batch can end an earlier one even if the frame timer has not fired.
        self.ai_take(cx.background_executor().now());
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
}
