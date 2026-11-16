//! Differences in a tab: a file's changes, staged or not, side by side or
//! in one column, drawn by gpui-component's diff view; and files in merge
//! conflict, resolved one conflict at a time.

use gpui::{AnyElement, Entity};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::diff::{Diff, DiffFile, DiffMode, DiffState};
use gpui_component::{Disableable as _, Icon, Selectable as _, Sizable as _};
use gpui_kit_assets::IconName;

use super::shell::{Colors, note};
use super::*;

/// Which differences of a file a tab shows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum DiffSource {
    /// Not staged yet: the staged version against the file on disk.
    Changes,
    /// Staged: the last commit against the staged version.
    Staged,
    /// A file in merge conflict, with its conflicts to resolve.
    Conflict,
    /// What a commit changed in the file, from Git Graph: the commit's
    /// hash, and where a renamed file came from.
    Commit(String, Option<String>),
    /// An edit Claude Code proposes, by the name of its tab; the patch is
    /// made from the file and the proposed text.
    Proposed(String),
}

/// A tab with differences.
pub(super) struct DiffTab {
    /// The file, from the repository's top folder.
    pub(super) path: String,
    pub(super) source: DiffSource,
    pub(super) state: Entity<DiffState>,
    pub(super) title: String,
}

/// The uri of a differences tab, distinct for each file and source.
pub(super) fn diff_uri(path: &str, source: &DiffSource) -> String {
    let kind = match source {
        DiffSource::Changes => "changes",
        DiffSource::Staged => "staged",
        DiffSource::Conflict => "conflict",
        DiffSource::Commit(hash, _) => hash.as_str(),
        DiffSource::Proposed(name) => return format!("keel-claude-diff:{name}"),
    };
    format!("keel-diff:{kind}/{path}")
}

impl Workbench {
    /// What a differences tab shows: the patch git makes, or the conflicts
    /// read from the file.
    fn diff_files(&self, path: &str, source: &DiffSource) -> Result<Vec<DiffFile>, String> {
        let repo = self.scm.repo.as_ref().ok_or_else(String::new)?;
        let language = Language::for_path(Path::new(path)).highlighter_name();
        let files = match source {
            DiffSource::Conflict => {
                let text = std::fs::read_to_string(repo.absolute(path))
                    .map_err(|error| error.to_string())?;
                vec![
                    DiffFile::parse_conflicts(path.to_owned(), &text)
                        .map_err(|error| error.message().to_string())?,
                ]
            }
            DiffSource::Commit(hash, original) => {
                let patch = repo
                    .commit_file_diff(hash, path, original.as_deref())
                    .map_err(|error| error.to_string())?;
                DiffFile::parse(&patch).map_err(|error| error.message().to_string())?
            }
            DiffSource::Proposed(_) => return Err("not a git difference".to_owned()),
            DiffSource::Changes | DiffSource::Staged => {
                let patch = repo
                    .diff(path, *source == DiffSource::Staged)
                    .map_err(|error| error.to_string())?;
                DiffFile::parse(&patch).map_err(|error| error.message().to_string())?
            }
        };
        Ok(files
            .into_iter()
            .map(|file| file.with_language(language))
            .collect())
    }

    /// Opens a file's differences in a preview tab, or brings it forward.
    pub(super) fn open_diff(
        &mut self,
        path: &str,
        source: DiffSource,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let uri = diff_uri(path, &source);
        if !self.documents.contains_key(&uri) {
            let files = match self.diff_files(path, &source) {
                Ok(files) => files,
                Err(message) => {
                    self.report(format!("{path}: {message}"));
                    cx.notify();
                    return;
                }
            };
            let mode = if self.diff_split {
                DiffMode::Split
            } else {
                DiffMode::Unified
            };
            let state = cx.new(|cx| DiffState::new(files, cx).with_mode(mode));
            // A resolved conflict changes what Save can do.
            cx.observe(&state, |_, _, cx| cx.notify()).detach();
            let strings = self.strings();
            let name = path.rsplit('/').next().unwrap_or(path);
            let title = match &source {
                DiffSource::Changes => format!("{name} ({})", strings.diff_working_tree),
                DiffSource::Staged => format!("{name} ({})", strings.diff_index),
                DiffSource::Conflict => format!("{name} ({})", strings.diff_conflict),
                DiffSource::Commit(hash, _) => format!("{name} ({})", &hash[..hash.len().min(8)]),
                DiffSource::Proposed(tab_name) => tab_name.clone(),
            };
            self.documents.insert(
                uri.clone(),
                OpenDocument {
                    uri: uri.clone(),
                    backing: Backing::Diff(DiffTab {
                        path: path.to_owned(),
                        source,
                        state,
                        title,
                    }),
                    language_id: "diff".to_owned(),
                    version: 1,
                    synced_text: String::new(),
                    modified: false,
                },
            );
        }
        let column = self.active.min(self.panes.len().saturating_sub(1));
        self.show_tab(&uri, column, OpenAs::Preview, true, window, cx);
    }

