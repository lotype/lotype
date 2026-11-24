//! A block's text as an element: its spans as text runs, its atoms drawn
//! by their views, and text being composed underlined in place.

use std::ops::Range;

use gpui::{
    AnyElement, App, Div, FontWeight, InteractiveElement, IntoElement, ParentElement, Styled,
    TextStyle, UnderlineStyle, Window, div, px, rems, rgb,
};
use lotype_core::{Atom, BlockId, Content, MAX_HEADING_LEVEL, Selection, Span, Value};

use super::EditorView;

/// What an empty paragraph says while the caret is in it.
const PLACEHOLDER: &str = "Type '/' for blocks, Space for AI, or just write";
use super::input::Shown;
use crate::marks::{CODE_FONT, text_runs};
use crate::text_el::TextElement;

/// The row of a document with no blocks: where writing starts, as the first
/// click, key or text makes it a paragraph.
pub(super) fn empty_document() -> AnyElement {
    div()
        .py(px(2.))
        .text_color(rgb(0x8a8f98))
        .debug_selector(|| "empty-document".to_owned())
        .child(PLACEHOLDER)
        .into_any_element()
}

impl EditorView {
    pub(super) fn block_text(
        &mut self,
        block: BlockId,
        base: &TextStyle,
        shown: Option<&Shown>,
        window: &mut Window,
        cx: &mut App,
    ) -> Option<Div> {
        let doc = self.editor.document();
        let block_type = doc.block_type(block).ok()?;
        let spec = doc.schema().block_spec(&block_type)?;
        if spec.content() != Content::Inline {
            return None;
        }
        // Where the lines go in their room, for a type that takes a style.
        let align = match doc.attr(block, "text_alignment") {
            Ok(Some(Value::Str(align))) if spec.takes_style() => match align.as_str() {
                "center" => 0.5,
                "right" => 1.,
                _ => 0.,
            },
            _ => 0.,
        };
        let mut style = base.clone();
        let mut line = div();
        match block_type.as_str() {
            "heading" => {
                let level = match doc.attr(block, "level") {
                    Ok(Some(Value::Int(level))) => level.clamp(1, MAX_HEADING_LEVEL),
                    _ => 1,
                };
                let sizes = [2.0, 1.5, 1.25, 1.125, 1.0];
                // The text takes its size from the element around it.
                line = line.text_size(rems(sizes[level as usize - 1]));
                style.font_weight = FontWeight::BOLD;
            }
            "code_block" => style.font_family = CODE_FONT.into(),
            _ => {}
        }
        let (spans, composed) = match shown.filter(|shown| shown.block == block) {
            Some(shown) => self.composed_spans(shown)?,
            None => (doc.spans(block).ok()?, None),
        };
        let mut runs = text_runs(&spans, &style);
        // Text being composed is underlined, and is a span of its own.
        let mut at = 0;
        for run in &mut runs {
            if composed.as_ref().is_some_and(|range| range.contains(&at)) {
                let color = Some(run.color);
                run.underline = Some(UnderlineStyle {
                    color,
                    thickness: px(1.),
                    wavy: false,
                });
            }
            at += run.len;
        }
        let mut atoms = Vec::new();
        let mut at = 0;
        for span in &spans {
            if let Some(atom) = &span.atom {
                atoms.push((
                    at..at + span.text.len(),
                    self.atom_element(atom, window, cx),
                ));
            }
            at += span.text.len();
        }
        let text: String = spans.iter().map(|span| span.text.as_str()).collect();
        let element = TextElement::new(text.into(), runs, atoms).align(align);
        self.layouts.insert(block, element.layout().clone());
        self.order.push(block);
        let line = line.relative().child(element);
        Some(match self.placeholder(block, &block_type, window) {
            Some(placeholder) => line.child(placeholder),
            None => line,
        })
    }

    /// What an empty paragraph says while the caret is in it.
    fn placeholder(&self, block: BlockId, block_type: &str, window: &Window) -> Option<Div> {
        let caret = match self.editor.selection() {
            Some(Selection::Text { anchor, head }) => anchor == head && head.block == block,
            _ => false,
        };
        let shown = caret
            && block_type == "paragraph"
            && self
                .editor
                .document()
                .text_len(block)
                .is_ok_and(|len| len == 0)
            && self.composition.is_none()
            && self.focus.is_focused(window);
        let placeholder = div()
            .absolute()
            .top_0()
            .left_0()
            .whitespace_nowrap()
            .text_color(rgb(0x8a8f98))
            .debug_selector(|| "placeholder".to_owned())
            .child(PLACEHOLDER);
        shown.then_some(placeholder)
    }

    /// The spans of a block with text being composed in it, which has the
    /// marks typed text would get, and where that text is.
    fn composed_spans(&self, shown: &Shown) -> Option<(Vec<Span>, Option<Range<usize>>)> {
        let doc = self.editor.document();
        let marked = shown.marked.clone().unwrap_or_default();
        let after = shown.text.len() - marked.end;
        let len = doc.text_len(shown.block).ok()?;
        let mut spans = doc.spans_in(shown.block, 0..marked.start).ok()?;
        let marks = self.editor.active_marks().unwrap_or_default();
        spans.push(Span::new(&shown.text[marked.clone()], marks));
        spans.extend(doc.spans_in(shown.block, len - after..len).ok()?);
        Some((spans, Some(marked)))
    }

    fn atom_element(&self, atom: &Atom, window: &mut Window, cx: &mut App) -> AnyElement {
        if let Some(view) = self.atom_views.get(&atom.name) {
            return view(atom, window, cx);
        }
        let spec = self.editor.document().schema().inline_spec(&atom.name);
        let text = spec.map_or_else(|| atom.name.clone(), |spec| spec.text(atom));
        let chip = div().px_1().rounded_sm().bg(rgb(0xebeced)).child(text);
        chip.into_any_element()
    }
}
