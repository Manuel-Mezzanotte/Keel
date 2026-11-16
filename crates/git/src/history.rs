//! The history of a repository, as Git Graph shows it: commits with their
//! branches and tags, the files a commit changed, the commands of its menu,
//! a file's history and who last changed each line.

use crate::{Repo, Result};

/// Fields of `git log`, separated by a unit separator, commits by a record
/// separator: neither appears in messages.
const LOG_FORMAT: &str = "--format=%H%x1f%P%x1f%an%x1f%ae%x1f%at%x1f%D%x1f%s%x1e";

/// The tree of an empty repository, to diff a first commit against.
const EMPTY_TREE: &str = "4b825dc642cb6eb9a060e54bf8d69288fbee4904";

/// A branch or tag pointing at a commit.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RefLabel {
    /// The branch checked out: `HEAD -> develop`.
    Head(String),
    /// HEAD on a commit with no branch.
    DetachedHead,
    Branch(String),
    /// `origin/develop`.
    Remote(String),
    Tag(String),
}

impl RefLabel {
    pub fn name(&self) -> &str {
        match self {
            RefLabel::Head(name)
            | RefLabel::Branch(name)
            | RefLabel::Remote(name)
            | RefLabel::Tag(name) => name,
            RefLabel::DetachedHead => "HEAD",
        }
    }
}

/// A commit in the history.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Commit {
    pub hash: String,
    pub parents: Vec<String>,
    pub author: String,
    pub email: String,
    /// Seconds since 1970.
    pub time: i64,
    pub refs: Vec<RefLabel>,
    pub subject: String,
}

impl Commit {
    pub fn short_hash(&self) -> &str {
        &self.hash[..self.hash.len().min(8)]
    }
}

/// What a commit says about itself beyond the history's line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommitDetails {
    pub hash: String,
    pub parents: Vec<String>,
    pub author: String,
    pub email: String,
    pub time: i64,
    pub committer: String,
    pub commit_time: i64,
    /// The whole message.
    pub message: String,
}

/// A file a commit changed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChangedFile {
    /// `M`, `A`, `D`, `R`...
    pub status: char,
    pub path: String,
    /// Where a renamed file came from.
    pub original: Option<String>,
}

/// Who last changed a line, and when.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BlameLine {
    pub hash: String,
    pub author: String,
    pub time: i64,
    pub summary: String,
}

impl BlameLine {
    /// Lines not committed yet blame a hash of zeros.
    pub fn is_uncommitted(&self) -> bool {
        self.hash.bytes().all(|byte| byte == b'0')
    }
}

/// Which commits the history lists.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LogOptions {
    /// Only these branches; every branch, remote and tag when empty.
    pub branches: Vec<String>,
    /// At most this many; Git Graph loads more on request.
    pub max: usize,
}

/// Reads `git log` with [`LOG_FORMAT`].
pub fn parse_log(output: &str) -> Vec<Commit> {
    output
        .split('\x1e')
        .filter_map(|record| {
            let record = record.trim_start_matches('\n');
            if record.is_empty() {
                return None;
            }
            let mut fields = record.splitn(7, '\x1f');
            let hash = fields.next()?.to_owned();
            let parents = fields
                .next()?
                .split_whitespace()
                .map(str::to_owned)
                .collect();
            let author = fields.next()?.to_owned();
            let email = fields.next()?.to_owned();
            let time = fields.next()?.parse().unwrap_or_default();
            let refs = parse_refs(fields.next()?);
            let subject = fields.next().unwrap_or_default().trim_end().to_owned();
            Some(Commit {
                hash,
                parents,
                author,
                email,
                time,
                refs,
                subject,
            })
        })
        .collect()
}

