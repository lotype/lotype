//! Keys, bound to editor commands by the core's keymap.

use gpui::{Context, KeyDownEvent, Keystroke, Window};

use super::{EditorView, motion};

/// Characters that Shift makes of the keys in `UNSHIFTED`, on a US layout.
const SHIFTED: &str = ")!@#$%^&*(_+{}|:\"<>?~";
const UNSHIFTED: &str = "0123456789-=[]\\;',./`";

/// The name the core's keymap gives a keystroke, such as `Mod-Shift-z`.
pub(crate) fn key_name(keystroke: &Keystroke) -> String {
    let modifiers = &keystroke.modifiers;
    // `Mod` is Command on macOS and Control elsewhere.
    let (command, control) = if cfg!(target_os = "macos") {
        (modifiers.platform, modifiers.control)
    } else {
        (modifiers.control, false)
    };
    // GPUI on macOS reports Shift with a digit or a sign as the shifted
    // character, without Shift: Command-Shift-7 comes as `cmd-&`. The
    // keymap names the key, as ProseMirror does, so take Shift back off.
    // shortcut: assumes a US layout, where other layouts shift differently;
    // upgrade when GPUI reports key codes.
    let mut key = keystroke.key.as_str();
    let mut shift = modifiers.shift;
    if let Some(at) = SHIFTED.find(key).filter(|_| key.len() == 1) {
        (key, shift) = (&UNSHIFTED[at..at + 1], true);
    }
    let mut name = String::new();
    for (held, prefix) in [
        (command, "Mod-"),
        (control, "Ctrl-"),
        (modifiers.alt, "Alt-"),
        (shift, "Shift-"),
    ] {
        if held {
            name.push_str(prefix);
        }
    }
    match key {
        "left" | "right" | "up" | "down" => {
            name.push_str("Arrow");
            name.push_str(&capitalized(key));
        }
        _ if key.chars().count() > 1 => name.push_str(&capitalized(key)),
        _ => name.push_str(key),
    }
    name
}

fn capitalized(key: &str) -> String {
    let mut chars = key.chars();
    chars.next().map_or_else(String::new, |first| {
        first.to_uppercase().chain(chars).collect()
    })
}

impl EditorView {
    pub(super) fn key_down(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // While text is being composed, keys are the IME's, and while a
        // field of the editor's has the focus, they are the field's.
        if self.composition.is_some() || !self.focus.is_focused(window) {
            return;
        }
        let name = key_name(&event.keystroke);
        // Any key closes the block menu or a toolbar's dropdown, and Escape
        // does nothing else.
        let dropdown = self.toolbar.menu.take().is_some();
        if self.side_menu.menu.take().is_some() || dropdown {
            cx.notify();
            if name == "Escape" {
                cx.stop_propagation();
                return;
            }
        }
        if self.slash_key(&name) || self.block_key(&name) {
            // Handled as a key on whole blocks.
        } else if let Some((motion, extend)) = motion::motion(&name) {
            self.move_cursor(motion, extend);
        } else if name == "Mod-a" {
            self.select_all();
        } else if name == "Mod-c" {
            self.copy(cx);
        } else if name == "Mod-x" {
            self.cut(cx);
        } else if name == "Mod-v" {
            self.paste(cx);
        } else {
            self.run_binding(&name, window, cx);
            return;
        }
        cx.stop_propagation();
        self.selection_changed(window, cx);
    }

    fn run_binding(&mut self, name: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(binding) = self.keymap.iter().find(|binding| binding.key == name) else {
            return;
        };
        let command = binding.command.clone();
        self.editor.run(&command).ok();
        cx.stop_propagation();
        // A command may move the caret, and the IME's window with it.
        window.notify_text_input_changed(&self.focus);
        self.selection_changed(window, cx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keystrokes_are_named_as_in_the_core_keymap() {
        let name = |source: &'static str| key_name(&Keystroke::parse(source).unwrap());
        let (bold, redo) = if cfg!(target_os = "macos") {
            ("cmd-b", "cmd-alt-shift-z")
        } else {
            ("ctrl-b", "ctrl-alt-shift-z")
        };
        assert_eq!(name(bold), "Mod-b");
        assert_eq!(name(redo), "Mod-Alt-Shift-z");
        assert_eq!(name("shift-tab"), "Shift-Tab");
        assert_eq!(name("enter"), "Enter");
        assert_eq!(name("left"), "ArrowLeft");
        assert_eq!(SHIFTED.len(), UNSHIFTED.len());
        // As GPUI reports Command-Shift-7 on macOS.
        let shifted = Keystroke::parse(if cfg!(target_os = "macos") {
            "cmd-&"
        } else {
            "ctrl-&"
        });
        assert_eq!(key_name(&shifted.unwrap()), "Mod-Shift-7");
        if cfg!(target_os = "macos") {
            assert_eq!(name("ctrl-b"), "Ctrl-b");
        }
    }
}
