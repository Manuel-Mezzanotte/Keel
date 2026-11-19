//! Git day by day, so the terminal is never needed for it: a fetch now
//! and then that says when commits wait to be pulled; the commits to push
//! and to pull in the status bar; switching branch with changes, taken
//! along or left behind and found again on the way back; the stashes, to
//! bring back, look at or drop; the link to a line on GitHub; the history
//! of the selected lines; and the last messages, with ↑ in the field.

use std::time::{SystemTime, UNIX_EPOCH};

use gpui::Div;

use super::notices::{Notice, fill};
use super::source_control::RemoteAction;
use super::*;
use crate::quick_pick::{self, PickItem};

actions!(
    keel,
    [
        ShowStashes,
        CopyPermalink,
        OpenPermalink,
        ShowLineHistory,
        CommitAndPush,
        CommitAndSync
    ]
);

/// The name a stash left on a branch gets, to find it again there.
fn left_on(branch: &str) -> String {
    format!("keel: {branch}")
}

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|time| time.as_secs() as i64)
        .unwrap_or(0)
}

/// What Git keeps between commands.
#[derive(Default)]
pub(super) struct GitDaily {
    /// The last commit messages, for ↑ and ↓ in the field, and the one
    /// shown now.
    messages: Option<Vec<String>>,
    message_at: Option<usize>,
    /// When the remote was last asked, and how many commits it had for us.
    pub(super) last_fetch: Option<i64>,
    known_behind: u32,
    /// The titles of the commits to push and to pull.
    pub(super) outgoing: Vec<String>,
    pub(super) incoming: Vec<String>,
    autofetch: Option<Task<()>>,
    counts: Option<Task<()>>,
}

impl Workbench {
    pub(super) fn git_daily_actions(&self, root: Div, cx: &mut Context<Self>) -> Div {
        root.on_action(
            cx.listener(|this, _: &ShowStashes, window, cx| this.show_stashes(window, cx)),
        )
        .on_action(
            cx.listener(|this, _: &CopyPermalink, window, cx| this.permalink(false, window, cx)),
        )
        .on_action(
            cx.listener(|this, _: &OpenPermalink, window, cx| this.permalink(true, window, cx)),
        )
        .on_action(
            cx.listener(|this, _: &ShowLineHistory, window, cx| this.show_line_history(window, cx)),
        )
        .on_action(cx.listener(|this, _: &CommitAndPush, window, cx| {
            this.scm.then = Some(RemoteAction::Push);
            this.commit(false, window, cx)
        }))
        .on_action(cx.listener(|this, _: &CommitAndSync, window, cx| {
            this.scm.then = Some(RemoteAction::Sync);
            this.commit(false, window, cx)
        }))
    }

    // -----------------------------------------------------------------
    // Fetch by itself

    fn autofetch_period(&self) -> Option<Duration> {
        let on = self
            .settings
            .get("git.autofetch")
            .and_then(Value::as_bool)
            .unwrap_or(!cfg!(test));
        let seconds = self.settings.number("git.autofetchPeriod").unwrap_or(180.);
        (on && self.scm.repo.is_some()).then(|| Duration::from_secs_f64(seconds.max(30.)))
    }

    /// Asks the remote for new commits every few minutes while the
    /// window is in front, without a word unless some came.
    pub(super) fn start_autofetch(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.autofetch_period().is_none() {
            self.git_daily.autofetch = None;
            return;
        }
        self.git_daily.autofetch = Some(cx.spawn_in(window, async move |this, cx| {
            loop {
                let Ok(Some(period)) = this.update(cx, |this, _| this.autofetch_period()) else {
                    return;
                };
                cx.background_executor().timer(period).await;
                let repo = this
                    .update_in(cx, |this, window, _| {
                        let idle = this.scm.busy.is_none() && window.is_window_active();
                        idle.then(|| this.scm.repo.clone()).flatten()
                    })
                    .ok()
                    .flatten();
                let Some(repo) = repo else {
                    continue;
                };
                let status = cx
                    .background_spawn(async move {
                        repo.fetch()?;
                        repo.status()
                    })
                    .await;
                this.update_in(cx, |this, window, cx| {
                    this.fetched_quietly(status, window, cx)
                })
                .ok();
            }
        }));
    }

