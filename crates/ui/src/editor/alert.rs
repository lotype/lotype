//! GitHub's alerts, as GitHub draws them: a title with the icon and the
//! colour of their kind. A click on the title picks another kind.

use gpui::{
    AnyElement, ClickEvent, Context, FontWeight, Hsla, MouseButton, MouseDownEvent, anchored,
    deferred, div, point, prelude::*, px, rgb,
};
use gpui_component::list::ListItem;
use gpui_component::{Sizable, ThemeStyled, h_flex, v_flex};
use lotype_core::{BlockId, Document, alert};

use super::EditorView;
use crate::icons::{self, icon};

/// A kind of alert: its name in the document, its title, colour and icon.
type Kind = (&'static str, &'static str, u32, &'static [u8]);

/// How an alert of a kind looks, as GitHub draws it.
fn look(kind: &'static str) -> Kind {
    let (title, color, svg) = match kind {
        alert::TIP => ("Tip", 0x1a7f37, icons::TIP),
        alert::IMPORTANT => ("Important", 0x8250df, icons::IMPORTANT),
        alert::WARNING => ("Warning", 0x9a6700, icons::WARNING),
        alert::CAUTION => ("Caution", 0xd1242f, icons::CAUTION),
        _ => ("Note", 0x0969da, icons::ALERT),
    };
    (kind, title, color, svg)
}

fn kind(doc: &Document, block: BlockId) -> Kind {
    look(doc.alert_kind(block))
}

/// The colour of the alert's bar and title.
pub(super) fn alert_color(doc: &Document, block: BlockId) -> Hsla {
    rgb(kind(doc, block).2).into()
}

impl EditorView {
    /// The alert's title, which opens the menu of kinds, with the menu
    /// while it is open.
    pub(super) fn alert_title(&self, block: BlockId, cx: &mut Context<Self>) -> AnyElement {
        let (current, title, color, svg) = kind(self.editor.document(), block);
        let toggle = cx.listener(move |view: &mut Self, _: &MouseDownEvent, _, cx| {
            cx.stop_propagation();
            view.alert_menu = Some(block);
            cx.notify();
        });
        let title = h_flex()
            .gap_1()
            .items_center()
            .text_color(rgb(color))
            .font_weight(FontWeight::SEMIBOLD)
            .cursor_pointer()
            .debug_selector(|| "alert-kind".to_owned())
            .on_mouse_down(MouseButton::Left, toggle)
            .child(icon(svg).size_4())
            .child(title)
            .child(icon(icons::CHEVRON_DOWN).xsmall().text_color(rgb(0x8a8f98)));
        let menu = (self.alert_menu == Some(block)).then(|| self.alert_menu(block, current, cx));
        // The menu hangs from under the title, where this empty line is.
        let under = menu.map(|menu| div().h_0().child(menu));
        v_flex().child(title).children(under).into_any_element()
    }

    fn alert_menu(&self, block: BlockId, current: &str, cx: &mut Context<Self>) -> AnyElement {
        let items = alert::KINDS
            .into_iter()
            .map(look)
            .map(|(name, title, color, svg)| {
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
                    .child(icon(svg).text_color(rgb(color)))
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
