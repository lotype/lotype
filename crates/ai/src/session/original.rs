//! What a block said before the model changed it, for a review to show
//! against what it says now, whatever others did to the document since.

use lotype_core::{BlockId, Span};

use super::AiSession;
use crate::Op;

impl AiSession {
    /// Keeps the text of the block `op` changes or removes, before the
    /// first edit of the session there.
    pub(super) fn keep_original(&mut self, op: &Op) {
        let (Op::Update { block, .. }
        | Op::ReplaceSelectedText { block, .. }
        | Op::Delete { block }) = op
        else {
            return;
        };
        if !self.original.contains_key(block)
            && let Ok(spans) = self.copy.document().spans(*block)
        {
            self.original.insert(*block, spans);
        }
    }

    /// The text of `block` as it was before the session changed or removed
    /// it. `None` for a block the session left alone, or one without text.
    pub fn original_spans(&self, block: BlockId) -> Option<&[Span]> {
        self.original.get(&block).map(Vec::as_slice)
    }
}
