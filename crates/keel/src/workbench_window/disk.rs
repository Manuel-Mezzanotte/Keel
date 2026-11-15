//! Changes made on disk outside Keel, for example by Claude Code: the tree
//! and the file list follow, open files reload, and a file with unsaved
//! changes asks before its save overwrites what is on disk.

use std::path::{Path, PathBuf};
use std::time::Duration;

use futures::channel::mpsc;

use super::*;

/// How long Keel waits for a burst of changes to settle.
const SETTLE: Duration = Duration::from_millis(150);

impl Workbench {
    /// Starts watching the project's folder.
    pub(super) fn watch_project(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // GPUI's test clock refuses wake-ups from FSEvents' thread; tests
        // call `disk_changed` directly and the watcher has its own test.
        if cfg!(test) {
            return;
        }
        let Some(root) = self.project.root.clone() else {
            return;
        };
        let (sender, mut receiver) = mpsc::unbounded::<Vec<PathBuf>>();
        match files::watch(&root, move |paths| {
            let _ = sender.unbounded_send(paths);
        }) {
            Ok(watcher) => self.watcher = Some(watcher),
            Err(error) => {
                self.log(format!("{}: {error}", root.display()));
                return;
            }
        }
        self.watching = cx.spawn_in(window, async move |this, cx| {
            while let Some(first) = receiver.next().await {
                // Tools write many files at once: take them together.
                let mut paths = first;
                cx.background_executor().timer(SETTLE).await;
                while let Ok(more) = receiver.try_recv() {
                    paths.extend(more);
                }
                let done =
                    this.update_in(cx, |this, window, cx| this.disk_changed(&paths, window, cx));
                if done.is_err() {
                    break;
                }
            }
        });
    }

    /// Follows changes made on disk: the branch, the tree, the file list
    /// and the open files.
    pub(super) fn disk_changed(
        &mut self,
        paths: &[PathBuf],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let head = paths.iter().any(|path| path.ends_with(".git/HEAD"));
        if head && self.project.refresh_branch() {
            cx.notify();
        }
        if paths.iter().any(|path| !path.ends_with(".git/HEAD")) {
            self.refresh_tree(cx);
            self.index_files(cx);
        }
        let touched: Vec<String> = self
            .documents
            .values()
            .filter_map(|document| match &document.backing {
                Backing::File(file) if paths.iter().any(|path| same_file(path, file.path())) => {
                    Some(document.uri.clone())
                }
                _ => None,
            })
            .collect();
        for uri in touched {
            self.reload_document(&uri, window, cx);
        }
    }

    /// Takes a file's new text from disk into its editors, unless it has
    /// unsaved changes, which are kept and only reported.
    pub(super) fn reload_document(
        &mut self,
        uri: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let strings = self.strings();
        let Some(document) = self.documents.get_mut(uri) else {
            return;
        };
        let Backing::File(file) = &mut document.backing else {
            return;
        };
        let Some(text) = file.changed_on_disk() else {
            return;
        };
        if document.modified {
            let message = strings.changed_on_disk(&file.file_name());
            self.report(message);
            cx.notify();
            return;
        }
        file.accept_disk_text(text.clone());
        document.synced_text = text.clone();
        document.version += 1;
        let params = json!({
            "uri": uri,
            "version": document.version,
            "text": text,
            "isDirty": false,
        });
        self.notify_host("documents/changed", params);
        let editors: Vec<Entity<EditorState>> = self
            .panes
            .iter()
            .filter(|pane| pane.uri == uri)
            .map(|pane| pane.editor.clone())
            .chain(
                self.groups
                    .iter()
                    .flat_map(|group| &group.tabs)
                    .filter(|tab| tab.uri == uri)
                    .filter_map(|tab| tab.parked.as_ref().map(|pane| pane.editor.clone())),
            )
            .collect();
        for editor in editors {
            tabs::replace_text(&editor, &text, window, cx);
        }
        self.schedule_gutter_refresh(window, cx);
        cx.notify();
    }

    /// Saves a file, but first asks when someone else changed it on disk
    /// since Keel read it, so neither version is lost by surprise.
    pub(super) fn save_file(
        &mut self,
        uri: &str,
        after: Option<AfterConfirm>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let strings = self.strings();
        let Some(Backing::File(file)) = self.documents.get(uri).map(|document| &document.backing)
        else {
            return;
        };
        if file.changed_on_disk().is_none() {
            self.save_file_now(uri, after, window, cx);
            return;
        }
        let answer = window.prompt(
            PromptLevel::Warning,
            &strings.overwrite_question(&file.file_name()),
            Some(strings.overwrite_detail),
            &[strings.overwrite, strings.use_disk_version, strings.cancel],
            cx,
        );
        let uri = uri.to_owned();
        cx.spawn_in(window, async move |this, cx| {
            let Ok(choice) = answer.await else {
                return;
            };
            this.update_in(cx, |this, window, cx| match choice {
                0 => this.save_file_now(&uri, after, window, cx),
                1 => {
                    // Keep what is on disk and drop the changes made in Keel.
                    if let Some(document) = this.documents.get_mut(&uri) {
                        document.modified = false;
                    }
                    this.reload_document(&uri, window, cx);
                    this.refresh_title(window);
                }
                _ => {}
            })
            .ok();
        })
        .detach();
    }

