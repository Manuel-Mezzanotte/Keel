//! The workbench: the window with its panels and editors, and the bridge
//! between Keel and the extension host. The layout is drawn in `shell`.

mod claude_code;
mod diff_view;
mod disk;
mod editing;
pub(crate) mod explorer;
mod navigation;
mod npm_scripts;
mod other_files;
mod panel_views;
mod restore;
mod search_view;
mod shell;
mod source_control;
mod tabs;
mod terminal_view;
mod terminals;

pub(crate) use editing::{
    CopyLinesDown, CopyLinesUp, DeleteLines, EDITOR_CONTEXT, InsertLineAfter, InsertLineBefore,
    MoveLinesDown, MoveLinesUp, ToggleBlockComment, ToggleLineComment, ToggleWordWrap, ZoomIn,
    ZoomOut, ZoomReset,
};
pub(crate) use navigation::{GoToLine, QuickOpen};

pub(crate) use tabs::{
    CloseAllEditors, CloseEditor, CloseEditorsInGroup, CloseOtherEditors, FocusGroup1, FocusGroup2,
    FocusGroup3, KeepEditor, NextEditor, PreviousEditor, ReopenClosedEditor, SplitEditor,
    TogglePinEditor,
};
use tabs::{Group, OpenAs};

pub(crate) use claude_code::{OpenClaudeCode, OpenClaudeCodeInPanel};
pub(crate) use npm_scripts::RunNpmScript;
pub(crate) use shell::window_options;
pub(crate) use source_control::{
    CheckoutBranch, Commit, CommitAmend, CreateBranch, DiscardAll, GitFetch, GitPull, GitPush,
    GitRefresh, GitStash, GitStashPop, GitSync, StageAll, UnstageAll,
    bindings as source_control_bindings,
};
pub(crate) use terminal_view::{TERMINAL_CONTEXT, bindings as terminal_bindings};
pub(crate) use terminals::{
    FocusNextTerminal, FocusPreviousTerminal, KillTerminal, NewTerminal, RenameTerminal,
    SplitTerminal, ToggleMaximizedPanel,
};

use std::collections::{BTreeMap, HashMap, HashSet};
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::Duration;

use editor::Document;
use extensions::{ExtensionHost, HostEvent, InstalledExtension};
use futures::StreamExt as _;
use futures::channel::mpsc::UnboundedReceiver;
use gpui::prelude::FluentBuilder as _;
use gpui::{
    App, AppContext as _, Context, Entity, EntityInputHandler as _, Focusable as _, FontStyle,
    FontWeight, HighlightStyle, Hsla, InteractiveElement as _, IntoElement, Keystroke,
    KeystrokeEvent, ParentElement as _, PromptLevel, Rgba, Styled as _, Subscription, Task, Window,
    actions, div, px,
};
use gpui_component::input::{
    CompletionProvider, Editor, EditorMode, EditorState, GutterDecoration,
    GutterDecorationCollection, InputEvent, InputModeKind as _, Redo, Rope, TextDecoration,
    TextDecorationCollection, Undo,
};
use gpui_component::{ActiveTheme as _, WindowExt as _};
use language::Language;
use lsp_types::{CompletionContext, CompletionItem, CompletionItemKind, CompletionResponse};
use serde_json::{Value, json};
use settings::Settings;
use workbench::position::{self, Position};
use workbench::uri::Uri;
use workbench::when_clause::{Context as WhenContext, WhenClause};

use crate::i18n::{Strings, UiLanguage};
use crate::layout::{Layout, PanelView, SideView};
use crate::paths;
use crate::project::Project;
use crate::quick_pick::{self, PickItem};

actions!(
    keel,
    [
        Save,
        CloseWindow,
        Quit,
        ShowCommands,
        SelectTheme,
        SelectLanguage,
        ToggleSidebar,
        TogglePanel,
        ToggleAgent,
        ShowExplorer,
        ShowSearch,
        ShowSourceControl,
        ShowExtensions,
        ToggleProblems,
        ToggleOutput,
        ToggleTerminal,
        NewFile,
        NewWindow,
        Open,
        OpenRecent
    ]
);

/// Opens a project folder, from File → Open Recent and the welcome screen.
#[derive(Clone, Debug, PartialEq, gpui::Action)]
#[action(namespace = keel, no_json)]
pub struct OpenProject {
    pub path: PathBuf,
}

/// What a new window shows: a project folder, a file, both or neither.
pub struct Opening {
    root: Option<PathBuf>,
    file: Option<PathBuf>,
    document: Option<Document>,
    /// Where the session and the backups live; none keeps nothing.
    data_dir: Option<PathBuf>,
}

impl Opening {
    pub fn new(root: Option<PathBuf>, file: Option<PathBuf>) -> Self {
        Self {
            root,
            file,
            document: None,
            data_dir: None,
        }
    }

    /// Keeps the window's session and unsaved changes in `data_dir`, and
    /// brings them back.
    pub fn with_data_dir(mut self, data_dir: PathBuf) -> Self {
        self.data_dir = Some(data_dir);
        self
    }
}

impl From<Option<Document>> for Opening {
    /// A document already read, in the project that holds it.
    fn from(document: Option<Document>) -> Self {
        let root = document
            .as_ref()
            .and_then(|document| std::path::absolute(document.path()).ok())
            .and_then(|path| settings::project_root_for(&path));
        Self {
            root,
            file: None,
            document,
            data_dir: None,
        }
    }
}

/// The most lines the Output view keeps.
const OUTPUT_LINES: usize = 1000;

/// Registers the workbench with Keel's default key bindings.
#[cfg(test)]
pub fn init(cx: &mut App) {
    init_with_user_bindings(&[], cx);
}

/// Registers the workbench, with the user's `keybindings.json` applied on
/// top of Keel's defaults. Returns problems found in the user's bindings.
pub fn init_with_user_bindings(user: &[crate::keymap::UserBinding], cx: &mut App) -> Vec<String> {
    let warnings = crate::keymap::bind(user, cx);
    // Reached only when no window handles Quit, so nothing is left unsaved.
    cx.on_action(|_: &Quit, cx| cx.quit());
    warnings
}

/// Everything the workbench needs to run extensions.
pub struct ExtensionSetup {
    /// The user's `keybindings.json` entries, some of which may be for
    /// extension commands.
    pub user_bindings: Vec<crate::keymap::UserBinding>,
    pub host: ExtensionHost,
    pub events: UnboundedReceiver<HostEvent>,
    pub installed: Vec<InstalledExtension>,
    pub data_dir: PathBuf,
}

/// How a document's text is stored.
enum Backing {
    File(Document),
    Untitled,
    /// Owned by an extension's file system, such as `vsauto-ghost:`.
    Extension,
    /// An image shown in a preview; extensions never see it.
    Image(PathBuf),
    /// A file's differences, drawn by gpui-component's diff view.
    Diff(diff_view::DiffTab),
}

impl OpenDocument {
    /// The file on disk behind the document, for files and images.
    fn disk_path(&self) -> Option<&Path> {
        match &self.backing {
            Backing::File(file) => Some(file.path()),
            Backing::Image(path) => Some(path),
            Backing::Diff(_) => None,
            Backing::Untitled | Backing::Extension => None,
        }
    }
}

struct OpenDocument {
    uri: String,
    backing: Backing,
    language_id: String,
    version: u64,
    /// The text extensions were last told about.
    synced_text: String,
    modified: bool,
}

impl OpenDocument {
    fn name(&self) -> String {
        if let Backing::Diff(tab) = &self.backing {
            return tab.title.clone();
        }
        Uri::parse(&self.uri)
            .map(|uri| uri.file_name().to_owned())
            .unwrap_or_else(|| self.uri.clone())
    }
}

struct Pane {
    editor_id: String,
    uri: String,
    editor: Entity<EditorState>,
    decorations: HashMap<String, TextDecorationCollection>,
    gutter: Option<GutterDecorationCollection>,
    _subscriptions: Vec<Subscription>,
}

/// A key binding contributed by an extension.
struct Binding {
    keystroke: BindingKeystroke,
    command: String,
    when: WhenClause,
    args: Option<Value>,
}

struct StatusItem {
    text: String,
    right: bool,
    priority: f64,
    visible: bool,
}

struct Extensions {
    host: Rc<ExtensionHost>,
    /// Commands registered by running extensions.
    commands: HashSet<String>,
    /// Commands declared in `package.json`, with their titles, for the palette.
    titles: Vec<(String, String)>,
    bindings: Vec<Binding>,
    decoration_types: HashMap<String, Value>,
    completion_providers: Vec<(u64, Value, Vec<String>)>,
    file_systems: HashSet<String>,
    content_providers: HashSet<String>,
    quick_diff: bool,
    status_items: BTreeMap<String, StatusItem>,
}

/// An installed extension, as the Extensions view lists it.
struct ExtensionSummary {
    name: String,
    version: String,
    id: String,
}

/// What to do once the user has dealt with unsaved changes.
#[derive(Clone, Copy)]
enum AfterConfirm {
    CloseWindow,
    QuitApp,
}

pub struct Workbench {
    settings: Settings,
    language: UiLanguage,
    /// The editor groups, one per pane, with their tabs.
    groups: Vec<Group>,
    /// Files closed lately, for Reopen Closed Editor.
    closed: Vec<String>,
    /// The Search view's fields, switches and results.
    search: search_view::SearchView,
    /// Very large files the user agreed to open.
    large_files_ok: other_files::LargeFilesOk,
    /// Where the session and the backups are kept, if anywhere.
    data_dir: Option<PathBuf>,
    /// True while the session comes back, so it is not written meanwhile.
    restoring: bool,
    session_task: Task<()>,
    backup_task: Task<()>,
    autosave_task: Task<()>,
    /// Word wrap in every editor, from `editor.wordWrap` and Alt+Z.
    word_wrap: bool,
    /// The editors' font size, from `editor.fontSize` and the zoom keys.
    font_size: f32,
    project: Project,
    layout: Layout,
    installed: Vec<ExtensionSummary>,
    /// Keel's messages for the Output view: problems with the settings,
    /// extension logs and errors.
    output: Vec<(panel_views::OutputChannel, String)>,
    /// The channel the Output view shows.
    output_channel: panel_views::OutputChannel,
    /// The ports the terminals' programs listen on, looked up every few
    /// seconds while terminals are open.
    ports: Vec<terminal::ListeningPort>,
    watching_ports: bool,
    ports_task: Task<()>,
    documents: HashMap<String, OpenDocument>,
    panes: Vec<Pane>,
    active: usize,
    next_editor: u64,
    next_untitled: u64,
    extensions: Option<Extensions>,
    context: WhenContext,
    status_message: Option<String>,
    discard_changes: bool,
    gutter_refresh: Task<()>,
    /// Whether the first frame was already drawn, for `KEEL_STARTUP_TIMING`.
    first_frame_drawn: bool,
    /// The selections extensions believe each editor has, by editor id.
    known_selections: HashMap<String, Value>,
    /// Holds the keyboard when no editor does, so shortcuts work on the
    /// welcome screen.
    focus_handle: gpui::FocusHandle,
    /// The Explorer: the project's tree, its rows as shown, the selected
    /// file or folder, and the name field when creating or renaming.
    tree: Option<files::FileTree>,
    tree_rows: Vec<files::Row>,
    tree_selection: Option<PathBuf>,
    tree_edit: Option<explorer::TreeEdit>,
    tree_focus: gpui::FocusHandle,
    /// Watches the project for changes made outside Keel.
    watcher: Option<files::ProjectWatcher>,
    watching: Task<()>,
    /// The project's files for Quick Open, listed in the background.
    file_index: Option<std::sync::Arc<Vec<String>>>,
    indexing: Task<()>,
    /// The terminals of the bottom panel, and the one in front.
    terminals: Vec<terminals::TerminalTab>,
    active_terminal: usize,
    next_terminal_group: usize,
    /// The bottom panel's height, and whether it takes the editors' room.
    panel_height: f32,
    panel_maximized: bool,
    /// Claude Code's panel on the right: its width and its terminal.
    agent_width: f32,
    agent_terminal: Option<claude_code::AgentTerminal>,
    /// Differences side by side, from `diffEditor.renderSideBySide`.
    diff_split: bool,
    /// The project's git repository and the Source Control view.
    scm: source_control::SourceControl,
    /// The scripts of the project's `package.json`.
    npm_scripts: Option<npm_scripts::NpmScripts>,
    _subscriptions: Vec<Subscription>,
}