    fn fetched_quietly(
        &mut self,
        status: git::Result<git::Status>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Ok(status) = status else {
            return;
        };
        self.git_daily.last_fetch = Some(now());
        let behind = status.behind;
        let before = self.git_daily.known_behind;
        self.git_status_arrived(Ok(status), cx);
        if behind > before {
            self.notify_fetched(behind, window, cx);
        }
    }

    /// After each status: what is to push and to pull, by their titles.
    pub(super) fn status_counts_changed(&mut self, cx: &mut Context<Self>) {
        let status = &self.scm.status;
        self.git_daily.known_behind = status.behind;
        if status.upstream.is_none() || (status.ahead == 0 && status.behind == 0) {
            self.git_daily.outgoing.clear();
            self.git_daily.incoming.clear();
            return;
        }
        let Some(repo) = self.scm.repo.clone() else {
            return;
        };
        self.git_daily.counts = Some(cx.spawn(async move |this, cx| {
            let (outgoing, incoming) = cx
                .background_spawn(async move { repo.ahead_and_behind(8) })
                .await;
            this.update(cx, |this, cx| {
                this.git_daily.outgoing = outgoing;
                this.git_daily.incoming = incoming;
                cx.notify();
            })
            .ok();
        }));
    }

    /// The status bar's sync, explained: what goes, what comes, and when
    /// the remote was last asked.
    pub(super) fn sync_tooltip(&self) -> String {
        let strings = self.strings();
        let status = &self.scm.status;
        if status.upstream.is_none() {
            return strings.git_publish_tip.to_owned();
        }
        let mut lines = Vec::new();
        let list = |lines: &mut Vec<String>, title: String, titles: &[String], total: u32| {
            lines.push(title);
            lines.extend(titles.iter().map(|title| format!("  • {title}")));
            if total as usize > titles.len() {
                lines.push("  …".to_owned());
            }
        };
        if status.ahead > 0 {
            let title = fill(
                strings.sync_to_push,
                &[("count", &status.ahead.to_string())],
            );
            list(&mut lines, title, &self.git_daily.outgoing, status.ahead);
        }
        if status.behind > 0 {
            let title = fill(
                strings.sync_to_pull,
                &[("count", &status.behind.to_string())],
            );
            list(&mut lines, title, &self.git_daily.incoming, status.behind);
        }
        if lines.is_empty() {
            lines.push(strings.sync_in_step.to_owned());
        }
        if let Some(at) = self.git_daily.last_fetch {
            lines.push(String::new());
            lines.push(fill(
                strings.sync_last_fetch,
                &[("when", &notices::ago(at, now(), strings))],
            ));
        }
        lines.push(strings.sync_click.to_owned());
        lines.join("\n")
    }

    // -----------------------------------------------------------------
    // Switching branch with changes

    /// Switches branch; with changes not committed, asks whether they go
    /// along or stay on this branch, put aside to find again later.
    pub(super) fn switch_branch(
        &mut self,
        branch: git::Branch,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let strings = self.strings();
        let changes = self.scm.status.files.iter().any(|file| {
            file.conflict
                || file.is_staged()
                || file.worktree != git::Change::Untracked && file.is_changed()
        });
        let from = self.scm.status.branch.clone().unwrap_or_default();
        if !changes || from.is_empty() {
            self.run_switch(branch, None, window, cx);
            return;
        }
        let target = branch
            .name
            .split_once('/')
            .filter(|_| branch.remote)
            .map_or(branch.name.as_str(), |(_, name)| name)
            .to_owned();
        let take = fill(strings.git_switch_take, &[("branch", &target)]);
        let leave = fill(strings.git_switch_leave, &[("branch", &from)]);
        let detail = fill(
            strings.git_switch_detail,
            &[("branch", &target), ("from", &from)],
        );
        let answer = window.prompt(
            PromptLevel::Info,
            strings.git_switch_question,
            Some(&detail),
            &[&take, &leave, strings.cancel],
            cx,
        );
        cx.spawn_in(window, async move |this, cx| {
            let Ok(choice) = answer.await else {
                return;
            };
            this.update_in(cx, |this, window, cx| match choice {
                0 => this.run_switch(branch, None, window, cx),
                1 => this.run_switch(branch, Some(from), window, cx),
                _ => {}
            })
            .ok();
        })
        .detach();
    }

