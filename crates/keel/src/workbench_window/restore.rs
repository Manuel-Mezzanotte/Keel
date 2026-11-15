//! Nothing lost: each project's tabs and layout come back when it opens
//! again, unsaved changes are kept in backups a moment after typing (and
//! survive a crash or Cmd+Q, as VS Code's hot exit), and files can save
//! themselves (`files.autoSave`).

use std::path::PathBuf;
use std::time::Duration;

use crate::layout::{PanelView, SideView};
use crate::session::{self, Backup, GroupState, TabState, WindowState};

use super::*;

/// How long after the last change the session and the backups are written.
const SESSION_DELAY: Duration = Duration::from_millis(800);
const BACKUP_DELAY: Duration = Duration::from_millis(1000);

impl Workbench {
    fn session_file(&self) -> Option<PathBuf> {
        Some(session::state_file(
            self.data_dir.as_ref()?,
            self.project.root.as_ref()?,
        ))
    }

    fn backup_folder(&self) -> Option<PathBuf> {
        Some(session::backup_dir(
            self.data_dir.as_ref()?,
            self.project.root.as_deref(),
        ))
    }

    /// The window's tabs and layout, as kept between runs. Only files on
    /// disk are kept as tabs; untitled documents come back from backups.
    pub(super) fn window_state(&self) -> WindowState {
        let groups = self
            .groups
            .iter()
            .map(|group| {
                let mut active = 0;
                let mut tabs = Vec::new();
                for (index, tab) in group.tabs.iter().enumerate() {
                    let Some(path) = self
                        .documents
                        .get(&tab.uri)
                        .and_then(|document| document.disk_path())
                    else {
                        continue;
                    };
                    if index == group.active {
                        active = tabs.len();
                    }
                    tabs.push(TabState {
                        path: path.to_path_buf(),
                        pinned: tab.pinned,
                    });
                }
                GroupState { tabs, active }
            })
            .filter(|group| !group.tabs.is_empty())
            .collect();
        WindowState {
            groups,
            active_group: self.active,
            sidebar: self.layout.sidebar,
            panel: self.layout.panel,
            agent: self.layout.agent,
            side_view: self.layout.side_view.id().to_owned(),
            panel_view: self.layout.panel_view.id().to_owned(),
            expanded: self
                .tree
                .as_ref()
                .map(files::FileTree::expanded_folders)
                .unwrap_or_default(),
        }
    }

    pub(super) fn save_session_now(&self) {
        if let Some(file) = self.session_file()
            && let Err(error) = session::save_state(&file, &self.window_state())
        {
            eprintln!("keel: could not keep the session: {error}");
        }
    }

    /// Keeps the session a moment after tabs or layout changed.
    pub(super) fn session_changed(&mut self, cx: &mut Context<Self>) {
        if self.data_dir.is_none() || self.restoring {
            return;
        }
        self.session_task = cx.spawn(async move |this, cx| {
            cx.background_executor().timer(SESSION_DELAY).await;
            this.update(cx, |this, _| this.save_session_now()).ok();
        });
    }

    /// Writes the unsaved changes of every document, and removes the
    /// backups of documents that have none any more.
    pub(super) fn write_backups_now(&self) {
        let Some(folder) = self.backup_folder() else {
            return;
        };
        for document in self.documents.values() {
            let path = match &document.backing {
                Backing::File(file) => Some(file.path().to_path_buf()),
                Backing::Untitled => None,
                Backing::Extension | Backing::Image(_) => continue,
            };
            if !document.modified {
                session::remove_backup(&folder, &document.uri);
                continue;
            }
            let backup = Backup {
                uri: document.uri.clone(),
                path,
                language_id: document.language_id.clone(),
                text: document.synced_text.clone(),
            };
            if let Err(error) = session::write_backup(&folder, &backup) {
                eprintln!("keel: could not back up {}: {error}", document.uri);
            }
        }
    }

