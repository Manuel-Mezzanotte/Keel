//! Git for Keel, through the `git` command: the same program, settings,
//! hooks and credentials as in the terminal. Reading the status takes about
//! 16 ms on a project of a thousand files.

pub mod graph;
mod history;
mod hunks;
mod status;

use std::fmt;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

pub use history::{
    BlameLine, ChangedFile, Commit, CommitDetails, LogOptions, RefLabel, parse_blame, parse_log,
};
pub use hunks::{Hunk, HunkKind, apply_hunk, block_text, hunks, revert_hunk, unified_patch};
pub use status::{Change, FileStatus, Status, parse as parse_status};

/// What git said when a command failed, ready to show.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GitError(pub String);

impl fmt::Display for GitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for GitError {}

pub type Result<T> = std::result::Result<T, GitError>;

/// A branch, local or of a remote.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Branch {
    /// `develop`, or `origin/develop` for a remote one.
    pub name: String,
    pub remote: bool,
    pub current: bool,
}

/// A git repository, found from any folder inside it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Repo {
    root: PathBuf,
}

impl Repo {
    /// The repository holding `folder`, if any.
    pub fn discover(folder: &Path) -> Option<Self> {
        let output = Command::new("git")
            .arg("-C")
            .arg(folder)
            .args(["rev-parse", "--show-toplevel"])
            .stderr(Stdio::null())
            .output()
            .ok()?;
        if !output.status.success() {
            return None;
        }
        let root = String::from_utf8(output.stdout).ok()?;
        let root = PathBuf::from(root.trim_end());
        Some(Self {
            root: root.canonicalize().unwrap_or(root),
        })
    }

    /// The repository's top folder.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// A path as git names it: from the top folder, with `/`.
    pub fn relative(&self, path: &Path) -> Option<String> {
        let path = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
        let relative = path.strip_prefix(&self.root).ok()?;
        Some(relative.to_string_lossy().replace('\\', "/"))
    }

    pub fn absolute(&self, path: &str) -> PathBuf {
        self.root.join(path)
    }

    fn command(&self, args: &[&str]) -> Command {
        let mut command = Command::new("git");
        command
            .arg("-C")
            .arg(&self.root)
            .args(["-c", "core.quotepath=false", "-c", "color.ui=false"])
            .args(args)
            // A push that needs a password fails instead of waiting for
            // a prompt nobody sees; the keychain still answers.
            .env("GIT_TERMINAL_PROMPT", "0")
            .stdin(Stdio::null());
        command
    }

    fn run(&self, args: &[&str]) -> Result<String> {
        self.run_with_input(args, None)
    }

    fn run_with_input(&self, args: &[&str], input: Option<&[u8]>) -> Result<String> {
        let mut command = self.command(args);
        command.stdout(Stdio::piped()).stderr(Stdio::piped());
        if input.is_some() {
            command.stdin(Stdio::piped());
        }
        let mut child = command
            .spawn()
            .map_err(|error| GitError(format!("git: {error}")))?;
        if let Some(input) = input
            && let Some(mut stdin) = child.stdin.take()
        {
            stdin
                .write_all(input)
                .map_err(|error| GitError(format!("git: {error}")))?;
        }
        let output = child
            .wait_with_output()
            .map_err(|error| GitError(format!("git: {error}")))?;
        if output.status.success() {
            Ok(String::from_utf8_lossy(&output.stdout).into_owned())
        } else {
            let message = String::from_utf8_lossy(&output.stderr);
            let message = message.trim();
            let message = if message.is_empty() {
                String::from_utf8_lossy(&output.stdout).trim().to_owned()
            } else {
                message.to_owned()
            };
            Err(GitError(message))
        }
    }

    /// The branch and every changed file. Takes no lock, so it never gets
    /// in the way of git commands run in a terminal.
    pub fn status(&self) -> Result<Status> {
        let output = self.run(&[
            "--no-optional-locks",
            "status",
            "--porcelain=v2",
            "--branch",
            "-z",
            "--untracked-files=all",
        ])?;
        Ok(status::parse(&output))
    }

    /// A file as the last commit has it; `None` when it is not there.
    pub fn head_text(&self, path: &str) -> Option<String> {
        self.run(&["show", &format!("HEAD:{path}")]).ok()
    }

