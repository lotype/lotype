//! Filterable, keyboard-navigable language picker popover for code block headers.

use gpui::{
    AnyElement, ClickEvent, Focusable as _, KeyDownEvent, MouseButton, MouseDownEvent,
    SharedString, anchored, deferred, div, point, prelude::*, px,
};
use gpui_component::list::ListItem;
use gpui_component::{ActiveTheme as _, ThemeStyled as _, h_flex, v_flex};

use super::controls::scroll_reconciler;
use super::{BlockRenderArgs, CODE_FONT, CodeState, elem_id};
use crate::languages::{CodeLanguage, LANGUAGES, canonical_language};

/// Returns the initial highlight index in [`LANGUAGES`] for `current`.
pub(super) fn initial_language_index(current: &str) -> usize {
    LANGUAGES
        .iter()
        .position(|entry| entry.id == current)
        .unwrap_or(0)
}

/// Filters and ranks [`LANGUAGES`] for `query` so exact ID/alias matches always
/// appear first (e.g. typing `"c"` highlights `C`, `"ts"` highlights `TypeScript`).
pub(super) fn filtered_languages(query: &str) -> Vec<&'static CodeLanguage> {
    let q = query.trim().to_ascii_lowercase();
    if q.is_empty() {
        return LANGUAGES.iter().collect();
    }
    let mut ranked: Vec<(u8, usize, &'static CodeLanguage)> = LANGUAGES
        .iter()
        .enumerate()
        .filter_map(|(idx, entry)| match_rank(entry, &q).map(|rank| (rank, idx, entry)))
        .collect();
    ranked.sort_by_key(|&(rank, idx, _)| (rank, idx));
    ranked.into_iter().map(|(_, _, entry)| entry).collect()
}

fn match_rank(language: &CodeLanguage, query: &str) -> Option<u8> {
    let id = language.id;
    let label = language.label.to_ascii_lowercase();
    if id == query || language.aliases.contains(&query) {
        return Some(0);
    }
    if label == query {
        return Some(1);
    }
    if id.starts_with(query) || label.starts_with(query) {
        return Some(2);
    }
    if language
        .aliases
        .iter()
        .any(|alias| alias.starts_with(query))
    {
        return Some(3);
    }
    if id.contains(query)
        || label.contains(query)
        || language.aliases.iter().any(|alias| alias.contains(query))
    {
        return Some(4);
    }
    None
}

