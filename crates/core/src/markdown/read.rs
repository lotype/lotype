use std::collections::{BTreeMap, HashMap};

use pulldown_cmark::{BlockQuoteKind, CodeBlockKind, Event, Options, Parser, Tag, TagEnd};

use super::PAGE_BREAK;
use crate::{BlockId, Document, MAX_DEPTH, Result, Span, Value};

/// A block to make.
struct Node {
    block_type: &'static str,
    attrs: Vec<(&'static str, Value)>,
    spans: Vec<Span>,
    children: Vec<Node>,
}

impl Node {
    fn new(block_type: &'static str) -> Self {
        Self {
            block_type,
            attrs: Vec::new(),
            spans: Vec::new(),
            children: Vec::new(),
        }
    }

    fn with(mut self, key: &'static str, value: impl Into<Value>) -> Self {
        self.attrs.push((key, value.into()));
        self
    }
}

/// What holds blocks in Markdown.
enum Container {
    Root,
    Quote(Option<BlockQuoteKind>),
    /// The number the next item starts at, for a numbered list.
    List(Option<i64>),
    /// Whether the item is a task, and done.
    Item(Option<bool>),
}

/// The block whose text is being read.
struct Leaf {
    node: Node,
    /// Text read so far that did not come from an image.
    plain: bool,
    image: Option<String>,
}

#[derive(Default)]
struct Reader {
    stack: Vec<(Container, Vec<Node>)>,
    leaf: Option<Leaf>,
    /// The values of each mark in [`MARKS`] that is open, innermost last.
    marks: [Vec<Value>; 4],
    /// Which marks were opened, in order, to close them in turn.
    opened: Vec<usize>,
    /// Inside an image, where text is its description, and whether it has any.
    described: Option<bool>,
}

/// The marks Markdown has signs for. Code is not among them: it does not nest.
const MARKS: [&str; 4] = ["bold", "italic", "strike", "link"];
const BOLD: usize = 0;
const ITALIC: usize = 1;
const STRIKE: usize = 2;
const LINK: usize = 3;

impl Reader {
    fn open(&mut self, node: Node) {
        self.close();
        let (plain, image) = (false, None);
        self.leaf = Some(Leaf { node, plain, image });
    }

    /// Ends the block being read. Text in a list item needs no paragraph
    /// around it in Markdown, so one can also end where no tag says so.
    fn close(&mut self) {
        let Some(leaf) = self.leaf.take() else {
            return;
        };
        let mut node = leaf.node;
        if let Some(url) = leaf.image.filter(|_| !leaf.plain) {
            // An image on its own is a block. The text was its description.
            let caption: String = node.spans.drain(..).map(|span| span.text).collect();
            node = Node::new("image").with("url", url.clone());
            if !caption.is_empty() && caption != url {
                node.attrs.push(("caption", caption.into()));
            }
        }
        self.push(node);
    }

    fn push(&mut self, node: Node) {
        if let Some((_, nodes)) = self.stack.last_mut() {
            nodes.push(node);
        }
    }

    fn text(&mut self, text: &str, code: bool) {
        if text.is_empty() {
            return;
        }
        if self.leaf.is_none() {
            self.open(Node::new("paragraph"));
        }
        // Not from every open mark: emphasis can nest as deep as the text is long.
        let mut marks: BTreeMap<String, Value> = BTreeMap::new();
        for (name, values) in MARKS.iter().zip(&self.marks) {
            if let Some(value) = values.last() {
                marks.insert((*name).to_owned(), value.clone());
            }
        }
        if let Some(described) = &mut self.described {
            *described = true;
        }
        if code {
            marks.insert("code".to_owned(), true.into());
        }
        let Some(leaf) = &mut self.leaf else { return };
        match leaf.node.spans.last_mut() {
            Some(last) if last.marks == marks => last.text.push_str(text),
            _ => leaf.node.spans.push(Span {
                text: text.to_owned(),
                marks,
            }),
        }
    }

    /// Text that is not the description of an image.
    fn plain(&mut self, text: &str, code: bool) {
        self.text(text, code);
        if let Some(leaf) = &mut self.leaf {
            leaf.plain |= self.described.is_none();
        }
    }

    fn mark(&mut self, mark: usize, value: impl Into<Value>) {
        self.marks[mark].push(value.into());
        self.opened.push(mark);
    }

    fn unmark(&mut self) -> Option<Value> {
        self.marks[self.opened.pop()?].pop()
    }

