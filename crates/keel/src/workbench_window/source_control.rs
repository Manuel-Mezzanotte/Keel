//! The Source Control view: the commit message, the staged files, the
//! changes and the merge conflicts, and the git commands, as in VS Code.
//! Git runs on a background thread; the view shows its last status.

use std::sync::Arc;

use gpui::{
    AnyElement, ClickEvent, Div, ElementId, Entity, KeyBinding, StatefulInteractiveElement as _,
    uniform_list,
};
use gpui_component::button::{Button, ButtonVariants as _, DropdownButton};
use gpui_component::input::{Input, InputState, Textarea, TextareaState};
use gpui_component::menu::DropdownMenu as _;
use gpui_component::{Disableable as _, Icon, Sizable as _};
use gpui_kit_assets::IconName;

use super::shell::{
    Colors, chevron_turn, empty_state, key_hint, own_clicks, section_header, view_header,
};
use super::*;

/// The key context around the commit message, for Cmd+Enter.
const MESSAGE_CONTEXT: &str = "CommitMessage";

/// How long the Commit button says «Done» after a commit.
const COMMITTED_FOR: Duration = Duration::from_millis(1600);

/// How long the status waits after a change before git is asked again.
const REFRESH_PAUSE: Duration = Duration::from_millis(250);

actions!(
    keel,
    [
        ToggleScmFolders,
        Commit,
        CommitAmend,
        StageAll,
        UnstageAll,
        DiscardAll,
        GitPush,
        GitPull,
        GitFetch,
        GitSync,
        GitStash,
        GitStashPop,
        GitRefresh,
        CheckoutBranch,
        CreateBranch,
        CompareWithBase,
        ExpandCommitMessage
    ]
);

/// The lines of the commit message's big window.
const BIG_MESSAGE_ROWS: usize = 18;

pub(crate) fn bindings() -> Vec<KeyBinding> {
    vec![
        KeyBinding::new("cmd-enter", Commit, Some(MESSAGE_CONTEXT)),
        KeyBinding::new("shift-escape", ExpandCommitMessage, Some(MESSAGE_CONTEXT)),
    ]
}

impl Workbench {
    /// "What changes against main": every file the branch changed since it
    /// parted from the base branch (`origin/HEAD`, else `main`), in one
    /// page, as a pull request would show them, before opening one.
    pub(super) fn compare_with_base(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let strings = self.strings();
        let Some(repo) = self.scm.repo.as_ref() else {
            return;
        };
        let Some(base) = repo.base_branch() else {
            self.report(strings.git_no_base.to_owned());
            cx.notify();
            return;
        };
        let from = match repo.merge_base(&base, "HEAD") {
            Ok(from) if !from.is_empty() => from,
            _ => {
                self.report(strings.git_no_base.to_owned());
                cx.notify();
                return;
            }
        };
        let changed = repo
            .files_between(&from, "HEAD")
            .map(|files| files.len())
            .unwrap_or(0);
        if changed == 0 {
            self.status_message = Some(strings.git_nothing_against(&base));
            cx.notify();
            return;
        }
        self.open_diff(
            "",
            super::diff_view::DiffSource::Branch { base, from },
            window,
            cx,
        );
    }
}

/// Which list of the view a file is in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) enum Section {
    Conflicts,
    Staged,
    Changes,
}

/// A row of the view's list: a section's title, or one of its files;
/// grouped by folder, a folder's row, by its first file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Row {
    Title(Section, usize),
    Folder(Section, usize, usize),
    File(Section, usize),
}

/// A file's folder, from the repository's top folder.
fn folder_of(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(folder, _)| folder)
}

/// The repository of the project and what the view shows of it.
pub(super) struct SourceControl {
    pub(super) repo: Option<Arc<git::Repo>>,
    pub(super) status: git::Status,
    pub(super) message: Entity<TextareaState>,
    /// What git is doing now, such as "Push", shown while it runs.
    pub(super) busy: Option<&'static str>,
    /// Files just staged or unstaged, which slide into their section, and
    /// a count that makes each move its own.
    pub(super) arrived: Vec<String>,
    pub(super) arrival: usize,
    /// Claude is writing the commit message.
    pub(super) writing_message: bool,
    /// What to run after the next commit: Commit & Push, Commit & Sync.
    pub(super) then: Option<RemoteAction>,
    /// The commit on a protected branch was confirmed.
    protection_ok: bool,
    /// When the last commit was made, for the check on the button.
    pub(super) committed_at: Option<std::time::Instant>,
    committed_task: Task<()>,
    /// Sections closed to their header.
    pub(super) collapsed: std::collections::HashSet<Section>,
    /// Grouped by folder, as VS Code's tree, and the folders closed.
    pub(super) by_folder: bool,
    folders_closed: std::collections::HashSet<(Section, String)>,
    refresh_task: Task<()>,
    operation: Task<()>,
    _message_grows: Subscription,
}

/// The commit message shows at least this many lines, and grows with its
/// text up to `MESSAGE_ROWS`.
const MESSAGE_MIN_ROWS: usize = 3;
const MESSAGE_ROWS: usize = 10;

impl SourceControl {
    pub(super) fn new(
        root: Option<&Path>,
        strings: &'static Strings,
        window: &mut Window,
        cx: &mut Context<Workbench>,
    ) -> Self {
        let repo = root.and_then(git::Repo::discover).map(Arc::new);
        let placeholder = strings.scm_message;
        let message = cx.new(|cx| {
            TextareaState::new(window, cx)
                .placeholder(placeholder)
                .rows(MESSAGE_MIN_ROWS)
        });
        // gpui-component's own auto_grow redraws the field on every frame,
        // which kept the CPU busy while idle; counting lines is enough.
        let message_grows = cx.subscribe(&message, |_, input, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) {
                let lines = input
                    .read(cx)
                    .value()
                    .split('\n')
                    .count()
                    .clamp(MESSAGE_MIN_ROWS, MESSAGE_ROWS);
                input.update(cx, |input, cx| input.set_rows(lines, cx));
            }
        });
        Self {
            repo,
            status: git::Status::default(),
            message,
            busy: None,
            arrived: Vec::new(),
            arrival: 0,
            writing_message: false,
            then: None,
            protection_ok: false,
            committed_at: None,
            committed_task: Task::ready(()),
            collapsed: Default::default(),
            by_folder: false,
            folders_closed: Default::default(),
            refresh_task: Task::ready(()),
            operation: Task::ready(()),
            _message_grows: message_grows,
        }
    }

    fn rows(&self) -> Vec<Row> {
        let mut rows = Vec::new();
        for section in [Section::Conflicts, Section::Staged, Section::Changes] {
            let files: Vec<usize> = self
                .status
                .files
                .iter()
                .enumerate()
                .filter(|(_, file)| match section {
                    Section::Conflicts => file.conflict,
                    Section::Staged => file.is_staged(),
                    Section::Changes => file.is_changed(),
                })
                .map(|(index, _)| index)
                .collect();
            if files.is_empty() {
                continue;
            }
            rows.push(Row::Title(section, files.len()));
            if self.collapsed.contains(&section) {
                continue;
            }
            if !self.by_folder {
                rows.extend(files.into_iter().map(|index| Row::File(section, index)));
                continue;
            }
            let mut files = files;
            files.sort_by(|a, b| {
                let (a, b) = (&self.status.files[*a].path, &self.status.files[*b].path);
                (folder_of(a), a).cmp(&(folder_of(b), b))
            });
            let mut index = 0;
            while index < files.len() {
                let folder = folder_of(&self.status.files[files[index]].path).to_owned();
                let count = files[index..]
                    .iter()
                    .take_while(|file| folder_of(&self.status.files[**file].path) == folder)
                    .count();
                if !folder.is_empty() {
                    rows.push(Row::Folder(section, files[index], count));
                }
                if folder.is_empty() || !self.folders_closed.contains(&(section, folder)) {
                    rows.extend(
                        files[index..index + count]
                            .iter()
                            .map(|file| Row::File(section, *file)),
                    );
                }
                index += count;
            }
        }
        rows
    }
}

/// The color of a change's letter, as in VS Code's themes.
fn change_color(file: &git::FileStatus, section: Section, cx: &App) -> Hsla {
    let theme = cx.theme();
    let change = match section {
        Section::Staged => file.index,
        _ => file.worktree,
    };
    if file.conflict {
        return theme.danger;
    }
    match change {
        git::Change::Added | git::Change::Untracked => theme.success,
        git::Change::Deleted => theme.danger,
        git::Change::Renamed | git::Change::Copied => theme.info,
        _ => theme.warning,
    }
}

impl Workbench {
    /// Asks git for the status again shortly, once changes settle.
    pub(super) fn refresh_git(&mut self, cx: &mut Context<Self>) {
        let Some(repo) = self.scm.repo.clone() else {
            return;
        };
        self.scm.refresh_task = cx.spawn(async move |this, cx| {
            cx.background_executor().timer(REFRESH_PAUSE).await;
            let status = cx.background_spawn(async move { repo.status() }).await;
            this.update(cx, |this, cx| this.git_status_arrived(status, cx))
                .ok();
        });
    }

