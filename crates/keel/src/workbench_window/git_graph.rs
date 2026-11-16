//! Git Graph in a tab: the history of the repository with its branches as
//! colored lines, each commit's description, date, author and hash, a
//! branch filter, search, and more commits on request. The lanes come from
//! `git::graph`; Keel draws them on a canvas, row by row.

use std::ops::Range;

use git::graph::{GraphRow, Point as GraphPoint};
use git::{Commit, LogOptions, RefLabel};
use gpui::{
    AnyElement, Bounds, Entity, Hsla, PathBuilder, Pixels, StatefulInteractiveElement as _,
    UniformListScrollHandle, canvas, point, size, uniform_list,
};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::input::{Input, InputEvent, InputState};
use gpui_component::menu::ContextMenuExt as _;
use gpui_component::{ActiveTheme as _, Icon, Sizable as _};
use gpui_kit_assets::IconName;

use super::shell::{Colors, note};
use super::*;
use crate::quick_pick::{self, PickItem};

actions!(keel, [ShowGitGraph]);

/// The uri of the graph's tab: one per window.
pub(super) const GRAPH_URI: &str = "keel-git-graph:";
/// How many commits are loaded at a time.
const PAGE: usize = 500;
const ROW_HEIGHT: f32 = 26.;
const LANE_WIDTH: f32 = 14.;
/// The hash of the row of changes not committed yet.
const UNCOMMITTED: &str = "*";

/// Git Graph's colors for its lanes.
pub(super) const PALETTE: [u32; 12] = [
    0x0085d9, 0xd9008f, 0x00d90a, 0xd98500, 0xa300d9, 0xff0000, 0x00d9cc, 0xe138e8, 0x85d900,
    0xdc5b23, 0x6f24d6, 0xffcc00,
];

pub(super) fn lane_color(index: usize) -> Hsla {
    gpui::rgb(PALETTE[index % PALETTE.len()]).into()
}

/// The graph's tab.
pub(super) struct GraphTab {
    pub(super) commits: Vec<Commit>,
    rows: Vec<GraphRow>,
    /// The widest row, in lanes.
    pub(super) lanes: usize,
    /// The branches shown; every one when empty.
    branches: Vec<String>,
    max: usize,
    has_more: bool,
    /// The commit chosen, by hash.
    pub(super) selected: Option<String>,
    pub(super) search: Entity<InputState>,
    pub(super) matches: Vec<usize>,
    current_match: usize,
    scroll: UniformListScrollHandle,
    error: Option<String>,
    _search_events: Subscription,
}

/// A date as Git Graph writes it: `16 nov 2026 14:32`, in local time.
pub(super) fn format_date(seconds: i64, italian: bool) -> String {
    // SAFETY: `localtime_r` fills the struct it is given.
    let parts = unsafe {
        let time = seconds as libc::time_t;
        let mut parts: libc::tm = std::mem::zeroed();
        libc::localtime_r(&time, &mut parts);
        parts
    };
    const ITALIAN: [&str; 12] = [
        "gen", "feb", "mar", "apr", "mag", "giu", "lug", "ago", "set", "ott", "nov", "dic",
    ];
    const ENGLISH: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    let month = (parts.tm_mon.clamp(0, 11)) as usize;
    let name = if italian {
        ITALIAN[month]
    } else {
        ENGLISH[month]
    };
    format!(
        "{} {name} {} {:02}:{:02}",
        parts.tm_mday,
        parts.tm_year + 1900,
        parts.tm_hour,
        parts.tm_min
    )
}

/// Whether `commit` matches a search: its message, hash, author or refs.
fn matches(commit: &Commit, query: &str) -> bool {
    let query = query.to_lowercase();
    commit.subject.to_lowercase().contains(&query)
        || commit.hash.starts_with(&query)
        || commit.author.to_lowercase().contains(&query)
        || commit
            .refs
            .iter()
            .any(|label| label.name().to_lowercase().contains(&query))
}

