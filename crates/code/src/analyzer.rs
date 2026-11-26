//! Async semantic analysis contract for Twoslash code blocks.

use std::future::Future;
use std::ops::Range;
use std::pin::Pin;

/// Diagnostic severity for inline squiggles and Twoslash banners.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Severity {
    /// Compiler or type error.
    Error,
    /// Compiler or lint warning.
    Warning,
    /// Informational note or `@log` message.
    Info,
    /// Subtle hint or `@annotate` callout.
    Hint,
}

/// Symbol category for an entry in a Twoslash `^|` autocomplete dropdown.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CompletionKind {
    /// Function or method (`fn` / `method`).
    Function,
    /// Struct field or object property (`prop`).
    Property,
    /// Local binding, constant, or parameter (`var`).
    Variable,
    /// Interface, struct, class, enum, or type alias (`type`).
    Type,
    /// Language keyword (`kw`).
    Keyword,
}

/// One candidate shown in a Twoslash `^|` completion list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompletionItem {
    /// Identifier label shown and matched against the typed prefix.
    pub label: String,
    /// Symbol category used for the completion badge.
    pub kind: CompletionKind,
    /// Optional type signature or summary displayed beside the label.
    pub detail: Option<String>,
}

/// A `^?` query location extracted from a Twoslash code block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuerySite {
    /// UTF-8 byte range of the queried token in [`AnalysisRequest::code`].
    pub target_range: Range<usize>,
    /// Zero-based source line of the queried token.
    pub line: usize,
    /// Zero-based character column of the `^` pointer on `line`.
    pub character: usize,
    /// Identifier or token text at `target_range`.
    pub symbol: String,
}

/// A `^|` completion query location extracted from a Twoslash code block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompletionSite {
    /// Zero-based source line of the code above `^|`.
    pub line: usize,
    /// Zero-based character column of the `^` pointer on `line`.
    pub character: usize,
    /// UTF-8 byte offset in [`AnalysisRequest::code`] at `(line, character)`.
    pub offset: usize,
    /// Identifier prefix immediately preceding `offset` (e.g. `"to"` in `count.to`).
    pub prefix: String,
    /// Receiver expression before `.` or `::` (e.g. `"count"` in `count.to`), if any.
    pub receiver: Option<String>,
}

/// A virtual file slice declared with `// @filename: <path>` inside a Twoslash block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VirtualFile {
    /// File path or module name declared by `@filename:`.
    pub name: String,
    /// Zero-based line index of the `@filename:` directive itself.
    pub header_line: usize,
    /// UTF-8 byte range of this virtual file's body in the block's text.
    pub range: Range<usize>,
}

/// Input sent to a [`CodeAnalyzer`] when inspecting a Twoslash block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnalysisRequest {
    /// Canonical or raw language identifier, if set on the block.
    pub language: Option<String>,
    /// Optional title or file name on the block.
    pub title: Option<String>,
    /// Full plain-text contents of the code block.
    pub code: String,
    /// All `^?` queries present in the block.
    pub queries: Vec<QuerySite>,
    /// All `^|` completion queries present in the block.
    pub completions: Vec<CompletionSite>,
    /// Virtual files split by `// @filename:` directives, if any.
    pub files: Vec<VirtualFile>,
    /// Whether `// @noErrors` was specified in the block.
    pub no_errors: bool,
    /// Expected compiler error codes declared via `// @errors: ...`.
    pub expected_errors: Vec<String>,
}

/// Hover signature and documentation attached to a byte range in the code block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HoverAnnotation {
    /// UTF-8 byte range of the symbol in the block's text.
    pub range: Range<usize>,
    /// Symbol name.
    pub symbol: String,
    /// Type signature or value summary shown in the hover card.
    pub signature: String,
    /// Optional documentation text shown below the signature.
    pub docs: Option<String>,
}

/// A diagnostic squiggle and optional inline callout banner.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiagnosticAnnotation {
    /// UTF-8 byte range underlined in the block's text.
    pub target_range: Range<usize>,
    /// Zero-based line after which an inline diagnostic banner is placed.
    pub after_line: usize,
    /// Severity level controlling squiggle and banner colors.
    pub severity: Severity,
    /// Optional error/lint code such as `"TS2322"` or `"E0308"`.
    pub code: Option<String>,
    /// Human-readable diagnostic message.
    pub message: String,
}

/// Semantic results returned by a [`CodeAnalyzer`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CodeAnalysis {
    /// Resolved signatures for `^?` queries, keyed by `target_range`.
    pub queries: Vec<(Range<usize>, String)>,
    /// Resolved completion lists for `^|` queries, keyed by [`CompletionSite::offset`].
    pub completions: Vec<(usize, Vec<CompletionItem>)>,
    /// Hover entries for identifiers in the block.
    pub hovers: Vec<HoverAnnotation>,
    /// Compiler or type-checker diagnostics.
    pub diagnostics: Vec<DiagnosticAnnotation>,
}

/// Boxed future returned by [`CodeAnalyzer::analyze`].
pub type AnalysisFuture = Pin<Box<dyn Future<Output = Result<CodeAnalysis, String>> + Send>>;

/// Pluggable semantic backend (built-in type extractor, `tsserver`, `rust-analyzer`, etc.)
/// for Twoslash code blocks.
pub trait CodeAnalyzer: Send + Sync {
    /// Analyzes a Twoslash code block asynchronously.
    fn analyze(&self, request: AnalysisRequest) -> AnalysisFuture;
}
