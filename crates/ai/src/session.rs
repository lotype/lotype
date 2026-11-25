//! A chat with a model about a document, and the copy the model edits.

use std::collections::{HashMap, HashSet};
use std::fmt;

use lotype_core::{BlockId, Document, Editor, Selection};

use crate::apply::{self, Effect};
use crate::context::{self, DocumentState};
use crate::{AiTranscript, LlmEvent, LlmProvider, LlmRequest, Message, Op, Role, partial, tools};

/// Where a session is.
#[derive(Debug, Clone, PartialEq)]
pub enum AiState {
    /// Waiting for the user to ask, with nothing to review.
    UserInput,
    /// Asked, and nothing has come of the answer yet.
    Thinking,
    /// The answer is coming, and its edits are being made in the copy.
    Writing,
    /// The answer is whole and its edits wait to be accepted, or the user
    /// to ask for more.
    Reviewing,
    /// The request failed, for the reason here. It can be tried again.
    Error(String),
    /// The edits were accepted. Nothing more is done with the session.
    Closed,
}

/// Why a session did not do what it was asked.
#[derive(Debug)]
pub enum AiError {
    /// The session is not where that can be done: see [`AiState`].
    State,
    /// The document refused.
    Document(lotype_core::Error),
}

impl fmt::Display for AiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::State => f.write_str("the session is not in a state for that"),
            Self::Document(error) => error.fmt(f),
        }
    }
}

impl std::error::Error for AiError {}

impl From<lotype_core::Error> for AiError {
    fn from(error: lotype_core::Error) -> Self {
        Self::Document(error)
    }
}

/// A block a session's edits changed, for the user to look at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Change {
    /// A block of the copy that the document has not.
    Added(BlockId),
    /// A block of both, changed in the copy.
    Updated(BlockId),
    /// A block of the document that the copy no longer has.
    Removed(BlockId),
}

/// What accepting a session's edits did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Accepted {
    /// The edits made in the document.
    pub applied: usize,
    /// Those left out: their block was gone from the document, or someone
    /// had changed it since the session started.
    pub skipped: usize,
}

/// An edit made in the copy, with what making it again elsewhere takes.
struct Done {
    op: Op,
    effect: Effect,
}

/// A call that is still coming.
struct Call {
    id: String,
    name: String,
    args: String,
    /// What it asked for when last read, made in the copy as one undo
    /// step, to take back when it asks for more.
    made: Option<(Op, Effect)>,
}

/// A chat with a model about a document. The model edits a copy of the
/// document that no one else sees; the user reviews the copy against the
/// document and accepts the edits, as one undo step, or drops the session
/// and with it the copy.
pub struct AiSession {
    copy: Editor,
    selection: Option<Selection>,
    state: AiState,
    transcript: AiTranscript,
    done: Vec<Done>,
    refused: usize,
    /// The copy and the edits made as they were when the user last asked.
    turn: (Vec<u8>, usize),
    reply: String,
    call: Option<Call>,
}

impl AiSession {
    /// A session on the document of `editor` as it is now, with its
    /// selection as what the user means.
    pub fn new(editor: &Editor) -> Result<Self, AiError> {
        let snapshot = editor.document().export_snapshot()?;
        Ok(Self {
            copy: copy_of(editor.document(), &snapshot)?,
            selection: editor.selection().cloned(),
            state: AiState::UserInput,
            transcript: AiTranscript::default(),
            done: Vec::new(),
            refused: 0,
            turn: (snapshot, 0),
            reply: String::new(),
            call: None,
        })
    }

    /// Goes on from a chat that was kept.
    pub fn with_transcript(mut self, transcript: AiTranscript) -> Self {
        self.transcript = transcript;
        self
    }

    /// Where the session is.
    pub fn state(&self) -> &AiState {
        &self.state
    }

    /// The copy, with the model's edits so far.
    pub fn document(&self) -> &Document {
        self.copy.document()
    }

    /// What was said so far.
    pub fn transcript(&self) -> &AiTranscript {
        &self.transcript
    }

    /// The edits made in the copy, in order.
    pub fn ops(&self) -> impl Iterator<Item = &Op> {
        self.done.iter().map(|done| &done.op)
    }

    /// How many calls of the model were no edit that could be made: of no
    /// tool, with arguments that are none, or on a block that is not there.
    pub fn refused(&self) -> usize {
        self.refused
    }

