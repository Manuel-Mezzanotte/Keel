//! "Import from Cursor": the extensions of Cursor in the Extensions view,
//! in three groups: to install (from Open VSX, or copied when built
//! locally), already in Keel, and those that cannot work in Keel with the
//! reason. One button installs all that can be installed.

use extensions::cursor::{self, Blocker, BuiltIn, CursorEntry, Verdict};
use gpui::AnyElement;
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::{Disableable as _, Icon, Sizable as _};
use gpui_kit_assets::IconName;

use super::shell::{Colors, heading, note};
use super::*;

actions!(keel, [ImportFromCursor]);

/// The report shown in the Extensions view.
#[derive(Default)]
pub(super) struct CursorImport {
    pub(super) loading: bool,
    pub(super) entries: Vec<CursorEntry>,
    pub(super) error: Option<String>,
}

fn blocker_text(blocker: Blocker, strings: &Strings) -> &'static str {
    match blocker {
        Blocker::CursorOnly => strings.cursor_blocker_cursor_only,
        Blocker::Debugger => strings.cursor_blocker_debugger,
        Blocker::Notebooks => strings.cursor_blocker_notebooks,
        Blocker::CustomEditors => strings.cursor_blocker_custom_editors,
        Blocker::Views => strings.cursor_blocker_views,
        Blocker::Webviews => strings.cursor_blocker_webviews,
        Blocker::LanguageServer => strings.cursor_blocker_language_server,
    }
}

fn built_in_text(feature: BuiltIn, strings: &Strings) -> &'static str {
    match feature {
        BuiltIn::Prettier => strings.cursor_builtin_prettier,
        BuiltIn::GitGraph => strings.cursor_builtin_git_graph,
        BuiltIn::ClaudeCode => strings.cursor_builtin_claude_code,
        BuiltIn::Pdf => strings.cursor_builtin_pdf,
        BuiltIn::MarkdownPreview => strings.cursor_builtin_markdown_preview,
        BuiltIn::Yaml => strings.cursor_builtin_yaml,
        BuiltIn::TypeScript => strings.cursor_builtin_typescript,
    }
}

