//! GPUI views for block equations (`math`) and inline equations (`inline_math`).

use std::collections::HashMap;
use std::sync::Arc;

use gpui::{
    AnyElement, App, AppContext as _, CursorStyle, Entity, FocusHandle, Focusable as _,
    MouseButton, MouseDownEvent, Pixels, SharedString, Subscription, Task, Window, div, prelude::*,
    px,
};
use gpui_component::scroll::ScrollableElement as _;
use gpui_component::{ActiveTheme as _, v_flex};
use katex_native::Layout;
use lotype_core::{BlockId, Pos, Selection};
use lotype_ui::{AtomContent, BlockContent, EditorView, RenderedAtom};

use crate::card::{CODE_FONT, EquationCardProps, render_equation_card};
use crate::engine::{MathOptions, typeset_with};
use crate::fonts;
use crate::popover::{InlineMathForm, open_inline_editor, render_inline_popover};
use crate::render::{display_element, inline_element};
use crate::{BLOCK_TYPE, INLINE_TYPE, allows_selection_toolbar, inline_tex, slash_item};

const MAX_LAYOUT_CACHE: usize = 256;
const INLINE_PAD_Y: f32 = 1.0;

fn elem_id(prefix: &str, block: BlockId) -> SharedString {
    SharedString::from(format!("{prefix}-{block}"))
}

/// Builder that configures and installs block & inline LaTeX equations on an [`EditorView`].
#[derive(Clone, Default)]
pub struct MathBlocks {
    options: MathOptions,
}

impl MathBlocks {
    /// Creates a new configuration with standard KaTeX display and inline typesetting.
    pub fn new() -> Self {
        Self::default()
    }

    /// Replaces the KaTeX [`MathOptions`] used for typesetting.
    pub fn with_options(mut self, options: MathOptions) -> Self {
        self.options = options;
        self
    }

    /// Registers a custom LaTeX macro, such as `("\\R", "\\mathbb{R}")`.
    pub fn with_macro(mut self, name: impl Into<String>, expansion: impl Into<String>) -> Self {
        self.options = self.options.with_macro(name, expansion);
        self
    }

    /// Installs the `math` block view, `inline_math` atom view, KaTeX fonts,
    /// and `/math` slash menu entry onto `editor`.
    pub fn install(self, editor: EditorView, cx: &mut App) -> EditorView {
        fonts::ensure_loaded(cx);
        let state = cx.new(|_| MathState::new(self.options));
        let block_state = state.clone();
        let inline_state = state;

        editor
            .with_extra_slash_items([slash_item()])
            .with_toolbar_filter(allows_selection_toolbar)
            .with_block_view(BLOCK_TYPE, move |content, window, cx| {
                block_state.update(cx, |state, cx| state.render_block(content, window, cx))
            })
            .with_atom_content_view(INLINE_TYPE, move |content, window, cx| {
                inline_state.update(cx, |state, cx| state.render_inline(content, window, cx))
            })
    }
}

/// Convenience installer for [`MathBlocks::new()`].
pub fn install(editor: EditorView, cx: &mut App) -> EditorView {
    MathBlocks::new().install(editor, cx)
}

pub(crate) struct MathState {
    options: MathOptions,
    cache: HashMap<(String, bool), Result<Arc<Layout>, String>>,
    last_valid: HashMap<BlockId, Arc<Layout>>,
    pub(crate) copied: Option<(SharedString, Task<()>)>,
    pub(crate) inline_form: Option<InlineMathForm>,
    editor_focus: Option<FocusHandle>,
    observer: Option<Subscription>,
}

impl MathState {
    fn new(options: MathOptions) -> Self {
        Self {
            options,
            cache: HashMap::new(),
            last_valid: HashMap::new(),
            copied: None,
            inline_form: None,
            editor_focus: None,
            observer: None,
        }
    }