    fn save_file_now(
        &mut self,
        uri: &str,
        after: Option<AfterConfirm>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let strings = self.strings();
        let Some(document) = self.documents.get_mut(uri) else {
            return;
        };
        let text = document.synced_text.clone();
        let Backing::File(file) = &mut document.backing else {
            return;
        };
        match file.save(&text) {
            Ok(()) => {
                document.modified = false;
                self.notify_host("documents/saved", json!({ "uri": uri }));
                self.forget_backup(uri);
                self.refresh_title(window);
                cx.notify();
                if let Some(after) = after {
                    self.finish(after, window, cx);
                }
            }
            Err(error) => {
                let message = strings.could_not_save(&file.file_name());
                show_error(&message, &error.to_string(), strings.ok, window, cx);
            }
        }
    }
}

/// Whether two paths are the same file, even when one goes through a
/// symbolic link, such as `/var` and `/private/var`.
fn same_file(changed: &Path, open: &Path) -> bool {
    changed == open
        || std::fs::canonicalize(open)
            .map(|open| open == changed)
            .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use std::fs;

    use gpui::{TestAppContext, VisualTestContext};

    use super::*;

    fn window_on(
        cx: &mut TestAppContext,
    ) -> (
        tempfile::TempDir,
        PathBuf,
        Entity<Workbench>,
        &mut VisualTestContext,
    ) {
        cx.update(|cx| {
            gpui_component::init(cx);
            language::register_grammars();
            crate::workbench_window::init(cx);
        });
        let folder = tempfile::tempdir().unwrap();
        let base = fs::canonicalize(folder.path()).unwrap();
        let path = base.join("app.ts");
        fs::write(&path, "let a = 1;\n").unwrap();
        let settings_file = base.join("settings.json");
        fs::write(&settings_file, r#"{ "keel.language": "en" }"#).unwrap();
        let settings = Settings::load(&settings_file, None);
        let opening = Opening::new(Some(base.clone()), Some(path.clone()));
        let (view, cx) =
            cx.add_window_view(|window, cx| Workbench::new(opening, settings, None, window, cx));
        (folder, path, view, cx)
    }

    fn text(view: &Entity<Workbench>, cx: &mut VisualTestContext) -> String {
        view.read_with(cx, |view, cx| {
            view.panes[view.active].editor.read(cx).value().to_string()
        })
    }

    #[gpui::test]
    fn files_changed_outside_reload_when_untouched(cx: &mut TestAppContext) {
        let (_folder, path, view, cx) = window_on(cx);
        fs::write(&path, "let a = 2;\n").unwrap();
        view.update_in(cx, |view, window, cx| {
            view.disk_changed(std::slice::from_ref(&path), window, cx)
        });
        assert_eq!(text(&view, cx), "let a = 2;\n");
        assert!(!view.read_with(cx, |view, _| view.has_unsaved_changes()));
    }

    #[gpui::test]
    fn unsaved_changes_are_kept_and_saving_asks_first(cx: &mut TestAppContext) {
        let (_folder, path, view, cx) = window_on(cx);
        cx.simulate_input("// mine\n");
        fs::write(&path, "let a = 3; // theirs\n").unwrap();
        view.update_in(cx, |view, window, cx| {
            view.disk_changed(std::slice::from_ref(&path), window, cx)
        });
        assert_eq!(text(&view, cx), "// mine\nlet a = 1;\n");
        let status = view.read_with(cx, |view, _| view.status_message.clone());
        assert!(status.unwrap().contains("app.ts"));

        cx.dispatch_action(Save);
        assert!(cx.has_pending_prompt());
        cx.simulate_prompt_answer("Cancel");
        cx.run_until_parked();
        assert_eq!(fs::read_to_string(&path).unwrap(), "let a = 3; // theirs\n");

        cx.dispatch_action(Save);
        cx.simulate_prompt_answer("Overwrite");
        cx.run_until_parked();
        assert_eq!(fs::read_to_string(&path).unwrap(), "// mine\nlet a = 1;\n");
    }

    #[gpui::test]
    fn the_version_on_disk_can_win(cx: &mut TestAppContext) {
        let (_folder, path, view, cx) = window_on(cx);
        cx.simulate_input("// mine\n");
        fs::write(&path, "let a = 4;\n").unwrap();
        cx.dispatch_action(Save);
        cx.simulate_prompt_answer("Use the Version on Disk");
        cx.run_until_parked();
        assert_eq!(text(&view, cx), "let a = 4;\n");
        assert!(!view.read_with(cx, |view, _| view.has_unsaved_changes()));
    }
}