fn reasons(blockers: &[Blocker], strings: &Strings) -> String {
    blockers
        .iter()
        .map(|blocker| blocker_text(*blocker, strings))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Which group an entry belongs to: 0 to install, 1 already in Keel,
/// 2 cannot work.
fn group(verdict: &Verdict) -> usize {
    match verdict {
        Verdict::Available { .. } | Verdict::Local { .. } => 0,
        Verdict::Installed | Verdict::BuiltIn(_) => 1,
        Verdict::Blocked(_) | Verdict::NeedsBlocked(_) | Verdict::NotOnOpenVsx => 2,
    }
}

impl Workbench {
    /// Reads Cursor's extensions and what Keel would do with each.
    pub(super) fn import_from_cursor(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.show_side_view(SideView::Extensions, cx);
        let Some(folder) = self
            .store
            .cursor_dir
            .clone()
            .or_else(cursor::cursor_extensions_dir)
        else {
            return;
        };
        self.store.cursor = Some(CursorImport {
            loading: true,
            ..Default::default()
        });
        let installed: HashSet<String> = self
            .installed
            .iter()
            .map(|extension| extension.id().to_lowercase())
            .collect();
        let market = self.store.client.clone();
        let task = cx.background_spawn(async move {
            let found = cursor::cursor_extensions(&folder)?;
            anyhow::Ok(cursor::analyze(&found, &installed, market.as_ref()))
        });
        cx.notify();
        cx.spawn_in(window, async move |this, cx| {
            let report = task.await;
            this.update(cx, |this, cx| {
                this.store.cursor = Some(match report {
                    Ok(entries) => CursorImport {
                        entries,
                        ..Default::default()
                    },
                    Err(error) => CursorImport {
                        error: Some(format!("{error:#}")),
                        ..Default::default()
                    },
                });
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Installs one entry: from Open VSX, or copied from Cursor's folder.
    fn install_cursor_entry(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(entry) = self
            .store
            .cursor
            .as_ref()
            .and_then(|report| report.entries.get(index))
            .cloned()
        else {
            return;
        };
        match entry.verdict {
            Verdict::Available { .. } => {
                self.install_from_open_vsx(&entry.extension.id, window, cx)
            }
            Verdict::Local { .. } => {
                let Some(data) = self.extension_data.clone() else {
                    return;
                };
                match cursor::copy_extension(
                    &entry.extension.folder,
                    &crate::paths::extensions_dir(&data),
                ) {
                    Ok(_) => {
                        self.status_message = Some(self.strings().extension_installed(&entry.name));
                        self.restart_extensions(window, cx);
                    }
                    Err(error) => self.report(format!("{error:#}")),
                }
            }
            _ => {}
        }
    }

    /// "Install the available ones": every entry to install not yet in Keel.
    fn install_all_from_cursor(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let indices: Vec<usize> = self
            .store
            .cursor
            .iter()
            .flat_map(|report| report.entries.iter().enumerate())
            .filter(|(_, entry)| {
                group(&entry.verdict) == 0 && self.installed_version(&entry.extension.id).is_none()
            })
            .map(|(index, _)| index)
            .collect();
        for index in indices {
            self.install_cursor_entry(index, window, cx);
        }
    }

    pub(super) fn cursor_import_actions(
        &self,
        root: gpui::Div,
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        root.on_action(
            cx.listener(|this, _: &ImportFromCursor, window, cx| {
                this.import_from_cursor(window, cx)
            }),
        )
    }

    /// The report in place of the installed extensions.
    pub(super) fn render_cursor_import(
        &self,
        report: &CursorImport,
        colors: Colors,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let strings = self.strings();
        let to_install = report
            .entries
            .iter()
            .filter(|entry| {
                group(&entry.verdict) == 0 && self.installed_version(&entry.extension.id).is_none()
            })
            .count();
        let header = div()
            .flex()
            .items_center()
            .gap_1()
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .child(heading(strings.cursor_heading, colors)),
            )
            .when(to_install > 0, |header| {
                header.child(
                    Button::new("cursor-install-all")
                        .primary()
                        .xsmall()
                        .label(strings.cursor_install_all(to_install))
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.install_all_from_cursor(window, cx)
                        })),
                )
            })
            .child(
                Button::new("cursor-close")
                    .ghost()
                    .xsmall()
                    .icon(Icon::new(IconName::X))
                    .tooltip(strings.cursor_close)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.store.cursor = None;
                        cx.notify();
                    })),
            );
        let mut view = div().flex().flex_col().gap_1().child(header);
        if report.loading {
            return view
                .child(note(strings.cursor_loading.to_owned(), colors))
                .into_any_element();
        }
        if let Some(error) = &report.error {
            return view.child(note(error.clone(), colors)).into_any_element();
        }
        if report.entries.is_empty() {
            return view
                .child(note(strings.cursor_none.to_owned(), colors))
                .into_any_element();
        }
        for (number, title) in [
            strings.cursor_group_install,
            strings.cursor_group_keel,
            strings.cursor_group_blocked,
        ]
        .into_iter()
        .enumerate()
        {
            let rows: Vec<AnyElement> = report
                .entries
                .iter()
                .enumerate()
                .filter(|(_, entry)| group(&entry.verdict) == number)
                .map(|(index, entry)| self.render_cursor_entry(index, entry, colors, cx))
                .collect();
            if !rows.is_empty() {
                view = view.child(heading(title, colors)).children(rows);
            }
        }
        view.into_any_element()
    }

    fn render_cursor_entry(
        &self,
        index: usize,
        entry: &CursorEntry,
        colors: Colors,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let strings = self.strings();
        let installed = self.installed_version(&entry.extension.id).is_some();
        let installing = self
            .store
            .installing
            .contains(&entry.extension.id.to_lowercase());
        let (version, detail) = match &entry.verdict {
            Verdict::Available { listing, partial } => (
                listing.version.clone(),
                (!partial.is_empty()).then(|| strings.cursor_partial(&reasons(partial, strings))),
            ),
            Verdict::Local { partial } => (
                entry.extension.version.clone(),
                Some(if partial.is_empty() {
                    strings.cursor_local.to_owned()
                } else {
                    format!(
                        "{} · {}",
                        strings.cursor_local,
                        strings.cursor_partial(&reasons(partial, strings))
                    )
                }),
            ),
            Verdict::Installed => (
                entry.extension.version.clone(),
                Some(strings.cursor_installed.to_owned()),
            ),
            Verdict::BuiltIn(feature) => (
                entry.extension.version.clone(),
                Some(built_in_text(*feature, strings).to_owned()),
            ),
            Verdict::Blocked(blockers) => (
                entry.extension.version.clone(),
                Some(strings.cursor_blocked(&reasons(blockers, strings))),
            ),
            Verdict::NeedsBlocked(need) => (
                entry.extension.version.clone(),
                Some(strings.cursor_needs(need)),
            ),
            Verdict::NotOnOpenVsx => (
                entry.extension.version.clone(),
                Some(strings.cursor_not_on_open_vsx.to_owned()),
            ),
        };
        let button = (group(&entry.verdict) == 0).then(|| {
            let button = Button::new(("cursor-install", index)).xsmall();
            if installed {
                button
                    .ghost()
                    .label(strings.extension_installed_badge)
                    .disabled(true)
            } else if installing {
                button
                    .ghost()
                    .label(strings.extension_installing)
                    .disabled(true)
            } else {
                button
                    .primary()
                    .label(strings.extension_install)
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.install_cursor_entry(index, window, cx)
                    }))
            }
        });
        div()
            .id(("cursor-entry", index))
            .px_2()
            .py_1()
            .flex()
            .items_center()
            .gap_2()
            .rounded(px(6.))
            .hover(|row| row.bg(colors.hover))
            .when(group(&entry.verdict) == 2, |row| row.opacity(0.6))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .flex()
                            .items_baseline()
                            .gap_1p5()
                            .child(
                                div()
                                    .min_w_0()
                                    .truncate()
                                    .text_sm()
                                    .text_color(colors.text)
                                    .child(entry.name.clone()),
                            )
                            .child(
                                div()
                                    .flex_none()
                                    .text_xs()
                                    .text_color(colors.faint)
                                    .child(version),
                            ),
                    )
                    .when(!entry.description.is_empty(), |text| {
                        text.child(
                            div()
                                .truncate()
                                .text_xs()
                                .text_color(colors.muted)
                                .child(entry.description.clone()),
                        )
                    })
                    .when_some(detail, |text, detail| {
                        text.child(div().text_xs().text_color(colors.faint).child(detail))
                    }),
            )
            .children(button)
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
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    #[gpui::test]
    fn cursors_extensions_are_sorted_and_installed(cx: &mut TestAppContext) {
        cx.executor().allow_parking();
        cx.update(|cx| {
            gpui_component::init(cx);
            language::register_grammars();
            init(cx);
        });
        let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures");
        // Cursor's folder: VSAuto Ghost built locally, Prettier and a
        // Remote SSH that only works in Cursor.
        let cursor = tempfile::tempdir().unwrap();
        let ghost = crate::paths::install_extension(
            cursor.path(),
            &fixtures.join("extensions/vsauto-ghost-local-0.6.1-editor-layout.vsix"),
        )
        .unwrap();
        let write = |id: &str| {
            let (publisher, name) = id.split_once('.').unwrap();
            let root = cursor.path().join(format!("{id}-1.0.0-universal"));
            std::fs::create_dir_all(&root).unwrap();
            std::fs::write(
                root.join("package.json"),
                format!(r#"{{ "name": "{name}", "publisher": "{publisher}", "version": "1.0.0", "displayName": "{name}" }}"#),
            )
            .unwrap();
            root
        };
        let prettier = write("esbenp.prettier-vscode");
        let ssh = write("anysphere.remote-ssh");
        let entry = |id: &str, folder: &Path, source: &str| {
            json!({ "identifier": { "id": id }, "version": "1.0.0",
                    "relativeLocation": folder.strip_prefix(cursor.path()).unwrap().to_str().unwrap(),
                    "metadata": { "source": source } })
        };
        std::fs::write(
            cursor.path().join("extensions.json"),
            json!([
                entry("manuel-local.vsauto-ghost", &ghost.root, "vsix"),
                entry("esbenp.prettier-vscode", &prettier, "gallery"),
                entry("anysphere.remote-ssh", &ssh, "gallery"),
            ])
            .to_string(),
        )
        .unwrap();

        let data = tempfile::tempdir().unwrap();
        let mut setup = crate::windows::extension_setup(data.path()).unwrap();
        setup.marketplace = None;
        let (view, cx) = cx.add_window_view(|window, cx| {
            let mut workbench = Workbench::new(None, test_settings(), None, window, cx);
            workbench.store.cursor_dir = Some(cursor.path().to_path_buf());
            workbench.start_extensions(setup, window, cx);
            workbench
        });
        view.update_in(cx, |view, window, cx| view.import_from_cursor(window, cx));
        wait_until(cx, "the report", |cx| {
            view.read_with(cx, |view, _| {
                view.store
                    .cursor
                    .as_ref()
                    .is_some_and(|report| !report.loading)
            })
        });
        view.read_with(cx, |view, _| {
            assert_eq!(view.layout.side_view, SideView::Extensions);
            let report = view.store.cursor.as_ref().unwrap();
            let groups: Vec<(&str, usize)> = report
                .entries
                .iter()
                .map(|entry| (entry.extension.id.as_str(), group(&entry.verdict)))
                .collect();
            assert_eq!(
                groups,
                [
                    ("anysphere.remote-ssh", 2),
                    ("esbenp.prettier-vscode", 1),
                    ("manuel-local.vsauto-ghost", 0)
                ]
            );
        });

        // "Install the available ones" copies VSAuto Ghost from Cursor.
        view.update_in(cx, |view, window, cx| {
            view.install_all_from_cursor(window, cx)
        });
        wait_until(cx, "VSAuto Ghost in Keel", |cx| {
            view.read_with(cx, |view, _| view.installed.len() == 1)
        });
        assert!(
            crate::paths::extensions_dir(data.path())
                .join("manuel-local.vsauto-ghost-0.6.1/package.json")
                .is_file()
        );
        // Cursor's own folder is left as it was.
        assert!(ghost.root.join("package.json").is_file());
    }
}
