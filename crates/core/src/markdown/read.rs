use std::borrow::Cow;
use std::collections::{BTreeMap, HashMap};

use pulldown_cmark::{BlockQuoteKind, CodeBlockKind, Event, Options, Parser, Tag, TagEnd};

mod media;

use self::media::{Description, Media};
use super::PAGE_BREAK;
use crate::{
    AlertKind, BlockId, Document, MAX_DEPTH, MAX_HEADING_LEVEL, Result, Span, Value, attr, block,
};

/// A block to make.
pub(super) struct Node {
    pub(super) block_type: Cow<'static, str>,
    pub(super) attrs: Vec<(Cow<'static, str>, Value)>,
    pub(super) spans: Vec<Span>,
    pub(super) children: Vec<Node>,
    /// What follows the signs that open a fenced block, where it is one.
    pub(super) info: Option<String>,
}

impl Node {
    fn new(block_type: &'static str) -> Self {
        Self {
            block_type: block_type.into(),
            attrs: Vec::new(),
            spans: Vec::new(),
            children: Vec::new(),
            info: None,
        }
    }

    fn with(mut self, key: &'static str, value: impl Into<Value>) -> Self {
        self.attrs.push((key.into(), value.into()));
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
    Table,
    /// A row of a table, the one with its headings included.
    Row,
}

/// The block whose text is being read.
struct Leaf {
    node: Node,
    /// Text read so far outside a single image or file link.
    plain: bool,
    media: Option<Media>,
}

#[derive(Default)]
pub(super) struct Reader {
    stack: Vec<(Container, Vec<Node>)>,
    leaf: Option<Leaf>,
    /// The values of each mark in [`MARKS`] that is open, innermost last.
    marks: [Vec<Value>; 4],
    /// Which marks were opened, in order, to close them in turn.
    opened: Vec<usize>,
    described: Option<Description>,
    in_file_link: bool,
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
        self.leaf = Some(Leaf {
            node,
            plain: false,
            media: None,
        });
    }

    /// Ends the block being read. Text in a list item needs no paragraph
    /// around it in Markdown, so one can also end where no tag says so.
    fn close(&mut self) {
        if let Some(leaf) = self.leaf.take() {
            self.push(leaf.into_node());
        }
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
            self.open(Node::new(block::PARAGRAPH));
        }
        // Not from every open mark: emphasis can nest as deep as the text is long.
        let mut marks: BTreeMap<String, Value> = BTreeMap::new();
        for (name, values) in MARKS.iter().zip(&self.marks) {
            if let Some(value) = values.last() {
                marks.insert((*name).to_owned(), value.clone());
            }
        }
        if let Some(described) = &mut self.described {
            described.has_text = true;
        }
        if code {
            marks.insert("code".to_owned(), true.into());
        }
        let Some(leaf) = &mut self.leaf else { return };
        match leaf.node.spans.last_mut() {
            Some(last) if last.marks == marks => last.text.push_str(text),
            _ => leaf.node.spans.push(Span::new(text, marks)),
        }
    }

