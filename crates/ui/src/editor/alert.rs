//! GitHub's alerts, as GitHub draws them: a title with the icon and the
//! colour of their kind. A click on the title picks another kind.

use std::collections::HashMap;

use gpui::{
    AnyElement, App, ClickEvent, Context, FontWeight, Hsla, MouseButton, MouseDownEvent, Rgba,
    SharedString, anchored, deferred, div, point, prelude::*, px,
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

/// A color in the light and in the dark color scheme.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Shade {
    pub light: Hsla,
    pub dark: Hsla,
}

impl Shade {
    /// The same color in both schemes.
    pub fn same(color: Hsla) -> Self {
        Self {
            light: color,
            dark: color,
        }
    }

    /// One of GitHub's, from its hex values.
    fn github(color: alert::Color) -> Self {
        let hex = |hex: &str| Rgba::try_from(hex).map(Hsla::from).unwrap_or_default();
        Self {
            light: hex(color.light),
            dark: hex(color.dark),
        }
    }

    fn of(self, dark: bool) -> Hsla {
        if dark { self.dark } else { self.light }
    }
}

/// How an alert of a kind looks: its title, the bar on its left, and the
/// color of its title and icon. GitHub's is the default; an application
/// gives its own with [`EditorView::with_alert_look`].
#[derive(Debug, Clone, PartialEq)]
pub struct AlertLook {
    pub title: SharedString,
    pub bar: Shade,
    pub color: Shade,
}

impl AlertLook {
    /// How GitHub draws an alert of `kind`, a note for a kind no one knows.
    pub fn github(kind: &str) -> Self {
        let style = alert::style(kind);
        Self {
            title: style.title.into(),
            bar: Shade::github(style.border),
            color: Shade::github(style.color),
        }
    }
}

/// The look of an alert of `kind`: the application's, or GitHub's.
fn look_of(looks: &HashMap<&'static str, AlertLook>, kind: &str) -> AlertLook {
    let kind = alert::style(kind).kind;
    looks
        .get(kind)
        .cloned()
        .unwrap_or_else(|| AlertLook::github(kind))
}

impl EditorView {
    /// Draws alerts of `kind`, one of [`alert::KINDS`], as `look` says
    /// instead of as GitHub does. A kind no alert is of changes nothing.
    pub fn with_alert_look(mut self, kind: &str, look: AlertLook) -> Self {
        if let Some(kind) = alert::KINDS
            .into_iter()
            .find(|known| known.eq_ignore_ascii_case(kind))
        {
            self.alert_looks.insert(kind, look);
        }
        self
    }

    /// The color of the bar on the left of an alert of `kind`.
    pub(super) fn alert_bar(&self, kind: &str, cx: &App) -> Hsla {
        look_of(&self.alert_looks, kind)
            .bar
            .of(cx.theme().mode.is_dark())
    }

    /// The alert's title, which opens the menu of kinds, with the menu
    /// while it is open.
    pub(super) fn alert_title(&self, block: BlockId, cx: &mut Context<Self>) -> AnyElement {
        let current = self.editor.document().alert_kind(block);
        let look = look_of(&self.alert_looks, current);
        let (title, svg) = (look.title, icon_of(current));
        let color = look.color.of(cx.theme().mode.is_dark());
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
        let dark = cx.theme().mode.is_dark();
        let items = alert::KINDS.into_iter().map(|name| {
            let look = look_of(&self.alert_looks, name);
            let (title, svg) = (look.title, icon_of(name));
            let color = look.color.of(dark);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_applications_look_replaces_githubs_for_its_kind_only() {
        let red = Hsla::from(Rgba::try_from("#ff0000").unwrap());
        let hint = AlertLook {
            title: "Hint".into(),
            bar: Shade::same(red),
            color: Shade::same(red),
        };
        let looks = HashMap::from([(alert::TIP, hint.clone())]);
        assert_eq!(look_of(&looks, "TIP"), hint);
        assert_eq!(look_of(&looks, alert::NOTE), AlertLook::github(alert::NOTE));
        // GitHub's own, in each scheme.
        let note = AlertLook::github("rumour");
        assert_eq!(note.title.as_ref(), "Note");
        let blue = |hex| Hsla::from(Rgba::try_from(hex).unwrap());
        assert_eq!(note.bar.of(false), blue("#0969da"));
        assert_eq!(note.color.of(true), blue("#4493f8"));
    }
}
