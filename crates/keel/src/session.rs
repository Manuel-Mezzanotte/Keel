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
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct GroupState {
    pub tabs: Vec<TabState>,
    pub active: usize,
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
            }],
            sidebar: true,
            side_view: "explorer".to_owned(),
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
