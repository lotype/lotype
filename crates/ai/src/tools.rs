//! What a model may do to a document, and how it says so.

use lotype_core::BlockId;
use serde_json::{Value, json};

use crate::ToolSpec;
use crate::partial::{Partial, Seg};

/// An edit a model made.
#[derive(Debug, Clone, PartialEq)]
pub enum Op {
    /// New blocks next to one that is there.
    Add {
        /// The block they go next to.
        reference: BlockId,
        /// Before it, or else after it.
        before: bool,
        /// The Markdown of each.
        blocks: Vec<String>,
    },
    /// Another text, type or attributes for a block.
    Update {
        /// The block.
        block: BlockId,
        /// What it is to be, as Markdown.
        markdown: String,
    },
    /// A block taken out, with what is in it.
    Delete {
        /// The block.
        block: BlockId,
    },
}

/// The tools a model edits a document with.
pub(crate) fn specs() -> Vec<ToolSpec> {
    let id = json!({"type": "string", "description": "The id of a block of the document."});
    let markdown = json!({"type": "string", "description": "One block, as Markdown."});
    vec![
        ToolSpec {
            name: "add",
            description: "Adds blocks before or after a block of the document.",
            schema: json!({
                "type": "object",
                "properties": {
                    "referenceId": id,
                    "position": {"type": "string", "enum": ["before", "after"]},
                    "blocks": {"type": "array", "items": markdown},
                },
                "required": ["referenceId", "position", "blocks"],
            }),
        },
        ToolSpec {
            name: "update",
            description: "Replaces a block of the document, and not what is nested in it.",
            schema: json!({
                "type": "object",
                "properties": {"id": id, "block": markdown},
                "required": ["id", "block"],
            }),
        },
        ToolSpec {
            name: "delete",
            description: "Removes a block of the document, with what is nested in it.",
            schema: json!({
                "type": "object",
                "properties": {"id": id},
                "required": ["id"],
            }),
        },
    ]
}

/// The call is none the tools take.
#[derive(Debug, PartialEq)]
pub(crate) struct Invalid;

/// The edit a call asks for, as far as its arguments say. `None` while
/// they do not say which block yet: an id is only read once it is whole,
/// as half of one is another block's. Markdown that is still being written
/// is taken as it is.
pub(crate) fn read(name: &str, args: &Partial) -> Result<Option<Op>, Invalid> {
    let Value::Object(fields) = &args.value else {
        return Err(Invalid);
    };
    // A field that is there and whole; `None` while it is not.
    let text = |key: &str| -> Result<Option<&str>, Invalid> {
        let cut = args.cut.as_deref() == Some(&[Seg::Key(key.to_owned())]);
        match fields.get(key) {
            Some(Value::String(text)) if !cut => Ok(Some(text)),
            Some(Value::String(_)) | None if !args.whole => Ok(None),
            _ => Err(Invalid),
        }
    };
    let block = |key: &str| -> Result<Option<BlockId>, Invalid> {
        let id = text(key)?.map(|id| id.parse().map_err(|_| Invalid));
        id.transpose()
    };
    Ok(match name {
        "add" => {
            let before = match text("position")? {
                Some("before") => Some(true),
                Some("after") => Some(false),
                Some(_) => return Err(Invalid),
                None => None,
            };
            let blocks = match fields.get("blocks") {
                Some(Value::Array(blocks)) => {
                    let texts = blocks.iter().map(|block| block.as_str().map(str::to_owned));
                    Some(texts.collect::<Option<Vec<_>>>().ok_or(Invalid)?)
                }
                None if !args.whole => None,
                _ => return Err(Invalid),
            };
            let reference = block("referenceId")?;
            let all = reference.zip(before).zip(blocks);
            all.map(|((reference, before), blocks)| Op::Add {
                reference,
                before,
                blocks,
            })
        }
        "update" => {
            let markdown = match fields.get("block") {
                Some(Value::String(markdown)) => Some(markdown.clone()),
                None if !args.whole => None,
                _ => return Err(Invalid),
            };
            let all = block("id")?.zip(markdown);
            all.map(|(block, markdown)| Op::Update { block, markdown })
        }
        // Not before it is certain: there is no taking half of it back.
        "delete" => block("id")?
            .filter(|_| args.whole)
            .map(|block| Op::Delete { block }),
        _ => return Err(Invalid),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::partial;

    fn op(name: &str, args: &str) -> Result<Option<Op>, Invalid> {
        read(name, &partial::read(args).unwrap().unwrap())
    }

    #[test]
    fn an_edit_is_read_once_it_says_which_block() {
        let args = r#"{"id": "3@7", "block": "new **text**"}"#;
        let block: BlockId = "3@7".parse().unwrap();
        for end in 1..=args.len() {
            let read = op("update", &args[..end]).unwrap();
            // The id closes at 12, and the block's text starts at 24.
            match end {
                ..24 => assert_eq!(read, None, "at {end}"),
                _ => {
                    let Some(Op::Update {
                        block: of,
                        markdown,
                    }) = read
                    else {
                        panic!("an update at {end}");
                    };
                    assert_eq!(of, block);
                    assert!("new **text**".starts_with(&markdown));
                }
            }
        }
        // Whichever comes first.
        assert_eq!(op("update", r#"{"block": "text", "id": "3@"#), Ok(None));
    }

    #[test]
    fn blocks_are_added_as_they_come_and_removed_only_when_certain() {
        let block: BlockId = "3@7".parse().unwrap();
        let add = |blocks: &[&str]| Op::Add {
            reference: block,
            before: false,
            blocks: blocks.iter().map(|b| (*b).to_owned()).collect(),
        };
        let start = r#"{"referenceId": "3@7", "position": "after", "blocks": ["#;
        assert_eq!(op("add", start), Ok(Some(add(&[]))));
        assert_eq!(
            op("add", &format!("{start}\"one\", \"tw")),
            Ok(Some(add(&["one", "tw"])))
        );
        assert_eq!(op("delete", r#"{"id": "3@7""#), Ok(None));
        assert_eq!(
            op("delete", r#"{"id": "3@7"}"#),
            Ok(Some(Op::Delete { block }))
        );
    }

    #[test]
    fn a_call_no_tool_takes_is_invalid() {
        for (name, args) in [
            ("rename", "{}"),
            ("update", "[]"),
            ("update", r#"{"id": "3@7"}"#),
            ("update", r#"{"id": 3, "block": "x"}"#),
            ("update", r#"{"id": "no block", "block": "x"}"#),
            (
                "add",
                r#"{"referenceId": "3@7", "position": "under", "blocks": []}"#,
            ),
            (
                "add",
                r#"{"referenceId": "3@7", "position": "after", "blocks": [1]}"#,
            ),
            ("delete", "{}"),
        ] {
            assert_eq!(op(name, args), Err(Invalid), "{name} {args}");
        }
    }
}
