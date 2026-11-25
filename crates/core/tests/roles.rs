//! A schema of its own names gets the editing of the built-in one through
//! the roles of its types.

use lotype_core::test_support::outline;
use lotype_core::{BlockId, BlockSpec, Content, Document, Editor, Pos, Role, Schema, Selection};

fn schema() -> Schema {
    let text = |name: &str, role| BlockSpec::new(name, Content::Inline).with_role(role);
    Schema::new()
        .with_text_block("line")
        .block(text("line", Role::Text).with_children())
        .block(text("task", Role::ListItem).with_children())
        .block(text("fold", Role::Toggle).with_children())
        .block(text("snippet", Role::Code))
        .block(
            BlockSpec::new("callout", Content::None)
                .with_children()
                .with_role(Role::Wrapper),
        )
}

/// An editor with the caret at `offset` in the last block made.
fn editor(made: impl FnOnce(&mut Document) -> BlockId, offset: usize) -> Editor {
    let mut doc = Document::new(schema());
    let block = made(&mut doc);
    let mut ed = Editor::new(doc);
    ed.set_selection(Some(Selection::caret(Pos::new(block, offset))))
        .unwrap();
    ed
}

fn shown(ed: &Editor) -> String {
    outline(ed.document(), ed.selection())
}

#[test]
fn a_list_item_goes_on_and_an_empty_one_leaves_its_list() {
    let mut ed = editor(
        |doc| {
            let task = doc.insert_block(None, 0, "task").unwrap();
            doc.insert_text(task, 0, "a").unwrap();
            task
        },
        1,
    );
    ed.split_block().unwrap();
    assert_eq!(shown(&ed), "task \"a\"\ntask \"<a>\"\n");
    ed.split_block().unwrap();
    assert_eq!(shown(&ed), "task \"a\"\nline \"<a>\"\n");
}

#[test]
fn enter_leaves_a_wrapper_for_the_text_block() {
    let mut ed = editor(
        |doc| {
            let callout = doc.insert_block(None, 0, "callout").unwrap();
            let line = doc.insert_block(Some(callout), 0, "line").unwrap();
            doc.insert_text(line, 0, "in").unwrap();
            line
        },
        2,
    );
    ed.split_block().unwrap();
    assert_eq!(shown(&ed), "callout\n  line \"in\"\nline \"<a>\"\n");
}

#[test]
fn code_takes_no_markdown_and_no_slash_menu() {
    let mut ed = editor(|doc| doc.insert_block(None, 0, "snippet").unwrap(), 0);
    ed.insert_text("**b** /x").unwrap();
    assert_eq!(shown(&ed), "snippet \"**b** /x<a>\"\n");
    assert_eq!(ed.slash_query(), None);
}

#[test]
fn enter_in_an_open_toggle_writes_a_text_block_in_it() {
    let mut ed = editor(
        |doc| {
            let fold = doc.insert_block(None, 0, "fold").unwrap();
            doc.insert_text(fold, 0, "title").unwrap();
            fold
        },
        5,
    );
    ed.split_toggle(true).unwrap();
    assert_eq!(shown(&ed), "fold \"title\"\n  line \"<a>\"\n");
}

#[test]
fn without_a_text_block_nothing_is_made_where_one_would_be() {
    let schema = Schema::new().block(BlockSpec::new("line", Content::Inline));
    assert_eq!(schema.text_block(), None);
    let mut doc = Document::new(schema);
    let line = doc.insert_block(None, 0, "line").unwrap();
    let mut ed = Editor::new(doc);
    ed.set_selection(Some(Selection::Blocks(vec![line])))
        .unwrap();
    ed.delete_selection().unwrap();
    assert_eq!(outline(ed.document(), None), "");
}
