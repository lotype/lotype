mod input;
mod keys;
mod motion;
mod paint;

use std::collections::HashMap;

use gpui::{
    App, Bounds, Context, CursorStyle, Div, ElementInputHandler, Entity, FocusHandle, Focusable,
    FontWeight, IntoElement, MouseButton, Pixels, Render, StyledText, TextLayout, TextRun,
    TextStyle, UnderlineStyle, Window, canvas, div, prelude::*, px, rems,
};
use lotype_core::{BlockId, Content, Editor, KeyBinding, Pos, Span, Value, default_keymap};

use crate::marks::{CODE_FONT, text_runs};
use input::Composition;

/// A GPUI view of an [`Editor`]: the text of its blocks, with their marks.
pub struct EditorView {
    editor: Editor,
    focus: FocusHandle,
    /// Where each block's text was laid out in the last frame.
    layouts: HashMap<BlockId, TextLayout>,
    /// The blocks laid out, in document order.
    order: Vec<BlockId>,
    composition: Option<Composition>,
    keymap: Vec<KeyBinding>,
    /// Where moving up or down last took the cursor, and the x position
    /// it keeps to while it goes on doing so.
    goal: Option<(Pos, Pixels)>,
    /// The mouse is selecting.
    dragging: bool,
}

impl EditorView {
    pub fn new(editor: Editor, cx: &mut Context<Self>) -> Self {
        Self {
            editor,
            focus: cx.focus_handle(),
            layouts: HashMap::new(),
            order: Vec::new(),
            composition: None,
            keymap: default_keymap(),
            goal: None,
            dragging: false,
        }
    }

    /// Replaces the key bindings, which are the core's [`default_keymap`]
    /// to begin with.
    pub fn with_keymap(mut self, keymap: Vec<KeyBinding>) -> Self {
        self.keymap = keymap;
        self
    }

    pub fn editor(&self) -> &Editor {
        &self.editor
    }

    /// Changes the editor and redraws, as an import from a peer does. Text
    /// being composed stays in place of the selection, which the editor
    /// keeps in place, unless the selection is no longer text.
    pub fn update_editor<R>(
        &mut self,
        cx: &mut Context<Self>,
        edit: impl FnOnce(&mut Editor) -> R,
    ) -> R {
        let result = edit(&mut self.editor);
        if self.shown().is_none() {
            self.composition = None;
        }
        cx.notify();
        result
    }

    /// The layout of a block's text in the last frame.
    pub fn text_layout(&self, block: BlockId) -> Option<&TextLayout> {
        self.layouts.get(&block)
    }

    fn block_text(&mut self, block: BlockId, base: &TextStyle) -> Option<Div> {
        let doc = self.editor.document();
        let block_type = doc.block_type(block).ok()?;
        let spec = doc.schema().block_spec(&block_type)?;
        if spec.content() != Content::Inline {
            return None;
        }
        let mut style = base.clone();
        let mut line = div();
        match block_type.as_str() {
            "heading" => {
                let level = match doc.attr(block, "level") {
                    Ok(Some(Value::Int(level))) => level.clamp(1, 6),
                    _ => 1,
                };
                let sizes = [2.0, 1.5, 1.25, 1.125, 1.0, 0.875];
                // The text takes its size from the element around it.
                line = line.text_size(rems(sizes[level as usize - 1]));
                style.font_weight = FontWeight::BOLD;
            }
            "code_block" => style.font_family = CODE_FONT.into(),
            _ => {}
        }
        let (text, runs) = match self.shown().filter(|shown| shown.block == block) {
            Some(shown) => (shown.text.clone(), self.composed_runs(&shown, &style)?),
            None => {
                let spans = doc.spans(block).ok()?;
                let text = spans.iter().map(|span| span.text.as_str()).collect();
                (text, text_runs(&spans, &style))
            }
        };
        let text = StyledText::new(text).with_runs(runs);
        self.layouts.insert(block, text.layout().clone());
        self.order.push(block);
        Some(line.child(text))
    }

    /// The runs of a block with text being composed in it, which is
    /// underlined and has the marks typed text would get.
    fn composed_runs(&self, shown: &input::Shown, style: &TextStyle) -> Option<Vec<TextRun>> {
        let doc = self.editor.document();
        let marked = shown.marked.clone().unwrap_or_default();
        let after = shown.text.len() - marked.end;
        let len = doc.text_len(shown.block).ok()?;
        let before = doc.spans_in(shown.block, 0..marked.start).ok()?;
        let after = doc.spans_in(shown.block, len - after..len).ok()?;
        let composed = Span {
            text: shown.text[marked].to_owned(),
            marks: self.editor.active_marks().unwrap_or_default(),
        };
        let mut composed = text_runs(&[composed], style);
        for run in &mut composed {
            let color = Some(run.color);
            run.underline = Some(UnderlineStyle {
                color,
                thickness: px(1.),
                wavy: false,
            });
        }
        let mut runs = text_runs(&before, style);
        runs.extend(composed);
        runs.extend(text_runs(&after, style));
        Some(runs)
    }
}

/// Gives the system's text input to the view while it has focus.
fn input_handler(view: Entity<EditorView>, focus: FocusHandle) -> impl IntoElement {
    let paint = move |bounds: Bounds<Pixels>, _: (), window: &mut Window, cx: &mut App| {
        window.handle_input(&focus, ElementInputHandler::new(bounds, view), cx);
    };
    canvas(|_, _, _| (), paint).absolute().size_full()
}

impl Focusable for EditorView {
    fn focus_handle(&self, _: &gpui::App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for EditorView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.layouts.clear();
        self.order.clear();
        let base = window.text_style();
        let blocks = self.editor.document().blocks();
        let texts: Vec<_> = blocks
            .into_iter()
            .filter_map(|block| self.block_text(block, &base))
            .collect();
        div()
            .track_focus(&self.focus)
            .key_context("LotypeEditor")
            .on_key_down(cx.listener(Self::key_down))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::mouse_down))
            .on_mouse_move(cx.listener(Self::mouse_move))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::mouse_up))
            .cursor(CursorStyle::IBeam)
            .relative()
            .flex()
            .flex_col()
            .gap_1()
            .child(paint::selection(cx.entity()))
            .children(texts)
            .child(paint::caret(cx.entity()))
            .child(input_handler(cx.entity(), self.focus.clone()))
    }
}
