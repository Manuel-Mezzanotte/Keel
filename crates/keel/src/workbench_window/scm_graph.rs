//! The Graph section under Source Control's changes, as VS Code's: the
//! latest commits with their lanes, branches and tags, in the sidebar. A
//! click opens the commit in Git Graph; the section closes to its header.

use std::ops::Range;

use git::graph::GraphRow;
use git::{Commit, LogOptions, RefLabel};
use gpui::{AnyElement, StatefulInteractiveElement as _, Task, canvas, uniform_list};
use gpui_component::button::ButtonVariants as _;
use gpui_component::{ActiveTheme as _, Sizable as _};

use super::git_graph::{PALETTE, format_date, lane_color, paint_graph_row};
use super::shell::{Colors, own_clicks, section_header};
use super::*;

/// How many commits the section shows; Git Graph has them all.
const SIDEBAR_COMMITS: usize = 150;
const ROW_HEIGHT: f32 = 24.;
/// Narrower lanes than Git Graph's, for the sidebar.
const LANE_WIDTH: f32 = 10.;
/// Lanes past these are cut, to leave room for the messages.
const MOST_LANES: usize = 5;

/// How long ago, as Apple's Mail lists dates: `ora`, `5 min`, `3 h`,
/// `2 g`, `4 sett`, `3 mesi`, `2 anni`.
pub(super) fn relative_time(seconds: i64, now: i64, italian: bool) -> String {
    let elapsed = (now - seconds).max(0);
    let (minute, hour, day) = (60, 3600, 86_400);
    let pick = |it: &str, en: &str| {
        if italian {
            it.to_owned()
        } else {
            en.to_owned()
        }
    };
    match elapsed {
        e if e < minute => pick("ora", "now"),
        e if e < hour => format!("{} min", e / minute),
        e if e < day => format!("{} h", e / hour),
        e if e < 7 * day => format!("{} {}", e / day, pick("g", "d")),
        e if e < 30 * day => format!("{} {}", e / (7 * day), pick("sett", "w")),
        e if e < 365 * day => {
            let months = e / (30 * day);
            let unit = match (italian, months) {
                (true, 1) => "mese",
                (true, _) => "mesi",
                (false, _) => "mo",
            };
            format!("{months} {unit}")
        }
        e => {
            let years = e / (365 * day);
            let unit = match (italian, years) {
                (true, 1) => "anno",
                (true, _) => "anni",
                (false, _) => "y",
            };
            format!("{years} {unit}")
        }
    }
}

#[derive(Default)]
pub(super) struct SidebarGraph {
    commits: Vec<Commit>,
    rows: Vec<GraphRow>,
    lanes: usize,
    _task: Option<Task<()>>,
}

impl Workbench {
    /// Reads the latest commits again, away from the interface.
    pub(super) fn reload_sidebar_graph(&mut self, cx: &mut Context<Self>) {
        let Some(repo) = self.scm.repo.clone() else {
            self.scm_graph = SidebarGraph::default();
            return;
        };
        let options = LogOptions {
            branches: Vec::new(),
            max: SIDEBAR_COMMITS,
        };
        let work = cx.background_spawn(async move {
            let commits = repo.log(&options)?;
            let rows = git::graph::layout(&commits, PALETTE.len());
            Ok::<_, git::GitError>((commits, rows))
        });
        self.scm_graph._task = Some(cx.spawn(async move |this, cx| {
            let Ok((commits, rows)) = work.await else {
                return;
            };
            this.update(cx, |this, cx| {
                this.scm_graph.lanes = rows.iter().map(|row| row.width).max().unwrap_or(1).max(1);
                this.scm_graph.commits = commits;
                this.scm_graph.rows = rows;
                cx.notify();
            })
            .ok();
        }));
    }