    /// A file as it was in a commit, such as `abc123` or `abc123^`;
    /// `None` when it was not there.
    pub fn text_at(&self, revision: &str, path: &str) -> Option<String> {
        self.run(&["show", &format!("{revision}:{path}")]).ok()
    }

    /// A file as it is staged; `None` when it is not in the index.
    pub fn index_text(&self, path: &str) -> Option<String> {
        self.run(&["show", &format!(":{path}")]).ok()
    }

    /// The changes of a file as a patch: staged ones, or those not staged
    /// yet. A file git does not know shows as entirely new.
    pub fn diff(&self, path: &str, staged: bool) -> Result<String> {
        let untracked = !staged && self.index_text(path).is_none();
        if untracked {
            let mut command = self.command(&[
                "diff",
                "--no-index",
                "--no-ext-diff",
                "--",
                "/dev/null",
                path,
            ]);
            let output = command
                .output()
                .map_err(|error| GitError(format!("git: {error}")))?;
            // `--no-index` exits with 1 when the files differ.
            return Ok(String::from_utf8_lossy(&output.stdout).into_owned());
        }
        let mut args = vec!["diff", "--no-ext-diff", "--find-renames"];
        if staged {
            args.push("--cached");
        }
        args.extend(["--", path]);
        self.run(&args)
    }

    /// Stages files, deletions included.
    pub fn stage(&self, paths: &[&str]) -> Result<()> {
        let mut args = vec!["add", "--all", "--"];
        args.extend(paths);
        self.run(&args).map(drop)
    }

    /// Takes files out of the stage, keeping their changes.
    pub fn unstage(&self, paths: &[&str]) -> Result<()> {
        let has_head = self.run(&["rev-parse", "--verify", "-q", "HEAD"]).is_ok();
        let mut args = if has_head {
            vec!["restore", "--staged", "--"]
        } else {
            vec!["rm", "--cached", "-r", "-q", "--"]
        };
        args.extend(paths);
        self.run(&args).map(drop)
    }

    /// Throws away the changes not staged of files git knows. Files git
    /// does not know are for the caller to move to the Trash.
    pub fn discard(&self, paths: &[&str]) -> Result<()> {
        let mut args = vec!["restore", "--worktree", "--"];
        args.extend(paths);
        self.run(&args).map(drop)
    }

    /// Stages a file with the given text, leaving the file on disk alone:
    /// how a single block gets staged.
    pub fn stage_text(&self, path: &str, text: &str) -> Result<()> {
        let object = self.run_with_input(
            &["hash-object", "-w", "--stdin", "--path", path],
            Some(text.as_bytes()),
        )?;
        let mode = self
            .run(&["ls-files", "-s", "--", path])
            .ok()
            .and_then(|line| line.split_whitespace().next().map(str::to_owned))
            .unwrap_or_else(|| "100644".to_owned());
        let info = format!("{mode},{},{path}", object.trim());
        self.run(&["update-index", "--add", "--cacheinfo", &info])
            .map(drop)
    }

    /// Commits what is staged. With `amend`, replaces the last commit; an
    /// empty message then keeps the old one.
    pub fn commit(&self, message: &str, amend: bool) -> Result<()> {
        let mut args = vec!["commit"];
        if amend {
            args.push("--amend");
            if message.trim().is_empty() {
                args.push("--no-edit");
                return self.run(&args).map(drop);
            }
        }
        args.extend(["--cleanup=strip", "-F", "-"]);
        self.run_with_input(&args, Some(message.as_bytes()))
            .map(drop)
    }

    /// The message of the last commit, to start an amend from.
    pub fn last_message(&self) -> Option<String> {
        self.run(&["log", "-1", "--format=%B"])
            .ok()
            .map(|message| message.trim_end().to_owned())
    }

    /// The local branches, then the remote ones.
    pub fn branches(&self) -> Result<Vec<Branch>> {
        let output = self.run(&[
            "for-each-ref",
            "--format=%(HEAD)%00%(refname)",
            "refs/heads",
            "refs/remotes",
        ])?;
        let mut branches = Vec::new();
        for line in output.lines() {
            let Some((head, name)) = line.split_once('\0') else {
                continue;
            };
            if let Some(local) = name.strip_prefix("refs/heads/") {
                branches.push(Branch {
                    name: local.to_owned(),
                    remote: false,
                    current: head == "*",
                });
            } else if let Some(remote) = name.strip_prefix("refs/remotes/")
                && !remote.ends_with("/HEAD")
            {
                branches.push(Branch {
                    name: remote.to_owned(),
                    remote: true,
                    current: false,
                });
            }
        }
        Ok(branches)
    }

