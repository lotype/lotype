//! Builders and assertions for tests, behind the `test-support` feature.
//!
//! Text may contain `<a>` (selection anchor) and `<b>` (selection head).
//! `<a>` alone is a caret. Everything here panics on invalid input.

use std::collections::BTreeMap;
use std::fmt::Write;

use crate::{BlockId, Document, Pos, Schema, Selection, Span, Value};

const ANCHOR: &str = "<a>";
const HEAD: &str = "<b>";

#[derive(Debug, Clone)]
pub struct BlockNode {
    block_type: String,
    attrs: Vec<(String, Value)>,
    content: Vec<InlineNode>,
    children: Vec<BlockNode>,
}

impl BlockNode {
    pub fn attr(mut self, key: &str, value: impl Into<Value>) -> Self {
        self.attrs.push((key.to_owned(), value.into()));
        self
    }
}

#[derive(Debug, Clone)]
pub enum InlineNode {
    Text(String),
    Marked {
        name: String,
        value: Value,
        content: Vec<InlineNode>,
    },
}

/// What a block builder accepts: text, marked text, or a child block.
#[derive(Debug, Clone)]
pub enum Item {
    Inline(InlineNode),
    Block(BlockNode),
}

impl From<&str> for Item {
    fn from(text: &str) -> Self {
        Self::Inline(InlineNode::Text(text.to_owned()))
    }
}

impl From<InlineNode> for Item {
    fn from(node: InlineNode) -> Self {
        Self::Inline(node)
    }
}

impl From<BlockNode> for Item {
    fn from(node: BlockNode) -> Self {
        Self::Block(node)
    }
}

pub fn block(block_type: &str, items: Vec<Item>) -> BlockNode {
    let mut node = BlockNode {
        block_type: block_type.to_owned(),
        attrs: Vec::new(),
        content: Vec::new(),
        children: Vec::new(),
    };
    for item in items {
        match item {
            Item::Inline(inline) => node.content.push(inline),
            Item::Block(child) => node.children.push(child),
        }
    }
    node
}

pub fn marked(name: &str, value: impl Into<Value>, items: Vec<Item>) -> InlineNode {
    let content = items.into_iter().map(|item| match item {
        Item::Inline(inline) => inline,
        Item::Block(_) => panic!("a mark cannot contain a block"),
    });
    InlineNode::Marked {
        name: name.to_owned(),
        value: value.into(),
        content: content.collect(),
    }
}

/// Top-level blocks of a document.
#[macro_export]
macro_rules! blocks {
    ($($block:expr),* $(,)?) => { ::std::vec![$($block),*] };
}

#[doc(hidden)]
#[macro_export]
macro_rules! __items {
    ($($item:expr),* $(,)?) => { ::std::vec![$($crate::test_support::Item::from($item)),*] };
}

#[macro_export]
macro_rules! p {
    ($($item:tt)*) => { $crate::test_support::block("paragraph", $crate::__items![$($item)*]) };
}

#[macro_export]
macro_rules! h1 {
    ($($item:tt)*) => {
        $crate::test_support::block("heading", $crate::__items![$($item)*]).attr("level", 1)
    };
}

#[macro_export]
macro_rules! bullet {
    ($($item:tt)*) => {
        $crate::test_support::block("bullet_list_item", $crate::__items![$($item)*])
    };
}

#[macro_export]
macro_rules! divider {
    () => {
        $crate::test_support::block("divider", ::std::vec![])
    };
}

#[macro_export]
macro_rules! b {
    ($($item:tt)*) => { $crate::test_support::marked("bold", true, $crate::__items![$($item)*]) };
}

#[macro_export]
macro_rules! i {
    ($($item:tt)*) => { $crate::test_support::marked("italic", true, $crate::__items![$($item)*]) };
}

/// Builds a document with the built-in schema and commits it.
pub fn build(blocks: Vec<BlockNode>) -> (Document, Option<Selection>) {
    let mut doc = Document::new(Schema::builtin());
    let mut markers = Markers::default();
    for (index, node) in blocks.iter().enumerate() {
        insert(&mut doc, None, index, node, &mut markers);
    }
    doc.commit();
    let selection = match (markers.anchor, markers.head) {
        (Some(anchor), Some(head)) => Some(Selection::Text { anchor, head }),
        (Some(anchor), None) => Some(Selection::caret(anchor)),
        (None, Some(_)) => panic!("`<b>` needs an `<a>`"),
        (None, None) => None,
    };
    (doc, selection)
}

/// Compares `doc` with `expected`. The selection is compared only when
/// `expected` contains markers.
#[track_caller]
pub fn assert_document(doc: &Document, selection: Option<&Selection>, expected: Vec<BlockNode>) {
    let (expected_doc, expected_selection) = build(expected);
    let selection = selection.filter(|_| expected_selection.is_some());
    assert_eq!(
        outline(doc, selection),
        outline(&expected_doc, expected_selection.as_ref())
    );
}