    fn start(&mut self, tag: Tag) {
        match tag {
            Tag::Paragraph => self.open(Node::new("paragraph")),
            Tag::Heading { level, .. } => {
                self.open(Node::new("heading").with("level", level as i64));
            }
            Tag::CodeBlock(kind) => {
                let mut node = Node::new("code_block");
                if let CodeBlockKind::Fenced(info) = kind
                    && let Some(language) = info.split_whitespace().next()
                {
                    node.attrs.push(("language", language.into()));
                }
                self.open(node);
            }
            Tag::HtmlBlock => self.open(Node::new("paragraph")),
            Tag::BlockQuote(kind) => self.enter(Container::Quote(kind)),
            Tag::List(start) => {
                let start = start.map(|start| i64::try_from(start).unwrap_or(1));
                self.enter(Container::List(start));
            }
            Tag::Item => self.enter(Container::Item(None)),
            Tag::Emphasis => self.mark(ITALIC, true),
            Tag::Strong => self.mark(BOLD, true),
            Tag::Strikethrough => self.mark(STRIKE, true),
            Tag::Link { dest_url, .. } => self.mark(LINK, dest_url.as_ref()),
            Tag::Image { dest_url, .. } => {
                if self.leaf.is_none() {
                    self.open(Node::new("paragraph"));
                }
                if let Some(leaf) = &mut self.leaf {
                    // A second image makes neither of them a block.
                    leaf.plain |= leaf.image.is_some();
                    leaf.image = Some(dest_url.to_string());
                }
                self.mark(LINK, dest_url.as_ref());
                self.described = Some(false);
            }
            _ => {}
        }
    }

    fn enter(&mut self, container: Container) {
        self.close();
        self.stack.push((container, Vec::new()));
    }

    fn end(&mut self, tag: TagEnd) {
        match tag {
            TagEnd::Paragraph | TagEnd::Heading(_) => self.close(),
            TagEnd::CodeBlock => {
                if let Some(leaf) = &mut self.leaf
                    && let Some(span) = leaf.node.spans.last_mut()
                    && span.text.ends_with('\n')
                {
                    span.text.pop();
                }
                self.close();
            }
            TagEnd::HtmlBlock => {
                if let Some(leaf) = &mut self.leaf {
                    let html: String = leaf.node.spans.iter().map(|s| s.text.as_str()).collect();
                    if html.trim() == PAGE_BREAK {
                        leaf.node = Node::new("page_break");
                    } else if let Some(span) = leaf.node.spans.last_mut() {
                        span.text.truncate(span.text.trim_end().len());
                    }
                }
                self.close();
            }
            TagEnd::BlockQuote(_) | TagEnd::List(_) | TagEnd::Item => self.leave(),
            TagEnd::Emphasis | TagEnd::Strong | TagEnd::Strikethrough | TagEnd::Link => {
                self.unmark();
            }
            TagEnd::Image => {
                // An image without a description still shows as its address.
                if let Some(Value::Str(url)) = self.unmark()
                    && self.described == Some(false)
                {
                    self.mark(LINK, url.as_str());
                    self.text(&url, false);
                    self.unmark();
                }
                self.described = None;
            }
            _ => {}
        }
    }

    fn leave(&mut self) {
        self.close();
        let Some((container, mut nodes)) = self.stack.pop() else {
            return;
        };
        // The first paragraph is the text of a list item or a quote.
        let mut lead = |block_type| {
            let mut node = Node::new(block_type);
            if nodes
                .first()
                .is_some_and(|first| first.block_type == "paragraph")
            {
                node.spans = nodes.remove(0).spans;
            }
            node.children = std::mem::take(&mut nodes);
            node
        };
        match container {
            Container::Root => self.stack.push((Container::Root, nodes)),
            Container::List(_) => {
                if let Some((_, parent)) = self.stack.last_mut() {
                    parent.extend(nodes);
                }
            }
            Container::Quote(None) => {
                let node = lead("quote");
                self.push(node);
            }
            Container::Quote(Some(kind)) => {
                let kind = match kind {
                    BlockQuoteKind::Note => "note",
                    BlockQuoteKind::Tip => "tip",
                    BlockQuoteKind::Important => "important",
                    BlockQuoteKind::Warning => "warning",
                    BlockQuoteKind::Caution => "caution",
                };
                let mut node = Node::new("alert").with("kind", kind);
                node.children = nodes;
                self.push(node);
            }
            Container::Item(checked) => {
                let number = match self.stack.last_mut() {
                    Some((Container::List(Some(number)), _)) => Some(std::mem::replace(number, 1)),
                    _ => None,
                };
                let node = match (checked, number) {
                    (Some(checked), _) => lead("check_list_item").with("checked", checked),
                    (None, Some(1)) => lead("numbered_list_item"),
                    (None, Some(start)) => lead("numbered_list_item").with("start", start),
                    (None, None) => lead("bullet_list_item"),
                };
                self.push(node);
            }
        }
    }

