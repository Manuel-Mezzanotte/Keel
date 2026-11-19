//! Local history, as VS Code's: each save of a file keeps a copy in Keel's
//! data folder, so an earlier version can be compared and brought back
//! even without a commit, and a deleted file found again. Saves less than
//! ten seconds apart count as one; each file keeps its last fifty.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use super::notices::{Notice, fill};
use super::*;
use crate::quick_pick::{self, PickItem};

actions!(
    keel,
    [ShowLocalHistory, FindDeletedFile, CompareWithClipboard]
);

/// The local versions of a file, from the Explorer's menu.
#[derive(Clone, Debug, PartialEq, gpui::Action)]
#[action(namespace = explorer, no_json)]
pub struct ShowLocalHistoryOf {
    pub path: PathBuf,
}

/// One kept version: its file in the history folder, and when it was saved.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Version {
    pub(super) file: PathBuf,
    /// Milliseconds since 1970.
    pub(super) at: u128,
}

/// Where a file's versions are kept.
pub(super) fn folder_of(data: &Path, path: &Path) -> PathBuf {
    use std::hash::{DefaultHasher, Hash, Hasher};
    let mut hasher = DefaultHasher::new();
    path.hash(&mut hasher);
    data.join("history")
        .join(format!("{:016x}", hasher.finish()))
}

/// A file's versions, the latest first.
pub(super) fn versions(folder: &Path) -> Vec<Version> {
    let Ok(entries) = std::fs::read_dir(folder) else {
        return Vec::new();
    };
    let mut versions: Vec<Version> = entries
        .flatten()
        .filter_map(|entry| {
            let file = entry.path();
            let at = file.file_stem()?.to_str()?.parse().ok()?;
            (file.extension()? == "txt").then_some(Version { file, at })
        })
        .collect();
    versions.sort_by_key(|version| std::cmp::Reverse(version.at));
    versions
}

/// Keeps `text` as the newest version of `path`: replaces a version saved
/// less than `merge` milliseconds ago, skips one equal to the last, and
/// drops the oldest beyond `most`.
pub(super) fn record(
    data: &Path,
    path: &Path,
    text: &str,
    now: u128,
    merge: u128,
    most: usize,
) -> std::io::Result<()> {
    let folder = folder_of(data, path);
    std::fs::create_dir_all(&folder)?;
    std::fs::write(folder.join("path"), path.to_string_lossy().as_bytes())?;
    let existing = versions(&folder);
    if let Some(last) = existing.first() {
        if std::fs::read_to_string(&last.file).is_ok_and(|kept| kept == text) {
            return Ok(());
        }
        if now.saturating_sub(last.at) < merge {
            std::fs::remove_file(&last.file)?;
        }
    }
    std::fs::write(folder.join(format!("{now}.txt")), text)?;
    for old in versions(&folder).into_iter().skip(most) {
        let _ = std::fs::remove_file(old.file);
    }
    Ok(())
}

/// The files with versions kept that are no longer on disk.
pub(super) fn deleted_files(data: &Path) -> Vec<(PathBuf, PathBuf)> {
    let Ok(entries) = std::fs::read_dir(data.join("history")) else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter_map(|entry| {
            let folder = entry.path();
            let path = PathBuf::from(std::fs::read_to_string(folder.join("path")).ok()?);
            (!path.exists()).then_some((path, folder))
        })
        .collect()
}

fn now_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |time| time.as_millis())
}

impl Workbench {
    pub(super) fn local_history_actions(
        &self,
        root: gpui::Div,
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        root.on_action(cx.listener(|this, _: &ShowLocalHistory, window, cx| {
            if let Some(path) = this.active_disk_path() {
                this.show_local_history(path, window, cx)
            }
        }))
        .on_action(
            cx.listener(|this, action: &ShowLocalHistoryOf, window, cx| {
                this.show_local_history(action.path.clone(), window, cx)
            }),
        )
        .on_action(
            cx.listener(|this, _: &FindDeletedFile, window, cx| this.find_deleted_file(window, cx)),
        )
        .on_action(cx.listener(|this, _: &CompareWithClipboard, window, cx| {
            this.compare_with_clipboard(window, cx)
        }))
    }

    fn active_disk_path(&self) -> Option<PathBuf> {
        let pane = self.panes.get(self.active)?;
        Some(self.documents.get(&pane.uri)?.disk_path()?.to_path_buf())
    }

    /// After a save: keeps the text as a version of the file.
    pub(super) fn record_local_history(&self, path: &Path, text: &str) {
        let on = self
            .settings
            .get("workbench.localHistory.enabled")
            .and_then(Value::as_bool)
            .unwrap_or(true);
        let Some(data) = self.data_dir.as_ref().filter(|_| on) else {
            return;
        };
        let largest = self
            .settings
            .number("workbench.localHistory.maxFileSize")
            .unwrap_or(256.)
            * 1024.;
        if text.len() as f64 > largest {
            return;
        }
        let most = self
            .settings
            .number("workbench.localHistory.maxFileEntries")
            .unwrap_or(50.) as usize;
        let merge = self
            .settings
            .number("workbench.localHistory.mergeWindow")
            .unwrap_or(10.)
            * 1000.;
        if let Err(error) = record(data, path, text, now_millis(), merge as u128, most.max(1)) {
            eprintln!("keel: local history of {}: {error}", path.display());
        }
    }

