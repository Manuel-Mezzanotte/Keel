//! The tree of a project's files, as the Explorer shows it: folders read
//! when opened, folders before files, files git ignores marked.

use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use globset::{Glob, GlobSet, GlobSetBuilder};
use ignore::WalkBuilder;

/// The files VS Code hides by default, its `files.exclude`.
pub const DEFAULT_EXCLUDES: &[&str] = &[
    "**/.git",
    "**/.svn",
    "**/.hg",
    "**/CVS",
    "**/.DS_Store",
    "**/Thumbs.db",
];

#[derive(Clone, Debug, PartialEq, Eq)]
struct Entry {
    path: PathBuf,
    name: String,
    is_dir: bool,
    ignored: bool,
}

/// A line of the tree as shown.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    pub path: PathBuf,
    pub name: String,
    /// How many folders deep, 0 for the project's own files.
    pub depth: usize,
    pub is_dir: bool,
    pub expanded: bool,
    /// Ignored by git, so shown dimmed.
    pub ignored: bool,
}

pub struct FileTree {
    root: PathBuf,
    excluded: GlobSet,
    expanded: HashSet<PathBuf>,
    /// The contents of the folders read so far.
    listings: HashMap<PathBuf, Vec<Entry>>,
}

impl FileTree {
    /// The tree of a project. `excludes` are glob patterns of files to hide,
    /// such as `**/.git`, relative to the project.
    pub fn new(root: impl Into<PathBuf>, excludes: &[String]) -> Self {
        let root = root.into();
        let mut tree = Self {
            excluded: glob_set(excludes),
            expanded: HashSet::from([root.clone()]),
            listings: HashMap::new(),
            root,
        };
        tree.load(&tree.root.clone());
        tree
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The rows shown, top to bottom.
    pub fn rows(&self) -> Vec<Row> {
        let mut rows = Vec::new();
        self.collect(&self.root, 0, &mut rows);
        rows
    }

    fn collect(&self, folder: &Path, depth: usize, rows: &mut Vec<Row>) {
        for entry in self.listings.get(folder).into_iter().flatten() {
            let expanded = entry.is_dir && self.expanded.contains(&entry.path);
            rows.push(Row {
                path: entry.path.clone(),
                name: entry.name.clone(),
                depth,
                is_dir: entry.is_dir,
                expanded,
                ignored: entry.ignored,
            });
            if expanded {
                self.collect(&entry.path, depth + 1, rows);
            }
        }
    }

    pub fn is_expanded(&self, folder: &Path) -> bool {
        self.expanded.contains(folder)
    }

    /// Opens a closed folder, or closes an open one.
    pub fn toggle(&mut self, folder: &Path) {
        if folder != self.root && self.expanded.remove(folder) {
            return;
        }
        self.expand(folder);
    }

    pub fn expand(&mut self, folder: &Path) {
        self.expanded.insert(folder.to_path_buf());
        if !self.listings.contains_key(folder) {
            self.load(folder);
        }
    }

    pub fn collapse(&mut self, folder: &Path) {
        if folder != self.root {
            self.expanded.remove(folder);
        }
    }

    pub fn collapse_all(&mut self) {
        let root = self.root.clone();
        self.expanded.retain(|folder| *folder == root);
    }

    /// Opens the folders down to a file, so the file is in view.
    pub fn reveal(&mut self, file: &Path) {
        if !file.starts_with(&self.root) {
            return;
        }
        let mut folders: Vec<PathBuf> = file
            .ancestors()
            .skip(1)
            .take_while(|folder| folder.starts_with(&self.root))
            .map(Path::to_path_buf)
            .collect();
        folders.reverse();
        for folder in folders {
            self.expand(&folder);
        }
    }

    /// Reads the open folders again, after files changed.
    pub fn refresh(&mut self) {
        let mut folders: Vec<PathBuf> = self.expanded.iter().cloned().collect();
        folders.sort();
        self.listings.clear();
        for folder in folders {
            if folder.is_dir() {
                self.load(&folder);
            } else {
                self.expanded.remove(&folder);
            }
        }
    }

    /// Whether a shown folder is ignored, from its parent's listing.
    fn is_ignored(&self, folder: &Path) -> bool {
        folder
            .parent()
            .and_then(|parent| self.listings.get(parent))
            .and_then(|entries| entries.iter().find(|entry| entry.path == folder))
            .is_some_and(|entry| entry.ignored)
    }

    fn load(&mut self, folder: &Path) {
        // Everything inside an ignored folder is ignored too. Otherwise the
        // walker answers which files git does not ignore, following every
        // .gitignore up to the repository and the global excludes.
        let folder_ignored = self.is_ignored(folder);
        let kept: HashSet<PathBuf> = WalkBuilder::new(folder)
            .max_depth(Some(1))
            .hidden(false)
            .ignore(false)
            .build()
            .filter_map(Result::ok)
            .map(ignore::DirEntry::into_path)
            .collect();
        let mut entries: Vec<Entry> = fs::read_dir(folder)
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|item| {
                let path = item.path();
                let relative = path.strip_prefix(&self.root).unwrap_or(&path);
                if self.excluded.is_match(relative) {
                    return None;
                }
                let is_dir = path.is_dir();
                Some(Entry {
                    name: item.file_name().to_string_lossy().into_owned(),
                    ignored: folder_ignored || !kept.contains(&path),
                    path,
                    is_dir,
                })
            })
            .collect();
        entries.sort_by(|a, b| {
            b.is_dir
                .cmp(&a.is_dir)
                .then_with(|| compare_names(&a.name, &b.name))
        });
        self.listings.insert(folder.to_path_buf(), entries);
    }
}

