//! Keel's commands and key bindings, with VS Code's command names and a
//! `keybindings.json` in VS Code's format to change them.

use std::path::Path;

use gpui::{Action, App, KeyBinding};
use serde_json::Value;

use crate::workbench_window::{CloseWindow, Quit, Save, SelectLanguage, SelectTheme, ShowCommands};

/// A Keel command: its VS Code name, the action it runs and its default key.
pub struct KeelCommand {
    pub id: &'static str,
    pub action: fn() -> Box<dyn Action>,
    pub default_key: Option<&'static str>,
}

/// Keel's own commands. Their names follow VS Code, so a `keybindings.json`
/// copied from VS Code or Cursor works for them.
pub const COMMANDS: &[KeelCommand] = &[
    KeelCommand {
        id: "workbench.action.files.save",
        action: || Box::new(Save),
        default_key: Some("cmd+s"),
    },
    KeelCommand {
        id: "workbench.action.closeWindow",
        action: || Box::new(CloseWindow),
        default_key: Some("cmd+w"),
    },
    KeelCommand {
        id: "workbench.action.quit",
        action: || Box::new(Quit),
        default_key: Some("cmd+q"),
    },
    KeelCommand {
        id: "workbench.action.showCommands",
        action: || Box::new(ShowCommands),
        default_key: Some("cmd+shift+p"),
    },
    KeelCommand {
        id: "workbench.action.selectTheme",
        action: || Box::new(SelectTheme),
        default_key: Some("cmd+k cmd+t"),
    },
    KeelCommand {
        id: "keel.selectLanguage",
        action: || Box::new(SelectLanguage),
        default_key: None,
    },
];

/// An entry of the user's `keybindings.json`.
#[derive(Clone, Debug, PartialEq)]
pub struct UserBinding {
    /// The command, without the leading `-` of a removal.
    pub command: String,
    /// `true` for `"-command"` entries, which remove a binding.
    pub removes: bool,
    /// The keystrokes in GPUI's notation, such as `cmd-shift-p`.
    pub keystrokes: String,
    pub when: Option<String>,
    pub args: Option<Value>,
}

/// Reads `keybindings.json`. A missing file means no changes; entries Keel
/// cannot read are skipped and reported.
pub fn read_user_bindings(file: &Path) -> (Vec<UserBinding>, Vec<String>) {
    let mut warnings = Vec::new();
    let text = match std::fs::read_to_string(file) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return (Vec::new(), warnings);
        }
        Err(error) => {
            warnings.push(format!("{}: {error}", file.display()));
            return (Vec::new(), warnings);
        }
    };
    let entries = match settings::parse_jsonc(&text) {
        Ok(Value::Array(entries)) => entries,
        Ok(_) => {
            warnings.push(format!(
                "{}: key bindings must be a JSON list",
                file.display()
            ));
            return (Vec::new(), warnings);
        }
        Err(error) => {
            warnings.push(format!("{}: {error}", file.display()));
            return (Vec::new(), warnings);
        }
    };
    let bindings = entries
        .iter()
        .filter_map(|entry| {
            let command = entry["command"].as_str()?;
            let key = entry["key"].as_str().unwrap_or_default();
            let Some(keystrokes) = workbench::keybinding::to_gpui(key, entry["mac"].as_str())
            else {
                warnings.push(format!("{}: cannot read the key \"{key}\"", file.display()));
                return None;
            };
            let (command, removes) = match command.strip_prefix('-') {
                Some(command) => (command, true),
                None => (command, false),
            };
            Some(UserBinding {
                command: command.to_owned(),
                removes,
                keystrokes,
                when: entry["when"].as_str().map(str::to_owned),
                args: entry.get("args").cloned(),
            })
        })
        .collect();
    (bindings, warnings)
}

