//! GitHub's pull requests: a page with the list and the details of the
//! one chosen, its files in a differences tab, the comments on lines in
//! the differences and in the editors, and the checks, also in the status
//! bar. Everything goes through `gh` (`github.rs`). Nothing is asked of
//! GitHub until the page has been opened once in the project, and nothing
//! is written there without a click that says so.

use std::collections::HashMap;

use gpui::{
    Anchor, AnyElement, Div, Pixels, Point, PromptLevel, SharedString,
    StatefulInteractiveElement as _, anchored, deferred, point,
};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::checkbox::Checkbox;
use gpui_component::input::{Input, InputState, Textarea, TextareaState};
use gpui_component::text::{TextView, TextViewState};
use gpui_component::{
    ActiveTheme as _, Disableable as _, Icon, Selectable as _, Sizable as _, WindowExt as _,
};
use gpui_kit_assets::IconName;

use super::diff_view::DiffSource;
use super::github::{
    Check, CheckState, Decision, Details, Filter, Gh, Pull, PullState, Said, Thread, Verdict,
    overall,
};
use super::notices::{Notice, ago, fill};
use super::settings_view::PageKind;
use super::shell::Colors;
use super::*;

actions!(
    keel,
    [OpenPullRequests, CreatePullRequest, OpenBranchPullRequest]
);

/// How often the branch's pull request is asked for: with checks running,
/// and without.
const SOON: Duration = Duration::from_secs(30);
const LATER: Duration = Duration::from_secs(240);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Part {
    #[default]
    Talk,
    Files,
    Checks,
}

/// The pull request whose details are shown.
struct Shown {
    pull: Pull,
    details: Details,
    threads: Vec<Thread>,
    body: Entity<TextViewState>,
    talk: Vec<(Said, Entity<TextViewState>)>,
    /// Comments on lines, as text to draw.
    texts: HashMap<u64, Entity<TextViewState>>,
}

/// The comments of a line, open in a card.
struct ThreadCard {
    number: u64,
    path: String,
    /// From 1.
    line: u32,
    head: String,
    texts: Vec<(String, i64, Entity<TextViewState>)>,
    /// The first comment, to answer; none for a new thread.
    first: Option<u64>,
    input: Entity<TextareaState>,
    sending: bool,
    error: Option<String>,
    anchor: Point<Pixels>,
}

#[derive(Default)]
pub(super) struct Pulls {
    /// The page has been opened here: the branch's is kept in sight.
    pub(super) watched: bool,
    filter: Filter,
    list: Vec<Pull>,
    loading: bool,
    /// What went wrong, or that there is no `gh`.
    error: Option<String>,
    selected: Option<u64>,
    opening: bool,
    shown: Option<Shown>,
    part: Part,
    /// The branch's own, its comments on lines, and the branch.
    pub(super) current: Option<(Pull, Vec<Thread>)>,
    current_of: Option<String>,
    card: Option<ThreadCard>,
    list_task: Option<Task<()>>,
    detail_task: Option<Task<()>>,
    current_task: Option<Task<()>>,
    poll: Option<Task<()>>,
}

impl Pulls {
    /// The comments known of a pull request.
    fn threads_of(&self, number: u64) -> &[Thread] {
        if let Some(shown) = self
            .shown
            .as_ref()
            .filter(|shown| shown.pull.number == number)
        {
            return &shown.threads;
        }
        match &self.current {
            Some((pull, threads)) if pull.number == number => threads,
            _ => &[],
        }
    }

    /// What is said of each line of a file, for the differences: from 0.
    pub(super) fn notes_of(&self, number: u64, path: &str) -> HashMap<usize, String> {
        let mut notes: HashMap<usize, String> = HashMap::new();
        for thread in self.threads_of(number) {
            let Some(line) = thread.line.filter(|_| thread.path == path && !thread.left) else {
                continue;
            };
            notes.insert(line.saturating_sub(1) as usize, summary(thread));
        }
        notes
    }
}

/// A thread in a few words: who said what, and how many answered.
fn summary(thread: &Thread) -> String {
    const MOST: usize = 60;
    let Some(first) = thread.comments.first() else {
        return String::new();
    };
    let line = first.body.lines().next().unwrap_or_default();
    let mut short: String = line.chars().take(MOST).collect();
    if line.chars().count() > MOST {
        short.push('…');
    }
    let more = match thread.comments.len() {
        0 | 1 => String::new(),
        count => format!(" (+{})", count - 1),
    };
    format!("💬 {}: {short}{more}", first.author)
}

fn check_look(state: CheckState, cx: &App) -> (IconName, Hsla) {
    let theme = cx.theme();
    match state {
        CheckState::Passed => (IconName::CircleCheck, theme.success),
        CheckState::Failed => (IconName::CircleX, theme.danger),
        CheckState::Pending => (IconName::LoaderCircle, theme.warning),
        CheckState::Skipped => (IconName::Minus, theme.muted_foreground),
    }
}

impl Workbench {
    pub(super) fn pull_actions(&self, root: Div, cx: &mut Context<Self>) -> Div {
        root.on_action(
            cx.listener(|this, _: &OpenPullRequests, window, cx| {
                this.open_pull_requests(window, cx)
            }),
        )
        .on_action(
            cx.listener(|this, _: &CreatePullRequest, window, cx| this.ask_new_pull(window, cx)),
        )
        .on_action(cx.listener(|this, _: &OpenBranchPullRequest, window, cx| {
            this.open_branch_pull(window, cx)
        }))
    }

    // -----------------------------------------------------------------
    // Asking

    /// The page, with its list asked again.
    pub(super) fn open_pull_requests(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.scm.repo.is_none() {
            self.status_message = Some(self.strings().scm_no_repository.to_owned());
            cx.notify();
            return;
        }
        self.pulls.watched = true;
        self.session_changed(cx);
        self.open_page(PageKind::Pulls, window, cx);
        self.load_pulls(cx);
        self.load_branch_pull(cx);
    }

    pub(super) fn open_branch_pull(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let number = self.pulls.current.as_ref().map(|(pull, _)| pull.number);
        self.open_pull_requests(window, cx);
        if let Some(number) = number {
            self.select_pull(number, cx);
        }
    }

