use std::ops::Range;

use crate::editor::BLOCK_STYLE;
use crate::table::{CELL, ROW, TABLE};
use crate::{
    AlertKind, BlockId, Document, Editor, Error, Pos, Result, Role, Selection, Value, attr, block,
    mark,
};

/// An entry of the slash menu: a block to insert. A paragraph is none: it
/// is what typing in an empty block makes already.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlashItem {
    /// A stable name, for a view to pick an icon or a translation by.
    pub key: String,
    /// The title the menu shows, in English.
    pub title: String,
    /// What the block is for, in a few words under the title.
    pub subtext: String,
    /// The group the entry is shown under.
    pub group: String,
    /// Other words that find the item.
    pub aliases: Vec<String>,
    /// The block type the entry makes.
    pub block_type: String,
    /// The attributes the block starts with.
    pub attrs: Vec<(String, Value)>,
}

fn item(
    key: &str,
    title: &str,
    subtext: &str,
    group: &str,
    aliases: &[&str],
    block_type: &str,
) -> SlashItem {
    SlashItem {
        key: key.to_owned(),
        title: title.to_owned(),
        subtext: subtext.to_owned(),
        group: group.to_owned(),
        aliases: aliases.iter().map(|alias| (*alias).to_owned()).collect(),
        block_type: block_type.to_owned(),
        attrs: Vec::new(),
    }
}

impl SlashItem {
    fn with(mut self, key: &str, value: impl Into<Value>) -> Self {
        self.attrs.push((key.to_owned(), value.into()));
        self
    }

    /// Whether `block` is of this entry's type, with its attributes: a
    /// heading of its level, as a peer's level out of range reads.
    pub fn describes(&self, doc: &Document, block: BlockId) -> bool {
        let same = |(key, value): &(String, Value)| match key.as_str() {
            attr::LEVEL => Value::Int(doc.heading_level(block)) == *value,
            _ => doc.attr(block, key).ok().flatten().as_ref() == Some(value),
        };
        doc.block_type(block)
            .is_ok_and(|kind| kind == self.block_type)
            && self.attrs.iter().all(same)
    }
}

/// The built-in entries, for the built-in schema, in menu order, each group
/// together.
pub fn default_slash_items() -> Vec<SlashItem> {
    let heading = |level: i64, subtext: &str| {
        let (key, title, alias) = (
            format!("heading_{level}"),
            format!("Heading {level}"),
            format!("h{level}"),
        );
        let group = if level <= 3 {
            "Headings"
        } else {
            "Subheadings"
        };
        item(
            &key,
            &title,
            subtext,
            group,
            &[&alias, "title"],
            block::HEADING,
        )
        .with(attr::LEVEL, level)
    };
    let basic = |key, title, subtext, aliases, block_type| {
        item(key, title, subtext, "Basic blocks", aliases, block_type)
    };
    vec![
        heading(1, "Top-level heading"),
        heading(2, "Key section heading"),
        heading(3, "Subsection and group heading"),
        heading(4, "Minor subsection heading"),
        heading(5, "Small subsection heading"),
        basic(
            "quote",
            "Quote",
            "Quote or excerpt",
            &["blockquote", "citation"],
            block::QUOTE,
        ),
        basic(
            "toggle_list",
            "Toggle List",
            "List with hideable sub-items",
            &["collapsible", "list"],
            block::TOGGLE_LIST_ITEM,
        ),
        basic(
            "bullet_list",
            "Bullet List",
            "List with unordered items",
            &["ul", "unordered", "list"],
            block::BULLET_LIST_ITEM,
        ),
        basic(
            "numbered_list",
            "Numbered List",
            "List with ordered items",
            &["ol", "ordered", "list"],
            block::NUMBERED_LIST_ITEM,
        ),
        basic(
            "check_list",
            "Check List",
            "List with checkboxes",
            &["todo", "task", "checkbox", "list"],
            block::CHECK_LIST_ITEM,
        )
        .with(attr::CHECKED, false),
        basic(
            "code_block",
            "Code Block",
            "Code in a block of its own",
            &["pre", "code"],
            block::CODE_BLOCK,
        ),
        basic(
            "alert",
            "Alert",
            "Note, tip, important, warning or caution",
            &["callout", "note", "tip", "important", "warning", "caution"],
            block::ALERT,
        )
        .with(attr::KIND, AlertKind::Note.name()),
        basic(
            "divider",
            "Divider",
            "Visually divide blocks",
            &["hr", "rule", "line", "separator"],
            block::DIVIDER,
        ),
        basic(
            "page_break",
            "Page Break",
            "Start a new page when printed",
            &["page"],
            block::PAGE_BREAK,
        ),
        item(
            "table",
            "Table",
            "Table with editable cells",
            "Advanced",
            &["grid", "rows", "columns"],
            block::TABLE,
        ),
        item(
            "image",
            "Image",
            "Image with a caption",
            "Media",
            &["picture", "photo", "media"],
            block::IMAGE,
        ),
        item(
            "file",
            "File",
            "Embedded file",
            "Media",
            &["attachment", "upload", "media"],
            block::FILE,
        ),
    ]
}