    /// Writes the backups a moment after the last change.
    pub(super) fn backups_changed(&mut self, cx: &mut Context<Self>) {
        if self.data_dir.is_none() || self.restoring {
            return;
        }
        self.backup_task = cx.spawn(async move |this, cx| {
            cx.background_executor().timer(BACKUP_DELAY).await;
            this.update(cx, |this, _| this.write_backups_now()).ok();
        });
    }

    /// Drops the backup of a document that was saved or discarded.
    pub(super) fn forget_backup(&self, uri: &str) {
        if let Some(folder) = self.backup_folder() {
            session::remove_backup(&folder, uri);
        }
    }

    /// Drops every backup of the window, after "Don't Save".
    pub(super) fn discard_backups(&self) {
        if let Some(folder) = self.backup_folder() {
            for backup in session::read_backups(&folder) {
                session::remove_backup(&folder, &backup.uri);
            }
        }
    }

    /// Keeps everything now: the session and the unsaved changes.
    pub(crate) fn persist_now(&self) {
        self.save_session_now();
        self.write_backups_now();
    }

    /// Brings back the project's tabs and layout, then the unsaved changes.
    pub(super) fn restore(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.restoring = true;
        if let Some(state) = self
            .session_file()
            .and_then(|file| session::load_state(&file))
        {
            self.restore_state(&state, window, cx);
        }
        if let Some(folder) = self.backup_folder() {
            for backup in session::read_backups(&folder) {
                self.restore_backup(backup, window, cx);
            }
        }
        self.restoring = false;
    }

    fn restore_state(&mut self, state: &WindowState, window: &mut Window, cx: &mut Context<Self>) {
        self.layout.sidebar = state.sidebar;
        self.layout.panel = state.panel;
        self.layout.agent = state.agent;
        if let Some(view) = SideView::from_id(&state.side_view) {
            self.layout.side_view = view;
        }
        if let Some(view) = PanelView::from_id(&state.panel_view) {
            self.layout.panel_view = view;
        }
        if let Some(tree) = &mut self.tree {
            for folder in state.expanded.iter().filter(|folder| folder.is_dir()) {
                tree.expand(folder);
            }
        }
        self.tree_changed();
        for group in &state.groups {
            let column = self.groups.len();
            for tab in group.tabs.iter().filter(|tab| tab.path.is_file()) {
                let Ok(uri) = self.load_document(&tab.path) else {
                    continue;
                };
                self.push_tab(column, &uri, tab.pinned, window, cx);
            }
            if column < self.groups.len() {
                let active = group.active.min(self.groups[column].tabs.len() - 1);
                self.activate_tab(column, active, false, window, cx);
            }
        }
        if !self.panes.is_empty() {
            self.active = state.active_group.min(self.panes.len() - 1);
            self.focus_pane(self.active, window, cx);
        }
    }

    fn restore_backup(&mut self, backup: Backup, window: &mut Window, cx: &mut Context<Self>) {
        let uri = match &backup.path {
            Some(path) => {
                let uri = Uri::file(path).to_string();
                if !self.documents.contains_key(&uri) {
                    match Document::open(path) {
                        Ok(document) => {
                            self.add_file_document(document);
                        }
                        Err(_) => return,
                    }
                }
                uri
            }
            None => {
                if !self.documents.contains_key(&backup.uri) {
                    let number = backup
                        .uri
                        .strip_prefix("untitled:Untitled-")
                        .and_then(|number| number.parse::<u64>().ok());
                    if let Some(number) = number {
                        self.next_untitled = self.next_untitled.max(number + 1);
                    }
                    self.add_document(
                        backup.uri.clone(),
                        Backing::Untitled,
                        backup.language_id.clone(),
                        String::new(),
                    );
                }
                backup.uri.clone()
            }
        };
        if self.find_tab(&uri, self.active).is_none() {
            let column = self.active.min(self.groups.len().saturating_sub(1));
            self.push_tab(column, &uri, false, window, cx);
        }
        self.set_document_text(&uri, &backup.text, window, cx);
    }