    /// Reads the open differences again after git's status changed.
    pub(super) fn refresh_open_diffs(&mut self, cx: &mut Context<Self>) {
        let tabs: Vec<(String, DiffSource, Entity<DiffState>)> = self
            .documents
            .values()
            .filter_map(|document| match &document.backing {
                // Conflicts keep the choices made so far until saved.
                // Conflicts keep their choices; a commit never changes.
                Backing::Diff(tab)
                    if !matches!(
                        tab.source,
                        DiffSource::Conflict | DiffSource::Commit(..) | DiffSource::Proposed(_)
                    ) =>
                {
                    Some((tab.path.clone(), tab.source.clone(), tab.state.clone()))
                }
                _ => None,
            })
            .collect();
        for (path, source, state) in tabs {
            if let Ok(files) = self.diff_files(&path, &source) {
                state.update(cx, |state, cx| state.set_files(files, cx));
            }
        }
    }

    pub(super) fn set_diff_split(&mut self, split: bool, cx: &mut Context<Self>) {
        self.diff_split = split;
        let mode = if split {
            DiffMode::Split
        } else {
            DiffMode::Unified
        };
        let states: Vec<Entity<DiffState>> = self
            .documents
            .values()
            .filter_map(|document| match &document.backing {
                Backing::Diff(tab) => Some(tab.state.clone()),
                _ => None,
            })
            .collect();
        for state in states {
            state.update(cx, |state, cx| state.set_mode(mode, cx));
        }
        cx.notify();
    }

    /// Writes a resolved conflict to the file and stages it.
    fn save_resolution(
        &mut self,
        path: String,
        state: Entity<DiffState>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(repo) = self.scm.repo.clone() else {
            return;
        };
        let Some(text) = state.read(cx).resolved_text(&path) else {
            return;
        };
        let file = repo.absolute(&path);
        if let Err(error) = std::fs::write(&file, text) {
            self.report(format!("{path}: {error}"));
            cx.notify();
            return;
        }
        let uri = diff_uri(&path, &DiffSource::Conflict);
        let label = self.strings().scm_stage;
        self.run_git(
            label,
            move |repo| repo.stage(&[path.as_str()]),
            move |this, window, cx| {
                // The conflict is gone: its tab gives way to the file.
                if let Some((group, tab)) = this.find_tab(&uri, this.active) {
                    this.close_tab_now(group, tab, window, cx);
                }
                this.open_file(&file, window, cx);
            },
            window,
            cx,
        );
    }

