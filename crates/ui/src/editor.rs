mod block_menu;
mod canvas;
mod clipboard;
mod input;
mod keys;
mod link;
mod motion;
mod paint;
mod popup;
mod render;
mod selection;
mod side_menu;
mod slash;
mod toolbar;
mod toolbar_menus;

use std::cell::Cell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use gpui::{
    AnyElement, App, Bounds, Context, CursorStyle, DispatchPhase, ElementInputHandler, Entity,
    FocusHandle, Focusable, IntoElement, MouseButton, MouseExitEvent, MouseMoveEvent, MouseUpEvent,
    Pixels, Render, ScrollHandle, Subscription, Window, canvas, div, prelude::*,
};
use lotype_core::{
    Atom, BlockId, Editor, KeyBinding, Pos, SlashItem, default_keymap, default_slash_items,
};

use crate::text_el::{Affinity, TextLayout};
use input::Composition;

/// Draws an atom of one type.
type AtomView = Rc<dyn Fn(&Atom, &mut Window, &mut App) -> AnyElement>;

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
    /// The side of a soft wrap the caret is on, while it stays where it was put.
    side: Option<(Pos, Affinity)>,
    atom_views: HashMap<String, AtomView>,
    /// Toggle list items whose children are hidden.
    closed: HashSet<BlockId>,
    scroll: ScrollHandle,
    /// The caret is to be scrolled into sight once it is laid out.
    reveal: Rc<Cell<bool>>,
    /// The blocks shown, in order, as rows.
    rows: Vec<BlockId>,
    /// Where a selection of whole blocks started and where it goes on.
    block_range: Option<(BlockId, BlockId)>,
    slash_items: Vec<SlashItem>,
    slash: slash::SlashMenu,
    toolbar: toolbar::Toolbar,
    links: link::Links,
    side_menu: side_menu::SideMenu,
    /// Redraws when the focus leaves, which closes the menus.
    blur: Option<Subscription>,
}

impl EditorView {
    pub fn new(editor: Editor, cx: &mut Context<Self>) -> Self {
        // The menus are drawn with gpui-component, in its theme.
        if !cx.has_global::<gpui_component::Theme>() {
            gpui_component::init(cx);
        }
        Self {
            editor,
            focus: cx.focus_handle(),
            layouts: HashMap::new(),
            order: Vec::new(),
            composition: None,
            keymap: default_keymap(),
            goal: None,
            dragging: false,
            side: None,
            atom_views: HashMap::new(),
            closed: HashSet::new(),
            scroll: ScrollHandle::new(),
            reveal: Rc::default(),
            rows: Vec::new(),
            block_range: None,
            slash_items: default_slash_items(),
            slash: slash::SlashMenu::default(),
            toolbar: toolbar::Toolbar::default(),
            links: link::Links::default(),
            side_menu: side_menu::SideMenu::default(),
            blur: None,
        }
    }

    /// Replaces the key bindings, which are the core's [`default_keymap`]
    /// to begin with.
    pub fn with_keymap(mut self, keymap: Vec<KeyBinding>) -> Self {
        self.keymap = keymap;
        self
    }

