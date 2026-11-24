//! A table: a grid of its cells, each of which holds text as a block does,
//! the first row its headings, as Markdown has them. While the mouse is over
//! it, a bar below adds a row and one on the right a column; the cell with
//! the caret has a menu for the rows and columns around it.

use std::collections::HashSet;

use gpui::{
    Anchor, AnyElement, ClickEvent, Context, Div, FontWeight, MouseButton, MouseDownEvent,
    SharedString, TextStyle, Window, anchored, deferred, div, point, prelude::*, px, rgb,
};
use gpui_component::list::ListItem;
use gpui_component::{ThemeStyled, v_flex};
use lotype_core::{BlockId, Editor, Pos, Selection};

use super::EditorView;
use super::input::Shown;
use super::paint::selection_color;
use crate::icons::{self, icon};

/// What the menu of a cell does.
#[derive(Clone, Copy)]
enum Edit {
    RowAbove,
    RowBelow,
    ColumnLeft,
    ColumnRight,
    DeleteRow,
    DeleteColumn,
}

const EDITS: [(Edit, &str, &str); 6] = [
    (Edit::RowAbove, "row-above", "Insert row above"),
    (Edit::RowBelow, "row-below", "Insert row below"),
    (Edit::ColumnLeft, "column-left", "Insert column left"),
    (Edit::ColumnRight, "column-right", "Insert column right"),
    (Edit::DeleteRow, "delete-row", "Delete row"),
    (Edit::DeleteColumn, "delete-column", "Delete column"),
];

/// The table `cell` is in, and the indexes of its row and its column.
fn place(editor: &Editor, cell: BlockId) -> Option<(BlockId, usize, usize)> {
    let doc = editor.document();
    let row = doc.parent(cell).ok()??;
    let table = doc.parent(row).ok()??;
    let column = doc.columns(table).ok()?;
    let column = column.iter().position(|id| Some(*id) == cell.column())?;
    Some((table, doc.index_of(row).ok()?, column))
}

/// A bar that adds a row or a column, faint until the mouse is on it.
fn add_bar(selector: &'static str) -> Div {
    div()
        .flex()
        .items_center()
        .justify_center()
        .rounded_sm()
        .bg(rgb(0xf3f4f6))
        .text_color(rgb(0x8a8f98))
        .hover(|bar| bar.bg(rgb(0xe5e7eb)))
        .cursor_pointer()
        .debug_selector(move || selector.to_owned())
        .child(icon(icons::PLUS).size_3())
}

impl EditorView {
    /// The grid of `table`'s cells, or `None` for a table that is not whole.
    pub(super) fn table_grid(
        &mut self,
        table: BlockId,
        base: &TextStyle,
        shown: Option<&Shown>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
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
        let selected: HashSet<BlockId> = match self.editor.selection() {
            Some(Selection::Cells { anchor, head }) => doc
                .cells_between(*anchor, *head)
                .unwrap_or_default()
                .into_iter()
                .collect(),
            _ => HashSet::new(),
        };
        let current = self.editor.current_cell();
        let (row_count, column_count) = (rows.len(), columns.len());
        let line = rgb(0xd0d4d9);
        let mut grid = div()
            .flex_1()
            .min_w_0()
            .flex()
            .flex_col()
            .border_t_1()
            .border_l_1();
        for (index, row) in cells.into_iter().enumerate() {
            let mut style = base.clone();
            if index == 0 {
                style.font_weight = FontWeight::SEMIBOLD;
            }
            let mut tr = div().flex().w_full();
            for cell in row {
                let text = self.block_text(cell, &style, shown, window, cx)?;
                let td = div().relative().flex_1().min_w_0().px_2().py_1();
                let td = td.border_r_1().border_b_1().border_color(line);
                let td = match () {
                    _ if selected.contains(&cell) => td.bg(selection_color()),
                    _ if index == 0 => td.bg(rgb(0xf6f8fa)),
                    _ => td,
                };
                let menu = (current == Some(cell)).then(|| self.cell_menu(cell, cx));
                tr = tr.child(td.child(text).children(menu));
            }
            grid = grid.child(tr);
        }
        let hovered = self.table_hover == Some(table);
        let add_row = cx.listener(move |view: &mut Self, _: &MouseDownEvent, window, cx| {
            cx.stop_propagation();
            view.press(window, cx);
            view.editor.add_row(table, row_count).ok();
            view.selection_changed(window, cx);
        });
        let add_column = cx.listener(move |view: &mut Self, _: &MouseDownEvent, window, cx| {
            cx.stop_propagation();
            view.press(window, cx);
            view.editor.add_column(table, column_count).ok();
            view.selection_changed(window, cx);
        });
        let hover = cx.listener(move |view: &mut Self, over: &bool, _, cx| {
            let now = over
                .then_some(table)
                .or(view.table_hover.filter(|at| *at != table));
            if now != view.table_hover {
                view.table_hover = now;
                cx.notify();
            }
        });
        // The bars keep their room while hidden, so the grid does not move.
        let bar = |bar: Div| if hovered { bar } else { bar.invisible() };
        let column_bar = bar(add_bar("table-add-column").w(px(16.)).ml_1())
            .on_mouse_down(MouseButton::Left, add_column);
        let row_bar = bar(add_bar("table-add-row").h(px(16.)).mt_1().mr(px(20.)))
            .on_mouse_down(MouseButton::Left, add_row);
        let id = SharedString::from(format!("table-{table}"));
        let table = v_flex()
            .id(id)
            .w_full()
            .on_hover(hover)
            .child(
                div()
                    .flex()
                    .child(grid.border_color(line))
                    .child(column_bar),
            )
            .child(row_bar);
        Some(table.into_any_element())
    }

