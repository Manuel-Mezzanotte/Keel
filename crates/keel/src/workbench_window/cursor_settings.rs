//! "Import from Cursor": its settings, keyboard shortcuts and recent
//! projects, from `~/Library/Application Support/Cursor/User`. Nothing of
//! Keel's is overwritten: settings Keel already has stay, shortcuts already
//! there are not doubled, and Keel's own recent projects stay first.
//! Cursor's files are only read; its database through macOS's `sqlite3`,
//! read-only.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde_json::{Map, Value};

use super::*;

/// What would come from Cursor.
#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct ImportPlan {
    pub(super) settings: Vec<(String, Value)>,
    pub(super) keybindings: Vec<Value>,
    pub(super) projects: Vec<PathBuf>,
}

impl ImportPlan {
    pub(super) fn is_empty(&self) -> bool {
        self.settings.is_empty() && self.keybindings.is_empty() && self.projects.is_empty()
    }
}

/// Cursor's user folder.
pub(super) fn cursor_user_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .map(|home| PathBuf::from(home).join("Library/Application Support/Cursor/User"))
        .filter(|dir| dir.is_dir())
}

/// Settings of Cursor's own that mean nothing in Keel, or that would
/// change what Keel looks like into something it does not have.
fn skipped(key: &str) -> bool {
    key.starts_with("cursor.")
        || key.starts_with("telemetry.")
        || key.starts_with("update.")
        || matches!(key, "workbench.colorTheme" | "workbench.iconTheme")
}

/// What to import, given Cursor's folder, Keel's files as they are, and
/// Cursor's recent folders (`folderUri` entries).
pub(super) fn plan(
    cursor_settings: &str,
    cursor_keybindings: &str,
    cursor_recent: &[String],
    keel_settings: &Map<String, Value>,
    keel_keybindings: &str,
) -> ImportPlan {
    let settings = match settings::parse_jsonc(cursor_settings) {
        Ok(Value::Object(values)) => values
            .into_iter()
            .filter(|(key, _)| !skipped(key) && !keel_settings.contains_key(key))
            .collect(),
        _ => Vec::new(),
    };
    let known: Vec<Value> = match settings::parse_jsonc(keel_keybindings) {
        Ok(Value::Array(entries)) => entries,
        _ => Vec::new(),
    };
    let same = |a: &Value, b: &Value| a["key"] == b["key"] && a["command"] == b["command"];
    let keybindings = match settings::parse_jsonc(cursor_keybindings) {
        Ok(Value::Array(entries)) => entries
            .into_iter()
            .filter(|entry| entry["key"].is_string() && entry["command"].is_string())
            .filter(|entry| !known.iter().any(|known| same(known, entry)))
            .collect(),
        _ => Vec::new(),
    };
    let projects = cursor_recent
        .iter()
        .filter_map(|uri| workbench::uri::Uri::parse(uri)?.file_path())
        .filter(|path| path.is_dir())
        .collect();
    ImportPlan {
        settings,
        keybindings,
        projects,
    }
}

/// Cursor's recent folders, from its database, read-only.
pub(super) fn cursor_recent_folders(user_dir: &Path) -> Vec<String> {
    let database = user_dir.join("globalStorage/state.vscdb");
    if !database.exists() {
        return Vec::new();
    }
    let Ok(output) = Command::new("/usr/bin/sqlite3")
        .arg("-readonly")
        .arg(&database)
        .arg("select value from ItemTable where key = 'history.recentlyOpenedPathsList';")
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
    else {
        return Vec::new();
    };
    recent_folders(&String::from_utf8_lossy(&output.stdout))
}

/// The folders of `history.recentlyOpenedPathsList`, most recent first.
pub(super) fn recent_folders(json: &str) -> Vec<String> {
    serde_json::from_str::<Value>(json.trim())
        .ok()
        .and_then(|value| value["entries"].as_array().cloned())
        .into_iter()
        .flatten()
        .filter_map(|entry| entry["folderUri"].as_str().map(str::to_owned))
        .collect()
}