impl CodeState {
    pub(super) fn render_language_menu(
        &self,
        args: &BlockRenderArgs,
        cx: &gpui::Context<Self>,
    ) -> Option<AnyElement> {
        if self.language_menu != Some(args.block) || !args.editable {
            return None;
        }
        let current = args
            .language
            .as_deref()
            .map(canonical_language)
            .unwrap_or_else(|| "text".to_owned());
        let block_id = args.block;
        let state_entity = cx.entity();
        let editor_view = args.editor_view.clone();
        let query = self.language_query.trim().to_ascii_lowercase();
        let matches = filtered_languages(&query);
        let highlighted = if matches.is_empty() {
            0
        } else {
            self.language_highlighted.min(matches.len() - 1)
        };

        let theme = cx.theme();
        let muted = theme.muted_foreground;
        let accent = theme.primary;
        let border = theme.border;
        let foreground = theme.foreground;

        let items: Vec<AnyElement> = matches
            .iter()
            .enumerate()
            .map(|(index, entry)| {
                let is_current = entry.id == current;
                let is_highlighted = index == highlighted;
                let chosen = (entry.id != "text").then_some(entry.id);
                let state = state_entity.clone();
                let view = editor_view.clone();
                let id = entry.id;
                let option_id = SharedString::from(format!("code-lang-option-{block_id}-{id}"));
                let show_id = entry.id != entry.label.to_ascii_lowercase();
                let row = h_flex()
                    .w_full()
                    .items_center()
                    .justify_between()
                    .gap_2()
                    .when(is_highlighted, |el| {
                        el.debug_selector(move || format!("code-lang-active-{id}"))
                    })
                    .child(
                        h_flex()
                            .items_center()
                            .gap_1p5()
                            .min_w_0()
                            .child(div().text_xs().child(entry.label))
                            .when(show_id, |el| {
                                el.child(
                                    div()
                                        .text_xs()
                                        .font_family(CODE_FONT)
                                        .text_color(muted)
                                        .child(entry.id),
                                )
                            }),
                    )
                    .when(is_current, |el| {
                        el.child(div().text_xs().text_color(accent).child("✓"))
                    });
                div()
                    .debug_selector(move || format!("code-lang-{id}"))
                    .child(
                        ListItem::new(option_id)
                            .selected(is_highlighted)
                            .child(row)
                            .on_click(move |_: &ClickEvent, window, cx| {
                                state.update(cx, |state, _| state.close_language_menu());
                                view.update(cx, |view, cx| {
                                    let _ = view.update_editor(cx, |editor| {
                                        editor.set_code_language(block_id, chosen)
                                    });
                                    window.focus(&view.focus_handle(cx), cx);
                                });
                            }),
                    )
                    .into_any_element()
            })
            .collect();

        let query_bar = h_flex()
            .w_full()
            .items_center()
            .justify_between()
            .px_2()
            .py_1()
            .mb_1()
            .border_b_1()
            .border_color(border)
            .debug_selector(|| "code-lang-query".to_owned())
            .child(if self.language_query.is_empty() {
                div().text_xs().text_color(muted).child("Type to filter…")
            } else {
                div()
                    .text_xs()
                    .font_family(CODE_FONT)
                    .text_color(foreground)
                    .child(self.language_query.clone())
            });

        let state_for_close = state_entity.clone();
        let editor_for_close = editor_view.clone();
        let state_for_keys = state_entity.clone();
        let editor_for_keys = editor_view.clone();
        let menu = v_flex()
            .track_focus(&self.language_focus)
            .popover_style(cx)
            .occlude()
            .w(px(196.))
            .p_1()
            .debug_selector(|| "code-language-menu".to_owned())
            .on_key_down(move |event: &KeyDownEvent, window, cx| {
                let key = event.keystroke.key.as_str();
                match key {
                    "escape" => {
                        cx.stop_propagation();
                        state_for_keys.update(cx, |state, _| state.close_language_menu());
                        editor_for_keys.update(cx, |view, cx| {
                            window.focus(&view.focus_handle(cx), cx);
                            cx.notify();
                        });
                    }
                    "down" => {
                        cx.stop_propagation();
                        state_for_keys.update(cx, |state, _| {
                            let count = filtered_languages(&state.language_query).len();
                            if count > 0 {
                                let next = (state.language_highlighted.min(count - 1) + 1) % count;
                                state.language_highlighted = next;
                                state.language_scroll.scroll_to_item(next);
                            }
                        });
                        editor_for_keys.update(cx, |_, cx| cx.notify());
                    }
                    "up" => {
                        cx.stop_propagation();
                        state_for_keys.update(cx, |state, _| {
                            let count = filtered_languages(&state.language_query).len();
                            if count > 0 {
                                let cur = state.language_highlighted.min(count - 1);
                                let prev = (cur + count - 1) % count;
                                state.language_highlighted = prev;
                                state.language_scroll.scroll_to_item(prev);
                            }
                        });
                        editor_for_keys.update(cx, |_, cx| cx.notify());
                    }
                    "backspace" => {
                        cx.stop_propagation();
                        state_for_keys.update(cx, |state, _| {
                            if state.language_query.pop().is_some() {
                                state.language_highlighted = 0;
                                state.language_scroll.scroll_to_item(0);
                            }
                        });
                        editor_for_keys.update(cx, |_, cx| cx.notify());
                    }
                    "enter" | "tab" => {
                        let chosen = {
                            let state = state_for_keys.read(cx);
                            let list = filtered_languages(&state.language_query);
                            list.get(state.language_highlighted.min(list.len().saturating_sub(1)))
                                .map(|lang| (lang.id != "text").then_some(lang.id))
                        };
                        if let Some(chosen) = chosen {
                            cx.stop_propagation();
                            state_for_keys.update(cx, |state, _| state.close_language_menu());
                            editor_for_keys.update(cx, |view, cx| {
                                let _ = view.update_editor(cx, |editor| {
                                    editor.set_code_language(block_id, chosen)
                                });
                                window.focus(&view.focus_handle(cx), cx);
                            });
                        }
                    }
                    _ if key.chars().count() == 1
                        && !event.keystroke.modifiers.platform
                        && !event.keystroke.modifiers.control
                        && !event.keystroke.modifiers.alt =>
                    {
                        cx.stop_propagation();
                        state_for_keys.update(cx, |state, _| {
                            state.language_query.push_str(&key.to_lowercase());
                            state.language_highlighted = 0;
                            state.language_scroll.scroll_to_item(0);
                        });
                        editor_for_keys.update(cx, |_, cx| cx.notify());
                    }
                    _ => {}
                }
            })
            .on_mouse_down_out(move |_: &MouseDownEvent, window, cx| {
                let closed = state_for_close.update(cx, |state, _| {
                    if state.language_menu == Some(block_id) {
                        state.close_language_menu();
                        true
                    } else {
                        false
                    }
                });
                if closed {
                    if state_for_close.read(cx).language_focus.is_focused(window) {
                        window.focus(&editor_for_close.read(cx).focus_handle(cx), cx);
                    }
                    editor_for_close.update(cx, |_, cx| cx.notify());
                }
            })
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(scroll_reconciler(state_entity, editor_view))
            .child(query_bar)
            .child(
                div()
                    .id(elem_id("code-language-scroll", block_id))
                    .track_scroll(&self.language_scroll)
                    .overflow_y_scroll()
                    .max_h(px(240.))
                    .children(if items.is_empty() {
                        vec![
                            div()
                                .px_2()
                                .py_1()
                                .text_xs()
                                .text_color(muted)
                                .child("No matching languages")
                                .into_any_element(),
                        ]
                    } else {
                        items
                    }),
            );
        let placed = anchored()
            .offset(point(px(0.), px(4.)))
            .snap_to_window_with_margin(px(8.))
            .child(menu);
        Some(deferred(placed).with_priority(3).into_any_element())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filtered_languages_ranks_exact_id_and_alias_before_substring_matches() {
        let c_matches: Vec<&str> = filtered_languages("c").into_iter().map(|l| l.id).collect();
        assert_eq!(c_matches[0], "c");
        assert_eq!(c_matches[1], "css");
        assert_eq!(c_matches[2], "cpp");

        let ts_matches: Vec<&str> = filtered_languages("ts").into_iter().map(|l| l.id).collect();
        assert_eq!(ts_matches[0], "typescript");

        let rs_matches: Vec<&str> = filtered_languages("rs").into_iter().map(|l| l.id).collect();
        assert_eq!(rs_matches[0], "rust");

        let sh_matches: Vec<&str> = filtered_languages("sh").into_iter().map(|l| l.id).collect();
        assert_eq!(sh_matches[0], "bash");
    }
}
