//! The formatting toolbar: over a text selection, a button for each mark
//! the schema has, showing whether all of the selection has it. A button
//! never takes the focus, so the selection stays as it is.

use gpui::{AnyElement, ClickEvent, Context, MouseButton, Window, div, prelude::*};
use gpui_component::button::{Button, ButtonVariants};
use gpui_component::{Selectable, Sizable, ThemeStyled, h_flex};
use lotype_core::Selection;

use super::EditorView;
use super::popup::popup;

/// The marks on the toolbar, with their labels.
const MARKS: [(&str, &str); 5] = [
    ("bold", "B"),
    ("italic", "I"),
    ("underline", "U"),
    ("strike", "S"),
    ("code", "</>"),
];

impl EditorView {
    /// The selected text, when it is some and not being dragged out.
    fn selected_text(&self) -> Option<(lotype_core::Pos, lotype_core::Pos)> {
        let Some(Selection::Text { anchor, head }) = self.editor.selection() else {
            return None;
        };
        // Marks go across no edge of a cell.
        let cell = anchor.block.column().or(head.block.column()).is_some();
        let shown = anchor != head && !cell && !self.dragging && self.composition.is_none();
        shown.then(|| self.editor.document().ordered(*anchor, *head).ok())?
    }

    /// Notes where the selection starts, at the start of a frame.
    pub(super) fn place_toolbar(&mut self) {
        let at = self
            .selected_text()
            .and_then(|(from, _)| self.caret_bounds(from));
        self.toolbar.update(at);
    }

    /// The toolbar, when it shows.
    pub(super) fn toolbar(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if !self.focus.is_focused(window) || self.selected_text().is_none() {
            return None;
        }
        let at = self.toolbar.get(window)?;
        let active = self.editor.active_marks().unwrap_or_default();
        let schema = self.editor.document().schema();
        let marks = MARKS
            .into_iter()
            .filter(|(name, _)| schema.mark_spec(name).is_some());
        let buttons = marks.map(|(name, label)| {
            let click = cx.listener(move |view: &mut Self, _: &ClickEvent, window, cx| {
                view.editor.toggle_mark(name).ok();
                view.selection_changed(window, cx);
            });
            let button = Button::new(name)
                .ghost()
                .small()
                .tab_stop(false)
                .label(label)
                .selected(active.contains_key(name))
                .toggled(active.contains_key(name))
                .on_click(click);
            div()
                .debug_selector(move || format!("toolbar-{name}"))
                .child(button)
        });
        let bar = h_flex()
            .popover_style(cx)
            .occlude()
            .p_1()
            .gap_1()
            // A click on the toolbar is no click in the text.
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .children(buttons);
        Some(popup(at, true, bar))
    }
}
