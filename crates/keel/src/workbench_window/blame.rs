//! Who last changed each line, in a column at the left of the editor that
//! can be turned on and off, and the history of a file to pick a version
//! from. The editor cannot draw text inside its lines, so the authors sit
//! beside it, one label where each commit's lines begin, aligned with the
//! editor's rows as it scrolls.

use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;

use git::BlameLine;
use gpui::{
    AnyElement, Bounds, MouseButton, MouseDownEvent, Pixels, SharedString, TextRun, canvas, point,
};
use gpui_component::{ActiveTheme as _, RopeExt as _};

use super::diff_view::DiffSource;
use super::git_graph::format_date;
use super::*;
use crate::quick_pick::{self, PickItem};

actions!(keel, [ToggleBlame, ShowFileHistory]);

/// The history of one file, from the Explorer's menu.
#[derive(Clone, Debug, PartialEq, gpui::Action)]
#[action(namespace = explorer, no_json)]
pub struct ShowFileHistoryOf {
    pub path: PathBuf,
}

const BLAME_WIDTH: f32 = 210.;
/// How long blame waits after the last keystroke before asking git again.
const BLAME_PAUSE: Duration = Duration::from_millis(500);

/// Where each label of the column was drawn, with its commit, to know
/// which one a click hits.
type Placed = Rc<std::cell::RefCell<Vec<(Bounds<Pixels>, String)>>>;

/// The authors of a file's lines.
#[derive(Default)]
pub(super) struct BlameState {
    pub(super) lines: Vec<BlameLine>,
    task: Option<Task<()>>,
}

impl Workbench {
    /// The file of the active tab, as git names it.
    fn active_git_path(&self) -> Option<(String, String)> {
        let repo = self.scm.repo.as_ref()?;
        let pane = self.panes.get(self.active)?;
        let path = self.documents.get(&pane.uri)?.disk_path()?;
        Some((pane.uri.clone(), repo.relative(path)?))
    }

    /// Turns the authors' column on or off for the active file.
    fn toggle_blame(&mut self, cx: &mut Context<Self>) {
        let Some((uri, _)) = self.active_git_path() else {
            self.status_message = Some(self.strings().blame_not_in_git.to_owned());
            cx.notify();
            return;
        };
        if self.blame.remove(&uri).is_none() {
            self.blame.insert(uri.clone(), BlameState::default());
            self.refresh_blame(&uri, false, cx);
        }
        cx.notify();
    }

    /// Asks git again who wrote each line of `uri`, unsaved text included;
    /// after a pause when the text is being typed.
    pub(super) fn refresh_blame(&mut self, uri: &str, pause: bool, cx: &mut Context<Self>) {
        if !self.blame.contains_key(uri) {
            return;
        }
        let (Some(repo), Some(document)) = (self.scm.repo.clone(), self.documents.get(uri)) else {
            return;
        };
        let Some(path) = document.disk_path().and_then(|path| repo.relative(path)) else {
            return;
        };
        let text = document.synced_text.clone();
        let uri_owned = uri.to_owned();
        let task = cx.spawn(async move |this, cx| {
            if pause {
                cx.background_executor().timer(BLAME_PAUSE).await;
            }
            let lines = cx
                .background_spawn(async move { repo.blame(&path, Some(&text)).unwrap_or_default() })
                .await;
            this.update(cx, |this, cx| {
                if let Some(state) = this.blame.get_mut(&uri_owned) {
                    state.lines = lines;
                    cx.notify();
                }
            })
            .ok();
        });
        if let Some(state) = self.blame.get_mut(uri) {
            state.task = Some(task);
        }
    }

    /// The author, date and message of the line under the cursor, for the
    /// status bar while the column is on.
    pub(super) fn current_line_blame(&self, cx: &App) -> Option<String> {
        let pane = self.panes.get(self.active)?;
        let state = self.blame.get(&pane.uri)?;
        let line = pane.editor.read(cx).cursor_position().line as usize;
        let blame = state.lines.get(line)?;
        if blame.is_uncommitted() {
            return Some(self.strings().blame_uncommitted.to_owned());
        }
        Some(format!(
            "{}, {} · {}",
            blame.author,
            format_date(blame.time, self.language == UiLanguage::Italian),
            blame.summary
        ))
    }

