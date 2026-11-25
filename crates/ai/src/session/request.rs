//! How the session's scope determines the context and tools a model receives.

use lotype_core::Editor;

use super::{AiError, AiSession, AiState};
use crate::context::{self, DocumentState};
use crate::{LlmRequest, selected, tools};

impl AiSession {
    /// Opens a session whose only edit tool replaces text in the selected ranges.
    ///
    /// Block boundaries, table cells and fragments containing inline atoms are not editable here.
    pub fn for_selection(editor: &Editor) -> Result<Self, AiError> {
        let scope = selected::TextSelection::new(editor.document(), editor.selection())?;
        let mut session = Self::for_document(editor.document())?;
        session.selected = Some(scope);
        Ok(session)
    }

    /// Builds a request using only the tools allowed by the session's scope.
    pub(super) fn request(&mut self) -> LlmRequest {
        self.state = AiState::Thinking;
        self.reply.clear();
        let doc = self.copy.document();
        let (context, system, tools) = if let Some(selected) = &self.selected {
            (
                selected.context(doc),
                selected::SYSTEM.to_owned(),
                vec![tools::selection_spec()],
            )
        } else {
            let selection = self
                .selection
                .as_ref()
                .filter(|selection| doc.check_selection(selection).is_ok());
            (
                DocumentState::of(doc, selection).to_json(),
                context::system(doc.schema()),
                tools::specs(),
            )
        };
        let mut messages = self.transcript.messages.clone();
        if let Some(asked) = messages.last_mut() {
            asked.text = context::user(&context, &asked.text);
        }
        LlmRequest {
            system,
            messages,
            tools,
        }
    }
}
