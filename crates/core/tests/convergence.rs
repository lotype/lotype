//! Peers that make random edits and exchange them in random order must end
//! up with the same document, the same block ids and a readable tree.

use std::collections::HashSet;

use lotype_core::test_support::{editor, outline};
use lotype_core::{BlockId, Document, Editor, Pos, Result, Schema, Selection, blocks, p};
use proptest::prelude::*;

const PEERS: usize = 3;
const TEXTS: [&str; 4] = ["a", "é", " wörd ", "🙂"];

#[derive(Debug, Clone)]
enum Edit {
    InsertBlock {
        parent: usize,
        index: usize,
    },
    RemoveBlock {
        block: usize,
    },
    MoveBlock {
        block: usize,
        parent: usize,
        index: usize,
    },
    InsertText {
        block: usize,
        at: usize,
        text: usize,
    },
    DeleteText {
        block: usize,
        at: usize,
        len: usize,
    },
    Mark {
        block: usize,
        at: usize,
        len: usize,
    },
    Split {
        block: usize,
        at: usize,
    },
    Merge {
        block: usize,
    },
    SetAttr {
        block: usize,
        value: i64,
    },
    /// An edit that changes the document and then fails.
    Fail {
        block: usize,
    },
    InsertTable {
        index: usize,
    },
    /// Something done to the `table`-th table, with numbers to pick by.
    Table {
        table: usize,
        what: usize,
        a: usize,
        b: usize,
    },
}

#[derive(Debug, Clone)]
enum Step {
    Edit(usize, Edit),
    Undo(usize),
    Redo(usize),
    Send { from: usize, to: usize },
}

fn block_edit() -> impl Strategy<Value = Edit> {
    let n = 0..8usize;
    prop_oneof![
        (n.clone(), n.clone()).prop_map(|(parent, index)| Edit::InsertBlock { parent, index }),
        n.clone().prop_map(|block| Edit::RemoveBlock { block }),
        (n.clone(), n.clone(), n.clone()).prop_map(|(block, parent, index)| Edit::MoveBlock {
            block,
            parent,
            index
        }),
        (n.clone(), n.clone(), 0..TEXTS.len()).prop_map(|(block, at, text)| Edit::InsertText {
            block,
            at,
            text
        }),
        (n.clone(), n.clone(), 1..4usize).prop_map(|(block, at, len)| Edit::DeleteText {
            block,
            at,
            len
        }),
        (n.clone(), n.clone(), 1..4usize).prop_map(|(block, at, len)| Edit::Mark {
            block,
            at,
            len
        }),
        (n.clone(), n.clone()).prop_map(|(block, at)| Edit::Split { block, at }),
        n.clone().prop_map(|block| Edit::Merge { block }),
        (n.clone(), 0..3i64).prop_map(|(block, value)| Edit::SetAttr { block, value }),
        n.prop_map(|block| Edit::Fail { block }),
    ]
}

fn edit() -> impl Strategy<Value = Edit> {
    let n = 0..8usize;
    let table = (n.clone(), 0..8usize, n.clone(), n.clone());
    prop_oneof![
        6 => block_edit(),
        1 => n.prop_map(|index| Edit::InsertTable { index }),
        4 => table.prop_map(|(table, what, a, b)| Edit::Table { table, what, a, b }),
    ]
}

fn step() -> impl Strategy<Value = Step> {
    let peer = 0..PEERS;
    prop_oneof![
        6 => (peer.clone(), edit()).prop_map(|(peer, edit)| Step::Edit(peer, edit)),
        1 => peer.clone().prop_map(Step::Undo),
        1 => peer.clone().prop_map(Step::Redo),
        3 => (peer.clone(), peer).prop_map(|(from, to)| Step::Send { from, to }),
    ]
}

/// The byte offset of the `at`-th character boundary, wrapping around.
fn boundary(text: &str, at: usize) -> usize {
    let boundaries: Vec<usize> = text
        .char_indices()
        .map(|(i, _)| i)
        .chain([text.len()])
        .collect();
    boundaries[at % boundaries.len()]
}

