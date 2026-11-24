//! A table: a grid of its cells, each of which holds text as a block does,
//! the first row its headings, as Markdown has them.

use gpui::{Context, Div, FontWeight, TextStyle, Window, div, prelude::*, rgb};
use lotype_core::{BlockId, Selection};

use super::EditorView;
use super::input::Shown;

impl EditorView {
    /// The grid of `table`'s cells, or `None` for a table that is not whole.
    pub(super) fn table_grid(
        &mut self,
        table: BlockId,
        base: &TextStyle,
        shown: Option<&Shown>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Div> {
        let doc = self.editor.document();
        let rows = doc.children(Some(table)).ok()?;
        let columns = doc.columns(table).ok()?;
        let cells = rows
            .iter()
            .map(|row| {
                let cells = columns.iter().map(|column| doc.cell(*row, *column));
                cells.collect::<Result<Vec<_>, _>>()
            })
            .collect::<Result<Vec<_>, _>>()
            .ok()?;
        let line = rgb(0xd0d4d9);
        let mut grid = div().w_full().flex().flex_col().border_t_1().border_l_1();
        for (index, row) in cells.into_iter().enumerate() {
            let mut style = base.clone();
            if index == 0 {
                style.font_weight = FontWeight::SEMIBOLD;
            }
            let mut tr = div().flex().w_full();
            for cell in row {
                let text = self.block_text(cell, &style, shown, window, cx)?;
                let td = div().flex_1().min_w_0().px_2().py_1();
                let td = td.border_r_1().border_b_1().border_color(line);
                tr = tr.child(td.when(index == 0, |td| td.bg(rgb(0xf6f8fa))).child(text));
            }
            grid = grid.child(tr);
        }
        Some(grid.border_color(line))
    }

    /// Tab in a cell goes to the next one, and Shift-Tab to the one before,
    /// in the order they read. Returns whether the caret was in a cell.
    pub(super) fn tab_cell(&mut self, backward: bool) -> bool {
        let Some(Selection::Text { head, .. }) = self.editor.selection() else {
            return false;
        };
        if head.block.column().is_none() {
            return false;
        }
        let at = self.order.iter().position(|block| *block == head.block);
        let next = at.and_then(|at| match backward {
            true => at.checked_sub(1),
            false => Some(at + 1),
        });
        let cell = next.and_then(|next| self.order.get(next).copied());
        if let Some(cell) = cell.filter(|cell| cell.column().is_some()) {
            let end = self.editor.document().text_len(cell).unwrap_or(0);
            let caret = Selection::caret(lotype_core::Pos::new(cell, end));
            self.editor.set_selection(Some(caret)).ok();
        }
        true
    }
}
