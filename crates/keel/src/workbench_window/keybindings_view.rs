//! The Keyboard Shortcuts tab (Cmd+K Cmd+S), as VS Code's: every command
//! with its keys, searchable by name, id or key. "Change" records the new
//! keys (Enter keeps them, Esc cancels); they go to the user's
//! `keybindings.json` in VS Code's format, keeping its comments, and work
//! at once. "Reset" takes the command back to its default keys.

use gpui::{
    AnyElement, FocusHandle, KeyBinding, KeyDownEvent, SharedString, Subscription, uniform_list,
};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::input::{Input, InputEvent, InputState};
use gpui_component::{Icon, Sizable as _};
use gpui_kit_assets::IconName;

use crate::keymap::{COMMANDS, KeelCommand, UserBinding};

use super::shell::Colors;
use super::*;

pub(super) struct KeybindingsView {
    pub(super) filter: Entity<InputState>,
    /// The user's entries, read when the tab opens and after each change.
    pub(super) user: Vec<UserBinding>,
    /// The command whose keys are being recorded, and those pressed so far.
    pub(super) recording: Option<(&'static str, Vec<String>)>,
    pub(super) recorder: FocusHandle,
    _subscription: Subscription,
}

/// `cmd-shift-p` as VS Code writes it: `cmd+shift+p`.
pub(super) fn to_vscode(keystrokes: &str) -> String {
    keystrokes
        .split_whitespace()
        .map(|keystroke| {
            let mut parts: Vec<&str> = Vec::new();
            let mut rest = keystroke;
            // Modifiers in any order, until the key; `cmd--` is Cmd and `-`.
            while let Some(modifier) = ["ctrl-", "alt-", "shift-", "cmd-", "fn-"]
                .into_iter()
                .find(|modifier| rest.starts_with(modifier) && rest.len() > modifier.len())
            {
                parts.push(&modifier[..modifier.len() - 1]);
                rest = &rest[modifier.len()..];
            }
            parts.push(rest);
            parts.join("+")
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// A key as people read it on a Mac: `⌘⇧P`.
pub(super) fn readable(keystrokes: &str) -> String {
    keystrokes
        .split_whitespace()
        .map(|keystroke| {
            let mut rest = keystroke;
            let mut held: Vec<&str> = Vec::new();
            while let Some(modifier) = ["ctrl-", "alt-", "shift-", "cmd-", "fn-"]
                .into_iter()
                .find(|modifier| rest.starts_with(modifier) && rest.len() > modifier.len())
            {
                held.push(modifier);
                rest = &rest[modifier.len()..];
            }
            // In the order macOS shows them: ⌃⌥⇧⌘.
            let mut text = String::new();
            for (modifier, symbol) in [
                ("fn-", "fn "),
                ("ctrl-", "⌃"),
                ("alt-", "⌥"),
                ("shift-", "⇧"),
                ("cmd-", "⌘"),
            ] {
                if held.contains(&modifier) {
                    text.push_str(symbol);
                }
            }
            let key = match rest {
                "enter" => "↩".to_owned(),
                "escape" => "⎋".to_owned(),
                "backspace" => "⌫".to_owned(),
                "up" => "↑".to_owned(),
                "down" => "↓".to_owned(),
                "left" => "←".to_owned(),
                "right" => "→".to_owned(),
                "space" => "Space".to_owned(),
                "tab" => "⇥".to_owned(),
                other if other.chars().count() == 1 => other.to_uppercase(),
                other => {
                    let mut chars = other.chars();
                    chars.next().map_or_else(String::new, |first| {
                        first.to_uppercase().chain(chars).collect()
                    })
                }
            };
            text.push_str(&key);
            text
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// The keys of `command` now: its default ones unless removed, and the
/// user's.
pub(super) fn keys_of(command: &KeelCommand, user: &[UserBinding]) -> Vec<String> {
    crate::keymap::keel_bindings(user)
        .into_iter()
        .filter(|(bound, _)| bound.id == command.id)
        .map(|(_, keys)| keys)
        .collect()
}

/// The new text of `keybindings.json` after giving `command` the keys
/// `new` (in GPUI's notation), or its defaults back (`None`). Its old
/// entries go; a new key also removes the default one, as VS Code writes.
pub(super) fn rewrite_keybindings(
    text: &str,
    command: &KeelCommand,
    new: Option<&str>,
) -> anyhow::Result<String> {
    let id = command.id;
    let text = if text.trim().is_empty() {
        "[\n]\n"
    } else {
        text
    };
    let text = settings::edit::remove_items(text, |entry| {
        let command = entry["command"].as_str().unwrap_or_default();
        command == id || command.strip_prefix('-') == Some(id)
    })?;
    let Some(new) = new else {
        return Ok(text);
    };
    let mut items = vec![json!({ "key": to_vscode(new), "command": id })];
    if let Some(default) = command.default_key
        && workbench::keybinding::to_gpui(default, None).as_deref() != Some(new)
    {
        items.push(json!({ "key": default, "command": format!("-{id}") }));
    }
    settings::edit::append_items(&text, &items)
}

impl Workbench {
    pub(super) fn create_keybindings_view(
        window: &mut Window,
        cx: &mut Context<Self>,
        strings: &Strings,
    ) -> KeybindingsView {
        let filter =
            cx.new(|cx| InputState::new(window, cx).placeholder(strings.keybindings_search));
        let subscription = cx.subscribe_in(&filter, window, |_, _, event: &InputEvent, _, cx| {
            if matches!(event, InputEvent::Change) {
                cx.notify();
            }
        });
        KeybindingsView {
            filter,
            user: Vec::new(),
            recording: None,
            recorder: cx.focus_handle(),
            _subscription: subscription,
        }
    }

    fn keybindings_file(&self) -> Option<PathBuf> {
        self.data_dir
            .as_deref()
            .map(crate::paths::user_keybindings_file)
    }

    pub(super) fn reload_user_keybindings(&mut self) {
        if let Some(file) = self.keybindings_file() {
            self.keys.user = crate::keymap::read_user_bindings(&file).0;
        }
    }

    fn start_recording(&mut self, id: &'static str, window: &mut Window, cx: &mut Context<Self>) {
        self.keys.recording = Some((id, Vec::new()));
        self.keys.recorder.focus(window, cx);
        cx.notify();
    }

    /// A key while recording: Enter keeps what was pressed, Esc cancels,
    /// anything else is added (two keystrokes at most, as `cmd+k cmd+s`).
    fn record_key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let Some((id, pressed)) = &mut self.keys.recording else {
            return;
        };
        let keystroke = &event.keystroke;
        let plain = !keystroke.modifiers.modified();
        match keystroke.key.as_str() {
            "enter" if plain && !pressed.is_empty() => {
                let (id, keys) = (*id, pressed.join(" "));
                self.keys.recording = None;
                self.set_keybinding(id, Some(keys), window, cx);
            }
            "escape" if plain => self.keys.recording = None,
            // Modifiers alone wait for their key.
            "shift" | "control" | "alt" | "platform" | "function" => {}
            _ => {
                let mut text = keystroke.unparse(gpui::OperatingSystem::Mac);
                // GPUI writes the key after the modifiers as macOS names it.
                if text.is_empty() {
                    text = keystroke.key.clone();
                }
                if pressed.len() >= 2 {
                    pressed.clear();
                }
                pressed.push(text);
            }
        }
        cx.stop_propagation();
        cx.notify();
    }

    /// Writes the new keys of a command (or its defaults back) and makes
    /// them work at once in every window.
    pub(super) fn set_keybinding(
        &mut self,
        id: &str,
        keys: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let _ = window;
        let Some(command) = COMMANDS.iter().find(|command| command.id == id) else {
            return;
        };
        let Some(file) = self.keybindings_file() else {
            return;
        };
        let before = keys_of(command, &self.keys.user);
        let text = std::fs::read_to_string(&file).unwrap_or_default();
        let written = rewrite_keybindings(&text, command, keys.as_deref()).and_then(|text| {
            if let Some(folder) = file.parent() {
                std::fs::create_dir_all(folder)?;
            }
            std::fs::write(&file, text)?;
            Ok(())
        });
        if let Err(error) = written {
            self.report(format!("keybindings.json: {error:#}"));
            cx.notify();
            return;
        }
        self.reload_user_keybindings();
        let after = keys_of(command, &self.keys.user);
        // The old keys stop, the new ones start, without a restart.
        let context = command
            .context
            .and_then(|context| gpui::KeyBindingContextPredicate::parse(context).ok())
            .map(std::rc::Rc::new);
        let action_name = (command.action)().name();
        let mut bindings = Vec::new();
        for old in before.iter().filter(|old| !after.contains(old)) {
            if let Ok(binding) = KeyBinding::load(
                old,
                Box::new(gpui::Unbind(action_name.into())),
                context.clone(),
                true,
                None,
                cx,
            ) {
                bindings.push(binding);
            }
        }
        for new in &after {
            if let Ok(binding) =
                KeyBinding::load(new, (command.action)(), context.clone(), true, None, cx)
            {
                bindings.push(binding);
            }
        }
        cx.bind_keys(bindings);
        cx.notify();
    }

    /// Binds the user's entries for Keel's commands now, as at the start:
    /// for keys imported from Cursor.
    pub(super) fn bind_user_keybindings(&mut self, cx: &mut Context<Self>) {
        let mut bindings = Vec::new();
        for binding in &self.keys.user {
            let Some(command) = COMMANDS
                .iter()
                .find(|command| command.id == binding.command)
            else {
                continue;
            };
            let context = command
                .context
                .and_then(|context| gpui::KeyBindingContextPredicate::parse(context).ok())
                .map(std::rc::Rc::new);
            let action: Box<dyn gpui::Action> = if binding.removes {
                Box::new(gpui::Unbind((command.action)().name().into()))
            } else {
                (command.action)()
            };
            if let Ok(binding) =
                KeyBinding::load(&binding.keystrokes, action, context, true, None, cx)
            {
                bindings.push(binding);
            }
        }
        cx.bind_keys(bindings);
    }

    pub(super) fn render_keybindings_page(
        &self,
        colors: Colors,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let strings = self.strings();
        let filter = self.keys.filter.read(cx).value().to_lowercase();
        let rows: Vec<&'static KeelCommand> = COMMANDS
            .iter()
            .filter(|command| {
                if filter.is_empty() {
                    return true;
                }
                let title = strings
                    .command_title(command.id)
                    .unwrap_or(command.id)
                    .to_lowercase();
                let keys = keys_of(command, &self.keys.user).join(" ");
                title.contains(&filter)
                    || command.id.to_lowercase().contains(&filter)
                    || readable(&keys).to_lowercase().contains(&filter)
                    || keys.contains(&filter)
            })
            .collect();
        let count = rows.len();
        let user = self.keys.user.clone();
        let recording = self.keys.recording.clone();
        let italian = self.language == UiLanguage::Italian;
        // A key as a little keycap, as macOS's menus and Settings draw them.
        let keycap = move |keys: String| {
            div()
                .flex_none()
                .h(px(22.))
                .px(px(7.))
                .flex()
                .items_center()
                .rounded(px(5.))
                .border_1()
                .border_color(colors.border)
                .bg(colors.text.opacity(0.04))
                .text_size(px(12.))
                .text_color(colors.text)
                .child(keys)
        };
        let list = uniform_list(
            "keybindings",
            count,
            cx.processor(move |this, range: std::ops::Range<usize>, _, cx| {
                let strings = this.strings();
                range
                    .map(|index| {
                        let command = rows[index];
                        let keys = keys_of(command, &user);
                        let custom = user.iter().any(|binding| binding.command == command.id);
                        let recorded = recording
                            .as_ref()
                            .filter(|(id, _)| *id == command.id)
                            .map(|(_, pressed)| pressed.join(" "));
                        let id = command.id;
                        let title = strings.command_title(id).unwrap_or(id);
                        // «Vista: Chiudi finestra»: the group in grey, the name in full.
                        let (group, name) = match title.split_once(": ") {
                            Some((group, name)) => (Some(format!("{group}: ")), name),
                            None => (None, title),
                        };
                        let keys_cell = match &recorded {
                            Some(pressed) => div()
                                .text_size(px(12.5))
                                .text_color(colors.accent)
                                .child(if pressed.is_empty() {
                                    strings.keybindings_press.to_owned()
                                } else {
                                    format!("{} ↩", readable(pressed))
                                })
                                .into_any_element(),
                            None if keys.is_empty() => {
                                div().text_color(colors.faint).child("—").into_any_element()
                            }
                            None => div()
                                .flex()
                                .gap(px(5.))
                                .children(keys.iter().map(|keys| keycap(readable(keys))))
                                .into_any_element(),
                        };
                        let row_group = SharedString::from(format!("keybinding-row-{index}"));
                        div()
                            .id(("keybinding", index))
                            .group(row_group.clone())
                            .h(px(36.))
                            .mx(px(16.))
                            .px(px(12.))
                            .flex()
                            .items_center()
                            .gap(px(14.))
                            .rounded(px(7.))
                            .text_size(px(13.))
                            .hover(|row| row.bg(colors.hover))
                            .when(recorded.is_some(), |row| {
                                row.bg(colors.accent.opacity(0.08))
                            })
                            .child(
                                div()
                                    .w(px(330.))
                                    .flex_none()
                                    .min_w_0()
                                    .flex()
                                    .overflow_hidden()
                                    .children(group.map(|group| {
                                        div().flex_none().text_color(colors.muted).child(group)
                                    }))
                                    .child(
                                        div()
                                            .min_w_0()
                                            .truncate()
                                            .text_color(colors.text)
                                            .child(name.to_owned()),
                                    ),
                            )
                            .child(div().w(px(200.)).flex_none().child(keys_cell))
                            .child(div().w(px(88.)).flex_none().child(if custom {
                                div()
                                    .flex_none()
                                    .h(px(20.))
                                    .px(px(8.))
                                    .flex()
                                    .items_center()
                                    .rounded_full()
                                    .bg(colors.accent.opacity(0.14))
                                    .text_size(px(11.5))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(colors.accent)
                                    .child(strings.keybindings_user)
                                    .into_any_element()
                            } else {
                                div()
                                    .text_size(px(12.))
                                    .text_color(colors.faint)
                                    .child(strings.keybindings_default)
                                    .into_any_element()
                            }))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .truncate()
                                    .text_size(px(11.5))
                                    .text_color(colors.faint)
                                    .child(id),
                            )
                            // The buttons show on the row under the pointer.
                            .child(
                                div()
                                    .flex_none()
                                    .flex()
                                    .gap(px(2.))
                                    .opacity(0.)
                                    .group_hover(row_group, |buttons| buttons.opacity(1.))
                                    .child(
                                        Button::new(("keybinding-change", index))
                                            .ghost()
                                            .small()
                                            .icon(Icon::new(IconName::Pencil))
                                            .tooltip(strings.keybindings_change)
                                            .on_click(cx.listener(move |this, _, window, cx| {
                                                this.start_recording(id, window, cx)
                                            })),
                                    )
                                    .children(custom.then(|| {
                                        Button::new(("keybinding-reset", index))
                                            .ghost()
                                            .small()
                                            .icon(Icon::new(IconName::Undo2))
                                            .tooltip(strings.keybindings_reset)
                                            .on_click(cx.listener(move |this, _, window, cx| {
                                                this.set_keybinding(id, None, window, cx)
                                            }))
                                    })),
                            )
                            .into_any_element()
                    })
                    .collect::<Vec<_>>()
            }),
        )
        .size_full();
        let column = |text: &'static str, width: Option<f32>| {
            div()
                .when_some(width, |cell, width| cell.w(px(width)).flex_none())
                .when(width.is_none(), |cell| cell.flex_1())
                .text_size(px(11.5))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(colors.faint)
                .child(text)
        };
        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(colors.editor)
            .track_focus(&self.keys.recorder)
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                this.record_key(event, window, cx)
            }))
            .child(
                div()
                    .px(px(28.))
                    .pt(px(22.))
                    .pb(px(14.))
                    .flex()
                    .flex_col()
                    .gap(px(6.))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(10.))
                            .child(
                                Icon::new(IconName::Keyboard)
                                    .size(px(20.))
                                    .text_color(colors.accent),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .text_size(px(22.))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(colors.text)
                                    .child(strings.keybindings_tab_title),
                            )
                            .child(
                                Button::new("open-keybindings-json")
                                    .outline()
                                    .small()
                                    .icon(IconName::FileBraces)
                                    .label(strings.open_keybindings_json)
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        if let Some(file) = this.keybindings_file() {
                                            if !file.exists() {
                                                let _ = std::fs::write(&file, "[\n]\n");
                                            }
                                            this.open_file(&file, window, cx);
                                        }
                                    })),
                            ),
                    )
                    .child(
                        div()
                            .text_size(px(13.))
                            .text_color(colors.muted)
                            .child(if italian {
                                "Ogni comando di Keel con i suoi tasti. Passa sopra una riga e premi la matita: poi i tasti nuovi e Invio."
                            } else {
                                "Every command of Keel with its keys. Hover a row and press the pencil, then the new keys and Return."
                            }),
                    )
                    .child(
                        div()
                            .pt(px(8.))
                            .flex()
                            .items_center()
                            .gap(px(12.))
                            .child(
                                div().w(px(420.)).max_w_full().child(
                                    Input::new(&self.keys.filter)
                                        .small()
                                        .cleanable(true)
                                        .prefix(
                                            Icon::new(IconName::Search)
                                                .small()
                                                .text_color(colors.muted),
                                        ),
                                ),
                            )
                            .child(
                                div()
                                    .text_size(px(12.))
                                    .text_color(colors.faint)
                                    .child(if italian {
                                        format!("{count} comandi")
                                    } else {
                                        format!("{count} commands")
                                    }),
                            ),
                    ),
            )
            .child(
                div()
                    .mx(px(16.))
                    .px(px(12.))
                    .pb(px(6.))
                    .flex()
                    .gap(px(14.))
                    .border_b_1()
                    .border_color(colors.border.opacity(0.7))
                    .child(column(if italian { "Comando" } else { "Command" }, Some(330.)))
                    .child(column(if italian { "Tasti" } else { "Keys" }, Some(200.)))
                    .child(column(if italian { "Da dove" } else { "Source" }, Some(88.)))
                    .child(column(if italian { "Nome nel file" } else { "Name in the file" }, None)),
            )
            .child(div().flex_1().min_h_0().pt(px(4.)).child(list))
            .when(count == 0, |page| {
                page.child(
                    div()
                        .px(px(28.))
                        .pb(px(24.))
                        .text_size(px(13.))
                        .text_color(colors.muted)
                        .child(strings.no_results),
                )
            })
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_in_vscode_spelling_and_for_people() {
        assert_eq!(to_vscode("cmd-shift-p"), "cmd+shift+p");
        assert_eq!(to_vscode("cmd-k cmd-0"), "cmd+k cmd+0");
        assert_eq!(to_vscode("cmd--"), "cmd+-");
        assert_eq!(readable("cmd-shift-p"), "⇧⌘P");
        assert_eq!(readable("cmd-k cmd-s"), "⌘K ⌘S");
        assert_eq!(readable("alt-up"), "⌥↑");
        assert_eq!(readable("f12"), "F12");
    }

    #[test]
    fn a_new_key_replaces_the_default_one_in_the_file() {
        let command = COMMANDS
            .iter()
            .find(|command| command.id == "editor.action.commentLine")
            .unwrap();
        let file =
            "// mine\n[\n    { \"key\": \"f5\", \"command\": \"workbench.action.quickOpen\" }\n]\n";
        let written = rewrite_keybindings(file, command, Some("cmd-alt-/")).unwrap();
        assert!(written.starts_with("// mine\n"));
        let entries = settings::parse_jsonc(&written).unwrap();
        assert_eq!(
            entries,
            json!([
                { "key": "f5", "command": "workbench.action.quickOpen" },
                { "key": "cmd+alt+/", "command": "editor.action.commentLine" },
                { "key": "cmd+/", "command": "-editor.action.commentLine" },
            ])
        );
        // Reset: its entries go, the others stay.
        let reset = rewrite_keybindings(&written, command, None).unwrap();
        assert_eq!(
            settings::parse_jsonc(&reset).unwrap(),
            json!([{ "key": "f5", "command": "workbench.action.quickOpen" }])
        );
    }
}
