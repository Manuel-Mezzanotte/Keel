//! Every file of a project, for Quick Open: what git ignores and what the
//! user hides are left out.

use std::path::Path;

use globset::{Glob, GlobSetBuilder};
use ignore::WalkBuilder;

/// The most files listed, so a huge folder cannot stall Keel.
pub const LIMIT: usize = 100_000;

/// The project's files as paths relative to `root`, with `/`, sorted.
pub fn list_files(root: &Path, excludes: &[String]) -> Vec<String> {
    let mut builder = GlobSetBuilder::new();
    for pattern in excludes {
        if let Ok(glob) = Glob::new(pattern) {
            builder.add(glob);
        }
    }
    let excluded = builder
        .build()
        .unwrap_or_else(|_| globset::GlobSet::empty());
    let base = root.to_path_buf();
    let mut files: Vec<String> = WalkBuilder::new(root)
        .hidden(false)
        .ignore(false)
        .filter_entry(move |entry| {
            let relative = entry.path().strip_prefix(&base).unwrap_or(entry.path());
            relative.as_os_str().is_empty() || !excluded.is_match(relative)
        })
        .build()
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_some_and(|kind| kind.is_file()))
        .filter_map(|entry| {
            entry
                .path()
                .strip_prefix(root)
                .ok()
                .map(|relative| relative.to_string_lossy().into_owned())
        })
        .take(LIMIT)
        .collect();
    files.sort();
    files
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    #[test]
    fn ignored_and_excluded_files_are_left_out() {
        let folder = tempfile::tempdir().unwrap();
        let root = folder.path();
        for dir in [".git", "src/components", "node_modules/react", "dist"] {
            fs::create_dir_all(root.join(dir)).unwrap();
        }
        for file in [
            ".gitignore",
            ".env.example",
            "src/app.ts",
            "src/components/card.tsx",
            "node_modules/react/index.js",
            "dist/bundle.js",
            ".DS_Store",
            ".git/HEAD",
        ] {
            fs::write(root.join(file), "").unwrap();
        }
        fs::write(root.join(".gitignore"), "node_modules\n").unwrap();
        let excludes: Vec<String> = crate::DEFAULT_EXCLUDES
            .iter()
            .map(|glob| glob.to_string())
            .chain(["**/dist".to_owned()])
            .collect();
        assert_eq!(
            list_files(root, &excludes),
            [
                ".env.example",
                ".gitignore",
                "src/app.ts",
                "src/components/card.tsx"
            ]
        );
    }
}

/// Times listing and searching a real project:
/// `KEEL_TIMING_PROJECT=../GestionaleHospitality cargo test -p files --release -- --ignored --nocapture`.
#[cfg(test)]
#[test]
#[ignore]
fn timing_on_a_real_project() {
    let Some(root) = std::env::var_os("KEEL_TIMING_PROJECT") else {
        return;
    };
    let excludes: Vec<String> = crate::DEFAULT_EXCLUDES
        .iter()
        .map(|glob| glob.to_string())
        .collect();
    let started = std::time::Instant::now();
    let files = list_files(Path::new(&root), &excludes);
    let listed = started.elapsed();
    let started = std::time::Instant::now();
    let matches = files
        .iter()
        .filter(|path| crate::fuzzy::score_path("bkcard", path).is_some())
        .count();
    println!(
        "{} files listed in {listed:?}; one search in {:?} ({matches} matches)",
        files.len(),
        started.elapsed()
    );
}
