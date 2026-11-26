mod ai;
mod ai_bar;
mod ai_controls;
mod ai_prompt;
mod ai_review;
mod alert;
mod blink;
mod block_menu;
mod canvas;
mod clipboard;
mod collab;
mod input;
mod keys;
mod link;
mod media;
mod motion;
mod paint;
mod popup;
mod render;
mod selection;
mod side_menu;
mod slash;
mod table;
mod toolbar;
mod toolbar_menus;
mod visible;

pub(crate) use ai_controls::{Act, send, status, tier};
pub use alert::{AlertLook, Shade};
pub use media::MediaSource;

use std::cell::Cell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use gpui::{
    AnyElement, App, Bounds, Context, CursorStyle, DispatchPhase, ElementInputHandler, Entity,
    FocusHandle, Focusable, IntoElement, MouseButton, MouseExitEvent, MouseMoveEvent, MouseUpEvent,
    Pixels, Render, ScrollHandle, Subscription, Window, canvas, div, prelude::*,
};
use lotype_ai::EditLocation;
use lotype_core::{
    Atom, BlockId, Document, Editor, KeyBinding, Pos, SlashItem, default_keymap,
    default_slash_items,
};

use crate::text_el::{Affinity, TextLayout};
use input::Composition;

/// Draws an atom of one type.
type AtomView = Rc<dyn Fn(&Atom, &mut Window, &mut App) -> AnyElement>;
type BlockView = Rc<dyn Fn(BlockContent, &mut Window, &mut App) -> AnyElement>;

/// What a view of a block of its own type draws from.
pub struct BlockContent<'a> {
    /// The block to draw.
    pub block: BlockId,
    /// The document, to read the block's attributes from.
    pub document: &'a Document,
    /// The block's text, which the editor lays out and edits, for a type
    /// that holds text: the view puts it where the text goes.
    pub text: Option<AnyElement>,
}

/// A GPUI view of an [`Editor`]: the text of its blocks, with their marks.
pub struct EditorView {
    editor: Editor,
    focus: FocusHandle,
    /// Where each block's text was laid out in the last frame.
    layouts: HashMap<BlockId, TextLayout>,
    /// The blocks laid out, in document order.
    order: Rc<Vec<BlockId>>,
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
    block_views: HashMap<String, BlockView>,
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
    media: media::Media,
    side_menu: side_menu::SideMenu,
    /// The alert whose menu of kinds is open.
    alert_menu: Option<BlockId>,
    /// How the application draws alerts of a kind, instead of as GitHub does.
    alert_looks: HashMap<lotype_core::AlertKind, alert::AlertLook>,
    /// The table the mouse is over, and the cell whose menu is open.
    table_hover: Option<BlockId>,
    table_menu: Option<BlockId>,
    /// Where a drag with the mouse started.
    drag_anchor: Option<Pos>,
    blink: blink::Blink,
    /// How tall the rows were when last laid out.
    heights: visible::Heights,
    /// The rows and the reading order, kept while nothing they come of changes.
    shape: visible::Shape,
    /// Where each row is among the scroll's children, for one laid out.
    row_items: Vec<Option<usize>>,
    /// Rows are being laid out apart, to measure them: their text is not
    /// one the keys or the mouse find.
    measuring: bool,
    /// Redraws when the focus leaves, which closes the menus, and blinks
    /// the caret again when the focus or the window comes back.
    watches: Vec<Subscription>,
    /// The other peers in the document, to draw their carets and selections.
    presence: Option<lotype_core::Presence>,
    presence_publisher: Option<collab::PresencePublisher>,
    ai: ai::Ai,
}

