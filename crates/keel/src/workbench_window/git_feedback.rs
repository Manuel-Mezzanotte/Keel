//! What git did, in words: a notice after a push, a pull, a commit or a
//! stash, with the button that comes next (Open on GitHub, Undo, See the
//! commits); and when git fails, a window that says why in plain words
//! and offers what fixes it, such as «Pull and push» for a rejected push.

use super::notices::{Notice, NoticeAction, fill};
use super::source_control::RemoteAction;
use super::*;

/// Why git failed, read from what it wrote.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Problem {
    /// The remote has commits this branch has not: pull first.
    Behind,
    /// The branch is not on the remote yet.
    NoUpstream,
    /// The repository has no remote at all.
    NoRemote,
    /// A merge stopped on files changed on both sides.
    Conflicts,
    /// Uncommitted changes are in the way of a pull or a switch.
    LocalChanges,
    /// The remote did not let you in.
    Auth,
    /// The remote did not answer.
    Offline,
    NothingToCommit,
    Other,
}

/// Reads git's message. The checks go from the most to the least telling,
/// since git often says more than one thing.
pub(super) fn classify(message: &str) -> Problem {
    let text = message.to_lowercase();
    let has = |words: &[&str]| words.iter().any(|word| text.contains(word));
    if has(&["conflict", "unmerged", "fix conflicts"]) {
        Problem::Conflicts
    } else if has(&[
        "would be overwritten",
        "commit your changes or stash them",
        "please commit or stash",
    ]) {
        Problem::LocalChanges
    } else if has(&["no upstream branch", "has no upstream"]) {
        Problem::NoUpstream
    } else if has(&[
        "no remote to push to",
        "no configured push destination",
        "does not appear to be a git repository",
    ]) {
        Problem::NoRemote
    } else if has(&[
        "non-fast-forward",
        "fetch first",
        "updates were rejected",
        "tip of your current branch is behind",
    ]) {
        Problem::Behind
    } else if has(&[
        "authentication failed",
        "permission denied",
        "could not read username",
        "could not read password",
        "terminal prompts disabled",
        "403",
        "access denied",
    ]) {
        Problem::Auth
    } else if has(&[
        "could not resolve host",
        "unable to access",
        "connection timed out",
        "connection refused",
        "network is unreachable",
        "operation timed out",
    ]) {
        Problem::Offline
    } else if has(&["nothing to commit", "no changes added to commit"]) {
        Problem::NothingToCommit
    } else {
        Problem::Other
    }
}

/// «1 commit», «3 commit» / «3 commits».
fn commits(count: usize, strings: &Strings) -> String {
    let text = if count == 1 {
        strings.notice_commit_one
    } else {
        strings.notice_commit_many
    };
    fill(text, &[("count", &count.to_string())])
}

/// What a push did.
pub(super) struct Pushed {
    pub(super) commits: u32,
    /// The branch had no upstream and now has one.
    pub(super) published: bool,
    pub(super) branch: Option<String>,
    pub(super) upstream: Option<String>,
}

