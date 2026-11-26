//! What Keel keeps between runs, in its data folder:
//!
//! - `sessions/`: each project's tabs, groups, layout and open folders;
//! - `backups/`: the unsaved changes of each window, written a moment
//!   after typing, so a crash or Cmd+Q loses nothing;
//! - `windows.json`: the projects that were open, to reopen at start.

use std::fs;
use std::hash::{DefaultHasher, Hash as _, Hasher as _};
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// A stable name for a project, or for windows without one.
pub fn key(root: Option<&Path>) -> String {
    match root {
        Some(root) => {
            let mut hasher = DefaultHasher::new();
            root.hash(&mut hasher);
            let name = root
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default();
            format!("{name}-{:016x}", hasher.finish())
        }
        None => "no-project".to_owned(),
    }
}

/// A project window's tabs and layout.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct WindowState {
    pub groups: Vec<GroupState>,
    pub active_group: usize,
    pub sidebar: bool,
    pub panel: bool,
    pub agent: bool,
    pub side_view: String,
    pub panel_view: String,
    /// The folders open in the Explorer.
    #[serde(default)]
    pub expanded: Vec<PathBuf>,
    /// How far down the Explorer was scrolled, in points.
    #[serde(default)]
    pub tree_scroll: Option<f32>,
    /// The bottom panel's height and Claude Code's width, in points.
    #[serde(default)]
    pub panel_height: Option<f32>,
    #[serde(default)]
    pub agent_width: Option<f32>,
    #[serde(default)]
    pub sidebar_width: Option<f32>,
    /// The height of Source Control's Graph section.
    #[serde(default)]
    pub graph_height: Option<f32>,
    /// Sections of the sidebar closed to their header, such as
    /// `"npm-scripts"`.
    #[serde(default)]
    pub collapsed: Vec<String>,
    /// The latest searches of the Search view.
    #[serde(default)]
    pub recent_searches: Vec<String>,
    /// The terminals of the bottom panel; what they showed is kept apart,
    /// in `terminals_dir`.
    #[serde(default)]
    pub terminals: Vec<TerminalState>,
    #[serde(default)]
    pub active_terminal: usize,
    /// The bookmarks of the project's files.
    #[serde(default)]
    pub bookmarks: Vec<BookmarkState>,
    /// The breakpoints, the watched expressions and the configuration F5
    /// starts.
    #[serde(default)]
    pub breakpoints: Vec<crate::workbench_window::Breakpoint>,
    #[serde(default)]
    pub watches: Vec<String>,
    #[serde(default)]
    pub debug_configuration: Option<String>,
    /// The pull requests page has been opened in the project.
    #[serde(default)]
    pub pulls: bool,
    /// The chat open in Claude's panel, to open again.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub claude_chat: Option<ChatState>,
}

/// The chat open in Claude's panel: Claude Code's id for it, and its folder
/// when it is another project's chat.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatState {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub folder: Option<PathBuf>,
}

/// A bookmark on a line, with the line's text to find it again after edits.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BookmarkState {
    pub path: PathBuf,
    /// From 0.
    pub line: u32,
    pub text: String,
}

/// A terminal of the bottom panel, as kept between runs.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TerminalState {
    /// The name the user gave it.
    #[serde(default)]
    pub name: Option<String>,
    /// The folder its shell was in.
    pub cwd: PathBuf,
    /// Terminals with the same group were side by side under one tab.
    #[serde(default)]
    pub group: usize,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct GroupState {
    pub tabs: Vec<TabState>,
    pub active: usize,
    /// Groups of the same column are stacked; sessions from before columns
    /// have none, and each group was a column of its own.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub column: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TabState {
    pub path: PathBuf,
    #[serde(default)]
    pub pinned: bool,
}

/// The unsaved text of a document.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Backup {
    pub uri: String,
    /// The file it belongs to, or none for an untitled document.
    pub path: Option<PathBuf>,
    pub language_id: String,
    pub text: String,
}

pub fn state_file(data: &Path, root: &Path) -> PathBuf {
    data.join("sessions")
        .join(format!("{}.json", key(Some(root))))
}

pub fn backup_dir(data: &Path, root: Option<&Path>) -> PathBuf {
    data.join("backups").join(key(root))
}

/// Where the text of a window's terminals waits for the next run.
pub fn terminals_dir(data: &Path, root: Option<&Path>) -> PathBuf {
    data.join("terminals").join(key(root))
}

pub fn windows_file(data: &Path) -> PathBuf {
    data.join("windows.json")
}

/// Writes a file in one step, so a crash never leaves half of it.
fn write_atomically(path: &Path, text: &str) -> io::Result<()> {
    if let Some(folder) = path.parent() {
        fs::create_dir_all(folder)?;
    }
    let temporary = path.with_extension("keel-save");
    fs::write(&temporary, text)?;
    fs::rename(&temporary, path)
}

