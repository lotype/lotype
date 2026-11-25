//! A model that says what a test tells it to.

use std::collections::VecDeque;
use std::pin::Pin;
use std::sync::Mutex;
use std::task::{Context, Poll};

use futures_core::Stream;

use crate::{LlmError, LlmEvent, LlmProvider, LlmRequest, LlmStream};

/// A provider with its answers written beforehand: each request gets the
/// next, and is kept to be looked at.
#[derive(Default)]
pub struct MockProvider {
    answers: Mutex<VecDeque<Vec<Result<LlmEvent, LlmError>>>>,
    requests: Mutex<Vec<LlmRequest>>,
}

impl MockProvider {
    /// With no answers: every request gets an empty one.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds an answer, for the request after those already answered for.
    pub fn answer(self, events: impl IntoIterator<Item = Result<LlmEvent, LlmError>>) -> Self {
        let mut answers = self.answers.lock().unwrap();
        answers.push_back(events.into_iter().collect());
        drop(answers);
        self
    }

    /// The requests made, in order.
    pub fn requests(&self) -> Vec<LlmRequest> {
        self.requests.lock().unwrap().clone()
    }
}

/// A call to `name` with `args`, as a model sends it: `size` bytes of the
/// arguments at a time, cut between characters.
pub fn tool_call(id: &str, name: &str, args: &str, size: usize) -> Vec<Result<LlmEvent, LlmError>> {
    let mut parts = Vec::new();
    let mut rest = args;
    while !rest.is_empty() {
        let mut end = size.clamp(1, rest.len());
        while !rest.is_char_boundary(end) {
            end += 1;
        }
        let (part, after) = rest.split_at(end);
        rest = after;
        parts.push(Ok(LlmEvent::ToolCallDelta {
            id: id.to_owned(),
            name: name.to_owned(),
            args_delta: part.to_owned(),
        }));
    }
    parts
}

struct Script(VecDeque<Result<LlmEvent, LlmError>>);

impl Stream for Script {
    type Item = Result<LlmEvent, LlmError>;

    fn poll_next(mut self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        Poll::Ready(self.0.pop_front())
    }
}

impl LlmProvider for MockProvider {
    fn stream(&self, request: LlmRequest) -> LlmStream {
        self.requests.lock().unwrap().push(request);
        let answer = self.answers.lock().unwrap().pop_front();
        Box::pin(Script(answer.unwrap_or_default().into()))
    }
}
