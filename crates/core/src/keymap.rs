use crate::{BlockKind, Editor, Result, Value, block};

/// Something a key, a menu or a toolbar asks an editor to do.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Command {
    /// Toggles a mark on the selection, or for the next typed text at a caret.
    ToggleMark(String),
    /// Turns the selected blocks into this type, with these attributes only.
    SetBlock {
        /// The name of the block type.
        block_type: String,
        /// The attributes the blocks get, and only those.
        attrs: Vec<(String, Value)>,
    },
    /// Nests the selected blocks under the block before them, as Tab does.
    Indent,
    /// Moves the selected blocks out of their parent, as Shift-Tab does.
    Outdent,
    /// Takes back the last step.
    Undo,
    /// Does again the last step undo took back.
    Redo,
    /// Enter.
    SplitBlock,
    /// Shift-Enter: a line break in the block.
    LineBreak,
    /// Backspace.
    DeleteBackward,
    /// Delete.
    DeleteForward,
}

impl Command {
    /// Toggles the mark `name`.
    pub fn toggle_mark(name: &str) -> Self {
        Self::ToggleMark(name.to_owned())
    }

    /// Turns the selected blocks into `block_type`, with no attributes; add some with
    /// [`Command::with`].
    pub fn set_block(block_type: &str) -> Self {
        let (block_type, attrs) = (block_type.to_owned(), Vec::new());
        Self::SetBlock { block_type, attrs }
    }

    /// Adds an attribute to [`Self::SetBlock`]. Other commands stay as they are.
    pub fn with(mut self, key: &str, value: impl Into<Value>) -> Self {
        if let Self::SetBlock { attrs, .. } = &mut self {
            attrs.push((key.to_owned(), value.into()));
        }
        self
    }
}

/// A key and what it does. Keys are named as in ProseMirror: modifiers in
/// the order `Mod-Alt-Shift-`, then the key. `Mod` is Command on macOS and
/// Control elsewhere.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyBinding {
    /// The key, as ProseMirror names it: `Mod-b`, `Shift-Enter`, `Mod-Alt-1`.
    pub key: String,
    /// What the key does.
    pub command: Command,
}

/// The built-in bindings, those of Tiptap and BlockNote. A view turns its
/// key events into these names, and adds its own bindings before or after.
pub fn default_keymap() -> Vec<KeyBinding> {
    let mut bindings = vec![
        ("Mod-b", Command::toggle_mark("bold")),
        ("Mod-i", Command::toggle_mark("italic")),
        ("Mod-u", Command::toggle_mark("underline")),
        ("Mod-Shift-s", Command::toggle_mark("strike")),
        ("Mod-e", Command::toggle_mark("code")),
        ("Mod-Alt-0", Command::turn_into(&BlockKind::Paragraph)),
        (
            "Mod-Shift-7",
            Command::turn_into(&BlockKind::NumberedListItem { start: None }),
        ),
        (
            "Mod-Shift-8",
            Command::turn_into(&BlockKind::BulletListItem),
        ),
        (
            "Mod-Shift-6",
            Command::turn_into(&BlockKind::ToggleListItem),
        ),
        // By name, so that an item that is one already stays checked.
        ("Mod-Shift-9", Command::set_block(block::CHECK_LIST_ITEM)),
        (
            "Mod-Alt-c",
            Command::turn_into(&BlockKind::CodeBlock { language: None }),
        ),
        ("Tab", Command::Indent),
        ("Shift-Tab", Command::Outdent),
        ("Mod-z", Command::Undo),
        ("Mod-Shift-z", Command::Redo),
        ("Mod-y", Command::Redo),
        ("Enter", Command::SplitBlock),
        ("Shift-Enter", Command::LineBreak),
        ("Backspace", Command::DeleteBackward),
        ("Delete", Command::DeleteForward),
    ];
    let heading = |level| Command::turn_into(&BlockKind::Heading(level));
    let levels = [
        "Mod-Alt-1",
        "Mod-Alt-2",
        "Mod-Alt-3",
        "Mod-Alt-4",
        "Mod-Alt-5",
    ];
    bindings.extend(
        levels
            .into_iter()
            .zip(1..)
            .map(|(key, level)| (key, heading(level))),
    );
    let bind = |(key, command): (&str, Command)| KeyBinding {
        key: key.to_owned(),
        command,
    };
    bindings.into_iter().map(bind).collect()
}

impl Editor {
    /// Does what `command` says. A command that finds nothing to do is not an error.
    pub fn run(&mut self, command: &Command) -> Result<()> {
        match command {
            Command::ToggleMark(name) => self.toggle_mark(name),
            Command::SetBlock { block_type, attrs } => self.set_block(block_type, attrs),
            Command::Indent => self.indent(),
            Command::Outdent => self.outdent(),
            Command::Undo => self.undo().map(drop),
            Command::Redo => self.redo().map(drop),
            Command::SplitBlock => self.split_block(),
            Command::LineBreak => self.insert_line_break(),
            Command::DeleteBackward => self.delete_backward(),
            Command::DeleteForward => self.delete_forward(),
        }
    }
}