    /// Checks out a branch. A remote one, such as `origin/fix`, becomes a
    /// local branch that follows it, as VS Code does.
    pub fn switch(&self, branch: &Branch) -> Result<()> {
        if branch.remote {
            let local = branch
                .name
                .split_once('/')
                .map_or(branch.name.as_str(), |(_, rest)| rest);
            return self
                .run(&["switch", "--track", "-c", local, &branch.name])
                .map(drop);
        }
        self.run(&["switch", &branch.name]).map(drop)
    }

    /// Creates a branch from where you are and checks it out.
    pub fn create_branch(&self, name: &str) -> Result<()> {
        self.run(&["switch", "-c", name]).map(drop)
    }

    pub fn fetch(&self) -> Result<()> {
        self.run(&["fetch", "--prune"]).map(drop)
    }

    pub fn pull(&self) -> Result<()> {
        self.run(&["pull"]).map(drop)
    }

    /// Pushes the branch; one without upstream is published to the first
    /// remote and follows it from then on.
    pub fn push(&self) -> Result<()> {
        let status = self.status()?;
        match (&status.upstream, &status.branch) {
            (None, Some(branch)) => {
                let remotes = self.run(&["remote"])?;
                let remote = remotes
                    .lines()
                    .find(|remote| *remote == "origin")
                    .or_else(|| remotes.lines().next())
                    .ok_or_else(|| GitError("no remote to push to".to_owned()))?
                    .to_owned();
                self.run(&["push", "-u", &remote, branch]).map(drop)
            }
            _ => self.run(&["push"]).map(drop),
        }
    }

    /// Pull, then push: VS Code's Sync.
    pub fn sync(&self) -> Result<()> {
        let status = self.status()?;
        if status.upstream.is_some() {
            self.pull()?;
        }
        self.push()
    }

    /// Puts every change aside, new files included.
    pub fn stash(&self) -> Result<()> {
        self.run(&["stash", "push", "--include-untracked"])
            .map(drop)
    }

    /// Brings back the last changes put aside.
    pub fn stash_pop(&self) -> Result<()> {
        self.run(&["stash", "pop"]).map(drop)
    }

    /// The commit checked out, in full; `None` before the first commit.
    pub fn head_hash(&self) -> Option<String> {
        self.run(&["rev-parse", "--verify", "-q", "HEAD"])
            .ok()
            .map(|hash| hash.trim().to_owned())
            .filter(|hash| !hash.is_empty())
    }

    /// How many commits `to` has that `from` has not, and how many files
    /// changed between them: what a pull brought.
    pub fn changes_between(&self, from: &str, to: &str) -> (usize, usize) {
        let range = format!("{from}..{to}");
        let commits = self
            .run(&["rev-list", "--count", &range])
            .ok()
            .and_then(|count| count.trim().parse().ok())
            .unwrap_or(0);
        let files = self
            .run(&["diff", "--name-only", from, to])
            .map(|names| names.lines().filter(|name| !name.is_empty()).count())
            .unwrap_or(0);
        (commits, files)
    }

    /// Undoes the last commit and keeps its changes staged, as VS Code's
    /// Undo Last Commit. The first commit has nothing before it, so its
    /// branch goes back to having no commit.
    pub fn undo_last_commit(&self) -> Result<()> {
        if self.run(&["rev-parse", "--verify", "-q", "HEAD~1"]).is_ok() {
            self.run(&["reset", "--soft", "HEAD~1"]).map(drop)
        } else {
            self.run(&["update-ref", "-d", "HEAD"]).map(drop)
        }
    }

    /// The web page of a remote, such as `origin`, when it has one.
    pub fn remote_web_url(&self, remote: &str) -> Option<String> {
        web_url(self.run(&["remote", "get-url", remote]).ok()?.trim())
    }

