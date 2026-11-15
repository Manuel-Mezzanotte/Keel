//! The workbench: the window with editors side by side, the status bar, and
//! the bridge between Keel and the extension host.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::ops::Range;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;

use editor::Document;
use extensions::{ExtensionHost, HostEvent, InstalledExtension};
use futures::StreamExt as _;
use futures::channel::mpsc::UnboundedReceiver;
use gpui::prelude::FluentBuilder as _;
use gpui::{
    App, AppContext as _, Context, Entity, EntityInputHandler as _, Focusable as _, FontStyle,
    FontWeight, HighlightStyle, Hsla, InteractiveElement as _, IntoElement, KeyBinding, Keystroke,
    KeystrokeEvent, ParentElement as _, PromptLevel, Render, Rgba, Styled as _, Subscription, Task,
    Window, actions, div, px,
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

use crate::paths;
use crate::quick_pick::{self, PickItem};

actions!(keel, [Save, CloseWindow, Quit, ShowCommands]);

/// Registers the workbench key bindings.
pub fn init(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("cmd-s", Save, None),
        KeyBinding::new("cmd-w", CloseWindow, None),
        KeyBinding::new("cmd-q", Quit, None),
        KeyBinding::new("cmd-shift-p", ShowCommands, None),
    ]);
    // Reached only when no window handles Quit, so nothing is left unsaved.
    cx.on_action(|_: &Quit, cx| cx.quit());
}