impl Workbench {
    pub fn new(
        opening: impl Into<Opening>,
        settings: Settings,
        extensions: Option<ExtensionSetup>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut subscriptions = Vec::new();
        let this = cx.weak_entity();
        subscriptions.push(cx.intercept_keystrokes({
            let this = this.clone();
            move |event, window, cx| {
                let consumed = this
                    .update(cx, |this, cx| this.intercept_keystroke(event, window, cx))
                    .unwrap_or(false);
                if consumed {
                    cx.stop_propagation();
                }
            }
        }));
        subscriptions.push(cx.observe_window_activation(window, |this, window, cx| {
            // Leaving Keel keeps everything, and may save files.
            if !window.is_window_active() {
                this.auto_save_on(true, window, cx);
                this.persist_now();
            }
            // Files may have changed in another app, such as Claude Code.
            if window.is_window_active() {
                this.project.refresh_branch();
                this.refresh_git(cx);
                this.refresh_tree(cx);
                this.index_files(cx);
            }
        }));
        subscriptions.push(cx.observe_window_appearance(window, |this, window, cx| {
            let choice = crate::theme::ThemeChoice::from_settings(&this.settings);
            if choice.follow_system {
                crate::theme::apply(&choice, Some(window), cx);
            }
        }));
        window.on_window_should_close(cx, {
            let this = this.clone();
            move |window, cx| {
                this.update(cx, |this, cx| {
                    let close = this.confirm(AfterConfirm::CloseWindow, window, cx);
                    if close {
                        this.before_close(cx);
                    }
                    close
                })
                .unwrap_or(true)
            }
        });

        let status_message = settings.warnings.first().cloned();
        let output = settings
            .warnings
            .iter()
            .map(|warning| (panel_views::OutputChannel::Keel, warning.clone()))
            .collect();
        let language = UiLanguage::from_setting(
            settings.string("keel.language"),
            UiLanguage::system_locale().as_deref(),
        );
        let opening = opening.into();
        let project = Project::new(opening.root.clone());
        let word_wrap = settings
            .string("editor.wordWrap")
            .is_some_and(|wrap| wrap != "off");
        let font_size = editing::font_size_setting(&settings);
        let scm = source_control::SourceControl::new(
            project.root.as_deref(),
            language.strings(),
            window,
            cx,
        );
        let mut workbench = Self {
            settings,
            language,
            groups: Vec::new(),
            closed: Vec::new(),
            large_files_ok: Default::default(),
            search: Self::create_search_view(window, cx, language.strings()),
            data_dir: None,
            restoring: false,
            session_task: Task::ready(()),
            backup_task: Task::ready(()),
            autosave_task: Task::ready(()),
            word_wrap,
            font_size,
            project,
            layout: Layout::default(),
            installed: Vec::new(),
            output,
            output_channel: panel_views::OutputChannel::Keel,
            ports: Vec::new(),
            watching_ports: false,
            ports_task: Task::ready(()),
            documents: HashMap::new(),
            panes: Vec::new(),
            active: 0,
            next_editor: 1,
            next_untitled: 1,
            extensions: None,
            context: WhenContext::default(),
            status_message,
            discard_changes: false,
            gutter_refresh: Task::ready(()),
            known_selections: HashMap::new(),
            focus_handle: cx.focus_handle(),
            tree: None,
            tree_rows: Vec::new(),
            tree_selection: None,
            tree_edit: None,
            tree_focus: cx.focus_handle(),
            watcher: None,
            watching: Task::ready(()),
            file_index: None,
            indexing: Task::ready(()),
            terminals: Vec::new(),
            active_terminal: 0,
            next_terminal_group: 0,
            panel_height: terminals::PANEL_HEIGHT,
            panel_maximized: false,
            agent_width: claude_code::AGENT_WIDTH,
            agent_terminal: None,
            npm_scripts: None,
            scm,
            diff_split: true,
            first_frame_drawn: false,
            _subscriptions: subscriptions,
        };
        workbench.build_tree();
        workbench.refresh_npm_scripts();
        workbench.diff_split = workbench
            .settings
            .get("diffEditor.renderSideBySide")
            .and_then(Value::as_bool)
            .unwrap_or(true);
        workbench.refresh_git(cx);
        workbench.index_files(cx);
        workbench.watch_project(window, cx);
        workbench.data_dir = opening.data_dir.clone();
        workbench.restore(window, cx);
        if let Some(setup) = extensions {
            workbench.start_extensions(setup, window, cx);
        }
        if let Some(document) = opening.document {
            let uri = workbench.add_file_document(document);
            workbench.open_in_pane(&uri, 0, true, window, cx);
        } else if let Some(file) = opening.file {
            workbench.open_file(&file, window, cx);
        }
        if workbench.panes.is_empty() {
            workbench.focus_handle.focus(window, cx);
        }
        workbench.refresh_title(window);
        workbench
    }

    /// Whether the window shows nothing: no project and no files.
    pub fn is_blank(&self) -> bool {
        self.project.root.is_none() && self.documents.is_empty()
    }

    /// Opens a file in the active pane, or brings it forward if a pane
    /// already shows it. A missing file opens empty and is created on save.
    pub fn open_file(&mut self, path: &Path, window: &mut Window, cx: &mut Context<Self>) {
        self.open_file_as(path, OpenAs::Normal, window, cx);
    }

    /// Opens a file in the active group, in a preview tab or a normal one.
    fn open_file_as(
        &mut self,
        path: &Path,
        open_as: OpenAs,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let path = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
        let uri = Uri::file(&path).to_string();
        if !self.documents.contains_key(&uri) {
            if !self.large_file_ok(&path, open_as, window, cx) {
                return;
            }
            if let Err(message) = self.load_document(&path) {
                self.report(message);
                cx.notify();
                return;
            }
        }
        let column = self.active.min(self.panes.len().saturating_sub(1));
        self.show_tab(&uri, column, open_as, true, window, cx);
    }

    /// Opens a new untitled document in the active pane.
    pub fn new_file(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let uri = self.add_untitled_document();
        let column = self.active.min(self.panes.len().saturating_sub(1));
        self.open_in_pane(&uri, column, true, window, cx);
    }

    /// Lets the user pick a recent project and opens it.
    fn choose_recent(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let projects = crate::recent::projects(cx);
        let strings = self.strings();
        if projects.is_empty() {
            self.status_message = Some(strings.no_recent_projects.to_owned());
            cx.notify();
            return;
        }
        let items = projects
            .iter()
            .map(|project| {
                let name = project
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_else(|| project.display().to_string());
                let mut item = PickItem::new(name);
                item.keywords.push(crate::recent::short_path(project));
                item
            })
            .collect();
        let title = Some(strings.open_recent_title.to_owned());
        let chosen = quick_pick::show(title, None, items, window, cx);
        cx.spawn_in(window, async move |_, cx| {
            if let Ok(Some(index)) = chosen.await {
                let path = projects[index].clone();
                let _ = cx.update(|_, cx| crate::windows::open(Some(path), cx));
            }
        })
        .detach();
    }