/// One line per block, indented by depth: `type attr=value "text"`.
/// Marked text reads `[bold,link=url|text]`.
pub fn outline(doc: &Document, selection: Option<&Selection>) -> String {
    let mut out = String::new();
    for block in doc.blocks() {
        let depth = doc.path(block).unwrap().len() - 1;
        let selected =
            matches!(selection, Some(Selection::Blocks(blocks)) if blocks.contains(&block));
        out.push_str(&"  ".repeat(depth));
        out.push_str(if selected { "* " } else { "" });
        out.push_str(&doc.block_type(block).unwrap());
        for (key, value) in doc.attrs(block).unwrap() {
            write!(out, " {key}={}", display(&value)).unwrap();
        }
        if let Ok(spans) = doc.spans(block) {
            write!(
                out,
                " \"{}\"",
                inline(&spans, &markers_in(block, selection))
            )
            .unwrap();
        }
        out.push('\n');
    }
    out
}

#[derive(Default)]
struct Markers {
    anchor: Option<Pos>,
    head: Option<Pos>,
}

fn insert(
    doc: &mut Document,
    parent: Option<BlockId>,
    index: usize,
    node: &BlockNode,
    markers: &mut Markers,
) {
    let block = doc.insert_block(parent, index, &node.block_type).unwrap();
    for (key, value) in &node.attrs {
        doc.set_attr(block, key, value.clone()).unwrap();
    }
    let mut spans = Vec::new();
    flatten(&node.content, &BTreeMap::new(), block, &mut spans, markers);
    if !spans.is_empty() {
        doc.insert_spans(block, 0, &spans).unwrap();
    }
    for (index, child) in node.children.iter().enumerate() {
        insert(doc, Some(block), index, child, markers);
    }
}

fn flatten(
    content: &[InlineNode],
    marks: &BTreeMap<String, Value>,
    block: BlockId,
    spans: &mut Vec<Span>,
    markers: &mut Markers,
) {
    for node in content {
        match node {
            InlineNode::Text(text) => {
                let offset: usize = spans.iter().map(|span| span.text.len()).sum();
                let text = take_markers(text, Pos::new(block, offset), markers);
                if !text.is_empty() {
                    let marks = marks.clone();
                    spans.push(Span { text, marks });
                }
            }
            InlineNode::Marked {
                name,
                value,
                content,
            } => {
                let mut marks = marks.clone();
                marks.insert(name.clone(), value.clone());
                flatten(content, &marks, block, spans, markers);
            }
        }
    }
}

/// Removes the markers from `text` and records where they were. `start` is where `text` begins.
fn take_markers(text: &str, start: Pos, markers: &mut Markers) -> String {
    let mut clean = String::new();
    let mut rest = text;
    loop {
        let anchor = rest.find(ANCHOR);
        let head = rest.find(HEAD);
        let (at, is_anchor) = match (anchor, head) {
            (Some(a), Some(h)) => (a.min(h), a < h),
            (Some(a), None) => (a, true),
            (None, Some(h)) => (h, false),
            (None, None) => break,
        };
        clean.push_str(&rest[..at]);
        let pos = Pos::new(start.block, start.offset + clean.len());
        let slot = if is_anchor {
            &mut markers.anchor
        } else {
            &mut markers.head
        };
        assert!(slot.replace(pos).is_none(), "duplicate selection marker");
        rest = &rest[at + ANCHOR.len()..];
    }
    clean.push_str(rest);
    clean
}

/// Marker offsets within `block`, sorted by offset.
fn markers_in(block: BlockId, selection: Option<&Selection>) -> Vec<(usize, &'static str)> {
    let Some(Selection::Text { anchor, head }) = selection else {
        return Vec::new();
    };
    let mut markers = vec![(*anchor, ANCHOR)];
    if anchor != head {
        markers.push((*head, HEAD));
    }
    markers.retain(|(pos, _)| pos.block == block);
    markers.sort_by_key(|(pos, _)| pos.offset);
    markers
        .into_iter()
        .map(|(pos, tag)| (pos.offset, tag))
        .collect()
}

fn inline(spans: &[Span], markers: &[(usize, &'static str)]) -> String {
    let mut out = String::new();
    let mut markers = markers.iter().peekable();
    let mut start = 0;
    for span in spans {
        while let Some((_, tag)) = markers.next_if(|(offset, _)| *offset == start) {
            out.push_str(tag);
        }
        let end = start + span.text.len();
        let mut text = String::new();
        let mut written = start;
        while let Some((offset, tag)) = markers.next_if(|(offset, _)| *offset < end) {
            text.push_str(&span.text[written - start..offset - start]);
            text.push_str(tag);
            written = *offset;
        }
        text.push_str(&span.text[written - start..]);
        if span.marks.is_empty() {
            out.push_str(&text);
        } else {
            let marks = span.marks.iter().map(|(name, value)| match value {
                Value::Bool(true) => name.clone(),
                other => format!("{name}={}", display(other)),
            });
            write!(out, "[{}|{text}]", marks.collect::<Vec<_>>().join(",")).unwrap();
        }
        start = end;
    }
    for (_, tag) in markers {
        out.push_str(tag);
    }
    out
}

fn display(value: &Value) -> String {
    match value {
        Value::Bool(b) => b.to_string(),
        Value::Int(i) => i.to_string(),
        Value::Str(s) => s.clone(),
    }
}