/// Everything the workbench needs to run extensions.
pub struct ExtensionSetup {
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

/// What to do once the user has dealt with unsaved changes.
#[derive(Clone, Copy)]
enum AfterConfirm {
    CloseWindow,
    QuitApp,
}

pub struct Workbench {
    settings: Settings,
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
    _subscriptions: Vec<Subscription>,
}

impl Workbench {
    pub fn new(
        document: Option<Document>,
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
        window.on_window_should_close(cx, {
            let this = this.clone();
            move |window, cx| {
                this.update(cx, |this, cx| {
                    this.confirm(AfterConfirm::CloseWindow, window, cx)
                })
                .unwrap_or(true)
            }
        });

        let status_message = settings.warnings.first().cloned();
        let mut workbench = Self {
            settings,
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
            first_frame_drawn: false,
            _subscriptions: subscriptions,
        };
        if let Some(setup) = extensions {
            workbench.start_extensions(setup, window, cx);
        }
        let uri = match document {
            Some(document) => workbench.add_file_document(document),
            None => workbench.add_untitled_document(),
        };
        workbench.open_in_pane(&uri, 0, true, window, cx);
        workbench
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

    /// Shows a loaded document in the pane at `column`, creating the pane
    /// when needed. Returns the editor id extensions know the pane by.
    fn open_in_pane(
        &mut self,
        uri: &str,
        column: usize,
        focus: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> String {
        if let Some(index) = self.pane_index(uri) {
            if focus {
                self.focus_pane(index, window, cx);
            }
            return self.panes[index].editor_id.clone();
        }
        let Some(document) = self.documents.get(uri) else {
            return String::new();
        };
        let language = Language::for_vscode_id(&document.language_id);
        let text = document.synced_text.clone();
        let tab_size = gpui_component::input::TabSize {
            tab_size: self
                .settings
                .number("editor.tabSize")
                .map_or(2, |size| size.clamp(1.0, 16.0) as usize),
            hard_tabs: self
                .settings
                .get("editor.insertSpaces")
                .and_then(Value::as_bool)
                == Some(false),
        };
        let soft_wrap = self
            .settings
            .string("editor.wordWrap")
            .is_some_and(|wrap| wrap != "off");
        let editor = cx.new(|cx| {
            EditorState::new(window, cx)
                .language(language.highlighter_name())
                .tab_size(tab_size)
                .soft_wrap(soft_wrap)
                .default_value(text)
        });
        let watched_uri = uri.to_owned();
        let subscription = cx.subscribe_in(
            &editor,
            window,
            move |this, _, event: &InputEvent, window, cx| {
                if matches!(event, InputEvent::Change) {
                    this.sync_document(&watched_uri, window, cx);
                }
            },
        );
        let editor_id = format!("editor-{}", self.next_editor);
        self.next_editor += 1;
        let pane = Pane {
            editor_id: editor_id.clone(),
            uri: uri.to_owned(),
            editor,
            decorations: HashMap::new(),
            gutter: None,
            _subscriptions: vec![subscription],
        };
        let index = if column < self.panes.len() {
            self.panes[column] = pane;
            column
        } else {
            self.panes.push(pane);
            self.panes.len() - 1
        };
        self.install_completions(index, cx);
        if focus || self.panes.len() == 1 {
            self.focus_pane(index, window, cx);
        }
        self.publish_editors(window, cx);
        self.schedule_gutter_refresh(window, cx);
        self.refresh_title(window);
        cx.notify();
        editor_id
    }

    fn focus_pane(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.active = index;
        let editor = self.panes[index].editor.clone();
        editor.update(cx, |editor, cx| editor.focus(window, cx));
    }

    fn close_pane(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let pane = self.panes.remove(index);
        let still_shown = self.panes.iter().any(|other| other.uri == pane.uri);
        let disposable = matches!(
            self.documents
                .get(&pane.uri)
                .map(|document| &document.backing),
            Some(Backing::Extension)
        );
        if !still_shown && disposable {
            self.documents.remove(&pane.uri);
            self.notify_host("documents/closed", json!({ "uri": pane.uri }));
        }
        if self.panes.is_empty() {
            let uri = self.add_untitled_document();
            self.open_in_pane(&uri, 0, true, window, cx);
            return;
        }
        self.active = self.active.min(self.panes.len() - 1);
        self.focus_pane(self.active, window, cx);
        self.publish_editors(window, cx);
        self.refresh_title(window);
        cx.notify();
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
            self.active = index;
            self.publish_editors(window, cx);
            self.refresh_title(window);
        }
    }

    /// Tells extensions about the new text of `uri`, unless they know it already.
    fn sync_document(&mut self, uri: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(index) = self.pane_index(uri) else {
            return;
        };
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
            Backing::Extension => false,
        };
        let params = json!({
            "uri": uri,
            "version": document.version,
            "text": text,
            "isDirty": document.modified,
        });
        document.synced_text = text;
        self.notify_host("documents/changed", params);
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
        if self.extensions.is_none() {
            return;
        }
        let editors: Vec<Value> = self
            .panes
            .iter()
            .enumerate()
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

    fn refresh_title(&self, window: &mut Window) {
        let name = self
            .panes
            .get(self.active)
            .and_then(|pane| self.documents.get(&pane.uri))
            .map_or_else(|| "Keel".to_owned(), OpenDocument::name);
        window.set_window_title(&format!("{name} — Keel"));
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
        let Some(document) = self.documents.get_mut(uri) else {
            return;
        };
        let text = document.synced_text.clone();
        match &mut document.backing {
            Backing::File(file) => match file.save(&text) {
                Ok(()) => {
                    document.modified = false;
                    self.notify_host("documents/saved", json!({ "uri": uri }));
                    self.refresh_title(window);
                    cx.notify();
                    if let Some(after) = after {
                        self.finish(after, window, cx);
                    }
                }
                Err(error) => {
                    let message = format!("Keel could not save {}", file.file_name());
                    show_error(&message, &error.to_string(), window, cx);
                }
            },
            Backing::Untitled => self.save_as(uri.to_owned(), text, after, window, cx),
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
        let folder = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/"));
        let chosen = cx.prompt_for_new_path(&folder, Some("Untitled.txt"));
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(path))) = chosen.await else {
                return;
            };
            this.update_in(cx, |this, window, cx| {
                let mut document = match Document::open(&path) {
                    Ok(document) => document,
                    Err(error) => {
                        let message = format!("Keel could not use {}", path.display());
                        show_error(&message, &error.to_string(), window, cx);
                        return;
                    }
                };
                if let Err(error) = document.save(&text) {
                    let message = format!("Keel could not save {}", document.file_name());
                    show_error(&message, &error.to_string(), window, cx);
                    return;
                }
                // The untitled buffer becomes the saved file.
                let index = this.pane_index(&uri);
                this.documents.remove(&uri);
                this.notify_host("documents/closed", json!({ "uri": uri }));
                let new_uri = this.add_file_document(document);
                if let Some(index) = index {
                    this.panes.remove(index);
                    this.open_in_pane(&new_uri, index, true, window, cx);
                }
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
            window.remove_window();
        }
    }

    fn quit(&mut self, _: &Quit, window: &mut Window, cx: &mut Context<Self>) {
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
        let name = self.documents[&uri].name();
        let message = format!("Do you want to save the changes you made to {name}?");
        let answer = window.prompt(
            PromptLevel::Warning,
            &message,
            Some("Your changes will be lost if you don't save them."),
            &["Save", "Don't Save", "Cancel"],
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
            AfterConfirm::CloseWindow => window.remove_window(),
            AfterConfirm::QuitApp => cx.quit(),
        }
    }

    // -----------------------------------------------------------------
    // Command palette

    fn show_commands(&mut self, _: &ShowCommands, window: &mut Window, cx: &mut Context<Self>) {
        let mut entries: Vec<(String, String)> = vec![
            ("keel.save".to_owned(), "File: Save".to_owned()),
            (
                "keel.closeWindow".to_owned(),
                "View: Close Window".to_owned(),
            ),
        ];
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
        let chosen = quick_pick::show(None, Some("Type a command".to_owned()), items, window, cx);
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Some(index)) = chosen.await else {
                return;
            };
            let command = entries[index].0.clone();
            this.update_in(cx, |this, window, cx| match command.as_str() {
                "keel.save" => this.save(&Save, window, cx),
                "keel.closeWindow" => this.close_window(&CloseWindow, window, cx),
                _ => {
                    if let Some(pane) = this.panes.get(this.active) {
                        let editor = pane.editor.clone();
                        editor.update(cx, |editor, cx| editor.focus(window, cx));
                    }
                    this.run_extension_command(command, Vec::new(), window, cx);
                }
            })
            .ok();
        })
        .detach();
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
            host,
            mut events,
            installed,
            data_dir,
        } = setup;
        let host = Rc::new(host);
        let mut titles = Vec::new();
        let mut bindings = Vec::new();
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
        // Extensions read the user's and the project's settings from the start.
        let _ = host.notify(
            "configuration/changed",
            json!({ "values": self.settings.merged() }),
        );
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
            "log" => eprintln!(
                "[extension {}] {}",
                params["level"].as_str().unwrap_or("info"),
                params["message"].as_str().unwrap_or_default()
            ),
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
                self.status_message = Some(format!("{id} could not start: {error}"));
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
                    if let Some(index) = self.pane_index(&uri) {
                        self.close_pane(index, window, cx);
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
                self.status_message = Some("Settings are not available yet".to_owned());
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
        buttons.push("Cancel");
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
        let uris: Vec<String> = self.panes.iter().map(|pane| pane.uri.clone()).collect();
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

impl Render for Workbench {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !self.first_frame_drawn {
            self.first_frame_drawn = true;
            if std::env::var_os("KEEL_STARTUP_TIMING").is_some() {
                window.on_next_frame(|_, _| {
                    if let Some(started) = crate::STARTED.get() {
                        eprintln!("keel: first frame after {:?}", started.elapsed());
                    }
                });
            }
        }
        let theme = cx.theme();
        let (foreground, muted, border, background) = (
            theme.foreground,
            theme.muted_foreground,
            theme.border,
            theme.background,
        );
        let panes = self.panes.iter().enumerate().map(|(index, pane)| {
            let name = self
                .documents
                .get(&pane.uri)
                .map(|document| {
                    if document.modified {
                        format!("{} ●", document.name())
                    } else {
                        document.name()
                    }
                })
                .unwrap_or_default();
            let header_color = if index == self.active {
                foreground
            } else {
                muted
            };
            div()
                .flex_1()
                .min_w_0()
                .h_full()
                .flex()
                .flex_col()
                .when(index > 0, |pane| pane.border_l_1().border_color(border))
                .child(
                    div()
                        .h(px(30.))
                        .px_3()
                        .flex()
                        .items_center()
                        .text_sm()
                        .text_color(header_color)
                        .border_b_1()
                        .border_color(border)
                        .child(name),
                )
                .child(
                    div()
                        .flex_1()
                        .min_h_0()
                        .child(Editor::new(&pane.editor).size_full().bordered(false)),
                )
        });

        let (mut left, mut right) = (Vec::new(), Vec::new());
        if let Some(extensions) = &self.extensions {
            let mut items: Vec<&StatusItem> = extensions
                .status_items
                .values()
                .filter(|item| item.visible)
                .collect();
            items.sort_by(|a, b| b.priority.total_cmp(&a.priority));
            for item in items {
                let text = strip_codicons(&item.text);
                if item.right {
                    right.push(text);
                } else {
                    left.push(text);
                }
            }
        }
        if let Some(message) = &self.status_message {
            left.push(message.clone());
        }

        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(background)
            .on_action(cx.listener(Self::save))
            .on_action(cx.listener(Self::close_window))
            .on_action(cx.listener(Self::quit))
            .on_action(cx.listener(Self::show_commands))
            .child(div().flex_1().min_h_0().flex().flex_row().children(panes))
            .child(
                div()
                    .h(px(24.))
                    .px_3()
                    .flex()
                    .items_center()
                    .justify_between()
                    .text_xs()
                    .text_color(muted)
                    .border_t_1()
                    .border_color(border)
                    .child(div().flex().gap_4().children(left))
                    .child(div().flex().gap_4().children(right)),
            )
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
fn show_error(message: &str, detail: &str, window: &mut Window, cx: &mut App) {
    // The dialog stays open on its own; there is no answer to wait for.
    let _dismissed = window.prompt(PromptLevel::Critical, message, Some(detail), &["OK"], cx);
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