impl Workbench {
    /// Git failed: a window with why and what to do. `retry` is the
    /// remote command that failed, to offer it again.
    pub(super) fn git_failed(
        &mut self,
        label: &'static str,
        message: String,
        retry: Option<RemoteAction>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let strings = self.strings();
        let problem = classify(&message);
        let run = |action: RemoteAction| -> NoticeAction {
            Rc::new(move |this: &mut Workbench, window, cx| this.remote_command(action, window, cx))
        };
        let open_scm: NoticeAction =
            Rc::new(|this: &mut Workbench, _, cx| this.show_side_view(SideView::SourceControl, cx));
        let open_terminal: NoticeAction = Rc::new(|this: &mut Workbench, window, cx| {
            if !this.layout.panel || this.layout.panel_view != PanelView::Terminal {
                this.toggle_terminal(window, cx);
            }
        });
        let (title, explanation, fixes): (&str, &str, Vec<(String, NoticeAction)>) = match problem {
            Problem::Behind => (
                strings.git_problem_behind,
                strings.git_problem_behind_why,
                vec![(
                    strings.git_fix_pull_push.to_owned(),
                    run(RemoteAction::Sync),
                )],
            ),
            Problem::NoUpstream => (
                strings.git_problem_no_upstream,
                strings.git_problem_no_upstream_why,
                vec![(strings.git_fix_publish.to_owned(), run(RemoteAction::Push))],
            ),
            Problem::NoRemote => (
                strings.git_problem_no_remote,
                strings.git_problem_no_remote_why,
                vec![(strings.git_fix_terminal.to_owned(), open_terminal)],
            ),
            Problem::Conflicts => (
                strings.git_problem_conflicts,
                strings.git_problem_conflicts_why,
                vec![(strings.git_fix_open_scm.to_owned(), open_scm)],
            ),
            Problem::LocalChanges => {
                let mut fixes = Vec::new();
                if let Some(action) =
                    retry.filter(|action| matches!(action, RemoteAction::Pull | RemoteAction::Sync))
                {
                    fixes.push((
                        strings.git_fix_stash_retry.to_owned(),
                        Rc::new(
                            move |this: &mut Workbench,
                                  window: &mut Window,
                                  cx: &mut Context<Workbench>| {
                                this.stash_around(action, window, cx)
                            },
                        ) as NoticeAction,
                    ));
                }
                fixes.push((strings.git_fix_stash.to_owned(), run(RemoteAction::Stash)));
                (
                    strings.git_problem_local_changes,
                    strings.git_problem_local_changes_why,
                    fixes,
                )
            }
            Problem::Auth => (
                strings.git_problem_auth,
                strings.git_problem_auth_why,
                vec![(strings.git_fix_terminal.to_owned(), open_terminal)],
            ),
            Problem::Offline => (
                strings.git_problem_offline,
                strings.git_problem_offline_why,
                retry
                    .map(|action| vec![(strings.git_fix_retry.to_owned(), run(action))])
                    .unwrap_or_default(),
            ),
            Problem::NothingToCommit => {
                self.notify_user(
                    Notice::info(strings.git_nothing_to_commit)
                        .message(strings.git_nothing_to_commit_why),
                    window,
                    cx,
                );
                return;
            }
            Problem::Other => (
                "",
                strings.git_problem_other_why,
                retry
                    .map(|action| vec![(strings.git_fix_retry.to_owned(), run(action))])
                    .unwrap_or_default(),
            ),
        };
        let title = if title.is_empty() {
            fill(strings.git_problem_other, &[("label", label)])
        } else {
            title.to_owned()
        };
        self.show_error(title, explanation.to_owned(), message, fixes, window, cx);
    }

    /// Puts the changes aside, runs a pull or a sync, and brings them back.
    pub(super) fn stash_around(
        &mut self,
        action: RemoteAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let label = self.strings().scm_pull;
        self.run_git_with(
            label,
            Some(action),
            move |repo| {
                repo.stash()?;
                let outcome = match action {
                    RemoteAction::Sync => repo.sync(),
                    _ => repo.pull(),
                };
                // The changes come back even when the pull failed.
                let back = repo.stash_pop();
                outcome?;
                back
            },
            |this, (), window, cx| {
                let strings = this.strings();
                this.notify_user(
                    Notice::success(strings.git_stash_retry_done)
                        .message(strings.git_stash_retry_done_why),
                    window,
                    cx,
                );
            },
            window,
            cx,
        );
    }

    /// After a push: how many commits went where, with the page to see
    /// them; a published branch offers its pull request.
    pub(super) fn notify_pushed(
        &mut self,
        pushed: Pushed,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let strings = self.strings();
        let upstream = pushed
            .upstream
            .clone()
            .or_else(|| {
                pushed
                    .branch
                    .as_ref()
                    .map(|branch| format!("origin/{branch}"))
            })
            .unwrap_or_default();
        let remote = upstream.split('/').next().unwrap_or("origin").to_owned();
        let web = self
            .scm
            .repo
            .as_ref()
            .and_then(|repo| repo.remote_web_url(&remote));
        let branch = pushed.branch.clone().unwrap_or_default();
        let notice = if pushed.published {
            let notice = Notice::success(fill(
                strings.git_published,
                &[("branch", &branch), ("remote", &remote)],
            ))
            .message(strings.git_published_why);
            match web {
                Some(web) => {
                    let pull_request = format!("{web}/pull/new/{branch}");
                    notice.action(strings.git_create_pull_request, move |_, _, cx| {
                        cx.open_url(&pull_request)
                    })
                }
                None => notice,
            }
        } else if pushed.commits == 0 {
            Notice::info(strings.git_push_nothing).message(fill(
                strings.git_push_nothing_why,
                &[("upstream", &upstream)],
            ))
        } else {
            let notice = Notice::success(fill(
                strings.git_pushed,
                &[
                    ("commits", &commits(pushed.commits as usize, strings)),
                    ("upstream", &upstream),
                ],
            ));
            match web {
                Some(web) => {
                    let page = if branch.is_empty() {
                        web
                    } else {
                        format!("{web}/tree/{branch}")
                    };
                    notice.action(strings.git_open_in_browser, move |_, _, cx| {
                        cx.open_url(&page)
                    })
                }
                None => notice,
            }
        };
        self.notify_user(notice, window, cx);
    }