/// The keystrokes of every Keel command: the defaults, minus the ones the
/// user removed, plus the ones the user added.
pub fn keel_bindings(user: &[UserBinding]) -> Vec<(&'static KeelCommand, String)> {
    let mut bindings = Vec::new();
    for command in COMMANDS {
        if let Some(key) = command.default_key {
            let keystrokes = workbench::keybinding::to_gpui(key, None).unwrap_or_default();
            let removed = user.iter().any(|binding| {
                binding.removes && binding.command == command.id && binding.keystrokes == keystrokes
            });
            if !removed {
                bindings.push((command, keystrokes));
            }
        }
    }
    for binding in user.iter().filter(|binding| !binding.removes) {
        if let Some(command) = COMMANDS
            .iter()
            .find(|command| command.id == binding.command)
        {
            bindings.push((command, binding.keystrokes.clone()));
        }
    }
    bindings
}

/// Binds Keel's commands. Keys follow the physical layout of non-US
/// keyboards, such as the Italian one, the way macOS apps do.
pub fn bind(user: &[UserBinding], cx: &mut App) -> Vec<String> {
    let mut warnings = Vec::new();
    let mut key_bindings = Vec::new();
    for (command, keystrokes) in keel_bindings(user) {
        match KeyBinding::load(&keystrokes, (command.action)(), None, true, None, cx) {
            Ok(binding) => key_bindings.push(binding),
            Err(error) => warnings.push(format!("{}: {error}", command.id)),
        }
    }
    cx.bind_keys(key_bindings);
    warnings
}

#[cfg(test)]
mod tests {
    use super::*;

    fn user(text: &str) -> Vec<UserBinding> {
        let folder = tempfile::tempdir().unwrap();
        let file = folder.path().join("keybindings.json");
        std::fs::write(&file, text).unwrap();
        let (bindings, warnings) = read_user_bindings(&file);
        assert!(warnings.is_empty(), "{warnings:?}");
        bindings
    }

    #[test]
    fn reads_the_vs_code_format() {
        let bindings = user(
            r#"[
                // Cursor-style palette
                { "key": "cmd+shift+a", "command": "workbench.action.showCommands" },
                { "key": "cmd+s", "command": "-workbench.action.files.save" },
                { "key": "ctrl+g", "mac": "cmd+g", "command": "vsauto.start", "when": "editorTextFocus", "args": { "x": 1 } },
            ]"#,
        );
        assert_eq!(bindings.len(), 3);
        assert_eq!(bindings[0].keystrokes, "shift-cmd-a");
        assert!(bindings[1].removes);
        assert_eq!(bindings[1].command, "workbench.action.files.save");
        assert_eq!(bindings[2].keystrokes, "cmd-g");
        assert_eq!(bindings[2].when.as_deref(), Some("editorTextFocus"));
        assert_eq!(bindings[2].args, Some(serde_json::json!({ "x": 1 })));
    }

    #[test]
    fn defaults_can_be_removed_and_added() {
        let bindings = user(
            r#"[
                { "key": "cmd+s", "command": "-workbench.action.files.save" },
                { "key": "cmd+alt+s", "command": "workbench.action.files.save" },
            ]"#,
        );
        let keel = keel_bindings(&bindings);
        let save: Vec<&str> = keel
            .iter()
            .filter(|(command, _)| command.id == "workbench.action.files.save")
            .map(|(_, keystrokes)| keystrokes.as_str())
            .collect();
        assert_eq!(save, ["alt-cmd-s"]);
        assert!(keel.iter().any(
            |(command, keys)| command.id == "workbench.action.showCommands"
                && keys == "shift-cmd-p"
        ));
        assert!(keel.iter().any(
            |(command, keys)| command.id == "workbench.action.selectTheme" && keys == "cmd-k cmd-t"
        ));
    }

    #[test]
    fn no_file_means_the_defaults() {
        let (bindings, warnings) =
            read_user_bindings(Path::new("/nonexistent/keel/keybindings.json"));
        assert!(bindings.is_empty() && warnings.is_empty());
        assert_eq!(keel_bindings(&[]).len(), 5);
    }

    #[test]
    fn unreadable_keys_are_reported() {
        let folder = tempfile::tempdir().unwrap();
        let file = folder.path().join("keybindings.json");
        std::fs::write(&file, r#"[{ "key": "hyper+x", "command": "a" }]"#).unwrap();
        let (bindings, warnings) = read_user_bindings(&file);
        assert!(bindings.is_empty());
        assert_eq!(warnings.len(), 1);
    }
}