    pub(super) fn render_diff(
        &self,
        tab: &DiffTab,
        colors: Colors,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if let DiffSource::Proposed(name) = &tab.source {
            return self.render_claude_diff(name, tab, colors, cx);
        }
        let strings = self.strings();
        let path = tab.path.clone();
        let source = tab.source.clone();
        let empty = tab.state.read(cx).files().is_empty();
        let mut toolbar = div()
            .h(px(34.))
            .flex_none()
            .px_3()
            .flex()
            .items_center()
            .gap_1()
            .border_b_1()
            .border_color(colors.border)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_xs()
                    .text_color(colors.muted)
                    .child(tab.path.clone()),
            );
        if source == DiffSource::Conflict {
            let resolved = tab.state.read(cx).resolved_text(&tab.path).is_some();
            let state = tab.state.clone();
            let save_path = path.clone();
            toolbar = toolbar.child(
                Button::new("diff-save-resolution")
                    .primary()
                    .xsmall()
                    .icon(Icon::new(IconName::Check))
                    .label(strings.diff_save_resolution)
                    .disabled(!resolved)
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.save_resolution(save_path.clone(), state.clone(), window, cx)
                    })),
            );
        } else {
            toolbar = toolbar
                .child(
                    Button::new("diff-split")
                        .ghost()
                        .xsmall()
                        .label(strings.diff_side_by_side)
                        .selected(self.diff_split)
                        .on_click(cx.listener(|this, _, _, cx| this.set_diff_split(true, cx))),
                )
                .child(
                    Button::new("diff-unified")
                        .ghost()
                        .xsmall()
                        .label(strings.diff_inline)
                        .selected(!self.diff_split)
                        .on_click(cx.listener(|this, _, _, cx| this.set_diff_split(false, cx))),
                );
            let stage_path = path.clone();
            // A commit's differences have nothing to stage.
            toolbar = toolbar.when(!matches!(source, DiffSource::Commit(..)), |toolbar| {
                toolbar.child(match source {
                    DiffSource::Staged => Button::new("diff-unstage")
                        .ghost()
                        .xsmall()
                        .icon(Icon::new(IconName::Minus))
                        .tooltip(strings.scm_unstage)
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.unstage_paths(vec![stage_path.clone()], window, cx)
                        })),
                    _ => Button::new("diff-stage")
                        .ghost()
                        .xsmall()
                        .icon(Icon::new(IconName::Plus))
                        .tooltip(strings.scm_stage)
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.stage_paths(vec![stage_path.clone()], window, cx)
                        })),
                })
            });
        }
        let open_path = path.clone();
        toolbar = toolbar.child(
            Button::new("diff-open-file")
                .ghost()
                .xsmall()
                .icon(Icon::new(IconName::File))
                .tooltip(strings.scm_open_file)
                .on_click(cx.listener(move |this, _, window, cx| {
                    if let Some(repo) = this.scm.repo.clone() {
                        let file = repo.absolute(&open_path);
                        if file.is_file() {
                            this.open_file(&file, window, cx);
                        }
                    }
                })),
        );
        let body: AnyElement = if empty {
            note(strings.scm_clean.to_owned(), colors).into_any_element()
        } else {
            Diff::new(&tab.state)
                .line_number(true)
                .syntax_highlight(true)
                .header_visible(false)
                .into_any_element()
        };
        div()
            .size_full()
            .flex()
            .flex_col()
            .child(toolbar)
            .child(div().flex_1().min_h_0().child(body))
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::process::Command;

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
    fn changes_open_in_a_tab_and_conflicts_are_resolved(cx: &mut TestAppContext) {
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
        fs::write(root.join("a.txt"), "uno\ndue\n").unwrap();
        git(&root, &["add", "."]);
        git(&root, &["commit", "-q", "-m", "first"]);
        // A conflict: two branches change the same line.
        git(&root, &["switch", "-q", "-c", "other"]);
        fs::write(root.join("a.txt"), "uno\nDUE\n").unwrap();
        git(&root, &["commit", "-q", "-am", "other"]);
        git(&root, &["switch", "-q", "main"]);
        fs::write(root.join("a.txt"), "uno\nDue\n").unwrap();
        git(&root, &["commit", "-q", "-am", "main"]);
        git(&root, &["merge", "-q", "other"]);
        fs::write(root.join("b.txt"), "nuovo\n").unwrap();

        let settings = Settings::load(root.join(".git/keel-test-settings.json"), None);
        let opening = Opening::new(Some(root.clone()), None);
        let (view, cx) =
            cx.add_window_view(|window, cx| Workbench::new(opening, settings, None, window, cx));
        view.update(cx, |view, _| {
            view.scm.status = view.scm.repo.as_ref().unwrap().status().unwrap();
            assert_eq!(view.scm.status.conflicts().count(), 1);
        });

        // A new file's changes open in a tab named after it.
        view.update_in(cx, |view, window, cx| {
            view.open_change(
                "b.txt",
                super::super::source_control::Section::Changes,
                window,
                cx,
            )
        });
        view.read_with(cx, |view, cx| {
            let document = &view.documents[&diff_uri("b.txt", &DiffSource::Changes)];
            let Backing::Diff(tab) = &document.backing else {
                panic!("not a diff");
            };
            assert!(tab.title.starts_with("b.txt ("));
            assert_eq!(tab.state.read(cx).files()[0].additions(), 1);
            assert_eq!(view.panes.len(), 1);
        });

        // The conflict, resolved by keeping the incoming change.
        view.update_in(cx, |view, window, cx| {
            view.open_change(
                "a.txt",
                super::super::source_control::Section::Conflicts,
                window,
                cx,
            )
        });
        let state = view.read_with(cx, |view, _| {
            let Backing::Diff(tab) =
                &view.documents[&diff_uri("a.txt", &DiffSource::Conflict)].backing
            else {
                panic!("not a diff");
            };
            tab.state.clone()
        });
        state.update(cx, |state, cx| {
            assert_eq!(state.files()[0].conflicts().len(), 1);
            state.resolve_conflict(
                "a.txt",
                0,
                Some(gpui_component::diff::DiffConflictResolution::Incoming),
                cx,
            );
        });
        view.update_in(cx, |view, window, cx| {
            view.save_resolution("a.txt".to_owned(), state.clone(), window, cx)
        });
        for _ in 0..50 {
            cx.run_until_parked();
            if view.read_with(cx, |view, _| view.scm.busy.is_none()) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        assert_eq!(
            fs::read_to_string(root.join("a.txt")).unwrap(),
            "uno\nDUE\n"
        );
        let status = git(&root, &["status", "--porcelain"]);
        assert!(status.contains("M  a.txt"), "{status}");
    }
}
