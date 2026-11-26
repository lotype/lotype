//! Editable image and file blocks. The host owns attachment storage and resolution.

mod form;

use std::path::PathBuf;
use std::rc::Rc;

use gpui::{
    AnyElement, App, ClickEvent, Context, Div, ImageSource, MouseButton, SharedString, StyledImage,
    Task, div, img, prelude::*, px,
};
use gpui_component::button::{Button, ButtonVariants};
use gpui_component::{ActiveTheme, Sizable, h_flex, v_flex};
use lotype_core::{BlockId, BlockKind, Document};

use super::EditorView;
use crate::icons;
use form::Form;

/// A location the host authorizes the editor to load or open.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MediaSource {
    /// A web URL using HTTP or HTTPS.
    Url(String),
    /// A local file selected or resolved by the host application.
    Path(PathBuf),
}

type Importer = Rc<dyn Fn(PathBuf, &mut App) -> Task<Result<String, String>>>;
type Resolver = Rc<dyn Fn(&str) -> Option<MediaSource>>;

/// Per-view attachment integration and the open editor, if any.
#[derive(Default)]
pub(super) struct Media {
    importer: Option<Importer>,
    resolver: Option<Resolver>,
    form: Option<Form>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Image,
    File,
}

#[derive(Clone, PartialEq, Eq)]
struct Attachment {
    kind: Kind,
    reference: String,
    label: String,
}

impl Attachment {
    fn read(doc: &Document, block: BlockId) -> Option<Self> {
        let (kind, reference, label) = match BlockKind::of(doc, block)? {
            BlockKind::Image { url, caption } => (Kind::Image, url, caption),
            BlockKind::File { url, name } => (Kind::File, url, name),
            _ => return None,
        };
        Some(Self {
            kind,
            reference,
            label,
        })
    }
}

impl EditorView {
    /// Enables the native file picker for image and file blocks.
    ///
    /// On Save, the host stores the selected file and returns the reference to
    /// put in the document, or an error message to display.
    /// If a concurrent edit invalidates the save, the host owns cleanup of
    /// any bytes it already persisted.
    pub fn with_media_importer(
        mut self,
        import: impl Fn(PathBuf, &mut App) -> Task<Result<String, String>> + 'static,
    ) -> Self {
        self.media.importer = Some(Rc::new(import));
        self
    }

    /// Resolves stored references for image previews and the file Open action.
    ///
    /// The callback runs while rendering and must return promptly. Validate any
    /// reference from a document before granting access to a local file. Without
    /// a resolver, only web URLs are used. Image requests use the application's GPUI HTTP client.
    pub fn with_media_resolver(
        mut self,
        resolve: impl Fn(&str) -> Option<MediaSource> + 'static,
    ) -> Self {
        self.media.resolver = Some(Rc::new(resolve));
        self
    }

    fn resolve_media(&self, reference: &str) -> Option<MediaSource> {
        if reference.is_empty() {
            return None;
        }
        let source = match &self.media.resolver {
            Some(resolve) => resolve(reference)?,
            None => MediaSource::Url(reference.to_owned()),
        };
        match &source {
            MediaSource::Url(url) if !web_address(url) => None,
            _ => Some(source),
        }
    }

    /// Keeps the active form mounted while its row is offscreen.
    pub(super) fn media_form_block(&self) -> Option<BlockId> {
        self.media.form.as_ref().map(|form| form.block)
    }