/// Reads `%D`: `HEAD -> develop, origin/develop, tag: v1.0`.
fn parse_refs(decorations: &str) -> Vec<RefLabel> {
    decorations
        .split(", ")
        .filter(|part| !part.is_empty())
        .filter_map(|part| {
            if let Some(branch) = part.strip_prefix("HEAD -> ") {
                return Some(RefLabel::Head(branch.to_owned()));
            }
            if part == "HEAD" {
                return Some(RefLabel::DetachedHead);
            }
            if let Some(tag) = part.strip_prefix("tag: ") {
                return Some(RefLabel::Tag(tag.to_owned()));
            }
            // `origin/HEAD` only says which branch the remote prefers.
            if part.ends_with("/HEAD") || part == "refs/stash" {
                return None;
            }
            Some(if part.contains('/') && !part.starts_with("refs/") {
                RefLabel::Remote(part.to_owned())
            } else {
                RefLabel::Branch(part.to_owned())
            })
        })
        .collect()
}

/// Reads `git blame --porcelain`: one entry per line of the file.
pub fn parse_blame(output: &str) -> Vec<BlameLine> {
    let mut known: std::collections::HashMap<String, BlameLine> = std::collections::HashMap::new();
    let mut lines = Vec::new();
    let mut current: Option<BlameLine> = None;
    for line in output.lines() {
        if let Some(content) = line.strip_prefix('\t') {
            let _ = content;
            if let Some(entry) = current.take() {
                known.insert(entry.hash.clone(), entry.clone());
                lines.push(entry);
            }
            continue;
        }
        let mut words = line.splitn(2, ' ');
        let first = words.next().unwrap_or_default();
        let rest = words.next().unwrap_or_default();
        // A header: the hash, then line numbers.
        if first.len() == 40 && first.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            current = Some(known.get(first).cloned().unwrap_or(BlameLine {
                hash: first.to_owned(),
                author: String::new(),
                time: 0,
                summary: String::new(),
            }));
            continue;
        }
        let Some(entry) = current.as_mut() else {
            continue;
        };
        match first {
            "author" => entry.author = rest.to_owned(),
            "author-time" => entry.time = rest.parse().unwrap_or_default(),
            "summary" => entry.summary = rest.to_owned(),
            _ => {}
        }
    }
    lines
}

/// Reads `--name-status -z`: a status letter, then one path or two.
fn parse_name_status(output: &str) -> Vec<ChangedFile> {
    let mut fields = output.split('\0').filter(|field| !field.is_empty());
    let mut files = Vec::new();
    while let Some(status) = fields.next() {
        let letter = status.chars().next().unwrap_or('M');
        let Some(first) = fields.next() else { break };
        if matches!(letter, 'R' | 'C') {
            let Some(second) = fields.next() else { break };
            files.push(ChangedFile {
                status: letter,
                path: second.to_owned(),
                original: Some(first.to_owned()),
            });
        } else {
            files.push(ChangedFile {
                status: letter,
                path: first.to_owned(),
                original: None,
            });
        }
    }
    files
}

impl Repo {
    /// The history, newest first in the order Git Graph uses: by date,
    /// children before their parents.
    pub fn log(&self, options: &LogOptions) -> Result<Vec<Commit>> {
        let max = format!("--max-count={}", options.max.max(1));
        let mut args = vec!["log", "--date-order", LOG_FORMAT, max.as_str()];
        if options.branches.is_empty() {
            args.extend(["--branches", "--tags", "--remotes", "HEAD"]);
        } else {
            args.extend(options.branches.iter().map(String::as_str));
        }
        args.push("--");
        Ok(parse_log(&self.run(&args)?))
    }

    /// The commits that changed `path`, following renames.
    pub fn file_log(&self, path: &str, max: usize) -> Result<Vec<Commit>> {
        let max = format!("--max-count={}", max.max(1));
        let output = self.run(&["log", "--follow", LOG_FORMAT, &max, "--", path])?;
        Ok(parse_log(&output))
    }