    /// The section: its header, and the commits when it is open.
    pub(super) fn render_scm_graph(&self, colors: Colors, cx: &mut Context<Self>) -> AnyElement {
        let strings = self.strings();
        let open = !self.graph_collapsed;
        let header = section_header("scm-graph-header", strings.scm_graph, open, colors)
            .on_click(cx.listener(|this, _, _, cx| {
                this.graph_collapsed = !this.graph_collapsed;
                this.session_changed(cx);
                cx.notify();
            }))
            .child(
                own_clicks("scm-graph-actions").child(
                    gpui_component::button::Button::new("scm-open-graph")
                        .ghost()
                        .xsmall()
                        .icon(gpui_component::Icon::new(
                            gpui_kit_assets::IconName::GitGraph,
                        ))
                        .tooltip(strings.command_git_graph)
                        .on_click(
                            cx.listener(|this, _, window, cx| this.open_git_graph(window, cx)),
                        ),
                ),
            );
        let count = self.scm_graph.commits.len();
        div()
            .flex()
            .flex_col()
            .border_t_1()
            .border_color(colors.border)
            .pt_1()
            .when(open, |section| section.flex_1().min_h(px(120.)))
            .when(!open, |section| section.flex_none())
            .child(header)
            .when(open, |section| {
                section.child(
                    uniform_list(
                        "scm-graph-rows",
                        count,
                        cx.processor(|this, range: Range<usize>, _, cx| {
                            let colors = Colors::new(cx);
                            range
                                .map(|index| this.render_scm_graph_row(index, colors, cx))
                                .collect::<Vec<_>>()
                        }),
                    )
                    .px_1()
                    .flex_1()
                    .min_h_0(),
                )
            })
            .into_any_element()
    }

