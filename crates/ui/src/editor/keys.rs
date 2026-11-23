//! Keys, bound to editor commands by the core's keymap.

use gpui::{Context, KeyDownEvent, Keystroke, Window};

use super::{EditorView, motion};

/// The name the core's keymap gives a keystroke, such as `Mod-Shift-z`.
pub(crate) fn key_name(keystroke: &Keystroke) -> String {
    let modifiers = &keystroke.modifiers;
    // `Mod` is Command on macOS and Control elsewhere.
    let (command, control) = if cfg!(target_os = "macos") {
        (modifiers.platform, modifiers.control)
    } else {
        (modifiers.control, false)
    };
    let mut name = String::new();
    for (held, prefix) in [
        (command, "Mod-"),
        (control, "Ctrl-"),
        (modifiers.alt, "Alt-"),
        (modifiers.shift, "Shift-"),
    ] {
        if held {
            name.push_str(prefix);
        }
    }
    let key = keystroke.key.as_str();
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
        // While text is being composed, keys are the IME's.
        if self.composition.is_some() {
            return;
        }
        let name = key_name(&event.keystroke);
        if let Some((motion, extend)) = motion::motion(&name) {
            self.move_cursor(motion, extend);
        } else if name == "Mod-a" {
            self.select_all();
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
        window.notify_text_input_changed(&self.focus);
        cx.notify();
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
        if cfg!(target_os = "macos") {
            assert_eq!(name("ctrl-b"), "Ctrl-b");
        }
    }
}