    fn strings(&self) -> &'static Strings {
        self.language.strings()
    }

    /// The indentation from `editor.tabSize` and `editor.insertSpaces`.
    fn tab_size(&self) -> gpui_component::input::TabSize {
        gpui_component::input::TabSize {
            tab_size: self
                .settings
                .number("editor.tabSize")
                .map_or(2, |size| size.clamp(1.0, 16.0) as usize),
            hard_tabs: self
                .settings
                .get("editor.insertSpaces")
                .and_then(Value::as_bool)
                == Some(false),
        }
    }

    /// Adds a line to the Output view.
    fn log(&mut self, line: impl Into<String>) {
        self.log_to(panel_views::OutputChannel::Keel, line);
    }

    /// Shows a problem in the status bar and keeps it in the Output view.
    fn report(&mut self, message: String) {
        self.log(message.clone());
        self.status_message = Some(message);
    }

    /// The name shown for a document: its file name, or "Untitled 1".
    fn display_name(&self, document: &OpenDocument) -> String {
        match document.uri.strip_prefix("untitled:Untitled-") {
            Some(number) => format!("{} {number}", self.strings().untitled),
            None => document.name(),
        }
    }

    // -----------------------------------------------------------------
    // Documents and panes

    fn add_file_document(&mut self, document: Document) -> String {
        let path =
            std::path::absolute(document.path()).unwrap_or_else(|_| document.path().to_path_buf());
        let uri = Uri::file(&path).to_string();
        let text = document.saved_text().to_owned();
        let language_id = language::vscode_id_for_path(&path).to_owned();
        self.add_document(uri, Backing::File(document), language_id, text)
    }

    fn add_untitled_document(&mut self) -> String {
        let uri = format!("untitled:Untitled-{}", self.next_untitled);
        self.next_untitled += 1;
        self.add_document(
            uri,
            Backing::Untitled,
            "plaintext".to_owned(),
            String::new(),
        )
    }

    fn add_document(
        &mut self,
        uri: String,
        backing: Backing,
        language_id: String,
        text: String,
    ) -> String {
        self.notify_host(
            "documents/opened",
            json!({ "uri": uri, "languageId": language_id, "version": 1, "text": text, "isDirty": false }),
        );
        self.documents.insert(
            uri.clone(),
            OpenDocument {
                uri: uri.clone(),
                backing,
                language_id,
                version: 1,
                synced_text: text,
                modified: false,
            },
        );
        uri
    }

    fn pane_index(&self, uri: &str) -> Option<usize> {
        self.panes.iter().position(|pane| pane.uri == uri)
    }

    fn pane_by_editor_id(&self, editor_id: &str) -> Option<usize> {
        self.panes
            .iter()
            .position(|pane| pane.editor_id == editor_id)
    }

    /// Shows a loaded document in group `column`, creating the group when
    /// needed. Returns the editor id extensions know it by.
    fn open_in_pane(
        &mut self,
        uri: &str,
        column: usize,
        focus: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> String {
        self.show_tab(uri, column, OpenAs::Normal, focus, window, cx)
    }

    fn focus_pane(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.active = index;
        let editor = self.panes[index].editor.clone();
        editor.update(cx, |editor, cx| editor.focus(window, cx));
        self.reveal_active_in_tree();
    }

    /// Closes the tab in view in group `index`, without asking.
    #[cfg(test)]
    fn close_pane(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(tab) = self.groups.get(index).map(|group| group.active) {
            self.close_tab_now(index, tab, window, cx);
        }
    }

    fn active_editor(&self) -> Option<Entity<EditorState>> {
        self.panes.get(self.active).map(|pane| pane.editor.clone())
    }

    fn active_editor_focused(&self, window: &Window, cx: &App) -> bool {
        self.panes
            .get(self.active)
            .is_some_and(|pane| pane.editor.read(cx).focus_handle(cx).is_focused(window))
    }

    /// Follows focus changes made by clicking into another pane.
    fn track_focus(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let focused = self
            .panes
            .iter()
            .position(|pane| pane.editor.read(cx).focus_handle(cx).is_focused(window));
        if let Some(index) = focused.filter(|index| *index != self.active) {
            self.auto_save_on(false, window, cx);
            self.active = index;
            self.reveal_active_in_tree();
            self.publish_editors(window, cx);
            self.refresh_title(window);
        }
    }

    /// Tells extensions about the new text of `uri`, unless they know it already.
    fn sync_document(&mut self, uri: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.sync_document_from(uri, None, window, cx);
    }

    /// Takes the text of the editor that changed, `source` or the first
    /// one showing `uri`, into its document, and into the other editors
    /// showing the same file side by side.
    fn sync_document_from(
        &mut self,
        uri: &str,
        source: Option<&str>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(index) = source
            .and_then(|id| self.pane_by_editor_id(id))
            .or_else(|| self.pane_index(uri))
        else {
            return;
        };
        // The editor's file may have been renamed since it was created.
        let uri = self.panes[index].uri.clone();
        let uri = uri.as_str();
        let text = self.panes[index].editor.read(cx).value().to_string();
        let Some(document) = self.documents.get_mut(uri) else {
            return;
        };
        if text == document.synced_text {
            return;
        }
        document.version += 1;
        document.modified = match &document.backing {
            Backing::File(file) => file.is_modified(&text),
            Backing::Untitled => !text.is_empty(),
            Backing::Extension | Backing::Image(_) | Backing::Diff(_) => false,
        };
        let params = json!({
            "uri": uri,
            "version": document.version,
            "text": text,
            "isDirty": document.modified,
        });
        document.synced_text = text.clone();
        let modified = document.modified;
        self.notify_host("documents/changed", params);
        self.backups_changed(cx);
        if modified {
            self.auto_save_after_change(window, cx);
        }
        // Editing a file keeps its preview tab.
        if modified {
            for tab in self
                .groups
                .iter_mut()
                .flat_map(|group| &mut group.tabs)
                .filter(|tab| tab.uri == uri)
            {
                tab.preview = false;
            }
        }
        let siblings: Vec<Entity<EditorState>> = self
            .panes
            .iter()
            .enumerate()
            .filter(|(other, pane)| *other != index && pane.uri == uri)
            .map(|(_, pane)| pane.editor.clone())
            .collect();
        for editor in siblings {
            tabs::replace_text(&editor, &text, window, cx);
        }
        self.schedule_gutter_refresh(window, cx);
        self.refresh_title(window);
        cx.notify();
    }

    fn selections_of(&self, index: usize, cx: &App) -> Value {
        let editor = self.panes[index].editor.read(cx);
        let text = editor.value().to_string();
        let range = editor.selected_range();
        let (anchor, active) = if editor.is_selection_reversed() {
            (range.end, range.start)
        } else {
            (range.start, range.end)
        };
        json!([{ "anchor": position::position_of(&text, anchor), "active": position::position_of(&text, active) }])
    }

    /// Tells extensions where an editor's cursor is, if they believe
    /// otherwise. Keel calls this after its own edits, before answering, and
    /// before running an extension command, since cursor moves by the user
    /// are not reported as they happen.
    fn publish_selection_if_changed(&mut self, index: usize, cx: &mut Context<Self>) {
        if self.extensions.is_none() || index >= self.panes.len() {
            return;
        }
        let current = self.selections_of(index, cx);
        let editor_id = self.panes[index].editor_id.clone();
        if self.known_selections.get(&editor_id) == Some(&current) {
            return;
        }
        self.notify_host(
            "editors/selectionChanged",
            json!({ "id": editor_id, "selections": current, "kind": null }),
        );
        self.known_selections.insert(editor_id, current);
    }

    /// Sends the visible editors and the active one to extensions.
    fn publish_editors(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        // Tabs changed: keep the session a moment later.
        self.session_changed(cx);
        if self.extensions.is_none() {
            return;
        }
        let editors: Vec<Value> = self
            .panes
            .iter()
            .enumerate()
            .filter(|(_, pane)| {
                !matches!(
                    self.documents
                        .get(&pane.uri)
                        .map(|document| &document.backing),
                    Some(Backing::Image(_) | Backing::Diff(_))
                )
            })
            .map(|(index, pane)| {
                json!({
                    "id": pane.editor_id,
                    "uri": pane.uri,
                    "viewColumn": index + 1,
                    "selections": self.selections_of(index, cx),
                })
            })
            .collect();
        for editor in &editors {
            if let Some(id) = editor["id"].as_str() {
                self.known_selections
                    .insert(id.to_owned(), editor["selections"].clone());
            }
        }
        let active = self
            .panes
            .get(self.active)
            .map(|pane| pane.editor_id.clone());
        self.notify_host(
            "editors/changed",
            json!({ "editors": editors, "activeId": active }),
        );
    }

    /// The window's title: the file and the project, as in VS Code.
    fn window_title(&self) -> String {
        let file = self
            .panes
            .get(self.active)
            .and_then(|pane| self.documents.get(&pane.uri))
            .map(|document| self.display_name(document));
        let project = self
            .project
            .root
            .as_ref()
            .map(|_| self.project.name.clone());
        match (file, project) {
            (Some(file), Some(project)) => format!("{file} — {project}"),
            (Some(name), None) | (None, Some(name)) => name,
            (None, None) => "Keel".to_owned(),
        }
    }

    fn refresh_title(&self, window: &mut Window) {
        window.set_window_title(&self.window_title());
        window.set_window_edited(self.has_unsaved_changes());
        let path = self
            .panes
            .get(self.active)
            .and_then(|pane| self.documents.get(&pane.uri))
            .and_then(|document| match &document.backing {
                Backing::File(file) => Some(file.path().to_path_buf()),
                _ => None,
            });
        window.set_document_path(path.as_deref());
    }

    fn has_unsaved_changes(&self) -> bool {
        self.documents.values().any(|document| {
            document.modified && matches!(document.backing, Backing::File(_) | Backing::Untitled)
        })
    }

    // -----------------------------------------------------------------
    // Saving, closing and quitting

    fn save(&mut self, _: &Save, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(uri) = self.panes.get(self.active).map(|pane| pane.uri.clone()) {
            self.save_document(&uri, None, window, cx);
        }
    }

    /// Saves a document, asking for a path if it is untitled, then runs
    /// `after` if the save went through.
    fn save_document(
        &mut self,
        uri: &str,
        after: Option<AfterConfirm>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(document) = self.documents.get(uri) else {
            return;
        };
        let text = document.synced_text.clone();
        match &document.backing {
            Backing::File(_) => self.save_file(uri, after, window, cx),
            Backing::Untitled => self.save_as(uri.to_owned(), text, after, window, cx),
            Backing::Image(_) | Backing::Diff(_) => {}
            Backing::Extension => {
                if let Some(extensions) = &self.extensions {
                    let write = extensions
                        .host
                        .call("fileSystem/writeFile", json!({ "uri": uri, "text": text }));
                    cx.spawn_in(window, async move |_, _| {
                        let _ = write.await;
                    })
                    .detach();
                }
            }
        }
    }

    fn save_as(
        &mut self,
        uri: String,
        text: String,
        after: Option<AfterConfirm>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let folder = self
            .project
            .root
            .clone()
            .or_else(|| std::env::current_dir().ok())
            .unwrap_or_else(|| PathBuf::from("/"));
        let chosen = cx.prompt_for_new_path(&folder, Some(self.strings().untitled_file_name));
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(path))) = chosen.await else {
                return;
            };
            this.update_in(cx, |this, window, cx| {
                let mut document = match Document::open(&path) {
                    Ok(document) => document,
                    Err(error) => {
                        let strings = this.strings();
                        let message = strings.could_not_use(&path.display().to_string());
                        show_error(&message, &error.to_string(), strings.ok, window, cx);
                        return;
                    }
                };
                if let Err(error) = document.save(&text) {
                    let strings = this.strings();
                    let message = strings.could_not_save(&document.file_name());
                    show_error(&message, &error.to_string(), strings.ok, window, cx);
                    return;
                }
                // The untitled buffer becomes the saved file, in the same tab.
                this.forget_backup(&uri);
                this.documents.remove(&uri);
                this.notify_host("documents/closed", json!({ "uri": uri }));
                let new_uri = this.add_file_document(document);
                this.retarget_uri(&uri, &new_uri);
                this.publish_editors(window, cx);
                this.refresh_title(window);
                cx.notify();
                if let Some(after) = after {
                    this.finish(after, window, cx);
                }
            })
            .ok();
        })
        .detach();
    }

    fn close_window(&mut self, _: &CloseWindow, window: &mut Window, cx: &mut Context<Self>) {
        if self.confirm(AfterConfirm::CloseWindow, window, cx) {
            self.before_close(cx);
            window.remove_window();
        }
    }

    /// Keeps the session before a window closes; unsaved changes the user
    /// chose not to save are dropped.
    fn before_close(&mut self, cx: &mut Context<Self>) {
        self.save_session_now();
        if self.discard_changes {
            self.discard_backups();
        } else {
            self.write_backups_now();
        }
        crate::windows::window_closing(cx.entity_id(), cx);
    }

    /// Quits. With `files.hotExit` on, as by default in VS Code, nothing is
    /// asked: unsaved changes are kept and come back with their windows.
    fn quit(&mut self, _: &Quit, window: &mut Window, cx: &mut Context<Self>) {
        let hot_exit = self.settings.string("files.hotExit") != Some("off");
        if hot_exit && self.data_dir.is_some() {
            self.persist_now();
            crate::windows::quit_keeping_everything(cx.entity_id(), cx);
            return;
        }
        if self.confirm(AfterConfirm::QuitApp, window, cx) {
            cx.quit();
        }
    }

    /// Returns `true` when it is safe to go ahead right away. Otherwise asks
    /// whether to save the changes, returns `false`, and finishes later.
    fn confirm(
        &mut self,
        after: AfterConfirm,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.discard_changes {
            return true;
        }
        let Some(uri) = self
            .documents
            .values()
            .find(|document| {
                document.modified
                    && matches!(document.backing, Backing::File(_) | Backing::Untitled)
            })
            .map(|document| document.uri.clone())
        else {
            return true;
        };
        let strings = self.strings();
        let message = strings.save_changes(&self.display_name(&self.documents[&uri]));
        let answer = window.prompt(
            PromptLevel::Warning,
            &message,
            Some(strings.changes_lost),
            &[strings.save, strings.dont_save, strings.cancel],
            cx,
        );
        cx.spawn_in(window, async move |this, cx| {
            let Ok(choice) = answer.await else {
                return;
            };
            this.update_in(cx, |this, window, cx| match choice {
                0 => this.save_document(&uri, Some(after), window, cx),
                1 => {
                    this.discard_changes = true;
                    this.finish(after, window, cx);
                }
                _ => {}
            })
            .ok();
        })
        .detach();
        false
    }

    fn finish(&mut self, after: AfterConfirm, window: &mut Window, cx: &mut Context<Self>) {
        // Another file may still have unsaved changes.
        if !self.confirm(after, window, cx) {
            return;
        }
        match after {
            AfterConfirm::CloseWindow => {
                self.before_close(cx);
                window.remove_window();
            }
            AfterConfirm::QuitApp => cx.quit(),
        }
    }

    // -----------------------------------------------------------------
    // Command palette

    fn show_commands(&mut self, _: &ShowCommands, window: &mut Window, cx: &mut Context<Self>) {
        let strings = self.strings();
        let mut entries: Vec<(String, String)> = crate::keymap::COMMANDS
            .iter()
            .filter_map(|command| {
                Some((
                    command.id.to_owned(),
                    strings.command_title(command.id)?.to_owned(),
                ))
            })
            .collect();
        if let Some(extensions) = &self.extensions {
            entries.extend(extensions.titles.iter().cloned());
        }
        let items = entries
            .iter()
            .map(|(command, title)| {
                let mut item = PickItem::new(title.clone());
                item.keywords.push(command.clone());
                item
            })
            .collect();
        let chosen = quick_pick::show(
            None,
            Some(strings.type_a_command.to_owned()),
            items,
            window,
            cx,
        );
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Some(index)) = chosen.await else {
                return;
            };
            let command = entries[index].0.clone();
            this.update_in(cx, |this, window, cx| {
                if let Some(pane) = this.panes.get(this.active) {
                    let editor = pane.editor.clone();
                    editor.update(cx, |editor, cx| editor.focus(window, cx));
                }
                match crate::keymap::COMMANDS
                    .iter()
                    .find(|keel| keel.id == command)
                {
                    Some(keel) => window.dispatch_action((keel.action)(), cx),
                    None => this.run_extension_command(command, Vec::new(), window, cx),
                }
            })
            .ok();
        })
        .detach();
    }

    fn choose_language(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let items = vec![PickItem::new("Italiano"), PickItem::new("English")];
        let title = Some(self.strings().choose_language.to_owned());
        let chosen = quick_pick::show(title, None, items, window, cx);
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Some(index)) = chosen.await else {
                return;
            };
            let language = if index == 0 {
                UiLanguage::Italian
            } else {
                UiLanguage::English
            };
            this.update_in(cx, |this, window, cx| {
                this.set_language(language, window, cx)
            })
            .ok();
        })
        .detach();
    }

    fn choose_theme(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let items = crate::theme::NAMES
            .iter()
            .map(|name| PickItem::new(*name))
            .collect();
        let title = Some(self.strings().choose_theme.to_owned());
        let chosen = quick_pick::show(title, None, items, window, cx);
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Some(index)) = chosen.await else {
                return;
            };
            this.update_in(cx, |this, window, cx| {
                this.set_theme(crate::theme::NAMES[index], window, cx)
            })
            .ok();
        })
        .detach();
    }

    /// Shows a theme at once and remembers it. When Keel follows macOS, the
    /// theme becomes the preferred one for its mode, light or dark.
    fn set_theme(&mut self, name: &str, window: &mut Window, cx: &mut Context<Self>) {
        let mut choice = crate::theme::ThemeChoice::from_settings(&self.settings);
        let key = if choice.follow_system {
            if crate::theme::is_dark(name) {
                choice.dark = name.to_owned();
                "workbench.preferredDarkColorTheme"
            } else {
                choice.light = name.to_owned();
                "workbench.preferredLightColorTheme"
            }
        } else {
            choice.theme = name.to_owned();
            "workbench.colorTheme"
        };
        if let Err(error) = self.settings.update_user(key, Some(json!(name))) {
            self.report(error.to_string());
        }
        crate::theme::apply(&choice, Some(window), cx);
        cx.notify();
    }

    /// Switches the interface language at once and remembers it.
    fn set_language(&mut self, language: UiLanguage, window: &mut Window, cx: &mut Context<Self>) {
        self.language = language;
        if let Err(error) = self
            .settings
            .update_user("keel.language", Some(json!(language.code())))
        {
            self.report(error.to_string());
        }
        self.notify_host("env/changed", json!({ "language": language.code() }));
        crate::menus::set(language, cx);
        self.refresh_title(window);
        cx.notify();
    }

    // -----------------------------------------------------------------
    // Extensions

    fn start_extensions(
        &mut self,
        setup: ExtensionSetup,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let ExtensionSetup {
            user_bindings,
            host,
            mut events,
            installed,
            data_dir,
        } = setup;
        let host = Rc::new(host);
        let mut titles = Vec::new();
        let mut bindings = Vec::new();
        self.installed = installed
            .iter()
            .map(|extension| ExtensionSummary {
                name: extension
                    .manifest
                    .display_name
                    .clone()
                    .unwrap_or_else(|| extension.manifest.name.clone()),
                version: extension.manifest.version.clone(),
                id: extension.id(),
            })
            .collect();
        for extension in &installed {
            for command in &extension.manifest.commands {
                let title = if command.title.is_empty() {
                    command.command.clone()
                } else {
                    command.title.clone()
                };
                titles.push((command.command.clone(), title));
            }
            bindings.extend(contributed_bindings(extension));
        }
        apply_user_bindings(&mut bindings, &user_bindings);
        // Extensions read the user's and the project's settings from the start.
        let _ = host.notify(
            "configuration/changed",
            json!({ "values": self.settings.merged() }),
        );
        let _ = host.notify("env/changed", json!({ "language": self.language.code() }));
        let folders: Vec<String> = self
            .project
            .root
            .iter()
            .map(|root| Uri::file(root).to_string())
            .collect();
        let _ = host.notify("workspace/foldersChanged", json!({ "folders": folders }));
        self.extensions = Some(Extensions {
            host: host.clone(),
            commands: HashSet::new(),
            titles,
            bindings,
            decoration_types: HashMap::new(),
            completion_providers: Vec::new(),
            file_systems: HashSet::new(),
            content_providers: HashSet::new(),
            quick_diff: false,
            status_items: BTreeMap::new(),
        });

        cx.spawn_in(window, async move |this, cx| {
            while let Some(event) = events.next().await {
                let handled =
                    this.update_in(cx, |this, window, cx| this.on_host_event(event, window, cx));
                if handled.is_err() {
                    break;
                }
            }
        })
        .detach();

        let loads: Vec<_> = installed
            .iter()
            .map(|extension| {
                let storage = paths::extension_storage_dir(&data_dir, &extension.id());
                let starts_now = extension
                    .manifest
                    .activation_events
                    .iter()
                    .any(|event| event == "*" || event == "onStartupFinished");
                (
                    extension.id(),
                    host.load_extension(extension, &storage),
                    starts_now.then(|| host.activate(&extension.id())),
                )
            })
            .collect();
        cx.spawn_in(window, async move |this, cx| {
            for (id, load, activation) in loads {
                let result = match load.await {
                    Ok(()) => match activation {
                        Some(activation) => activation.await,
                        None => Ok(()),
                    },
                    Err(error) => Err(error),
                };
                if let Err(error) = result {
                    this.update_in(cx, |this, _, cx| {
                        this.status_message =
                            Some(format!("{id}: {}", first_line(&error.to_string())));
                        cx.notify();
                    })
                    .ok();
                }
            }
        })
        .detach();
    }

    fn notify_host(&self, method: &str, params: Value) {
        if let Some(extensions) = &self.extensions {
            let _ = extensions.host.notify(method, params);
        }
    }

    fn reply(&self, id: u64, result: Result<Value, String>) {
        if let Some(extensions) = &self.extensions {
            let _ = extensions.host.reply(id, result);
        }
    }

    /// Runs an extension command, first telling extensions where the
    /// cursor is, since moving it is not reported as it happens.
    fn run_extension_command(
        &mut self,
        command: String,
        args: Vec<Value>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.extensions.is_none() {
            return;
        }
        self.publish_selection_if_changed(self.active, cx);
        let Some(extensions) = &self.extensions else {
            return;
        };
        let call = extensions.host.execute_command(&command, args);
        cx.spawn_in(window, async move |this, cx| {
            if let Err(error) = call.await {
                this.update_in(cx, |this, _, cx| {
                    this.status_message =
                        Some(format!("{command}: {}", first_line(&error.to_string())));
                    cx.notify();
                })
                .ok();
            }
        })
        .detach();
    }

    fn refresh_context(&mut self, window: &Window, cx: &App) {
        let focused = self.active_editor_focused(window, cx);
        let pane = self.panes.get(self.active);
        let document = pane.and_then(|pane| self.documents.get(&pane.uri));
        let uri = document.and_then(|document| Uri::parse(&document.uri));
        let menu_open =
            pane.is_some_and(|pane| EditorMode::is_context_menu_open(pane.editor.read(cx), cx));
        let context = &mut self.context;
        context.set("isMac", true);
        context.set("editorTextFocus", focused);
        context.set("editorFocus", focused);
        context.set("textInputFocus", focused);
        context.set("suggestWidgetVisible", menu_open);
        context.set(
            "editorLangId",
            document.map_or(String::new(), |document| document.language_id.clone()),
        );
        context.set(
            "resourceScheme",
            uri.as_ref().map_or(String::new(), |uri| uri.scheme.clone()),
        );
        let file_name = uri
            .as_ref()
            .map_or(String::new(), |uri| uri.file_name().to_owned());
        let extension = file_name
            .rfind('.')
            .map_or(String::new(), |dot| file_name[dot..].to_owned());
        context.set("resourceFilename", file_name);
        context.set("resourceExtname", extension);
    }

    /// Decides whether Keel or an extension handles a keystroke. Returns
    /// `true` when it must not reach the editor.
    fn intercept_keystroke(
        &mut self,
        event: &KeystrokeEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        // Typing `>` in JSX or HTML closes the tag, as in VS Code.
        let keystroke = &event.keystroke;
        if keystroke.key_char.as_deref() == Some(">")
            && !keystroke.modifiers.platform
            && !keystroke.modifiers.control
            && !keystroke.modifiers.alt
            && self.active_editor_focused(window, cx)
            && self.close_tag(window, cx)
        {
            return true;
        }
        // Escape leaves the Explorer's name field without changing anything.
        if self.tree_edit.is_some()
            && event.keystroke.key == "escape"
            && !event.keystroke.modifiers.modified()
        {
            self.finish_tree_edit(false, window, cx);
            return true;
        }
        if self.extensions.is_none() || window.has_active_dialog(cx) {
            return false;
        }
        self.track_focus(window, cx);
        self.refresh_context(window, cx);
        let keystroke = &event.keystroke;
        let Some(extensions) = &self.extensions else {
            return false;
        };
        // Like VS Code, the binding registered last wins.
        let binding = extensions.bindings.iter().rev().find(|binding| {
            same_keystroke(&binding.keystroke, keystroke) && binding.when.evaluate(&self.context)
        });
        if let Some(binding) = binding {
            let command = binding.command.clone();
            let args = binding.args.clone().into_iter().collect();
            self.run_extension_command(command, args, window, cx);
            return true;
        }
        if extensions.commands.contains("type")
            && self.active_editor_focused(window, cx)
            && let Some(text) = typed_text(keystroke)
        {
            self.run_extension_command(
                "type".to_owned(),
                vec![json!({ "text": text })],
                window,
                cx,
            );
            return true;
        }
        false
    }

    fn on_host_event(&mut self, event: HostEvent, window: &mut Window, cx: &mut Context<Self>) {
        match event {
            HostEvent::Notify { method, params } => self.on_notify(&method, params, window, cx),
            HostEvent::Call { id, method, params } => self.on_call(id, &method, params, window, cx),
        }
    }

    fn on_notify(
        &mut self,
        method: &str,
        params: Value,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(extensions) = self.extensions.as_mut() else {
            return;
        };
        match method {
            "log" => {
                let line = format!(
                    "[{}] {}",
                    params["level"].as_str().unwrap_or("info"),
                    params["message"].as_str().unwrap_or_default()
                );
                eprintln!("keel: extension {line}");
                self.log_to(panel_views::OutputChannel::Extensions, line);
            }
            "commands/registered" => {
                if let Some(command) = params["command"].as_str() {
                    extensions.commands.insert(command.to_owned());
                }
            }
            "commands/unregistered" => {
                if let Some(command) = params["command"].as_str() {
                    extensions.commands.remove(command);
                }
            }
            "extensions/activationFailed" => {
                let id = params["id"].as_str().unwrap_or_default();
                let error = first_line(params["error"].as_str().unwrap_or_default());
                let message = self.strings().could_not_start(id, error);
                self.report(message);
            }
            "decorations/createType" => {
                if let Some(key) = params["key"].as_str() {
                    extensions
                        .decoration_types
                        .insert(key.to_owned(), params["options"].clone());
                }
            }
            "decorations/disposeType" => {
                if let Some(key) = params["key"].as_str() {
                    extensions.decoration_types.remove(key);
                    for pane in &mut self.panes {
                        if let Some(collection) = pane.decorations.remove(key) {
                            collection.clear(cx);
                        }
                    }
                }
            }
            "editor/setDecorations" => self.set_decorations(&params, cx),
            "editor/setSelections" => self.set_selections(&params, cx),
            "editor/revealRange" => self.reveal_range(&params, window, cx),
            "fileSystem/registered" => {
                if let Some(scheme) = params["scheme"].as_str() {
                    extensions.file_systems.insert(scheme.to_owned());
                }
            }
            "contentProvider/registered" => {
                if let Some(scheme) = params["scheme"].as_str() {
                    extensions.content_providers.insert(scheme.to_owned());
                }
            }
            "contentProvider/changed" => self.schedule_gutter_refresh(window, cx),
            "completion/registered" => {
                let id = params["id"].as_u64().unwrap_or_default();
                let triggers = params["triggerCharacters"]
                    .as_array()
                    .map(|list| {
                        list.iter()
                            .filter_map(|value| value.as_str().map(str::to_owned))
                            .collect()
                    })
                    .unwrap_or_default();
                extensions
                    .completion_providers
                    .push((id, params["selector"].clone(), triggers));
                for index in 0..self.panes.len() {
                    self.install_completions(index, cx);
                }
            }
            "completion/unregistered" => {
                let id = params["id"].as_u64().unwrap_or_default();
                extensions
                    .completion_providers
                    .retain(|(other, _, _)| *other != id);
            }
            "scm/quickDiffProvider" => {
                extensions.quick_diff = params["active"].as_bool().unwrap_or(false);
                self.schedule_gutter_refresh(window, cx);
            }
            "scm/disposed" => {
                extensions.quick_diff = false;
                for pane in &mut self.panes {
                    if let Some(gutter) = pane.gutter.take() {
                        gutter.clear(cx);
                    }
                }
            }
            "statusBar/update" => {
                if let Some(id) = params["id"].as_str() {
                    extensions.status_items.insert(
                        id.to_owned(),
                        StatusItem {
                            text: params["text"].as_str().unwrap_or_default().to_owned(),
                            right: params["alignment"].as_i64() == Some(2),
                            priority: params["priority"].as_f64().unwrap_or(0.0),
                            visible: params["visible"].as_bool().unwrap_or(false),
                        },
                    );
                }
            }
            "statusBar/remove" => {
                if let Some(id) = params["id"].as_str() {
                    extensions.status_items.remove(id);
                }
            }
            "window/statusBarMessage" => {
                let text = params["text"].as_str().unwrap_or_default();
                self.status_message = (!text.is_empty()).then(|| text.to_owned());
            }
            _ => {}
        }
        cx.notify();
    }

    fn on_call(
        &mut self,
        id: u64,
        method: &str,
        params: Value,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match method {
            "commands/executeHost" => {
                let command = params["command"].as_str().unwrap_or_default().to_owned();
                let args = params["args"].as_array().cloned().unwrap_or_default();
                let result = self.execute_host_command(&command, &args, window, cx);
                self.reply(id, result);
            }
            "workspace/applyEdit" => {
                let mut result = Ok(json!(true));
                for entry in params["entries"].as_array().into_iter().flatten() {
                    let uri = entry["uri"].as_str().unwrap_or_default();
                    let edits = entry["edits"].as_array().cloned().unwrap_or_default();
                    if let Err(error) = self.apply_edits(uri, &edits, window, cx) {
                        result = Err(error);
                    }
                }
                self.reply(id, result.map(|_| json!(true)));
            }
            "workspace/openTextDocument" => {
                let uri = params["uri"].as_str().unwrap_or_default().to_owned();
                self.ensure_loaded(uri, window, cx, move |this, uri, _, _| {
                    this.reply(id, uri.map(|uri| json!(uri)));
                });
            }
            "window/showTextDocument" => {
                let uri = params["uri"].as_str().unwrap_or_default().to_owned();
                let view_column = params["viewColumn"].as_i64();
                let preserve_focus = params["preserveFocus"].as_bool().unwrap_or(false);
                self.ensure_loaded(uri, window, cx, move |this, uri, window, cx| match uri {
                    Ok(uri) => {
                        let column = match view_column {
                            Some(-2) => this.active + 1,
                            Some(column) if column >= 1 => (column - 1) as usize,
                            _ => this.active,
                        };
                        let editor_id =
                            this.open_in_pane(&uri, column, !preserve_focus, window, cx);
                        this.reply(id, Ok(json!(editor_id)));
                    }
                    Err(error) => this.reply(id, Err(error)),
                });
            }
            "window/closeTabs" => {
                let uris: Vec<String> = params["tabs"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|tab| tab["input"]["uri"].as_str().map(str::to_owned))
                    .collect();
                for uri in uris {
                    while let Some((group, tab)) = self.find_tab(&uri, self.active) {
                        self.close_tab_now(group, tab, window, cx);
                    }
                }
                self.reply(id, Ok(json!(true)));
            }
            "languages/setTextDocumentLanguage" => {
                let uri = params["uri"].as_str().unwrap_or_default();
                let language_id = params["languageId"]
                    .as_str()
                    .unwrap_or("plaintext")
                    .to_owned();
                if let Some(document) = self.documents.get_mut(uri) {
                    document.language_id = language_id.clone();
                }
                if let Some(index) = self.pane_index(uri) {
                    let language = Language::for_vscode_id(&language_id);
                    self.panes[index].editor.update(cx, |editor, cx| {
                        editor.set_highlighter(language.highlighter_name(), cx)
                    });
                    self.install_completions(index, cx);
                }
                self.notify_host(
                    "documents/changed",
                    json!({ "uri": uri, "languageId": language_id }),
                );
                self.reply(id, Ok(json!(true)));
            }
            "documents/save" => {
                let uri = params["uri"].as_str().unwrap_or_default().to_owned();
                self.save_document(&uri, None, window, cx);
                self.reply(id, Ok(json!(true)));
            }
            "window/showMessage" => self.show_message(id, &params, window, cx),
            "window/showQuickPick" => {
                let items = params["items"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(|item| {
                        let mut pick = PickItem::new(item["label"].as_str().unwrap_or_default());
                        for key in ["description", "detail"] {
                            if let Some(text) = item[key].as_str() {
                                pick.keywords.push(text.to_owned());
                            }
                        }
                        pick
                    })
                    .collect();
                let options = &params["options"];
                let many = options["canPickMany"].as_bool().unwrap_or(false);
                let chosen = quick_pick::show(
                    options["title"].as_str().map(str::to_owned),
                    options["placeHolder"].as_str().map(str::to_owned),
                    items,
                    window,
                    cx,
                );
                cx.spawn_in(window, async move |this, cx| {
                    let choice = chosen.await.ok().flatten();
                    this.update(cx, |this, _| {
                        let result = match choice {
                            Some(index) if many => json!([index]),
                            Some(index) => json!(index),
                            None => Value::Null,
                        };
                        this.reply(id, Ok(result));
                    })
                    .ok();
                })
                .detach();
            }
            "configuration/update" => {
                let key = params["key"].as_str().unwrap_or_default().to_owned();
                let value = (!params["value"].is_null()).then(|| params["value"].clone());
                let result = self
                    .settings
                    .update_user(&key, value)
                    .map(|_| Value::Null)
                    .map_err(|error| error.to_string());
                self.reply(id, result);
            }
            "env/readClipboard" => {
                let text = cx
                    .read_from_clipboard()
                    .and_then(|item| item.text())
                    .unwrap_or_default();
                self.reply(id, Ok(json!(text)));
            }
            "env/writeClipboard" => {
                let text = params["text"].as_str().unwrap_or_default().to_owned();
                cx.write_to_clipboard(gpui::ClipboardItem::new_string(text));
                self.reply(id, Ok(Value::Null));
            }
            "env/openExternal" => {
                if let Some(uri) = params["uri"].as_str() {
                    cx.open_url(uri);
                }
                self.reply(id, Ok(json!(true)));
            }
            _ => self.reply(id, Err(format!("Keel does not support {method} yet"))),
        }
    }

    fn execute_host_command(
        &mut self,
        command: &str,
        args: &[Value],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<Value, String> {
        match command {
            "setContext" => {
                let key = args.first().and_then(Value::as_str).unwrap_or_default();
                match args.get(1) {
                    None | Some(Value::Null) => self.context.remove(key),
                    Some(value) => self.context.set(key, value.clone()),
                }
                Ok(Value::Null)
            }
            "default:type" => {
                let text = args
                    .first()
                    .and_then(|arg| arg["text"].as_str())
                    .unwrap_or_default()
                    .to_owned();
                if let Some(index) = self.panes.get(self.active).map(|_| self.active) {
                    let editor = self.panes[index].editor.clone();
                    editor.update(cx, |editor, cx| {
                        editor.replace_text_in_range(None, &text, window, cx)
                    });
                    let uri = self.panes[index].uri.clone();
                    self.sync_document(&uri, window, cx);
                    self.publish_selection_if_changed(index, cx);
                }
                Ok(Value::Null)
            }
            "hideSuggestWidget" => {
                if let Some(editor) = self.active_editor() {
                    editor.update(cx, EditorMode::hide_context_menu);
                }
                Ok(Value::Null)
            }
            "editor.action.triggerSuggest" => {
                if let Some(editor) = self.active_editor() {
                    editor.update(cx, |editor, cx| editor.show_completions(window, cx));
                }
                Ok(Value::Null)
            }
            "undo" => {
                window.dispatch_action(Box::new(Undo), cx);
                Ok(Value::Null)
            }
            "redo" => {
                window.dispatch_action(Box::new(Redo), cx);
                Ok(Value::Null)
            }
            "workbench.action.openSettings" => {
                self.status_message = Some(self.strings().settings_unavailable.to_owned());
                cx.notify();
                Ok(Value::Null)
            }
            _ => Err(format!("command '{command}' not found")),
        }
    }

    fn show_message(
        &mut self,
        id: u64,
        params: &Value,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let message = params["message"].as_str().unwrap_or_default().to_owned();
        let items: Vec<String> = params["items"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|item| item.as_str().map(str::to_owned))
            .collect();
        let modal = params["modal"].as_bool().unwrap_or(false);
        if items.is_empty() && !modal {
            self.status_message = Some(message);
            cx.notify();
            self.reply(id, Ok(Value::Null));
            return;
        }
        let level = match params["severity"].as_str() {
            Some("error") => PromptLevel::Critical,
            Some("warning") => PromptLevel::Warning,
            _ => PromptLevel::Info,
        };
        let mut buttons: Vec<&str> = items.iter().map(String::as_str).collect();
        buttons.push(self.strings().cancel);
        let answer = window.prompt(level, &message, params["detail"].as_str(), &buttons, cx);
        let count = items.len();
        cx.spawn_in(window, async move |this, cx| {
            let choice = answer.await.ok().filter(|index| *index < count);
            this.update(cx, |this, _| {
                this.reply(id, Ok(choice.map_or(Value::Null, |index| json!(index))))
            })
            .ok();
        })
        .detach();
    }

    /// Loads a document if Keel does not have it yet, then calls `done` with
    /// its URI. Documents of extension file systems are read through the host.
    fn ensure_loaded(
        &mut self,
        uri: String,
        window: &mut Window,
        cx: &mut Context<Self>,
        done: impl FnOnce(&mut Self, Result<String, String>, &mut Window, &mut Context<Self>) + 'static,
    ) {
        if self.documents.contains_key(&uri) {
            done(self, Ok(uri), window, cx);
            return;
        }
        let Some(parsed) = Uri::parse(&uri) else {
            done(self, Err(format!("not a valid URI: {uri}")), window, cx);
            return;
        };
        if let Some(path) = parsed.file_path() {
            let result = Document::open(&path)
                .map(|document| self.add_file_document(document))
                .map_err(|error| error.to_string());
            done(self, result, window, cx);
            return;
        }
        let Some(extensions) = &self.extensions else {
            done(
                self,
                Err(format!("no extension provides {uri}")),
                window,
                cx,
            );
            return;
        };
        let read = if extensions.file_systems.contains(&parsed.scheme) {
            Some(
                extensions
                    .host
                    .call("fileSystem/readFile", json!({ "uri": uri })),
            )
        } else if extensions.content_providers.contains(&parsed.scheme) {
            Some(
                extensions
                    .host
                    .call("contentProvider/provide", json!({ "uri": uri })),
            )
        } else {
            None
        };
        let Some(read) = read else {
            done(
                self,
                Err(format!(
                    "no extension provides {}: documents",
                    parsed.scheme
                )),
                window,
                cx,
            );
            return;
        };
        let language_id =
            language::vscode_id_for_path(std::path::Path::new(&parsed.path)).to_owned();
        cx.spawn_in(window, async move |this, cx| {
            let result = read.await;
            this.update_in(cx, |this, window, cx| match result {
                Ok(value) => {
                    let text = value["text"]
                        .as_str()
                        .or(value.as_str())
                        .unwrap_or_default()
                        .to_owned();
                    let uri = this.add_document(uri, Backing::Extension, language_id, text);
                    done(this, Ok(uri), window, cx);
                }
                Err(error) => done(this, Err(error.to_string()), window, cx),
            })
            .ok();
        })
        .detach();
    }

    /// Applies an extension's edits, given in VS Code positions of the text
    /// before the edit, then tells extensions about the new text.
    fn apply_edits(
        &mut self,
        uri: &str,
        edits: &[Value],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        let Some(index) = self.pane_index(uri) else {
            let Some(document) = self.documents.get_mut(uri) else {
                return Err(format!("{uri} is not open"));
            };
            let mut text = document.synced_text.clone();
            for (range, new_text) in byte_edits(&text, edits).into_iter().rev() {
                text.replace_range(range, &new_text);
            }
            document.synced_text = text.clone();
            document.version += 1;
            let version = document.version;
            self.notify_host(
                "documents/changed",
                json!({ "uri": uri, "version": version, "text": text, "isDirty": false }),
            );
            return Ok(());
        };
        let editor = self.panes[index].editor.clone();
        editor.update(cx, |editor, cx| {
            let text = editor.value().to_string();
            let edits = byte_edits(&text, edits);
            let selection = editor.selected_range();
            let reversed = editor.is_selection_reversed();
            for (range, new_text) in edits.iter().rev() {
                editor.set_selected_range(range.clone(), cx);
                editor.replace(new_text.clone(), window, cx);
            }
            let start = map_offset(selection.start, &edits);
            let end = map_offset(selection.end, &edits);
            editor.set_selected_range_with_direction(start..end, reversed, cx);
        });
        self.sync_document(uri, window, cx);
        self.publish_selection_if_changed(index, cx);
        Ok(())
    }

    fn set_selections(&mut self, params: &Value, cx: &mut Context<Self>) {
        let Some(index) = params["editorId"]
            .as_str()
            .and_then(|id| self.pane_by_editor_id(id))
        else {
            return;
        };
        let selections: Vec<(Position, Position)> = params["selections"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|selection| {
                let anchor = serde_json::from_value(selection["anchor"].clone()).ok()?;
                let active = serde_json::from_value(selection["active"].clone()).ok()?;
                Some((anchor, active))
            })
            .collect();
        let editor = self.panes[index].editor.clone();
        editor.update(cx, |editor, cx| {
            let text = editor.value().to_string();
            for (number, (anchor, active)) in selections.iter().enumerate() {
                let anchor = position::offset_of(&text, *anchor);
                let active = position::offset_of(&text, *active);
                let range = anchor.min(active)..anchor.max(active);
                if number == 0 {
                    editor.set_selected_range_with_direction(range, active < anchor, cx);
                } else {
                    editor.add_selection(range, cx);
                }
            }
        });
        // What the editor now has, after clipping, is what extensions know.
        let current = self.selections_of(index, cx);
        self.known_selections
            .insert(self.panes[index].editor_id.clone(), current);
    }

    fn reveal_range(&mut self, params: &Value, window: &mut Window, cx: &mut Context<Self>) {
        let Some(index) = params["editorId"]
            .as_str()
            .and_then(|id| self.pane_by_editor_id(id))
        else {
            return;
        };
        if index != self.active || !self.active_editor_focused(window, cx) {
            return;
        }
        // Scrolling follows the cursor; re-placing an empty cursor brings it into view.
        let editor = self.panes[index].editor.clone();
        editor.update(cx, |editor, cx| {
            let range = editor.selected_range();
            if range.is_empty() {
                let text = editor.value().to_string();
                let line_position = position::position_of(&text, range.start);
                let line_start = position::offset_of(&text, Position::new(line_position.line, 0));
                let column = text[line_start..range.start].chars().count() as u32;
                editor.set_cursor_position(
                    lsp_types::Position::new(line_position.line, column),
                    window,
                    cx,
                );
            }
        });
    }

    fn set_decorations(&mut self, params: &Value, cx: &mut Context<Self>) {
        let Some(extensions) = &self.extensions else {
            return;
        };
        let key = params["key"].as_str().unwrap_or_default().to_owned();
        let Some(options) = extensions.decoration_types.get(&key) else {
            return;
        };
        let style = decoration_style(options, cx);
        let Some(index) = params["editorId"]
            .as_str()
            .and_then(|id| self.pane_by_editor_id(id))
        else {
            return;
        };
        let editor = self.panes[index].editor.clone();
        let text = editor.read(cx).value().to_string();
        let decorations: Vec<TextDecoration> = params["ranges"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|item| {
                let start = serde_json::from_value(item["range"]["start"].clone()).ok()?;
                let end = serde_json::from_value(item["range"]["end"].clone()).ok()?;
                let range = position::offset_of(&text, start)..position::offset_of(&text, end);
                (!range.is_empty()).then(|| TextDecoration::new(range, style))
            })
            .collect();
        let pane = &mut self.panes[index];
        match pane.decorations.get(&key) {
            Some(collection) => collection.set(decorations, cx),
            None => {
                let collection = editor.update(cx, |editor, cx| {
                    editor.create_decorations_collection(decorations, cx)
                });
                pane.decorations.insert(key, collection);
            }
        }
    }

    /// Gives a pane's editor the completion providers that match its document.
    fn install_completions(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(extensions) = &self.extensions else {
            return;
        };
        let pane = &self.panes[index];
        let Some(document) = self.documents.get(&pane.uri) else {
            return;
        };
        let scheme = Uri::parse(&document.uri)
            .map(|uri| uri.scheme)
            .unwrap_or_default();
        let triggers: Vec<String> = extensions
            .completion_providers
            .iter()
            .filter(|(_, selector, _)| selector_matches(selector, &scheme, &document.language_id))
            .flat_map(|(_, _, triggers)| triggers.clone())
            .collect();
        let matches = extensions
            .completion_providers
            .iter()
            .any(|(_, selector, _)| selector_matches(selector, &scheme, &document.language_id));
        if !matches {
            return;
        }
        let provider = ExtensionCompletions {
            host: extensions.host.clone(),
            uri: pane.uri.clone(),
            triggers,
        };
        pane.editor.update(cx, |editor, _| {
            editor.lsp_mut().completion_provider = Some(Rc::new(provider));
        });
    }

    /// Recomputes the green "added lines" bars shortly after the last change.
    fn schedule_gutter_refresh(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(extensions) = &self.extensions else {
            return;
        };
        if !extensions.quick_diff {
            return;
        }
        let host = extensions.host.clone();
        let content_providers = extensions.content_providers.clone();
        let uris: Vec<String> = self
            .panes
            .iter()
            .filter(|pane| {
                !matches!(
                    self.documents
                        .get(&pane.uri)
                        .map(|document| &document.backing),
                    Some(Backing::Image(_) | Backing::Diff(_))
                )
            })
            .map(|pane| pane.uri.clone())
            .collect();
        self.gutter_refresh = cx.spawn_in(window, async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(120))
                .await;
            for uri in uris {
                let Ok(original) = host
                    .call("scm/originalResource", json!({ "uri": uri }))
                    .await
                else {
                    continue;
                };
                let Some(original) = original.as_str().map(str::to_owned) else {
                    continue;
                };
                let scheme = Uri::parse(&original)
                    .map(|uri| uri.scheme)
                    .unwrap_or_default();
                let baseline = if content_providers.contains(&scheme) {
                    host.call("contentProvider/provide", json!({ "uri": original }))
                        .await
                        .ok()
                        .and_then(|value| value.as_str().map(str::to_owned))
                } else {
                    Uri::parse(&original)
                        .and_then(|uri| uri.file_path())
                        .and_then(|path| std::fs::read_to_string(path).ok())
                };
                let Some(baseline) = baseline else {
                    continue;
                };
                this.update(cx, |this, cx| this.set_gutter(&uri, &baseline, cx))
                    .ok();
            }
        });
    }

    fn set_gutter(&mut self, uri: &str, baseline: &str, cx: &mut Context<Self>) {
        let Some(index) = self.pane_index(uri) else {
            return;
        };
        let editor = self.panes[index].editor.clone();
        let text = editor.read(cx).value().to_string();
        let green: Hsla = gpui::rgb(0x2ea043).into();
        let decorations: Vec<GutterDecoration> = workbench::line_diff::added_lines(baseline, &text)
            .into_iter()
            .map(|line| {
                let start = position::offset_of(&text, Position::new(line as u32, 0));
                let end = position::offset_of(&text, Position::new(line as u32, u32::MAX));
                GutterDecoration::new(start..end.max(start)).with_color(green)
            })
            .collect();
        let pane = &mut self.panes[index];
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

/// Asks the extension host for completions.
struct ExtensionCompletions {
    host: Rc<ExtensionHost>,
    uri: String,
    triggers: Vec<String>,
}

impl CompletionProvider for ExtensionCompletions {
    fn completions(
        &self,
        text: &Rope,
        offset: usize,
        trigger: CompletionContext,
        _window: &mut Window,
        cx: &mut App,
    ) -> Task<anyhow::Result<CompletionResponse>> {
        let text = text.to_string();
        let position = position::position_of(&text, offset);
        let call = self.host.call(
            "completion/provide",
            json!({ "uri": self.uri, "position": position, "triggerCharacter": trigger.trigger_character }),
        );
        cx.spawn(async move |_| {
            let items = call.await?;
            let items = items
                .as_array()
                .into_iter()
                .flatten()
                .map(|item| CompletionItem {
                    label: item["label"].as_str().unwrap_or_default().to_owned(),
                    // VS Code counts kinds from 0, LSP from 1.
                    kind: item["kind"].as_i64().and_then(|kind| {
                        serde_json::from_value::<CompletionItemKind>(json!(kind + 1)).ok()
                    }),
                    detail: item["detail"].as_str().map(str::to_owned),
                    sort_text: item["sortText"].as_str().map(str::to_owned),
                    filter_text: item["filterText"].as_str().map(str::to_owned),
                    insert_text: item["insertText"].as_str().map(str::to_owned),
                    preselect: item["preselect"].as_bool(),
                    ..Default::default()
                })
                .collect();
            Ok(CompletionResponse::Array(items))
        })
    }

    fn is_completion_trigger(&self, _offset: usize, new_text: &str, _cx: &mut App) -> bool {
        self.triggers.iter().any(|trigger| trigger == new_text)
    }
}

/// Applies the user's `keybindings.json` to extension commands: `-command`
/// entries remove a binding, the others come last so they win.
fn apply_user_bindings(bindings: &mut Vec<Binding>, user: &[crate::keymap::UserBinding]) {
    let keel_commands: HashSet<&str> = crate::keymap::COMMANDS
        .iter()
        .map(|command| command.id)
        .collect();
    for entry in user
        .iter()
        .filter(|entry| !keel_commands.contains(entry.command.as_str()))
    {
        // Chords are not supported for extension commands yet.
        let Some(keystroke) = BindingKeystroke::parse(&entry.keystrokes) else {
            continue;
        };
        if entry.removes {
            bindings.retain(|binding| {
                !(binding.command == entry.command && binding.keystroke == keystroke)
            });
            continue;
        }
        let Ok(when) = WhenClause::parse(entry.when.as_deref().unwrap_or_default()) else {
            continue;
        };
        bindings.push(Binding {
            keystroke,
            command: entry.command.clone(),
            when,
            args: entry.args.clone(),
        });
    }
}

/// Key bindings an extension declares, ready to match keystrokes.
fn contributed_bindings(extension: &InstalledExtension) -> Vec<Binding> {
    let raw = &extension.manifest.raw()["contributes"]["keybindings"];
    raw.as_array()
        .into_iter()
        .flatten()
        .filter_map(|binding| {
            let key = binding["key"].as_str().unwrap_or_default();
            let keystrokes = workbench::keybinding::to_gpui(key, binding["mac"].as_str())?;
            // Chords are not supported yet.
            if keystrokes.contains(' ') {
                return None;
            }
            let keystroke = BindingKeystroke::parse(&keystrokes)?;
            let when = WhenClause::parse(binding["when"].as_str().unwrap_or_default()).ok()?;
            Some(Binding {
                keystroke,
                command: binding["command"].as_str()?.to_owned(),
                when,
                args: binding.get("args").cloned(),
            })
        })
        .collect()
}

/// A keystroke such as `shift-cmd-i`, read from an extension's bindings.
#[derive(Debug, PartialEq)]
struct BindingKeystroke {
    modifiers: gpui::Modifiers,
    key: String,
}

impl BindingKeystroke {
    fn parse(text: &str) -> Option<Self> {
        let mut modifiers = gpui::Modifiers::default();
        // The key itself may be "-", as in "cmd--".
        let (prefix, key) = match text.strip_suffix("--") {
            Some(prefix) => (prefix, "-"),
            None => match text.rsplit_once('-') {
                Some((prefix, key)) => (prefix, key),
                None => ("", text),
            },
        };
        for modifier in prefix.split('-').filter(|part| !part.is_empty()) {
            match modifier {
                "ctrl" => modifiers.control = true,
                "alt" => modifiers.alt = true,
                "shift" => modifiers.shift = true,
                "cmd" => modifiers.platform = true,
                "fn" => modifiers.function = true,
                _ => return None,
            }
        }
        (!key.is_empty()).then(|| Self {
            modifiers,
            key: key.to_owned(),
        })
    }
}

fn same_keystroke(binding: &BindingKeystroke, pressed: &Keystroke) -> bool {
    binding.modifiers == pressed.modifiers && binding.key == pressed.key
}

/// The text a keystroke types, if it types any.
fn typed_text(keystroke: &Keystroke) -> Option<String> {
    let modifiers = &keystroke.modifiers;
    if modifiers.control || modifiers.platform || modifiers.function {
        return None;
    }
    // macOS fills `key_char`; keystrokes built from text, as in tests, only
    // have the key name, which for printable keys is the character itself.
    let text = match &keystroke.key_char {
        Some(text) => text.clone(),
        None if keystroke.key.chars().count() == 1 => {
            if modifiers.shift {
                keystroke.key.to_uppercase()
            } else {
                keystroke.key.clone()
            }
        }
        None if keystroke.key == "space" => " ".to_owned(),
        None => return None,
    };
    (!text.is_empty() && !text.chars().any(char::is_control)).then_some(text)
}

/// Converts edits from VS Code positions to byte ranges of `text`, sorted.
fn byte_edits(text: &str, edits: &[Value]) -> Vec<(Range<usize>, String)> {
    let mut converted: Vec<(Range<usize>, String)> = edits
        .iter()
        .filter_map(|edit| {
            let start: Position = serde_json::from_value(edit["range"]["start"].clone()).ok()?;
            let end: Position = serde_json::from_value(edit["range"]["end"].clone()).ok()?;
            let start = position::offset_of(text, start);
            let end = position::offset_of(text, end);
            Some((
                start.min(end)..start.max(end),
                edit["newText"].as_str().unwrap_or_default().to_owned(),
            ))
        })
        .collect();
    converted.sort_by_key(|(range, _)| range.start);
    converted
}

/// Where a byte offset ends up after sorted edits are applied.
fn map_offset(offset: usize, edits: &[(Range<usize>, String)]) -> usize {
    let mut shift: isize = 0;
    for (range, new_text) in edits {
        if range.end <= offset {
            shift += new_text.len() as isize - range.len() as isize;
        } else if range.start < offset {
            return (range.start as isize + shift) as usize + new_text.len();
        }
    }
    (offset as isize + shift).max(0) as usize
}

fn selector_matches(selector: &Value, scheme: &str, language_id: &str) -> bool {
    match selector {
        Value::Array(parts) => parts
            .iter()
            .any(|part| selector_matches(part, scheme, language_id)),
        Value::String(language) => language == "*" || language == language_id,
        Value::Object(fields) => {
            let scheme_ok = fields
                .get("scheme")
                .and_then(Value::as_str)
                .is_none_or(|wanted| wanted == "*" || wanted == scheme);
            let language_ok = fields
                .get("language")
                .and_then(Value::as_str)
                .is_none_or(|wanted| wanted == "*" || wanted == language_id);
            scheme_ok && language_ok
        }
        _ => false,
    }
}

/// The highlight style of an extension decoration type.
fn decoration_style(options: &Value, cx: &App) -> HighlightStyle {
    let mut style = HighlightStyle::default();
    let mut color = match &options["color"] {
        Value::Object(color)
            if color.get("themeColor").and_then(Value::as_str)
                == Some("editorGhostText.foreground") =>
        {
            Some(cx.theme().muted_foreground)
        }
        Value::Object(_) => Some(cx.theme().muted_foreground),
        Value::String(css) => parse_css_color(css),
        _ => None,
    };
    if let (Some(color), Some(opacity)) = (
        color.as_mut(),
        options["opacity"]
            .as_str()
            .and_then(|value| value.parse::<f32>().ok()),
    ) {
        color.a *= opacity;
    }
    style.color = color;
    if options["fontStyle"].as_str() == Some("italic") {
        style.font_style = Some(FontStyle::Italic);
    }
    if options["fontWeight"].as_str() == Some("bold") {
        style.font_weight = Some(FontWeight::BOLD);
    }
    if let Some(background) = options["backgroundColor"]
        .as_str()
        .and_then(parse_css_color)
    {
        style.background_color = Some(background);
    }
    style
}

fn parse_css_color(css: &str) -> Option<Hsla> {
    Rgba::try_from(css).ok().map(Into::into)
}

/// VS Code status bar texts can contain icons such as `$(sync~spin)`.
fn strip_codicons(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find("$(") {
        result.push_str(&rest[..start]);
        match rest[start..].find(')') {
            Some(end) => rest = &rest[start + end + 1..],
            None => {
                rest = &rest[start..];
                break;
            }
        }
    }
    result.push_str(rest);
    result.trim().to_owned()
}

fn first_line(text: &str) -> &str {
    text.lines().next().unwrap_or_default()
}

/// Shows an error in a dialog with a single OK button.
fn show_error(message: &str, detail: &str, ok: &str, window: &mut Window, cx: &mut App) {
    // The dialog stays open on its own; there is no answer to wait for.
    let _dismissed = window.prompt(PromptLevel::Critical, message, Some(detail), &[ok], cx);
}

#[cfg(test)]
mod tests {
    use gpui::TestAppContext;

    use super::*;

    pub(super) fn test_settings() -> Settings {
        Settings::load("/nonexistent/keel-test/settings.json", None)
    }

    fn set_up(cx: &mut TestAppContext) {
        cx.update(|cx| {
            gpui_component::init(cx);
            language::register_grammars();
            init(cx);
        });
    }

    fn active_text(view: &Entity<Workbench>, cx: &mut gpui::VisualTestContext) -> String {
        view.read_with(cx, |view, cx| {
            view.panes[view.active].editor.read(cx).value().to_string()
        })
    }

    #[gpui::test]
    fn typing_marks_the_document_modified_and_save_writes_it(cx: &mut TestAppContext) {
        set_up(cx);
        let folder = tempfile::tempdir().unwrap();
        let path = folder.path().join("app.ts");
        std::fs::write(&path, "let total = 1;\n").unwrap();
        let document = Document::open(&path).unwrap();

        let (view, cx) = cx.add_window_view(|window, cx| {
            Workbench::new(Some(document), test_settings(), None, window, cx)
        });
        assert!(!view.read_with(cx, |view, _| view.has_unsaved_changes()));

        cx.simulate_input("è");
        cx.run_until_parked();
        assert!(view.read_with(cx, |view, _| view.has_unsaved_changes()));
        let typed = active_text(&view, cx);
        assert!(typed.contains('è'), "{typed:?}");

        cx.dispatch_action(Save);
        cx.run_until_parked();
        assert!(!view.read_with(cx, |view, _| view.has_unsaved_changes()));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), typed);
    }

    #[gpui::test]
    fn undoing_back_to_the_saved_text_clears_the_modified_mark(cx: &mut TestAppContext) {
        set_up(cx);
        let folder = tempfile::tempdir().unwrap();
        let path = folder.path().join("index.tsx");
        std::fs::write(&path, "export const a = <div />;\n").unwrap();
        let document = Document::open(&path).unwrap();

        let (view, cx) = cx.add_window_view(|window, cx| {
            Workbench::new(Some(document), test_settings(), None, window, cx)
        });
        cx.simulate_input("x");
        cx.run_until_parked();
        assert!(view.read_with(cx, |view, _| view.has_unsaved_changes()));

        cx.simulate_keystrokes("cmd-z");
        cx.run_until_parked();
        assert!(!view.read_with(cx, |view, _| view.has_unsaved_changes()));
    }

    #[gpui::test]
    fn the_interface_language_follows_the_setting_and_switches_at_once(cx: &mut TestAppContext) {
        set_up(cx);
        let folder = tempfile::tempdir().unwrap();
        let user_file = folder.path().join("settings.json");
        std::fs::write(&user_file, r#"{ "keel.language": "it" }"#).unwrap();
        let settings = Settings::load(&user_file, None);

        let (view, cx) =
            cx.add_window_view(|window, cx| Workbench::new(None, settings, None, window, cx));
        view.update_in(cx, |view, window, cx| view.new_file(window, cx));
        let name = |view: &Entity<Workbench>, cx: &mut gpui::VisualTestContext| {
            view.read_with(cx, |view, _| {
                let document = &view.documents[&view.panes[view.active].uri];
                view.display_name(document)
            })
        };
        assert_eq!(name(&view, cx), "Senza titolo 1");

        view.update_in(cx, |view, window, cx| {
            view.set_language(UiLanguage::English, window, cx)
        });
        assert_eq!(name(&view, cx), "Untitled 1");
        let saved = settings::parse_jsonc(&std::fs::read_to_string(&user_file).unwrap()).unwrap();
        assert_eq!(saved["keel.language"], "en");
    }

    #[gpui::test]
    fn choosing_a_theme_shows_it_and_saves_it(cx: &mut TestAppContext) {
        set_up(cx);
        cx.update(crate::theme::init);
        let folder = tempfile::tempdir().unwrap();
        let user_file = folder.path().join("settings.json");
        let settings = Settings::load(&user_file, None);
        let (view, cx) =
            cx.add_window_view(|window, cx| Workbench::new(None, settings, None, window, cx));

        view.update_in(cx, |view, window, cx| view.set_theme("Chiaro", window, cx));

        assert!(cx.update(|_, cx| !gpui_component::ActiveTheme::theme(cx).is_dark()));
        let saved = settings::parse_jsonc(&std::fs::read_to_string(&user_file).unwrap()).unwrap();
        assert_eq!(saved["workbench.colorTheme"], "Chiaro");
    }

    #[test]
    fn the_user_can_remove_and_add_extension_bindings() {
        let binding = |keys: &str, command: &str| Binding {
            keystroke: BindingKeystroke::parse(keys).unwrap(),
            command: command.to_owned(),
            when: WhenClause::True,
            args: None,
        };
        let mut bindings = vec![
            binding("cmd-i", "vsauto.stop"),
            binding("cmd-o", "vsauto.requestOrExpandSuggestion"),
        ];
        let user = vec![
            crate::keymap::UserBinding {
                command: "vsauto.stop".to_owned(),
                removes: true,
                keystrokes: "cmd-i".to_owned(),
                when: None,
                args: None,
            },
            crate::keymap::UserBinding {
                command: "vsauto.stop".to_owned(),
                removes: false,
                keystrokes: "cmd-.".to_owned(),
                when: Some("vsauto.running".to_owned()),
                args: None,
            },
            // Keel's own commands are bound by GPUI, not here.
            crate::keymap::UserBinding {
                command: "workbench.action.files.save".to_owned(),
                removes: false,
                keystrokes: "cmd-alt-s".to_owned(),
                when: None,
                args: None,
            },
        ];
        apply_user_bindings(&mut bindings, &user);
        let summary: Vec<(String, String)> = bindings
            .iter()
            .map(|binding| (binding.keystroke.key.clone(), binding.command.clone()))
            .collect();
        assert_eq!(
            summary,
            [
                (
                    "o".to_owned(),
                    "vsauto.requestOrExpandSuggestion".to_owned()
                ),
                (".".to_owned(), "vsauto.stop".to_owned())
            ]
        );
    }

    #[test]
    fn edits_shift_offsets_after_them() {
        let edits = vec![(2..4, "xyz".to_owned()), (10..10, "ab".to_owned())];
        assert_eq!(map_offset(1, &edits), 1);
        assert_eq!(map_offset(5, &edits), 6);
        assert_eq!(map_offset(3, &edits), 5);
        assert_eq!(map_offset(12, &edits), 15);
    }

    #[test]
    fn selectors_match_scheme_and_language() {
        let selector = json!({ "scheme": "vsauto-ghost" });
        assert!(selector_matches(
            &selector,
            "vsauto-ghost",
            "typescriptreact"
        ));
        assert!(!selector_matches(&selector, "file", "typescriptreact"));
        assert!(selector_matches(&json!("typescript"), "file", "typescript"));
        assert!(selector_matches(
            &json!([{ "language": "x" }, "*"]),
            "file",
            "y"
        ));
    }

    #[test]
    fn status_texts_lose_their_icons() {
        assert_eq!(strip_codicons("$(sync~spin) VSAuto 42%"), "VSAuto 42%");
        assert_eq!(strip_codicons("plain"), "plain");
    }

    #[test]
    fn extension_keystrokes_are_read() {
        let keystroke = BindingKeystroke::parse("shift-cmd-i").unwrap();
        assert!(
            keystroke.modifiers.shift
                && keystroke.modifiers.platform
                && !keystroke.modifiers.control
        );
        assert_eq!(keystroke.key, "i");
        assert_eq!(BindingKeystroke::parse("cmd--").unwrap().key, "-");
        assert_eq!(
            BindingKeystroke::parse("tab").unwrap().modifiers,
            gpui::Modifiers::default()
        );
        assert!(same_keystroke(
            &BindingKeystroke::parse("cmd-i").unwrap(),
            &Keystroke::parse("cmd-i").unwrap()
        ));
        assert_eq!(BindingKeystroke::parse("hyper-a"), None);
    }

    #[test]
    fn printable_keystrokes_type_text() {
        let typed = |text: &'static str| typed_text(&Keystroke::parse(text).unwrap());
        assert_eq!(typed("a"), Some("a".to_owned()));
        assert_eq!(typed("cmd-a"), None);
        assert_eq!(typed("enter"), None);
    }
}

