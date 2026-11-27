//! GPUI block view for code blocks: Tree-sitter highlighting, built-in & async
//! semantic analysis, folded Shiki Twoslash preview, and hover popovers.

mod controls;
mod header;
mod language_menu;

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use gpui::{
    Anchor, AnyElement, App, AppContext as _, Bounds, Entity, FocusHandle, MouseExitEvent,
    MouseMoveEvent, Pixels, ScrollHandle, SharedString, Subscription, Task, Window, anchored,
    deferred, div, point, prelude::*, px,
};
use gpui_component::input::InputState;
use gpui_component::{ActiveTheme as _, ThemeStyled as _, v_flex};
use lotype_core::{BlockId, Document, block};
use lotype_ui::{EditorView, TextDecoration};

use crate::analyzer::{AnalysisRequest, CodeAnalysis, CodeAnalyzer, HoverAnnotation};
use crate::highlight::{CODE_FONT as HL_FONT, HighlightCache, build_text_decoration};
use crate::infer::BuiltInAnalyzer;
use crate::lsp::LspAnalyzer;
use crate::twoslash::{ParsedCode, parse_code_annotations};

const ANALYZE_DEBOUNCE: Duration = Duration::from_millis(80);
pub(super) const COPIED_FEEDBACK: Duration = Duration::from_millis(1400);
pub(super) const CODE_FONT: &str = HL_FONT;

pub(super) fn elem_id(prefix: &str, block: BlockId) -> SharedString {
    SharedString::from(format!("{prefix}-{block}"))
}

/// Builder that configures and installs rich code blocks onto an [`EditorView`].
#[derive(Clone, Default)]
pub struct CodeBlocks {
    analyzer: Option<Arc<dyn CodeAnalyzer>>,
}

impl CodeBlocks {
    /// Creates a new configuration with Tree-sitter highlighting, JavaScript/TypeScript
    /// Twoslash directives, and zero-latency [`BuiltInAnalyzer`] type/hover inference.
    pub fn new() -> Self {
        Self::default()
    }

    /// Attaches an asynchronous [`CodeAnalyzer`] (such as an LSP bridge or custom compiler)
    /// to resolve `^?` queries, `^|` completions, hover signatures, and diagnostics.
    pub fn with_analyzer(mut self, analyzer: Arc<dyn CodeAnalyzer>) -> Self {
        self.analyzer = Some(analyzer);
        self
    }

    /// Attaches a stdio [`LspAnalyzer`] for external language servers (`tsserver`, `deno lsp`, `rust-analyzer`).
    pub fn with_lsp(self, lsp: LspAnalyzer) -> Self {
        self.with_analyzer(Arc::new(lsp))
    }

    /// Registers the code block decorator and view on `editor`.
    pub fn install(self, editor: EditorView, cx: &mut App) -> EditorView {
        let state = cx.new(|cx| CodeState::new(self.analyzer, cx.focus_handle()));
        let decorator_state = state.clone();
        let view_state = state;
        editor
            .with_text_decorator(
                block::CODE_BLOCK,
                move |block_id, doc, text, editing, _window, cx| {
                    decorator_state.update(cx, |state, cx| {
                        state.decorate(block_id, doc, text, editing, cx)
                    })
                },
            )
            .with_block_view(block::CODE_BLOCK, move |content, window, cx| {
                let block_id = content.block;
                let language = content.document.code_language(block_id);
                let title = content.document.code_title(block_id);
                let twoslash = content.document.is_twoslash(block_id);
                let code = content.document.plain_text(block_id).unwrap_or_default();
                let editable = content.is_editable();
                let editor_view = content.view().clone();
                let text = content.text;
                view_state.update(cx, |state, cx| {
                    state.render_block(
                        BlockRenderArgs {
                            block: block_id,
                            language,
                            title,
                            twoslash,
                            code,
                            editable,
                            editor_view,
                            text,
                        },
                        window,
                        cx,
                    )
                })
            })
    }
}

/// Convenience installer for [`CodeBlocks::new()`].
pub fn install(editor: EditorView, cx: &mut App) -> EditorView {
    CodeBlocks::new().install(editor, cx)
}

pub(super) struct BlockRenderArgs {
    pub(super) block: BlockId,
    pub(super) language: Option<String>,
    pub(super) title: Option<String>,
    pub(super) twoslash: bool,
    pub(super) code: String,
    pub(super) editable: bool,
    pub(super) editor_view: Entity<EditorView>,
    pub(super) text: Option<AnyElement>,
}

