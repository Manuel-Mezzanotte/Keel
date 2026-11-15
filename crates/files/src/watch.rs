//! Watching a project for changes made outside Keel, such as files written
//! by Claude Code, a formatter or `git checkout`.

use std::path::{Path, PathBuf};

use notify::{EventKind, RecursiveMode, Watcher as _};

/// Keeps the watch going; dropping it stops it.
pub struct ProjectWatcher {
    _watcher: notify::RecommendedWatcher,
}

/// Calls `changed` with the paths that changed under `root`. Inside `.git`
/// only `HEAD` counts, so a checkout is noticed but git's busywork is not.
pub fn watch(
    root: &Path,
    changed: impl Fn(Vec<PathBuf>) + Send + 'static,
) -> notify::Result<ProjectWatcher> {
    let git = root.join(".git");
    let head = git.join("HEAD");
    let mut watcher = notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
        let Ok(event) = event else {
            return;
        };
        if matches!(event.kind, EventKind::Access(_)) {
            return;
        }
        let paths: Vec<PathBuf> = event
            .paths
            .into_iter()
            .filter(|path| !path.starts_with(&git) || *path == head)
            .collect();
        if !paths.is_empty() {
            changed(paths);
        }
    })?;
    watcher.watch(root, RecursiveMode::Recursive)?;
    Ok(ProjectWatcher { _watcher: watcher })
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;
    use std::time::Duration;

    use super::*;

    #[test]
    fn changes_made_outside_are_reported() {
        let folder = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(folder.path()).unwrap();
        std::fs::create_dir_all(root.join(".git/objects")).unwrap();
        let (sender, receiver) = mpsc::channel();
        let _watcher = watch(&root, move |paths| {
            let _ = sender.send(paths);
        })
        .unwrap();
        // FSEvents starts a moment after the watch is set up.
        std::thread::sleep(Duration::from_millis(300));
        std::fs::write(root.join(".git/objects/ab"), "").unwrap();
        std::fs::write(root.join("app.ts"), "export {};\n").unwrap();

        let mut seen = Vec::new();
        while let Ok(paths) = receiver.recv_timeout(Duration::from_secs(5)) {
            seen.extend(paths);
            if seen.iter().any(|path| path.ends_with("app.ts")) {
                break;
            }
        }
        assert!(seen.iter().any(|path| path.ends_with("app.ts")), "{seen:?}");
        assert!(
            !seen
                .iter()
                .any(|path| path.starts_with(root.join(".git/objects")))
        );
    }
}