    /// The messages of the last `most` commits, the latest first, for
    /// going back through them in the commit field.
    pub fn recent_messages(&self, most: usize) -> Vec<String> {
        self.run(&["log", "-n", &most.to_string(), "--format=%B%x1e"])
            .map(|output| {
                output
                    .split('\x1e')
                    .map(str::trim)
                    .filter(|message| !message.is_empty())
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Puts every change aside under a name, new files included.
    pub fn stash_named(&self, message: &str) -> Result<()> {
        self.run(&["stash", "push", "--include-untracked", "-m", message])
            .map(drop)
    }

    /// The changes put aside, the latest first.
    pub fn stashes(&self) -> Result<Vec<Stash>> {
        let output = self.run(&["stash", "list", "--format=%gd%x1f%gs%x1f%ct"])?;
        Ok(output
            .lines()
            .filter_map(|line| {
                let mut parts = line.split('\x1f');
                let name = parts.next()?;
                let message = parts.next()?.to_owned();
                let time = parts.next()?.trim().parse().unwrap_or(0);
                let index = name
                    .strip_prefix("stash@{")?
                    .strip_suffix('}')?
                    .parse()
                    .ok()?;
                Some(Stash {
                    index,
                    message,
                    time,
                })
            })
            .collect())
    }

    /// Brings back a stash and drops it, or keeps it with `keep`.
    pub fn stash_restore(&self, index: usize, keep: bool) -> Result<()> {
        let name = format!("stash@{{{index}}}");
        let verb = if keep { "apply" } else { "pop" };
        self.run(&["stash", verb, "--index", &name])
            .or_else(|_| self.run(&["stash", verb, &name]))
            .map(drop)
    }

    pub fn stash_drop(&self, index: usize) -> Result<()> {
        self.run(&["stash", "drop", &format!("stash@{{{index}}}")])
            .map(drop)
    }

    /// The files a stash changed, new ones included.
    pub fn stash_files(&self, index: usize) -> Result<Vec<String>> {
        let name = format!("stash@{{{index}}}");
        let mut files: Vec<String> = self
            .run(&["stash", "show", "--name-only", &name])?
            .lines()
            .map(str::to_owned)
            .collect();
        // New files sit in the stash's third parent.
        if let Ok(untracked) = self.run(&["show", "--name-only", "--format=", &format!("{name}^3")])
        {
            files.extend(
                untracked
                    .lines()
                    .filter(|line| !line.is_empty())
                    .map(str::to_owned),
            );
        }
        files.sort();
        files.dedup();
        Ok(files)
    }

    /// The titles of the commits to push and of those to pull, at most
    /// `most` each; nothing when the branch follows no upstream.
    pub fn ahead_and_behind(&self, most: usize) -> (Vec<String>, Vec<String>) {
        let list = |range: &str| {
            self.run(&["log", "--format=%s", "-n", &most.to_string(), range])
                .map(|output| output.lines().map(str::to_owned).collect())
                .unwrap_or_default()
        };
        (list("@{upstream}..HEAD"), list("HEAD..@{upstream}"))
    }

    /// The commits that changed lines `start..=end` (from one) of a file,
    /// the latest first.
    pub fn line_history(
        &self,
        path: &str,
        start: usize,
        end: usize,
    ) -> Result<Vec<history::Commit>> {
        let range = format!("-L{start},{end}:{path}");
        let output = self.run(&[
            "log",
            &range,
            "--no-patch",
            "--format=%H%x1f%an%x1f%ct%x1f%s",
            "-n",
            "200",
        ])?;
        Ok(output
            .lines()
            .filter_map(|line| {
                let mut parts = line.split('\x1f');
                Some(history::Commit::summary(
                    parts.next()?.to_owned(),
                    parts.next()?.to_owned(),
                    parts.next()?.parse().unwrap_or(0),
                    parts.next().unwrap_or_default().to_owned(),
                ))
            })
            .collect())
    }

    pub fn stash_count(&self) -> usize {
        self.run(&["stash", "list"])
            .map(|list| list.lines().count())
            .unwrap_or(0)
    }
}

/// Changes put aside with `git stash`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Stash {
    /// The `n` of `stash@{n}`.
    pub index: usize,
    pub message: String,
    pub time: i64,
}

/// The web page of a repository from the address git pushes to:
/// `git@github.com:me/app.git`, `ssh://git@host/me/app` and
/// `https://user@host/me/app.git` all become `https://host/me/app`.
pub fn web_url(remote: &str) -> Option<String> {
    let remote = remote.trim();
    let (host, path) = if let Some(rest) = remote.strip_prefix("https://") {
        rest.split_once('/')?
    } else if let Some(rest) = remote
        .strip_prefix("ssh://")
        .or_else(|| remote.strip_prefix("git://"))
    {
        rest.split_once('/')?
    } else if let Some((user_host, path)) = remote.split_once(':') {
        // scp-like: git@github.com:me/app.git
        if user_host.contains('/') {
            return None;
        }
        (user_host, path)
    } else {
        return None;
    };
    let host = host.rsplit('@').next()?;
    // An ssh port is not the web page's.
    let host = host.split(':').next()?;
    let path = path.trim_end_matches('/').trim_end_matches(".git");
    if host.is_empty() || path.is_empty() {
        return None;
    }
    Some(format!("https://{host}/{path}"))
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    pub(crate) fn git(folder: &Path, args: &[&str]) {
        let status = Command::new("git")
            .arg("-C")
            .arg(folder)
            .args(args)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .unwrap();
        assert!(status.success(), "git {args:?}");
    }

    /// A repository with one commit, and a name for commits made in it.
    pub(crate) fn repository(folder: &Path) -> Repo {
        git(folder, &["init", "-q", "-b", "main"]);
        git(folder, &["config", "user.name", "Test"]);
        git(folder, &["config", "user.email", "test@example.com"]);
        git(folder, &["config", "commit.gpgsign", "false"]);
        fs::write(folder.join("a.txt"), "uno\ndue\ntre\n").unwrap();
        git(folder, &["add", "."]);
        git(folder, &["commit", "-q", "-m", "first"]);
        Repo::discover(folder).unwrap()
    }

    #[test]
    fn found_from_a_subfolder_and_paths_from_the_top() {
        let folder = tempfile::tempdir().unwrap();
        let repo = repository(folder.path());
        fs::create_dir(folder.path().join("src")).unwrap();
        let inside = Repo::discover(&folder.path().join("src")).unwrap();
        assert_eq!(inside, repo);
        assert_eq!(
            repo.relative(&folder.path().join("a.txt")).as_deref(),
            Some("a.txt")
        );
        let elsewhere = tempfile::tempdir().unwrap();
        assert_eq!(Repo::discover(elsewhere.path()), None);
    }

    #[test]
    fn status_stage_unstage_discard_and_commit() {
        let folder = tempfile::tempdir().unwrap();
        let repo = repository(folder.path());
        fs::write(folder.path().join("a.txt"), "uno\nDUE\ntre\n").unwrap();
        fs::write(folder.path().join("nuovo.txt"), "ciao\n").unwrap();
        let status = repo.status().unwrap();
        assert_eq!(status.branch.as_deref(), Some("main"));
        assert_eq!(status.changed().count(), 2);
        assert!(repo.diff("a.txt", false).unwrap().contains("+DUE"));
        assert!(repo.diff("nuovo.txt", false).unwrap().contains("+ciao"));

        repo.stage(&["a.txt", "nuovo.txt"]).unwrap();
        let status = repo.status().unwrap();
        assert_eq!(status.staged().count(), 2);
        assert!(repo.diff("a.txt", true).unwrap().contains("-due"));

        repo.unstage(&["nuovo.txt"]).unwrap();
        assert_eq!(repo.status().unwrap().staged().count(), 1);

        repo.commit("change due\n\nwith a body", false).unwrap();
        assert_eq!(
            repo.last_message().as_deref(),
            Some("change due\n\nwith a body")
        );
        assert_eq!(repo.head_text("a.txt").as_deref(), Some("uno\nDUE\ntre\n"));

        repo.stage(&["nuovo.txt"]).unwrap();
        repo.commit("", true).unwrap();
        assert_eq!(
            repo.last_message().as_deref(),
            Some("change due\n\nwith a body")
        );
        assert!(repo.status().unwrap().files.is_empty());

        fs::write(folder.path().join("a.txt"), "rotto\n").unwrap();
        repo.discard(&["a.txt"]).unwrap();
        assert_eq!(
            fs::read_to_string(folder.path().join("a.txt")).unwrap(),
            "uno\nDUE\ntre\n"
        );
    }

    #[test]
    fn stage_one_block() {
        let folder = tempfile::tempdir().unwrap();
        let repo = repository(folder.path());
        let new = "zero\nuno\nDUE\ntre\n";
        fs::write(folder.path().join("a.txt"), new).unwrap();
        let staged = repo.index_text("a.txt").unwrap();
        let found = hunks(&staged, new);
        assert_eq!(found.len(), 2);
        repo.stage_text("a.txt", &apply_hunk(&staged, new, &found[1]))
            .unwrap();
        assert_eq!(repo.index_text("a.txt").as_deref(), Some("uno\nDUE\ntre\n"));
        // The file on disk keeps both changes; one is still to stage.
        assert_eq!(
            fs::read_to_string(folder.path().join("a.txt")).unwrap(),
            new
        );
        let file = repo.status().unwrap().files.remove(0);
        assert!(file.is_staged() && file.is_changed());
    }

    #[test]
    fn branches_stash_push_and_pull() {
        let remote = tempfile::tempdir().unwrap();
        git(remote.path(), &["init", "-q", "--bare", "-b", "main"]);
        let folder = tempfile::tempdir().unwrap();
        let repo = repository(folder.path());
        git(
            folder.path(),
            &["remote", "add", "origin", remote.path().to_str().unwrap()],
        );

        // A branch without upstream is published.
        repo.push().unwrap();
        let status = repo.status().unwrap();
        assert_eq!(status.upstream.as_deref(), Some("origin/main"));

        repo.create_branch("fix").unwrap();
        assert_eq!(repo.status().unwrap().branch.as_deref(), Some("fix"));
        let branches = repo.branches().unwrap();
        assert!(
            branches
                .iter()
                .any(|branch| branch.name == "fix" && branch.current)
        );
        assert!(
            branches
                .iter()
                .any(|branch| branch.name == "origin/main" && branch.remote)
        );
        let main = branches
            .iter()
            .find(|branch| branch.name == "main")
            .unwrap();
        repo.switch(main).unwrap();

        fs::write(folder.path().join("a.txt"), "messo da parte\n").unwrap();
        repo.stash().unwrap();
        assert!(repo.status().unwrap().files.is_empty());
        assert_eq!(repo.stash_count(), 1);
        repo.stash_pop().unwrap();
        assert_eq!(repo.status().unwrap().changed().count(), 1);
        repo.discard(&["a.txt"]).unwrap();

        // Another clone pushes a commit; pull brings it, behind goes to 0.
        let other = tempfile::tempdir().unwrap();
        git(
            other.path(),
            &["clone", "-q", remote.path().to_str().unwrap(), "."],
        );
        git(other.path(), &["config", "user.name", "Other"]);
        git(other.path(), &["config", "user.email", "other@example.com"]);
        fs::write(other.path().join("b.txt"), "dall'altro\n").unwrap();
        git(other.path(), &["add", "."]);
        git(other.path(), &["commit", "-q", "-m", "other"]);
        git(other.path(), &["push", "-q"]);

        repo.fetch().unwrap();
        assert_eq!(repo.status().unwrap().behind, 1);
        repo.sync().unwrap();
        let status = repo.status().unwrap();
        assert_eq!((status.ahead, status.behind), (0, 0));
        assert!(folder.path().join("b.txt").exists());
    }

    #[test]
    fn stashes_are_listed_brought_back_and_dropped() {
        let folder = tempfile::tempdir().unwrap();
        git(folder.path(), &["init", "-q", "-b", "main"]);
        git(folder.path(), &["config", "user.email", "me@example.com"]);
        git(folder.path(), &["config", "user.name", "Me"]);
        let repo = Repo::discover(folder.path()).unwrap();
        fs::write(folder.path().join("a.txt"), "uno\n").unwrap();
        repo.stage(&["a.txt"]).unwrap();
        repo.commit("first", false).unwrap();
        fs::write(folder.path().join("a.txt"), "due\n").unwrap();
        fs::write(folder.path().join("new.txt"), "nuovo\n").unwrap();
        repo.stash_named("keel: main").unwrap();
        let stashes = repo.stashes().unwrap();
        assert_eq!(stashes.len(), 1);
        assert_eq!(stashes[0].index, 0);
        assert!(
            stashes[0].message.ends_with("keel: main"),
            "{:?}",
            stashes[0]
        );
        assert_eq!(repo.stash_files(0).unwrap(), ["a.txt", "new.txt"]);
        repo.stash_restore(0, true).unwrap();
        assert_eq!(
            fs::read_to_string(folder.path().join("a.txt")).unwrap(),
            "due\n"
        );
        assert_eq!(repo.stashes().unwrap().len(), 1, "kept");
        repo.stash_drop(0).unwrap();
        assert!(repo.stashes().unwrap().is_empty());

        // The lines' own history.
        repo.stage(&["a.txt", "new.txt"]).unwrap();
        repo.commit("second", false).unwrap();
        let history = repo.line_history("a.txt", 1, 1).unwrap();
        let subjects: Vec<&str> = history
            .iter()
            .map(|commit| commit.subject.as_str())
            .collect();
        assert_eq!(subjects, ["second", "first"]);
    }

    #[test]
    fn remotes_have_web_pages() {
        for remote in [
            "git@github.com:me/app.git",
            "https://github.com/me/app.git",
            "https://token@github.com/me/app",
            "ssh://git@github.com:22/me/app.git",
        ] {
            assert_eq!(
                web_url(remote).as_deref(),
                Some("https://github.com/me/app"),
                "{remote}"
            );
        }
        assert_eq!(web_url("/Users/me/backup.git"), None);
        assert_eq!(web_url("../other"), None);
    }

    #[test]
    fn the_last_commit_is_undone_and_its_changes_stay_staged() {
        let folder = tempfile::tempdir().unwrap();
        git(folder.path(), &["init", "-q", "-b", "main"]);
        git(folder.path(), &["config", "user.email", "me@example.com"]);
        git(folder.path(), &["config", "user.name", "Me"]);
        let repo = Repo::discover(folder.path()).unwrap();
        fs::write(folder.path().join("a.txt"), "one\n").unwrap();
        repo.stage(&["a.txt"]).unwrap();
        repo.commit("first", false).unwrap();
        let first = repo.head_hash().unwrap();
        fs::write(folder.path().join("a.txt"), "two\n").unwrap();
        fs::write(folder.path().join("b.txt"), "new\n").unwrap();
        repo.stage(&["a.txt", "b.txt"]).unwrap();
        repo.commit("second", false).unwrap();
        let second = repo.head_hash().unwrap();
        assert_eq!(repo.changes_between(&first, &second), (1, 2));

        repo.undo_last_commit().unwrap();
        assert_eq!(repo.head_hash().as_deref(), Some(first.as_str()));
        assert_eq!(repo.status().unwrap().staged().count(), 2);
        // The first commit too: the branch has no commit again.
        repo.undo_last_commit().unwrap();
        assert_eq!(repo.head_hash(), None);
        assert_eq!(repo.status().unwrap().staged().count(), 2);
    }
}

/// How long reading the status and finding the blocks take; run with
/// `KEEL_GIT_TIMING=/path/to/repo cargo test --release -p git timing --
/// --ignored --nocapture`.
#[cfg(test)]
mod timing {
    use std::time::Instant;

    use super::*;

    #[test]
    #[ignore]
    fn status_and_hunks() {
        let folder = std::env::var_os("KEEL_GIT_TIMING")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")));
        let repo = Repo::discover(&folder).unwrap();
        repo.status().unwrap();
        let start = Instant::now();
        for _ in 0..20 {
            repo.status().unwrap();
        }
        eprintln!(
            "status of {}: {:?} each",
            repo.root().display(),
            start.elapsed() / 20
        );

        let old: String = (0..20_000)
            .map(|line| format!("let value_{line} = {line};\n"))
            .collect();
        let new: String = old
            .lines()
            .enumerate()
            .filter(|(line, _)| line % 97 != 0)
            .map(|(line, text)| {
                if line % 89 == 0 {
                    format!("{text} // changed\n")
                } else {
                    format!("{text}\n")
                }
            })
            .collect();
        let start = Instant::now();
        let found = hunks(&old, &new);
        eprintln!(
            "{} blocks in 20,000 lines: {:?}",
            found.len(),
            start.elapsed()
        );
    }
}