    /// After a pull: how many commits and files came, with Git Graph to
    /// see them.
    pub(super) fn notify_pulled(
        &mut self,
        (commit_count, file_count): (usize, usize),
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let strings = self.strings();
        let notice = if commit_count == 0 {
            Notice::info(strings.git_pull_nothing)
        } else {
            Notice::success(fill(
                strings.git_pulled,
                &[("commits", &commits(commit_count, strings))],
            ))
            .message(fill(
                strings.git_pulled_files,
                &[("count", &file_count.to_string())],
            ))
            .action(strings.git_see_commits, |this, window, cx| {
                window.dispatch_action(Box::new(ShowGitGraph), cx);
                let _ = this;
            })
        };
        self.notify_user(notice, window, cx);
    }

    /// After a fetch: how many commits wait to be pulled.
    pub(super) fn notify_fetched(
        &mut self,
        behind: u32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let strings = self.strings();
        let notice = if behind == 0 {
            Notice::info(strings.git_fetch_nothing)
        } else {
            Notice::info(fill(
                strings.git_fetched,
                &[("commits", &commits(behind as usize, strings))],
            ))
            .action(strings.scm_pull, |this, window, cx| {
                this.remote_command(RemoteAction::Pull, window, cx)
            })
        };
        self.notify_user(notice, window, cx);
    }

    /// After a commit: its title, and Undo for a few seconds.
    pub(super) fn notify_committed(
        &mut self,
        message: &str,
        amend: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let strings = self.strings();
        let subject = message.lines().next().unwrap_or_default().trim().to_owned();
        let title = if amend {
            strings.git_amended
        } else {
            strings.git_committed
        };
        let mut notice = Notice::success(title).message(subject);
        if !amend {
            notice = notice.action(strings.git_undo_commit, |this, window, cx| {
                this.undo_last_commit(window, cx)
            });
        }
        self.notify_user(notice, window, cx);
    }

    /// Undoes the last commit: its changes go back to staged, its message
    /// back to the field.
    pub(super) fn undo_last_commit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(repo) = self.scm.repo.clone() else {
            return;
        };
        let message = repo.last_message().unwrap_or_default();
        let label = self.strings().git_undo_label;
        self.run_git_with(
            label,
            None,
            |repo| repo.undo_last_commit(),
            move |this, (), window, cx| {
                if this.scm.message.read(cx).value().trim().is_empty() {
                    this.scm
                        .message
                        .update(cx, |input, cx| input.set_value(message.clone(), window, cx));
                }
                let strings = this.strings();
                this.notify_user(
                    Notice::info(strings.git_commit_undone).message(strings.git_commit_undone_why),
                    window,
                    cx,
                );
            },
            window,
            cx,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn git_messages_are_understood() {
        let cases = [
            (
                "To github.com:me/app.git\n ! [rejected]        main -> main (fetch first)\nerror: failed to push some refs\nhint: Updates were rejected because the remote contains work that you do not have locally.",
                Problem::Behind,
            ),
            (
                "fatal: The current branch feature has no upstream branch.",
                Problem::NoUpstream,
            ),
            ("no remote to push to", Problem::NoRemote),
            (
                "CONFLICT (content): Merge conflict in src/a.ts\nAutomatic merge failed; fix conflicts and then commit the result.",
                Problem::Conflicts,
            ),
            (
                "error: Your local changes to the following files would be overwritten by merge:\n\tsrc/a.ts\nPlease commit your changes or stash them before you merge.",
                Problem::LocalChanges,
            ),
            (
                "fatal: could not read Username for 'https://github.com': terminal prompts disabled",
                Problem::Auth,
            ),
            (
                "fatal: unable to access 'https://github.com/me/app.git/': Could not resolve host: github.com",
                Problem::Offline,
            ),
            (
                "nothing to commit, working tree clean",
                Problem::NothingToCommit,
            ),
            ("fatal: bad object HEAD", Problem::Other),
        ];
        for (message, problem) in cases {
            assert_eq!(classify(message), problem, "{message}");
        }
    }

    #[test]
    fn counts_read_in_both_languages() {
        use crate::i18n::{ENGLISH, ITALIAN};
        assert_eq!(commits(1, &ITALIAN), "1 commit");
        assert_eq!(commits(3, &ITALIAN), "3 commit");
        assert_eq!(commits(3, &ENGLISH), "3 commits");
    }
}