impl EditorView {
    /// A view of `editor`, with the built-in keymap and slash menu entries.
    pub fn new(editor: Editor, cx: &mut Context<Self>) -> Self {
        // The menus are drawn with gpui-component, in its theme.
        if !cx.has_global::<gpui_component::Theme>() {
            gpui_component::init(cx);
        }
        Self {
            editor,
            focus: cx.focus_handle(),
            layouts: HashMap::new(),
            order: Rc::default(),
            composition: None,
            keymap: default_keymap(),
            goal: None,
            dragging: false,
            side: None,
            atom_views: HashMap::new(),
            block_views: HashMap::new(),
            closed: HashSet::new(),
            scroll: ScrollHandle::new(),
            reveal: Rc::default(),
            rows: Vec::new(),
            block_range: None,
            slash_items: default_slash_items(),
            slash: slash::SlashMenu::default(),
            toolbar: toolbar::Toolbar::default(),
            links: link::Links::default(),
            media: media::Media::default(),
            side_menu: side_menu::SideMenu::default(),
            alert_menu: None,
            alert_looks: HashMap::new(),
            table_hover: None,
            table_menu: None,
            drag_anchor: None,
            blink: blink::Blink::default(),
            heights: visible::Heights::default(),
            shape: visible::Shape::default(),
            row_items: Vec::new(),
            measuring: false,
            watches: Vec::new(),
            presence: None,
            presence_publisher: None,
            ai: ai::Ai::default(),
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

    /// The document's editor. While an AI session is open the view shows
    /// the session's copy instead, which is no part of the document.
    pub fn editor(&self) -> &Editor {
        self.ai.main.as_ref().unwrap_or(&self.editor)
    }

    /// Changes the editor and redraws, as an import from a peer does. Text
    /// being composed stays in place of the selection, which the editor
    /// keeps in place, unless the selection is no longer text.
    pub fn update_editor<R>(
        &mut self,
        cx: &mut Context<Self>,
        edit: impl FnOnce(&mut Editor) -> R,
    ) -> R {
        let result = edit(self.ai.main.as_mut().unwrap_or(&mut self.editor));
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

    /// Draws blocks of `block_type` with `view`, which gets the block's
    /// text, laid out and edited by the editor, to put where it goes. It
    /// draws a type of the application's own, or one of the built-in ones
    /// otherwise; the editor still draws the block's colors, indent,
    /// markers, side menu and selection around it.
    pub fn with_block_view(
        mut self,
        block_type: &str,
        view: impl Fn(BlockContent, &mut Window, &mut App) -> AnyElement + 'static,
    ) -> Self {
        self.block_views
            .insert(block_type.to_owned(), Rc::new(view));
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
        if self.watches.is_empty() {
            let blur = cx.on_blur(&self.focus, window, |_, _, cx| cx.notify());
            // The blinking ends with the focus, and nothing else starts it.
            let wake = |view: &mut Self, window: &mut Window, cx: &mut Context<Self>| {
                view.restart_blink(window, cx);
                cx.notify();
            };
            let focus = cx.on_focus(&self.focus, window, wake);
            let active = cx.observe_window_activation(window, wake);
            self.watches = vec![blur, focus, active];
        }
        if std::mem::take(&mut self.ai.wanted) {
            self.open_inline_ai_prompt(window, cx);
        }
        // The frame's cost, up to the caret's paint, decides the blinking.
        self.blink.started.set(Some(std::time::Instant::now()));
        self.place_slash_menu();
        self.place_toolbar();
        self.place_side_menu();
        self.place_links();
        self.place_ai_prompt();
        self.ai_refocus_field(window, cx);
        self.layouts.clear();
        self.ai.diffed.clear();
        self.measure_rows();
        let base = window.text_style();
        let shown = self.shown();
        self.open_around_selection();
        let selected = match self.editor.selection() {
            Some(lotype_core::Selection::Blocks(blocks)) => blocks.iter().copied().collect(),
            _ => HashSet::new(),
        };
        // The rows of the frame before stay whole while these are made, for
        // what a row draws by the rows around it, as where a block lands.
        let all = self.shape(selected);
        self.keep_media_form(&all);
        let candidates: Vec<BlockId> = all.iter().map(|row| row.block).collect();
        let ai_cursor = self.ai_cursor_row(&candidates);
        let peers = self
            .presence
            .as_ref()
            .map(|presence| presence.collaborators(self.editor.document()))
            .unwrap_or_default();
        let mut ai_cursor_blocks: HashSet<_> = peers
            .iter()
            .filter_map(|peer| self.visible_cursor_row(peer.ai_block?, &candidates))
            .collect();
        // The live cursor carries the label while the model writes text.
        if let Some(EditLocation::Block(block) | EditLocation::Removal(Some(block))) = ai_cursor
            && self.ai_writing_block() != Some(block)
        {
            ai_cursor_blocks.insert(block);
        }
        // Nothing is left but what the model removed, which is shown.
        if let (Some(EditLocation::Removal(None)), Some(first)) = (ai_cursor, all.first()) {
            ai_cursor_blocks.insert(first.block);
        }
        // Rows far from the view lay out nothing: those next to each other
        // are one space as tall as they are.
        let plan = self.plan_rows(&candidates, window);
        let (mut rows, mut blocks, mut items) = (Vec::new(), Vec::new(), Vec::new());
        let mut space = Pixels::ZERO;
        let spacer = |height| div().h(height).w_full().flex_none().into_any_element();
        for (row, plan) in all.iter().zip(plan) {
            match plan {
                visible::Plan::Lay => {}
                visible::Plan::Space(height) => {
                    space += height;
                    blocks.push(row.block);
                    items.push(None);
                    continue;
                }
                visible::Plan::Measure(height) => {
                    // Out of the column, unseen, and with no text the keys
                    // or the mouse would find.
                    space += height;
                    blocks.push(row.block);
                    items.push(None);
                    self.measuring = true;
                    let element = self.row_element(row, false, &base, shown.as_ref(), window, cx);
                    self.measuring = false;
                    match element {
                        Some(element) => {
                            // As wide as the column, which starts after the gutter.
                            let gutter = gpui::px(side_menu::GUTTER);
                            let apart = div().absolute().top_0().left(gutter).right_0().invisible();
                            self.measuring(row.block, rows.len());
                            rows.push(apart.child(element).into_any_element());
                        }
                        None => self.undrawn(row.block),
                    }
                    continue;
                }
            }
            let marked = ai_cursor_blocks.contains(&row.block);
            let Some(element) = self.row_element(row, marked, &base, shown.as_ref(), window, cx)
            else {
                self.undrawn(row.block);
                continue;
            };
            if space > Pixels::ZERO {
                rows.push(spacer(std::mem::replace(&mut space, Pixels::ZERO)));
            }
            blocks.push(row.block);
            items.push(Some(rows.len()));
            rows.push(element);
        }
        if space > Pixels::ZERO {
            rows.push(spacer(space));
        }
        self.rows = blocks;
        self.row_items = items;
        let blank = self.editor.document().children(None);
        if blank.is_ok_and(|blocks| blocks.is_empty()) && all.is_empty() {
            let empty = render::empty_document(self.has_ai(), cx);
            rows.push(if ai_cursor == Some(EditLocation::Removal(None)) {
                div()
                    .relative()
                    .child(empty)
                    .child(ai_bar::cursor(cx))
                    .into_any_element()
            } else {
                empty
            });
        }
        // A block's menu goes with it, deleted or hidden.
        if self
            .side_menu
            .menu
            .is_some_and(|block| !self.rows.contains(&block))
        {
            self.side_menu.menu = None;
        }
        if self
            .alert_menu
            .is_some_and(|block| !self.rows.contains(&block))
        {
            self.alert_menu = None;
        }
        if self.table_menu.is_some() && self.table_menu != self.editor.current_cell() {
            self.table_menu = None;
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
            // The peers under the caret, which stays in sight where one is.
            .child(collab::cursors(cx.entity(), peers))
            .child(ai_bar::marks(cx.entity()))
            .child(paint::caret(cx.entity()))
            // A session's copy is the model's to write in, and no one else's.
            .children(self.ai.session.is_some().then(|| {
                let cover = div().absolute().top_0().left_0().size_full();
                cover.block_mouse_except_scroll()
            }))
            .children(self.ai_bar(window, cx))
            .when(self.ai.session.is_none(), |editor| {
                editor
                    .children(self.slash_menu(window, cx))
                    .children(self.toolbar(window, cx))
                    .children(self.link_field(window, cx))
                    .children(self.link_toolbar(window, cx))
            })
            .child(window_handlers(cx.entity(), self.focus.clone()))
    }
}
