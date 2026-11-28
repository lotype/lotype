//! Headless LaTeX typesetting backed by `katex-rs` and `katex-native`.

use std::collections::BTreeMap;
use std::fmt;
use std::sync::OnceLock;

use katex_native::katex::macro_expander::MacroMap;
use katex_native::katex::macros::MacroDefinition;
use katex_native::katex::{KatexContext, Settings};
use katex_native::{Layout, layout_with, svg};

/// Configuration for KaTeX typesetting.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct MathOptions {
    /// Custom LaTeX macro definitions (for example `\\R` -> `\\mathbb{R}`).
    pub macros: BTreeMap<String, String>,
    /// Place equation tags (`\\tag{...}`) on the left instead of the right.
    pub leqno: bool,
    /// Flush-left display equations instead of centering them.
    pub fleqn: bool,
}

impl MathOptions {
    /// Default KaTeX options with centered display math and right-aligned tags.
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers a custom LaTeX macro, such as `("\\R", "\\mathbb{R}")`.
    pub fn with_macro(mut self, name: impl Into<String>, expansion: impl Into<String>) -> Self {
        self.macros.insert(name.into(), expansion.into());
        self
    }

    /// Places equation tags on the left edge when `leqno` is `true`.
    pub fn with_leqno(mut self, leqno: bool) -> Self {
        self.leqno = leqno;
        self
    }

    /// Aligns display equations flush-left when `fleqn` is `true`.
    pub fn with_fleqn(mut self, fleqn: bool) -> Self {
        self.fleqn = fleqn;
        self
    }

    fn to_katex_settings(&self, display: bool) -> Settings {
        let mut macros = MacroMap::default();
        for (name, body) in &self.macros {
            macros.insert(name.clone(), MacroDefinition::String(body.clone()));
        }
        Settings::builder()
            .display_mode(display)
            .leqno(self.leqno)
            .fleqn(self.fleqn)
            .macros(macros)
            .build()
    }
}

/// Error returned when a LaTeX expression cannot be parsed or laid out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MathError {
    /// Human-readable diagnostic from KaTeX.
    pub message: String,
}

impl MathError {
    fn from_katex(err: katex_native::Error) -> Self {
        let raw = err.to_string();
        let message = raw
            .strip_prefix("KaTeX parse error: ")
            .unwrap_or(&raw)
            .trim()
            .to_owned();
        Self { message }
    }
}

impl fmt::Display for MathError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for MathError {}

fn context() -> &'static KatexContext {
    static CTX: OnceLock<KatexContext> = OnceLock::new();
    CTX.get_or_init(KatexContext::default)
}

/// Typesets `tex` into a native display list using default [`MathOptions`].
pub fn typeset(tex: &str, display: bool) -> Result<Layout, MathError> {
    typeset_with(tex, display, &MathOptions::default())
}

/// Typesets `tex` into a native display list using custom [`MathOptions`].
pub fn typeset_with(tex: &str, display: bool, options: &MathOptions) -> Result<Layout, MathError> {
    let settings = options.to_katex_settings(display);
    layout_with(context(), tex, &settings).map_err(MathError::from_katex)
}

/// Checks whether `tex` parses and lays out cleanly without returning the display list.
pub fn validate(tex: &str, display: bool) -> Result<(), MathError> {
    typeset(tex, display).map(|_| ())
}

/// Renders `tex` to a standalone SVG string at `px_per_em` pixels per em.
pub fn to_svg(tex: &str, display: bool, px_per_em: f64, color: &str) -> Result<String, MathError> {
    let layout = typeset(tex, display)?;
    Ok(svg::to_svg(&layout, px_per_em, color))
}

/// Renders display math `tex` centered in a `width_em`-wide standalone SVG with right-aligned tags.
pub fn to_svg_display(
    tex: &str,
    px_per_em: f64,
    color: &str,
    width_em: f64,
) -> Result<String, MathError> {
    let layout = typeset(tex, true)?;
    Ok(svg::to_svg_display(&layout, px_per_em, color, width_em))
}