    /// The column of authors beside pane `index`'s editor, when it is on.
    pub(super) fn render_blame_column(
        &self,
        index: usize,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let pane = self.panes.get(index)?;
        let state = self.blame.get(&pane.uri)?;
        let editor = pane.editor.read(cx);
        let rows = editor.visible_row_range().unwrap_or(0..0);
        let italian = self.language == UiLanguage::Italian;
        let uncommitted = self.strings().blame_uncommitted;
        let theme = cx.theme();
        let (muted, border) = (theme.muted_foreground, theme.border);
        // One label where each commit's lines begin, and at the top.
        let mut labels: Vec<(Pixels, Pixels, String, String, bool)> = Vec::new();
        for row in rows.clone() {
            let Some(line) = state.lines.get(row) else {
                break;
            };
            let starts = row == rows.start
                || state
                    .lines
                    .get(row.wrapping_sub(1))
                    .is_none_or(|before| before.hash != line.hash);
            if !starts {
                continue;
            }
            let offset = editor.text().line_start_offset(row);
            let Some(bounds) = editor.range_to_bounds(&(offset..offset)) else {
                continue;
            };
            let text = if line.is_uncommitted() {
                uncommitted.to_owned()
            } else {
                format!(
                    "{} · {}",
                    format_date(line.time, italian)
                        .split(' ')
                        .take(3)
                        .collect::<Vec<_>>()
                        .join(" "),
                    line.author
                )
            };
            labels.push((
                bounds.origin.y,
                bounds.size.height,
                text,
                line.hash.clone(),
                row != rows.start,
            ));
        }
        let placed: Placed = Rc::default();
        let painted = placed.clone();
        let column = canvas(
            |_, _, _| {},
            move |bounds, _, window, cx| {
                let font = window.text_style().font();
                let size = px(11.5);
                let mut spots = Vec::new();
                for (y, height, text, hash, separator) in &labels {
                    if *separator {
                        window.paint_quad(gpui::fill(
                            Bounds::new(
                                point(bounds.origin.x, *y),
                                gpui::size(bounds.size.width, px(1.)),
                            ),
                            border,
                        ));
                    }
                    let run = TextRun {
                        len: text.len(),
                        font: font.clone(),
                        color: muted,
                        background_color: None,
                        underline: None,
                        strikethrough: None,
                    };
                    let line = window.text_system().shape_line(
                        SharedString::from(text.clone()),
                        size,
                        &[run],
                        None,
                    );
                    let origin = point(bounds.origin.x + px(8.), *y + (*height - size * 1.3) / 2.);
                    let _ = line.paint(origin, size * 1.3, gpui::TextAlign::Left, None, window, cx);
                    spots.push((
                        Bounds::new(
                            point(bounds.origin.x, *y),
                            gpui::size(bounds.size.width, *height),
                        ),
                        hash.clone(),
                    ));
                }
                *painted.borrow_mut() = spots;
            },
        )
        .size_full();
        Some(
            div()
                .w(px(BLAME_WIDTH))
                .h_full()
                .flex_none()
                .border_r_1()
                .border_color(border)
                .cursor_pointer()
                // A click on a label shows its commit in Git Graph.
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                        let hash = placed
                            .borrow()
                            .iter()
                            .rev()
                            .find(|(bounds, _)| event.position.y >= bounds.origin.y)
                            .map(|(_, hash)| hash.clone());
                        if let Some(hash) =
                            hash.filter(|hash| !hash.bytes().all(|byte| byte == b'0'))
                        {
                            this.open_git_graph(window, cx);
                            this.select_commit(&hash, window, cx);
                        }
                    }),
                )
                .child(column)
                .into_any_element(),
        )
    }

    /// Lists the commits that changed a file; the one chosen opens its
    /// changes to the file.
    fn show_file_history(
        &mut self,
        path: Option<PathBuf>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(repo) = self.scm.repo.clone() else {
            return;
        };
        let relative = match path {
            Some(path) => repo.relative(&path),
            None => self.active_git_path().map(|(_, relative)| relative),
        };
        let Some(relative) = relative else {
            self.status_message = Some(self.strings().blame_not_in_git.to_owned());
            cx.notify();
            return;
        };
        let italian = self.language == UiLanguage::Italian;
        let title = self
            .strings()
            .file_history_title(relative.rsplit('/').next().unwrap_or(&relative));
        let wanted = relative.clone();
        let task =
            cx.background_spawn(async move { repo.file_log(&wanted, 500).unwrap_or_default() });
        cx.spawn_in(window, async move |this, cx| {
            let commits = task.await;
            this.update_in(cx, |_, window, cx| {
                let items: Vec<PickItem> = commits
                    .iter()
                    .map(|commit| {
                        let mut item = PickItem::new(commit.subject.clone());
                        item.detail = Some(format!(
                            "{} · {} · {}",
                            commit.short_hash(),
                            commit.author,
                            format_date(commit.time, italian)
                        ));
                        item.keywords = vec![commit.hash.clone(), commit.author.clone()];
                        item
                    })
                    .collect();
                let chosen = quick_pick::show(Some(title), None, items, window, cx);
                cx.spawn_in(window, async move |this, cx| {
                    let Some(index) = chosen.await.ok().flatten() else {
                        return;
                    };
                    let Some(commit) = commits.get(index) else {
                        return;
                    };
                    let hash = commit.hash.clone();
                    this.update_in(cx, |this, window, cx| {
                        this.open_diff(&relative, DiffSource::Commit(hash, None), window, cx)
                    })
                    .ok();
                })
                .detach();
            })
            .ok();
        })
        .detach();
    }

    pub(super) fn blame_actions(&self, root: gpui::Div, cx: &mut Context<Self>) -> gpui::Div {
        root.on_action(cx.listener(|this, _: &ToggleBlame, _, cx| this.toggle_blame(cx)))
            .on_action(cx.listener(|this, _: &ShowFileHistory, window, cx| {
                this.show_file_history(None, window, cx)
            }))
            .on_action(cx.listener(|this, action: &ShowFileHistoryOf, window, cx| {
                this.show_file_history(Some(action.path.clone()), window, cx)
            }))
    }
}