    pub fn commit_details(&self, hash: &str) -> Result<CommitDetails> {
        let output = self.run(&[
            "show",
            "-s",
            "--format=%H%x1f%P%x1f%an%x1f%ae%x1f%at%x1f%cn%x1f%ct%x1f%B",
            hash,
        ])?;
        let mut fields = output.splitn(8, '\x1f');
        let mut next = || fields.next().unwrap_or_default().to_owned();
        Ok(CommitDetails {
            hash: next(),
            parents: next().split_whitespace().map(str::to_owned).collect(),
            author: next(),
            email: next(),
            time: next().parse().unwrap_or_default(),
            committer: next(),
            commit_time: next().parse().unwrap_or_default(),
            message: next().trim_end().to_owned(),
        })
    }

    /// What a commit changed against its first parent (or nothing, for
    /// the first commit), so merges list what they brought in.
    pub fn commit_files(&self, hash: &str) -> Result<Vec<ChangedFile>> {
        let parent = self.first_parent(hash)?;
        let output = self.run(&["diff", "--name-status", "-z", "-M", &parent, hash])?;
        Ok(parse_name_status(&output))
    }

    /// The patch of one file in a commit.
    pub fn commit_file_diff(
        &self,
        hash: &str,
        path: &str,
        original: Option<&str>,
    ) -> Result<String> {
        let parent = self.first_parent(hash)?;
        let mut args = vec!["diff", "-M", parent.as_str(), hash, "--", path];
        if let Some(original) = original {
            args.push(original);
        }
        self.run(&args)
    }

    /// The text of `path` as of commit `hash`.
    pub fn file_at(&self, hash: &str, path: &str) -> Option<String> {
        self.run(&["show", &format!("{hash}:{path}")]).ok()
    }

    fn first_parent(&self, hash: &str) -> Result<String> {
        let parents = self.run(&["rev-list", "--parents", "-n", "1", hash])?;
        Ok(parents
            .split_whitespace()
            .nth(1)
            .unwrap_or(EMPTY_TREE)
            .to_owned())
    }

    /// Who last changed each line of `path`. With `text`, the lines of the
    /// editor, unsaved changes included, count instead of the file on disk.
    pub fn blame(&self, path: &str, text: Option<&str>) -> Result<Vec<BlameLine>> {
        let output = match text {
            Some(text) => self.run_with_input(
                &["blame", "--porcelain", "--contents", "-", "--", path],
                Some(text.as_bytes()),
            )?,
            None => self.run(&["blame", "--porcelain", "--", path])?,
        };
        Ok(parse_blame(&output))
    }

    // -----------------------------------------------------------------
    // The commands of a commit's menu

    /// Checks out a commit, leaving HEAD detached.
    pub fn checkout_commit(&self, hash: &str) -> Result<()> {
        self.run(&["switch", "--detach", hash]).map(drop)
    }

    /// Creates a branch at a commit, and checks it out if asked.
    pub fn create_branch_at(&self, name: &str, hash: &str, checkout: bool) -> Result<()> {
        if checkout {
            self.run(&["switch", "-c", name, hash]).map(drop)
        } else {
            self.run(&["branch", name, hash]).map(drop)
        }
    }

    /// Creates a tag at a commit: annotated when it has a message.
    pub fn create_tag(&self, name: &str, hash: &str, message: Option<&str>) -> Result<()> {
        match message.filter(|message| !message.trim().is_empty()) {
            Some(message) => self
                .run(&["tag", "-a", name, "-m", message, hash])
                .map(drop),
            None => self.run(&["tag", name, hash]).map(drop),
        }
    }

    pub fn delete_tag(&self, name: &str) -> Result<()> {
        self.run(&["tag", "-d", name]).map(drop)
    }

    /// Deletes a local branch; `force` deletes it even if not merged.
    pub fn delete_branch(&self, name: &str, force: bool) -> Result<()> {
        self.run(&["branch", if force { "-D" } else { "-d" }, name])
            .map(drop)
    }