    /// Switches, first putting the changes aside on `leave_on` if given;
    /// on arrival, offers back what was left on the new branch.
    fn run_switch(
        &mut self,
        branch: git::Branch,
        leave_on: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let label = self.strings().scm_checkout;
        let left = leave_on.clone();
        self.run_git_with(
            label,
            None,
            move |repo| {
                if let Some(from) = &leave_on {
                    repo.stash_named(&left_on(from))?;
                }
                repo.switch(&branch)?;
                let name = repo.status()?.branch.unwrap_or_default();
                let waiting = repo
                    .stashes()
                    .unwrap_or_default()
                    .into_iter()
                    .find(|stash| stash.message.ends_with(&left_on(&name)));
                Ok((name, waiting))
            },
            move |this, (name, waiting), window, cx| {
                let strings = this.strings();
                let mut notice = Notice::info(fill(strings.git_switched, &[("branch", &name)]));
                if let Some(from) = &left {
                    notice = notice.message(fill(strings.git_switch_left, &[("branch", from)]));
                }
                this.notify_user(notice, window, cx);
                if let Some(stash) = waiting {
                    let index = stash.index;
                    this.notify_user(
                        Notice::info(fill(strings.git_switch_waiting, &[("branch", &name)]))
                            .action(strings.git_stash_bring_back, move |this, window, cx| {
                                this.restore_stash(index, false, window, cx)
                            }),
                        window,
                        cx,
                    );
                }
            },
            window,
            cx,
        );
    }

    // -----------------------------------------------------------------
    // Stashes

    fn restore_stash(
        &mut self,
        index: usize,
        keep: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let label = self.strings().scm_stash_pop;
        self.run_git(
            label,
            move |repo| repo.stash_restore(index, keep),
            |this, window, cx| {
                let strings = this.strings();
                this.notify_user(Notice::success(strings.git_unstashed), window, cx);
            },
            window,
            cx,
        );
    }