fn apply(doc: &mut Document, edit: &Edit) -> Result<()> {
    let all = doc.blocks();
    let pick = |index: usize| all.get(index % (all.len() + 1)).copied();
    let text_of = |doc: &Document, block| doc.text(block).unwrap_or_default();
    match *edit {
        Edit::InsertBlock { parent, index } => {
            let parent = pick(parent);
            let index = index % (doc.children(parent)?.len() + 1);
            doc.insert_block(parent, index, "paragraph")?;
        }
        Edit::RemoveBlock { block } => {
            if let Some(block) = pick(block) {
                doc.remove_block(block)?;
            }
        }
        Edit::MoveBlock {
            block,
            parent,
            index,
        } => {
            if let Some(block) = pick(block) {
                let parent = pick(parent);
                let index = index % (doc.children(parent)?.len() + 1);
                doc.move_block(block, parent, index)?;
            }
        }
        Edit::InsertText { block, at, text } => {
            if let Some(block) = pick(block) {
                let at = boundary(&text_of(doc, block), at);
                doc.insert_text(block, at, TEXTS[text])?;
            }
        }
        Edit::DeleteText { block, at, len } => {
            if let Some(block) = pick(block) {
                let text = text_of(doc, block);
                let (start, end) = (boundary(&text, at), boundary(&text, at + len));
                doc.delete_text(block, start.min(end)..start.max(end))?;
            }
        }
        Edit::Mark { block, at, len } => {
            if let Some(block) = pick(block) {
                let text = text_of(doc, block);
                let (start, end) = (boundary(&text, at), boundary(&text, at + len));
                doc.mark(block, start.min(end)..start.max(end), "bold", true)?;
            }
        }
        Edit::Split { block, at } => {
            if let Some(block) = pick(block) {
                let at = boundary(&text_of(doc, block), at);
                doc.split_block(Pos::new(block, at))?;
            }
        }
        Edit::Merge { block } => {
            if let Some(block) = pick(block) {
                doc.merge_backward(block)?;
            }
        }
        Edit::SetAttr { block, value } => {
            if let Some(block) = pick(block) {
                doc.set_attr(block, "level", value)?;
            }
        }
        Edit::InsertTable { index } => {
            let index = index % (doc.children(None)?.len() + 1);
            doc.insert_table(None, index, 1 + index % 2, 2)?;
        }
        Edit::Table { table, what, a, b } => {
            let is_table = |block: &BlockId| doc.columns(*block).is_ok();
            let tables: Vec<BlockId> = all.iter().copied().filter(is_table).collect();
            let Some(table) = tables.get(table % (tables.len() + 1)).copied() else {
                return Ok(());
            };
            let (rows, columns) = (doc.children(Some(table))?, doc.columns(table)?);
            let row = rows.get(a % (rows.len() + 1)).copied();
            let column = columns.get(b % (columns.len() + 1)).copied();
            match (what, row, column) {
                (0, _, _) => drop(doc.insert_row(table, a % (rows.len() + 1))?),
                (1, _, _) => drop(doc.insert_column(table, b % (columns.len() + 1))?),
                (2, _, Some(column)) => doc.remove_column(table, column)?,
                (3, _, Some(column)) => doc.move_column(table, column, a % columns.len())?,
                (4, Some(row), _) => doc.move_block(row, Some(table), b % rows.len())?,
                (5, Some(row), Some(column)) => {
                    let cell = doc.cell(row, column)?;
                    let at = boundary(&text_of(doc, cell), a);
                    doc.insert_text(cell, at, TEXTS[b % TEXTS.len()])?;
                }
                (6, Some(row), Some(column)) => {
                    let cell = doc.cell(row, column)?;
                    let text = text_of(doc, cell);
                    let (start, end) = (boundary(&text, a), boundary(&text, a + 1 + b));
                    doc.delete_text(cell, start.min(end)..start.max(end))?;
                    doc.mark(cell, 0..start.min(end), "bold", true)?;
                }
                (_, Some(row), _) => doc.remove_block(row)?,
                _ => {}
            }
        }
        Edit::Fail { block } => {
            if let Some(block) = pick(block) {
                doc.remove_block(block)?;
            }
            doc.insert_block(None, 0, "paragraph")?;
            doc.insert_block(None, 0, "no such type")?;
        }
    }
    Ok(())
}

fn send(peers: &mut [Editor], from: usize, to: usize) {
    if from != to {
        let since = peers[to].document().version();
        let update = peers[from].document().export_updates(&since).unwrap();
        peers[to].import(&update).unwrap();
    }
}

fn check(editor: &Editor) {
    let doc = editor.document();
    let blocks = doc.blocks();
    let unique: HashSet<_> = blocks.iter().collect();
    assert_eq!(unique.len(), blocks.len(), "a block id is used twice");
    for block in &blocks {
        assert!(doc.contains(*block));
        doc.parent(*block).unwrap();
        doc.path(*block).unwrap();
        doc.attrs(*block).unwrap();
    }
    if let Some(selection) = editor.selection() {
        doc.check_selection(selection).unwrap();
    }
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 64, ..ProptestConfig::default() })]

    #[test]
    fn peers_converge(steps in prop::collection::vec(step(), 1..60)) {
        let first = editor(blocks![p!["one<a>"], p!["two", p!["child"]], p!["three"]]);
        let snapshot = first.document().export_snapshot().unwrap();
        let mut peers = vec![first];
        for _ in 1..PEERS {
            let mut doc = Document::new(Schema::builtin());
            doc.import(&snapshot).unwrap();
            let caret = Selection::caret(Pos::new(doc.blocks()[0], 0));
            let mut editor = Editor::new(doc);
            editor.set_selection(Some(caret)).unwrap();
            peers.push(editor);
        }

        for step in &steps {
            match step {
                Step::Edit(peer, edit) => {
                    // An edit that does not fit the document fails and is rolled back.
                    let _ = peers[*peer].apply(|doc, selection| {
                        apply(doc, edit)?;
                        Ok(selection.cloned().filter(|s| doc.check_selection(s).is_ok()))
                    });
                }
                // Loro cannot undo every step that remote changes cut across.
                Step::Undo(peer) => drop(peers[*peer].undo()),
                Step::Redo(peer) => drop(peers[*peer].redo()),
                Step::Send { from, to } => send(&mut peers, *from, *to),
            }
            peers.iter().for_each(check);
        }

        for _ in 0..2 {
            for from in 0..PEERS {
                for to in 0..PEERS {
                    send(&mut peers, from, to);
                }
            }
        }
        let expected = outline(peers[0].document(), None);
        for peer in &peers {
            check(peer);
            prop_assert_eq!(outline(peer.document(), None), expected.clone());
            prop_assert_eq!(peer.document().blocks(), peers[0].document().blocks());
            prop_assert_eq!(peer.document().version(), peers[0].document().version());
        }
    }
}
