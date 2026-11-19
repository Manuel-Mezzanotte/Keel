//! Git in the editor's margin: lines added in green, changed in blue and a
//! red mark where lines were removed, against the staged version as VS Code
//! does; and, with the cursor in a changed block, a small bar to see the
//! block's differences, stage it or undo it.

use gpui::{AnyElement, Div};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::diff::{DiffLinePosition, DiffSide};
use gpui_component::{Icon, Sizable as _};
use gpui_kit_assets::IconName;

use super::diff_view::DiffSource;
use super::shell::Colors;
use super::*;

actions!(
    keel,
    [
        StageChange,
        RevertChange,
        PreviewChange,
        NextChange,
        PreviousChange
    ]
);

/// The colors VS Code's themes give the margin.
const ADDED: u32 = 0x2ea043;
const MODIFIED: u32 = 0x1b81a8;
const DELETED: u32 = 0xf85149;

impl Workbench {
    /// The file of a pane as git names it, when it is in the repository.
    fn git_path_of(&self, uri: &str) -> Option<String> {
        let repo = self.scm.repo.as_ref()?;
        let document = self.documents.get(uri)?;
        let Backing::File(file) = &document.backing else {
            return None;
        };
        repo.relative(file.path())
    }

    /// The staged versions are read again, after git's status changed.
    pub(super) fn forget_git_baselines(&mut self, cx: &mut Context<Self>) {
        self.git_baselines.clear();
        self.schedule_git_gutter(cx);
    }

