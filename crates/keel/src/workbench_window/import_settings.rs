//! "Import from VS Code" and "Import from Cursor": their settings, keyboard
//! shortcuts, snippets and recent projects, from their folder in
//! `~/Library/Application Support`. Nothing of Keel's is overwritten:
//! settings Keel already has stay, shortcuts already there are not doubled,
//! a snippet file Keel has keeps its own, and Keel's recent projects stay
//! first. The other editor's files are only read; its database through
//! macOS's `sqlite3`, read-only.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde_json::{Map, Value};

use super::other_editor::OtherEditor;
use super::*;

/// What runs once an import is over, imported or not.
pub(super) type AfterImport = Box<dyn FnOnce(&mut Workbench, &mut Window, &mut Context<Workbench>)>;

/// What would come from the other editor.
#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct ImportPlan {
    pub(super) settings: Vec<(String, Value)>,
    pub(super) keybindings: Vec<Value>,
    /// Snippet files Keel has none of by that name: copied as they are.
    pub(super) snippets: Vec<PathBuf>,
    pub(super) projects: Vec<PathBuf>,
}

impl ImportPlan {
    pub(super) fn is_empty(&self) -> bool {
        self.settings.is_empty()
            && self.keybindings.is_empty()
            && self.snippets.is_empty()
            && self.projects.is_empty()
    }

    /// Settings, shortcuts, snippet files and projects, as the words say them.
    pub(super) fn counts(&self) -> [usize; 4] {
        [
            self.settings.len(),
            self.keybindings.len(),
            self.snippets.len(),
            self.projects.len(),
        ]
    }
}

/// The snippet files of `folder` (`*.json`, `*.code-snippets`) that
/// `keel_snippets` has none of by the same name.
pub(super) fn new_snippets(folder: &Path, keel_snippets: &Path) -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = std::fs::read_dir(folder)
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.is_file()
                && path
                    .extension()
                    .is_some_and(|extension| extension == "json" || extension == "code-snippets")
        })
        .filter(|path| {
            path.file_name()
                .is_some_and(|name| !keel_snippets.join(name).exists())
        })
        .collect();
    found.sort();
    found
}

/// What to import, given the other editor's files, Keel's files as they
/// are, and the other editor's recent folders (`folderUri` entries).
pub(super) fn plan(
    editor: OtherEditor,
    other_settings: &str,
    other_keybindings: &str,
    other_recent: &[String],
    keel_settings: &Map<String, Value>,
    keel_keybindings: &str,
) -> ImportPlan {
    let settings = match settings::parse_jsonc(other_settings) {
        Ok(Value::Object(values)) => values
            .into_iter()
            .filter(|(key, _)| !editor.skips(key) && !keel_settings.contains_key(key))
            .collect(),
        _ => Vec::new(),
    };
    let known: Vec<Value> = match settings::parse_jsonc(keel_keybindings) {
        Ok(Value::Array(entries)) => entries,
        _ => Vec::new(),
    };
    let same = |a: &Value, b: &Value| a["key"] == b["key"] && a["command"] == b["command"];
    let keybindings = match settings::parse_jsonc(other_keybindings) {
        Ok(Value::Array(entries)) => entries
            .into_iter()
            .filter(|entry| entry["key"].is_string() && entry["command"].is_string())
            .filter(|entry| !known.iter().any(|known| same(known, entry)))
            .collect(),
        _ => Vec::new(),
    };
    let projects = other_recent
        .iter()
        .filter_map(|uri| workbench::uri::Uri::parse(uri)?.file_path())
        .filter(|path| path.is_dir())
        .collect();
    ImportPlan {
        settings,
        keybindings,
        snippets: Vec::new(),
        projects,
    }
}

