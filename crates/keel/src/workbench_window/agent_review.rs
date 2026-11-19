//! The agent's changes, file by file: Keel keeps each file's text from
//! before the agent first touched it in the conversation, lists the files
//! that differ now with their added and removed lines, and lets you look
//! at the differences, keep a change (Accept) or put the file back
//! (Reject), one by one or all together.

use gpui::{AnyElement, StatefulInteractiveElement as _};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::diff::{Diff, DiffFile, DiffMode, DiffState};
use gpui_component::{ActiveTheme as _, Icon, Sizable as _};
use gpui_kit_assets::IconName;

use super::diff_view::{DiffSource, DiffTab, diff_uri};
use super::shell::Colors;
use super::*;

/// A file the agent changed, as it stands now.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct AgentChange {
    pub(super) path: PathBuf,
    /// None: the agent created it.
    pub(super) before: Option<String>,
    pub(super) added: usize,
    pub(super) removed: usize,
}

impl Workbench {
    /// Reads again which kept files differ from what is on disk now.
    pub(super) fn refresh_agent_changes(&mut self) {
        let changes = self
            .agent_panel
            .snapshots
            .iter()
            .filter_map(|(path, before)| {
                let now = std::fs::read_to_string(path).ok();
                if now == *before {
                    return None;
                }
                let old = before.as_deref().unwrap_or_default();
                let new = now.as_deref().unwrap_or_default();
                let hunks = git::hunks(old, new);
                Some(AgentChange {
                    path: path.clone(),
                    before: before.clone(),
                    added: hunks.iter().map(|hunk| hunk.new.len()).sum(),
                    removed: hunks.iter().map(|hunk| hunk.old.len()).sum(),
                })
            })
            .collect();
        self.agent_panel.changes = changes;
    }

    /// Keeps the agent's change to `path`.
    pub(super) fn accept_agent_change(
        &mut self,
        path: &Path,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.agent_panel
            .snapshots
            .retain(|(known, _)| known != path);
        self.close_agent_change_tab(path, window, cx);
        self.refresh_agent_changes();
        cx.notify();
    }

    /// Puts `path` back as it was; a file the agent created goes to the
    /// Trash.
    pub(super) fn reject_agent_change(
        &mut self,
        path: &Path,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(index) = self
            .agent_panel
            .snapshots
            .iter()
            .position(|(known, _)| known == path)
        else {
            return;
        };
        let (_, before) = self.agent_panel.snapshots.remove(index);
        let restored = match &before {
            Some(text) => std::fs::write(path, text).map_err(|error| error.to_string()),
            None if path.exists() => files::trash(path).map_err(|error| error.to_string()),
            None => Ok(()),
        };
        if let Err(error) = restored {
            self.report(format!("{}: {error}", path.display()));
        }
        // An open file without unsaved changes shows the old text at once.
        self.reload_document(&Uri::file(path).to_string(), window, cx);
        self.close_agent_change_tab(path, window, cx);
        self.refresh_agent_changes();
        cx.notify();
    }

    pub(super) fn accept_all_agent_changes(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let paths: Vec<PathBuf> = self
            .agent_panel
            .changes
            .iter()
            .map(|change| change.path.clone())
            .collect();
        for path in paths {
            self.accept_agent_change(&path, window, cx);
        }
    }

    pub(super) fn reject_all_agent_changes(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let paths: Vec<PathBuf> = self
            .agent_panel
            .changes
            .iter()
            .map(|change| change.path.clone())
            .collect();
        for path in paths {
            self.reject_agent_change(&path, window, cx);
        }
    }

    /// A tab with the differences between the kept text and the file now.
    pub(super) fn open_agent_change(
        &mut self,
        path: &Path,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(change) = self
            .agent_panel
            .changes
            .iter()
            .find(|change| change.path == path)
            .cloned()
        else {
            return;
        };
        let now = std::fs::read_to_string(path).unwrap_or_default();
        let shown = self
            .project
            .root
            .as_ref()
            .and_then(|root| path.strip_prefix(root).ok())
            .unwrap_or(path)
            .to_string_lossy()
            .into_owned();
        let patch = git::unified_patch(
            &shown,
            change.before.as_deref().unwrap_or_default(),
            &now,
            3,
        );
        let doc = super::diff_pane::DiffDoc::new(
            vec![super::diff_model::FileDiff::new(
                path.display().to_string(),
                change.before.as_deref().unwrap_or_default(),
                &now,
                false,
            )],
            self.diff_split,
            false,
        );
        let language = Language::for_path(path).highlighter_name();
        let files: Vec<DiffFile> = DiffFile::parse(&patch)
            .map(|files| {
                files
                    .into_iter()
                    .map(|file| file.with_language(language))
                    .collect()
            })
            .unwrap_or_default();
        let source = DiffSource::Agent(path.display().to_string());
        let uri = diff_uri("", &source);
        let mode = if self.diff_split {
            DiffMode::Split
        } else {
            DiffMode::Unified
        };
        let state = cx.new(|cx| DiffState::new(files, cx).with_mode(mode));
        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        // Read again each time it opens: the agent may have changed it since.
        self.documents.insert(
            uri.clone(),
            OpenDocument {
                uri: uri.clone(),
                backing: Backing::Diff(DiffTab {
                    path: path.display().to_string(),
                    source,
                    state,
                    title: format!("{name} (Claude)"),

                    doc: Some(doc),
                }),
                language_id: "diff".to_owned(),
                version: 1,
                synced_text: String::new(),
                modified: false,
            },
        );
        let column = self.active.min(self.panes.len().saturating_sub(1));
        self.show_tab(&uri, column, OpenAs::Preview, true, window, cx);
        cx.notify();
    }