    /// The button of the menu of the cell with the caret, and the menu
    /// while it is open.
    fn cell_menu(&self, cell: BlockId, cx: &mut Context<Self>) -> AnyElement {
        let open = cx.listener(move |view: &mut Self, _: &MouseDownEvent, window, cx| {
            cx.stop_propagation();
            view.press(window, cx);
            view.table_menu = Some(cell);
            cx.notify();
        });
        let button = div()
            .absolute()
            .top_1()
            .right_1()
            .rounded_sm()
            .bg(rgb(0xffffff))
            .border_1()
            .border_color(rgb(0xd0d4d9))
            .text_color(rgb(0x8a8f98))
            .cursor_pointer()
            .debug_selector(|| "table-menu".to_owned())
            .on_mouse_down(MouseButton::Left, open)
            .child(icon(icons::CHEVRON_DOWN).size_3());
        let menu = (self.table_menu == Some(cell)).then(|| {
            let items = EDITS.iter().enumerate().map(|(at, &(edit, name, title))| {
                let choose = cx.listener(move |view: &mut Self, _: &ClickEvent, window, cx| {
                    view.table_menu = None;
                    view.press(window, cx);
                    view.edit_table(cell, edit);
                    view.selection_changed(window, cx);
                });
                // The deletions apart from the insertions.
                let item = div().when(at == 4, |item| item.mt_1().pt_1().border_t_1());
                item.border_color(rgb(0xe5e7eb))
                    .debug_selector(move || format!("table-menu-{name}"))
                    .child(ListItem::new(name).child(title).on_click(choose))
            });
            let close = cx.listener(|view: &mut Self, _: &MouseDownEvent, _, cx| {
                if view.table_menu.take().is_some() {
                    cx.notify();
                }
            });
            let menu = v_flex()
                .popover_style(cx)
                .occlude()
                .p_1()
                .w(px(180.))
                .text_sm()
                .on_mouse_down_out(close)
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .children(items);
            // Its right edge under the button's, so that it opens over the
            // table rather than off the page.
            let menu = anchored()
                .anchor(Anchor::TopRight)
                .offset(point(px(14.), px(18.)))
                .snap_to_window_with_margin(px(8.))
                .child(menu);
            deferred(menu).with_priority(1)
        });
        button.children(menu).into_any_element()
    }

    fn edit_table(&mut self, cell: BlockId, edit: Edit) {
        let Some((table, row, column)) = place(&self.editor, cell) else {
            return;
        };
        let editor = &mut self.editor;
        let done = match edit {
            Edit::RowAbove => editor.add_row(table, row),
            Edit::RowBelow => editor.add_row(table, row + 1),
            Edit::ColumnLeft => editor.add_column(table, column),
            Edit::ColumnRight => editor.add_column(table, column + 1),
            Edit::DeleteRow => editor.delete_row(cell),
            Edit::DeleteColumn => editor.delete_column(cell),
        };
        done.ok();
    }

    /// Shift and an arrow with cells selected moves the cell the selection
    /// ends in one cell that way, within the table. Returns whether it did.
    pub(super) fn extend_cells(&mut self, name: &str) -> bool {
        let (down, right): (isize, isize) = match name {
            "Shift-ArrowUp" => (-1, 0),
            "Shift-ArrowDown" => (1, 0),
            "Shift-ArrowLeft" => (0, -1),
            "Shift-ArrowRight" => (0, 1),
            _ => return false,
        };
        let Some(Selection::Cells { anchor, head }) = self.editor.selection().cloned() else {
            return false;
        };
        let Some((table, row, column)) = place(&self.editor, head) else {
            return false;
        };
        let doc = self.editor.document();
        let (rows, columns) = match (doc.children(Some(table)), doc.columns(table)) {
            (Ok(rows), Ok(columns)) => (rows, columns),
            _ => return false,
        };
        let step = |at: usize, by: isize, len: usize| at.saturating_add_signed(by).min(len - 1);
        let row = rows[step(row, down, rows.len())];
        let column = columns[step(column, right, columns.len())];
        if let Ok(head) = doc.cell(row, column) {
            let cells = Selection::Cells { anchor, head };
            self.editor.set_selection(Some(cells)).ok();
        }
        true
    }

    /// Tab in a cell goes to the next one, and from the last a new row
    /// begins; Shift-Tab goes to the one before. Returns whether the caret
    /// was in a cell.
    pub(super) fn tab_cell(&mut self, backward: bool) -> bool {
        let Some(Selection::Text { head, .. }) = self.editor.selection() else {
            return false;
        };
        let Some((table, ..)) = place(&self.editor, head.block) else {
            return false;
        };
        let at = self.order.iter().position(|block| *block == head.block);
        let next = at.and_then(|at| match backward {
            true => at.checked_sub(1),
            false => Some(at + 1),
        });
        let next = next.and_then(|next| self.order.get(next).copied());
        let same = next.filter(|next| place(&self.editor, *next).is_some_and(|at| at.0 == table));
        match same {
            Some(cell) => {
                let end = self.editor.document().text_len(cell).unwrap_or(0);
                self.editor
                    .set_selection(Some(Selection::caret(Pos::new(cell, end))))
                    .ok();
            }
            None if !backward => {
                let rows = self
                    .editor
                    .document()
                    .children(Some(table))
                    .map(|rows| rows.len());
                self.editor.add_row(table, rows.unwrap_or(0)).ok();
            }
            None => {}
        }
        true
    }
}