    /// Git: Show Stashes — pick one, then what to do with it.
    pub(super) fn show_stashes(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(repo) = self.scm.repo.clone() else {
            return;
        };
        let strings = self.strings();
        let italian = self.language == UiLanguage::Italian;
        let stashes = repo.stashes().unwrap_or_default();
        if stashes.is_empty() {
            self.notify_user(Notice::info(strings.git_no_stashes), window, cx);
            return;
        }
        let items: Vec<PickItem> = stashes
            .iter()
            .map(|stash| {
                // «On main: message» and «WIP on main: abc message».
                let message = stash
                    .message
                    .split_once(": ")
                    .map_or(stash.message.as_str(), |(_, rest)| rest);
                let mut item = PickItem::new(message.to_owned());
                item.detail = Some(super::git_graph::format_date(stash.time, italian));
                item
            })
            .collect();
        let chosen = quick_pick::show(
            None,
            Some(strings.git_stashes_title.to_owned()),
            items,
            window,
            cx,
        );
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Some(picked)) = chosen.await else {
                return;
            };
            let index = stashes[picked].index;
            this.update_in(cx, |this, window, cx| this.stash_menu(index, window, cx))
                .ok();
        })
        .detach();
    }

    fn stash_menu(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let strings = self.strings();
        let items = vec![
            PickItem::new(strings.git_stash_pop_item),
            PickItem::new(strings.git_stash_apply_item),
            PickItem::new(strings.git_stash_files_item),
            PickItem::new(strings.git_stash_drop_item),
        ];
        let chosen = quick_pick::show(
            None,
            Some(strings.git_stash_what.to_owned()),
            items,
            window,
            cx,
        );
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Some(choice)) = chosen.await else {
                return;
            };
            this.update_in(cx, |this, window, cx| match choice {
                0 => this.restore_stash(index, false, window, cx),
                1 => this.restore_stash(index, true, window, cx),
                2 => this.stash_files(index, window, cx),
                _ => this.drop_stash(index, window, cx),
            })
            .ok();
        })
        .detach();
    }

    fn stash_files(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(repo) = self.scm.repo.clone() else {
            return;
        };
        let files = repo.stash_files(index).unwrap_or_default();
        let items = files
            .iter()
            .map(|file| PickItem::new(file.clone()))
            .collect();
        let title = self.strings().git_stash_files_item.to_owned();
        let chosen = quick_pick::show(None, Some(title), items, window, cx);
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Some(picked)) = chosen.await else {
                return;
            };
            let path = files[picked].clone();
            this.update_in(cx, |this, window, cx| {
                this.open_diff(
                    &path,
                    super::diff_view::DiffSource::Commit(format!("stash@{{{index}}}"), None),
                    window,
                    cx,
                )
            })
            .ok();
        })
        .detach();
    }

    fn drop_stash(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let strings = self.strings();
        let answer = window.prompt(
            PromptLevel::Warning,
            strings.git_stash_drop_question,
            Some(strings.git_stash_drop_detail),
            &[strings.git_stash_drop_item, strings.cancel],
            cx,
        );
        cx.spawn_in(window, async move |this, cx| {
            if answer.await != Ok(0) {
                return;
            }
            this.update_in(cx, |this, window, cx| {
                let label = this.strings().git_stash_drop_item;
                this.run_git(
                    label,
                    move |repo| repo.stash_drop(index),
                    |_, _, _| {},
                    window,
                    cx,
                );
            })
            .ok();
        })
        .detach();
    }

    // -----------------------------------------------------------------
    // Lines

    /// The file of the active editor as git names it, and its selected
    /// lines, counted from one.
    fn selected_lines(&self, cx: &App) -> Option<(String, usize, usize)> {
        let repo = self.scm.repo.as_ref()?;
        let pane = self.panes.get(self.active)?;
        let path = self.documents.get(&pane.uri)?.disk_path()?;
        let relative = repo.relative(path)?;
        let editor = pane.editor.read(cx);
        let text = editor.value().to_string();
        let selection = editor.selected_range();
        let clamp = |offset: usize| {
            let mut offset = offset.min(text.len());
            while !text.is_char_boundary(offset) {
                offset -= 1;
            }
            offset
        };
        let line_of = |offset: usize| text[..clamp(offset)].matches('\n').count() + 1;
        let start = line_of(selection.start);
        let mut end = line_of(selection.end);
        // A selection ending at the start of a line does not take it.
        if end > start && text[..clamp(selection.end)].ends_with('\n') {
            end -= 1;
        }
        Some((relative, start, end))
    }

    /// Git: Copy or Open the link to the selected lines on the remote's
    /// site, at the commit checked out so the link keeps pointing there.
    fn permalink(&mut self, open: bool, window: &mut Window, cx: &mut Context<Self>) {
        let strings = self.strings();
        let Some(repo) = self.scm.repo.clone() else {
            return;
        };
        let Some((path, start, end)) = self.selected_lines(cx) else {
            return;
        };
        let remote = self
            .scm
            .status
            .upstream
            .as_deref()
            .and_then(|upstream| upstream.split('/').next())
            .unwrap_or("origin")
            .to_owned();
        let (Some(web), Some(hash)) = (repo.remote_web_url(&remote), repo.head_hash()) else {
            self.notify_user(Notice::warning(strings.git_no_web_remote), window, cx);
            return;
        };
        let lines = match (start == end, web.contains("gitlab")) {
            (true, _) => format!("#L{start}"),
            (false, true) => format!("#L{start}-{end}"),
            (false, false) => format!("#L{start}-L{end}"),
        };
        let link = format!("{web}/blob/{hash}/{path}{lines}");
        if open {
            cx.open_url(&link);
        } else {
            cx.write_to_clipboard(gpui::ClipboardItem::new_string(link.clone()));
            self.notify_user(
                Notice::success(strings.git_link_copied)
                    .message(link.clone())
                    .action(strings.git_open_in_browser, move |_, _, cx| {
                        cx.open_url(&link)
                    }),
                window,
                cx,
            );
        }
    }

    /// Git: History of the Selected Lines — the commits that changed them.
    fn show_line_history(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(repo) = self.scm.repo.clone() else {
            return;
        };
        let Some((path, start, end)) = self.selected_lines(cx) else {
            return;
        };
        let strings = self.strings();
        let italian = self.language == UiLanguage::Italian;
        let commits = repo.line_history(&path, start, end).unwrap_or_default();
        if commits.is_empty() {
            self.notify_user(Notice::info(strings.git_no_line_history), window, cx);
            return;
        }
        let items = commits
            .iter()
            .map(|commit| {
                let mut item = PickItem::new(commit.subject.clone());
                item.detail = Some(format!(
                    "{} · {} · {}",
                    commit.short_hash(),
                    commit.author,
                    super::git_graph::format_date(commit.time, italian)
                ));
                item
            })
            .collect();
        let title = fill(
            strings.git_line_history_title,
            &[(
                "lines",
                &if start == end {
                    start.to_string()
                } else {
                    format!("{start}–{end}")
                },
            )],
        );
        let chosen = quick_pick::show(None, Some(title), items, window, cx);
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Some(picked)) = chosen.await else {
                return;
            };
            let hash = commits[picked].hash.clone();
            this.update_in(cx, |this, window, cx| {
                this.open_diff(
                    &path,
                    super::diff_view::DiffSource::Commit(hash, None),
                    window,
                    cx,
                )
            })
            .ok();
        })
        .detach();
    }

    // -----------------------------------------------------------------
    // The commit field

    /// ↑ and ↓ in an empty commit field, or one showing an old message,
    /// go through the last messages, as in VS Code.
    pub(super) fn commit_message_key(
        &mut self,
        key: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if !matches!(key, "up" | "down") {
            return false;
        }
        let field = self.scm.message.clone();
        if !field.read(cx).focus_handle(cx).is_focused(window) {
            return false;
        }
        let value = field.read(cx).value().to_string();
        let browsing = self
            .git_daily
            .message_at
            .zip(self.git_daily.messages.as_ref())
            .is_some_and(|(at, messages)| messages.get(at) == Some(&value));
        if !value.is_empty() && !browsing {
            self.git_daily.message_at = None;
            return false;
        }
        if self.git_daily.messages.is_none() {
            let messages = self
                .scm
                .repo
                .as_ref()
                .map(|repo| repo.recent_messages(30))
                .unwrap_or_default();
            self.git_daily.messages = Some(messages);
        }
        let count = self.git_daily.messages.as_ref().map_or(0, Vec::len);
        let next = match (key, self.git_daily.message_at) {
            ("up", None) if count > 0 => Some(0),
            ("up", Some(at)) => Some((at + 1).min(count.saturating_sub(1))),
            ("down", Some(0)) | ("down", None) => None,
            ("down", Some(at)) => Some(at - 1),
            _ => return false,
        };
        self.git_daily.message_at = next;
        let text = next
            .and_then(|at| self.git_daily.messages.as_ref()?.get(at).cloned())
            .unwrap_or_default();
        field.update(cx, |input, cx| input.set_value(text, window, cx));
        true
    }

    /// Forgets the messages read, after a commit adds one.
    pub(super) fn commit_messages_changed(&mut self) {
        self.git_daily.messages = None;
        self.git_daily.message_at = None;
    }

    /// The title's and the lines' lengths, past what reads well in git:
    /// 50 characters for the title, 72 for each line under it.
    pub(super) fn commit_message_hint(&self, cx: &App) -> Option<(String, bool)> {
        let strings = self.strings();
        let value = self.scm.message.read(cx).value().to_string();
        let mut lines = value.lines();
        let title = lines.next().unwrap_or_default().chars().count();
        if let Some(line) = lines
            .enumerate()
            .find(|(_, line)| line.chars().count() > 72)
            .map(|(index, _)| index + 2)
        {
            return Some((
                fill(strings.commit_line_long, &[("line", &line.to_string())]),
                true,
            ));
        }
        (title > 40).then(|| (format!("{title}/50"), title > 50))
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::process::Command;

    use gpui::{TestAppContext, VisualTestContext};

    use super::*;

    #[test]
    fn a_stash_left_on_a_branch_is_named_after_it() {
        assert_eq!(left_on("feature/login"), "keel: feature/login");
    }

    fn git(folder: &Path, args: &[&str]) -> String {
        let output = Command::new("git")
            .arg("-C")
            .arg(folder)
            .args(args)
            .output()
            .unwrap();
        assert!(output.status.success(), "git {args:?}");
        String::from_utf8_lossy(&output.stdout).into_owned()
    }

    fn window<'a>(
        cx: &'a mut TestAppContext,
        root: &Path,
        settings: &str,
        data: &Path,
    ) -> (Entity<Workbench>, &'a mut VisualTestContext) {
        let file = data.join("settings.json");
        fs::write(&file, settings).unwrap();
        let settings = Settings::load(&file, None);
        let opening = Opening::new(Some(root.to_path_buf()), None).with_data_dir(data.join("data"));
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
        (view, cx)
    }

    fn settle(view: &Entity<Workbench>, cx: &mut VisualTestContext) {
        for _ in 0..80 {
            cx.executor().advance_clock(Duration::from_millis(600));
            cx.run_until_parked();
            if view.read_with(cx, |view, _| {
                view.scm.busy.is_none() && view.scm.repo.is_some()
            }) {
                cx.run_until_parked();
                return;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    fn last_notice(view: &Entity<Workbench>, cx: &mut VisualTestContext) -> String {
        view.read_with(cx, |view, _| {
            view.notices.list.front().map(|notice| notice.title.clone())
        })
        .unwrap_or_default()
    }

    #[gpui::test]
    fn git_day_by_day(cx: &mut TestAppContext) {
        cx.executor().allow_parking();
        cx.update(|cx| {
            gpui_component::init(cx);
            language::register_grammars();
            crate::workbench_window::init(cx);
        });
        let folder = tempfile::tempdir().unwrap();
        let base = fs::canonicalize(folder.path()).unwrap();
        let root = base.join("app");
        fs::create_dir_all(&root).unwrap();
        git(&root, &["init", "-q", "-b", "main"]);
        git(&root, &["config", "user.name", "Test"]);
        git(&root, &["config", "user.email", "test@example.com"]);
        git(&root, &["config", "commit.gpgsign", "false"]);
        git(
            &root,
            &["remote", "add", "origin", "git@github.com:me/app.git"],
        );
        fs::write(root.join("a.txt"), "uno\ndue\n").unwrap();
        git(&root, &["add", "."]);
        git(&root, &["commit", "-q", "-m", "feat: the first lines"]);
        git(&root, &["branch", "feature"]);
        let (view, cx) = window(
            cx,
            &root,
            r#"{ "keel.language": "en", "git.branchProtection": ["main"] }"#,
            &base,
        );
        settle(&view, cx);

        // ↑ in the empty field brings the last message back, ↓ takes it away.
        view.update_in(cx, |view, window, cx| {
            view.scm
                .message
                .update(cx, |input, cx| input.focus(window, cx));
        });
        cx.update(|window, _| window.refresh());
        cx.run_until_parked();
        cx.simulate_keystrokes("up");
        assert_eq!(
            view.read_with(cx, |view, cx| view.scm.message.read(cx).value().to_string()),
            "feat: the first lines"
        );
        cx.simulate_keystrokes("down");
        assert_eq!(
            view.read_with(cx, |view, cx| view.scm.message.read(cx).value().to_string()),
            ""
        );

        // The link to a line, at the commit checked out.
        let file = root.join("a.txt");
        view.update_in(cx, |view, window, cx| view.open_file(&file, window, cx));
        view.update_in(cx, |view, window, cx| view.permalink(false, window, cx));
        let hash = git(&root, &["rev-parse", "HEAD"]).trim().to_owned();
        let copied = cx
            .read_from_clipboard()
            .and_then(|item| item.text())
            .unwrap();
        assert_eq!(
            copied,
            format!("https://github.com/me/app/blob/{hash}/a.txt#L1")
        );

        // Switching with changes: left on main, found again on the way back.
        fs::write(root.join("a.txt"), "uno\nDUE\n").unwrap();
        view.update(cx, |view, cx| view.refresh_git(cx));
        settle(&view, cx);
        let feature = git::Branch {
            name: "feature".to_owned(),
            remote: false,
            current: false,
        };
        view.update_in(cx, |view, window, cx| {
            view.switch_branch(feature, window, cx)
        });
        assert!(cx.has_pending_prompt());
        cx.simulate_prompt_answer("Leave Them on main");
        settle(&view, cx);
        assert_eq!(git(&root, &["branch", "--show-current"]).trim(), "feature");
        assert_eq!(
            fs::read_to_string(root.join("a.txt")).unwrap(),
            "uno\ndue\n"
        );
        let main = git::Branch {
            name: "main".to_owned(),
            remote: false,
            current: false,
        };
        view.update_in(cx, |view, window, cx| view.switch_branch(main, window, cx));
        settle(&view, cx);
        assert_eq!(last_notice(&view, cx), "You left changes on main");
        view.update_in(cx, |view, window, cx| {
            let (_, bring_back) = view.notices.list[0].actions[0].clone();
            bring_back(view, window, cx);
        });
        settle(&view, cx);
        assert_eq!(
            fs::read_to_string(root.join("a.txt")).unwrap(),
            "uno\nDUE\n"
        );

        // A commit on a protected branch asks first; with nothing staged,
        // «Always» stages everything from now on.
        view.update_in(cx, |view, window, cx| {
            view.scm.message.update(cx, |input, cx| {
                input.set_value("fix: the second line", window, cx)
            });
            view.commit(false, window, cx);
        });
        assert!(cx.has_pending_prompt());
        cx.simulate_prompt_answer("Commit");
        cx.run_until_parked();
        assert!(cx.has_pending_prompt());
        cx.simulate_prompt_answer("Always");
        settle(&view, cx);
        assert_eq!(
            git(&root, &["log", "-1", "--format=%s"]).trim(),
            "fix: the second line"
        );
        let written = fs::read_to_string(base.join("settings.json")).unwrap();
        assert!(
            written.contains("\"git.enableSmartCommit\": true"),
            "{written}"
        );

        // The selected lines' own history.
        view.update(cx, |view, cx| {
            let lines = view.selected_lines(cx).unwrap();
            assert_eq!(lines, ("a.txt".to_owned(), 1, 1));
        });
        let history = git::Repo::discover(&root)
            .unwrap()
            .line_history("a.txt", 2, 2)
            .unwrap();
        assert_eq!(history.len(), 2);
    }

    #[gpui::test]
    fn each_save_is_kept_and_can_come_back(cx: &mut TestAppContext) {
        cx.executor().allow_parking();
        cx.update(|cx| {
            gpui_component::init(cx);
            language::register_grammars();
            crate::workbench_window::init(cx);
        });
        let folder = tempfile::tempdir().unwrap();
        let base = fs::canonicalize(folder.path()).unwrap();
        let root = base.join("app");
        fs::create_dir_all(&root).unwrap();
        let file = root.join("a.txt");
        fs::write(&file, "prima\n").unwrap();
        let (view, cx) = window(cx, &root, r#"{ "keel.language": "en" }"#, &base);
        view.update_in(cx, |view, window, cx| view.open_file(&file, window, cx));
        cx.simulate_input("x");
        cx.simulate_keystrokes("cmd-s");
        cx.run_until_parked();
        let data = base.join("data");
        let kept = super::super::local_history::versions(&super::super::local_history::folder_of(
            &data, &file,
        ));
        assert_eq!(kept.len(), 1);
        assert_eq!(fs::read_to_string(&kept[0].file).unwrap(), "xprima\n");

        // Another text, then the kept version back in its tab.
        fs::write(&file, "dopo\n").unwrap();
        view.update_in(cx, |view, window, cx| {
            let version = kept[0].file.to_string_lossy().into_owned();
            view.open_diff(
                &file.to_string_lossy(),
                super::super::diff_view::DiffSource::LocalHistory(version),
                window,
                cx,
            );
            view.restore_version(&file, &kept[0].file, window, cx);
        });
        cx.run_until_parked();
        assert_eq!(fs::read_to_string(&file).unwrap(), "xprima\n");
        assert_eq!(last_notice(&view, cx), "Version restored");
    }
}