    fn read(mut self, markdown: &str) -> Vec<Node> {
        let options = Options::ENABLE_GFM
            | Options::ENABLE_STRIKETHROUGH
            | Options::ENABLE_TASKLISTS
            | Options::ENABLE_MATH;
        self.stack.push((Container::Root, Vec::new()));
        for event in Parser::new_ext(markdown, options) {
            match event {
                Event::Start(tag) => self.start(tag),
                Event::End(tag) => self.end(tag),
                Event::Text(text) | Event::Html(text) | Event::InlineHtml(text) => {
                    self.plain(&text, false);
                }
                Event::Code(text) => self.plain(&text, true),
                Event::InlineMath(text) => self.plain(&format!("${text}$"), false),
                Event::DisplayMath(text) => self.plain(&format!("$${text}$$"), false),
                Event::FootnoteReference(label) => self.plain(&format!("[^{label}]"), false),
                Event::SoftBreak => self.plain(" ", false),
                Event::HardBreak => self.plain("\n", false),
                Event::Rule => {
                    self.close();
                    self.push(Node::new("divider"));
                }
                Event::TaskListMarker(done) => {
                    if let Some((Container::Item(checked), _)) = self.stack.last_mut() {
                        *checked = Some(done);
                    }
                }
            }
        }
        self.close();
        self.stack.pop().map_or_else(Vec::new, |(_, nodes)| nodes)
    }
}

impl Document {
    /// Inserts the blocks that `markdown` describes among the children of
    /// `parent`, from `index` on. Returns those it made at that level.
    ///
    /// Markdown is CommonMark with GitHub's alerts, task lists and
    /// strikethrough. What the schema lacks is kept as plainly as it can be:
    /// a block of a missing type becomes a paragraph, and a missing mark is
    /// left off. Blocks nested deeper than the document allows, or under a
    /// block that takes no children, follow that block instead.
    pub fn insert_markdown(
        &mut self,
        parent: Option<BlockId>,
        index: usize,
        markdown: &str,
    ) -> Result<Vec<BlockId>> {
        let top = parent;
        let depth = top.map_or(Ok(0), |parent| self.depth(parent))?;
        let mut next = HashMap::from([(top, index)]);
        let mut made = Vec::new();
        // In reading order: a block, what is in it, then the block after it.
        let nodes = Reader::default().read(markdown);
        let mut stack: Vec<_> = nodes.into_iter().rev().map(|n| (top, depth, n)).collect();
        while let Some((parent, depth, mut node)) = stack.pop() {
            if self.schema().block_spec(node.block_type).is_none() {
                if node.block_type == "image" {
                    node.spans = image_link(&node.attrs);
                }
                // Without its type, its attributes mean nothing.
                (node.block_type, node.attrs) = ("paragraph", Vec::new());
                if node.spans.is_empty() {
                    // It says nothing itself, so what is in it takes its place.
                    let children = node.children.into_iter().rev();
                    stack.extend(children.map(|child| (parent, depth, child)));
                    continue;
                }
            }
            let holds_text = self.holds_text(node.block_type)?;
            if !holds_text && !node.spans.is_empty() {
                // The type holds no text here, so its text goes first inside it.
                let mut text = Node::new("paragraph");
                text.spans = std::mem::take(&mut node.spans);
                node.children.insert(0, text);
            }
            let index = next.entry(parent).or_insert(0);
            let block = self.insert_block(parent, *index, node.block_type)?;
            *index += 1;
            if parent == top {
                made.push(block);
            }
            for (key, value) in node.attrs {
                self.set_attr(block, key, value)?;
            }
            if holds_text {
                for span in &mut node.spans {
                    let fits = |name: &String, value: &mut Value| {
                        let spec = self.mark_spec(name);
                        spec.is_ok_and(|spec| spec.check_value(value).is_ok())
                    };
                    span.marks.retain(fits);
                }
                self.insert_spans(block, 0, &node.spans)?;
            }
            let nests = self.spec(block)?.allows_children() && depth + 1 < MAX_DEPTH;
            let inside = if nests {
                (Some(block), depth + 1)
            } else {
                (parent, depth)
            };
            let children = node.children.into_iter().rev();
            stack.extend(children.map(|child| (inside.0, inside.1, child)));
        }
        Ok(made)
    }
}

/// An image as text: its description, linked to its address.
fn image_link(attrs: &[(&'static str, Value)]) -> Vec<Span> {
    let text = |wanted| match attrs.iter().find(|(key, _)| *key == wanted) {
        Some((_, Value::Str(text))) => text.clone(),
        _ => String::new(),
    };
    let (url, caption) = (text("url"), text("caption"));
    let marks = BTreeMap::from([("link".to_owned(), Value::from(url.as_str()))]);
    let text = if caption.is_empty() { url } else { caption };
    vec![Span { text, marks }]
}