/// The other editor's recent folders, most recent first: from its
/// database, read-only, or else from its File menu's «Open Recent», which
/// newer versions of VS Code keep in `storage.json`.
pub(super) fn recent_folders_of(user_dir: &Path) -> Vec<String> {
    let storage = user_dir.join("globalStorage");
    let database = storage.join("state.vscdb");
    if database.exists() {
        let query = "select value from ItemTable where key = 'history.recentlyOpenedPathsList';";
        // The editor running holds its database: `immutable` reads it anyway.
        let names = [
            database.display().to_string(),
            format!("file:{}?immutable=1", database.display()),
        ];
        for name in names {
            let Ok(output) = Command::new("/usr/bin/sqlite3")
                .arg("-readonly")
                .arg(&name)
                .arg(query)
                .stdin(Stdio::null())
                .stderr(Stdio::null())
                .output()
            else {
                continue;
            };
            let found = recent_folders(&String::from_utf8_lossy(&output.stdout));
            if output.status.success() && !found.is_empty() {
                return found;
            }
        }
    }
    std::fs::read_to_string(storage.join("storage.json"))
        .map(|text| menu_recent_folders(&text))
        .unwrap_or_default()
}

/// The folders of «Open Recent» in the File menu that `storage.json` keeps
/// for the next start, as `file:` addresses.
pub(super) fn menu_recent_folders(json: &str) -> Vec<String> {
    fn walk(items: &[Value], found: &mut Vec<String>) {
        for item in items {
            if item["id"] == "openRecentFolder"
                && item["uri"]["scheme"] == "file"
                && let Some(path) = item["uri"]["path"].as_str()
            {
                found.push(workbench::uri::Uri::file(Path::new(path)).to_string());
            }
            if let Some(items) = item["submenu"]["items"].as_array() {
                walk(items, found);
            }
        }
    }
    let Ok(value) = serde_json::from_str::<Value>(json) else {
        return Vec::new();
    };
    let mut found = Vec::new();
    if let Some(items) = value["lastKnownMenubarData"]["menus"]["File"]["items"].as_array() {
        walk(items, &mut found);
    }
    found
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
    /// Reads the other editor's files, then asks before importing what
    /// Keel lacks. `then` runs once it is over, imported or not.
    pub(super) fn import_settings_from(
        &mut self,
        editor: OtherEditor,
        then: Option<AfterImport>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let strings = self.strings();
        let Some(user_dir) = editor.user_dir() else {
            self.status_message = Some(strings.import_not_found(editor.name()));
            cx.notify();
            if let Some(then) = then {
                then(self, window, cx);
            }
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
        let mut plan = plan(
            editor,
            &read(user_dir.join("settings.json")),
            &read(user_dir.join("keybindings.json")),
            &recent_folders_of(&user_dir),
            &self.settings.layer(settings::Source::User),
            &read(keybindings_file),
        );
        if let Some(data) = self.data_dir.as_deref() {
            plan.snippets = new_snippets(&user_dir.join("snippets"), &data.join("snippets"));
        }
        // Projects already in Keel's list are not news.
        let known = crate::recent::projects(cx);
        plan.projects.retain(|project| !known.contains(project));
        if plan.is_empty() {
            self.status_message = Some(strings.import_nothing_new(editor.name()));
            cx.notify();
            if let Some(then) = then {
                then(self, window, cx);
            }
            return;
        }
        let message = strings.import_question(editor.name(), plan.counts());
        let answer = window.prompt(
            PromptLevel::Info,
            &message,
            Some(strings.cursor_import_detail),
            &[strings.cursor_import_button, strings.cancel],
            cx,
        );
        cx.spawn_in(window, async move |this, cx| {
            let yes = answer.await.ok() == Some(0);
            this.update_in(cx, |this, window, cx| {
                if yes {
                    this.apply_import(editor, plan, window, cx);
                }
                if let Some(then) = then {
                    then(this, window, cx);
                }
            })
            .ok();
        })
        .detach();
    }

    fn apply_import(
        &mut self,
        editor: OtherEditor,
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
        if !plan.snippets.is_empty()
            && let Some(data) = self.data_dir.clone()
        {
            let folder = data.join("snippets");
            let _ = std::fs::create_dir_all(&folder);
            for file in &plan.snippets {
                let Some(name) = file.file_name() else {
                    continue;
                };
                if let Err(error) = std::fs::copy(file, folder.join(name)) {
                    problems.push(format!("{}: {error}", file.display()));
                }
            }
        }
        if !plan.projects.is_empty() {
            crate::recent::add_older(&plan.projects, cx);
            crate::menus::set(self.language, cx);
        }
        let strings = self.strings();
        let done = strings.imported(editor.name(), plan.counts());
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
            OtherEditor::Cursor,
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

    #[test]
    fn vs_code_recent_folders_from_its_file_menu() {
        let json = r#"{"lastKnownMenubarData":{"menus":{"File":{"items":[
            {"id":"workbench.action.files.newUntitledFile","label":"&&New Text File"},
            {"id":"submenuitem.MenubarRecentMenu","label":"Open &&Recent","submenu":{"items":[
                {"id":"openRecentFolder","uri":{"$mid":1,"path":"/Users/m/Keel","scheme":"file"},"label":"~/Keel"},
                {"id":"openRecentFolder","uri":{"$mid":1,"path":"/srv/app","scheme":"vscode-remote"},"label":"app [SSH]"},
                {"id":"openRecentFile","uri":{"$mid":1,"path":"/Users/m/notes.md","scheme":"file"},"label":"notes.md"},
                {"id":"openRecentFolder","uri":{"$mid":1,"path":"/Users/m/My Site","scheme":"file"},"label":"~/My Site"}
            ]}}
        ]}}}}"#;
        let folders = menu_recent_folders(json);
        assert_eq!(folders.len(), 2, "{folders:?}");
        assert!(folders[0].ends_with("/Users/m/Keel"));
        assert!(folders[1].contains("My%20Site"), "{folders:?}");
        assert!(menu_recent_folders("not json").is_empty());
    }

    #[test]
    fn snippet_files_keel_has_keep_its_own() {
        let other = tempfile::tempdir().unwrap();
        let keel = tempfile::tempdir().unwrap();
        for name in [
            "rust.json",
            "web.code-snippets",
            "typescript.json",
            "notes.txt",
        ] {
            std::fs::write(other.path().join(name), "{}").unwrap();
        }
        std::fs::write(keel.path().join("typescript.json"), "{}").unwrap();
        let names: Vec<String> = new_snippets(other.path(), keel.path())
            .iter()
            .map(|path| path.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, ["rust.json", "web.code-snippets"]);
        assert!(new_snippets(&other.path().join("missing"), keel.path()).is_empty());
    }

    /// What would come from this Mac's VS Code and Cursor into an empty
    /// Keel; only reads. Run with
    /// `cargo test -p keel -- --ignored --nocapture this_macs_editors`.
    #[test]
    #[ignore]
    fn this_macs_editors() {
        for editor in OtherEditor::ALL {
            let Some(user_dir) = editor.user_dir() else {
                println!("{}: not here", editor.name());
                continue;
            };
            let read =
                |name: &str| std::fs::read_to_string(user_dir.join(name)).unwrap_or_default();
            let recent = recent_folders_of(&user_dir);
            let mut plan = plan(
                editor,
                &read("settings.json"),
                &read("keybindings.json"),
                &recent,
                &Map::new(),
                "",
            );
            plan.snippets =
                new_snippets(&user_dir.join("snippets"), Path::new("/nonexistent-keel"));
            println!(
                "{}: settings {:?}\nkeybindings {}\nsnippets {:?}\nprojects {} of {} recent",
                editor.name(),
                plan.settings.iter().map(|(key, _)| key).collect::<Vec<_>>(),
                plan.keybindings.len(),
                plan.snippets,
                plan.projects.len(),
                recent.len()
            );
        }
    }
}