    /// Puts text into a document and every editor showing it, as if typed.
    fn set_document_text(
        &mut self,
        uri: &str,
        text: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(document) = self.documents.get_mut(uri) else {
            return;
        };
        document.modified = match &document.backing {
            Backing::File(file) => file.is_modified(text),
            Backing::Untitled => !text.is_empty(),
            Backing::Extension | Backing::Image(_) => false,
        };
        document.synced_text = text.to_owned();
        document.version += 1;
        let params = json!({
            "uri": uri,
            "version": document.version,
            "text": text,
            "isDirty": document.modified,
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
            tabs::replace_text(&editor, text, window, cx);
        }
        self.refresh_title(window);
    }

    // -----------------------------------------------------------------
    // Saving by itself

    fn auto_save_mode(&self) -> &str {
        self.settings.string("files.autoSave").unwrap_or("off")
    }

    /// After a change: with `files.autoSave` set to `afterDelay`, saves a
    /// moment later (`files.autoSaveDelay`, 1000 ms).
    pub(super) fn auto_save_after_change(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.auto_save_mode() != "afterDelay" {
            return;
        }
        let delay = self
            .settings
            .number("files.autoSaveDelay")
            .map_or(1000, |delay| delay.max(0.) as u64);
        self.autosave_task = cx.spawn_in(window, async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(delay))
                .await;
            this.update_in(cx, |this, window, cx| this.save_all_quietly(window, cx))
                .ok();
        });
    }

    /// With `onFocusChange`, saves when another editor takes the focus;
    /// `onWindowChange` saves when Keel goes to the background, as does
    /// `onFocusChange`.
    pub(super) fn auto_save_on(
        &mut self,
        window_changed: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let mode = self.auto_save_mode();
        if mode == "onFocusChange" || (window_changed && mode == "onWindowChange") {
            self.save_all_quietly(window, cx);
        }
    }

    /// Saves every file with changes, except those that also changed on
    /// disk, which wait for a save that asks.
    fn save_all_quietly(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let uris: Vec<String> = self
            .documents
            .values()
            .filter(|document| document.modified)
            .filter(|document| match &document.backing {
                Backing::File(file) => file.changed_on_disk().is_none(),
                _ => false,
            })
            .map(|document| document.uri.clone())
            .collect();
        for uri in uris {
            self.save_file(&uri, None, window, cx);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;

    use gpui::{TestAppContext, VisualTestContext};

    use super::*;

    fn set_up(cx: &mut TestAppContext) {
        cx.update(|cx| {
            gpui_component::init(cx);
            language::register_grammars();
            crate::workbench_window::init(cx);
        });
    }

    struct Project {
        _folder: tempfile::TempDir,
        root: PathBuf,
        data: PathBuf,
        settings: PathBuf,
    }

    fn project(settings: &str) -> Project {
        let folder = tempfile::tempdir().unwrap();
        let base = fs::canonicalize(folder.path()).unwrap();
        let root = base.join("Keel");
        fs::create_dir_all(root.join("src")).unwrap();
        for name in ["a", "b", "c"] {
            fs::write(root.join(format!("src/{name}.rs")), format!("// {name}\n")).unwrap();
        }
        let settings_file = base.join("settings.json");
        fs::write(&settings_file, settings).unwrap();
        Project {
            _folder: folder,
            root,
            data: base.join("data"),
            settings: settings_file,
        }
    }

    fn open_window<'a>(
        cx: &'a mut TestAppContext,
        project: &Project,
    ) -> (Entity<Workbench>, &'a mut VisualTestContext) {
        let settings = Settings::load(&project.settings, None);
        let opening =
            Opening::new(Some(project.root.clone()), None).with_data_dir(project.data.clone());
        cx.add_window_view(|window, cx| Workbench::new(opening, settings, None, window, cx))
    }

    fn open(view: &Entity<Workbench>, cx: &mut VisualTestContext, path: &Path) {
        view.update_in(cx, |view, window, cx| view.open_file(path, window, cx));
    }

    #[gpui::test]
    fn tabs_layout_and_folders_come_back(cx: &mut TestAppContext) {
        set_up(cx);
        let project = project(r#"{ "keel.language": "en" }"#);
        let (view, window) = open_window(cx, &project);
        open(&view, window, &project.root.join("src/a.rs"));
        open(&view, window, &project.root.join("src/b.rs"));
        view.update_in(window, |view, window, cx| {
            view.run_tab_command(
                &tabs::TabAction {
                    group: 0,
                    tab: 1,
                    command: tabs::TabCommand::TogglePin,
                },
                window,
                cx,
            );
            view.layout.agent = false;
        });
        window.simulate_keystrokes("cmd-\\");
        let before = view.read_with(window, |view, _| view.window_state());
        view.read_with(window, |view, _| view.persist_now());

        let (again, window) = open_window(cx, &project);
        let after = again.read_with(window, |view, _| view.window_state());
        assert_eq!(after, before);
        assert_eq!(after.groups.len(), 2);
        assert!(after.groups[0].tabs[0].pinned);
        assert!(!after.agent);
        assert!(after.expanded.contains(&project.root.join("src")));
    }

    #[gpui::test]
    fn unsaved_changes_come_back_after_a_crash(cx: &mut TestAppContext) {
        set_up(cx);
        let project = project(r#"{ "keel.language": "en" }"#);
        let (view, window) = open_window(cx, &project);
        open(&view, window, &project.root.join("src/a.rs"));
        window.simulate_input("fn kept() {}\n");
        window.simulate_keystrokes("cmd-n");
        window.simulate_input("appunti");
        // The backups are written a moment after typing.
        window.executor().advance_clock(BACKUP_DELAY * 2);
        window.run_until_parked();

        let (again, window) = open_window(cx, &project);
        let restored = again.read_with(window, |view, _| {
            let mut texts: Vec<(String, bool)> = view
                .documents
                .values()
                .map(|document| (document.synced_text.clone(), document.modified))
                .collect();
            texts.sort();
            texts
        });
        assert_eq!(
            restored,
            [
                ("appunti".to_owned(), true),
                ("fn kept() {}\n// a\n".to_owned(), true)
            ]
        );

        // Saving drops the backup.
        again.update_in(window, |view, window, cx| {
            let uri = Uri::file(&project.root.join("src/a.rs")).to_string();
            view.save_document(&uri, None, window, cx);
        });
        let folder = session::backup_dir(&project.data, Some(&project.root));
        assert_eq!(session::read_backups(&folder).len(), 1);
        assert_eq!(
            fs::read_to_string(project.root.join("src/a.rs")).unwrap(),
            "fn kept() {}\n// a\n"
        );
    }

    #[gpui::test]
    fn files_save_themselves_after_a_delay(cx: &mut TestAppContext) {
        set_up(cx);
        let project = project(
            r#"{ "keel.language": "en", "files.autoSave": "afterDelay", "files.autoSaveDelay": 200 }"#,
        );
        let (view, window) = open_window(cx, &project);
        open(&view, window, &project.root.join("src/c.rs"));
        window.simulate_input("x");
        assert_eq!(
            fs::read_to_string(project.root.join("src/c.rs")).unwrap(),
            "// c\n"
        );
        window.executor().advance_clock(Duration::from_millis(300));
        window.run_until_parked();
        assert_eq!(
            fs::read_to_string(project.root.join("src/c.rs")).unwrap(),
            "x// c\n"
        );
        assert!(!view.read_with(window, |view, _| view.has_unsaved_changes()));
    }
}