    /// Text that can keep a media item from becoming a block.
    fn plain(&mut self, text: &str, code: bool) {
        self.text(text, code);
        if let Some(leaf) = &mut self.leaf {
            leaf.plain |= !text.is_empty() && self.described.is_none() && !self.in_file_link;
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
            Tag::Paragraph => self.open(Node::new(block::PARAGRAPH)),
            // A heading goes in no list item, quote or alert: there it is
            // read as a paragraph, the text of a list item where it leads.
            Tag::Heading { .. } if self.nested() => self.open(Node::new(block::PARAGRAPH)),
            Tag::Heading { level, .. } => {
                let level = (level as i64).min(MAX_HEADING_LEVEL);
                self.open(Node::new(block::HEADING).with(attr::LEVEL, level));
            }
            Tag::CodeBlock(kind) => {
                let mut node = Node::new(block::CODE_BLOCK);
                if let CodeBlockKind::Fenced(info) = kind {
                    if let Some(language) = info.split_whitespace().next() {
                        node.attrs.push(("language".into(), language.into()));
                    }
                    node.info = Some(info.to_string());
                }
                self.open(node);
            }
            Tag::HtmlBlock => self.open(Node::new(block::PARAGRAPH)),
            Tag::BlockQuote(kind) => self.enter(Container::Quote(kind)),
            Tag::List(start) => {
                let start = start.map(|start| i64::try_from(start).unwrap_or(1));
                self.enter(Container::List(start));
            }
            Tag::Item => self.enter(Container::Item(None)),
            Tag::Table(_) => self.enter(Container::Table),
            Tag::TableHead | Tag::TableRow => self.enter(Container::Row),
            Tag::TableCell => self.open(Node::new(block::TABLE_CELL)),
            Tag::Emphasis => self.mark(ITALIC, true),
            Tag::Strong => self.mark(BOLD, true),
            Tag::Strikethrough => self.mark(STRIKE, true),
            Tag::Link {
                dest_url, title, ..
            } => self.start_link(&dest_url, &title),
            Tag::Image { dest_url, .. } => self.start_image(&dest_url),
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
                        leaf.node = Node::new(block::PAGE_BREAK);
                    } else if let Some(span) = leaf.node.spans.last_mut() {
                        span.text.truncate(span.text.trim_end().len());
                    }
                }
                self.close();
            }
            TagEnd::BlockQuote(_) | TagEnd::List(_) | TagEnd::Item => self.leave(),
            TagEnd::Table | TagEnd::TableHead | TagEnd::TableRow => self.leave(),
            TagEnd::TableCell => self.close(),
            TagEnd::Emphasis | TagEnd::Strong | TagEnd::Strikethrough => {
                self.unmark();
            }
            TagEnd::Link => self.end_link(),
            TagEnd::Image => self.end_image(),
            _ => {}
        }
    }

    /// Whether what is read now goes in a list item, a quote or an alert.
    fn nested(&self) -> bool {
        let nests = |(container, _): &(Container, _)| !matches!(container, Container::Root);
        self.stack.iter().any(nests)
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
                .is_some_and(|first| first.block_type == block::PARAGRAPH)
            {
                node.spans = nodes.remove(0).spans;
            }
            node.children = std::mem::take(&mut nodes);
            node
        };
        match container {
            Container::Root => self.stack.push((Container::Root, nodes)),
            Container::Table | Container::Row => {
                let row = matches!(container, Container::Row);
                let mut node = Node::new(if row { block::TABLE_ROW } else { block::TABLE });
                node.children = nodes;
                self.push(node);
            }
            Container::List(_) => {
                if let Some((_, parent)) = self.stack.last_mut() {
                    parent.extend(nodes);
                }
            }
            Container::Quote(None) => {
                let node = lead(block::QUOTE);
                self.push(node);
            }
            Container::Quote(Some(kind)) => {
                let kind = match kind {
                    BlockQuoteKind::Note => AlertKind::Note,
                    BlockQuoteKind::Tip => AlertKind::Tip,
                    BlockQuoteKind::Important => AlertKind::Important,
                    BlockQuoteKind::Warning => AlertKind::Warning,
                    BlockQuoteKind::Caution => AlertKind::Caution,
                };
                let mut node = Node::new(block::ALERT).with(attr::KIND, kind.name());
                node.children = nodes;
                self.push(node);
            }
            Container::Item(checked) => {
                let number = match self.stack.last_mut() {
                    Some((Container::List(Some(number)), _)) => Some(std::mem::replace(number, 1)),
                    _ => None,
                };
                let node = match (checked, number) {
                    (Some(checked), _) => lead(block::CHECK_LIST_ITEM).with(attr::CHECKED, checked),
                    (None, Some(1)) => lead(block::NUMBERED_LIST_ITEM),
                    (None, Some(start)) => lead(block::NUMBERED_LIST_ITEM).with(attr::START, start),
                    (None, None) => lead(block::BULLET_LIST_ITEM),
                };
                self.push(node);
            }
        }
    }

    pub(super) fn read(mut self, markdown: &str) -> Vec<Node> {
        let options = Options::ENABLE_GFM
            | Options::ENABLE_STRIKETHROUGH
            | Options::ENABLE_TASKLISTS
            | Options::ENABLE_TABLES
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
                    self.push(Node::new(block::DIVIDER));
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
        self.batch(|doc| doc.read_markdown(parent, index, markdown))
    }

    fn read_markdown(
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
            self.unfence(&mut node);
            // A table that cannot be made, for want of room below it or of
            // a type in the schema, is read as what is in it, like any block
            // of a type the schema lacks. So are its rows and cells then:
            // they only get here when their table was taken apart.
            let lacks = |name: &str| self.schema().block_spec(name).is_none();
            let unmade = node.block_type == block::TABLE
                && (depth + 2 > MAX_DEPTH || lacks(block::TABLE_ROW) || lacks(block::TABLE_CELL));
            let part = matches!(&*node.block_type, block::TABLE_ROW | block::TABLE_CELL);
            // Nor is a heading in another block, where none goes.
            let top_level = |name: &str| {
                let spec = self.schema().block_spec(name);
                spec.is_some_and(|spec| spec.is_top_level())
            };
            let nested = parent.is_some() && top_level(&node.block_type);
            if unmade || part || nested || lacks(&node.block_type) {
                node.keep_attachment_as_link();
                // Without its type, its attributes mean nothing.
                (node.block_type, node.attrs) = (block::PARAGRAPH.into(), Vec::new());
                if node.spans.is_empty() {
                    // It says nothing itself, so what is in it takes its place.
                    let children = node.children.into_iter().rev();
                    stack.extend(children.map(|child| (parent, depth, child)));
                    continue;
                }
            }
            if node.block_type == block::TABLE {
                let index = next.entry(parent).or_insert(0);
                let table = self.insert_rows(parent, *index, node.children)?;
                *index += 1;
                made.extend((parent == top).then_some(table));
                continue;
            }
            let holds_text = self.holds_text(&node.block_type)?;
            if !holds_text && !node.spans.is_empty() {
                // The type holds no text here, so its text goes first inside it.
                let mut text = Node::new(block::PARAGRAPH);
                text.spans = std::mem::take(&mut node.spans);
                node.children.insert(0, text);
            }
            let index = next.entry(parent).or_insert(0);
            let block = self.insert_block(parent, *index, &node.block_type)?;
            *index += 1;
            if parent == top {
                made.push(block);
            }
            for (key, value) in node.attrs {
                self.set_attr(block, &key, value)?;
            }
            if holds_text {
                self.fit_marks(&mut node.spans);
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

impl Document {
    /// Leaves off the marks the schema lacks, or has with another kind of
    /// value, and the atoms of types it lacks.
    pub(crate) fn fit_marks(&self, spans: &mut Vec<Span>) {
        spans.retain(|span| {
            let atom = span.atom.as_ref();
            atom.is_none_or(|atom| self.schema().inline_spec(&atom.name).is_some())
        });
        for span in spans {
            let fits = |name: &String, value: &mut Value| {
                let spec = self.mark_spec(name);
                spec.is_ok_and(|spec| spec.check_value(value).is_ok())
            };
            span.marks.retain(fits);
        }
    }

    /// Inserts a table with these rows of cells, as wide as its widest row.
    fn insert_rows(
        &mut self,
        parent: Option<BlockId>,
        index: usize,
        rows: Vec<Node>,
    ) -> Result<BlockId> {
        let width = rows.iter().map(|row| row.children.len()).max();
        let table = self.insert_table(parent, index, rows.len(), width.unwrap_or(0))?;
        let (blocks, columns) = (self.children(Some(table))?, self.columns(table)?);
        for (row, block) in rows.into_iter().zip(blocks) {
            for (mut cell, column) in row.children.into_iter().zip(&columns) {
                self.fit_marks(&mut cell.spans);
                self.insert_spans(self.cell(block, *column)?, 0, &cell.spans)?;
            }
        }
        Ok(table)
    }
}