    pub(super) fn git_status_arrived(
        &mut self,
        status: git::Result<git::Status>,
        cx: &mut Context<Self>,
    ) {
        match status {
            Ok(status) => {
                if status.branch.is_some() || status.head.is_some() {
                    self.project.branch = status.branch.clone().or_else(|| status.head.clone());
                }
                self.scm.status = status;
                self.pulls_branch_changed(cx);
                self.git_marks = match (&self.project.root, &self.scm.repo) {
                    (Some(root), Some(repo)) => {
                        let real = root.canonicalize().unwrap_or_else(|_| root.clone());
                        git_marks::GitMarks::new(root, &real, repo.root(), &self.scm.status)
                    }
                    _ => Default::default(),
                };
                self.status_counts_changed(cx);
                self.ensure_line_blame(cx);
                self.git_status_changed(cx);
            }
            Err(error) => self.log(format!("git: {error}")),
        }
        cx.notify();
    }

    /// Runs a git command in the background, shows that it runs, reports
    /// what went wrong and reads the status again. `then` runs on success.
    pub(super) fn run_git(
        &mut self,
        label: &'static str,
        operation: impl FnOnce(&git::Repo) -> git::Result<()> + Send + 'static,
        then: impl FnOnce(&mut Self, &mut Window, &mut Context<Self>) + 'static,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.run_git_with(
            label,
            None,
            operation,
            move |this, (), window, cx| then(this, window, cx),
            window,
            cx,
        );
    }

    /// Like `run_git`, with what the operation found for `then`; when it
    /// fails, the error window offers `retry` again.
    pub(super) fn run_git_with<T: Send + 'static>(
        &mut self,
        label: &'static str,
        retry: Option<RemoteAction>,
        operation: impl FnOnce(&git::Repo) -> git::Result<T> + Send + 'static,
        then: impl FnOnce(&mut Self, T, &mut Window, &mut Context<Self>) + 'static,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(repo) = self.scm.repo.clone() else {
            return;
        };
        self.scm.busy = Some(label);
        cx.notify();
        self.scm.operation = cx.spawn_in(window, async move |this, cx| {
            let result = cx
                .background_spawn(async move {
                    let result = operation(&repo);
                    let status = repo.status();
                    (result, status)
                })
                .await;
            this.update_in(cx, |this, window, cx| {
                this.scm.busy = None;
                let (result, status) = result;
                this.git_status_arrived(status, cx);
                match result {
                    Ok(value) => then(this, value, window, cx),
                    Err(error) => this.git_failed(label, error.to_string(), retry, window, cx),
                }
                cx.notify();
            })
            .ok();
        });
    }

    fn file_paths(&self, section: Section) -> Vec<String> {
        self.scm
            .status
            .files
            .iter()
            .filter(|file| match section {
                Section::Conflicts => file.conflict,
                Section::Staged => file.is_staged(),
                Section::Changes => file.is_changed(),
            })
            .map(|file| file.path.clone())
            .collect()
    }

    pub(super) fn stage_paths(
        &mut self,
        paths: Vec<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if paths.is_empty() {
            return;
        }
        let label = self.strings().scm_stage;
        self.scm.arrived = paths.clone();
        self.scm.arrival += 1;
        self.run_git(
            label,
            move |repo| repo.stage(&paths.iter().map(String::as_str).collect::<Vec<_>>()),
            |_, _, _| {},
            window,
            cx,
        );
    }

    pub(super) fn unstage_paths(
        &mut self,
        paths: Vec<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if paths.is_empty() {
            return;
        }
        let label = self.strings().scm_unstage;
        self.scm.arrived = paths.clone();
        self.scm.arrival += 1;
        self.run_git(
            label,
            move |repo| repo.unstage(&paths.iter().map(String::as_str).collect::<Vec<_>>()),
            |_, _, _| {},
            window,
            cx,
        );
    }

    /// Throws away changes after asking: files git knows go back to their
    /// staged version, new files go to the Trash.
    pub(super) fn discard_paths(
        &mut self,
        paths: Vec<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(repo) = self.scm.repo.clone() else {
            return;
        };
        if paths.is_empty() {
            return;
        }
        let strings = self.strings();
        let question = match paths.as_slice() {
            [one] => strings.scm_discard_question(one.rsplit('/').next().unwrap_or(one)),
            many => strings.scm_discard_many_question(many.len()),
        };
        let answer = window.prompt(
            PromptLevel::Warning,
            &question,
            Some(strings.scm_discard_detail),
            &[strings.scm_discard_button, strings.cancel],
            cx,
        );
        let untracked: Vec<String> = paths
            .iter()
            .filter(|path| {
                self.scm
                    .status
                    .file(path)
                    .is_some_and(|file| file.worktree == git::Change::Untracked)
            })
            .cloned()
            .collect();
        cx.spawn_in(window, async move |this, cx| {
            if answer.await != Ok(0) {
                return;
            }
            this.update_in(cx, |this, window, cx| {
                for path in &untracked {
                    if let Err(error) = files::trash(&repo.absolute(path)) {
                        this.report(format!("{path}: {error}"));
                    }
                }
                let tracked: Vec<String> = paths
                    .into_iter()
                    .filter(|path| !untracked.contains(path))
                    .collect();
                let label = this.strings().scm_discard;
                this.run_git(
                    label,
                    move |repo| {
                        if tracked.is_empty() {
                            return Ok(());
                        }
                        repo.discard(&tracked.iter().map(String::as_str).collect::<Vec<_>>())
                    },
                    |_, _, _| {},
                    window,
                    cx,
                );
            })
            .ok();
        })
        .detach();
    }

    /// Commits the staged changes with the message. With nothing staged,
    /// offers to stage everything first, as VS Code's smart commit does.
    pub(super) fn commit(&mut self, amend: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.scm.repo.is_none() {
            return;
        }
        let strings = self.strings();
        let message = self.scm.message.read(cx).value().to_string();
        if message.trim().is_empty() && !amend {
            self.status_message = Some(strings.scm_no_message.to_owned());
            self.scm
                .message
                .update(cx, |input, cx| input.focus(window, cx));
            cx.notify();
            return;
        }
        let nothing_staged = self.scm.status.staged().next().is_none();
        let changes = self.file_paths(Section::Changes);
        // A protected branch asks first, as VS Code's git.branchProtection.
        let branch = self.scm.status.branch.clone().unwrap_or_default();
        let protected = self
            .settings
            .get("git.branchProtection")
            .and_then(Value::as_array)
            .is_some_and(|list| {
                list.iter()
                    .any(|name| name.as_str() == Some(branch.as_str()))
            });
        if protected && !amend && !std::mem::take(&mut self.scm.protection_ok) {
            let question = notices::fill(strings.git_protected_question, &[("branch", &branch)]);
            let detail = notices::fill(strings.git_protected_detail, &[("branch", &branch)]);
            let answer = window.prompt(
                PromptLevel::Warning,
                &question,
                Some(&detail),
                &[
                    strings.git_protected_commit,
                    strings.git_protected_branch,
                    strings.cancel,
                ],
                cx,
            );
            cx.spawn_in(window, async move |this, cx| {
                let Ok(choice) = answer.await else {
                    return;
                };
                this.update_in(cx, |this, window, cx| match choice {
                    0 => {
                        this.scm.protection_ok = true;
                        this.commit(false, window, cx)
                    }
                    1 => this.ask_branch_name(window, cx),
                    _ => this.scm.then = None,
                })
                .ok();
            })
            .detach();
            return;
        }
        if nothing_staged && !amend && !changes.is_empty() {
            let smart = self
                .settings
                .get("git.enableSmartCommit")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            if smart {
                self.run_commit(message, false, Some(changes), window, cx);
                return;
            }
            let answer = window.prompt(
                PromptLevel::Info,
                strings.scm_stage_and_commit_question,
                None,
                &[
                    strings.scm_stage_and_commit,
                    strings.scm_stage_always,
                    strings.cancel,
                ],
                cx,
            );
            cx.spawn_in(window, async move |this, cx| {
                let Ok(choice) = answer.await else {
                    return;
                };
                this.update_in(cx, |this, window, cx| {
                    if choice == 1 {
                        this.change_setting(
                            "git.enableSmartCommit",
                            Some(Value::Bool(true)),
                            window,
                            cx,
                        );
                    }
                    if choice <= 1 {
                        this.run_commit(message, false, Some(changes), window, cx)
                    } else {
                        this.scm.then = None;
                    }
                })
                .ok();
            })
            .detach();
            return;
        }
        self.run_commit(message, amend, None, window, cx);
    }

    fn run_commit(
        &mut self,
        message: String,
        amend: bool,
        stage_first: Option<Vec<String>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let label = self.strings().scm_commit;
        let written = message.clone();
        self.run_git(
            label,
            move |repo| {
                if let Some(paths) = stage_first {
                    repo.stage(&paths.iter().map(String::as_str).collect::<Vec<_>>())?;
                }
                repo.commit(&message, amend)
            },
            move |this, window, cx| {
                this.scm
                    .message
                    .update(cx, |input, cx| input.set_value("", window, cx));
                this.scm.committed_at = Some(std::time::Instant::now());
                this.scm.committed_task = cx.spawn(async move |this, cx| {
                    cx.background_executor().timer(COMMITTED_FOR).await;
                    this.update(cx, |this, cx| {
                        this.scm.committed_at = None;
                        cx.notify();
                    })
                    .ok();
                });
                this.notify_committed(&written, amend, window, cx);
                this.commit_messages_changed();
                // Commit & Push, or what the settings want after a commit.
                let after = this.scm.then.take().or_else(|| {
                    match this.settings.string("git.postCommitCommand") {
                        Some("push") => Some(RemoteAction::Push),
                        Some("sync") => Some(RemoteAction::Sync),
                        _ => None,
                    }
                });
                if let Some(after) = after {
                    this.remote_command(after, window, cx);
                }
            },
            window,
            cx,
        );
    }

    /// Commit (Amend): puts the last message in the field to change it,
    /// or amends with the message already written.
    fn commit_amend(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let written = self.scm.message.read(cx).value().to_string();
        if written.trim().is_empty()
            && let Some(last) = self.scm.repo.as_ref().and_then(|repo| repo.last_message())
        {
            self.scm
                .message
                .update(cx, |input, cx| input.set_value(last, window, cx));
        }
        self.commit(true, window, cx);
    }

    pub(super) fn remote_command(
        &mut self,
        action: RemoteAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let strings = self.strings();
        match action {
            RemoteAction::Push => self.run_git_with(
                strings.scm_push,
                Some(action),
                |repo| {
                    let before = repo.status()?;
                    repo.push()?;
                    let after = repo.status().ok();
                    Ok(git_feedback::Pushed {
                        commits: before.ahead,
                        published: before.upstream.is_none(),
                        branch: before.branch,
                        upstream: after.and_then(|status| status.upstream),
                    })
                },
                |this, pushed, window, cx| this.notify_pushed(pushed, window, cx),
                window,
                cx,
            ),
            RemoteAction::Pull => self.run_git_with(
                strings.scm_pull,
                Some(action),
                |repo| {
                    let before = repo.head_hash();
                    repo.pull()?;
                    Ok(match (before, repo.head_hash()) {
                        (Some(before), Some(after)) if before != after => {
                            repo.changes_between(&before, &after)
                        }
                        _ => (0, 0),
                    })
                },
                |this, pulled, window, cx| this.notify_pulled(pulled, window, cx),
                window,
                cx,
            ),
            RemoteAction::Fetch => self.run_git_with(
                strings.scm_fetch,
                Some(action),
                |repo| {
                    repo.fetch()?;
                    Ok(repo.status()?.behind)
                },
                |this, behind, window, cx| this.notify_fetched(behind, window, cx),
                window,
                cx,
            ),
            RemoteAction::Sync => self.run_git_with(
                strings.scm_sync,
                Some(action),
                |repo| {
                    let before = repo.status()?;
                    let head = repo.head_hash();
                    repo.sync()?;
                    let pulled = match (head, repo.head_hash()) {
                        (Some(before), Some(after)) if before != after => {
                            repo.changes_between(&before, &after).0
                        }
                        _ => 0,
                    };
                    let after = repo.status()?;
                    Ok((pulled, before.ahead, after.upstream))
                },
                |this, (pulled, pushed, upstream), window, cx| {
                    let strings = this.strings();
                    let upstream = upstream.unwrap_or_default();
                    this.notify_user(
                        notices::Notice::success(notices::fill(
                            strings.git_synced,
                            &[("upstream", &upstream)],
                        ))
                        .message(notices::fill(
                            strings.git_synced_why,
                            &[
                                ("pulled", &pulled.to_string()),
                                ("pushed", &pushed.to_string()),
                            ],
                        )),
                        window,
                        cx,
                    );
                },
                window,
                cx,
            ),
            RemoteAction::Stash => self.run_git_with(
                strings.scm_stash,
                Some(action),
                |repo| repo.stash(),
                |this, (), window, cx| {
                    let strings = this.strings();
                    this.notify_user(
                        notices::Notice::success(strings.git_stashed)
                            .message(strings.git_stashed_why)
                            .action(strings.git_stash_bring_back, |this, window, cx| {
                                this.remote_command(RemoteAction::StashPop, window, cx)
                            }),
                        window,
                        cx,
                    );
                },
                window,
                cx,
            ),
            RemoteAction::StashPop => self.run_git_with(
                strings.scm_stash_pop,
                Some(action),
                |repo| repo.stash_pop(),
                |this, (), window, cx| {
                    let strings = this.strings();
                    this.notify_user(notices::Notice::success(strings.git_unstashed), window, cx);
                },
                window,
                cx,
            ),
        }
    }

    /// The branch in the status bar: switch to another, or create one.
    pub(super) fn choose_branch(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(repo) = self.scm.repo.clone() else {
            return;
        };
        let strings = self.strings();
        let branches = match repo.branches() {
            Ok(branches) => branches,
            Err(error) => {
                self.report(format!("git: {error}"));
                cx.notify();
                return;
            }
        };
        let choices: Vec<git::Branch> = branches
            .into_iter()
            .filter(|branch| !branch.current)
            .collect();
        let mut items = vec![PickItem::new(strings.scm_new_branch)];
        items.extend(choices.iter().map(|branch| {
            let mut item = PickItem::new(branch.name.clone());
            if branch.remote {
                item.detail = Some(strings.scm_remote_branch.to_owned());
            }
            item
        }));
        let chosen = quick_pick::show(
            None,
            Some(strings.scm_choose_branch.to_owned()),
            items,
            window,
            cx,
        );
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Some(index)) = chosen.await else {
                return;
            };
            this.update_in(cx, |this, window, cx| {
                if index == 0 {
                    this.ask_branch_name(window, cx);
                    return;
                }
                let branch = choices[index - 1].clone();
                this.switch_branch(branch, window, cx);
            })
            .ok();
        })
        .detach();
    }

    /// The commit message in a big window, for a long one, as Zed's
    /// Shift+Esc: the same text, kept in step with the field, so Claude's
    /// ✦ and the commit read it too. ⌘↩ commits, Esc or Shift+Esc goes
    /// back to the field.
    pub(super) fn expand_commit_message(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.scm.repo.is_none() || window.has_active_dialog(cx) {
            return;
        }
        let strings = self.strings();
        let text = self.scm.message.read(cx).value().to_string();
        let big = cx.new(|cx| {
            TextareaState::new(window, cx)
                .placeholder(strings.scm_message)
                .rows(BIG_MESSAGE_ROWS)
                .default_value(text)
        });
        let field = self.scm.message.clone();
        cx.subscribe_in(
            &big,
            window,
            move |_, big, event: &InputEvent, window, cx| {
                if matches!(event, InputEvent::Change) {
                    let value = big.read(cx).value().to_string();
                    field.update(cx, |field, cx| field.set_value(value, window, cx));
                }
            },
        )
        .detach();
        let branch = self.scm.status.branch.clone().unwrap_or_default();
        let title = if branch.is_empty() {
            strings.scm_message.to_owned()
        } else {
            format!("{} · {branch}", strings.scm_message)
        };
        let this = cx.weak_entity();
        let shown = big.clone();
        window.open_dialog(cx, move |dialog, _, cx| {
            let colors = Colors::new(cx);
            let hint = this
                .upgrade()
                .and_then(|this| this.read(cx).commit_message_hint(cx));
            let (keys, back, close) = (this.clone(), this.clone(), this.clone());
            dialog
                .w(px(720.))
                .title(title.clone())
                .on_close(move |_, window, cx| {
                    if let Some(this) = back.upgrade() {
                        let field = this.read(cx).scm.message.clone();
                        field.update(cx, |field, cx| field.focus(window, cx));
                    }
                })
                .child(
                    div()
                        .key_context(MESSAGE_CONTEXT)
                        .capture_key_down(move |event, window, cx| {
                            let keystroke = &event.keystroke;
                            let modifiers = keystroke.modifiers;
                            if keystroke.key == "enter" && modifiers.platform && !modifiers.shift {
                                cx.stop_propagation();
                                window.close_dialog(cx);
                                keys.update(cx, |this, cx| this.commit(false, window, cx))
                                    .ok();
                            } else if keystroke.key == "escape" && modifiers.shift {
                                cx.stop_propagation();
                                window.close_dialog(cx);
                            }
                        })
                        .flex()
                        .flex_col()
                        .gap_2()
                        .child(Textarea::new(&shown))
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap_2()
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .text_xs()
                                        .text_color(match &hint {
                                            Some((_, true)) => cx.theme().warning,
                                            _ => colors.faint,
                                        })
                                        .child(match hint.clone() {
                                            Some((hint, _)) => hint,
                                            None => strings.commit_title_long_tip.to_owned(),
                                        }),
                                )
                                .child(key_hint("⌘↩", colors))
                                .child(
                                    Button::new("scm-big-commit")
                                        .primary()
                                        .small()
                                        .icon(Icon::new(IconName::Check))
                                        .label(strings.scm_commit)
                                        .on_click({
                                            let close = close.clone();
                                            move |_, window, cx| {
                                                window.close_dialog(cx);
                                                close
                                                    .update(cx, |this, cx| {
                                                        this.commit(false, window, cx)
                                                    })
                                                    .ok();
                                            }
                                        }),
                                ),
                        ),
                )
        });
        big.update(cx, |big, cx| big.focus(window, cx));
    }

    fn ask_branch_name(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let placeholder = self.strings().scm_branch_name;
        let input = cx.new(|cx| InputState::new(window, cx).placeholder(placeholder));
        let this = cx.weak_entity();
        let field = input.clone();
        window.open_dialog(cx, move |dialog, _, _| {
            let this = this.clone();
            let shown = field.clone();
            let field = field.clone();
            dialog
                .w(px(380.))
                .on_ok(move |_, window, cx| {
                    let name = field.read(cx).value().trim().replace(' ', "-");
                    if !name.is_empty() {
                        this.update(cx, |this, cx| {
                            let label = this.strings().scm_create_branch;
                            let created = name.clone();
                            this.run_git(
                                label,
                                move |repo| repo.create_branch(&name),
                                move |this, window, cx| {
                                    let strings = this.strings();
                                    this.notify_user(
                                        notices::Notice::success(notices::fill(
                                            strings.git_branch_created,
                                            &[("branch", &created)],
                                        )),
                                        window,
                                        cx,
                                    );
                                },
                                window,
                                cx,
                            );
                        })
                        .ok();
                    }
                    true
                })
                .child(Input::new(&shown).small())
        });
        input.update(cx, |input, cx| input.focus(window, cx));
    }

    pub(super) fn source_control_actions(&self, root: Div, cx: &mut Context<Self>) -> Div {
        root.on_action(cx.listener(|this, _: &Commit, window, cx| this.commit(false, window, cx)))
            .on_action(cx.listener(|this, _: &CompareWithBase, window, cx| {
                this.compare_with_base(window, cx)
            }))
            .on_action(cx.listener(|this, _: &ExpandCommitMessage, window, cx| {
                this.expand_commit_message(window, cx)
            }))
            .on_action(
                cx.listener(|this, _: &CommitAmend, window, cx| this.commit_amend(window, cx)),
            )
            .on_action(cx.listener(|this, _: &StageAll, window, cx| {
                let paths = this.file_paths(Section::Changes);
                this.stage_paths(paths, window, cx)
            }))
            .on_action(cx.listener(|this, _: &UnstageAll, window, cx| {
                let paths = this.file_paths(Section::Staged);
                this.unstage_paths(paths, window, cx)
            }))
            .on_action(cx.listener(|this, _: &DiscardAll, window, cx| {
                let paths = this.file_paths(Section::Changes);
                this.discard_paths(paths, window, cx)
            }))
            .on_action(cx.listener(|this, _: &GitPush, window, cx| {
                this.remote_command(RemoteAction::Push, window, cx)
            }))
            .on_action(cx.listener(|this, _: &GitPull, window, cx| {
                this.remote_command(RemoteAction::Pull, window, cx)
            }))
            .on_action(cx.listener(|this, _: &GitFetch, window, cx| {
                this.remote_command(RemoteAction::Fetch, window, cx)
            }))
            .on_action(cx.listener(|this, _: &GitSync, window, cx| {
                this.remote_command(RemoteAction::Sync, window, cx)
            }))
            .on_action(cx.listener(|this, _: &GitStash, window, cx| {
                this.remote_command(RemoteAction::Stash, window, cx)
            }))
            .on_action(cx.listener(|this, _: &GitStashPop, window, cx| {
                this.remote_command(RemoteAction::StashPop, window, cx)
            }))
            .on_action(cx.listener(|this, _: &GitRefresh, _, cx| this.refresh_git(cx)))
            .on_action(cx.listener(|this, _: &ToggleScmFolders, window, cx| {
                this.scm.by_folder = !this.scm.by_folder;
                let mode = if this.scm.by_folder { "tree" } else { "list" };
                this.change_setting("scm.defaultViewMode", Some(Value::from(mode)), window, cx);
                cx.notify();
            }))
            .on_action(
                cx.listener(|this, _: &CheckoutBranch, window, cx| this.choose_branch(window, cx)),
            )
            .on_action(
                cx.listener(|this, _: &CreateBranch, window, cx| this.ask_branch_name(window, cx)),
            )
    }

    // -----------------------------------------------------------------
    // Drawing

    /// The number on the Source Control icon.
    pub(super) fn git_change_count(&self) -> usize {
        self.scm.status.count()
    }

    /// The branch's distance from its upstream for the status bar:
    /// `↓1 ↑2`, or nothing when in step.
    pub(super) fn sync_text(&self) -> Option<String> {
        let status = &self.scm.status;
        if let Some(busy) = self.scm.busy {
            return Some(format!("{busy}…"));
        }
        self.scm.repo.as_ref()?;
        match (&status.upstream, &status.branch) {
            (None, Some(_)) => Some(self.strings().git_publish_short.to_owned()),
            (Some(_), _) if status.ahead > 0 || status.behind > 0 => {
                Some(format!("↓{} ↑{}", status.behind, status.ahead))
            }
            _ => None,
        }
    }

    pub(super) fn render_source_control(
        &self,
        colors: Colors,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let strings = self.strings();
        if self.scm.repo.is_none() {
            return div()
                .size_full()
                .flex()
                .flex_col()
                .child(view_header(strings.view_source_control, colors))
                .child(empty_state(
                    IconName::GitBranch,
                    strings.scm_no_repository,
                    strings.scm_no_repository_hint,
                    colors,
                ))
                .into_any_element();
        }
        let focus = self.focus_handle.clone();
        let by_folder = self.scm.by_folder;
        let header = view_header(strings.view_source_control, colors)
            .child(
                Button::new("scm-pulls")
                    .ghost()
                    .small()
                    .icon(Icon::new(IconName::GitPullRequest))
                    .tooltip(strings.pulls_title_page)
                    .on_click(
                        cx.listener(|this, _, window, cx| this.open_pull_requests(window, cx)),
                    ),
            )
            .child(
                Button::new("scm-all-changes")
                    .ghost()
                    .small()
                    .icon(Icon::new(IconName::Layers))
                    .tooltip(strings.scm_all_changes)
                    .disabled(self.scm.status.changed().next().is_none())
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.open_diff("", diff_view::DiffSource::AllChanges, window, cx)
                    })),
            )
            .child(
                Button::new("scm-refresh")
                    .ghost()
                    .small()
                    .icon(Icon::new(IconName::RefreshCw))
                    .tooltip(strings.scm_refresh)
                    .on_click(cx.listener(|this, _, _, cx| this.refresh_git(cx))),
            )
            .child({
                let focus = focus.clone();
                Button::new("scm-more")
                    .ghost()
                    .small()
                    .icon(Icon::new(IconName::Ellipsis))
                    .dropdown_menu(move |menu, _, _| {
                        menu.action_context(focus.clone())
                            .menu(strings.scm_pull, Box::new(GitPull))
                            .menu(strings.scm_push, Box::new(GitPush))
                            .menu(strings.scm_fetch, Box::new(GitFetch))
                            .menu(strings.scm_sync, Box::new(GitSync))
                            .separator()
                            .menu(strings.scm_commit_amend, Box::new(CommitAmend))
                            .separator()
                            .menu(strings.scm_checkout, Box::new(CheckoutBranch))
                            .menu(strings.scm_create_branch, Box::new(CreateBranch))
                            .menu(strings.command_compare_with_base, Box::new(CompareWithBase))
                            .separator()
                            .menu(strings.scm_stash, Box::new(GitStash))
                            .menu(strings.scm_stash_pop, Box::new(GitStashPop))
                            .menu(strings.command_show_stashes, Box::new(super::ShowStashes))
                            .separator()
                            .menu_with_check(
                                strings.scm_group_by_folder,
                                by_folder,
                                Box::new(ToggleScmFolders),
                            )
                    })
            });

        let branch = self
            .scm
            .status
            .branch
            .clone()
            .unwrap_or_else(|| self.scm.status.head.clone().unwrap_or_default());
        let nothing = self.scm.status.files.is_empty();
        let busy = self.scm.busy.is_some();
        let committing = self.scm.busy == Some(strings.scm_commit);
        // Ahead and behind, or "publish" for a branch with no upstream.
        let sync = self.sync_text().map(|text| {
            let label = if text == "↑" {
                strings.scm_publish.to_owned()
            } else {
                text
            };
            Button::new("scm-sync")
                .ghost()
                .small()
                .icon(Icon::new(IconName::RefreshCw))
                .label(label)
                .tooltip(strings.scm_sync)
                .disabled(busy)
                .on_click(cx.listener(|this, _, window, cx| {
                    let action = if this.scm.status.upstream.is_none() {
                        RemoteAction::Push
                    } else {
                        RemoteAction::Sync
                    };
                    this.remote_command(action, window, cx)
                }))
        });
        let branch_chip = div()
            .id("scm-branch")
            .flex_1()
            .min_w_0()
            .h(px(24.))
            .px_1p5()
            .flex()
            .items_center()
            .gap_1p5()
            .rounded(px(6.))
            .cursor_pointer()
            .hover(|chip| chip.bg(colors.hover))
            .tooltip(move |window, cx| {
                gpui_component::tooltip::Tooltip::new(strings.scm_checkout).build(window, cx)
            })
            .on_click(cx.listener(|this, _, window, cx| this.choose_branch(window, cx)))
            .child(
                Icon::new(IconName::GitBranch)
                    .small()
                    .text_color(colors.faint),
            )
            .child(
                div()
                    .min_w_0()
                    .truncate()
                    .text_xs()
                    .text_color(colors.muted)
                    .child(branch.clone()),
            );
        let typing = self
            .scm
            .message
            .read(cx)
            .focus_handle(cx)
            .is_focused(window);
        // One card, as a message composer: the text on top, under a hairline
        // the branch and its sync. The field lights up while typing.
        let composer = div()
            .key_context(MESSAGE_CONTEXT)
            .mx_1()
            .flex()
            .flex_col()
            .rounded(px(10.))
            .border_1()
            .border_color(if typing {
                colors.accent.opacity(0.7)
            } else {
                colors.border
            })
            .bg(colors.editor)
            .child(
                div()
                    .px_0p5()
                    .pt_1()
                    .text_sm()
                    .child(Textarea::new(&self.scm.message).appearance(false)),
            )
            .child(
                div()
                    .h(px(34.))
                    .px_1()
                    .flex()
                    .items_center()
                    .gap_1()
                    .border_t_1()
                    .border_color(colors.border.opacity(0.6))
                    .child(branch_chip)
                    .children(sync)
                    // ⤢: the message in a big window (Shift+Esc).
                    .child(
                        Button::new("scm-message-expand")
                            .ghost()
                            .xsmall()
                            .icon(Icon::new(IconName::Maximize))
                            .tooltip(strings.scm_expand_message)
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.expand_commit_message(window, cx)
                            })),
                    )
                    // ✦: Claude writes the message from the changes.
                    .child(
                        Button::new("scm-message-claude")
                            .ghost()
                            .xsmall()
                            .icon(Icon::new(IconName::Sparkles))
                            .tooltip(strings.commit_with_claude)
                            .loading(self.scm.writing_message)
                            .disabled(self.scm.writing_message || nothing)
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.write_commit_message(window, cx)
                            })),
                    )
                    .child(match self.commit_message_hint(cx) {
                        // Past what reads well: the count, in yellow.
                        Some((hint, over)) => div()
                            .id("scm-message-hint")
                            .pr_1()
                            .text_xs()
                            .text_color(if over {
                                cx.theme().warning
                            } else {
                                colors.faint
                            })
                            .tooltip(move |window, cx| {
                                gpui_component::tooltip::Tooltip::new(strings.commit_title_long_tip)
                                    .build(window, cx)
                            })
                            .child(hint)
                            .into_any_element(),
                        None => div()
                            .pr_1()
                            .child(key_hint("⌘↩", colors))
                            .into_any_element(),
                    }),
            );
        let just_committed = self.scm.committed_at.is_some();
        let commit = DropdownButton::new("scm-commit-split")
            .primary()
            .mx_1()
            .button(
                Button::new("scm-commit")
                    .flex_1()
                    .icon(Icon::new(if just_committed {
                        IconName::CircleCheck
                    } else {
                        IconName::Check
                    }))
                    .label(if just_committed {
                        strings.git_committed
                    } else {
                        strings.scm_commit
                    })
                    .tooltip(strings.scm_commit_on(&branch))
                    .loading(committing)
                    .disabled(busy || nothing)
                    .on_click(cx.listener(|this, _, window, cx| this.commit(false, window, cx))),
            )
            .menu_accessibility_label(strings.scm_more_commit)
            .dropdown_menu(move |menu, _, _| {
                menu.action_context(focus.clone())
                    .menu(strings.scm_commit_amend, Box::new(CommitAmend))
                    .menu(strings.scm_commit_push, Box::new(super::CommitAndPush))
                    .menu(strings.scm_commit_sync, Box::new(super::CommitAndSync))
                    .separator()
                    .menu(strings.scm_push, Box::new(GitPush))
                    .menu(strings.scm_pull, Box::new(GitPull))
                    .menu(strings.scm_sync, Box::new(GitSync))
            });

        let rows = self.scm.rows();
        let changes: AnyElement = if rows.is_empty() {
            div()
                .flex_1()
                .min_h(px(60.))
                .flex()
                .flex_col()
                .justify_center()
                .child(empty_state(
                    IconName::CircleCheck,
                    strings.scm_clean,
                    strings.scm_clean_hint,
                    colors,
                ))
                .into_any_element()
        } else {
            div()
                .flex_1()
                .min_h(px(60.))
                .child(
                    uniform_list(
                        "scm-files",
                        rows.len(),
                        cx.processor(move |this, range: Range<usize>, window, cx| {
                            let rows = this.scm.rows();
                            range
                                .filter_map(|index| rows.get(index).copied())
                                .map(|row| this.render_scm_row(row, colors, window, cx))
                                .collect::<Vec<_>>()
                        }),
                    )
                    .track_scroll(&self.scm_scroll)
                    .px_1()
                    .size_full(),
                )
                .into_any_element()
        };

        div()
            .size_full()
            .flex()
            .flex_col()
            .gap_2()
            .on_drag_move::<scm_graph::ScmGraphResize>(
                cx.listener(|this, event, _, cx| this.resize_scm_graph(event, cx)),
            )
            .child(header)
            .child(composer)
            .child(commit)
            // The changes take the room the graph leaves; the graph keeps
            // the height it was dragged to.
            .child(changes)
            .child(self.render_scm_graph(colors, window, cx))
            .into_any_element()
    }

    fn render_scm_row(
        &self,
        row: Row,
        colors: Colors,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let strings = self.strings();
        // As large as the rows allow, so they are easy to hit.
        let small = |id: ElementId, icon: IconName, tooltip: &'static str| {
            Button::new(id)
                .ghost()
                .with_size(gpui_component::Size::Size(px(22.)))
                .icon(Icon::new(icon))
                .tooltip(tooltip)
        };
        match row {
            Row::Title(section, count) => {
                let title = match section {
                    Section::Conflicts => strings.scm_conflicts,
                    Section::Staged => strings.scm_staged,
                    Section::Changes => strings.scm_changes,
                };
                let buttons = match section {
                    Section::Staged => vec![
                        small(
                            ("scm-unstage-all", 0usize).into(),
                            IconName::Minus,
                            strings.scm_unstage_all,
                        )
                        .on_click(cx.listener(|this, _, window, cx| {
                            let paths = this.file_paths(Section::Staged);
                            this.unstage_paths(paths, window, cx)
                        })),
                    ],
                    Section::Changes => vec![
                        small(
                            ("scm-discard-all", 0usize).into(),
                            IconName::Undo2,
                            strings.scm_discard_all,
                        )
                        .on_click(cx.listener(|this, _, window, cx| {
                            let paths = this.file_paths(Section::Changes);
                            this.discard_paths(paths, window, cx)
                        })),
                        small(
                            ("scm-stage-all", 0usize).into(),
                            IconName::Plus,
                            strings.scm_stage_all,
                        )
                        .on_click(cx.listener(|this, _, window, cx| {
                            let paths = this.file_paths(Section::Changes);
                            this.stage_paths(paths, window, cx)
                        })),
                    ],
                    Section::Conflicts => Vec::new(),
                };
                let open = !self.scm.collapsed.contains(&section);
                let turn = chevron_turn(
                    ElementId::Name(format!("scm-turn-{section:?}").into()),
                    open,
                    self.look,
                    window,
                    cx,
                );
                section_header(
                    ElementId::Name(format!("scm-title-{section:?}").into()),
                    title,
                    turn,
                    colors,
                )
                .group("scm-title")
                // Every row of the list is as tall and as wide as the others.
                .h(px(24.))
                .w_full()
                .on_click(cx.listener(move |this, _, _, cx| {
                    if !this.scm.collapsed.remove(&section) {
                        this.scm.collapsed.insert(section);
                    }
                    cx.notify();
                }))
                .child(
                    own_clicks(ElementId::Name(
                        format!("scm-title-actions-{section:?}").into(),
                    ))
                    .invisible()
                    .group_hover("scm-title", |buttons| buttons.visible())
                    .children(buttons),
                )
                .child(
                    div()
                        .min_w(px(18.))
                        .h(px(16.))
                        .px_1()
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded_full()
                        .bg(colors.hover)
                        .text_color(colors.muted)
                        .child(count.to_string()),
                )
                .into_any_element()
            }
            Row::Folder(section, first, count) => {
                let Some(file) = self.scm.status.files.get(first) else {
                    return div().into_any_element();
                };
                let folder = folder_of(&file.path).to_owned();
                let open = !self.scm.folders_closed.contains(&(section, folder.clone()));
                let turn = chevron_turn(
                    ElementId::Name(format!("scm-folder-turn-{section:?}-{folder}").into()),
                    open,
                    self.look,
                    window,
                    cx,
                );
                let key = (section, folder.clone());
                div()
                    .id(ElementId::Name(
                        format!("scm-folder-{section:?}-{folder}").into(),
                    ))
                    .h(px(24.))
                    .w_full()
                    .pl_1()
                    .pr_2()
                    .flex()
                    .items_center()
                    .gap_1()
                    .rounded(px(6.))
                    .text_sm()
                    .cursor_pointer()
                    .hover(|row| row.bg(colors.hover))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if !this.scm.folders_closed.remove(&key) {
                            this.scm.folders_closed.insert(key.clone());
                        }
                        cx.notify();
                    }))
                    .child(
                        Icon::new(IconName::ChevronRight)
                            .xsmall()
                            .rotate(gpui::radians(turn * std::f32::consts::FRAC_PI_2))
                            .text_color(colors.faint),
                    )
                    .child(
                        Icon::new(if open {
                            IconName::FolderOpen
                        } else {
                            IconName::Folder
                        })
                        .small()
                        .text_color(colors.accent),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_color(colors.muted)
                            .child(folder),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(colors.faint)
                            .child(count.to_string()),
                    )
                    .into_any_element()
            }
            Row::File(section, index) => {
                let Some(file) = self.scm.status.files.get(index) else {
                    return div().into_any_element();
                };
                let in_folder = self.scm.by_folder && !folder_of(&file.path).is_empty();
                let path = file.path.clone();
                let (folder, name) = match path.rsplit_once('/') {
                    // Grouped, the folder is on its own row above.
                    Some((_, name)) if in_folder => (String::new(), name.to_owned()),
                    Some((folder, name)) => (folder.to_owned(), name.to_owned()),
                    None => (String::new(), path.clone()),
                };
                let letter = if file.conflict {
                    "!"
                } else if section == Section::Staged {
                    file.index.letter()
                } else {
                    file.worktree.letter()
                };
                let color = change_color(file, section, cx);
                let deleted = matches!(
                    if section == Section::Staged {
                        file.index
                    } else {
                        file.worktree
                    },
                    git::Change::Deleted
                );
                let id = ElementId::Name(format!("scm-{section:?}-{path}").into());
                let mut buttons = Vec::new();
                if !deleted {
                    let path = path.clone();
                    buttons.push(
                        small(
                            (id.clone(), "open").into(),
                            IconName::File,
                            strings.scm_open_file,
                        )
                        .on_click(cx.listener(
                            move |this, _, window, cx| {
                                if let Some(repo) = this.scm.repo.clone() {
                                    this.open_file(&repo.absolute(&path), window, cx);
                                }
                            },
                        )),
                    );
                }
                match section {
                    Section::Staged => {
                        let path = path.clone();
                        buttons.push(
                            small(
                                (id.clone(), "unstage").into(),
                                IconName::Minus,
                                strings.scm_unstage,
                            )
                            .on_click(cx.listener(
                                move |this, _, window, cx| {
                                    this.unstage_paths(vec![path.clone()], window, cx)
                                },
                            )),
                        );
                    }
                    Section::Changes | Section::Conflicts => {
                        if section == Section::Changes {
                            let path = path.clone();
                            buttons.push(
                                small(
                                    (id.clone(), "discard").into(),
                                    IconName::Undo2,
                                    strings.scm_discard,
                                )
                                .on_click(cx.listener(
                                    move |this, _, window, cx| {
                                        this.discard_paths(vec![path.clone()], window, cx)
                                    },
                                )),
                            );
                        }
                        let path = path.clone();
                        buttons.push(
                            small(
                                (id.clone(), "stage").into(),
                                IconName::Plus,
                                strings.scm_stage,
                            )
                            .on_click(cx.listener(
                                move |this, _, window, cx| {
                                    this.stage_paths(vec![path.clone()], window, cx)
                                },
                            )),
                        );
                    }
                }
                let open_path = path.clone();
                let dragged = self
                    .scm
                    .repo
                    .as_ref()
                    .map(|repo| repo.absolute(&path))
                    .filter(|absolute| !deleted && absolute.exists())
                    .map(|absolute| super::explorer::DraggedPath {
                        name: name.clone(),
                        path: absolute,
                        others: Vec::new(),
                    });
                let icon = self.file_icon_element(Path::new(&path), cx);
                // Just staged or unstaged: it slides into its section.
                let arrived = self
                    .scm
                    .arrived
                    .contains(&path)
                    .then(|| format!("{}-{}-{path}", self.scm.arrival, section as u8));
                let row = div()
                    .id(id.clone())
                    .group("scm-row")
                    .relative()
                    .h(px(24.))
                    .w_full()
                    .pl(px(if in_folder { 26. } else { 8. }))
                    .pr_1()
                    .flex()
                    .items_center()
                    .gap_1p5()
                    .rounded(px(6.))
                    .text_sm()
                    .cursor_pointer()
                    .hover(|row| row.bg(colors.hover))
                    .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                        this.open_change(&open_path, section, window, cx)
                    }))
                    // Dragged as from the Explorer: into the terminal it
                    // writes its path, into the editor it opens, into the
                    // chat it goes with the message. A deleted file stays.
                    .when_some(dragged, |row, dragged| {
                        row.on_drag(dragged, |dragged, _, _, cx| cx.new(|_| dragged.clone()))
                    })
                    .child(icon)
                    // The name keeps its room; the folder gives way first.
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .items_center()
                            .gap_1p5()
                            .overflow_hidden()
                            .child(
                                div()
                                    .min_w_0()
                                    .truncate()
                                    .text_color(if deleted { colors.muted } else { colors.text })
                                    .when(deleted, |name| name.line_through())
                                    .child(name),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .min_w(px(24.))
                                    .truncate()
                                    .text_xs()
                                    .text_color(colors.faint)
                                    .child(folder),
                            ),
                    )
                    // The state, always in the same place on the right.
                    .child(
                        div()
                            .debug_selector(|| format!("scm-badge-{path}"))
                            .size(px(16.))
                            .flex_none()
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded(px(4.))
                            .bg(color.opacity(0.16))
                            .text_size(px(10.))
                            .font_weight(FontWeight::BOLD)
                            .text_color(color)
                            .child(letter),
                    )
                    // On hover the actions cover the end of the row, so
                    // nothing moves.
                    .child(
                        own_clicks((id, "actions"))
                            .absolute()
                            .top_0()
                            .bottom_0()
                            .right(px(22.))
                            .pl_1()
                            .gap_0p5()
                            .rounded(px(6.))
                            .bg(colors.hover)
                            .invisible()
                            .group_hover("scm-row", |buttons| buttons.visible())
                            .children(buttons),
                    )
                    .into_any_element();
                match arrived {
                    Some(key) => super::motion_bits::arriving(row, &key, self.look),
                    None => row,
                }
            }
        }
    }

    /// A click on a changed file opens its differences, or its conflicts.
    pub(super) fn open_change(
        &mut self,
        path: &str,
        section: Section,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let source = match section {
            Section::Conflicts => diff_view::DiffSource::Conflict,
            Section::Staged => diff_view::DiffSource::Staged,
            Section::Changes => diff_view::DiffSource::Changes,
        };
        self.open_diff(path, source, window, cx);
    }

    /// Called when the status changed: open differences follow.
    fn git_status_changed(&mut self, cx: &mut Context<Self>) {
        self.refresh_open_diffs(cx);
        self.git_graph_changed(cx);
        self.reload_sidebar_graph(cx);
        // A commit or a checkout changes who wrote the lines.
        let blamed: Vec<String> = self.blame.keys().cloned().collect();
        for uri in blamed {
            self.refresh_blame(&uri, false, cx);
        }
        self.forget_git_baselines(cx);
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum RemoteAction {
    Push,
    Pull,
    Fetch,
    Sync,
    Stash,
    StashPop,
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::process::Command;

    use gpui::TestAppContext;

    use super::*;

    fn git(folder: &Path, args: &[&str]) {
        let status = Command::new("git")
            .arg("-C")
            .arg(folder)
            .args(args)
            .output()
            .unwrap()
            .status;
        assert!(status.success(), "git {args:?}");
    }

    /// The rows follow the sidebar's width: the state of every file sits
    /// at its right edge, for short and long names, narrow or wide.
    #[gpui::test]
    fn rows_follow_the_width_of_the_sidebar(cx: &mut TestAppContext) {
        cx.executor().allow_parking();
        cx.update(|cx| {
            gpui_component::init(cx);
            crate::workbench_window::init(cx);
        });
        let folder = tempfile::tempdir().unwrap();
        let root = fs::canonicalize(folder.path()).unwrap();
        git(&root, &["init", "-q", "-b", "main"]);
        git(&root, &["config", "user.name", "Test"]);
        git(&root, &["config", "user.email", "test@example.com"]);
        git(&root, &["config", "commit.gpgsign", "false"]);
        let long = "una-cartella-davvero-lunga/con-un-nome-di-file-molto-molto-lungo.ts";
        fs::create_dir_all(root.join("una-cartella-davvero-lunga")).unwrap();
        for file in ["a.ts", long] {
            fs::write(root.join(file), "uno\n").unwrap();
        }
        git(&root, &["add", "."]);
        git(&root, &["commit", "-q", "-m", "first"]);
        for file in ["a.ts", long] {
            fs::write(root.join(file), "due\n").unwrap();
        }
        let settings = Settings::load("/nonexistent/keel-test/settings.json", None);
        let opening = Opening::new(Some(root.clone()), None);
        let (view, cx) =
            cx.add_window_view(|window, cx| Workbench::new(opening, settings, None, window, cx));
        view.update(cx, |view, _| {
            view.layout
                .select_side_view(crate::layout::SideView::SourceControl)
        });
        let selector = |path: &str| -> &'static str {
            Box::leak(format!("scm-badge-{path}").into_boxed_str())
        };
        let (short, long_badge) = (selector("a.ts"), selector(long));
        let mut ready = false;
        for _ in 0..100 {
            cx.executor().advance_clock(REFRESH_PAUSE * 2);
            cx.update(|window, _| window.refresh());
            cx.run_until_parked();
            if cx.debug_bounds(short).is_some()
                && cx.debug_bounds(long_badge).is_some()
                && cx.debug_bounds("scm-graph-when-0").is_some()
            {
                ready = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(ready, "the two changed files and the commit are listed");
        for width in [240., 420.] {
            view.update(cx, |view, cx| {
                view.sidebar_width = width;
                cx.notify();
            });
            cx.update(|window, _| window.refresh());
            cx.run_until_parked();
            let sidebar = cx.debug_bounds("sidebar").unwrap();
            let (a, b) = (
                cx.debug_bounds(short).unwrap(),
                cx.debug_bounds(long_badge).unwrap(),
            );
            assert_eq!(f32::from(sidebar.size.width), width);
            // Both at the same place, inside the sidebar, near its edge.
            assert_eq!(a.right(), b.right(), "aligned at {width}");
            let gap = f32::from(sidebar.right() - a.right());
            assert!((0. ..30.).contains(&gap), "{gap} from the edge at {width}");
            // The date of the commit in the graph too.
            let when = cx.debug_bounds("scm-graph-when-0").unwrap();
            let gap = f32::from(sidebar.right() - when.right());
            assert!(
                (0. ..30.).contains(&gap),
                "date {gap} from the edge at {width}"
            );
        }
    }

    #[gpui::test]
    fn stage_and_commit_from_the_view(cx: &mut TestAppContext) {
        cx.executor().allow_parking();
        cx.update(|cx| {
            gpui_component::init(cx);
            crate::workbench_window::init(cx);
        });
        let folder = tempfile::tempdir().unwrap();
        let root = fs::canonicalize(folder.path()).unwrap();
        git(&root, &["init", "-q", "-b", "main"]);
        git(&root, &["config", "user.name", "Test"]);
        git(&root, &["config", "user.email", "test@example.com"]);
        git(&root, &["config", "commit.gpgsign", "false"]);
        fs::write(root.join("a.txt"), "uno\n").unwrap();
        git(&root, &["add", "."]);
        git(&root, &["commit", "-q", "-m", "first"]);
        fs::write(root.join("a.txt"), "uno\ndue\n").unwrap();
        fs::write(root.join("b.txt"), "nuovo\n").unwrap();

        let config = tempfile::tempdir().unwrap();
        let settings_file = config.path().join("settings.json");
        fs::write(&settings_file, r#"{ "keel.language": "en" }"#).unwrap();
        let settings = Settings::load(&settings_file, None);
        let opening = Opening::new(Some(root.clone()), None);
        let (view, cx) = cx.add_window_view(|window, cx| {
            gpui_component::Root::new(
                cx.new(|cx| Workbench::new(opening, settings, None, window, cx)),
                window,
                cx,
            )
        });
        let view = view.read_with(cx, |root, _| {
            root.view().clone().downcast::<Workbench>().unwrap()
        });
        let settle = |cx: &mut gpui::VisualTestContext| {
            for _ in 0..50 {
                cx.executor().advance_clock(REFRESH_PAUSE * 2);
                cx.run_until_parked();
                if view.read_with(cx, |view, _| view.scm.busy.is_none()) {
                    break;
                }
                std::thread::sleep(Duration::from_millis(20));
            }
        };
        settle(cx);
        view.read_with(cx, |view, _| {
            assert_eq!(view.git_change_count(), 2);
            assert_eq!(view.file_paths(Section::Changes), ["a.txt", "b.txt"]);
        });
        // A closed section keeps its title and count, without its files.
        view.update(cx, |view, _| {
            view.scm.collapsed.insert(Section::Changes);
            assert_eq!(view.scm.rows(), [Row::Title(Section::Changes, 2)]);
            view.scm.collapsed.clear();
        });

        view.update_in(cx, |view, window, cx| {
            view.stage_paths(vec!["a.txt".into()], window, cx)
        });
        settle(cx);
        assert_eq!(
            view.read_with(cx, |view, _| view.file_paths(Section::Staged)),
            ["a.txt"]
        );

        // Cmd+Enter in the message commits what is staged.
        view.update(cx, |view, cx| {
            view.layout.show_side_view(SideView::SourceControl);
            cx.notify();
        });
        cx.run_until_parked();
        view.update_in(cx, |view, window, cx| {
            view.scm
                .message
                .update(cx, |input, cx| input.set_value("add due", window, cx));
            view.scm
                .message
                .update(cx, |input, cx| input.focus(window, cx));
        });
        cx.simulate_keystrokes("cmd-enter");
        settle(cx);
        let log = Command::new("git")
            .arg("-C")
            .arg(&root)
            .args(["log", "--format=%s"])
            .output()
            .unwrap();
        assert_eq!(String::from_utf8_lossy(&log.stdout), "add due\nfirst\n");
        view.read_with(cx, |view, cx| {
            assert_eq!(view.file_paths(Section::Changes), ["b.txt"]);
            assert!(view.scm.message.read(cx).value().is_empty());
        });

        // With nothing staged, Commit offers to stage everything.
        view.update_in(cx, |view, window, cx| {
            view.scm
                .message
                .update(cx, |input, cx| input.set_value("add b", window, cx));
            view.commit(false, window, cx);
        });
        assert!(cx.has_pending_prompt());
        cx.simulate_prompt_answer("Stage All and Commit");
        settle(cx);
        assert_eq!(view.read_with(cx, |view, _| view.git_change_count()), 0);
    }

    /// Shift+Esc in the message opens it in a big window: typing there
    /// changes the field, and Cmd+Enter commits.
    #[gpui::test]
    fn a_long_commit_message_is_written_in_a_big_window(cx: &mut TestAppContext) {
        cx.executor().allow_parking();
        cx.update(|cx| {
            gpui_component::init(cx);
            crate::workbench_window::init(cx);
        });
        let folder = tempfile::tempdir().unwrap();
        let root = fs::canonicalize(folder.path()).unwrap();
        git(&root, &["init", "-q", "-b", "main"]);
        git(&root, &["config", "user.name", "Test"]);
        git(&root, &["config", "user.email", "test@example.com"]);
        git(&root, &["config", "commit.gpgsign", "false"]);
        fs::write(root.join("a.txt"), "uno\n").unwrap();
        git(&root, &["add", "."]);
        git(&root, &["commit", "-q", "-m", "first"]);
        fs::write(root.join("a.txt"), "uno\ndue\n").unwrap();
        git(&root, &["add", "."]);

        let config = tempfile::tempdir().unwrap();
        let settings_file = config.path().join("settings.json");
        fs::write(&settings_file, r#"{ "keel.language": "en" }"#).unwrap();
        let settings = Settings::load(&settings_file, None);
        let opening = Opening::new(Some(root.clone()), None);
        let (view, cx) = cx.add_window_view(|window, cx| {
            gpui_component::Root::new(
                cx.new(|cx| Workbench::new(opening, settings, None, window, cx)),
                window,
                cx,
            )
        });
        let view = view.read_with(cx, |root, _| {
            root.view().clone().downcast::<Workbench>().unwrap()
        });
        let settle = |cx: &mut gpui::VisualTestContext| {
            for _ in 0..50 {
                cx.executor().advance_clock(REFRESH_PAUSE * 2);
                cx.run_until_parked();
                if view.read_with(cx, |view, _| view.scm.busy.is_none()) {
                    break;
                }
                std::thread::sleep(Duration::from_millis(20));
            }
        };
        settle(cx);
        view.update(cx, |view, cx| {
            view.layout.show_side_view(SideView::SourceControl);
            cx.notify();
        });
        cx.run_until_parked();
        view.update_in(cx, |view, window, cx| {
            view.scm
                .message
                .update(cx, |input, cx| input.set_value("add due", window, cx));
            view.scm
                .message
                .update(cx, |input, cx| input.focus(window, cx));
        });
        cx.simulate_keystrokes("shift-escape");
        cx.run_until_parked();
        assert!(cx.update(|window, cx| window.has_active_dialog(cx)));
        // The big field starts with the text and gives it back as it changes.
        cx.simulate_keystrokes("cmd-down enter enter");
        cx.simulate_input("Why: the second line.");
        cx.run_until_parked();
        assert_eq!(
            view.read_with(cx, |view, cx| view.scm.message.read(cx).value().to_string()),
            "add due\n\nWhy: the second line."
        );
        cx.simulate_keystrokes("cmd-enter");
        settle(cx);
        assert!(!cx.update(|window, cx| window.has_active_dialog(cx)));
        let log = Command::new("git")
            .arg("-C")
            .arg(&root)
            .args(["log", "-1", "--format=%B"])
            .output()
            .unwrap();
        assert_eq!(
            String::from_utf8_lossy(&log.stdout).trim_end(),
            "add due\n\nWhy: the second line."
        );
    }

    /// Git and the Explorer speak to each other, and every git command
    /// says what it did: colours in the tree, the tree on the file whose
    /// differences are open, a notice after a commit with Undo, after a
    /// push, and a window with the fix when the remote is ahead.
    #[gpui::test]
    fn git_colours_the_tree_and_says_what_it_did(cx: &mut TestAppContext) {
        cx.executor().allow_parking();
        cx.update(|cx| {
            gpui_component::init(cx);
            crate::workbench_window::init(cx);
        });
        let folder = tempfile::tempdir().unwrap();
        let base = fs::canonicalize(folder.path()).unwrap();
        let remote = base.join("remote.git");
        let root = base.join("app");
        let other = base.join("other");
        fs::create_dir_all(&root).unwrap();
        git(&base, &["init", "-q", "--bare", "-b", "main", "remote.git"]);
        git(&root, &["init", "-q", "-b", "main"]);
        git(&root, &["config", "user.name", "Test"]);
        git(&root, &["config", "user.email", "test@example.com"]);
        git(&root, &["config", "commit.gpgsign", "false"]);
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(root.join("a.txt"), "uno\n").unwrap();
        git(&root, &["add", "."]);
        git(&root, &["commit", "-q", "-m", "first"]);
        git(
            &root,
            &["remote", "add", "origin", remote.to_str().unwrap()],
        );
        fs::write(root.join("a.txt"), "uno\ndue\n").unwrap();
        fs::write(root.join("src/b.txt"), "nuovo\n").unwrap();

        let config = tempfile::tempdir().unwrap();
        let settings_file = config.path().join("settings.json");
        fs::write(&settings_file, r#"{ "keel.language": "en" }"#).unwrap();
        let settings = Settings::load(&settings_file, None);
        let opening = Opening::new(Some(root.clone()), None);
        let (view, cx) = cx.add_window_view(|window, cx| {
            gpui_component::Root::new(
                cx.new(|cx| Workbench::new(opening, settings, None, window, cx)),
                window,
                cx,
            )
        });
        let view = view.read_with(cx, |root, _| {
            root.view().clone().downcast::<Workbench>().unwrap()
        });
        let settle = |cx: &mut gpui::VisualTestContext| {
            for _ in 0..80 {
                cx.executor().advance_clock(REFRESH_PAUSE * 2);
                cx.run_until_parked();
                if view.read_with(cx, |view, _| view.scm.busy.is_none()) {
                    break;
                }
                std::thread::sleep(Duration::from_millis(20));
            }
        };
        let last = |cx: &mut gpui::VisualTestContext| {
            view.read_with(cx, |view, _| {
                let notice = view.notices.list.front().expect("a notice");
                (notice.title.clone(), notice.message.clone())
            })
        };
        settle(cx);

        // M on the changed file, U on the new one, the folder's dot.
        view.read_with(cx, |view, _| {
            use super::git_marks::Mark;
            assert_eq!(
                view.git_marks.file(&root.join("a.txt")),
                Some(Mark::Modified)
            );
            assert_eq!(
                view.git_marks.file(&root.join("src/b.txt")),
                Some(Mark::Untracked)
            );
            assert_eq!(
                view.git_marks.folder(&root.join("src")),
                Some(Mark::Untracked)
            );
            assert_eq!(view.git_marks.folder(&root), Some(Mark::Modified));
        });

        // A file's differences take the tree to the file.
        view.update_in(cx, |view, window, cx| {
            view.open_change("src/b.txt", Section::Changes, window, cx);
        });
        view.read_with(cx, |view, _| {
            assert_eq!(
                view.tree_selection.as_deref(),
                Some(root.join("src/b.txt").as_path())
            );
        });

        // A commit says so, and Undo takes it back with its message.
        view.update_in(cx, |view, window, cx| {
            view.stage_paths(vec!["a.txt".into()], window, cx)
        });
        settle(cx);
        view.update_in(cx, |view, window, cx| {
            view.scm
                .message
                .update(cx, |input, cx| input.set_value("add due", window, cx));
            view.commit(false, window, cx);
        });
        settle(cx);
        assert_eq!(
            last(cx),
            ("Committed".to_owned(), Some("add due".to_owned()))
        );
        view.read_with(cx, |view, _| assert!(view.scm.committed_at.is_some()));
        view.update_in(cx, |view, window, cx| {
            let (label, undo) = view.notices.list[0].actions[0].clone();
            assert_eq!(label, "Undo");
            undo(view, window, cx);
        });
        settle(cx);
        assert_eq!(last(cx).0, "Commit undone");
        view.read_with(cx, |view, cx| {
            assert_eq!(view.scm.message.read(cx).value().to_string(), "add due");
            assert_eq!(view.file_paths(Section::Staged), ["a.txt"]);
        });

        // The first push publishes the branch, the next one counts.
        view.update_in(cx, |view, window, cx| view.commit(false, window, cx));
        settle(cx);
        view.update_in(cx, |view, window, cx| {
            view.remote_command(RemoteAction::Push, window, cx)
        });
        settle(cx);
        assert_eq!(last(cx).0, "Branch main published to origin");
        fs::write(root.join("a.txt"), "uno\ndue\ntre\n").unwrap();
        git(&root, &["commit", "-q", "-am", "tre"]);
        view.update_in(cx, |view, window, cx| {
            view.remote_command(RemoteAction::Push, window, cx)
        });
        settle(cx);
        assert_eq!(last(cx).0, "Pushed 1 commit to origin/main");

        // Someone else pushes: ours is rejected, and the window offers
        // the pull that fixes it.
        git(&base, &["clone", "-q", remote.to_str().unwrap(), "other"]);
        git(&other, &["config", "user.name", "Other"]);
        git(&other, &["config", "user.email", "other@example.com"]);
        git(&other, &["config", "commit.gpgsign", "false"]);
        fs::write(other.join("c.txt"), "altro\n").unwrap();
        git(&other, &["add", "."]);
        git(&other, &["commit", "-q", "-m", "theirs"]);
        git(&other, &["push", "-q"]);
        fs::write(root.join("d.txt"), "mio\n").unwrap();
        git(&root, &["add", "d.txt"]);
        git(&root, &["commit", "-q", "-m", "mine"]);
        git(&root, &["config", "pull.rebase", "false"]);
        view.update_in(cx, |view, window, cx| {
            view.remote_command(RemoteAction::Push, window, cx)
        });
        settle(cx);
        assert!(cx.has_pending_prompt());
        cx.simulate_prompt_answer("Pull and Push");
        settle(cx);
        assert_eq!(last(cx).0, "Synced with origin/main");
        let log = Command::new("git")
            .arg("-C")
            .arg(&remote)
            .args(["log", "--format=%s", "main"])
            .output()
            .unwrap();
        let log = String::from_utf8_lossy(&log.stdout);
        assert!(log.contains("mine") && log.contains("theirs"), "{log}");
    }
}