struct AnalysisCacheEntry {
    key: (Option<String>, Option<String>, String),
    analysis: Option<CodeAnalysis>,
    _task: Option<Task<()>>,
}

struct BuiltinCacheEntry {
    key: (Option<String>, String),
    analysis: CodeAnalysis,
}

pub(super) struct TitleForm {
    pub(super) block: BlockId,
    pub(super) initial: Option<String>,
    pub(super) input: Entity<InputState>,
    pub(super) _sub: Subscription,
}

#[derive(Clone, PartialEq)]
pub(super) struct ActiveHover {
    pub(super) block: BlockId,
    pub(super) language: Option<String>,
    pub(super) text: String,
    pub(super) editing: bool,
    pub(super) hover: HoverAnnotation,
    pub(super) bounds: Bounds<Pixels>,
}

struct CachedParsed {
    text: String,
    editing: bool,
    parsed: ParsedCode,
}

pub(super) struct CodeState {
    analyzer: Option<Arc<dyn CodeAnalyzer>>,
    pub(super) language_focus: FocusHandle,
    pub(super) editor_focus: Option<FocusHandle>,
    pub(super) control_subs: Vec<Subscription>,
    pub(super) reconcile_scheduled: bool,
    highlights: HighlightCache,
    parsed: HashMap<BlockId, CachedParsed>,
    builtin_cache: HashMap<BlockId, BuiltinCacheEntry>,
    analyses: HashMap<BlockId, AnalysisCacheEntry>,
    pub(super) language_menu: Option<BlockId>,
    pub(super) language_scroll: ScrollHandle,
    pub(super) language_query: String,
    pub(super) language_highlighted: usize,
    pub(super) title_form: Option<TitleForm>,
    pub(super) copied: Option<(BlockId, Task<()>)>,
    pub(super) hovered: Option<ActiveHover>,
}

impl CodeState {
    fn new(analyzer: Option<Arc<dyn CodeAnalyzer>>, language_focus: FocusHandle) -> Self {
        Self {
            analyzer,
            language_focus,
            editor_focus: None,
            control_subs: Vec::new(),
            reconcile_scheduled: false,
            highlights: HighlightCache::default(),
            parsed: HashMap::new(),
            builtin_cache: HashMap::new(),
            analyses: HashMap::new(),
            language_menu: None,
            language_scroll: ScrollHandle::new(),
            language_query: String::new(),
            language_highlighted: 0,
            title_form: None,
            copied: None,
            hovered: None,
        }
    }

    pub(super) fn close_language_menu(&mut self) {
        self.language_menu = None;
        self.language_query.clear();
        self.language_highlighted = 0;
    }

    fn decorate(
        &mut self,
        block: BlockId,
        doc: &Document,
        text: &str,
        editing: bool,
        cx: &mut gpui::Context<Self>,
    ) -> TextDecoration {
        self.prune_caches(doc);
        let language = doc.code_language(block);
        let title = doc.code_title(block);
        let twoslash = doc.is_twoslash(block);
        let fence_lines = doc.code_highlights(block);
        if self.hovered.as_ref().is_some_and(|h| {
            h.block == block && (!twoslash || h.text != text || h.editing != editing)
        }) {
            self.hovered = None;
        }
        let mut parsed = parse_code_annotations(text, fence_lines.as_deref(), twoslash);

        if twoslash {
            if self.analyzer.is_some() {
                if let Some(analysis) = self.cached_analysis(block, &language, &title, text) {
                    parsed.merge_analysis(&analysis);
                }
            } else {
                let builtin = self.builtin_analysis(block, &language, &title, text, &parsed);
                parsed.merge_analysis(&builtin);
            }
        } else {
            self.analyses.remove(&block);
            self.builtin_cache.remove(&block);
        }

        let decoration = build_text_decoration(
            &mut self.highlights,
            block,
            language.as_deref(),
            text,
            &parsed,
            editing,
            cx,
        );
        self.parsed.insert(
            block,
            CachedParsed {
                text: text.to_owned(),
                editing,
                parsed,
            },
        );
        decoration
    }

    fn prune_caches(&mut self, doc: &Document) {
        let alive = |id: &BlockId| {
            doc.block_type(*id)
                .is_ok_and(|kind| kind == block::CODE_BLOCK)
        };
        self.highlights.retain_blocks(alive);
        self.parsed.retain(|id, _| alive(id));
        self.builtin_cache.retain(|id, _| alive(id));
        self.analyses.retain(|id, _| alive(id));
        if self.hovered.as_ref().is_some_and(|h| !alive(&h.block)) {
            self.hovered = None;
        }
    }

