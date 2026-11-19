//! The colours and letters git gives files in the Explorer, on the tabs and
//! in the other lists, as VS Code draws them: M for a changed file, U for a
//! new one, A for one added to the next commit, D, R and C. A folder takes
//! the strongest colour of what it holds, with a dot, up to the project, so
//! even a closed folder says there is something inside.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use gpui::{App, Hsla, hsla};
use gpui_component::ActiveTheme as _;

/// What git says of a file, from the weakest to the strongest: a folder
/// shows the strongest of its files.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(super) enum Mark {
    Renamed,
    Untracked,
    Added,
    Deleted,
    Modified,
    Conflict,
}

impl Mark {
    /// The file's mark, from both sides of its status: what changed on
    /// disk counts first, as in VS Code's Explorer.
    pub(super) fn of(file: &git::FileStatus) -> Option<Self> {
        use git::Change;
        if file.conflict {
            return Some(Self::Conflict);
        }
        let side = |change: Change| match change {
            Change::Unmodified => None,
            Change::Untracked => Some(Self::Untracked),
            Change::Added => Some(Self::Added),
            Change::Deleted => Some(Self::Deleted),
            Change::Renamed | Change::Copied => Some(Self::Renamed),
            Change::Modified | Change::TypeChanged => Some(Self::Modified),
        };
        side(file.worktree).or_else(|| side(file.index))
    }

    /// The letter beside the name.
    pub(super) fn letter(self) -> &'static str {
        match self {
            Self::Renamed => "R",
            Self::Untracked => "U",
            Self::Added => "A",
            Self::Deleted => "D",
            Self::Modified => "M",
            Self::Conflict => "C",
        }
    }

    /// The colour of the name and of its letter.
    pub(super) fn color(self, cx: &App) -> Hsla {
        let theme = cx.theme();
        match self {
            Self::Renamed => theme.info,
            Self::Untracked | Self::Added => theme.success,
            Self::Deleted => theme.danger,
            Self::Modified => theme.warning,
            // Violet, as VS Code's conflicts, distinct from a deletion.
            Self::Conflict => {
                if theme.mode.is_dark() {
                    hsla(0.74, 0.75, 0.72, 1.)
                } else {
                    hsla(0.74, 0.6, 0.5, 1.)
                }
            }
        }
    }
}

/// The marks of a project's files and folders, by their path as the
/// Explorer spells it.
#[derive(Default)]
pub(super) struct GitMarks {
    files: HashMap<PathBuf, Mark>,
    folders: HashMap<PathBuf, Mark>,
}

impl GitMarks {
    /// From git's status. `project` is the project's folder as Keel opened
    /// it; `real_project` the same folder with its links resolved, as git
    /// names it under `repo_root`. Only files inside the project are kept.
    pub(super) fn new(
        project: &Path,
        real_project: &Path,
        repo_root: &Path,
        status: &git::Status,
    ) -> Self {
        let mut marks = Self::default();
        for file in &status.files {
            let Some(mark) = Mark::of(file) else {
                continue;
            };
            let real = repo_root.join(&file.path);
            let Ok(inside) = real.strip_prefix(real_project) else {
                continue;
            };
            let path = project.join(inside);
            let mut folder = path.parent();
            while let Some(parent) = folder {
                if !parent.starts_with(project) {
                    break;
                }
                let strongest = marks.folders.entry(parent.to_path_buf()).or_insert(mark);
                *strongest = (*strongest).max(mark);
                if parent == project {
                    break;
                }
                folder = parent.parent();
            }
            marks.files.insert(path, mark);
        }
        marks
    }

    pub(super) fn file(&self, path: &Path) -> Option<Mark> {
        self.files.get(path).copied()
    }

    pub(super) fn folder(&self, path: &Path) -> Option<Mark> {
        self.folders.get(path).copied()
    }

    /// A file's mark, or a folder's.
    pub(super) fn of(&self, path: &Path, is_dir: bool) -> Option<Mark> {
        if is_dir {
            self.folder(path)
        } else {
            self.file(path)
        }
    }
}

#[cfg(test)]
mod tests {
    use git::{Change, FileStatus, Status};

    use super::*;

    fn file(path: &str, index: Change, worktree: Change) -> FileStatus {
        FileStatus {
            path: path.to_owned(),
            original: None,
            index,
            worktree,
            conflict: false,
        }
    }

    #[test]
    fn files_get_letters_and_folders_the_strongest_colour() {
        let status = Status {
            files: vec![
                file("app/src/a.ts", Change::Unmodified, Change::Modified),
                file("app/src/new.ts", Change::Unmodified, Change::Untracked),
                file("app/docs/b.md", Change::Added, Change::Unmodified),
                file("app/old.ts", Change::Deleted, Change::Unmodified),
                FileStatus {
                    conflict: true,
                    ..file("app/src/c.ts", Change::Modified, Change::Modified)
                },
                // Outside the project, opened at `app`.
                file("other/x.ts", Change::Unmodified, Change::Modified),
            ],
            ..Status::default()
        };
        // The project was opened through a link: git names the real folder.
        let marks = GitMarks::new(
            Path::new("/link/app"),
            Path::new("/real/repo/app"),
            Path::new("/real/repo"),
            &status,
        );
        let file = |path: &str| marks.file(Path::new(path)).map(Mark::letter);
        assert_eq!(file("/link/app/src/a.ts"), Some("M"));
        assert_eq!(file("/link/app/src/new.ts"), Some("U"));
        assert_eq!(file("/link/app/docs/b.md"), Some("A"));
        assert_eq!(file("/link/app/old.ts"), Some("D"));
        assert_eq!(file("/link/app/src/c.ts"), Some("C"));
        assert_eq!(file("/link/app/src/same.ts"), None);
        assert!(marks.file(Path::new("/link/other/x.ts")).is_none());

        let folder = |path: &str| marks.folder(Path::new(path));
        assert_eq!(folder("/link/app/src"), Some(Mark::Conflict));
        assert_eq!(folder("/link/app/docs"), Some(Mark::Added));
        // The project takes the strongest of all.
        assert_eq!(folder("/link/app"), Some(Mark::Conflict));
        assert_eq!(folder("/link"), None);
    }

    #[test]
    fn what_changed_on_disk_counts_before_the_index() {
        let both = file("a", Change::Added, Change::Modified);
        assert_eq!(Mark::of(&both), Some(Mark::Modified));
        let staged = file("a", Change::Renamed, Change::Unmodified);
        assert_eq!(Mark::of(&staged), Some(Mark::Renamed));
        let clean = file("a", Change::Unmodified, Change::Unmodified);
        assert_eq!(Mark::of(&clean), None);
    }
}
