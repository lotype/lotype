use std::collections::HashMap;

use gpui::{
    Context, Div, FocusHandle, Focusable, FontWeight, IntoElement, Render, StyledText, TextLayout,
    TextStyle, Window, div, prelude::*, rems,
};
use lotype_core::{BlockId, Content, Editor, Value};

use crate::marks::{CODE_FONT, text_runs};

/// A GPUI view of an [`Editor`]: the text of its blocks, with their marks.
pub struct EditorView {
    editor: Editor,
    focus: FocusHandle,
    /// Where each block's text was laid out in the last frame.
    layouts: HashMap<BlockId, TextLayout>,
}

impl EditorView {
    pub fn new(editor: Editor, cx: &mut Context<Self>) -> Self {
        Self {
            editor,
            focus: cx.focus_handle(),
            layouts: HashMap::new(),
        }
    }

    pub fn editor(&self) -> &Editor {
        &self.editor
    }

    /// Changes the editor and redraws.
    pub fn update_editor<R>(
        &mut self,
        cx: &mut Context<Self>,
        edit: impl FnOnce(&mut Editor) -> R,
    ) -> R {
        let result = edit(&mut self.editor);
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
        let spans = doc.spans(block).ok()?;
        let text: String = spans.iter().map(|span| span.text.as_str()).collect();
        let text = StyledText::new(text).with_runs(text_runs(&spans, &style));
        self.layouts.insert(block, text.layout().clone());
        Some(line.child(text))
    }
}

impl Focusable for EditorView {
    fn focus_handle(&self, _: &gpui::App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for EditorView {
    fn render(&mut self, window: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        self.layouts.clear();
        let base = window.text_style();
        let blocks = self.editor.document().blocks();
        let texts: Vec<_> = blocks
            .into_iter()
            .filter_map(|block| self.block_text(block, &base))
            .collect();
        div()
            .track_focus(&self.focus)
            .flex()
            .flex_col()
            .gap_1()
            .children(texts)
    }
}