    /// Marks the changed blocks of the visible files shortly after the
    /// last change.
    pub(super) fn schedule_git_gutter(&mut self, cx: &mut Context<Self>) {
        let Some(repo) = self.scm.repo.clone() else {
            return;
        };
        let wanted: Vec<(String, String)> = self
            .panes
            .iter()
            .filter_map(|pane| Some((pane.uri.clone(), self.git_path_of(&pane.uri)?)))
            .collect();
        let missing: Vec<String> = wanted
            .iter()
            .map(|(_, path)| path.clone())
            .filter(|path| !self.git_baselines.contains_key(path))
            .collect();
        self.gutter_refresh = cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(120))
                .await;
            let fetched: Vec<(String, Option<String>)> = cx
                .background_spawn(async move {
                    missing
                        .into_iter()
                        .map(|path| {
                            let text = repo.index_text(&path);
                            (path, text)
                        })
                        .collect()
                })
                .await;
            this.update(cx, |this, cx| {
                this.git_baselines.extend(fetched);
                for (uri, path) in wanted {
                    let baseline = this.git_baselines.get(&path).cloned().flatten();
                    this.set_git_gutter(&uri, baseline.as_deref(), cx);
                }
                this.update_hunk_bar(cx);
            })
            .ok();
        });
    }

    fn set_git_gutter(&mut self, uri: &str, baseline: Option<&str>, cx: &mut Context<Self>) {
        let indexes: Vec<usize> = self
            .panes
            .iter()
            .enumerate()
            .filter(|(_, pane)| pane.uri == uri)
            .map(|(index, _)| index)
            .collect();
        for index in indexes {
            let editor = self.panes[index].editor.clone();
            let text = editor.read(cx).value().to_string();
            // A file git does not track has no marks, as in VS Code.
            let found = baseline
                .map(|baseline| git::hunks(baseline, &text))
                .unwrap_or_default();
            let line_count = text.split('\n').count();
            let decorations: Vec<GutterDecoration> = found
                .iter()
                .map(|hunk| {
                    let (lines, color) = match hunk.kind() {
                        git::HunkKind::Added => (hunk.new.clone(), ADDED),
                        git::HunkKind::Modified => (hunk.new.clone(), MODIFIED),
                        git::HunkKind::Deleted => {
                            let line = hunk.new.start.min(line_count.saturating_sub(1));
                            (line..line + 1, DELETED)
                        }
                    };
                    let start = position::offset_of(&text, Position::new(lines.start as u32, 0));
                    let last = lines.end.saturating_sub(1).max(lines.start);
                    let end = position::offset_of(&text, Position::new(last as u32, u32::MAX));
                    GutterDecoration::new(start..end.max(start)).with_color(gpui::rgb(color).into())
                })
                .collect();
            let pane = &mut self.panes[index];
            pane.hunks = found;
            match &pane.gutter {
                Some(gutter) => gutter.set(decorations, cx),
                None => {
                    pane.gutter = Some(editor.update(cx, |editor, cx| {
                        editor.create_gutter_decorations_collection(decorations, cx)
                    }));
                }
            }
        }
    }

    /// The changed block under the active editor's cursor.
    fn hunk_at_cursor(&self, cx: &App) -> Option<(usize, git::Hunk)> {
        let pane = self.panes.get(self.active)?;
        let line = pane.editor.read(cx).cursor_position().line as usize;
        let hunk = pane.hunks.iter().find(|hunk| hunk.contains_line(line))?;
        Some((self.active, hunk.clone()))
    }

    /// Shows or hides the block bar when the cursor enters or leaves a
    /// changed block, without drawing the window on every move.
    pub(super) fn update_hunk_bar(&mut self, cx: &mut Context<Self>) {
        let current = self
            .hunk_at_cursor(cx)
            .map(|(pane, hunk)| (pane, hunk.new.start, hunk.new.end));
        if current != self.hunk_bar {
            self.hunk_bar = current;
            cx.notify();
        }
    }

    fn stage_change(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some((pane, hunk)) = self.hunk_at_cursor(cx) else {
            return;
        };
        let uri = self.panes[pane].uri.clone();
        let Some(path) = self.git_path_of(&uri) else {
            return;
        };
        let Some(Some(baseline)) = self.git_baselines.get(&path).cloned() else {
            return;
        };
        let text = self.panes[pane].editor.read(cx).value().to_string();
        let staged = git::apply_hunk(&baseline, &text, &hunk);
        let label = self.strings().scm_stage;
        self.run_git(
            label,
            move |repo| repo.stage_text(&path, &staged),
            |this, _, cx| this.forget_git_baselines(cx),
            window,
            cx,
        );
    }

    fn revert_change(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some((pane, hunk)) = self.hunk_at_cursor(cx) else {
            return;
        };
        let uri = self.panes[pane].uri.clone();
        let Some(path) = self.git_path_of(&uri) else {
            return;
        };
        let Some(Some(baseline)) = self.git_baselines.get(&path).cloned() else {
            return;
        };
        let text = self.panes[pane].editor.read(cx).value().to_string();
        let reverted = git::revert_hunk(&baseline, &text, &hunk);
        let editor = self.panes[pane].editor.clone();
        tabs::replace_text(&editor, &reverted, window, cx);
        self.sync_document_from(&uri, None, window, cx);
    }

    fn preview_change(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some((pane, hunk)) = self.hunk_at_cursor(cx) else {
            return;
        };
        let Some(path) = self.git_path_of(&self.panes[pane].uri.clone()) else {
            return;
        };
        self.open_diff(&path, DiffSource::Changes, window, cx);
        let uri = format!("keel-diff:changes/{path}");
        if let Some(OpenDocument {
            backing: Backing::Diff(tab),
            ..
        }) = self.documents.get(&uri)
        {
            if let Some(doc) = &tab.doc {
                // A block that only took lines away starts where they were.
                doc.scroll_to_new_line(hunk.new.start);
            } else {
                let position =
                    DiffLinePosition::new(path.clone(), DiffSide::Modified, hunk.new.start + 1);
                tab.state
                    .update(cx, |state, cx| state.scroll_to_line(position, cx));
            }
        }
    }

    /// Alt+F5 and Shift+Alt+F5: the cursor goes to the next or previous
    /// changed block, round the end.
    fn go_to_change(&mut self, forward: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(pane) = self.panes.get(self.active) else {
            return;
        };
        if pane.hunks.is_empty() {
            return;
        }
        let line = pane.editor.read(cx).cursor_position().line as usize;
        let starts: Vec<usize> = pane.hunks.iter().map(|hunk| hunk.new.start).collect();
        let target = if forward {
            starts
                .iter()
                .find(|start| **start > line)
                .or(starts.first())
        } else {
            starts
                .iter()
                .rev()
                .find(|start| **start < line)
                .or(starts.last())
        };
        if let Some(&target) = target {
            self.go_to(target as u32 + 1, 1, window, cx);
            self.update_hunk_bar(cx);
        }
    }

    pub(super) fn git_gutter_actions(&self, root: Div, cx: &mut Context<Self>) -> Div {
        root.on_action(
            cx.listener(|this, _: &StageChange, window, cx| this.stage_change(window, cx)),
        )
        .on_action(cx.listener(|this, _: &RevertChange, window, cx| this.revert_change(window, cx)))
        .on_action(
            cx.listener(|this, _: &PreviewChange, window, cx| this.preview_change(window, cx)),
        )
        .on_action(
            cx.listener(|this, _: &NextChange, window, cx| this.go_to_change(true, window, cx)),
        )
        .on_action(
            cx.listener(|this, _: &PreviousChange, window, cx| {
                this.go_to_change(false, window, cx)
            }),
        )
    }

    /// The bar over an editor whose cursor is in a changed block.
    pub(super) fn render_hunk_bar(
        &self,
        pane: usize,
        colors: Colors,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let (bar_pane, _, _) = self.hunk_bar?;
        if bar_pane != pane || pane != self.active {
            return None;
        }
        let strings = self.strings();
        let button = |id: &'static str, icon: IconName, label: &'static str| {
            Button::new(id)
                .ghost()
                .xsmall()
                .icon(Icon::new(icon))
                .label(label)
        };
        Some(
            div()
                .absolute()
                .top_1()
                .right_4()
                .px_1()
                .py_0p5()
                .flex()
                .items_center()
                .gap_0p5()
                .rounded(px(8.))
                .border_1()
                .border_color(colors.border)
                .bg(cx.theme().popover)
                .shadow_md()
                .text_xs()
                .child(
                    div()
                        .px_1()
                        .text_color(colors.muted)
                        .child(strings.hunk_changed),
                )
                .child(
                    button("hunk-preview", IconName::Eye, strings.hunk_preview).on_click(
                        cx.listener(|this, _, window, cx| this.preview_change(window, cx)),
                    ),
                )
                .child(
                    button("hunk-stage", IconName::Plus, strings.hunk_stage)
                        .on_click(cx.listener(|this, _, window, cx| this.stage_change(window, cx))),
                )
                .child(
                    button("hunk-revert", IconName::Undo2, strings.hunk_revert).on_click(
                        cx.listener(|this, _, window, cx| this.revert_change(window, cx)),
                    ),
                )
                .into_any_element(),
        )
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::process::Command;
    use std::time::Duration;

    use gpui::TestAppContext;

    use super::*;

    fn git(folder: &Path, args: &[&str]) -> String {
        let output = Command::new("git")
            .arg("-C")
            .arg(folder)
            .args(args)
            .output()
            .unwrap();
        String::from_utf8_lossy(&output.stdout).into_owned()
    }

    #[gpui::test]
    fn the_margin_marks_blocks_that_can_be_staged_and_undone(cx: &mut TestAppContext) {
        cx.executor().allow_parking();
        cx.update(|cx| {
            gpui_component::init(cx);
            language::register_grammars();
            crate::workbench_window::init(cx);
        });
        let folder = tempfile::tempdir().unwrap();
        let root = fs::canonicalize(folder.path()).unwrap();
        git(&root, &["init", "-q", "-b", "main"]);
        git(&root, &["config", "user.name", "Test"]);
        git(&root, &["config", "user.email", "test@example.com"]);
        git(&root, &["config", "commit.gpgsign", "false"]);
        fs::write(root.join("a.txt"), "uno\ndue\ntre\nquattro\n").unwrap();
        git(&root, &["add", "."]);
        git(&root, &["commit", "-q", "-m", "first"]);
        // Added first line, changed "due", removed "quattro".
        fs::write(root.join("a.txt"), "zero\nuno\nDUE\ntre\n").unwrap();

        let settings = Settings::load(root.join(".git/keel-test-settings.json"), None);
        let opening = Opening::new(Some(root.clone()), Some(root.join("a.txt")));
        let (view, cx) =
            cx.add_window_view(|window, cx| Workbench::new(opening, settings, None, window, cx));
        let settle = |cx: &mut gpui::VisualTestContext| {
            for _ in 0..30 {
                cx.executor().advance_clock(Duration::from_millis(400));
                cx.run_until_parked();
                std::thread::sleep(Duration::from_millis(10));
            }
        };
        settle(cx);
        let kinds = view.read_with(cx, |view, _| {
            view.panes[0]
                .hunks
                .iter()
                .map(git::Hunk::kind)
                .collect::<Vec<_>>()
        });
        assert_eq!(
            kinds,
            [
                git::HunkKind::Added,
                git::HunkKind::Modified,
                git::HunkKind::Deleted
            ]
        );

        // Alt+F5 goes to the next block; the bar appears there.
        view.update_in(cx, |view, window, cx| {
            view.go_to(1, 1, window, cx);
            view.go_to_change(true, window, cx);
        });
        let bar = view.read_with(cx, |view, _| view.hunk_bar);
        assert_eq!(bar, Some((0, 2, 3)));

        // Staging that block changes the index and leaves the file alone.
        view.update_in(cx, |view, window, cx| view.stage_change(window, cx));
        settle(cx);
        assert_eq!(git(&root, &["show", ":a.txt"]), "uno\nDUE\ntre\nquattro\n");

        // Undoing the added line takes it out of the editor.
        view.update_in(cx, |view, window, cx| {
            view.go_to(1, 1, window, cx);
            view.update_hunk_bar(cx);
            view.revert_change(window, cx);
        });
        let text = view.read_with(cx, |view, cx| {
            view.panes[0].editor.read(cx).value().to_string()
        });
        assert_eq!(text, "uno\nDUE\ntre\n");
    }
}
