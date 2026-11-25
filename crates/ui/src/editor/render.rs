//! A block's text as an element: its spans as text runs, its atoms drawn
//! by their views, and text being composed underlined in place.

use std::ops::Range;

use gpui::{
    AnyElement, App, Div, FontWeight, InteractiveElement, IntoElement, ParentElement, Styled,
    TextStyle, UnderlineStyle, Window, div, px, rems,
};
use gpui_component::ActiveTheme;
use lotype_core::{Align, Atom, BlockId, Content, Role, Selection, Span, block};

use super::EditorView;

/// What an empty paragraph says while the caret is in it.
const PLACEHOLDER: &str = "Type '/' for blocks, or just write";
const AI_PLACEHOLDER: &str = "Type '/' for blocks, Space for AI, or just write";
use super::input::Shown;
use crate::marks::{CODE_FONT, Inks, text_runs};
use crate::text_el::TextElement;

/// The row of a document with no blocks: where writing starts, as the first
/// click, key or text makes it a paragraph.
pub(super) fn empty_document(has_ai: bool, cx: &App) -> AnyElement {
    div()
        .py(px(2.))
        .text_color(cx.theme().muted_foreground)
        .debug_selector(|| "empty-document".to_owned())
        .child(if has_ai { AI_PLACEHOLDER } else { PLACEHOLDER })
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
        let doc = self.document_of(block);
        let block_type = doc.block_type(block).ok()?;
        let spec = doc.schema().block_spec(&block_type)?;
        if spec.content() != Content::Inline {
            return None;
        }
        let review = self.ai_review_spans(block);
        // Where the lines go in their room, for a type that takes a style.
        let align = match doc.alignment(block) {
            Align::Left => 0.,
            Align::Center => 0.5,
            Align::Right => 1.,
        };
        let mut style = base.clone();
        let mut line = div();
        match block_type.as_str() {
            block::HEADING => {
                let level = doc.heading_level(block);
                let sizes = [2.0, 1.5, 1.25, 1.125, 1.0];
                // The text takes its size from the element around it.
                line = line.text_size(rems(sizes[level as usize - 1]));
                style.font_weight = FontWeight::BOLD;
            }
            _ if doc.role(block) == Role::Code => style.font_family = CODE_FONT.into(),
            _ => {}
        }
        let (spans, composed, marks) = match (shown.filter(|shown| shown.block == block), review) {
            (Some(shown), _) => {
                let (spans, composed) = self.composed_spans(shown)?;
                (spans, composed, None)
            }
            (None, Some((spans, marks))) => (spans, None, Some(marks)),
            (None, None) => (doc.spans(block).ok()?, None, None),
        };
        let inks = Inks {
            link: cx.theme().link,
            code: cx.theme().muted,
            dark: cx.theme().mode.is_dark(),
        };
        let mut runs = text_runs(&spans, &style, inks);
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
        if let Some(marks) = &marks {
            super::ai_review::mark_runs(&mut runs, marks, cx);
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
        if !self.measuring {
            self.layouts.insert(block, element.layout().clone());
            if marks.is_some() {
                self.ai.diffed.insert(block);
            }
        }
        let line = line.relative().child(element);
        Some(match self.placeholder(block, &block_type, window, cx) {
            Some(placeholder) => line.child(placeholder),
            None => line,
        })
    }

    /// What an empty paragraph says while the caret is in it.
    fn placeholder(
        &self,
        block: BlockId,
        block_type: &str,
        window: &Window,
        cx: &App,
    ) -> Option<Div> {
        let caret = match self.editor.selection() {
            Some(Selection::Text { anchor, head }) => anchor == head && head.block == block,
            _ => false,
        };
        let text = self.editor.document().schema().text_block();
        let shown = caret
            && text == Some(block_type)
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
            .text_color(cx.theme().muted_foreground)
            .debug_selector(|| "placeholder".to_owned())
            .child(if self.has_ai() {
                AI_PLACEHOLDER
            } else {
                PLACEHOLDER
            });
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
        let chip = div()
            .px_1()
            .rounded_sm()
            .bg(cx.theme().secondary)
            .child(text);
        chip.into_any_element()
    }
}
