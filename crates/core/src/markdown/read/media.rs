//! Promote a standalone image or marked file link to a document block.

use std::collections::BTreeMap;

use super::super::{FILE_TITLE, UNNAMED_FILE_TITLE};
use super::{LINK, Leaf, Node, Reader};
use crate::{Span, Value, attr, block, mark};

pub(super) enum Media {
    Image { url: String, caption: bool },
    File { url: String, named: bool },
}

/// Tracks text inside an image description, including descriptions of nested images.
pub(super) struct Description {
    pub(super) has_text: bool,
    nested: usize,
    url: String,
}

impl Leaf {
    /// Returns the completed text block or its standalone media block.
    pub(super) fn into_node(self) -> Node {
        let alone = !self.plain && self.node.block_type == block::PARAGRAPH;
        let Some(media) = self.media.filter(|_| alone) else {
            return self.node;
        };
        let (kind, url, has_label, key) = match media {
            Media::Image { url, caption } => (block::IMAGE, url, caption, attr::CAPTION),
            Media::File { url, named } => (block::FILE, url, named, attr::NAME),
        };
        let mut media = Node::new(kind).with(attr::URL, url);
        if has_label {
            let label: String = self
                .node
                .spans
                .iter()
                .map(|span| span.text.as_str())
                .collect();
            if !label.is_empty() {
                media = media.with(key, label);
            }
        }
        if kind == block::FILE {
            // Preserve the original linked text for schemas without files and rich-text fences.
            media.spans = self.node.spans;
        }
        media
    }
}

impl Reader {
    /// Opens a link and remembers blocks that can be read as attachments.
    pub(super) fn start_link(&mut self, url: &str, title: &str) {
        if self.described.is_some() {
            // A link in a description contributes its label to the outer image.
            let image = self.marks[LINK]
                .last()
                .cloned()
                .unwrap_or_else(|| url.into());
            self.mark(LINK, image);
            return;
        }
        self.in_file_link = self.opened.is_empty()
            && self.leaf.as_ref().is_none_or(|leaf| !leaf.heading)
            && matches!(title, FILE_TITLE | UNNAMED_FILE_TITLE);
        if self.in_file_link {
            self.media_candidate(
                Media::File {
                    url: url.to_owned(),
                    named: title == FILE_TITLE,
                },
                false,
            );
        } else if let Some(leaf) = &mut self.leaf {
            leaf.plain = true;
        }
        self.mark(LINK, url);
    }

    /// Closes a link. Links inside image descriptions may be nested in a file link.
    pub(super) fn end_link(&mut self) {
        if self.described.is_none() {
            self.in_file_link = false;
        }
        self.unmark();
    }

    /// Starts reading an image description.
    pub(super) fn start_image(&mut self, url: &str) {
        if let Some(description) = &mut self.described {
            description.nested += 1;
            return;
        }
        // A linked image stays text pointing at the outer link.
        let outer = self.marks[LINK].last().cloned();
        if !self.in_file_link {
            self.media_candidate(
                Media::Image {
                    url: url.to_owned(),
                    caption: false,
                },
                outer.is_some(),
            );
        }
        self.mark(LINK, outer.unwrap_or_else(|| url.into()));
        self.described = Some(Description {
            url: url.to_owned(),
            has_text: false,
            nested: 0,
        });
    }

    /// Closes an image description and supplies link text for an unlabeled inline image.
    pub(super) fn end_image(&mut self) {
        let Some(description) = &mut self.described else {
            return;
        };
        if description.nested > 0 {
            description.nested -= 1;
            return;
        }
        let Some(description) = self.described.take() else {
            return;
        };
        let link = self.unmark();
        let media = self.leaf.as_mut().and_then(|leaf| leaf.media.as_mut());
        if let Some(Media::Image { caption, .. }) = media {
            *caption = description.has_text;
        }
        if let Some(link) = link.filter(|_| !description.has_text) {
            self.mark(LINK, link);
            self.text(&description.url, false);
            self.unmark();
        }
    }

    /// Records a media item and rules out promotion if it shares a paragraph.
    fn media_candidate(&mut self, media: Media, inside_link: bool) {
        if self.leaf.is_none() {
            self.open(Node::new(block::PARAGRAPH));
        }
        if let Some(leaf) = &mut self.leaf {
            leaf.plain |= leaf.media.is_some() || inside_link;
            leaf.media = Some(media);
        }
    }
}

impl Node {
    /// Keeps an attachment as a link or as plain text when its block type is unavailable.
    pub(super) fn keep_attachment_as_link(&mut self) {
        let key = match self.block_type.as_ref() {
            block::IMAGE => attr::CAPTION,
            _ => return,
        };
        let text = |wanted| match self.attrs.iter().find(|(key, _)| *key == wanted) {
            Some((_, Value::Str(text))) => text.as_str(),
            _ => "",
        };
        let (url, label) = (text(attr::URL), text(key));
        let label = if label.is_empty() { url } else { label };
        if !label.is_empty() {
            let mut marks = BTreeMap::new();
            if !url.is_empty() {
                marks.insert(mark::LINK.to_owned(), Value::from(url));
            }
            self.spans = vec![Span::new(label, marks)];
        }
    }
}