    /// Runs `work` with `gh`, away from the window.
    fn with_gh<T: Send + 'static>(
        &self,
        cx: &mut Context<Self>,
        work: impl FnOnce(Gh) -> Result<T, String> + Send + 'static,
    ) -> Option<Task<Result<T, String>>> {
        let root = self.scm.repo.as_ref()?.root().to_path_buf();
        let missing = self.strings().pulls_no_gh;
        Some(cx.background_spawn(async move {
            let gh = Gh::find(&root).ok_or_else(|| missing.to_owned())?;
            work(gh)
        }))
    }

    fn load_pulls(&mut self, cx: &mut Context<Self>) {
        let filter = self.pulls.filter;
        let Some(task) = self.with_gh(cx, move |gh| gh.list(filter)) else {
            return;
        };
        self.pulls.loading = true;
        cx.notify();
        self.pulls.list_task = Some(cx.spawn(async move |this, cx| {
            let list = task.await;
            this.update(cx, |this, cx| {
                this.pulls.loading = false;
                match list {
                    Ok(list) => {
                        this.pulls.error = None;
                        let first = list.first().map(|pull| pull.number);
                        this.pulls.list = list;
                        if this.pulls.selected.is_none()
                            && let Some(first) = first
                        {
                            this.select_pull(first, cx);
                        }
                    }
                    Err(error) => {
                        this.pulls.list.clear();
                        this.pulls.error = Some(error);
                    }
                }
                cx.notify();
            })
            .ok();
        }));
    }

    /// Shows a pull request's details.
    pub(super) fn select_pull(&mut self, number: u64, cx: &mut Context<Self>) {
        self.pulls.selected = Some(number);
        let Some(task) = self.with_gh(cx, move |gh| {
            let found = gh.view(Some(number))?;
            // The comments may be missing without the rest being so.
            let threads = gh.threads(number).unwrap_or_default();
            Ok(found.map(|(pull, details)| (pull, details, threads)))
        }) else {
            return;
        };
        self.pulls.opening = true;
        cx.notify();
        self.pulls.detail_task = Some(cx.spawn(async move |this, cx| {
            let found = task.await;
            this.update(cx, |this, cx| {
                if this.pulls.selected != Some(number) {
                    return;
                }
                this.pulls.opening = false;
                match found {
                    Ok(Some((pull, details, threads))) => {
                        this.pulls.error = None;
                        this.show_pull(pull, details, threads, cx);
                    }
                    Ok(None) => {}
                    Err(error) => this.pulls.error = Some(error),
                }
                cx.notify();
            })
            .ok();
        }));
    }

    fn show_pull(
        &mut self,
        pull: Pull,
        details: Details,
        threads: Vec<Thread>,
        cx: &mut Context<Self>,
    ) {
        let mut markdown =
            |text: &str| cx.new(|cx| TextViewState::markdown(text, cx).selectable(true));
        let body = markdown(&details.body);
        let talk = details
            .talk
            .iter()
            .map(|said| (said.clone(), markdown(&said.body)))
            .collect();
        let texts = threads
            .iter()
            .flat_map(|thread| &thread.comments)
            .map(|comment| (comment.id, markdown(&comment.body)))
            .collect();
        let number = pull.number;
        // The list says the same.
        if let Some(known) = self
            .pulls
            .list
            .iter_mut()
            .find(|known| known.number == number)
        {
            *known = pull.clone();
        }
        self.pulls.shown = Some(Shown {
            pull,
            details,
            threads,
            body,
            talk,
            texts,
        });
        self.refresh_pull_diffs(number);
    }

    /// Asks for the branch's pull request, once the page was opened here.
    pub(super) fn load_branch_pull(&mut self, cx: &mut Context<Self>) {
        if !self.pulls.watched || self.scm.repo.is_none() {
            return;
        }
        let branch = self.project.branch.clone();
        let asked = branch.clone();
        let Some(task) = self.with_gh(cx, |gh| {
            let found = gh.view(None)?;
            Ok(match found {
                Some((pull, _)) => {
                    let threads = gh.threads(pull.number).unwrap_or_default();
                    Some((pull, threads))
                }
                None => None,
            })
        }) else {
            return;
        };
        self.pulls.current_of = branch;
        self.pulls.current_task = Some(cx.spawn(async move |this, cx| {
            let found = task.await;
            this.update(cx, |this, cx| {
                // The branch may have changed meanwhile.
                if this.pulls.current_of != asked {
                    return;
                }
                let before = this
                    .pulls
                    .current
                    .as_ref()
                    .map(|(pull, _)| (pull.number, overall(&pull.checks)));
                if let Ok(found) = found {
                    this.pulls.current = found;
                }
                if let Some((pull, _)) = &this.pulls.current {
                    let number = pull.number;
                    let now = overall(&pull.checks);
                    // The checks ended while you worked: the status bar
                    // says it, a failure in words too.
                    if before == Some((number, Some(CheckState::Pending)))
                        && now == Some(CheckState::Failed)
                    {
                        let text = fill(
                            this.strings().pulls_checks_failed,
                            &[("number", &number.to_string())],
                        );
                        this.status_message = Some(text);
                    }
                    this.refresh_pull_diffs(number);
                }
                this.watch_branch_pull(cx);
                cx.notify();
            })
            .ok();
        }));
    }

    /// Asks again later: soon while checks run.
    fn watch_branch_pull(&mut self, cx: &mut Context<Self>) {
        let pending = self
            .pulls
            .current
            .as_ref()
            .is_some_and(|(pull, _)| overall(&pull.checks) == Some(CheckState::Pending));
        let wait = if pending { SOON } else { LATER };
        self.pulls.poll = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(wait).await;
            this.update(cx, |this, cx| this.load_branch_pull(cx)).ok();
        }));
    }

    /// The branch changed: its pull request is another.
    pub(super) fn pulls_branch_changed(&mut self, cx: &mut Context<Self>) {
        if self.pulls.watched && self.pulls.current_of != self.project.branch {
            self.pulls.current = None;
            self.load_branch_pull(cx);
        }
    }

    /// What the status bar says of the branch's pull request.
    pub(super) fn pull_status(&self) -> Option<String> {
        let (pull, _) = self.pulls.current.as_ref()?;
        if self.pulls.current_of != self.project.branch || pull.state != PullState::Open {
            return None;
        }
        let mark = match overall(&pull.checks) {
            Some(CheckState::Passed) => " ✓",
            Some(CheckState::Failed) => " ✗",
            Some(CheckState::Pending) => " ●",
            _ => "",
        };
        Some(format!("PR #{}{mark}", pull.number))
    }

    /// The comments on the lines of a file of the branch's, by line from 0.
    pub(super) fn pull_comments_of(&self, uri: &str) -> HashMap<usize, String> {
        let Some((pull, _)) = &self.pulls.current else {
            return HashMap::new();
        };
        if self.pulls.current_of != self.project.branch || pull.state != PullState::Open {
            return HashMap::new();
        }
        let relative = self
            .documents
            .get(uri)
            .and_then(|document| document.disk_path())
            .and_then(|path| self.scm.repo.as_ref()?.relative(path));
        relative.map_or_else(HashMap::new, |path| self.pulls.notes_of(pull.number, &path))
    }

    // -----------------------------------------------------------------
    // The files

    /// A pull request's changes, every file in one page, from its commits.
    fn open_pull_changes(
        &mut self,
        only: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(shown) = &self.pulls.shown else {
            return;
        };
        let Some(repo) = self.scm.repo.clone() else {
            return;
        };
        let (number, base) = (shown.pull.number, shown.pull.base.clone());
        let (head_oid, base_oid) = (
            shown.details.head_oid.clone(),
            shown.details.base_oid.clone(),
        );
        let is_hash =
            |text: &str| text.len() >= 7 && text.bytes().all(|byte| byte.is_ascii_hexdigit());
        if !is_hash(&head_oid) || !is_hash(&base_oid) {
            return;
        }
        let remote = self.pull_remote();
        self.status_message = Some(fill(
            self.strings().pulls_fetching,
            &[("number", &number.to_string())],
        ));
        cx.notify();
        let task = cx.background_spawn(async move {
            // Already here, as for a branch of one's own.
            let here = repo.merge_base(&base_oid, &head_oid);
            let from = match here {
                Ok(from) => from,
                Err(_) => {
                    repo.fetch_pull(&remote, number, &base)
                        .map_err(|error| error.to_string())?;
                    repo.merge_base(&base_oid, &head_oid)
                        .map_err(|error| error.to_string())?
                }
            };
            Ok::<_, String>((from, head_oid))
        });
        cx.spawn_in(window, async move |this, cx| {
            let found = task.await;
            this.update_in(cx, |this, window, cx| {
                this.status_message = None;
                match found {
                    Ok((from, to)) => {
                        let source = DiffSource::Pull { number, from, to };
                        this.open_diff(&only.unwrap_or_default(), source, window, cx);
                    }
                    Err(error) => this.report(error),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// The remote the branch follows, or the usual one.
    fn pull_remote(&self) -> String {
        self.scm
            .status
            .upstream
            .as_deref()
            .and_then(|upstream| upstream.split('/').next())
            .unwrap_or("origin")
            .to_owned()
    }

    /// The comments of an open page of changes, drawn again.
    fn refresh_pull_diffs(&mut self, number: u64) {
        let tabs: Vec<(String, DiffSource, diff_pane::DiffDoc)> = self
            .documents
            .values()
            .filter_map(|document| match &document.backing {
                Backing::Diff(tab) if matches!(&tab.source, DiffSource::Pull { number: of, .. } if *of == number) => {
                    Some((tab.path.clone(), tab.source.clone(), tab.doc.clone()?))
                }
                _ => None,
            })
            .collect();
        for (path, source, doc) in tabs {
            if let Ok(files) = self.diff_docs(&path, &source) {
                doc.set_files(files);
            }
        }
    }

    // -----------------------------------------------------------------
    // Comments on lines

    /// The comments of a line of the branch's pull request, from an editor.
    pub(super) fn open_branch_comments(
        &mut self,
        uri: &str,
        line: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(number) = self.pulls.current.as_ref().map(|(pull, _)| pull.number) else {
            return;
        };
        let path = self
            .documents
            .get(uri)
            .and_then(|document| document.disk_path())
            .and_then(|path| self.scm.repo.as_ref()?.relative(path));
        if let Some(path) = path {
            self.open_line_comments(number, path, line as u32 + 1, window, cx);
        }
    }

    /// The comments of a line, from 1, in a card, with a field to answer
    /// or to begin.
    pub(super) fn open_line_comments(
        &mut self,
        number: u64,
        path: String,
        line: u32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let head = match &self.pulls.shown {
            Some(shown) if shown.pull.number == number => shown.details.head_oid.clone(),
            _ => String::new(),
        };
        let thread = self
            .pulls
            .threads_of(number)
            .iter()
            .find(|thread| thread.path == path && thread.line == Some(line) && !thread.left)
            .cloned();
        let texts = thread
            .iter()
            .flat_map(|thread| &thread.comments)
            .map(|comment| {
                let state =
                    cx.new(|cx| TextViewState::markdown(&comment.body, cx).selectable(true));
                (comment.author.clone(), comment.at, state)
            })
            .collect();
        let placeholder = if thread.is_some() {
            self.strings().pulls_reply_hint
        } else {
            self.strings().pulls_comment_hint
        };
        let input = cx.new(|cx| {
            TextareaState::new(window, cx)
                .placeholder(placeholder)
                .rows(3)
        });
        input.update(cx, |input, cx| input.focus(window, cx));
        let mouse = window.mouse_position();
        self.pulls.card = Some(ThreadCard {
            number,
            path,
            line,
            head,
            texts,
            first: thread.and_then(|thread| thread.comments.first().map(|comment| comment.id)),
            input,
            sending: false,
            error: None,
            anchor: point(mouse.x - px(80.), mouse.y + px(14.)),
        });
        cx.notify();
    }

    /// Sends what is written in the card: an answer, or a new comment.
    fn send_line_comment(&mut self, cx: &mut Context<Self>) {
        let Some(card) = &mut self.pulls.card else {
            return;
        };
        let body = card.input.read(cx).value().trim().to_owned();
        if body.is_empty() || card.sending {
            return;
        }
        card.sending = true;
        card.error = None;
        let (number, first) = (card.number, card.first);
        let (path, line, head) = (card.path.clone(), card.line, card.head.clone());
        cx.notify();
        let Some(task) = self.with_gh(cx, move |gh| match first {
            Some(first) => gh.reply(number, first, &body),
            None => {
                // The commit the comment is on.
                let head = match head.is_empty() {
                    false => head,
                    true => gh
                        .view(Some(number))?
                        .map(|(_, details)| details.head_oid)
                        .unwrap_or_default(),
                };
                gh.comment(number, &head, &path, line, &body)
            }
        }) else {
            return;
        };
        cx.spawn(async move |this, cx| {
            let sent = task.await;
            this.update(cx, |this, cx| {
                match sent {
                    Ok(_) => {
                        this.pulls.card = None;
                        if this.pulls.selected == Some(number) {
                            this.select_pull(number, cx);
                        }
                        this.load_branch_pull(cx);
                    }
                    Err(error) => {
                        // What was written stays.
                        if let Some(card) = &mut this.pulls.card {
                            card.sending = false;
                            card.error = Some(error);
                        }
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    pub(super) fn render_thread_card(
        &self,
        colors: Colors,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let card = self.pulls.card.as_ref()?;
        let strings = self.strings();
        let theme = cx.theme();
        let now = notices::now();
        let name = card.path.rsplit('/').next().unwrap_or(&card.path);
        let comments = card
            .texts
            .iter()
            .enumerate()
            .map(|(index, (author, at, text))| {
                div()
                    .id(("pull-comment", index))
                    .flex()
                    .flex_col()
                    .gap_0p5()
                    .child(
                        div()
                            .flex()
                            .gap_2()
                            .text_xs()
                            .child(
                                div()
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .text_color(colors.text)
                                    .child(author.clone()),
                            )
                            .child(div().text_color(colors.faint).child(ago(*at, now, strings))),
                    )
                    .child(div().text_sm().child(TextView::new(text)))
            });
        Some(
            deferred(
                anchored()
                    .position(card.anchor)
                    .anchor(Anchor::TopLeft)
                    .snap_to_window_with_margin(px(10.))
                    .child(
                        div()
                            .id("pull-thread")
                            .occlude()
                            .w(px(520.))
                            .max_h(px(460.))
                            .p_3()
                            .flex()
                            .flex_col()
                            .gap_2()
                            .rounded(px(10.))
                            .border_1()
                            .border_color(colors.border)
                            .bg(theme.popover)
                            .shadow_lg()
                            .capture_action(cx.listener(
                                |this, _: &gpui_component::input::Escape, _, cx| {
                                    this.pulls.card = None;
                                    cx.stop_propagation();
                                    cx.notify();
                                },
                            ))
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .text_xs()
                                    .text_color(colors.muted)
                                    .child(
                                        div().flex_1().truncate().child(format!(
                                            "#{} · {name}:{}",
                                            card.number, card.line
                                        )),
                                    )
                                    .child(
                                        Button::new("pull-thread-close")
                                            .ghost()
                                            .xsmall()
                                            .icon(Icon::new(IconName::X))
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.pulls.card = None;
                                                cx.notify();
                                            })),
                                    ),
                            )
                            .child(
                                div()
                                    .id("pull-thread-comments")
                                    .flex_1()
                                    .min_h_0()
                                    .overflow_y_scroll()
                                    .flex()
                                    .flex_col()
                                    .gap_3()
                                    .children(comments),
                            )
                            .child(Textarea::new(&card.input))
                            .children(
                                card.error.clone().map(|error| {
                                    div().text_xs().text_color(theme.danger).child(error)
                                }),
                            )
                            .child(
                                div().flex().justify_end().child(
                                    Button::new("pull-thread-send")
                                        .primary()
                                        .small()
                                        .label(if card.first.is_some() {
                                            strings.pulls_reply
                                        } else {
                                            strings.pulls_comment
                                        })
                                        .loading(card.sending)
                                        .on_click(
                                            cx.listener(|this, _, _, cx| {
                                                this.send_line_comment(cx)
                                            }),
                                        ),
                                ),
                            ),
                    ),
            )
            .with_priority(2)
            .into_any_element(),
        )
    }

    // -----------------------------------------------------------------
    // What is done on GitHub

    /// Runs something on GitHub, then says how it went and asks again.
    fn pull_done(
        &mut self,
        number: u64,
        done: String,
        task: Option<Task<Result<String, String>>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(task) = task else {
            return;
        };
        cx.spawn_in(window, async move |this, cx| {
            let result = task.await;
            this.update_in(cx, |this, window, cx| {
                let notice = match result {
                    Ok(_) => Notice::success(done),
                    Err(error) => Notice::error(this.strings().pulls_failed).message(error),
                };
                this.notify_user(notice, window, cx);
                this.refresh_git(cx);
                this.load_pulls(cx);
                if this.pulls.selected == Some(number) {
                    this.select_pull(number, cx);
                }
                this.load_branch_pull(cx);
            })
            .ok();
        })
        .detach();
    }

    fn checkout_pull(&mut self, number: u64, window: &mut Window, cx: &mut Context<Self>) {
        let done = fill(
            self.strings().pulls_checked_out,
            &[("number", &number.to_string())],
        );
        let task = self.with_gh(cx, move |gh| gh.checkout(number));
        self.pull_done(number, done, task, window, cx);
    }

    /// Asks how to merge; what is chosen is done.
    fn ask_merge(&mut self, number: u64, window: &mut Window, cx: &mut Context<Self>) {
        let strings = self.strings();
        let question = fill(
            strings.pulls_merge_question,
            &[("number", &number.to_string())],
        );
        let answer = window.prompt(
            PromptLevel::Warning,
            &question,
            Some(strings.pulls_merge_detail),
            &[
                strings.pulls_squash,
                strings.pulls_merge,
                strings.pulls_rebase,
                strings.cancel,
            ],
            cx,
        );
        cx.spawn_in(window, async move |this, cx| {
            let how = match answer.await {
                Ok(0) => "--squash",
                Ok(1) => "--merge",
                Ok(2) => "--rebase",
                _ => return,
            };
            this.update_in(cx, |this, window, cx| {
                let done = fill(
                    this.strings().pulls_merged,
                    &[("number", &number.to_string())],
                );
                let task = this.with_gh(cx, move |gh| gh.merge(number, how));
                this.pull_done(number, done, task, window, cx);
            })
            .ok();
        })
        .detach();
    }

    /// Writes a review; its button sends it.
    fn ask_review(
        &mut self,
        number: u64,
        verdict: Verdict,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let strings = self.strings();
        let title = match verdict {
            Verdict::Approve => strings.pulls_approve,
            Verdict::RequestChanges => strings.pulls_request_changes,
            Verdict::Comment => strings.pulls_comment,
        };
        let field = cx.new(|cx| {
            TextareaState::new(window, cx)
                .placeholder(strings.pulls_review_hint)
                .rows(6)
        });
        let this = cx.weak_entity();
        let shown = field.clone();
        window.open_dialog(cx, move |dialog, _, _| {
            let (this, read) = (this.clone(), shown.clone());
            dialog
                .w(px(560.))
                .title(format!("{title} · #{number}"))
                .on_ok(move |_, window, cx| {
                    let body = read.read(cx).value().trim().to_owned();
                    // Only an approval can say nothing.
                    if body.is_empty() && verdict != Verdict::Approve {
                        return false;
                    }
                    this.update(cx, |this, cx| {
                        let done = fill(
                            this.strings().pulls_reviewed,
                            &[("number", &number.to_string())],
                        );
                        let task = this.with_gh(cx, move |gh| gh.review(number, verdict, &body));
                        this.pull_done(number, done, task, window, cx);
                    })
                    .ok();
                    true
                })
                .child(Textarea::new(&shown))
        });
        field.update(cx, |field, cx| field.focus(window, cx));
    }

    /// A new pull request from the branch: its title and text, with the
    /// commits already in it. Its button creates it.
    fn ask_new_pull(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let strings = self.strings();
        let Some(repo) = self.scm.repo.clone() else {
            return;
        };
        if self.scm.status.upstream.is_none() || self.scm.status.ahead > 0 {
            self.notify_user(
                Notice::info(strings.pulls_push_first).action(
                    strings.pulls_push,
                    |_: &mut Workbench, window: &mut Window, cx: &mut Context<Workbench>| {
                        window.dispatch_action(Box::new(super::GitSync), cx);
                    },
                ),
                window,
                cx,
            );
            return;
        }
        let message = repo.last_message().unwrap_or_default();
        let (title, body) = message.split_once('\n').unwrap_or((&message, ""));
        let title_field = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(strings.pulls_title)
                .default_value(title.trim().to_owned())
        });
        let body_field = cx.new(|cx| {
            TextareaState::new(window, cx)
                .placeholder(strings.pulls_body)
                .default_value(body.trim().to_owned())
                .rows(10)
        });
        let draft = std::rc::Rc::new(std::cell::Cell::new(false));
        let this = cx.weak_entity();
        let branch = self.project.branch.clone().unwrap_or_default();
        let (shown_title, shown_body) = (title_field.clone(), body_field);
        window.open_dialog(cx, move |dialog, _, _| {
            let (this, title, body) = (this.clone(), shown_title.clone(), shown_body.clone());
            let (is_draft, toggled) = (draft.clone(), draft.clone());
            dialog
                .w(px(620.))
                .title(format!("{} · {branch}", strings.pulls_new))
                .on_ok(move |_, window, cx| {
                    let title = title.read(cx).value().trim().to_owned();
                    let body = body.read(cx).value().trim().to_owned();
                    if title.is_empty() {
                        return false;
                    }
                    let draft = is_draft.get();
                    this.update(cx, |this, cx| {
                        this.create_pull(title, body, draft, window, cx)
                    })
                    .ok();
                    true
                })
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_2()
                        .child(Input::new(&shown_title))
                        .child(Textarea::new(&shown_body))
                        .child(
                            Checkbox::new("pull-draft")
                                .checked(toggled.get())
                                .label(strings.pulls_draft)
                                .on_click({
                                    let toggled = toggled.clone();
                                    move |on: &bool, window, _| {
                                        toggled.set(*on);
                                        window.refresh();
                                    }
                                }),
                        ),
                )
        });
        title_field.update(cx, |field, cx| field.focus(window, cx));
    }

    fn create_pull(
        &mut self,
        title: String,
        body: String,
        draft: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(task) = self.with_gh(cx, move |gh| gh.create(&title, &body, draft)) else {
            return;
        };
        self.pulls.watched = true;
        cx.spawn_in(window, async move |this, cx| {
            let created = task.await;
            this.update_in(cx, |this, window, cx| {
                let strings = this.strings();
                let notice =
                    match created {
                        Ok(url) => {
                            let link = url.lines().last().unwrap_or_default().to_owned();
                            Notice::success(strings.pulls_created).message(link.clone()).action(
                            strings.pulls_open_on_github,
                            move |_: &mut Workbench, _: &mut Window, cx: &mut Context<Workbench>| {
                                if link.starts_with("https://") {
                                    cx.open_url(&link);
                                }
                            },
                        )
                        }
                        Err(error) => Notice::error(strings.pulls_failed).message(error),
                    };
                this.notify_user(notice, window, cx);
                this.load_pulls(cx);
                this.load_branch_pull(cx);
            })
            .ok();
        })
        .detach();
    }

    /// Asks Claude about a check that failed, with the end of its log.
    fn ask_claude_about_check(
        &mut self,
        check: &Check,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(job) = check.job else {
            return;
        };
        let Some(task) = self.with_gh(cx, move |gh| gh.failed_log(job)) else {
            return;
        };
        let name = check.name.clone();
        self.status_message = Some(self.strings().pulls_reading_log.to_owned());
        cx.notify();
        cx.spawn_in(window, async move |this, cx| {
            let log = task.await;
            this.update_in(cx, |this, window, cx| {
                this.status_message = None;
                match log {
                    Ok(log) => {
                        let prompt = fill(
                            this.strings().pulls_ask_claude_prompt,
                            &[("check", &name), ("log", &log)],
                        );
                        this.ask_claude(prompt, window, cx);
                    }
                    Err(error) => this.report(error),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    // -----------------------------------------------------------------
    // The page

    pub(super) fn render_pulls_page(&self, colors: Colors, cx: &mut Context<Self>) -> AnyElement {
        div()
            .size_full()
            .flex()
            .flex_row()
            .child(self.render_pull_list(colors, cx))
            .child(self.render_pull_details(colors, cx))
            .into_any_element()
    }

    fn render_pull_list(&self, colors: Colors, cx: &mut Context<Self>) -> AnyElement {
        let strings = self.strings();
        let now = notices::now();
        let filters = Filter::ALL.into_iter().enumerate().map(|(index, filter)| {
            let label = match filter {
                Filter::Open => strings.pulls_filter_open,
                Filter::Mine => strings.pulls_filter_mine,
                Filter::ToReview => strings.pulls_filter_review,
                Filter::Closed => strings.pulls_filter_closed,
            };
            Button::new(("pull-filter", index))
                .ghost()
                .xsmall()
                .label(label)
                .selected(self.pulls.filter == filter)
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.pulls.filter = filter;
                    this.load_pulls(cx);
                }))
        });
        let rows: Vec<AnyElement> = self
            .pulls
            .list
            .iter()
            .map(|pull| {
                let number = pull.number;
                let chosen = self.pulls.selected == Some(number);
                let (icon, color) = self.pull_look(pull, cx);
                let check = overall(&pull.checks).map(|state| check_look(state, cx));
                div()
                    .id(("pull", number as usize))
                    .mx_2()
                    .px_2()
                    .py_1p5()
                    .flex()
                    .gap_2()
                    .rounded(px(7.))
                    .cursor_pointer()
                    .when(chosen, |row| row.bg(colors.selected))
                    .hover(|row| row.bg(colors.hover))
                    .child(
                        Icon::new(icon)
                            .small()
                            .flex_none()
                            .mt_0p5()
                            .text_color(color),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(colors.text)
                                    .line_clamp(2)
                                    .child(pull.title.clone()),
                            )
                            .child(div().text_xs().text_color(colors.faint).truncate().child(
                                format!(
                                    "#{number} · {} · {}",
                                    pull.author,
                                    ago(pull.updated, now, strings)
                                ),
                            )),
                    )
                    .children(check.map(|(icon, color)| {
                        Icon::new(icon)
                            .xsmall()
                            .flex_none()
                            .mt_1()
                            .text_color(color)
                    }))
                    .on_click(cx.listener(move |this, _, _, cx| this.select_pull(number, cx)))
                    .into_any_element()
            })
            .collect();
        let empty = rows.is_empty();
        div()
            .w(px(330.))
            .flex_none()
            .h_full()
            .flex()
            .flex_col()
            .border_r_1()
            .border_color(colors.border)
            .child(
                div()
                    .px_3()
                    .pt_3()
                    .flex()
                    .items_center()
                    .child(
                        div()
                            .flex_1()
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .child(strings.pulls_title_page),
                    )
                    .child(
                        Button::new("pulls-refresh")
                            .ghost()
                            .small()
                            .icon(Icon::new(IconName::RefreshCw))
                            .loading(self.pulls.loading)
                            .tooltip(strings.scm_refresh)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.load_pulls(cx);
                                if let Some(number) = this.pulls.selected {
                                    this.select_pull(number, cx);
                                }
                                this.load_branch_pull(cx);
                            })),
                    )
                    .child(
                        Button::new("pulls-new")
                            .ghost()
                            .small()
                            .icon(Icon::new(IconName::Plus))
                            .tooltip(strings.pulls_new)
                            .on_click(
                                cx.listener(|this, _, window, cx| this.ask_new_pull(window, cx)),
                            ),
                    ),
            )
            .child(
                div()
                    .px_2()
                    .py_2()
                    .flex()
                    .flex_wrap()
                    .gap_0p5()
                    .children(filters),
            )
            .child(
                div()
                    .id("pulls-list")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .children(rows)
                    .when(empty && !self.pulls.loading, |list| {
                        list.child(div().p_3().text_sm().text_color(colors.muted).child(
                            match &self.pulls.error {
                                Some(error) => error.clone(),
                                None => strings.pulls_empty.to_owned(),
                            },
                        ))
                    }),
            )
            .into_any_element()
    }

    fn pull_look(&self, pull: &Pull, cx: &App) -> (IconName, Hsla) {
        let theme = cx.theme();
        match pull.state {
            PullState::Merged => (IconName::GitMerge, theme.primary),
            PullState::Closed => (IconName::GitPullRequestClosed, theme.danger),
            PullState::Open if pull.draft => {
                (IconName::GitPullRequestDraft, theme.muted_foreground)
            }
            PullState::Open => (IconName::GitPullRequest, theme.success),
        }
    }

    fn render_pull_details(&self, colors: Colors, cx: &mut Context<Self>) -> AnyElement {
        let strings = self.strings();
        let Some(shown) = &self.pulls.shown else {
            return div()
                .flex_1()
                .h_full()
                .flex()
                .items_center()
                .justify_center()
                .text_sm()
                .text_color(colors.faint)
                .child(if self.pulls.opening {
                    strings.pulls_loading
                } else {
                    strings.pulls_choose
                })
                .into_any_element();
        };
        let Shown { pull, details, .. } = shown;
        let number = pull.number;
        let theme = cx.theme();
        let (icon, color) = self.pull_look(pull, cx);
        let state = match pull.state {
            PullState::Merged => strings.pulls_state_merged,
            PullState::Closed => strings.pulls_state_closed,
            PullState::Open if pull.draft => strings.pulls_state_draft,
            PullState::Open => strings.pulls_state_open,
        };
        let decision = match pull.decision {
            Decision::Approved => Some((strings.pulls_approved, theme.success)),
            Decision::Changes => Some((strings.pulls_changes_requested, theme.danger)),
            Decision::Required => Some((strings.pulls_review_required, theme.warning)),
            Decision::None => None,
        };
        let conflicts = details.mergeable == Some(false) && pull.state == PullState::Open;
        let url = pull.url.clone();
        let badge = |text: &str, color: Hsla| {
            div()
                .flex_none()
                .px_1p5()
                .rounded(px(5.))
                .bg(color.opacity(0.15))
                .text_xs()
                .text_color(color)
                .child(text.to_owned())
        };
        let open = pull.state == PullState::Open;
        let header = div()
            .p_4()
            .pb_2()
            .flex()
            .flex_col()
            .gap_2()
            .child(
                div()
                    .flex()
                    .gap_2()
                    .child(Icon::new(icon).text_color(color).flex_none().mt_1())
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_lg()
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .child(pull.title.clone()),
                    )
                    .child(
                        div()
                            .flex_none()
                            .text_lg()
                            .text_color(colors.faint)
                            .child(format!("#{number}")),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap_2()
                    .child(badge(state, color))
                    .children(decision.map(|(text, color)| badge(text, color)))
                    .when(conflicts, |row| {
                        row.child(badge(strings.pulls_conflicts, theme.danger))
                    })
                    .child(
                        div()
                            .text_xs()
                            .text_color(colors.muted)
                            .child(format!("{}: {} → {}", pull.author, pull.head, pull.base)),
                    )
                    .child(
                        div()
                            .flex_none()
                            .flex()
                            .gap_1p5()
                            .text_xs()
                            .child(
                                div()
                                    .text_color(theme.success)
                                    .child(format!("+{}", details.additions)),
                            )
                            .child(
                                div()
                                    .text_color(theme.danger)
                                    .child(format!("−{}", details.deletions)),
                            ),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap_1()
                    .child(
                        Button::new("pull-changes")
                            .primary()
                            .small()
                            .icon(Icon::new(IconName::Columns2))
                            .label(strings.pulls_see_changes)
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.open_pull_changes(None, window, cx)
                            })),
                    )
                    .when(open, |buttons| {
                        buttons
                            .child(
                                Button::new("pull-checkout")
                                    .small()
                                    .icon(Icon::new(IconName::GitBranch))
                                    .label(strings.pulls_checkout)
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        this.checkout_pull(number, window, cx)
                                    })),
                            )
                            .child(
                                Button::new("pull-approve")
                                    .small()
                                    .label(strings.pulls_approve)
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        this.ask_review(number, Verdict::Approve, window, cx)
                                    })),
                            )
                            .child(
                                Button::new("pull-request-changes")
                                    .small()
                                    .label(strings.pulls_request_changes)
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        this.ask_review(number, Verdict::RequestChanges, window, cx)
                                    })),
                            )
                            .child(
                                Button::new("pull-comment")
                                    .small()
                                    .label(strings.pulls_comment)
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        this.ask_review(number, Verdict::Comment, window, cx)
                                    })),
                            )
                            .child(
                                Button::new("pull-merge")
                                    .small()
                                    .icon(Icon::new(IconName::GitMerge))
                                    .label(strings.pulls_merge)
                                    .disabled(conflicts || pull.draft)
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        this.ask_merge(number, window, cx)
                                    })),
                            )
                    })
                    .child(
                        Button::new("pull-web")
                            .ghost()
                            .small()
                            .label(strings.pulls_open_on_github)
                            .on_click(move |_, _, cx| {
                                if url.starts_with("https://") {
                                    cx.open_url(&url);
                                }
                            }),
                    ),
            );
        let part = |id: &'static str, part: Part, label: String, cx: &mut Context<Self>| {
            Button::new(id)
                .ghost()
                .small()
                .label(label)
                .selected(self.pulls.part == part)
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.pulls.part = part;
                    cx.notify();
                }))
        };
        let parts = div()
            .px_4()
            .pb_1()
            .flex()
            .gap_1()
            .border_b_1()
            .border_color(colors.border)
            .child(part(
                "pull-part-talk",
                Part::Talk,
                strings.pulls_conversation.to_owned(),
                cx,
            ))
            .child(part(
                "pull-part-files",
                Part::Files,
                format!("{} {}", strings.pulls_files, details.files.len()),
                cx,
            ))
            .child(part(
                "pull-part-checks",
                Part::Checks,
                format!("{} {}", strings.pulls_checks, pull.checks.len()),
                cx,
            ));
        let content = match self.pulls.part {
            Part::Talk => self.render_pull_talk(shown, colors, cx),
            Part::Files => self.render_pull_files(shown, colors, cx),
            Part::Checks => self.render_pull_checks(shown, colors, cx),
        };
        div()
            .flex_1()
            .min_w_0()
            .h_full()
            .flex()
            .flex_col()
            .child(header)
            .child(parts)
            .children(self.pulls.error.clone().map(|error| {
                div()
                    .px_4()
                    .py_2()
                    .text_xs()
                    .text_color(cx.theme().danger)
                    .child(error)
            }))
            .child(
                div()
                    .id("pull-content")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .p_4()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .children(content),
            )
            .into_any_element()
    }

    fn render_pull_talk(
        &self,
        shown: &Shown,
        colors: Colors,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let strings = self.strings();
        let now = notices::now();
        let theme = cx.theme();
        let (success, danger) = (theme.success, theme.danger);
        let frame =
            |author: &str, when: String, mark: Option<(&str, Hsla)>, text: AnyElement| {
                div()
                    .rounded(px(9.))
                    .border_1()
                    .border_color(colors.border)
                    .child(
                        div()
                            .px_3()
                            .py_1p5()
                            .flex()
                            .items_center()
                            .gap_2()
                            .border_b_1()
                            .border_color(colors.border)
                            .bg(colors.hover.opacity(0.5))
                            .rounded_t(px(9.))
                            .text_xs()
                            .child(
                                div()
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .child(author.to_owned()),
                            )
                            .children(mark.map(|(text, color)| {
                                div().text_color(color).child(text.to_owned())
                            }))
                            .child(div().text_color(colors.faint).child(when)),
                    )
                    .child(div().px_3().py_2().text_sm().child(text))
                    .into_any_element()
            };
        let mut parts = Vec::new();
        let body = if shown.details.body.is_empty() {
            div()
                .text_color(colors.faint)
                .child(strings.pulls_no_description)
                .into_any_element()
        } else {
            TextView::new(&shown.body).into_any_element()
        };
        parts.push(frame(&shown.pull.author, String::new(), None, body));
        for (said, text) in &shown.talk {
            let mark = match said.review.as_deref() {
                Some("APPROVED") => Some((strings.pulls_approved, success)),
                Some("CHANGES_REQUESTED") => Some((strings.pulls_changes_requested, danger)),
                _ => None,
            };
            let body = if said.body.is_empty() {
                div().into_any_element()
            } else {
                TextView::new(text).into_any_element()
            };
            parts.push(frame(&said.author, ago(said.at, now, strings), mark, body));
        }
        // The comments on lines, each with where it is.
        for (index, thread) in shown.threads.iter().enumerate() {
            let place = match thread.line {
                Some(line) => format!("{}:{line}", thread.path),
                None => format!("{} · {}", thread.path, strings.pulls_outdated),
            };
            let (path, line, number) = (thread.path.clone(), thread.line, shown.pull.number);
            parts.push(
                div()
                    .rounded(px(9.))
                    .border_1()
                    .border_color(colors.border)
                    .child(
                        div()
                            .id(("pull-thread-place", index))
                            .px_3()
                            .py_1p5()
                            .border_b_1()
                            .border_color(colors.border)
                            .bg(colors.hover.opacity(0.5))
                            .rounded_t(px(9.))
                            .text_xs()
                            .text_color(colors.accent)
                            .font_family(cx.theme().mono_font_family.clone())
                            .when(line.is_some(), |place| place.cursor_pointer())
                            .child(place)
                            .on_click(cx.listener(move |this, _, window, cx| {
                                if let Some(line) = line {
                                    this.open_line_comments(number, path.clone(), line, window, cx);
                                }
                            })),
                    )
                    .children(thread.comments.iter().map(|comment| {
                        let text = shown.texts.get(&comment.id);
                        div()
                            .px_3()
                            .py_2()
                            .text_sm()
                            .child(
                                div()
                                    .flex()
                                    .gap_2()
                                    .text_xs()
                                    .child(
                                        div()
                                            .font_weight(gpui::FontWeight::SEMIBOLD)
                                            .child(comment.author.clone()),
                                    )
                                    .child(
                                        div()
                                            .text_color(colors.faint)
                                            .child(ago(comment.at, now, strings)),
                                    ),
                            )
                            .children(text.map(TextView::new))
                    }))
                    .into_any_element(),
            );
        }
        parts
    }

    fn render_pull_files(
        &self,
        shown: &Shown,
        colors: Colors,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let theme = cx.theme();
        let (success, danger) = (theme.success, theme.danger);
        shown
            .details
            .files
            .iter()
            .enumerate()
            .map(|(index, file)| {
                let path = file.path.clone();
                let count = shown
                    .threads
                    .iter()
                    .filter(|thread| thread.path == file.path)
                    .count();
                div()
                    .id(("pull-file", index))
                    .px_2()
                    .py_1()
                    .flex()
                    .items_center()
                    .gap_2()
                    .rounded(px(6.))
                    .cursor_pointer()
                    .hover(|row| row.bg(colors.hover))
                    .text_sm()
                    .child(self.file_icon_element(Path::new(&file.path), cx))
                    .child(div().flex_1().min_w_0().truncate().child(file.path.clone()))
                    .when(count > 0, |row| {
                        row.child(
                            div()
                                .text_xs()
                                .text_color(colors.muted)
                                .child(format!("💬 {count}")),
                        )
                    })
                    .child(
                        div()
                            .text_xs()
                            .text_color(success)
                            .child(format!("+{}", file.additions)),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(danger)
                            .child(format!("−{}", file.deletions)),
                    )
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.open_pull_changes(Some(path.clone()), window, cx)
                    }))
                    .into_any_element()
            })
            .collect()
    }

    fn render_pull_checks(
        &self,
        shown: &Shown,
        colors: Colors,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let strings = self.strings();
        if shown.pull.checks.is_empty() {
            return vec![
                div()
                    .text_sm()
                    .text_color(colors.faint)
                    .child(strings.pulls_no_checks)
                    .into_any_element(),
            ];
        }
        shown
            .pull
            .checks
            .iter()
            .enumerate()
            .map(|(index, check)| {
                let (icon, color) = check_look(check.state, cx);
                let failed = check.state == CheckState::Failed && check.job.is_some();
                let (asked, url) = (check.clone(), check.url.clone());
                div()
                    .id(SharedString::from(format!("pull-check-{index}")))
                    .px_2()
                    .py_1()
                    .flex()
                    .items_center()
                    .gap_2()
                    .rounded(px(6.))
                    .hover(|row| row.bg(colors.hover))
                    .text_sm()
                    .child(Icon::new(icon).small().flex_none().text_color(color))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .child(check.name.clone()),
                    )
                    .children(check.workflow.clone().map(|name| {
                        div()
                            .flex_none()
                            .text_xs()
                            .text_color(colors.faint)
                            .child(name)
                    }))
                    .when(failed, |row| {
                        row.child(
                            Button::new(("pull-check-claude", index))
                                .ghost()
                                .xsmall()
                                .icon(Icon::new(IconName::Sparkles))
                                .label(strings.pulls_ask_claude)
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.ask_claude_about_check(&asked, window, cx)
                                })),
                        )
                    })
                    .children(url.map(|url| {
                        Button::new(("pull-check-open", index))
                            .ghost()
                            .xsmall()
                            .label(strings.pulls_open)
                            .on_click(move |_, _, cx| cx.open_url(&url))
                    }))
                    .into_any_element()
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::os::unix::fs::PermissionsExt as _;
    use std::process::Command;
    use std::sync::{Mutex, MutexGuard};
    use std::time::Instant;

    use gpui::{TestAppContext, VisualTestContext};

    use super::super::github::FAKE_GH;
    use super::*;

    static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());

    fn git(folder: &Path, args: &[&str]) -> String {
        let output = Command::new("git")
            .arg("-C")
            .arg(folder)
            .args(args)
            .output()
            .unwrap();
        assert!(output.status.success(), "git {args:?}");
        String::from_utf8_lossy(&output.stdout).trim().to_owned()
    }

    struct Set<'a> {
        _folder: tempfile::TempDir,
        root: PathBuf,
        /// What `gh` was asked, a line each.
        asked: PathBuf,
        view: Entity<Workbench>,
        cx: &'a mut VisualTestContext,
        _one: MutexGuard<'static, ()>,
    }

    /// A repository on a branch of its own, with a `gh` that answers about
    /// its pull request, number 12.
    fn set(cx: &mut TestAppContext) -> Set<'_> {
        let one = ONE_AT_A_TIME
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        cx.executor().allow_parking();
        cx.update(|cx| {
            gpui_component::init(cx);
            crate::workbench_window::init(cx);
        });
        let folder = tempfile::tempdir().unwrap();
        let base = fs::canonicalize(folder.path()).unwrap();
        let root = base.join("app");
        fs::create_dir(&root).unwrap();
        git(&root, &["init", "-q", "-b", "main"]);
        git(&root, &["config", "user.name", "Ada"]);
        git(&root, &["config", "user.email", "ada@example.com"]);
        git(&root, &["config", "commit.gpgsign", "false"]);
        fs::write(root.join("a.rs"), "uno\ndue\ntre\n").unwrap();
        fs::write(root.join("b.rs"), "b\n").unwrap();
        git(&root, &["add", "."]);
        git(&root, &["commit", "-q", "-m", "prima"]);
        let from = git(&root, &["rev-parse", "HEAD"]);
        git(&root, &["checkout", "-q", "-b", "ramo"]);
        fs::write(root.join("a.rs"), "uno\nDUE\ntre\nquattro\n").unwrap();
        git(&root, &["commit", "-q", "-a", "-m", "poi"]);
        let to = git(&root, &["rev-parse", "HEAD"]);

        let pull = json!({
            "number": 12, "title": "Due in grande", "author": { "login": "ada" },
            "headRefName": "ramo", "baseRefName": "main", "isDraft": false, "state": "OPEN",
            "updatedAt": "2026-11-19T18:08:12Z", "reviewDecision": "REVIEW_REQUIRED",
            "url": "https://github.com/a/b/pull/12",
            "statusCheckRollup": [
                { "name": "test", "status": "COMPLETED", "conclusion": "FAILURE", "detailsUrl": "https://github.com/a/b/actions/runs/1/job/78" },
            ],
            "body": "Cosa fa", "headRefOid": to, "baseRefOid": from, "mergeable": "MERGEABLE",
            "additions": 2, "deletions": 1,
            "files": [{ "path": "a.rs", "additions": 2, "deletions": 1 }],
            "reviews": [], "comments": [{ "author": { "login": "bo" }, "body": "Bella", "createdAt": "2026-11-19T17:00:00Z" }],
        });
        let comments = json!([
            { "id": 1, "path": "a.rs", "line": 2, "side": "RIGHT", "user": { "login": "bo" }, "body": "Perché in grande?", "created_at": "2026-11-19T17:30:00Z", "in_reply_to_id": null },
            { "id": 2, "path": "a.rs", "line": 2, "side": "RIGHT", "user": { "login": "ada" }, "body": "Si legge meglio", "created_at": "2026-11-19T17:40:00Z", "in_reply_to_id": 1 },
        ]);
        fs::write(base.join("pull.json"), pull.to_string()).unwrap();
        fs::write(base.join("list.json"), json!([pull]).to_string()).unwrap();
        fs::write(base.join("comments.json"), comments.to_string()).unwrap();
        let asked = base.join("asked");
        fs::write(&asked, "").unwrap();
        let script = format!(
            r#"#!/bin/sh
cd '{base}'
echo "$*" >> asked
case "$*" in
  "pr list"*) cat list.json ;;
  "pr view"*) cat pull.json ;;
  "api --paginate"*) cat comments.json ;;
  "run view --job 78 --log-failed") printf 'riga\nerrore vero\n' ;;
  *POST*) cat > sent; if [ -e refuse ]; then echo '{{"message":"Validation Failed"}}'; echo 'gh: Validation Failed (HTTP 422)' >&2; exit 1; fi ;;
  "pr review"*|"pr merge"*) cat > sent ;;
  *) exit 2 ;;
