//! GitHub's alerts, as GitHub draws them: a title with the icon and the
//! colour of their kind. A click on the title picks another kind.

use gpui::{
    AnyElement, App, ClickEvent, Context, FontWeight, Hsla, MouseButton, MouseDownEvent, Rgba,
    anchored, deferred, div, point, prelude::*, px,
};
use gpui_component::list::ListItem;
use gpui_component::{ActiveTheme, Sizable, ThemeStyled, h_flex, v_flex};
use lotype_core::{BlockId, alert};

use super::EditorView;
use crate::icons::{self, icon};

/// The icon of an alert of a kind, Lucide's for GitHub's octicon.
fn icon_of(kind: &str) -> &'static [u8] {
    match kind {
        alert::TIP => icons::TIP,
        alert::IMPORTANT => icons::IMPORTANT,
        alert::WARNING => icons::WARNING,
        alert::CAUTION => icons::CAUTION,
        _ => icons::ALERT,
    }
}

/// One of GitHub's alert colors, in the theme's color scheme.
fn shade(color: alert::Color, cx: &App) -> Hsla {
    let hex = if cx.theme().mode.is_dark() {
        color.dark
    } else {
        color.light
    };
    Rgba::try_from(hex).map_or(cx.theme().foreground, Hsla::from)
}

/// The color of the bar on the left of an alert of `kind`.
pub(super) fn alert_bar(kind: &str, cx: &App) -> Hsla {
    shade(alert::style(kind).border, cx)
}

impl EditorView {
    /// The alert's title, which opens the menu of kinds, with the menu
    /// while it is open.
    pub(super) fn alert_title(&self, block: BlockId, cx: &mut Context<Self>) -> AnyElement {
        let style = alert::style(self.editor.document().alert_kind(block));
        let (current, title, svg) = (style.kind, style.title, icon_of(style.kind));
        let color = shade(style.color, cx);
        let toggle = cx.listener(move |view: &mut Self, _: &MouseDownEvent, _, cx| {
            cx.stop_propagation();
            view.alert_menu = Some(block);
            cx.notify();
        });
        let title = h_flex()
            .gap_1()
            .items_center()
            .text_color(color)
            .font_weight(FontWeight::SEMIBOLD)
            .cursor_pointer()
            .debug_selector(|| "alert-kind".to_owned())
            .on_mouse_down(MouseButton::Left, toggle)
            .child(icon(svg).size_4())
            .child(title)
            .child(
                icon(icons::CHEVRON_DOWN)
                    .xsmall()
                    .text_color(cx.theme().muted_foreground),
            );
        let menu = (self.alert_menu == Some(block)).then(|| self.alert_menu(block, current, cx));
        // The menu hangs from under the title, where this empty line is.
        let under = menu.map(|menu| div().h_0().child(menu));
        v_flex().child(title).children(under).into_any_element()
    }

    fn alert_menu(&self, block: BlockId, current: &str, cx: &mut Context<Self>) -> AnyElement {
        let items = alert::KINDS.into_iter().map(alert::style).map(|style| {
            let (name, title, svg) = (style.kind, style.title, icon_of(style.kind));
            let color = shade(style.color, cx);
            let choose = cx.listener(move |view: &mut Self, _: &ClickEvent, _, cx| {
                view.alert_menu = None;
                view.editor.set_alert_kind(block, name).ok();
                cx.notify();
            });
            let row = h_flex()
                .w_full()
                .gap_2()
                .text_sm()
                .items_center()
                .child(icon(svg).text_color(color))
                .child(div().flex_1().child(title))
                .children((name == current).then(|| icon(icons::CHECK)));
            div()
                .debug_selector(move || format!("alert-kind-{name}"))
                .child(ListItem::new(name).child(row).on_click(choose))
        });
        let close = cx.listener(|view: &mut Self, _: &MouseDownEvent, _, cx| {
            if view.alert_menu.take().is_some() {
                cx.notify();
            }
        });
        let menu = v_flex()
            .popover_style(cx)
            .occlude()
            .p_1()
            .w(px(160.))
            .on_mouse_down_out(close)
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .children(items);
        deferred(anchored().offset(point(px(0.), px(4.))).child(menu))
            .with_priority(1)
            .into_any_element()
    }
}
