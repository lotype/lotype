//! LaTeX block and inline equations for lotype documents, typeset natively
//! with KaTeX (`katex-rs` + `katex-native`) and an optional GPUI vector view.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

mod engine;

#[cfg(feature = "gpui")]
mod card;
#[cfg(feature = "gpui")]
mod fonts;
#[cfg(feature = "gpui")]
mod popover;
#[cfg(feature = "gpui")]
mod render;
#[cfg(feature = "gpui")]
mod view;

use lotype_core::{
    Atom, BlockId, BlockSpec, Content, Document, InlineSpec, Pos, Role, Schema, SlashItem, Value,
};

pub use engine::{MathError, MathOptions, to_svg, to_svg_display, typeset, typeset_with, validate};
pub use katex_native::{Color, Family, Font, Item, Layout, PathCommand, Rect};

#[cfg(feature = "gpui")]
pub use view::{MathBlocks, install};

/// Block type name for display LaTeX equations.
pub const BLOCK_TYPE: &str = "math";

/// Inline atom type name for inline LaTeX equations.
pub const INLINE_TYPE: &str = "inline_math";

/// Atom attribute key storing the LaTeX source of an inline equation.
pub const TEX_ATTR: &str = "tex";

/// [`BlockSpec`] for display equations: stores LaTeX source in `LoroText` with
/// [`Role::Code`], no block styling, `$$...$$` Markdown support, and ` ```math `
/// fence fallback.
pub fn block_spec() -> BlockSpec {
    BlockSpec::new(BLOCK_TYPE, Content::Inline)
        .with_role(Role::Code)
        .without_style()
        .written_as_fence(BLOCK_TYPE)
        .as_display_math()
        .with_hint("display LaTeX equations (`$$...$$` or ` ```math `)")
}

/// [`InlineSpec`] for inline equations: stores LaTeX in [`TEX_ATTR`], formats
/// as `$...$` in plain text and Markdown, and enables the `$...$` input rule.
pub fn inline_spec() -> InlineSpec {
    InlineSpec::new(INLINE_TYPE, inline_plain_text).as_inline_math(TEX_ATTR)
}

/// Registers both [`block_spec`] and [`inline_spec`] on `schema`.
pub fn extend_schema(schema: Schema) -> Schema {
    schema.block(block_spec()).inline(inline_spec())
}

/// Slash menu entry that inserts a display equation block (`/math`, `/equation`).
pub fn slash_item() -> SlashItem {
    SlashItem {
        key: BLOCK_TYPE.to_owned(),
        title: "Equation".to_owned(),
        subtext: "Display LaTeX equation block".to_owned(),
        group: "Advanced".to_owned(),
        aliases: vec![
            "math".to_owned(),
            "latex".to_owned(),
            "katex".to_owned(),
            "formula".to_owned(),
            "tex".to_owned(),
        ],
        block_type: BLOCK_TYPE.to_owned(),
        attrs: Vec::new(),
    }
}

/// Builds an inline equation [`Atom`] holding `tex`.
pub fn inline_atom(tex: impl Into<String>) -> Atom {
    Atom::new(INLINE_TYPE).attr(TEX_ATTR, tex.into())
}

/// Reads the LaTeX source from an inline equation [`Atom`], or `""` if absent.
pub fn inline_tex(atom: &Atom) -> &str {
    match atom.attrs.get(TEX_ATTR) {
        Some(Value::Str(tex)) => tex.as_str(),
        _ => "",
    }
}

/// Reads the LaTeX source of a `math` block from `doc`, or `None` for other blocks.
pub fn block_tex(doc: &Document, block: BlockId) -> Option<String> {
    if doc.block_type(block).ok().as_deref() != Some(BLOCK_TYPE) {
        return None;
    }
    doc.plain_text(block).ok()
}

/// Returns `true` when the ordered text selection `(from, to)` targets a block
/// equation (`math`) or consists solely of inline equation atoms (`inline_math`).
pub fn is_equation_selection(doc: &Document, from: Pos, to: Pos) -> bool {
    let is_math_block = |id: BlockId| doc.block_type(id).ok().as_deref() == Some(BLOCK_TYPE);
    if is_math_block(from.block) || is_math_block(to.block) {
        return true;
    }
    if from.block != to.block || from.offset >= to.offset {
        return false;
    }
    let Ok(spans) = doc.spans_in(from.block, from.offset..to.offset) else {
        return false;
    };
    let mut has_inline_math = false;
    for span in &spans {
        match &span.atom {
            Some(atom) if atom.name == INLINE_TYPE => has_inline_math = true,
            Some(_) => return false,
            None if span.text.trim().is_empty() => {}
            None => return false,
        }
    }
    has_inline_math
}

/// Single policy helper deciding whether the text-selection formatting toolbar
/// may appear for `(from, to)`. Returns `false` for block and inline equations.
pub fn allows_selection_toolbar(doc: &Document, from: Pos, to: Pos) -> bool {
    !is_equation_selection(doc, from, to)
}

fn inline_plain_text(atom: &Atom) -> String {
    let tex = inline_tex(atom);
    if tex.is_empty() {
        String::new()
    } else {
        format!("${tex}$")
    }
}