impl Workbench {
    /// Reads Cursor's files, then asks before importing what Keel lacks.
    pub(super) fn import_cursor_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let strings = self.strings();
        let Some(user_dir) = cursor_user_dir() else {
            self.status_message = Some(strings.cursor_not_found.to_owned());
            cx.notify();
            return;
        };
        let Some(keybindings_file) = self
            .data_dir
            .as_deref()
            .map(crate::paths::user_keybindings_file)
        else {
            return;
        };
        let read = |path: PathBuf| std::fs::read_to_string(path).unwrap_or_default();
        let plan = plan(
            &read(user_dir.join("settings.json")),
            &read(user_dir.join("keybindings.json")),
            &cursor_recent_folders(&user_dir),
            &self.settings.layer(settings::Source::User),
            &read(keybindings_file),
        );
        // Projects already in Keel's list are not news.
        let mut plan = plan;
        let known = crate::recent::projects(cx);
        plan.projects.retain(|project| !known.contains(project));
        if plan.is_empty() {
            self.status_message = Some(strings.cursor_nothing_new.to_owned());
            cx.notify();
            return;
        }
        let message = strings.cursor_import_question(
            plan.settings.len(),
            plan.keybindings.len(),
            plan.projects.len(),
        );
        let answer = window.prompt(
            PromptLevel::Info,
            &message,
            Some(strings.cursor_import_detail),
            &[strings.cursor_import_button, strings.cancel],
            cx,
        );
        cx.spawn_in(window, async move |this, cx| {
            if answer.await.ok() != Some(0) {
                return;
            }
            this.update_in(cx, |this, window, cx| {
                this.apply_cursor_import(plan, window, cx)
            })
            .ok();
        })
        .detach();
    }

    fn apply_cursor_import(
        &mut self,
        plan: ImportPlan,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let mut problems = Vec::new();
        let mut imported = Vec::new();
        for (key, value) in &plan.settings {
            if let Err(error) = self.settings.update_user(key, Some(value.clone())) {
                problems.push(format!("{key}: {error:#}"));
            } else {
                imported.push(key.clone());
            }
        }
        self.settings_changed(&imported, window, cx);
        if !plan.keybindings.is_empty()
            && let Some(file) = self
                .data_dir
                .as_deref()
                .map(crate::paths::user_keybindings_file)
        {
            let text = std::fs::read_to_string(&file).unwrap_or_default();
            match settings::edit::append_items(&text, &plan.keybindings)
                .and_then(|text| Ok(std::fs::write(&file, text)?))
            {
                Ok(()) => {
                    self.reload_user_keybindings();
                    self.bind_user_keybindings(cx);
                }
                Err(error) => problems.push(format!("keybindings.json: {error:#}")),
            }
        }
        if !plan.projects.is_empty() {
            crate::recent::add_older(&plan.projects, cx);
            crate::menus::set(self.language, cx);
        }
        let strings = self.strings();
        let done = strings.cursor_imported(
            plan.settings.len(),
            plan.keybindings.len(),
            plan.projects.len(),
        );
        for problem in &problems {
            self.log(problem.clone());
        }
        self.status_message = Some(if problems.is_empty() {
            done
        } else {
            format!("{done} {}", strings.cursor_import_problems)
        });
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn only_what_keel_lacks_comes_from_cursor() {
        let cursor_settings = "{\n  // mine\n  \"editor.minimap.enabled\": false,\n  \"files.autoSave\": \"afterDelay\",\n  \"workbench.colorTheme\": \"Cursor Dark\",\n  \"cursor.cpp.enable\": true,\n  \"vsauto.smartInput.enabled\": true,\n}";
        let cursor_keys = "// comment\n[\n  { \"key\": \"cmd+l\", \"command\": \"geminicodeassist.showinEditor\", \"when\": \"editorTextFocus\" },\n  { \"key\": \"cmd+i\", \"command\": \"-geminicodeassist.showinEditor\" },\n  { \"key\": \"cmd+k\" }\n]";
        let keel_settings: Map<String, Value> =
            serde_json::from_value(json!({ "files.autoSave": "off" })).unwrap();
        let keel_keys =
            "[ { \"key\": \"cmd+i\", \"command\": \"-geminicodeassist.showinEditor\" } ]";
        let here = std::env::temp_dir();
        let recent = [
            workbench::uri::Uri::file(&here).to_string(),
            "file:///nowhere/gone".to_owned(),
        ];
        let plan = plan(
            cursor_settings,
            cursor_keys,
            &recent,
            &keel_settings,
            keel_keys,
        );
        let keys: Vec<&str> = plan.settings.iter().map(|(key, _)| key.as_str()).collect();
        assert_eq!(
            keys,
            ["editor.minimap.enabled", "vsauto.smartInput.enabled"]
        );
        assert_eq!(plan.keybindings.len(), 1, "{:?}", plan.keybindings);
        assert_eq!(plan.keybindings[0]["when"], json!("editorTextFocus"));
        // The folder that is gone is left out.
        assert_eq!(plan.projects.len(), 1);
        assert!(plan.projects[0].is_dir());
    }

    #[test]
    fn cursor_recent_folders_from_its_database_value() {
        let value = r#"{"entries":[{"folderUri":"file:///Users/m/Keel"},{"fileUri":"file:///Users/m/notes.md"},{"folderUri":"file:///Users/m/Hondana"}]}"#;
        assert_eq!(
            recent_folders(value),
            ["file:///Users/m/Keel", "file:///Users/m/Hondana"]
        );
        assert!(recent_folders("").is_empty());
    }

    /// What would come from this Mac's Cursor into an empty Keel; only reads.
    /// Run with `cargo test -p keel -- --ignored --nocapture this_macs_cursor`.
    #[test]
    #[ignore]
    fn this_macs_cursor() {
        let Some(user_dir) = cursor_user_dir() else {
            println!("no Cursor");
            return;
        };
        let read = |name: &str| std::fs::read_to_string(user_dir.join(name)).unwrap_or_default();
        let recent = cursor_recent_folders(&user_dir);
        let plan = plan(
            &read("settings.json"),
            &read("keybindings.json"),
            &recent,
            &Map::new(),
            "",
        );
        println!(
            "settings {:?}\nkeybindings {}\nprojects {} of {} recent",
            plan.settings.iter().map(|(key, _)| key).collect::<Vec<_>>(),
            plan.keybindings.len(),
            plan.projects.len(),
            recent.len()
        );
    }
}