    /// The user asks. Gives the request to send a model, whose answer goes
    /// to [`AiSession::feed`] as it comes.
    pub fn prompt(&mut self, text: &str) -> Result<LlmRequest, AiError> {
        if !matches!(self.state, AiState::UserInput | AiState::Reviewing) {
            return Err(AiError::State);
        }
        self.turn = (self.copy.document().export_snapshot()?, self.done.len());
        let (role, text) = (Role::User, text.to_owned());
        self.transcript.messages.push(Message { role, text });
        Ok(self.request())
    }

    fn request(&mut self) -> LlmRequest {
        self.state = AiState::Thinking;
        self.reply.clear();
        let doc = self.copy.document();
        let selection = self.selection.as_ref();
        let selection = selection.filter(|selection| doc.check_selection(selection).is_ok());
        let state = DocumentState::of(doc, selection);
        // The document goes with the last thing asked, as it is now.
        let mut messages = self.transcript.messages.clone();
        if let Some(asked) = messages.last_mut() {
            asked.text = context::user(&state, &asked.text);
        }
        LlmRequest {
            system: context::system(doc.schema()),
            messages,
            tools: tools::specs(),
        }
    }

    /// Takes a part of the model's answer. An edit is made in the copy as
    /// its call comes, text that is still being written as far as it goes.
    pub fn feed(&mut self, event: LlmEvent) -> Result<(), AiError> {
        if !matches!(self.state, AiState::Thinking | AiState::Writing) {
            return Err(AiError::State);
        }
        self.state = AiState::Writing;
        match event {
            LlmEvent::TextDelta(text) => self.reply.push_str(&text),
            LlmEvent::ToolCallDelta {
                id,
                name,
                args_delta,
            } => {
                if self.call.as_ref().is_some_and(|call| call.id != id) {
                    self.end_call()?;
                }
                let (args, made) = (String::new(), None);
                let call = self.call.get_or_insert(Call {
                    id,
                    name,
                    args,
                    made,
                });
                call.args.push_str(&args_delta);
                self.write(false)?;
            }
            LlmEvent::Done => {
                self.end_call()?;
                self.end_turn();
            }
        }
        Ok(())
    }

    /// Makes what the call asks for so far, in place of what it asked for
    /// before. With `whole`, the call has ended: what is no edit by now is
    /// none.
    fn write(&mut self, whole: bool) -> Result<(), AiError> {
        let Some(call) = &self.call else {
            return Ok(());
        };
        let args = partial::read(&call.args);
        let op = match args {
            Ok(Some(args)) if args.whole || !whole => tools::read(&call.name, &args).ok(),
            _ => None,
        };
        // Nothing to read yet, or nothing new: what was made stays.
        let asked = match (op, whole) {
            (Some(Some(op)), _) => Some(op),
            (_, false) => return Ok(()),
            (_, true) => None,
        };
        if asked.is_some() && asked.as_ref() == call.made.as_ref().map(|(op, _)| op) {
            return Ok(());
        }
        if self
            .call
            .as_mut()
            .and_then(|call| call.made.take())
            .is_some()
        {
            self.copy.undo()?;
        }
        let made = match &asked {
            Some(op) => self.make(op).map(|effect| (op.clone(), effect)),
            None => None,
        };
        if let Some(call) = &mut self.call {
            call.made = made;
        }
        Ok(())
    }

    /// Makes an edit in the copy, as one undo step. `None` where it cannot
    /// be made, or makes nothing.
    fn make(&mut self, op: &Op) -> Option<Effect> {
        let before = self.copy.document().version();
        let mut effect = None;
        let made = self.copy.apply(|doc, _| {
            effect = apply::run(doc, op, &HashMap::new(), None)?;
            Ok(None)
        });
        let changed = self.copy.document().version() != before;
        effect.filter(|_| made.is_ok() && changed)
    }

    /// The call that was coming is whole, and stays as it was made.
    fn end_call(&mut self) -> Result<(), AiError> {
        self.write(true)?;
        match self.call.take().map(|call| call.made) {
            Some(Some((op, effect))) => self.done.push(Done { op, effect }),
            Some(None) => self.refused += 1,
            None => {}
        }
        Ok(())
    }

    fn end_turn(&mut self) {
        let text = std::mem::take(&mut self.reply);
        if !text.is_empty() {
            let role = Role::Assistant;
            self.transcript.messages.push(Message { role, text });
        }
        self.state = match self.done.is_empty() {
            true => AiState::UserInput,
            false => AiState::Reviewing,
        };
    }

