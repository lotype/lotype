//! The model, as the application brings it.

use std::fmt;

use futures_core::stream::BoxStream;
use serde_json::Value;

/// What a model sends back, as it comes.
pub type LlmStream = BoxStream<'static, Result<LlmEvent, LlmError>>;

/// A model. The application brings one, with its own keys.
pub trait LlmProvider: Send + Sync {
    /// Sends `request` and gives the answer as it comes. Dropping the
    /// stream cancels the request.
    fn stream(&self, request: LlmRequest) -> LlmStream;
}

/// A part of a model's answer.
#[derive(Debug, Clone, PartialEq)]
pub enum LlmEvent {
    /// More of what it says to the user.
    TextDelta(String),
    /// More of a call to a tool. A call's arguments are JSON, in as many
    /// parts as the model sends them in, and a call ends where another
    /// starts or the answer does.
    ToolCallDelta {
        /// Which call, among those of the answer.
        id: String,
        /// The tool called.
        name: String,
        /// More of its arguments.
        args_delta: String,
    },
    /// The answer is whole.
    Done,
}

/// What went wrong with a request, in the provider's words.
#[derive(Debug, Clone, PartialEq)]
pub struct LlmError(pub String);

impl fmt::Display for LlmError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for LlmError {}

/// What to send a model.
#[derive(Debug, Clone, PartialEq)]
pub struct LlmRequest {
    /// What the model is to do, whatever is asked of it.
    pub system: String,
    /// What was said so far, the last of it the user's and to be answered.
    pub messages: Vec<Message>,
    /// The tools the model may call.
    pub tools: Vec<ToolSpec>,
}

/// Something said in a chat.
#[derive(Debug, Clone, PartialEq)]
pub struct Message {
    /// Who said it.
    pub role: Role,
    /// What was said.
    pub text: String,
}

/// Who said a [`Message`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    /// The person.
    User,
    /// The model.
    Assistant,
}

/// A tool a model may call.
#[derive(Debug, Clone, PartialEq)]
pub struct ToolSpec {
    /// The name to call it by.
    pub name: &'static str,
    /// What it does, for the model.
    pub description: &'static str,
    /// Its arguments, as a JSON Schema.
    pub schema: Value,
}