    fn render_scm_graph_row(
        &self,
        index: usize,
        colors: Colors,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(commit) = self.scm_graph.commits.get(index) else {
            return div().into_any_element();
        };
        let row = self.scm_graph.rows.get(index).cloned();
        let lane = row.as_ref().map_or(0, |row| row.color);
        let head = commit.refs.iter().find_map(|label| match label {
            RefLabel::Head(name) => Some(name.clone()),
            RefLabel::DetachedHead => Some("HEAD".to_owned()),
            _ => None,
        });
        let is_head = head.is_some();
        let background = cx.theme().background;
        let lanes_width = self.scm_graph.lanes.min(MOST_LANES) as f32 * LANE_WIDTH + 8.;
        let graph = div()
            .w(px(lanes_width))
            .h_full()
            .flex_none()
            .overflow_hidden()
            .child(
                canvas(
                    |_, _, _| {},
                    move |bounds, _, window, _| {
                        if let Some(row) = &row {
                            paint_graph_row(
                                bounds, row, is_head, false, background, LANE_WIDTH, window,
                            );
                        }
                    },
                )
                .size_full(),
            );
        let italian = self.language == UiLanguage::Italian;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |elapsed| elapsed.as_secs() as i64);
        let when = relative_time(commit.time, now, italian);
        let details = format!(
            "{}\n{} · {} · {}",
            commit.subject,
            commit.author,
            format_date(commit.time, italian),
            commit.short_hash()
        );
        // Only the branch you are on gets its name, as a small capsule.
        let pill = head.map(|name| {
            let color = lane_color(lane);
            div()
                .flex_none()
                .max_w(px(110.))
                .h(px(16.))
                .px_1p5()
                .flex()
                .items_center()
                .rounded_full()
                .bg(color.opacity(0.16))
                .text_size(px(10.5))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(color)
                .child(div().min_w_0().truncate().child(name))
        });
        let hash = commit.hash.clone();
        div()
            .id(("scm-graph-row", index))
            .h(px(ROW_HEIGHT))
            .w_full()
            .pr_1p5()
            .flex()
            .items_center()
            .gap_1p5()
            .rounded(px(6.))
            .text_sm()
            .cursor_pointer()
            .hover(|row| row.bg(colors.hover))
            .tooltip(move |window, cx| {
                gpui_component::tooltip::Tooltip::new(details.clone()).build(window, cx)
            })
            .on_click(cx.listener(move |this, _, window, cx| {
                this.open_git_graph(window, cx);
                this.select_commit(&hash, window, cx);
            }))
            .child(graph)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .items_center()
                    .gap_1()
                    .children(pill)
                    .child(
                        div()
                            .min_w_0()
                            .truncate()
                            .text_color(if is_head { colors.text } else { colors.muted })
                            .child(commit.subject.clone()),
                    ),
            )
            .child(
                div()
                    .debug_selector(|| format!("scm-graph-when-{index}"))
                    .flex_none()
                    .text_xs()
                    .text_color(colors.faint)
                    .child(when),
            )
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;
    use std::process::Command;
    use std::time::Duration;

    use gpui::TestAppContext;

    use super::*;

    #[test]
    fn dates_as_mail_lists_them() {
        let now = 1_800_000_000;
        assert_eq!(relative_time(now - 20, now, true), "ora");
        assert_eq!(relative_time(now - 5 * 60, now, true), "5 min");
        assert_eq!(relative_time(now - 3 * 3600, now, true), "3 h");
        assert_eq!(relative_time(now - 2 * 86_400, now, true), "2 g");
        assert_eq!(relative_time(now - 15 * 86_400, now, true), "2 sett");
        assert_eq!(relative_time(now - 40 * 86_400, now, true), "1 mese");
        assert_eq!(relative_time(now - 800 * 86_400, now, true), "2 anni");
        assert_eq!(relative_time(now - 2 * 86_400, now, false), "2 d");
        // A clock behind the commit is not "in the future".
        assert_eq!(relative_time(now + 100, now, true), "ora");
    }

    fn remember_search_for_test(view: &mut Workbench) {
        super::super::search_view::remember_search(&mut view.search.recent, "booking");
    }

    fn git(folder: &Path, args: &[&str]) {
        let output = Command::new("git")
            .arg("-C")
            .arg(folder)
            .args(args)
            .output()
            .unwrap();
        assert!(output.status.success(), "git {args:?}");
    }

    #[gpui::test]
    fn the_graph_section_shows_the_latest_commits(cx: &mut TestAppContext) {
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
        for (file, message) in [("a.txt", "primo"), ("b.txt", "secondo")] {
            fs::write(root.join(file), "x\n").unwrap();
            git(&root, &["add", "."]);
            git(&root, &["commit", "-q", "-m", message]);
        }
        let settings = Settings::load("/nonexistent/keel-test/settings.json", None);
        let opening = Opening::new(Some(root.clone()), None);
        let (view, cx) =
            cx.add_window_view(|window, cx| Workbench::new(opening, settings, None, window, cx));
        let mut subjects = Vec::new();
        for _ in 0..200 {
            cx.executor().advance_clock(Duration::from_millis(500));
            cx.run_until_parked();
            subjects = view.read_with(cx, |view, _| {
                view.scm_graph
                    .commits
                    .iter()
                    .map(|commit| commit.subject.clone())
                    .collect::<Vec<_>>()
            });
            if !subjects.is_empty() {
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        assert_eq!(subjects, ["secondo", "primo"]);
        // Both views draw with their new parts.
        for view_kind in [
            crate::layout::SideView::SourceControl,
            crate::layout::SideView::Search,
        ] {
            view.update(cx, |view, cx| {
                view.layout.select_side_view(view_kind);
                remember_search_for_test(view);
                cx.notify();
            });
            cx.update(|window, _| window.refresh());
            cx.run_until_parked();
        }
        // Closed to its header, and remembered so.
        view.update(cx, |view, _| {
            view.graph_collapsed = true;
            view.npm_collapsed = true;
            view.sidebar_width = 320.;
        });
        let state = view.read_with(cx, |view, _| view.window_state());
        assert_eq!(state.collapsed, ["npm-scripts", "git-graph"]);
        assert_eq!(state.sidebar_width, Some(320.));
    }
}