    pub(crate) fn ensure_observer(
        &mut self,
        editor_view: &Entity<EditorView>,
        window: &mut Window,
        cx: &mut gpui::Context<Self>,
    ) {
        if self.editor_focus.is_none() {
            self.editor_focus = Some(editor_view.read(cx).focus_handle(cx));
        }
        if self.observer.is_some() {
            return;
        }
        let sub = cx.observe_in(editor_view, window, |state, editor_view, window, cx| {
            let Some(form) = &state.inline_form else {
                return;
            };
            let view = editor_view.read(cx);
            let still_exists = view
                .editor()
                .document()
                .spans_in(
                    form.block,
                    form.offset..form.offset + lotype_core::OBJECT.len_utf8(),
                )
                .ok()
                .and_then(|spans| spans.into_iter().next())
                .and_then(|span| span.atom)
                .is_some_and(|atom| atom.name == INLINE_TYPE);
            if !still_exists {
                let focus_back = form.input.read(cx).focus_handle(cx).is_focused(window);
                state.inline_form = None;
                if focus_back && let Some(focus) = state.editor_focus.clone() {
                    window.focus(&focus, cx);
                }
                editor_view.update(cx, |_, cx| cx.notify());
            }
        });
        self.observer = Some(sub);
    }

    pub(crate) fn layout_cached(
        &mut self,
        tex: &str,
        display: bool,
    ) -> Result<Arc<Layout>, String> {
        let key = (tex.to_owned(), display);
        if let Some(entry) = self.cache.get(&key) {
            return entry.clone();
        }
        if self.cache.len() >= MAX_LAYOUT_CACHE {
            self.cache.clear();
        }
        let result = typeset_with(tex, display, &self.options)
            .map(Arc::new)
            .map_err(|err| err.message);
        self.cache.insert(key, result.clone());
        result
    }

    fn render_block(
        &mut self,
        content: BlockContent,
        window: &mut Window,
        cx: &mut gpui::Context<Self>,
    ) -> AnyElement {
        let block = content.block;
        let editable = content.is_editable();
        let editing = editable && content.is_editing();
        let editor_view = content.view().clone();
        let tex = content.document.plain_text(block).unwrap_or_default();
        let trimmed = tex.trim();
        let font_size = window.text_style().font_size.to_pixels(window.rem_size());
        let display_em = (font_size * 1.15).max(px(16.));
        let theme = cx.theme();
        let fg = theme.foreground;
        let muted_fg = theme.muted_foreground;
        let border = theme.border;
        let muted_bg = theme.muted;
        let danger = theme.danger;
        let fleqn = self.options.fleqn;
        let leqno = self.options.leqno;

        if !editable {
            self.inline_form = None;
        }

        let rendered_result = if trimmed.is_empty() {
            None
        } else {
            let res = self.layout_cached(trimmed, true);
            if let Ok(layout) = &res {
                self.last_valid.insert(block, layout.clone());
            }
            Some(res)
        };

        if !editing {
            let body = match rendered_result {
                None => div()
                    .w_full()
                    .py_3()
                    .flex()
                    .justify_center()
                    .text_sm()
                    .text_color(muted_fg)
                    .child("Click to write a LaTeX equation")
                    .into_any_element(),
                Some(Ok(layout)) => div()
                    .w_full()
                    .px_4()
                    .py_2p5()
                    .child(display_element(layout, display_em, fg, fleqn, leqno))
                    .overflow_x_scrollbar()
                    .id(elem_id("math-scroll", block))
                    .into_any_element(),
                Some(Err(err)) => v_flex()
                    .w_full()
                    .px_3()
                    .py_2()
                    .gap_1()
                    .child(
                        div()
                            .text_xs()
                            .text_color(danger)
                            .child(format!("LaTeX error: {err}")),
                    )
                    .child(
                        div()
                            .text_xs()
                            .font_family(CODE_FONT)
                            .text_color(muted_fg)
                            .child(tex.clone()),
                    )
                    .into_any_element(),
            };

            let end_offset = tex.len();
            let editor_for_click = editor_view;
            return div()
                .id(elem_id("math-block", block))
                .w_full()
                .rounded_md()
                .border_1()
                .border_color(gpui::transparent_black())
                .when(editable, |el| {
                    el.cursor(CursorStyle::PointingHand)
                        .hover(|style| style.bg(muted_bg.opacity(0.6)).border_color(border))
                        .on_mouse_down(
                            MouseButton::Left,
                            move |event: &MouseDownEvent, window, cx| {
                                if event.modifiers.shift {
                                    return;
                                }
                                cx.stop_propagation();
                                editor_for_click.update(cx, |view, cx| {
                                    let focus = view.focus_handle(cx);
                                    window.focus(&focus, cx);
                                    view.update_editor(cx, |editor| {
                                        let _ = editor.set_selection(Some(Selection::caret(
                                            Pos::new(block, end_offset),
                                        )));
                                    });
                                });
                            },
                        )
                })
                .debug_selector(|| "math-block".to_owned())
                .child(body)
                .into_any_element();
        }

        let stale_preview = self.last_valid.get(&block).cloned();
        let source = div().w_full().children(content.text).into_any_element();
        let editor_for_done = editor_view.clone();
        render_equation_card(
            self,
            editor_view,
            EquationCardProps {
                title: "Equation",
                hint: "Block · Esc to preview",
                empty_placeholder: r"Type LaTeX, e.g. \int_0^\infty e^{-x^2}\,dx = \frac{\sqrt{\pi}}{2}",
                tex,
                copy_key: elem_id("math-copy", block),
                preview: rendered_result,
                stale_preview,
                preview_em: display_em,
                fleqn,
                leqno,
                source,
                delete_action: None,
                done_id: elem_id("math-done", block),
                done_debug_id: "math-done",
                on_done: Box::new(move |_, window, cx| {
                    editor_for_done.update(cx, |view, cx| {
                        view.update_editor(cx, |editor| {
                            let _ = editor.set_selection(Some(Selection::Blocks(vec![block])));
                        });
                        let focus = view.focus_handle(cx);
                        window.focus(&focus, cx);
                    });
                }),
                floating: false,
                debug_id: "math-block-editing",
            },
            cx,
        )
    }