    /// Applies a commit on the current branch. A merge takes its first
    /// parent's side, as Git Graph does.
    pub fn cherry_pick(&self, hash: &str, is_merge: bool) -> Result<()> {
        let mut args = vec!["cherry-pick"];
        if is_merge {
            args.extend(["-m", "1"]);
        }
        args.push(hash);
        self.run(&args).map(drop)
    }

    /// Makes a commit that undoes `hash`.
    pub fn revert(&self, hash: &str, is_merge: bool) -> Result<()> {
        let mut args = vec!["revert", "--no-edit"];
        if is_merge {
            args.extend(["-m", "1"]);
        }
        args.push(hash);
        self.run(&args).map(drop)
    }

    /// Merges a branch or commit into the current branch.
    pub fn merge(&self, what: &str) -> Result<()> {
        self.run(&["merge", "--no-edit", what]).map(drop)
    }

    /// Whether a name is a valid branch name for git.
    pub fn is_valid_branch_name(&self, name: &str) -> bool {
        self.run(&["check-ref-format", "--branch", name]).is_ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn log_records_with_refs() {
        let output = "aaa\x1fbbb ccc\x1fManuel\x1fm@x.it\x1f1700000000\x1fHEAD -> develop, origin/develop, tag: v1.0, origin/HEAD\x1fMerge branch 'fix'\x1e\nbbb\x1f\x1fManuel\x1fm@x.it\x1f1690000000\x1f\x1fPrimo commit\x1e\n";
        let commits = parse_log(output);
        assert_eq!(commits.len(), 2);
        assert_eq!(commits[0].parents, ["bbb", "ccc"]);
        assert_eq!(
            commits[0].refs,
            [
                RefLabel::Head("develop".into()),
                RefLabel::Remote("origin/develop".into()),
                RefLabel::Tag("v1.0".into())
            ]
        );
        assert_eq!(commits[1].parents, Vec::<String>::new());
        assert_eq!(commits[1].subject, "Primo commit");
    }

    #[test]
    fn renames_and_blame() {
        let files = parse_name_status("M\0src/a.ts\0R100\0old.ts\0new.ts\0D\0gone.ts\0");
        assert_eq!(files.len(), 3);
        assert_eq!(files[1].path, "new.ts");
        assert_eq!(files[1].original.as_deref(), Some("old.ts"));
        let hash = "1".repeat(40);
        let blame = format!(
            "{hash} 1 1 2\nauthor Manuel\nauthor-time 1700000000\nsummary Primo\nfilename a.ts\n\tuno\n{hash} 2 2\n\tdue\n{zero} 3 3 1\nauthor Not Committed Yet\nauthor-time 1700000100\nsummary Version of a.ts from a.ts\n\ttre\n",
            zero = "0".repeat(40)
        );
        let lines = parse_blame(&blame);
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[1].author, "Manuel");
        assert_eq!(lines[1].summary, "Primo");
        assert!(lines[2].is_uncommitted());
    }