    /// Takes back the edit of a call that did not end.
    fn drop_call(&mut self) -> Result<(), AiError> {
        if self.call.take().and_then(|call| call.made).is_some() {
            self.copy.undo()?;
        }
        Ok(())
    }

    /// The request failed. The edits of the calls that ended stay in the
    /// copy until the request is tried again.
    pub fn fail(&mut self, error: &dyn fmt::Display) -> Result<(), AiError> {
        if !matches!(self.state, AiState::Thinking | AiState::Writing) {
            return Err(AiError::State);
        }
        self.drop_call()?;
        self.state = AiState::Error(error.to_string());
        Ok(())
    }

    /// The user stops the answer. What it did so far is there to review.
    pub fn abort(&mut self) -> Result<(), AiError> {
        if !matches!(self.state, AiState::Thinking | AiState::Writing) {
            return Err(AiError::State);
        }
        self.drop_call()?;
        self.end_turn();
        Ok(())
    }

    /// Asks again what failed, with the copy as it was when the user asked.
    pub fn retry(&mut self) -> Result<LlmRequest, AiError> {
        if !matches!(self.state, AiState::Error(_)) {
            return Err(AiError::State);
        }
        self.copy = copy_of(self.copy.document(), &self.turn.0)?;
        self.done.truncate(self.turn.1);
        Ok(self.request())
    }

    /// Sends `request` to `provider` and takes its answer to the end. A
    /// request that fails leaves the session in [`AiState::Error`].
    pub async fn run(
        &mut self,
        provider: &dyn LlmProvider,
        request: LlmRequest,
    ) -> Result<(), AiError> {
        let mut stream = provider.stream(request);
        loop {
            let next = std::future::poll_fn(|cx| stream.as_mut().poll_next(cx));
            match next.await {
                Some(Ok(event)) => {
                    let done = event == LlmEvent::Done;
                    self.feed(event)?;
                    if done {
                        return Ok(());
                    }
                }
                Some(Err(error)) => return self.fail(&error),
                // An answer that ends without saying so has ended.
                None => return self.feed(LlmEvent::Done),
            }
        }
    }

    /// The blocks the edits changed, for a view to mark: those of the copy
    /// in reading order, then those it no longer has.
    pub fn changes(&self) -> Vec<Change> {
        let doc = self.copy.document();
        let (mut added, mut updated, mut removed) = (HashSet::new(), HashSet::new(), Vec::new());
        for Done { op, effect } in &self.done {
            added.extend(effect.made.iter().copied());
            match op {
                Op::Update { block, .. } if effect.made.is_empty() => {
                    updated.insert(*block);
                }
                Op::Update { block, .. } | Op::Delete { block } => removed.push(*block),
                Op::Add { .. } => {}
            }
        }
        let mut changes = Vec::new();
        for block in doc.blocks() {
            if added.contains(&block) {
                changes.push(Change::Added(block));
            } else if updated.contains(&block) {
                changes.push(Change::Updated(block));
            }
        }
        // What the model made and took out again was never the document's.
        removed.retain(|block| !added.contains(block));
        changes.extend(removed.into_iter().map(Change::Removed));
        changes
    }

    /// Makes the edits in the document of `editor`, as one undo step, and
    /// closes the session. The document may have changed since the session
    /// started: each edit is made again in it as it is now, so what others
    /// did stays, and an edit whose block is gone or changed is left out.
    pub fn accept(&mut self, editor: &mut Editor) -> Result<Accepted, AiError> {
        if !matches!(self.state, AiState::Reviewing | AiState::UserInput) {
            return Err(AiError::State);
        }
        let (mut applied, mut skipped) = (0, 0);
        let done = &self.done;
        editor.apply(|doc, selection| {
            // The blocks made here, by what they are in the copy.
            let mut made = HashMap::new();
            for Done { op, effect } in done {
                match apply::run(doc, op, &made, effect.before.as_ref())? {
                    Some(here) => {
                        made.extend(effect.made.iter().copied().zip(here.made));
                        applied += 1;
                    }
                    None => skipped += 1,
                }
            }
            let selection = selection.filter(|selection| doc.check_selection(selection).is_ok());
            Ok(selection.cloned())
        })?;
        self.state = AiState::Closed;
        Ok(Accepted { applied, skipped })
    }
}

/// A document of its own that starts as `doc` is in `snapshot`.
fn copy_of(doc: &Document, snapshot: &[u8]) -> Result<Editor, AiError> {
    let mut copy = Document::new(doc.schema().clone());
    copy.import(snapshot)?;
    Ok(Editor::new(copy))
}