#[cfg(test)]
mod extension_tests {
    use std::path::Path;

    use gpui::{TestAppContext, VisualTestContext};

    use super::tests::test_settings;
    use super::*;

    fn wait_until(
        cx: &mut VisualTestContext,
        what: &str,
        mut done: impl FnMut(&mut VisualTestContext) -> bool,
    ) {
        // VSAuto Ghost types at a human pace with real timers, so allow time.
        for _ in 0..3000 {
            // GPUI's test clock only moves when told to.
            cx.executor().advance_clock(Duration::from_millis(20));
            cx.run_until_parked();
            if done(cx) {
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        panic!("timed out waiting for {what}");
    }

    fn describe(view: &Entity<Workbench>, cx: &mut VisualTestContext) -> String {
        view.update_in(cx, |view, window, cx| {
            let panes: Vec<String> = view
                .panes
                .iter()
                .map(|pane| {
                    let editor = pane.editor.read(cx);
                    format!(
                        "{} focused={} cursor={:?} text={:?}",
                        pane.uri,
                        editor.focus_handle(cx).is_focused(window),
                        editor.selected_range(),
                        editor.value().chars().take(60).collect::<String>()
                    )
                })
                .collect();
            format!(
                "active={} status={:?} running={:?} ghostFocused={:?} type={}\n{}",
                view.active,
                view.status_message,
                view.context.get("vsauto.running"),
                view.context.get("vsauto.ghostFocused"),
                view.extensions
                    .as_ref()
                    .is_some_and(|extensions| extensions.commands.contains("type")),
                panes.join("\n")
            )
        })
    }

    fn pane_texts(view: &Entity<Workbench>, cx: &mut VisualTestContext) -> Vec<(String, String)> {
        view.read_with(cx, |view, cx| {
            view.panes
                .iter()
                .map(|pane| (pane.uri.clone(), pane.editor.read(cx).value().to_string()))
                .collect()
        })
    }

    #[gpui::test]
    fn vsauto_ghost_types_the_original_file_into_its_ghost(cx: &mut TestAppContext) {
        cx.executor().allow_parking();
        cx.update(|cx| {
            gpui_component::init(cx);
            language::register_grammars();
            init(cx);
        });
        let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures");
        let data = tempfile::tempdir().unwrap();
        let installed = crate::paths::install_extension(
            data.path(),
            &fixtures.join("extensions/vsauto-ghost-local-0.6.1-editor-layout.vsix"),
        )
        .unwrap();
        let folder = tempfile::tempdir().unwrap();
        let path = folder.path().join("booking-card.tsx");
        std::fs::copy(fixtures.join("samples/booking-card.tsx"), &path).unwrap();
        let original = std::fs::read_to_string(&path).unwrap();

        let (host, events) = ExtensionHost::start().unwrap();
        let setup = ExtensionSetup {
            user_bindings: Vec::new(),
            host,
            events,
            installed: vec![installed],
            data_dir: data.path().to_path_buf(),
        };
        let document = Document::open(&path).unwrap();
        // Dialogs need gpui-component's Root around the workbench, as in Keel.
        let mut workbench = None;
        let (_root, cx) = cx.add_window_view(|window, cx| {
            let view = cx
                .new(|cx| Workbench::new(Some(document), test_settings(), Some(setup), window, cx));
            workbench = Some(view.clone());
            gpui_component::Root::new(view, window, cx)
        });
        let view = workbench.unwrap();

        view.update_in(cx, |view, window, cx| {
            view.run_extension_command("vsauto.start".to_owned(), Vec::new(), window, cx);
        });
        wait_until(cx, "the ghost editor beside the original", |cx| {
            pane_texts(&view, cx).len() == 2
        });
        let ghost_uri = pane_texts(&view, cx)[1].0.clone();
        assert!(ghost_uri.starts_with("vsauto-ghost:"), "{ghost_uri}");
        wait_until(cx, "the running session", |cx| {
            view.read_with(cx, |view, _| {
                view.context.get("vsauto.running") == Some(&json!(true))
                    && view.context.get("vsauto.ghostFocused") == Some(&json!(true))
            })
        });

        // Keys come at a human pace: VSAuto Ghost drops the extra ones of a
        // burst on purpose, so no backlog builds up during its pauses.
        for _ in 0..30 {
            cx.simulate_keystrokes("a");
            for _ in 0..4 {
                cx.executor().advance_clock(Duration::from_millis(20));
                cx.run_until_parked();
                std::thread::sleep(Duration::from_millis(15));
            }
        }
        for _ in 0..3000 {
            cx.executor().advance_clock(Duration::from_millis(20));
            cx.run_until_parked();
            if pane_texts(&view, cx)[1].1.len() >= 20 {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(
            pane_texts(&view, cx)[1].1.len() >= 20,
            "nothing typed: {}",
            describe(&view, cx)
        );

        let (_, ghost_text) = pane_texts(&view, cx)[1].clone();
        assert!(
            original.starts_with(&ghost_text),
            "the ghost should hold the start of the original file, found {ghost_text:?}"
        );
        // The original file is untouched.
        assert_eq!(std::fs::read_to_string(&path).unwrap(), original);

        // The typed lines get the green bar, and grey suggestions show up in pauses.
        wait_until(cx, "the green bar of added lines", |cx| {
            view.read_with(cx, |view, _| view.panes[1].gutter.is_some())
        });
        // Suggestions show up at random in pauses; Cmd+O asks for one.
        cx.simulate_keystrokes("cmd-o");
        wait_until(cx, "a grey suggestion", |cx| {
            view.read_with(cx, |view, cx| {
                view.panes[1]
                    .decorations
                    .values()
                    .any(|collection| !collection.get_ranges(cx).is_empty())
            })
        });
        let (_, with_suggestion) = pane_texts(&view, cx)[1].clone();
        assert!(
            original.starts_with(&with_suggestion),
            "{with_suggestion:?}"
        );
        println!(
            "ghost: {} characters typed, {} with the grey suggestion",
            ghost_text.chars().count(),
            with_suggestion.chars().count()
        );

        cx.simulate_keystrokes("cmd-i");
        wait_until(cx, "the session to stop", |cx| {
            view.read_with(cx, |view, _| {
                view.context.get("vsauto.running") != Some(&json!(true))
            })
        });
    }
}