/// The types a block with text can be turned into, as a toolbar's select
/// offers them: the paragraph, and the slash menu's headings, quote, lists
/// and code block, without the attributes a new block starts with.
pub fn default_block_types() -> Vec<SlashItem> {
    let keys = [
        "heading_1",
        "heading_2",
        "heading_3",
        "heading_4",
        "heading_5",
        "quote",
        "toggle_list",
        "bullet_list",
        "numbered_list",
        "check_list",
        "code_block",
    ];
    let slash = default_slash_items();
    let found = keys
        .iter()
        .filter_map(|key| slash.iter().find(|item| item.key == *key));
    let paragraph = item(
        "paragraph",
        "Paragraph",
        "Plain text",
        "Basic blocks",
        &["p"],
        block::PARAGRAPH,
    );
    let types = std::iter::once(paragraph).chain(found.cloned());
    types
        .map(|mut item| {
            item.attrs.retain(|(key, _)| key == attr::LEVEL);
            item
        })
        .collect()
}

/// The entries that `query` finds, in their order: those whose title or one
/// of whose aliases contains it, whatever the case.
pub fn filter_slash_items<'a>(items: &'a [SlashItem], query: &str) -> Vec<&'a SlashItem> {
    let query = query.to_lowercase();
    let found = |text: &String| text.to_lowercase().contains(&query);
    let matches = |item: &&SlashItem| found(&item.title) || item.aliases.iter().any(found);
    items.iter().filter(matches).collect()
}

/// A slash and what was typed after it, up to the caret.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlashQuery {
    /// The block the slash is in.
    pub block: BlockId,
    /// From the slash to the caret, in bytes.
    pub range: Range<usize>,
    /// Without the slash.
    pub query: String,
}

impl Editor {
    /// The slash menu's query, when the caret is right after one: a `/` at
    /// the start of a block or after whitespace, with no whitespace typed since.
    pub fn slash_query(&self) -> Option<SlashQuery> {
        let Some(Selection::Text { anchor, head }) = self.selection() else {
            return None;
        };
        let doc = self.document();
        // A slash in code is a slash.
        let code = doc.role(head.block) == Role::Code
            || doc
                .marks_at(*head)
                .is_ok_and(|marks| marks.contains_key(mark::CODE));
        // Nor is there a menu in a cell, which is no block to turn into another.
        let plain = anchor == head && !code && head.block.column().is_none();
        let text = plain.then(|| doc.text(head.block).ok())??;
        let typed = text.get(..head.offset)?;
        let slash = typed.rfind('/')?;
        let query = &typed[slash + 1..];
        let starts = typed[..slash]
            .chars()
            .next_back()
            .is_none_or(char::is_whitespace);
        (starts && !query.contains(char::is_whitespace)).then(|| SlashQuery {
            block: head.block,
            range: slash..head.offset,
            query: query.to_owned(),
        })
    }

    /// Takes the slash query out and inserts the entry's block: in place of
    /// the block the caret is in when that is left empty, and below it
    /// otherwise. The caret goes into the block, or below it when it holds no
    /// text. One undo step.
    pub fn accept_slash_item(&mut self, item: &SlashItem) -> Result<()> {
        if item.block_type == ROW || item.block_type == CELL {
            return Err(Error::TablePart);
        }
        let query = self.slash_query().ok_or(Error::NoSelection)?;
        let block = query.block;
        self.apply(|doc, _| {
            doc.delete_text(block, query.range)?;
            let holds_text = doc.holds_text(&item.block_type)?;
            let (parent, below) = (doc.parent(block)?, doc.index_of(block)? + 1);
            let empty = doc.text_len(block)? == 0 && doc.children(Some(block))?.is_empty();
            if item.block_type == TABLE {
                // Two by two to start with, and typing goes on in the first cell.
                let table = doc.create_table(parent, below, 2, 2)?;
                if empty {
                    doc.remove_block(block)?;
                }
                let (rows, columns) = (doc.children(Some(table))?, doc.columns(table)?);
                let first = doc.cell(rows[0], columns[0])?;
                return Ok(Some(Selection::caret(Pos::new(first, 0))));
            }
            let target = if empty {
                let spec = doc.schema().block_spec(&item.block_type);
                let styled = spec.is_some_and(|spec| spec.takes_style());
                let kept = |key: &String| styled && BLOCK_STYLE.contains(&key.as_str());
                let keys = doc.attrs(block)?.into_keys();
                for key in keys.filter(|key| !kept(key)) {
                    doc.remove_attr(block, &key)?;
                }
                doc.set_block_type(block, &item.block_type)?;
                block
            } else {
                doc.create_block(parent, below, &item.block_type)?
            };
            for (key, value) in &item.attrs {
                doc.set_attr(target, key, value.clone())?;
            }
            // A heading picked in a nest comes out of it where it reads.
            if doc.spec(target)?.is_top_level() {
                doc.lift_to_top(target)?;
            }
            // Typing goes on in the block, or where text can go next to it.
            let caret = if holds_text {
                target
            } else if doc.spec(target)?.allows_children() {
                doc.create_block(Some(target), 0, &doc.text_block()?)?
            } else {
                let below = doc.index_of(target)? + 1;
                doc.create_block(parent, below, &doc.text_block()?)?
            };
            Ok(Some(Selection::caret(Pos::new(caret, 0))))
        })
    }
}