    pub(super) fn media_block(&self, block: BlockId, cx: &mut Context<Self>) -> Option<AnyElement> {
        let media = Attachment::read(self.editor.document(), block)?;
        let frame = v_flex().min_w_0().gap_2().id(block.to_string());
        if let Some(form) = &self.media.form
            && form.block == block
        {
            return Some(frame.child(self.media_form(form, cx)).into_any_element());
        }
        let editing = self.ai.session.is_none() && self.media.form.is_none();
        let edit = cx.listener(move |view, _: &ClickEvent, window, cx| {
            view.open_media_form(block, window, cx);
        });
        let empty = media.reference.is_empty();
        let symbol = match media.kind {
            Kind::Image => icons::IMAGE,
            Kind::File => icons::FILE,
        };
        let action = match (media.kind, empty) {
            (Kind::Image, true) => "Add image",
            (Kind::File, true) => "Add file",
            (Kind::Image, false) => "Edit image",
            (Kind::File, false) => "Edit file",
        };
        let edit = control("media-edit", action).on_click(edit);
        if empty {
            let placeholder = h_flex()
                .gap_2()
                .min_w_0()
                .child(icons::icon(symbol))
                .children((!media.label.is_empty()).then(|| {
                    div()
                        .min_w_0()
                        .flex_1()
                        .debug_selector(|| "media-title".into())
                        .child(media.label.clone())
                }))
                .when(editing, |row| row.child(edit))
                .when(!editing && media.label.is_empty(), |row| row.child(action));
            return Some(frame.child(panel(cx).child(placeholder)).into_any_element());
        }

        let source = self.resolve_media(&media.reference);
        let label = if media.label.is_empty() {
            &media.reference
        } else {
            &media.label
        };
        let row = h_flex().gap_2().min_w_0();
        let open = source
            .clone()
            .filter(|_| media.kind == Kind::File)
            .map(|destination| {
                let shown = media.clone();
                control("media-open", "Open").on_click(cx.listener(
                    move |view, _: &ClickEvent, _, cx| {
                        if Attachment::read(view.editor.document(), block).as_ref() != Some(&shown)
                            || view.resolve_media(&shown.reference).as_ref() != Some(&destination)
                        {
                            cx.notify();
                            return;
                        }
                        match &destination {
                            MediaSource::Url(url) => cx.open_url(url),
                            MediaSource::Path(path) => cx.open_with_system(path),
                        }
                    },
                ))
            });
        let title = div()
            .min_w_0()
            .flex_1()
            .debug_selector(|| "media-title".into())
            .when(media.kind == Kind::File, |title| title.truncate())
            .child(label.clone());
        let header = row
            .child(icons::icon(symbol))
            .child(title)
            .children(open)
            .children(editing.then_some(edit));
        let frame = match media.kind {
            Kind::Image => frame
                .child(image_preview(source, cx))
                .child(header.text_color(cx.theme().muted_foreground)),
            Kind::File => frame.child(panel(cx).child(header)),
        };
        Some(frame.into_any_element())
    }
}

fn panel(cx: &App) -> Div {
    div()
        .flex()
        .gap_2()
        .p_2()
        .rounded_md()
        .border_1()
        .border_color(cx.theme().border)
        .bg(cx.theme().muted)
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
}

fn control(id: &'static str, title: &'static str) -> Button {
    Button::new(id)
        .ghost()
        .small()
        .label(title)
        .debug_selector(move || id.to_owned())
}

fn image_preview(source: Option<MediaSource>, cx: &App) -> Div {
    let (fill, ink) = (cx.theme().muted, cx.theme().muted_foreground);
    let message = move |text: SharedString| {
        div()
            .p_3()
            .w_full()
            .rounded_md()
            .bg(fill)
            .text_color(ink)
            .child(text)
    };
    let Some(source) = source else {
        return message("Image unavailable".into());
    };
    let source = match source {
        MediaSource::Url(url) => ImageSource::from(url),
        MediaSource::Path(path) => ImageSource::from(path),
    };
    let preview = img(source)
        .id("media-preview")
        .max_w_full()
        .max_h(px(420.))
        .debug_selector(|| "media-preview".to_owned())
        .with_loading(move || message("Loading image…".into()).into_any_element())
        .with_fallback(move || message("Image unavailable".into()).into_any_element());
    div().w_full().flex().items_start().child(preview)
}

fn web_address(address: &str) -> bool {
    let Some((scheme, authority)) = address.split_once("://") else {
        return false;
    };
    (scheme.eq_ignore_ascii_case("https") || scheme.eq_ignore_ascii_case("http"))
        && !authority.starts_with(['/', '\\'])
        && !address
            .chars()
            .any(|c| c.is_ascii_control() || c.is_ascii_whitespace())
        && url::Url::parse(address).is_ok_and(|url| url.has_host())
}