    fn builtin_analysis(
        &mut self,
        block: BlockId,
        language: &Option<String>,
        title: &Option<String>,
        code: &str,
        parsed: &ParsedCode,
    ) -> CodeAnalysis {
        if let Some(entry) = self.builtin_cache.get(&block)
            && entry.key.0 == *language
            && entry.key.1 == code
        {
            return entry.analysis.clone();
        }
        let req = AnalysisRequest {
            language: language.clone(),
            title: title.clone(),
            code: code.to_owned(),
            queries: parsed.query_sites(),
            completions: parsed.completion_sites(),
            files: parsed.files.clone(),
            no_errors: parsed.no_errors,
            expected_errors: parsed.expected_errors.clone(),
        };
        let analysis = BuiltInAnalyzer::analyze_sync(&req);
        self.builtin_cache.insert(
            block,
            BuiltinCacheEntry {
                key: (language.clone(), code.to_owned()),
                analysis: analysis.clone(),
            },
        );
        analysis
    }

    fn cached_analysis(
        &self,
        block: BlockId,
        language: &Option<String>,
        title: &Option<String>,
        code: &str,
    ) -> Option<CodeAnalysis> {
        let entry = self.analyses.get(&block)?;
        (entry.key.0 == *language && entry.key.1 == *title && entry.key.2 == code)
            .then(|| entry.analysis.clone())
            .flatten()
    }

    fn ensure_analysis(&mut self, args: &BlockRenderArgs, cx: &mut gpui::Context<Self>) {
        if !args.editable {
            return;
        }
        if !args.twoslash {
            self.analyses.remove(&args.block);
            return;
        }
        let Some(analyzer) = self.analyzer.clone() else {
            return;
        };
        let key = (args.language.clone(), args.title.clone(), args.code.clone());
        if self
            .analyses
            .get(&args.block)
            .is_some_and(|entry| entry.key == key)
        {
            return;
        }
        let fresh;
        let parsed = match self.parsed.get(&args.block) {
            Some(entry) if entry.text == args.code => &entry.parsed,
            _ => {
                fresh = parse_code_annotations(&args.code, None, args.twoslash);
                &fresh
            }
        };
        let request = AnalysisRequest {
            language: args.language.clone(),
            title: args.title.clone(),
            code: args.code.clone(),
            queries: parsed.query_sites(),
            completions: parsed.completion_sites(),
            files: parsed.files.clone(),
            no_errors: parsed.no_errors,
            expected_errors: parsed.expected_errors.clone(),
        };
        let fallback_req = request.clone();
        let block_id = args.block;
        let editor_view = args.editor_view.clone();
        let expected_key = key.clone();
        let task = cx.spawn(async move |state, cx| {
            cx.background_executor().timer(ANALYZE_DEBOUNCE).await;
            let analysis = match analyzer.analyze(request).await {
                Ok(analysis) => analysis,
                Err(_) => BuiltInAnalyzer::analyze_sync(&fallback_req),
            };
            let updated = state.update(cx, |state, _| {
                let Some(entry) = state.analyses.get_mut(&block_id) else {
                    return false;
                };
                if entry.key != expected_key {
                    return false;
                }
                entry.analysis = Some(analysis);
                true
            });
            if matches!(updated, Ok(true)) {
                editor_view.update(cx, |_, cx| cx.notify());
            }
        });
        self.analyses.insert(
            block_id,
            AnalysisCacheEntry {
                key,
                analysis: None,
                _task: Some(task),
            },
        );
    }