fn read_json<T: for<'a> Deserialize<'a>>(path: &Path) -> Option<T> {
    serde_json::from_str(&fs::read_to_string(path).ok()?).ok()
}

pub fn load_state(file: &Path) -> Option<WindowState> {
    read_json(file)
}

pub fn save_state(file: &Path, state: &WindowState) -> io::Result<()> {
    write_atomically(file, &serde_json::to_string_pretty(state)?)
}

fn backup_file(dir: &Path, uri: &str) -> PathBuf {
    let mut hasher = DefaultHasher::new();
    uri.hash(&mut hasher);
    dir.join(format!("{:016x}.json", hasher.finish()))
}

pub fn write_backup(dir: &Path, backup: &Backup) -> io::Result<()> {
    write_atomically(
        &backup_file(dir, &backup.uri),
        &serde_json::to_string(backup)?,
    )
}

/// Deletes the backup of a document that was saved or discarded. A backup
/// that does not exist is fine.
pub fn remove_backup(dir: &Path, uri: &str) {
    let _ = fs::remove_file(backup_file(dir, uri));
}

pub fn read_backups(dir: &Path) -> Vec<Backup> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut backups: Vec<Backup> = entries
        .flatten()
        .filter(|entry| {
            entry
                .path()
                .extension()
                .is_some_and(|extension| extension == "json")
        })
        .filter_map(|entry| read_json(&entry.path()))
        .collect();
    backups.sort_by(|a, b| a.uri.cmp(&b.uri));
    backups
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct OpenWindows {
    windows: Vec<PathBuf>,
}

pub fn load_windows(data: &Path) -> Vec<PathBuf> {
    read_json::<OpenWindows>(&windows_file(data))
        .map(|open| open.windows)
        .unwrap_or_default()
}

pub fn save_windows(data: &Path, windows: &[PathBuf]) -> io::Result<()> {
    let open = OpenWindows {
        windows: windows.to_vec(),
    };
    write_atomically(&windows_file(data), &serde_json::to_string_pretty(&open)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn groups_saved_before_columns_still_read() {
        let old: GroupState =
            serde_json::from_str(r#"{ "tabs": [{ "path": "/a.rs" }], "active": 0 }"#).unwrap();
        assert_eq!(old.column, None);
        let new = GroupState {
            column: Some(2),
            ..old.clone()
        };
        let text = serde_json::to_string(&new).unwrap();
        assert!(text.contains(r#""column":2"#), "{text}");
        assert!(!serde_json::to_string(&old).unwrap().contains("column"));
    }

    #[test]
    fn states_backups_and_windows_survive_a_restart() {
        let data = tempfile::tempdir().unwrap();
        let root = Path::new("/Users/manuel/Desktop/MiniPCProgram/Keel");
        let state = WindowState {
            groups: vec![GroupState {
                tabs: vec![TabState {
                    path: root.join("src/main.rs"),
                    pinned: true,
                }],
                active: 0,
                column: Some(3),
            }],
            sidebar: true,
            side_view: "explorer".to_owned(),
            claude_chat: Some(ChatState {
                id: "b1f0c2d4-9e1a-4c56-8a2b-3f5e6d7c8b9a".to_owned(),
                folder: Some(PathBuf::from("/Users/manuel/Desktop/MiniPCProgram")),
            }),
            ..WindowState::default()
        };
        let file = state_file(data.path(), root);
        assert!(file.ends_with(format!("sessions/{}.json", key(Some(root)))));
        save_state(&file, &state).unwrap();
        assert_eq!(load_state(&file), Some(state));

        let dir = backup_dir(data.path(), Some(root));
        let backup = Backup {
            uri: "untitled:Untitled-1".to_owned(),
            path: None,
            language_id: "plaintext".to_owned(),
            text: "appunti".to_owned(),
        };
        write_backup(&dir, &backup).unwrap();
        assert_eq!(read_backups(&dir), std::slice::from_ref(&backup));
        remove_backup(&dir, &backup.uri);
        remove_backup(&dir, &backup.uri);
        assert!(read_backups(&dir).is_empty());

        save_windows(data.path(), &[root.to_path_buf()]).unwrap();
        assert_eq!(load_windows(data.path()), [root.to_path_buf()]);
    }

    #[test]
    fn projects_get_different_keys() {
        let keel = key(Some(Path::new("/a/Keel")));
        assert!(keel.starts_with("Keel-"));
        assert_ne!(keel, key(Some(Path::new("/b/Keel"))));
        assert_eq!(key(None), "no-project");
    }
}