    /// Replaces the entries of the slash menu, which are the core's
    /// [`default_slash_items`] to begin with.
    pub fn with_slash_items(mut self, items: Vec<SlashItem>) -> Self {
        self.slash_items = items;
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

    /// Draws atoms of type `name` with `view`, instead of as a chip with
    /// the plain text their schema gives them.
    pub fn with_atom_view(
        mut self,
        name: &str,
        view: impl Fn(&Atom, &mut Window, &mut App) -> AnyElement + 'static,
    ) -> Self {
        self.atom_views.insert(name.to_owned(), Rc::new(view));
        self
    }

    /// Where a caret at `pos` was drawn in the last frame, as tall as its text.
    pub fn caret_bounds(&self, pos: Pos) -> Option<Bounds<Pixels>> {
        let affinity = self.affinity(pos);
        self.layouts.get(&pos.block)?.caret(pos.offset, affinity)
    }

    /// Where a block's text was in the last frame.
    pub fn text_bounds(&self, block: BlockId) -> Option<Bounds<Pixels>> {
        self.layouts.get(&block)?.bounds()
    }

    /// The side of a soft wrap the caret at `head` is on.
    pub(crate) fn affinity(&self, head: Pos) -> Affinity {
        let side = self.side.filter(|(at, _)| *at == head);
        side.map_or_else(Affinity::default, |(_, affinity)| affinity)
    }
}

/// Gives the system's text input to the view while it has focus, and the
/// mouse while it drags a selection, wherever in the window the mouse goes.
fn window_handlers(view: Entity<EditorView>, focus: FocusHandle) -> impl IntoElement {
    let paint = move |bounds: Bounds<Pixels>, _: (), window: &mut Window, cx: &mut App| {
        let moved = view.clone();
        window.on_mouse_event(move |event: &MouseMoveEvent, phase, window, cx| {
            if phase == DispatchPhase::Bubble {
                moved.update(cx, |view, cx| view.mouse_move(event, window, cx));
            }
        });
        let left = view.clone();
        window.on_mouse_event(move |_: &MouseExitEvent, phase, _, cx| {
            if phase == DispatchPhase::Bubble {
                left.update(cx, |view, cx| view.side_menu_leave(cx));
            }
        });
        let released = view.clone();
        window.on_mouse_event(move |event: &MouseUpEvent, phase, window, cx| {
            if phase == DispatchPhase::Bubble {
                released.update(cx, |view, cx| view.mouse_up(event, window, cx));
            }
        });
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
        if self.blur.is_none() {
            let blur = cx.on_blur(&self.focus, window, |_, _, cx| cx.notify());
            self.blur = Some(blur);
        }
        self.place_slash_menu();
        self.place_toolbar();
        self.place_side_menu();
        self.place_links();
        self.layouts.clear();
        self.order.clear();
        let base = window.text_style();
        let shown = self.shown();
        self.open_around_selection();
        let selected = match self.editor.selection() {
            Some(lotype_core::Selection::Blocks(blocks)) => blocks.iter().copied().collect(),
            _ => HashSet::new(),
        };
        self.rows.clear();
        let mut rows = Vec::new();
        for row in canvas::rows(self.editor.document(), &self.closed, &selected) {
            if let Some(element) = self.row_element(&row, &base, shown.as_ref(), window, cx) {
                self.rows.push(row.block);
                rows.push(element);
            }
        }
        if self
            .editor
            .document()
            .children(None)
            .is_ok_and(|blocks| blocks.is_empty())
        {
            rows.push(render::empty_document());
        }
        // A block menu goes with its block, deleted or hidden.
        if self
            .side_menu
            .menu
            .is_some_and(|block| !self.rows.contains(&block))
        {
            self.side_menu.menu = None;
        }
        div()
            .id("lotype-editor")
            .track_scroll(&self.scroll)
            .overflow_y_scroll()
            .size_full()
            // Room for the side menu of the top-level blocks.
            .pl(gpui::px(side_menu::GUTTER))
            .track_focus(&self.focus)
            .key_context("LotypeEditor")
            .on_key_down(cx.listener(Self::key_down))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::mouse_down))
            .cursor(CursorStyle::IBeam)
            .relative()
            .flex()
            .flex_col()
            .children(rows)
            // Over the rows, whose colors would hide it, and see-through.
            .child(paint::selection(cx.entity()))
            .child(paint::caret(cx.entity()))
            .children(self.slash_menu(window, cx))
            .children(self.toolbar(window, cx))
            .children(self.link_field(window, cx))
            .children(self.link_toolbar(window, cx))
            .child(window_handlers(cx.entity(), self.focus.clone()))
    }
}
