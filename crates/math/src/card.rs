//! Shared equation editor card used by both block equations and the inline
//! equation popover so header, live KaTeX preview, error banner, Copy LaTeX,
//! and monospace source editor stay visually and behaviorally identical.

use std::sync::Arc;
use std::time::Duration;

use gpui::{
    AnyElement, App, ClickEvent, ClipboardItem, Entity, FontWeight, MouseButton, Pixels,
    SharedString, Window, div, prelude::*, px,
};
use gpui_component::Icon;
use gpui_component::button::{Button, ButtonCustomVariant, ButtonVariants as _};
use gpui_component::scroll::ScrollableElement as _;
use gpui_component::{ActiveTheme as _, Sizable as _, h_flex, v_flex};
use lotype_ui::EditorView;

use crate::render::{CachedLayout, display_element};
use crate::view::MathState;

pub(crate) const CODE_FONT: &str = "Menlo";
const COPIED_FEEDBACK: Duration = Duration::from_millis(1400);

pub(crate) type ActionHandler = Box<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>;

pub(crate) struct EquationCardProps {
    pub(crate) title: &'static str,
    pub(crate) hint: &'static str,
    pub(crate) empty_placeholder: &'static str,
    pub(crate) tex: String,
    pub(crate) copy_key: SharedString,
    pub(crate) preview: Option<Result<Arc<CachedLayout>, String>>,
    pub(crate) stale_preview: Option<Arc<CachedLayout>>,
    pub(crate) preview_em: Pixels,
    pub(crate) fleqn: bool,
    pub(crate) leqno: bool,
    pub(crate) source: AnyElement,
    pub(crate) delete_action: Option<(SharedString, &'static str, ActionHandler)>,
    pub(crate) done_id: SharedString,
    pub(crate) done_debug_id: &'static str,
    pub(crate) on_done: ActionHandler,
    pub(crate) floating: bool,
    pub(crate) debug_id: &'static str,
}

pub(crate) fn render_equation_card(
    state: &MathState,
    editor_view: Entity<EditorView>,
    props: EquationCardProps,
    cx: &gpui::Context<MathState>,
) -> AnyElement {
    let theme = cx.theme();
    let fg = theme.foreground;
    let muted_fg = theme.muted_foreground;
    let border = theme.border;
    let muted_bg = theme.muted;
    let bg = theme.background;
    let danger = theme.danger;
    let accent = theme.primary.opacity(0.6);

    let preview_scroll_id = SharedString::from(format!("math-preview-scroll-{}", props.copy_key));
    let stale_scroll_id = SharedString::from(format!("math-stale-scroll-{}", props.copy_key));
    let copy_button = render_copy_button(state, props.copy_key, props.tex, editor_view, cx);
    let delete_button = props
        .delete_action
        .map(|(id, debug_id, on_delete)| render_delete_button(id, debug_id, on_delete, cx));
    let done_button = render_done_button(props.done_id, props.done_debug_id, props.on_done, cx);

    let header = h_flex()
        .w_full()
        .rounded_t_md()
        .justify_between()
        .items_center()
        .gap_2()
        .px_2p5()
        .py_1()
        .border_b_1()
        .border_color(border)
        .bg(bg.opacity(0.6))
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .child(
            h_flex()
                .gap_2()
                .items_center()
                .min_w_0()
                .child(
                    div()
                        .text_xs()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(fg)
                        .child(props.title),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(muted_fg)
                        .truncate()
                        .child(props.hint),
                ),
        )
        .child(
            h_flex()
                .gap_1()
                .items_center()
                .flex_none()
                .child(copy_button)
                .children(delete_button)
                .child(done_button),
        );

    let preview = match props.preview {
        None => div()
            .w_full()
            .py_3()
            .px_3()
            .flex()
            .justify_center()
            .text_xs()
            .text_color(muted_fg)
            .child(props.empty_placeholder)
            .into_any_element(),
        Some(Ok(layout)) => div()
            .w_full()
            .px_4()
            .py_2p5()
            .child(display_element(
                layout,
                props.preview_em,
                fg,
                props.fleqn,
                props.leqno,
            ))
            .overflow_x_scrollbar()
            .id(preview_scroll_id)
            .into_any_element(),
        Some(Err(err)) => v_flex()
            .w_full()
            .px_3()
            .py_2()
            .gap_1p5()
            .children(props.stale_preview.map(|layout| {
                div()
                    .w_full()
                    .opacity(0.45)
                    .debug_selector(|| "math-stale-preview".to_owned())
                    .child(display_element(
                        layout,
                        props.preview_em,
                        fg,
                        props.fleqn,
                        props.leqno,
                    ))
                    .overflow_x_scrollbar()
                    .id(stale_scroll_id)
            }))
            .child(
                div()
                    .w_full()
                    .px_2()
                    .py_1()
                    .rounded_sm()
                    .bg(danger.opacity(0.1))
                    .text_xs()
                    .text_color(danger)
                    .debug_selector(|| "math-error".to_owned())
                    .child(err),
            )
            .into_any_element(),
    };

    let source = div()
        .w_full()
        .px_3()
        .py_2()
        .border_t_1()
        .border_color(border)
        .bg(bg.opacity(0.4))
        .rounded_b_md()
        .font_family(CODE_FONT)
        .text_sm()
        .debug_selector(|| "math-source".to_owned())
        .child(props.source);

    let debug_id = props.debug_id;
    let card = v_flex()
        .w_full()
        .rounded_md()
        .border_1()
        .border_color(accent)
        .bg(muted_bg)
        .debug_selector(move || debug_id.to_owned())
        .child(header)
        .child(preview)
        .child(source);

    if props.floating {
        div()
            .w(px(380.))
            .rounded_md()
            .bg(bg)
            .shadow_md()
            .occlude()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(card)
            .into_any_element()
    } else {
        card.into_any_element()
    }
}

fn render_copy_button(
    state: &MathState,
    copy_key: SharedString,
    tex: String,
    editor_view: Entity<EditorView>,
    cx: &gpui::Context<MathState>,
) -> AnyElement {
    let is_copied = state
        .copied
        .as_ref()
        .is_some_and(|(copied_key, _)| *copied_key == copy_key);
    let tooltip = if is_copied { "Copied" } else { "Copy LaTeX" };
    let state_entity = cx.entity();
    let key_for_click = copy_key.clone();

    Button::new(copy_key)
        .ghost()
        .xsmall()
        .icon(Icon::default().data(include_bytes!("../assets/icons/clipboard.svg")))
        .tooltip(tooltip)
        .tooltip_show_delay(Duration::from_millis(250))
        .accessibility_label(tooltip)
        .when(is_copied, |button| button.label("Copied"))
        .debug_selector(|| "math-copy".to_owned())
        .on_click(move |_: &ClickEvent, _, cx| {
            cx.write_to_clipboard(ClipboardItem::new_string(tex.clone()));
            let notify_editor = editor_view.clone();
            let active_key = key_for_click.clone();
            state_entity.update(cx, |state, cx| {
                let clear_key = active_key.clone();
                let reset = cx.spawn(async move |state, cx| {
                    cx.background_executor().timer(COPIED_FEEDBACK).await;
                    let cleared = state.update(cx, |state, cx| {
                        if state
                            .copied
                            .as_ref()
                            .is_some_and(|(key, _)| *key == clear_key)
                        {
                            state.copied = None;
                            cx.notify();
                            true
                        } else {
                            false
                        }
                    });
                    if matches!(cleared, Ok(true)) {
                        notify_editor.update(cx, |_, cx| cx.notify());
                    }
                });
                state.copied = Some((active_key, reset));
                cx.notify();
            });
            editor_view.update(cx, |_, cx| cx.notify());
        })
        .into_any_element()
}

fn render_done_button(
    id: SharedString,
    debug_id: &'static str,
    on_done: ActionHandler,
    cx: &App,
) -> AnyElement {
    let accent = cx.theme().blue;
    let style = ButtonCustomVariant::new(cx)
        .color(accent)
        .foreground(cx.theme().background)
        .hover(accent.opacity(0.9))
        .active(accent.opacity(0.8));
    Button::new(id)
        .xsmall()
        .custom(style)
        .bg(accent)
        .label("Done")
        .debug_selector(move || debug_id.to_owned())
        .on_click(move |event: &ClickEvent, window, cx| on_done(event, window, cx))
        .into_any_element()
}

fn render_delete_button(
    id: SharedString,
    debug_id: &'static str,
    on_delete: ActionHandler,
    cx: &App,
) -> AnyElement {
    let danger = cx.theme().danger;
    let style = ButtonCustomVariant::new(cx)
        .color(danger.opacity(0.12))
        .foreground(danger)
        .hover(danger.opacity(0.22))
        .active(danger.opacity(0.32));
    Button::new(id)
        .xsmall()
        .custom(style)
        .bg(danger.opacity(0.12))
        .label("Delete")
        .debug_selector(move || debug_id.to_owned())
        .on_click(move |event: &ClickEvent, window, cx| on_delete(event, window, cx))
        .into_any_element()
}
