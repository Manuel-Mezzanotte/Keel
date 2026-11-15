//! The project a window works on: its folder, its name and its git branch.

use std::path::{Path, PathBuf};

pub struct Project {
    /// The project's folder, or `None` for a window with only untitled files.
    pub root: Option<PathBuf>,
    /// The folder's name, shown in the toolbar.
    pub name: String,
    /// The checked-out branch, or the short commit when detached.
    pub branch: Option<String>,
}

impl Project {
    /// The project of a file: the nearest folder with `.git`, `.vscode` or
    /// `.keel`, otherwise the file's own folder.
    pub fn for_file(file: Option<&Path>) -> Self {
        let root = file
            .and_then(|file| std::path::absolute(file).ok())
            .and_then(|file| settings::project_root_for(&file));
        let name = root.as_deref().and_then(Path::file_name).map_or_else(
            || "Keel".to_owned(),
            |name| name.to_string_lossy().into_owned(),
        );
        let branch = root.as_deref().and_then(git_branch);
        Self { root, name, branch }
    }

    /// Reads the branch again, after a checkout made outside Keel.
    pub fn refresh_branch(&mut self) -> bool {
        let branch = self.root.as_deref().and_then(git_branch);
        let changed = branch != self.branch;
        self.branch = branch;
        changed
    }
}

/// The branch checked out in a git repository, read from `.git/HEAD`.
/// Worktrees, where `.git` is a file pointing elsewhere, are followed.
pub fn git_branch(root: &Path) -> Option<String> {
    let dot_git = root.join(".git");
    let git_dir = if dot_git.is_file() {
        let pointer = std::fs::read_to_string(&dot_git).ok()?;
        let target = pointer.trim().strip_prefix("gitdir:")?.trim();
        root.join(target)
    } else {
        dot_git
    };
    let head = std::fs::read_to_string(git_dir.join("HEAD")).ok()?;
    let head = head.trim();
    match head.strip_prefix("ref:") {
        Some(reference) => {
            let reference = reference.trim();
            Some(
                reference
                    .strip_prefix("refs/heads/")
                    .unwrap_or(reference)
                    .to_owned(),
            )
        }
        None if head.len() >= 7 && head.chars().all(|c| c.is_ascii_hexdigit()) => {
            Some(head[..7].to_owned())
        }
        None => None,
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    #[test]
    fn the_branch_comes_from_head() {
        let folder = tempfile::tempdir().unwrap();
        let root = folder.path().join("GestionaleHospitality");
        fs::create_dir_all(root.join(".git")).unwrap();
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(root.join(".git/HEAD"), "ref: refs/heads/feature/login\n").unwrap();

        let mut project = Project::for_file(Some(&root.join("src/page.tsx")));
        assert_eq!(project.root.as_deref(), Some(root.as_path()));
        assert_eq!(project.name, "GestionaleHospitality");
        assert_eq!(project.branch.as_deref(), Some("feature/login"));

        fs::write(
            root.join(".git/HEAD"),
            "6b27e95c1d0e8f7a2b3c4d5e6f708192a3b4c5d6\n",
        )
        .unwrap();
        assert!(project.refresh_branch());
        assert_eq!(project.branch.as_deref(), Some("6b27e95"));
        assert!(!project.refresh_branch());
    }

    #[test]
    fn worktrees_are_followed() {
        let folder = tempfile::tempdir().unwrap();
        let main = folder.path().join("main/.git/worktrees/fix");
        fs::create_dir_all(&main).unwrap();
        fs::write(main.join("HEAD"), "ref: refs/heads/fix\n").unwrap();
        let worktree = folder.path().join("fix");
        fs::create_dir_all(&worktree).unwrap();
        fs::write(
            worktree.join(".git"),
            "gitdir: ../main/.git/worktrees/fix\n",
        )
        .unwrap();
        assert_eq!(git_branch(&worktree).as_deref(), Some("fix"));
    }

    #[test]
    fn untitled_windows_have_no_project() {
        let project = Project::for_file(None);
        assert_eq!(project.root, None);
        assert_eq!(project.name, "Keel");
        assert_eq!(project.branch, None);
    }
}