esac
"#,
            base = base.display()
        );
        let gh = base.join("gh");
        fs::write(&gh, script).unwrap();
        fs::set_permissions(&gh, fs::Permissions::from_mode(0o755)).unwrap();
        *FAKE_GH
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(gh);

        let settings_file = base.join("settings.json");
        fs::write(&settings_file, r#"{ "keel.language": "it" }"#).unwrap();
        let settings = Settings::load(&settings_file, None);
        let opening = Opening::new(Some(root.clone()), None);
        let (view, cx) =
            cx.add_window_view(|window, cx| Workbench::new(opening, settings, None, window, cx));
        view.update(cx, |view, cx| view.refresh_git(cx));
        let set = Set {
            _folder: folder,
            root,
            asked,
            view,
            cx,
            _one: one,
        };
        let Set { view, cx, .. } = &set;
        let _ = (view, cx);
        set
    }

    fn wait_until(
        cx: &mut VisualTestContext,
        what: &str,
        check: impl Fn(&mut VisualTestContext) -> bool,
    ) {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            cx.executor().advance_clock(Duration::from_millis(300));
            cx.run_until_parked();
            if check(cx) {
                return;
            }
            assert!(Instant::now() < deadline, "gave up waiting for {what}");
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    #[gpui::test]
    fn nothing_is_asked_of_github_before_the_page_is_opened(cx: &mut TestAppContext) {
        let set = set(cx);
        let (view, cx) = (set.view.clone(), set.cx);
        wait_until(cx, "the branch", |cx| {
            view.read_with(cx, |view, _| view.project.branch.as_deref() == Some("ramo"))
        });
        cx.executor().advance_clock(Duration::from_secs(600));
        cx.run_until_parked();
        assert_eq!(fs::read_to_string(&set.asked).unwrap(), "");
        assert_eq!(view.read_with(cx, |view, _| view.pull_status()), None);
    }

    #[gpui::test]
    fn the_page_the_status_bar_and_the_editors_show_it(cx: &mut TestAppContext) {
        let set = set(cx);
        let (view, root, cx) = (set.view.clone(), set.root.clone(), set.cx);
        view.update_in(cx, |view, window, cx| {
            view.open_file(&root.join("a.rs"), window, cx)
        });
        wait_until(cx, "the branch", |cx| {
            view.read_with(cx, |view, _| view.project.branch.as_deref() == Some("ramo"))
        });
        view.update_in(cx, |view, window, cx| view.open_pull_requests(window, cx));
        wait_until(cx, "the details", |cx| {
            view.read_with(cx, |view, _| {
                view.pulls.shown.is_some() && view.pulls.current.is_some()
            })
        });
        view.update(cx, |view, cx| {
            let shown = view.pulls.shown.as_ref().unwrap();
            assert_eq!(shown.pull.title, "Due in grande");
            assert_eq!(shown.talk.len(), 1);
            assert_eq!(shown.threads[0].comments.len(), 2);
            assert_eq!(view.pull_status().as_deref(), Some("PR #12 ✗"));
            // The page draws in each of its parts.
            let colors = Colors::new(cx);
            for part in [Part::Talk, Part::Files, Part::Checks] {
                view.pulls.part = part;
                view.render_pulls_page(colors, cx);
            }
            // In the editor, beside the line.
            let uri = Uri::file(&root.join("a.rs")).to_string();
            let comments = view.pull_comments_of(&uri);
            assert_eq!(comments[&1], "💬 bo: Perché in grande? (+1)");
            let pane = view.panes.iter().position(|pane| pane.uri == uri);
            assert!(pane.is_none(), "the page is in front");
        });
        // Only read: nothing was written there.
        let asked = fs::read_to_string(&set.asked).unwrap();
        assert!(
            !asked.contains("POST") && !asked.contains("pr review"),
            "{asked}"
        );
    }

    #[gpui::test]
    fn its_changes_open_with_the_comments_on_their_lines(cx: &mut TestAppContext) {
        let set = set(cx);
        let (view, cx) = (set.view.clone(), set.cx);
        wait_until(cx, "the repository", |cx| {
            view.read_with(cx, |view, _| view.scm.repo.is_some())
        });
        view.update_in(cx, |view, window, cx| view.open_pull_requests(window, cx));
        wait_until(cx, "the details", |cx| {
            view.read_with(cx, |view, _| view.pulls.shown.is_some())
        });
        view.update_in(cx, |view, window, cx| {
            view.open_pull_changes(None, window, cx)
        });
        wait_until(cx, "the changes", |cx| {
            view.read_with(cx, |view, _| {
                view.panes[view.active].uri.starts_with("keel-pull:12/")
            })
        });
        let uri = view.read_with(cx, |view, _| view.panes[view.active].uri.clone());
        view.read_with(cx, |view, _| {
            let Backing::Diff(tab) = &view.documents[&uri].backing else {
                panic!("not a diff");
            };
            assert_eq!(tab.title, "PR #12");
            let doc = tab.doc.as_ref().unwrap();
            assert_eq!((doc.files_len(), doc.counts()), (1, (2, 1)));
            assert!(!view.diff_editable(&uri));
        });

        // A double click on the line: its comments, and an answer.
        view.update_in(cx, |view, window, cx| {
            view.diff_line_double_click(&uri, 0, 1, window, cx);
            let card = view.pulls.card.as_ref().unwrap();
            assert_eq!((card.line, card.texts.len(), card.first), (2, 2, Some(1)));
            let colors = Colors::new(cx);
            assert!(view.render_thread_card(colors, cx).is_some());
            let input = card.input.clone();
            input.update(cx, |input, cx| {
                input.set_value("Va bene «così»", window, cx)
            });
        });
        // Refused: what was written stays, with the reason.
        let base = set.asked.parent().unwrap().to_path_buf();
        fs::write(base.join("refuse"), "").unwrap();
        view.update(cx, |view, cx| view.send_line_comment(cx));
        wait_until(cx, "the refusal", |cx| {
            view.read_with(cx, |view, _| {
                view.pulls
                    .card
                    .as_ref()
                    .is_some_and(|card| card.error.is_some())
            })
        });
        view.read_with(cx, |view, cx| {
            let card = view.pulls.card.as_ref().unwrap();
            assert!(card.error.as_ref().unwrap().contains("Validation Failed"));
            assert_eq!(card.input.read(cx).value().to_string(), "Va bene «così»");
        });
        fs::remove_file(base.join("refuse")).unwrap();
        view.update(cx, |view, cx| view.send_line_comment(cx));
        wait_until(cx, "the answer", |cx| {
            view.read_with(cx, |view, _| view.pulls.card.is_none())
        });
        assert_eq!(
            fs::read_to_string(base.join("sent")).unwrap(),
            "Va bene «così»"
        );
        let asked = fs::read_to_string(&set.asked).unwrap();
        assert!(
            asked.contains("pulls/12/comments/1/replies -F body=@-"),
            "{asked}"
        );

        // A line with none: a new comment, on the last commit.
        view.update_in(cx, |view, window, cx| {
            view.diff_line_double_click(&uri, 0, 3, window, cx);
            let card = view.pulls.card.as_ref().unwrap();
            assert_eq!((card.line, card.first), (4, None));
            let input = card.input.clone();
            input.update(cx, |input, cx| input.set_value("E quattro?", window, cx));
            view.send_line_comment(cx);
        });
        wait_until(cx, "the comment", |cx| {
            view.read_with(cx, |view, _| view.pulls.card.is_none())
        });
        let asked = fs::read_to_string(&set.asked).unwrap();
        assert!(
            asked.contains("-f path=a.rs -F line=4 -f side=RIGHT"),
            "{asked}"
        );
    }

    #[gpui::test]
    fn a_review_goes_only_with_its_text(cx: &mut TestAppContext) {
        let set = set(cx);
        let (view, cx) = (set.view.clone(), set.cx);
        wait_until(cx, "the repository", |cx| {
            view.read_with(cx, |view, _| view.scm.repo.is_some())
        });
        let gh = view.read_with(cx, |view, _| {
            Gh::find(view.scm.repo.as_ref().unwrap().root()).unwrap()
        });
        gh.review(12, Verdict::RequestChanges, "Manca un test")
            .unwrap();
        gh.review(12, Verdict::Approve, "").unwrap();
        gh.merge(12, "--squash").unwrap();
        assert!(gh.failed_log(78).unwrap().ends_with("errore vero"));
        assert!(gh.checkout(12).is_err());
        let asked = fs::read_to_string(&set.asked).unwrap();
        let lines: Vec<&str> = asked.lines().collect();
        assert_eq!(
            lines[lines.len() - 5..],
            [
                "pr review 12 --request-changes --body-file -",
                "pr review 12 --approve",
                "pr merge 12 --squash",
                "run view --job 78 --log-failed",
                "pr checkout 12",
            ]
        );
    }
}