fn glob_set(patterns: &[String]) -> GlobSet {
    let mut builder = GlobSetBuilder::new();
    for pattern in patterns {
        if let Ok(glob) = Glob::new(pattern) {
            builder.add(glob);
        }
    }
    builder.build().unwrap_or_else(|_| GlobSet::empty())
}

/// Orders names as Finder and VS Code do: ignoring case, and numbers by
/// value, so `file2` comes before `file10`.
fn compare_names(a: &str, b: &str) -> Ordering {
    let key = |name: &str| {
        let mut parts: Vec<(String, u128)> = Vec::new();
        let mut text = String::new();
        let mut number = String::new();
        for character in name.chars() {
            if character.is_ascii_digit() {
                number.push(character);
            } else {
                if !number.is_empty() {
                    parts.push((
                        std::mem::take(&mut text),
                        number.parse().unwrap_or(u128::MAX),
                    ));
                    number.clear();
                }
                text.extend(character.to_lowercase());
            }
        }
        parts.push((text, number.parse().unwrap_or(0)));
        parts
    };
    key(a).cmp(&key(b)).then_with(|| a.cmp(b))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project() -> (tempfile::TempDir, PathBuf) {
        let folder = tempfile::tempdir().unwrap();
        let root = fs::canonicalize(folder.path()).unwrap().join("Hospitality");
        for dir in [".git", "src/components", "node_modules/react", "public"] {
            fs::create_dir_all(root.join(dir)).unwrap();
        }
        for file in [
            "README.md",
            "package.json",
            ".gitignore",
            ".env",
            ".DS_Store",
            "src/page10.tsx",
            "src/page2.tsx",
            "src/components/Card.tsx",
            "node_modules/react/index.js",
        ] {
            fs::write(root.join(file), "").unwrap();
        }
        fs::write(root.join(".gitignore"), "node_modules\n.env\n").unwrap();
        (folder, root)
    }

    fn excludes() -> Vec<String> {
        DEFAULT_EXCLUDES
            .iter()
            .map(|glob| glob.to_string())
            .collect()
    }

    fn names(tree: &FileTree) -> Vec<String> {
        tree.rows()
            .iter()
            .map(|row| format!("{}{}", "  ".repeat(row.depth), row.name))
            .collect()
    }

    #[test]
    fn folders_come_first_and_excluded_files_are_hidden() {
        let (_folder, root) = project();
        let tree = FileTree::new(&root, &excludes());
        assert_eq!(
            names(&tree),
            [
                "node_modules",
                "public",
                "src",
                ".env",
                ".gitignore",
                "package.json",
                "README.md"
            ]
        );
        let ignored: Vec<_> = tree
            .rows()
            .into_iter()
            .filter(|row| row.ignored)
            .map(|row| row.name)
            .collect();
        assert_eq!(ignored, ["node_modules", ".env"]);
    }

    #[test]
    fn folders_open_and_close_and_numbers_sort_by_value() {
        let (_folder, root) = project();
        let mut tree = FileTree::new(&root, &excludes());
        tree.toggle(&root.join("src"));
        tree.toggle(&root.join("node_modules"));
        tree.toggle(&root.join("node_modules/react"));
        assert_eq!(
            names(&tree),
            [
                "node_modules",
                "  react",
                "    index.js",
                "public",
                "src",
                "  components",
                "  page2.tsx",
                "  page10.tsx",
                ".env",
                ".gitignore",
                "package.json",
                "README.md"
            ]
        );
        // Files inside an ignored folder are ignored too.
        let react = tree
            .rows()
            .into_iter()
            .find(|row| row.name == "index.js")
            .unwrap();
        assert!(react.ignored);

        tree.toggle(&root.join("node_modules"));
        assert!(!names(&tree).contains(&"  react".to_owned()));
        tree.collapse_all();
        assert_eq!(tree.rows().len(), 7);
    }

    #[test]
    fn revealing_a_file_opens_its_folders_and_refresh_sees_new_files() {
        let (_folder, root) = project();
        let mut tree = FileTree::new(&root, &excludes());
        tree.reveal(&root.join("src/components/Card.tsx"));
        assert!(tree.is_expanded(&root.join("src/components")));
        assert!(names(&tree).contains(&"    Card.tsx".to_owned()));

        fs::write(root.join("src/components/Badge.tsx"), "").unwrap();
        fs::remove_dir_all(root.join("public")).unwrap();
        tree.refresh();
        let shown = names(&tree);
        assert!(shown.contains(&"    Badge.tsx".to_owned()));
        assert!(!shown.contains(&"public".to_owned()));
    }

    #[test]
    fn user_excludes_hide_more() {
        let (_folder, root) = project();
        let mut patterns = excludes();
        patterns.push("**/node_modules".to_owned());
        let tree = FileTree::new(&root, &patterns);
        assert!(!names(&tree).contains(&"node_modules".to_owned()));
    }
}