    fn close_agent_change_tab(&mut self, path: &Path, window: &mut Window, cx: &mut Context<Self>) {
        let uri = diff_uri("", &DiffSource::Agent(path.display().to_string()));
        while let Some((group, tab)) = self.groups.iter().enumerate().find_map(|(group, tabs)| {
            tabs.tabs
                .iter()
                .position(|tab| tab.uri == uri)
                .map(|tab| (group, tab))
        }) {
            self.close_tab_now(group, tab, window, cx);
        }
        self.documents.remove(&uri);
    }

    /// The list above the message box: one row per changed file.
    pub(super) fn render_agent_changes(
        &self,
        colors: Colors,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let changes = &self.agent_panel.changes;
        if changes.is_empty() {
            return None;
        }
        let strings = self.strings();
        let theme = cx.theme();
        let (green, red) = (theme.success, theme.danger);
        let header =
            div()
                .flex()
                .items_center()
                .gap_1()
                .text_xs()
                .child(
                    div()
                        .flex_1()
                        .text_color(colors.muted)
                        .child(strings.agent_changes(changes.len())),
                )
                .child(
                    Button::new("agent-reject-all")
                        .ghost()
                        .small()
                        .label(strings.agent_reject_all)
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.reject_all_agent_changes(window, cx)
                        })),
                )
                .child(
                    Button::new("agent-accept-all")
                        .primary()
                        .small()
                        .label(strings.agent_accept_all)
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.accept_all_agent_changes(window, cx)
                        })),
                );
        let rows =
            changes.iter().enumerate().map(|(index, change)| {
                let name = change
                    .path
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_default();
                let (open, accept, reject) = (
                    change.path.clone(),
                    change.path.clone(),
                    change.path.clone(),
                );
                div()
                    .id(("agent-change", index))
                    .flex()
                    .items_center()
                    .gap_1()
                    .text_xs()
                    .px_1()
                    .rounded(px(4.))
                    .cursor_pointer()
                    .hover(|row| row.bg(colors.hover))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.open_agent_change(&open, window, cx)
                    }))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_color(colors.text)
                            .child(name),
                    )
                    .when(change.before.is_none(), |row| {
                        row.child(div().text_color(colors.faint).child(strings.agent_new_file))
                    })
                    .child(div().text_color(green).child(format!("+{}", change.added)))
                    .child(div().text_color(red).child(format!("−{}", change.removed)))
                    .child(
                        Button::new(("agent-change-reject", index))
                            .ghost()
                            .small()
                            .icon(Icon::new(IconName::Undo2).xsmall())
                            .tooltip(strings.claude_reject)
                            .on_click(cx.listener(move |this, _, window, cx| {
                                cx.stop_propagation();
                                this.reject_agent_change(&reject, window, cx)
                            })),
                    )
                    .child(
                        Button::new(("agent-change-accept", index))
                            .ghost()
                            .small()
                            .icon(Icon::new(IconName::Check).xsmall())
                            .tooltip(strings.claude_accept)
                            .on_click(cx.listener(move |this, _, window, cx| {
                                cx.stop_propagation();
                                this.accept_agent_change(&accept, window, cx)
                            })),
                    )
            });
        Some(
            div()
                .flex_none()
                .mx_3()
                .mb_1()
                .px_2()
                .py_1p5()
                .rounded(px(8.))
                .border_1()
                .border_color(colors.border)
                .flex()
                .flex_col()
                .gap_0p5()
                .child(header)
                .children(rows)
                .into_any_element(),
        )
    }

    /// The tab of one change: the differences, with Accept and Reject.
    pub(super) fn render_agent_change_tab(
        &self,
        path: &str,
        tab: &DiffTab,
        colors: Colors,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let strings = self.strings();
        let waiting = self
            .agent_panel
            .changes
            .iter()
            .any(|change| change.path == Path::new(path));
        let (accept, reject) = (PathBuf::from(path), PathBuf::from(path));
        let toolbar = div()
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
                    .child(format!("{} · {}", strings.agent_change_of_claude, tab.path)),
            )
            .when(waiting, |toolbar| {
                toolbar
                    .child(
                        Button::new("agent-tab-reject")
                            .ghost()
                            .small()
                            .icon(Icon::new(IconName::Undo2))
                            .label(strings.claude_reject)
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.reject_agent_change(&reject, window, cx)
                            })),
                    )
                    .child(
                        Button::new("agent-tab-accept")
                            .primary()
                            .small()
                            .icon(Icon::new(IconName::Check))
                            .label(strings.claude_accept)
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.accept_agent_change(&accept, window, cx)
                            })),
                    )
            });
        div()
            .size_full()
            .flex()
            .flex_col()
            .child(toolbar)
            .child(
                div().flex_1().min_h_0().child(match &tab.doc {
                    Some(doc) => self.render_diff_doc(
                        &diff_uri("", &DiffSource::Agent(path.to_owned())),
                        doc,
                        super::diff_pane::BlockActions::None,
                        colors,
                        window,
                        cx,
                    ),
                    None => Diff::new(&tab.state)
                        .line_number(true)
                        .syntax_highlight(true)
                        .header_visible(false)
                        .into_any_element(),
                }),
            )
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use gpui::{TestAppContext, VisualTestContext};

    use super::super::tests::test_settings;
    use super::*;

    fn wait_until(
        cx: &mut VisualTestContext,
        what: &str,
        check: impl Fn(&mut VisualTestContext) -> bool,
    ) {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            cx.executor().advance_clock(Duration::from_millis(100));
            cx.run_until_parked();
            if check(cx) {
                return;
            }
            assert!(Instant::now() < deadline, "timed out waiting for {what}");
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    /// Asks the fake agent to change `old` into `new` and allows it.
    fn agent_edit(
        view: &Entity<Workbench>,
        cx: &mut VisualTestContext,
        path: &Path,
        old: &str,
        new: &str,
    ) {
        view.update_in(cx, |view, window, cx| {
            let message = format!("edit {} {old} {new}", path.display());
            view.agent_panel
                .input
                .update(cx, |input, cx| input.set_value(message, window, cx));
            view.send_to_agent(window, cx);
        });
        wait_until(cx, "the permission", |cx| {
            view.read_with(cx, |view, _| !view.agent_panel.permissions.is_empty())
        });
        view.update(cx, |view, cx| view.answer_permission(0, "allow", cx));
        wait_until(cx, "the change", |cx| {
            view.read_with(cx, |view, _| {
                !view.agent_panel.running && !view.agent_panel.changes.is_empty()
            })
        });
    }

    #[gpui::test]
    fn the_agents_changes_are_reviewed_file_by_file(cx: &mut TestAppContext) {
        cx.executor().allow_parking();
        cx.update(|cx| {
            gpui_component::init(cx);
            language::register_grammars();
            init(cx);
        });
        let project = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(project.path().join(".git")).unwrap();
        let path = std::path::absolute(project.path().join("booking.ts")).unwrap();
        std::fs::write(&path, "const nights = 3;\nconst price = 90;\n").unwrap();
        let document = Document::open(&path).unwrap();
        let (view, cx) = cx.add_window_view(|window, cx| {
            let mut workbench = Workbench::new(Some(document), test_settings(), None, window, cx);
            workbench.agent_panel.fake = true;
            workbench.open_agent(window, cx);
            workbench
        });
        wait_until(cx, "the agent", |cx| {
            view.read_with(cx, |view, _| {
                view.agent_panel.status == super::super::agent_panel::AgentStatus::Ready
            })
        });

        agent_edit(&view, cx, &path, "90", "95");
        view.read_with(cx, |view, _| {
            let change = &view.agent_panel.changes[0];
            assert_eq!(change.path, path);
            assert_eq!((change.added, change.removed), (1, 1));
        });

        // The differences open in a tab.
        view.update_in(cx, |view, window, cx| {
            view.open_agent_change(&path, window, cx)
        });
        view.read_with(cx, |view, cx| {
            let uri = view.panes[view.active].uri.clone();
            assert!(uri.starts_with("keel-agent-diff:"), "{uri}");
            let Some(Backing::Diff(tab)) =
                view.documents.get(&uri).map(|document| &document.backing)
            else {
                panic!()
            };
            assert_eq!(tab.state.read(cx).files()[0].additions(), 1);
        });

        // Reject puts the file back, and the editor shows it.
        view.update_in(cx, |view, window, cx| {
            view.reject_agent_change(&path, window, cx)
        });
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "const nights = 3;\nconst price = 90;\n"
        );
        view.read_with(cx, |view, cx| {
            assert!(view.agent_panel.changes.is_empty());
            assert!(view.agent_panel.snapshots.is_empty());
            let uri = Uri::file(&path).to_string();
            let pane = view.panes.iter().find(|pane| pane.uri == uri).unwrap();
            assert_eq!(
                pane.editor.read(cx).value().to_string(),
                "const nights = 3;\nconst price = 90;\n"
            );
        });

        // Accept keeps it, and the list empties.
        agent_edit(&view, cx, &path, "3", "4");
        view.update_in(cx, |view, window, cx| {
            view.accept_all_agent_changes(window, cx)
        });
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "const nights = 4;\nconst price = 90;\n"
        );
        view.read_with(cx, |view, _| {
            assert!(view.agent_panel.changes.is_empty());
            assert!(view.agent_panel.snapshots.is_empty());
        });
    }
}