    fn render_inline(
        &mut self,
        content: AtomContent,
        window: &mut Window,
        cx: &mut gpui::Context<Self>,
    ) -> RenderedAtom {
        let block = content.block;
        let offset = content.offset;
        let editable = content.is_editable();
        let editor_view = content.view().clone();
        let tex = inline_tex(content.atom).to_owned();
        let style = window.text_style();
        let font_size: Pixels = style.font_size.to_pixels(window.rem_size());
        let ink = style.color;
        let theme = cx.theme();
        let hover_bg = theme.muted;
        let active_bg = theme.primary.opacity(0.14);
        let danger = theme.danger;

        let is_open = self
            .inline_form
            .as_ref()
            .is_some_and(|form| form.block == block && form.offset == offset);
        let popover = if editable && is_open {
            render_inline_popover(self, &editor_view, block, offset, cx)
        } else {
            None
        };

        let (inner, baseline) = if tex.trim().is_empty() {
            let empty = div()
                .px_1()
                .text_xs()
                .text_color(theme.muted_foreground)
                .child("fx")
                .into_any_element();
            (empty, None)
        } else {
            match self.layout_cached(tex.trim(), false) {
                Ok(layout) => {
                    let (canvas_el, base) = inline_element(layout, font_size, ink);
                    (canvas_el, Some(base + px(INLINE_PAD_Y)))
                }
                Err(_) => {
                    let err = div()
                        .px_1()
                        .rounded_sm()
                        .bg(danger.opacity(0.12))
                        .text_xs()
                        .font_family(CODE_FONT)
                        .text_color(danger)
                        .child(format!("${tex}$"))
                        .into_any_element();
                    (err, None)
                }
            }
        };

        let state_entity = cx.entity();
        let tex_for_click = tex;
        let wrapper = div()
            .id(SharedString::from(format!("inline-math-{block}-{offset}")))
            .px(px(2.))
            .py(px(INLINE_PAD_Y))
            .rounded_sm()
            .when(is_open, |el| el.bg(active_bg))
            .when(editable, |el| {
                el.cursor(CursorStyle::PointingHand)
                    .hover(|style| style.bg(hover_bg))
                    .on_mouse_down(
                        MouseButton::Left,
                        move |event: &MouseDownEvent, window, cx| {
                            if event.modifiers.shift {
                                return;
                            }
                            cx.stop_propagation();
                            open_inline_editor(
                                &state_entity,
                                &editor_view,
                                block,
                                offset,
                                &tex_for_click,
                                window,
                                cx,
                            );
                        },
                    )
            })
            .debug_selector(|| "inline-math".to_owned())
            .child(inner)
            .children(popover)
            .into_any_element();

        match baseline {
            Some(base) => RenderedAtom::new(wrapper).with_baseline(base),
            None => RenderedAtom::new(wrapper),
        }
    }
}