    #[test]
    fn a_real_history_its_files_blame_and_commands() {
        use crate::tests::{git, repository};
        let folder = tempfile::tempdir().unwrap();
        let root = folder.path();
        let repo = repository(root);
        git(root, &["switch", "-q", "-c", "fix"]);
        std::fs::write(root.join("b.txt"), "nuovo\n").unwrap();
        git(root, &["add", "."]);
        git(root, &["commit", "-q", "-m", "add b"]);
        git(root, &["switch", "-q", "main"]);
        std::fs::write(root.join("a.txt"), "uno\ndue\ntre\nquattro\n").unwrap();
        git(root, &["commit", "-q", "-am", "four"]);
        git(root, &["merge", "-q", "--no-edit", "fix"]);
        git(root, &["tag", "v1"]);

        let commits = repo
            .log(&LogOptions {
                branches: Vec::new(),
                max: 100,
            })
            .unwrap();
        let subjects: Vec<&str> = commits
            .iter()
            .map(|commit| commit.subject.as_str())
            .collect();
        assert_eq!(subjects.len(), 4);
        assert!(subjects[0].starts_with("Merge branch 'fix'"));
        assert!(commits[0].refs.contains(&RefLabel::Head("main".into())));
        assert!(commits[0].refs.contains(&RefLabel::Tag("v1".into())));
        let rows = crate::graph::layout(&commits, 8);
        assert_eq!(rows.iter().map(|row| row.width).max(), Some(2));
        // Only one branch.
        let fix = repo
            .log(&LogOptions {
                branches: vec!["fix".into()],
                max: 100,
            })
            .unwrap();
        assert_eq!(fix.len(), 2);

        // The merge brought in b.txt; the first commit added a.txt.
        let merge = &commits[0];
        let files = repo.commit_files(&merge.hash).unwrap();
        assert_eq!(
            files,
            [ChangedFile {
                status: 'A',
                path: "b.txt".into(),
                original: None
            }]
        );
        let first = commits.last().unwrap();
        assert_eq!(repo.commit_files(&first.hash).unwrap()[0].path, "a.txt");
        let patch = repo.commit_file_diff(&first.hash, "a.txt", None).unwrap();
        assert!(patch.contains("+uno"));
        let details = repo.commit_details(&merge.hash).unwrap();
        assert_eq!(details.parents.len(), 2);
        assert_eq!(
            repo.file_at(&first.hash, "a.txt").as_deref(),
            Some("uno\ndue\ntre\n")
        );
        assert_eq!(repo.file_log("a.txt", 10).unwrap().len(), 2);

        // Blame, with a line not committed yet.
        let blame = repo
            .blame("a.txt", Some("uno\ndue\ntre\nquattro\ncinque\n"))
            .unwrap();
        assert_eq!(blame.len(), 5);
        assert_eq!(blame[0].summary, "first");
        assert_eq!(blame[3].summary, "four");
        assert!(blame[4].is_uncommitted());

        // The menu's commands.
        repo.create_branch_at("da-qui", &first.hash, false).unwrap();
        repo.create_tag("v0", &first.hash, Some("la prima"))
            .unwrap();
        assert!(
            repo.branches()
                .unwrap()
                .iter()
                .any(|branch| branch.name == "da-qui")
        );
        repo.revert(&commits[1].hash, false).unwrap();
        assert_eq!(
            std::fs::read_to_string(root.join("a.txt")).unwrap(),
            "uno\ndue\ntre\n"
        );
        repo.delete_tag("v0").unwrap();
        repo.delete_branch("da-qui", false).unwrap();
        repo.checkout_commit(&first.hash).unwrap();
        assert_eq!(repo.status().unwrap().branch, None);
        assert!(repo.is_valid_branch_name("feature/x"));
        assert!(!repo.is_valid_branch_name("con spazio"));
    }

    /// Times the graph on a real repository: `KEEL_TEST_REPO=folder
    /// [KEEL_TEST_BLAME=file] cargo test -p git --release timing_graph --
    /// --ignored --nocapture`.
    #[test]
    #[ignore]
    fn timing_graph() {
        let folder = std::env::var("KEEL_TEST_REPO").expect("KEEL_TEST_REPO");
        let repo = Repo::discover(std::path::Path::new(&folder)).unwrap();
        for max in [500, 5000] {
            let started = std::time::Instant::now();
            let commits = repo
                .log(&LogOptions {
                    branches: Vec::new(),
                    max,
                })
                .unwrap();
            let read = started.elapsed();
            let started = std::time::Instant::now();
            let rows = crate::graph::layout(&commits, 12);
            println!(
                "{} commits read in {read:?}, laid out in {:?}, {} lanes at most",
                commits.len(),
                started.elapsed(),
                rows.iter().map(|row| row.width).max().unwrap_or(0)
            );
        }
        if let Ok(file) = std::env::var("KEEL_TEST_BLAME") {
            let started = std::time::Instant::now();
            let lines = repo.blame(&file, None).unwrap();
            println!(
                "blame of {file}: {} lines in {:?}",
                lines.len(),
                started.elapsed()
            );
        }
    }
}