/// Keeps the authors of the files whose column is on.
pub(super) type BlameMap = HashMap<String, BlameState>;

#[cfg(test)]
mod tests {
    use std::fs;
    use std::process::Command;
    use std::time::Instant;

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
            cx.executor().advance_clock(Duration::from_millis(600));
            cx.run_until_parked();
            if check(cx) {
                return;
            }
            assert!(Instant::now() < deadline, "gave up waiting for {what}");
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    #[gpui::test]
    fn the_authors_column_follows_the_text(cx: &mut TestAppContext) {
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
        fs::write(root.join("a.txt"), "uno\ndue\n").unwrap();
        git(&root, &["add", "."]);
        git(&root, &["commit", "-q", "-m", "le prime righe"]);
        let config = tempfile::tempdir().unwrap();
        let settings_file = config.path().join("settings.json");
        fs::write(&settings_file, r#"{ "keel.language": "it" }"#).unwrap();
        let settings = Settings::load(&settings_file, None);
        let opening = Opening::new(Some(root.clone()), None);
        let (view, cx) =
            cx.add_window_view(|window, cx| Workbench::new(opening, settings, None, window, cx));
        let path = root.join("a.txt");
        view.update_in(cx, |view, window, cx| view.open_file(&path, window, cx));
        wait_until(cx, "the repository", |cx| {
            view.read_with(cx, |view, _| view.scm.repo.is_some())
        });
        view.update(cx, |view, cx| view.toggle_blame(cx));
        wait_until(cx, "the authors", |cx| {
            view.read_with(cx, |view, _| {
                view.blame
                    .values()
                    .next()
                    .is_some_and(|state| state.lines.len() == 2)
            })
        });
        let status = view
            .read_with(cx, |view, cx| view.current_line_blame(cx))
            .unwrap();
        assert!(
            status.starts_with("Manuel, ") && status.ends_with("· le prime righe"),
            "{status}"
        );
        cx.update(|window, cx| {
            let _ = window.draw(cx);
        });
        view.update(cx, |view, cx| {
            assert!(view.render_blame_column(view.active, cx).is_some())
        });

        // A new line, not saved: blamed as not committed yet.
        view.update_in(cx, |view, window, cx| {
            let editor = view.active_editor().unwrap();
            editor.update(cx, |editor, cx| {
                editor.set_value("zero\nuno\ndue\n", window, cx)
            });
            let uri = view.panes[view.active].uri.clone();
            view.sync_document_from(&uri, None, window, cx);
        });
        wait_until(cx, "the new line", |cx| {
            view.read_with(cx, |view, _| {
                view.blame
                    .values()
                    .next()
                    .is_some_and(|state| state.lines.len() == 3 && state.lines[0].is_uncommitted())
            })
        });
        // The cursor is at the start, on the new line.
        view.update_in(cx, |view, window, cx| {
            let editor = view.active_editor().unwrap();
            editor.update(cx, |editor, cx| {
                editor.set_cursor_position(gpui_component::input::Position::new(0, 0), window, cx)
            });
        });
        assert_eq!(
            view.read_with(cx, |view, cx| view.current_line_blame(cx))
                .as_deref(),
            Some("Non ancora committato")
        );

        // Off again.
        view.update(cx, |view, cx| view.toggle_blame(cx));
        view.read_with(cx, |view, _| assert!(view.blame.is_empty()));
    }
}