    fn render_block(
        &mut self,
        args: BlockRenderArgs,
        window: &mut Window,
        cx: &mut gpui::Context<Self>,
    ) -> AnyElement {
        if !args.editable {
            self.close_controls_if_readonly(window, cx);
            self.hovered = None;
        } else if !args.twoslash && self.hovered.as_ref().is_some_and(|h| h.block == args.block) {
            self.hovered = None;
        }
        self.ensure_analysis(&args, cx);

        let header = self.render_header(&args, window, cx);
        let hover_card = self.render_hover_popover(args.block, cx);

        let block_id = args.block;
        let twoslash = args.twoslash;
        let language_for_hover = args.language.clone();
        let editor_for_move = args.editor_view.clone();
        let state_for_move = cx.entity();
        let state_for_leave = cx.entity();
        let state_for_scroll = cx.entity();
        let editor_for_scroll = args.editor_view.clone();

        let body = div()
            .id(elem_id("code-body", block_id))
            .w_full()
            .px_3()
            .py_2p5()
            .debug_selector(|| "code-body".to_owned())
            .on_mouse_move(move |event: &MouseMoveEvent, window, cx| {
                if !twoslash {
                    return;
                }
                let hit = editor_for_move
                    .read(cx)
                    .byte_offset_at(block_id, event.position);
                let changed = state_for_move.update(cx, |state, cx| {
                    let cleared = state.reconcile_visible(&editor_for_move, window, cx);
                    if state.language_menu.is_some() || state.title_form.is_some() {
                        return cleared;
                    }
                    let next = hit.and_then(|offset| {
                        let entry = state.parsed.get(&block_id)?;
                        let hover = entry.parsed.hover_at(offset)?.clone();
                        let bounds = editor_for_move
                            .read(cx)
                            .range_bounds(block_id, hover.range.clone())?;
                        Some(ActiveHover {
                            block: block_id,
                            language: language_for_hover.clone(),
                            text: entry.text.clone(),
                            editing: entry.editing,
                            hover,
                            bounds,
                        })
                    });
                    if next.is_some() {
                        state.ensure_observers(&editor_for_move, window, cx);
                    }
                    if state.hovered != next {
                        state.hovered = next;
                        true
                    } else {
                        cleared
                    }
                });
                if changed {
                    editor_for_move.update(cx, |_, cx| cx.notify());
                }
            })
            .on_mouse_exit(move |_: &MouseExitEvent, _, cx| {
                let clear = state_for_leave.update(cx, |state, _| {
                    if state.hovered.as_ref().is_some_and(|h| h.block == block_id) {
                        state.hovered = None;
                        true
                    } else {
                        false
                    }
                });
                if clear {
                    args.editor_view.update(cx, |_, cx| cx.notify());
                }
            })
            .on_scroll(move |_, _, cx| {
                let clear = state_for_scroll.update(cx, |state, _| {
                    if state.hovered.as_ref().is_some_and(|h| h.block == block_id) {
                        state.hovered = None;
                        true
                    } else {
                        false
                    }
                });
                if clear {
                    editor_for_scroll.update(cx, |_, cx| cx.notify());
                }
            })
            .children(args.text);

        let theme = cx.theme();
        v_flex()
            .w_full()
            .rounded_md()
            .border_1()
            .border_color(theme.border)
            .bg(theme.muted)
            .debug_selector(|| "code-block".to_owned())
            .child(header)
            .child(body)
            .children(hover_card)
            .into_any_element()
    }

    fn render_hover_popover(
        &mut self,
        block: BlockId,
        cx: &gpui::Context<Self>,
    ) -> Option<AnyElement> {
        if self.language_menu.is_some() || self.title_form.is_some() {
            return None;
        }
        let active = self.hovered.as_ref().filter(|h| h.block == block)?.clone();
        let signature = self.highlights.highlight_snippet(
            active.language.as_deref(),
            &active.hover.signature,
            cx,
        );
        let theme = cx.theme();
        let border = theme.border;
        let docs = active.hover.docs.map(|doc| {
            div()
                .pt_1p5()
                .mt_1p5()
                .border_t_1()
                .border_color(border)
                .text_xs()
                .text_color(theme.muted_foreground)
                .child(doc)
        });
        let card = v_flex()
            .popover_style(cx)
            .px_2p5()
            .py_1p5()
            .max_w(px(420.))
            .debug_selector(|| "twoslash-hover".to_owned())
            .child(div().text_xs().font_family(CODE_FONT).child(signature))
            .children(docs);
        let place_below = active.bounds.top() < px(72.);
        let (card_anchor, attach_anchor, y_offset) = if place_below {
            (Anchor::TopLeft, Anchor::BottomLeft, px(4.))
        } else {
            (Anchor::BottomLeft, Anchor::TopLeft, px(-4.))
        };
        let placed = anchored()
            .anchor(card_anchor)
            .attach(active.bounds, attach_anchor)
            .offset(point(px(0.), y_offset))
            .snap_to_window_with_margin(px(8.))
            .child(card);
        Some(deferred(placed).with_priority(2).into_any_element())
    }
}