/// Puts the row of changes not committed yet above HEAD, as Git Graph does.
fn with_uncommitted(mut commits: Vec<Commit>, changes: usize, label: &str) -> Vec<Commit> {
    if changes == 0 {
        return commits;
    }
    let head = commits.iter().find(|commit| {
        commit
            .refs
            .iter()
            .any(|label| matches!(label, RefLabel::Head(_) | RefLabel::DetachedHead))
    });
    let Some(head) = head else { return commits };
    let row = Commit {
        hash: UNCOMMITTED.to_owned(),
        parents: vec![head.hash.clone()],
        author: String::new(),
        email: String::new(),
        time: 0,
        refs: Vec::new(),
        subject: format!("{label} ({changes})"),
    };
    commits.insert(0, row);
    commits
}

impl Workbench {
    pub(super) fn graph_tab(&self) -> Option<&GraphTab> {
        match &self.documents.get(GRAPH_URI)?.backing {
            Backing::Graph(tab) => Some(tab),
            _ => None,
        }
    }

    pub(super) fn graph_tab_mut(&mut self) -> Option<&mut GraphTab> {
        match &mut self.documents.get_mut(GRAPH_URI)?.backing {
            Backing::Graph(tab) => Some(tab),
            _ => None,
        }
    }

    /// Opens the graph in a tab, or brings it forward.
    pub(super) fn open_git_graph(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.scm.repo.is_none() {
            self.status_message = Some(self.strings().graph_no_repository.to_owned());
            cx.notify();
            return;
        }
        if !self.documents.contains_key(GRAPH_URI) {
            let search = cx.new(|cx| {
                InputState::new(window, cx).placeholder(self.strings().graph_search_placeholder)
            });
            let search_events = cx.subscribe_in(
                &search,
                window,
                |this, _, event: &InputEvent, _, cx| match event {
                    InputEvent::Change => this.search_graph(cx),
                    InputEvent::PressEnter { shift, .. } => this.next_graph_match(!shift, cx),
                    _ => {}
                },
            );
            let tab = GraphTab {
                commits: Vec::new(),
                rows: Vec::new(),
                lanes: 1,
                branches: Vec::new(),
                max: PAGE,
                has_more: false,
                selected: None,
                search,
                matches: Vec::new(),
                current_match: 0,
                scroll: UniformListScrollHandle::new(),
                error: None,
                _search_events: search_events,
            };
            self.documents.insert(
                GRAPH_URI.to_owned(),
                OpenDocument {
                    uri: GRAPH_URI.to_owned(),
                    backing: Backing::Graph(Box::new(tab)),
                    language_id: "git-graph".to_owned(),
                    version: 1,
                    synced_text: String::new(),
                    modified: false,
                },
            );
            self.reload_git_graph(cx);
        }
        let column = self.active.min(self.panes.len().saturating_sub(1));
        self.show_tab(GRAPH_URI, column, OpenAs::Normal, true, window, cx);
    }

