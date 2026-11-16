//! What `git status --porcelain=v2` says: the branch, how far it is from
//! its upstream, and each changed file in the index and the working tree.

/// A file's state on one side: in the index or in the working tree.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Change {
    Unmodified,
    Modified,
    Added,
    Deleted,
    Renamed,
    Copied,
    TypeChanged,
    Untracked,
}

impl Change {
    fn from_code(code: u8) -> Self {
        match code {
            b'M' => Change::Modified,
            b'A' => Change::Added,
            b'D' => Change::Deleted,
            b'R' => Change::Renamed,
            b'C' => Change::Copied,
            b'T' => Change::TypeChanged,
            _ => Change::Unmodified,
        }
    }

    /// The letter VS Code shows beside the file.
    pub fn letter(self) -> &'static str {
        match self {
            Change::Unmodified => "",
            Change::Modified => "M",
            Change::Added => "A",
            Change::Deleted => "D",
            Change::Renamed => "R",
            Change::Copied => "C",
            Change::TypeChanged => "T",
            Change::Untracked => "U",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileStatus {
    /// The path from the repository's top folder, with `/`.
    pub path: String,
    /// Where a renamed or copied file came from.
    pub original: Option<String>,
    pub index: Change,
    pub worktree: Change,
    /// Both sides changed it in a merge, and it waits to be resolved.
    pub conflict: bool,
}

impl FileStatus {
    pub fn is_staged(&self) -> bool {
        !self.conflict && !matches!(self.index, Change::Unmodified | Change::Untracked)
    }

    pub fn is_changed(&self) -> bool {
        !self.conflict && self.worktree != Change::Unmodified
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Status {
    /// The branch checked out; `None` when HEAD is detached.
    pub branch: Option<String>,
    /// The commit checked out, abbreviated; `None` before the first commit.
    pub head: Option<String>,
    pub upstream: Option<String>,
    pub ahead: u32,
    pub behind: u32,
    pub files: Vec<FileStatus>,
}

impl Status {
    pub fn staged(&self) -> impl Iterator<Item = &FileStatus> {
        self.files.iter().filter(|file| file.is_staged())
    }

    pub fn changed(&self) -> impl Iterator<Item = &FileStatus> {
        self.files.iter().filter(|file| file.is_changed())
    }

    pub fn conflicts(&self) -> impl Iterator<Item = &FileStatus> {
        self.files.iter().filter(|file| file.conflict)
    }

    /// The number VS Code shows on the Source Control icon: every file
    /// with changes, staged or not.
    pub fn count(&self) -> usize {
        self.files.len()
    }

    pub fn file(&self, path: &str) -> Option<&FileStatus> {
        self.files.iter().find(|file| file.path == path)
    }
}

/// Reads `git status --porcelain=v2 --branch -z`.
pub fn parse(output: &str) -> Status {
    let mut status = Status::default();
    let mut records = output.split('\0').peekable();
    while let Some(record) = records.next() {
        if record.is_empty() {
            continue;
        }
        if let Some(header) = record.strip_prefix("# ") {
            let (key, value) = header.split_once(' ').unwrap_or((header, ""));
            match key {
                "branch.oid" if value != "(initial)" => {
                    status.head = Some(value.chars().take(7).collect());
                }
                "branch.head" if value != "(detached)" => status.branch = Some(value.to_owned()),
                "branch.upstream" => status.upstream = Some(value.to_owned()),
                "branch.ab" => {
                    for part in value.split_whitespace() {
                        if let Some(ahead) = part.strip_prefix('+') {
                            status.ahead = ahead.parse().unwrap_or(0);
                        } else if let Some(behind) = part.strip_prefix('-') {
                            status.behind = behind.parse().unwrap_or(0);
                        }
                    }
                }
                _ => {}
            }
            continue;
        }
        let kind = record.as_bytes()[0];
        match kind {
            // 1 XY sub mH mI mW hH hI path
            b'1' => {
                let fields: Vec<&str> = record.splitn(9, ' ').collect();
                if let [_, xy, .., path] = fields.as_slice() {
                    status.files.push(entry(xy, path, None));
                }
            }
            // 2 XY sub mH mI mW hH hI Xscore path, then the original path
            b'2' => {
                let fields: Vec<&str> = record.splitn(10, ' ').collect();
                let original = records.next().map(str::to_owned);
                if let [_, xy, .., path] = fields.as_slice() {
                    status.files.push(entry(xy, path, original));
                }
            }
            // u XY sub m1 m2 m3 mW h1 h2 h3 path
            b'u' => {
                let fields: Vec<&str> = record.splitn(11, ' ').collect();
                if let Some(path) = fields.last() {
                    status.files.push(FileStatus {
                        path: (*path).to_owned(),
                        original: None,
                        index: Change::Modified,
                        worktree: Change::Modified,
                        conflict: true,
                    });
                }
            }
            b'?' => status.files.push(FileStatus {
                path: record[2..].to_owned(),
                original: None,
                index: Change::Untracked,
                worktree: Change::Untracked,
                conflict: false,
            }),
            _ => {}
        }
    }
    status
}

fn entry(xy: &str, path: &str, original: Option<String>) -> FileStatus {
    let bytes = xy.as_bytes();
    FileStatus {
        path: path.to_owned(),
        original,
        index: Change::from_code(bytes.first().copied().unwrap_or(b'.')),
        worktree: Change::from_code(bytes.get(1).copied().unwrap_or(b'.')),
        conflict: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_branch_files_renames_conflicts_and_untracked() {
        let output = [
            "# branch.oid 4f2a9c1e0b",
            "# branch.head develop",
            "# branch.upstream origin/develop",
            "# branch.ab +2 -1",
            "1 .M N... 100644 100644 100644 aaa bbb src/app/page.tsx",
            "1 A. N... 000000 100644 100644 000 ccc src/nuovo file.ts",
            "2 R. N... 100644 100644 100644 ddd ddd R100 lib/new.ts",
            "lib/old.ts",
            "u UU N... 100644 100644 100644 100644 e1 e2 e3 README.md",
            "? appunti.md",
            "",
        ]
        .join("\0");
        let status = parse(&output);
        assert_eq!(status.branch.as_deref(), Some("develop"));
        assert_eq!(status.head.as_deref(), Some("4f2a9c1"));
        assert_eq!(status.upstream.as_deref(), Some("origin/develop"));
        assert_eq!((status.ahead, status.behind), (2, 1));
        assert_eq!(status.count(), 5);
        let staged: Vec<&str> = status.staged().map(|file| file.path.as_str()).collect();
        assert_eq!(staged, ["src/nuovo file.ts", "lib/new.ts"]);
        let changed: Vec<&str> = status.changed().map(|file| file.path.as_str()).collect();
        assert_eq!(changed, ["src/app/page.tsx", "appunti.md"]);
        assert_eq!(
            status.file("lib/new.ts").unwrap().original.as_deref(),
            Some("lib/old.ts")
        );
        assert_eq!(status.conflicts().count(), 1);
        assert_eq!(status.file("appunti.md").unwrap().worktree.letter(), "U");
    }

    #[test]
    fn a_new_repository_and_a_detached_head() {
        let status = parse("# branch.oid (initial)\0# branch.head main\0");
        assert_eq!(
            (status.head, status.branch.as_deref()),
            (None, Some("main"))
        );
        let status = parse("# branch.oid 1234567890\0# branch.head (detached)\0");
        assert_eq!(status.branch, None);
    }
}