    /// The versions of a file, to compare one with the file now.
    pub(super) fn show_local_history(
        &mut self,
        path: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let strings = self.strings();
        let Some(data) = self.data_dir.clone() else {
            return;
        };
        let versions = versions(&folder_of(&data, &path));
        if versions.is_empty() {
            self.notify_user(Notice::info(strings.history_none), window, cx);
            return;
        }
        let now = now_millis();
        let items = versions
            .iter()
            .map(|version| {
                let text = std::fs::read_to_string(&version.file).unwrap_or_default();
                let mut item = PickItem::new(notices::ago(
                    (version.at / 1000) as i64,
                    (now / 1000) as i64,
                    strings,
                ));
                item.detail = Some(fill(
                    strings.history_lines,
                    &[("count", &text.lines().count().to_string())],
                ));
                item
            })
            .collect();
        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        let title = fill(strings.history_title, &[("name", &name)]);
        let chosen = quick_pick::show(None, Some(title), items, window, cx);
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Some(picked)) = chosen.await else {
                return;
            };
            let version = versions[picked].clone();
            this.update_in(cx, |this, window, cx| {
                this.open_diff(
                    &path.to_string_lossy(),
                    super::diff_view::DiffSource::LocalHistory(
                        version.file.to_string_lossy().into_owned(),
                    ),
                    window,
                    cx,
                )
            })
            .ok();
        })
        .detach();
    }

    /// Writes an old version back over the file, through its editor when
    /// it is open, so Undo still works.
    pub(super) fn restore_version(
        &mut self,
        path: &Path,
        version: &Path,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Ok(text) = std::fs::read_to_string(version) else {
            return;
        };
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let uri = Uri::file(path).to_string();
        if !self.documents.contains_key(&uri) && std::fs::write(path, &text).is_ok() {
            self.open_file(path, window, cx);
        } else {
            self.write_file_text(path, &text, window, cx);
        }
        let strings = self.strings();
        self.notify_user(Notice::success(strings.history_restored), window, cx);
        self.refresh_tree(cx);
    }

    /// Local History: Find Entry to Restore — a deleted file, then its version.
    fn find_deleted_file(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let strings = self.strings();
        let Some(data) = self.data_dir.clone() else {
            return;
        };
        let root = self.project.root.clone();
        let files: Vec<(PathBuf, PathBuf)> = deleted_files(&data)
            .into_iter()
            .filter(|(path, _)| root.as_ref().is_none_or(|root| path.starts_with(root)))
            .collect();
        if files.is_empty() {
            self.notify_user(Notice::info(strings.history_no_deleted), window, cx);
            return;
        }
        let items = files
            .iter()
            .map(|(path, _)| {
                let shown = root
                    .as_ref()
                    .and_then(|root| path.strip_prefix(root).ok())
                    .unwrap_or(path);
                PickItem::new(shown.to_string_lossy().into_owned())
            })
            .collect();
        let chosen = quick_pick::show(
            None,
            Some(strings.history_deleted_title.to_owned()),
            items,
            window,
            cx,
        );
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Some(picked)) = chosen.await else {
                return;
            };
            let (path, folder) = files[picked].clone();
            this.update_in(cx, |this, window, cx| {
                if let Some(latest) = versions(&folder).first() {
                    this.restore_version(&path, &latest.file, window, cx);
                }
            })
            .ok();
        })
        .detach();
    }

    /// File: Compare Active File with Clipboard.
    fn compare_with_clipboard(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(pane) = self.panes.get(self.active) else {
            return;
        };
        let Some(document) = self.documents.get(&pane.uri) else {
            return;
        };
        let name = document
            .disk_path()
            .map(|path| path.to_string_lossy().into_owned())
            .unwrap_or_else(|| self.display_name(document));
        let clipboard = cx
            .read_from_clipboard()
            .and_then(|item| item.text())
            .unwrap_or_default();
        self.clipboard_text = clipboard;
        self.open_diff(&name, super::diff_view::DiffSource::Clipboard, window, cx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_are_kept_merged_and_counted() {
        let data = tempfile::tempdir().unwrap();
        let path = Path::new("/Users/me/app/src/a.ts");
        record(data.path(), path, "uno", 1_000, 10_000, 3).unwrap();
        // The same text again: nothing new.
        record(data.path(), path, "uno", 50_000, 10_000, 3).unwrap();
        // Later: a version of its own.
        record(data.path(), path, "due", 55_000, 10_000, 3).unwrap();
        // Three seconds after: it replaces the one before.
        record(data.path(), path, "due!", 58_000, 10_000, 3).unwrap();
        let kept = versions(&folder_of(data.path(), path));
        assert_eq!(
            kept.iter().map(|version| version.at).collect::<Vec<_>>(),
            [58_000, 1_000]
        );
        assert_eq!(std::fs::read_to_string(&kept[0].file).unwrap(), "due!");
        for (index, text) in ["tre", "quattro", "cinque"].iter().enumerate() {
            record(
                data.path(),
                path,
                text,
                100_000 + index as u128 * 20_000,
                10_000,
                3,
            )
            .unwrap();
        }
        let kept = versions(&folder_of(data.path(), path));
        assert_eq!(kept.len(), 3, "the oldest went");
        assert_eq!(std::fs::read_to_string(&kept[0].file).unwrap(), "cinque");
        // The file is gone from disk: it is found among the deleted.
        let deleted = deleted_files(data.path());
        assert_eq!(deleted.len(), 1);
        assert_eq!(deleted[0].0, path);
    }
}
