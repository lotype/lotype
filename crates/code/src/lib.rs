//! Code blocks for lotype documents: language metadata, line ranges,
//! JavaScript/TypeScript Twoslash directives, built-in & LSP semantic analysis,
//! and an optional GPUI view.

#![warn(missing_docs)]

mod analyzer;
mod infer;
mod languages;
mod lines;
mod lsp;
mod twoslash;

#[cfg(feature = "gpui")]
mod highlight;
#[cfg(feature = "gpui")]
mod view;

use lotype_core::{BlockId, Document, block};

pub use analyzer::{
    AnalysisFuture, AnalysisRequest, CodeAnalysis, CodeAnalyzer, CompletionItem, CompletionKind,
    CompletionSite, DiagnosticAnnotation, HoverAnnotation, QuerySite, Severity, VirtualFile,
};
pub use infer::BuiltInAnalyzer;
pub use languages::{CodeLanguage, LANGUAGES, canonical_language, language_label};
pub use lines::{LineRangeSet, parse_line_ranges};
pub use lsp::{LspAnalyzer, LspServerConfig};
pub use twoslash::{
    CompletionCallout, CutKind, LineMarker, LineTag, ParsedCode, QueryCallout, clean_copy_text,
    parse_code_annotations,
};

#[cfg(feature = "gpui")]
pub use view::{CodeBlocks, install};

/// Resolved metadata and parsed annotations for a `code_block` in a [`Document`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodeBlockMeta {
    /// Raw language attribute, if set.
    pub language: Option<String>,
    /// Human-readable display label for the language.
    pub label: String,
    /// Optional title or file path on the fence.
    pub title: Option<String>,
    /// Whether Twoslash directives are enabled on the block.
    pub twoslash: bool,
    /// Parsed highlighted line ranges from the fence.
    pub highlight_lines: LineRangeSet,
    /// Extracted line tags, Twoslash directives, and built-in semantic annotations.
    pub annotations: ParsedCode,
}

impl CodeBlockMeta {
    /// Reads a `code_block`'s attributes and parses its annotations from `doc`.
    pub fn of(doc: &Document, block_id: BlockId) -> Option<Self> {
        if doc.block_type(block_id).ok()? != block::CODE_BLOCK {
            return None;
        }
        let language = doc.code_language(block_id);
        let label = language_label(language.as_deref());
        let title = doc.code_title(block_id);
        let twoslash = doc.is_twoslash(block_id);
        let raw_lines = doc.code_highlights(block_id);
        let highlight_lines = raw_lines
            .as_deref()
            .map(parse_line_ranges)
            .unwrap_or_default();
        let code = doc.plain_text(block_id).ok()?;
        let mut annotations = parse_code_annotations(&code, raw_lines.as_deref(), twoslash);
        if twoslash {
            let req = AnalysisRequest {
                language: language.clone(),
                title: title.clone(),
                code,
                queries: annotations.query_sites(),
                completions: annotations.completion_sites(),
                files: annotations.files.clone(),
                no_errors: annotations.no_errors,
                expected_errors: annotations.expected_errors.clone(),
            };
            let analysis = BuiltInAnalyzer::analyze_sync(&req);
            annotations.merge_analysis(&analysis);
        }
        Some(Self {
            language,
            label,
            title,
            twoslash,
            highlight_lines,
            annotations,
        })
    }
}