    /// Reads the history again, keeping the commit chosen.
    pub(super) fn reload_git_graph(&mut self, cx: &mut Context<Self>) {
        let Some(repo) = self.scm.repo.clone() else {
            return;
        };
        let Some(tab) = self.graph_tab() else { return };
        let options = LogOptions {
            branches: tab.branches.clone(),
            max: tab.max,
        };
        let changes = self.scm.status.count();
        let label = self.strings().graph_uncommitted.to_owned();
        let task = cx.background_spawn(async move {
            let commits = repo.log(&options)?;
            let has_more = commits.len() >= options.max;
            let commits = with_uncommitted(commits, changes, &label);
            let rows = git::graph::layout(&commits, PALETTE.len());
            Ok::<_, git::GitError>((commits, rows, has_more))
        });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            this.update(cx, |this, cx| {
                let Some(tab) = this.graph_tab_mut() else {
                    return;
                };
                match result {
                    Ok((commits, rows, has_more)) => {
                        tab.lanes = rows.iter().map(|row| row.width).max().unwrap_or(1).max(1);
                        tab.commits = commits;
                        tab.rows = rows;
                        tab.has_more = has_more;
                        tab.error = None;
                    }
                    Err(error) => tab.error = Some(error.to_string()),
                }
                this.search_graph(cx);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Git changed: the open graph follows.
    pub(super) fn git_graph_changed(&mut self, cx: &mut Context<Self>) {
        if self.graph_tab().is_some() {
            self.reload_git_graph(cx);
        }
    }

    fn load_more_commits(&mut self, cx: &mut Context<Self>) {
        if let Some(tab) = self.graph_tab_mut() {
            tab.max += PAGE;
            self.reload_git_graph(cx);
        }
    }

    /// Chooses the branches the graph shows: every one, or a single one.
    fn choose_graph_branches(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(repo) = self.scm.repo.clone() else {
            return;
        };
        let branches = repo.branches().unwrap_or_default();
        let strings = self.strings();
        let mut items = vec![PickItem::new(strings.graph_all_branches)];
        items.extend(
            branches
                .iter()
                .map(|branch| PickItem::new(branch.name.clone())),
        );
        let chosen = quick_pick::show(
            None,
            Some(strings.graph_branches.to_owned()),
            items,
            window,
            cx,
        );
        cx.spawn_in(window, async move |this, cx| {
            let Some(index) = chosen.await.ok().flatten() else {
                return;
            };
            this.update(cx, |this, cx| {
                let Some(tab) = this.graph_tab_mut() else {
                    return;
                };
                tab.branches = match index {
                    0 => Vec::new(),
                    index => branches
                        .get(index - 1)
                        .map(|branch| vec![branch.name.clone()])
                        .unwrap_or_default(),
                };
                tab.max = PAGE;
                this.reload_git_graph(cx);
            })
            .ok();
        })
        .detach();
    }

    fn search_graph(&mut self, cx: &mut Context<Self>) {
        let Some(tab) = self.graph_tab_mut() else {
            return;
        };
        let query = tab.search.read(cx).value().trim().to_owned();
        tab.matches = if query.is_empty() {
            Vec::new()
        } else {
            tab.commits
                .iter()
                .enumerate()
                .filter(|(_, commit)| commit.hash != UNCOMMITTED && matches(commit, &query))
                .map(|(index, _)| index)
                .collect()
        };
        tab.current_match = 0;
        if let Some(&row) = tab.matches.first() {
            tab.scroll.scroll_to_item(row, gpui::ScrollStrategy::Center);
        }
        cx.notify();
    }

    fn next_graph_match(&mut self, forward: bool, cx: &mut Context<Self>) {
        let Some(tab) = self.graph_tab_mut() else {
            return;
        };
        let count = tab.matches.len();
        if count == 0 {
            return;
        }
        tab.current_match = if forward {
            (tab.current_match + 1) % count
        } else {
            (tab.current_match + count - 1) % count
        };
        let row = tab.matches[tab.current_match];
        tab.scroll.scroll_to_item(row, gpui::ScrollStrategy::Center);
        cx.notify();
    }

    pub(super) fn git_graph_actions(&self, root: gpui::Div, cx: &mut Context<Self>) -> gpui::Div {
        root.on_action(
            cx.listener(|this, _: &ShowGitGraph, window, cx| this.open_git_graph(window, cx)),
        )
    }

    // -----------------------------------------------------------------
    // Drawing

    pub(super) fn render_git_graph(
        &self,
        tab: &GraphTab,
        colors: Colors,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let strings = self.strings();
        let branch_label = match tab.branches.first() {
            Some(branch) => branch.clone(),
            None => strings.graph_all_branches.to_owned(),
        };
        let counter = match tab.matches.len() {
            0 if tab.search.read(cx).value().trim().is_empty() => String::new(),
            0 => strings.no_results.to_owned(),
            count => format!("{}/{count}", tab.current_match + 1),
        };
        let toolbar = div()
            .h(px(34.))
            .flex_none()
            .px_2()
            .flex()
            .items_center()
            .gap_1()
            .border_b_1()
            .border_color(colors.border)
            .child(
                div()
                    .px_1()
                    .text_xs()
                    .text_color(colors.muted)
                    .child(strings.graph_branches),
            )
            .child(
                Button::new("graph-branches")
                    .ghost()
                    .xsmall()
                    .label(branch_label)
                    .icon(Icon::new(IconName::ChevronDown))
                    .on_click(
                        cx.listener(|this, _, window, cx| this.choose_graph_branches(window, cx)),
                    ),
            )
            .child(div().flex_1())
            .child(div().w(px(240.)).child(Input::new(&tab.search).xsmall()))
            .child(
                div()
                    .min_w(px(40.))
                    .text_xs()
                    .text_color(colors.muted)
                    .child(counter),
            )
            .child(
                Button::new("graph-previous")
                    .ghost()
                    .xsmall()
                    .icon(Icon::new(IconName::ChevronUp))
                    .tooltip(strings.find_previous)
                    .on_click(cx.listener(|this, _, _, cx| this.next_graph_match(false, cx))),
            )
            .child(
                Button::new("graph-next")
                    .ghost()
                    .xsmall()
                    .icon(Icon::new(IconName::ChevronDown))
                    .tooltip(strings.find_next)
                    .on_click(cx.listener(|this, _, _, cx| this.next_graph_match(true, cx))),
            )
            .child(
                Button::new("graph-fetch")
                    .ghost()
                    .xsmall()
                    .icon(Icon::new(IconName::ArrowDown))
                    .tooltip(strings.scm_fetch)
                    .on_click(cx.listener(|_, _, window, cx| {
                        window.dispatch_action(Box::new(GitFetch), cx)
                    })),
            )
            .child(
                Button::new("graph-refresh")
                    .ghost()
                    .xsmall()
                    .icon(Icon::new(IconName::RefreshCw))
                    .tooltip(strings.scm_refresh)
                    .on_click(cx.listener(|this, _, _, cx| this.reload_git_graph(cx))),
            );
        let graph_width = tab.lanes as f32 * LANE_WIDTH + LANE_WIDTH;
        let header = div()
            .h(px(24.))
            .flex_none()
            .px_2()
            .flex()
            .items_center()
            .gap_3()
            .text_xs()
            .text_color(colors.faint)
            .border_b_1()
            .border_color(colors.border)
            .child(
                div()
                    .w(px(graph_width))
                    .flex_none()
                    .child(strings.graph_column_graph),
            )
            .child(div().flex_1().child(strings.graph_column_description))
            .child(
                div()
                    .w(px(130.))
                    .flex_none()
                    .child(strings.graph_column_date),
            )
            .child(
                div()
                    .w(px(130.))
                    .flex_none()
                    .child(strings.graph_column_author),
            )
            .child(
                div()
                    .w(px(70.))
                    .flex_none()
                    .child(strings.graph_column_commit),
            );
        let body: AnyElement = match &tab.error {
            Some(error) => note(error.clone(), colors).into_any_element(),
            None => {
                let count = tab.commits.len() + usize::from(tab.has_more);
                uniform_list(
                    "git-graph",
                    count,
                    cx.processor(|this, range: Range<usize>, _, cx| {
                        this.render_graph_rows(range, cx)
                    }),
                )
                .track_scroll(&tab.scroll)
                .size_full()
                .into_any_element()
            }
        };
        div()
            .size_full()
            .flex()
            .flex_col()
            .child(toolbar)
            .child(header)
            .child(div().flex_1().min_h_0().child(body))
            .children(self.render_commit_details(colors, cx))
            .into_any_element()
    }

    fn render_graph_rows(
        &mut self,
        range: Range<usize>,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let colors = Colors::new(cx);
        range
            .map(|index| self.render_graph_row(index, colors, cx))
            .collect()
    }

    fn render_graph_row(&self, index: usize, colors: Colors, cx: &mut Context<Self>) -> AnyElement {
        let Some(tab) = self.graph_tab() else {
            return div().into_any_element();
        };
        let strings = self.strings();
        let Some(commit) = tab.commits.get(index) else {
            // The last row loads more.
            return div()
                .id(("graph-more", index))
                .h(px(ROW_HEIGHT))
                .px_4()
                .flex()
                .items_center()
                .text_sm()
                .text_color(colors.accent)
                .cursor_pointer()
                .on_click(cx.listener(|this, _, _, cx| this.load_more_commits(cx)))
                .child(strings.graph_load_more(PAGE))
                .into_any_element();
        };
        let row = tab.rows.get(index).cloned();
        let lanes = tab.lanes;
        let selected = tab.selected.as_deref() == Some(commit.hash.as_str());
        let is_match = tab.matches.contains(&index);
        let current_match = tab.matches.get(tab.current_match) == Some(&index);
        let uncommitted = commit.hash == UNCOMMITTED;
        let is_head = commit
            .refs
            .iter()
            .any(|label| matches!(label, RefLabel::Head(_) | RefLabel::DetachedHead));
        let theme = cx.theme();
        let background = theme.background;
        let graph = canvas(
            |_, _, _| {},
            move |bounds, _, window, _| {
                if let Some(row) = &row {
                    paint_graph_row(bounds, row, is_head, uncommitted, background, window);
                }
            },
        )
        .w(px(lanes as f32 * LANE_WIDTH + LANE_WIDTH))
        .h_full()
        .flex_none();
        let lane = tab.rows.get(index).map_or(0, |row| row.color);
        let labels = commit
            .refs
            .iter()
            .map(|label| render_ref(label, lane, colors));
        let hash = commit.hash.clone();
        let row = div()
            .id(("graph-row", index))
            .h(px(ROW_HEIGHT))
            .px_2()
            .flex()
            .items_center()
            .gap_3()
            .text_sm()
            .cursor_pointer()
            .when(selected, |row| row.bg(colors.selected))
            .when(!selected && current_match, |row| {
                row.bg(theme.warning.opacity(0.3))
            })
            .when(!selected && is_match && !current_match, |row| {
                row.bg(theme.warning.opacity(0.12))
            })
            .when(!selected, |row| row.hover(|row| row.bg(colors.hover)))
            .on_click(cx.listener(move |this, _, window, cx| this.select_commit(&hash, window, cx)))
            .child(graph)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .items_center()
                    .gap_1()
                    .children(labels)
                    .child(
                        div()
                            .min_w_0()
                            .truncate()
                            .text_color(if uncommitted {
                                colors.muted
                            } else {
                                colors.text
                            })
                            .when(uncommitted, |text| text.italic())
                            .child(commit.subject.clone()),
                    ),
            )
            .child(
                div()
                    .w(px(130.))
                    .flex_none()
                    .truncate()
                    .text_xs()
                    .text_color(colors.muted)
                    .child(if uncommitted {
                        String::new()
                    } else {
                        format_date(commit.time, self.language == UiLanguage::Italian)
                    }),
            )
            .child(
                div()
                    .w(px(130.))
                    .flex_none()
                    .truncate()
                    .text_xs()
                    .text_color(colors.muted)
                    .child(commit.author.clone()),
            )
            .child(
                div()
                    .w(px(70.))
                    .flex_none()
                    .text_xs()
                    .font_family(theme.mono_font_family.clone())
                    .text_color(colors.faint)
                    .child(if uncommitted {
                        String::new()
                    } else {
                        commit.short_hash().to_owned()
                    }),
            );
        if uncommitted {
            return row.into_any_element();
        }
        // Right-click: the commit's menu, as in Git Graph.
        let strings = self.strings();
        let focus = self.focus_handle.clone();
        let commit = commit.clone();
        row.context_menu(move |menu, _, _| {
            super::commit_view::commit_menu(strings, &commit, menu.action_context(focus.clone()))
        })
        .into_any_element()
    }
}

/// A branch or tag next to a commit's message.
fn render_ref(label: &RefLabel, lane: usize, colors: Colors) -> AnyElement {
    let color = lane_color(lane);
    let (icon, name, strong) = match label {
        RefLabel::Head(name) => (IconName::GitBranch, name.as_str(), true),
        RefLabel::DetachedHead => (IconName::GitBranch, "HEAD", true),
        RefLabel::Branch(name) => (IconName::GitBranch, name.as_str(), false),
        RefLabel::Remote(name) => (IconName::Globe, name.as_str(), false),
        RefLabel::Tag(name) => (IconName::Tag, name.as_str(), false),
    };
    div()
        .flex_none()
        .px_1p5()
        .h(px(18.))
        .flex()
        .items_center()
        .gap_1()
        .rounded(px(4.))
        .border_1()
        .border_color(color)
        .when(strong, |label| label.bg(color.opacity(0.25)))
        .text_xs()
        .text_color(colors.text)
        .child(Icon::new(icon).xsmall().text_color(color))
        .child(name.to_owned())
        .into_any_element()
}

/// Paints one row of the graph: its lines, then the commit's dot.
fn paint_graph_row(
    bounds: Bounds<Pixels>,
    row: &GraphRow,
    is_head: bool,
    uncommitted: bool,
    background: Hsla,
    window: &mut Window,
) {
    let x = |column: usize| bounds.origin.x + px(LANE_WIDTH * column as f32 + LANE_WIDTH / 2. + 4.);
    let top = bounds.origin.y;
    let middle = top + bounds.size.height / 2.;
    let bottom = top + bounds.size.height;
    let place = |at: GraphPoint| match at {
        GraphPoint::Top(column) => point(x(column), top),
        GraphPoint::Dot => point(x(row.column), middle),
        GraphPoint::Bottom(column) => point(x(column), bottom),
    };
    for line in &row.lines {
        let (from, to) = (place(line.from), place(line.to));
        let mut path = PathBuilder::stroke(px(2.));
        path.move_to(from);
        if from.x == to.x {
            path.line_to(to);
        } else {
            // A soft bend, vertical at both ends.
            let half = (to.y - from.y) / 2.;
            path.cubic_bezier_to(to, point(from.x, from.y + half), point(to.x, to.y - half));
        }
        if let Ok(path) = path.build() {
            let color = lane_color(line.color);
            window.paint_path(
                path,
                if uncommitted {
                    color.opacity(0.5)
                } else {
                    color
                },
            );
        }
    }
    let color = lane_color(row.color);
    let radius = px(4.);
    let center = place(GraphPoint::Dot);
    let dot = Bounds::new(
        point(center.x - radius, center.y - radius),
        size(radius * 2., radius * 2.),
    );
    let fill = if is_head || uncommitted {
        background
    } else {
        color
    };
    window.paint_quad(
        gpui::fill(dot, fill)
            .corner_radii(radius)
            .border_widths(px(2.))
            .border_color(if uncommitted {
                color.opacity(0.5)
            } else {
                color
            }),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn commit(hash: &str, parents: &[&str], refs: Vec<RefLabel>, subject: &str) -> Commit {
        Commit {
            hash: hash.to_owned(),
            parents: parents.iter().map(|parent| (*parent).to_owned()).collect(),
            author: "Manuel".to_owned(),
            email: String::new(),
            time: 0,
            refs,
            subject: subject.to_owned(),
        }
    }

    #[test]
    fn uncommitted_changes_sit_above_head() {
        let commits = vec![
            commit("b", &["a"], vec![RefLabel::Branch("fix".into())], "fix"),
            commit("a", &[], vec![RefLabel::Head("main".into())], "first"),
        ];
        let with = with_uncommitted(commits.clone(), 3, "Modifiche non committate");
        assert_eq!(with[0].hash, UNCOMMITTED);
        assert_eq!(with[0].parents, ["a"]);
        assert_eq!(with[0].subject, "Modifiche non committate (3)");
        assert_eq!(with_uncommitted(commits, 0, "x").len(), 2);
    }

    #[test]
    fn search_reads_messages_hashes_authors_and_branches() {
        let found = commit(
            "4f2a9c1e",
            &[],
            vec![RefLabel::Tag("v1.0".into())],
            "Aggiungi il grafico",
        );
        assert!(matches(&found, "grafico"));
        assert!(matches(&found, "4f2a"));
        assert!(matches(&found, "manuel"));
        assert!(matches(&found, "V1.0"));
        assert!(!matches(&found, "assente"));
    }

    #[test]
    fn dates_like_git_graph() {
        let text = format_date(1_700_000_000, true);
        assert!(text.contains("nov 2023"), "{text}");
        assert!(format_date(1_700_000_000, false).contains("Nov 2023"));
    }
}

#[cfg(test)]
mod window_tests {
    use std::fs;
    use std::process::Command;
    use std::time::{Duration, Instant};

    use gpui::{TestAppContext, VisualTestContext};

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
    fn the_graph_its_commits_and_their_menu(cx: &mut TestAppContext) {
        cx.executor().allow_parking();
        cx.update(|cx| {
            gpui_component::init(cx);
            crate::workbench_window::init(cx);
        });
        let folder = tempfile::tempdir().unwrap();
        let root = fs::canonicalize(folder.path()).unwrap();
        git(&root, &["init", "-q", "-b", "main"]);
        git(&root, &["config", "user.name", "Manuel"]);
        git(&root, &["config", "user.email", "m@example.com"]);
        git(&root, &["config", "commit.gpgsign", "false"]);
        fs::write(root.join("a.txt"), "uno\n").unwrap();
        git(&root, &["add", "."]);
        git(&root, &["commit", "-q", "-m", "first"]);
        git(&root, &["switch", "-q", "-c", "fix"]);
        fs::write(root.join("b.txt"), "nuovo\n").unwrap();
        git(&root, &["add", "."]);
        git(&root, &["commit", "-q", "-m", "aggiungi b"]);
        git(&root, &["switch", "-q", "main"]);
        fs::write(root.join("a.txt"), "uno\ndue\n").unwrap();
        git(&root, &["commit", "-q", "-am", "due righe"]);
        git(&root, &["merge", "-q", "--no-edit", "fix"]);
        // A change not committed yet.
        fs::write(root.join("a.txt"), "uno\ndue\ntre\n").unwrap();

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
        wait_until(cx, "git's status", |cx| {
            view.read_with(cx, |view, _| view.scm.status.count() == 1)
        });
        view.update_in(cx, |view, window, cx| view.open_git_graph(window, cx));
        wait_until(cx, "the history", |cx| {
            view.read_with(cx, |view, _| {
                view.graph_tab().is_some_and(|tab| tab.commits.len() == 5)
            })
        });
        view.read_with(cx, |view, _| {
            let tab = view.graph_tab().unwrap();
            let subjects: Vec<&str> = tab
                .commits
                .iter()
                .map(|commit| commit.subject.as_str())
                .collect();
            assert_eq!(subjects[0], "Uncommitted Changes (1)");
            assert!(subjects[1].starts_with("Merge branch 'fix'"));
            assert_eq!(tab.lanes, 2);
            assert_eq!(view.panes[view.active].uri, GRAPH_URI);
        });

        // Search finds by message.
        view.update_in(cx, |view, window, cx| {
            let search = view.graph_tab().unwrap().search.clone();
            search.update(cx, |search, cx| search.set_value("aggiungi", window, cx));
            view.search_graph(cx);
        });
        let found = view.read_with(cx, |view, _| view.graph_tab().unwrap().matches.clone());
        assert_eq!(found.len(), 1);

        // Choosing the branch's commit shows its file; the file opens its diff.
        let hash = view.read_with(cx, |view, _| {
            view.graph_tab().unwrap().commits[found[0]].hash.clone()
        });
        view.update_in(cx, |view, window, cx| view.select_commit(&hash, window, cx));
        wait_until(cx, "the commit's files", |cx| {
            view.read_with(cx, |view, _| {
                view.commit_view
                    .as_ref()
                    .is_some_and(|commit| !commit.files.is_empty())
            })
        });
        view.update_in(cx, |view, window, cx| {
            view.open_diff(
                "b.txt",
                super::super::diff_view::DiffSource::Commit(hash.clone(), None),
                window,
                cx,
            )
        });
        view.read_with(cx, |view, _| {
            let uri = &view.panes[view.active].uri;
            assert!(uri.contains(&hash), "{uri}");
            assert_eq!(
                view.documents[uri].name(),
                format!("b.txt ({})", &hash[..8])
            );
        });

        // A tag on the commit shows as its label; deleting it from the
        // menu asks first.
        git(&root, &["tag", "v1", &hash]);
        view.update(cx, |view, cx| view.reload_git_graph(cx));
        wait_until(cx, "the tag", |cx| {
            view.read_with(cx, |view, _| {
                view.graph_tab()
                    .unwrap()
                    .commits
                    .iter()
                    .any(|commit| commit.refs.contains(&RefLabel::Tag("v1".into())))
            })
        });
        view.update_in(cx, |view, window, cx| {
            let action = super::super::commit_view::CommitMenu {
                hash: hash.clone(),
                command: super::super::commit_view::CommitCommand::DeleteTag("v1".into()),
            };
            view.run_commit_command(&action, window, cx);
        });
        assert!(cx.has_pending_prompt());
        cx.simulate_prompt_answer("Delete");
        wait_until(cx, "the tag to go", |cx| {
            view.read_with(cx, |view, _| {
                view.graph_tab()
                    .unwrap()
                    .commits
                    .iter()
                    .all(|commit| commit.refs.iter().all(|label| label.name() != "v1"))
            })
        });
        // Copying the hash needs no question.
        view.update_in(cx, |view, window, cx| {
            let action = super::super::commit_view::CommitMenu {
                hash: hash.clone(),
                command: super::super::commit_view::CommitCommand::CopyHash,
            };
            view.run_commit_command(&action, window, cx);
        });
        assert_eq!(
            cx.read_from_clipboard().and_then(|item| item.text()),
            Some(hash)
        );
    }
}
