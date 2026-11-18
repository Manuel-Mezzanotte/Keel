//! Language servers for the open files: starting them (and installing them
//! when missing), keeping them in step with the editors, their messages in
//! the Output view, and restarting them when they stop.
//!
//! The documents reach this module through the same notifications the
//! extension host gets (`documents/opened`, `changed`, `saved`, `closed`).
//! Changes go to the servers at once, so a request made right after a
//! keystroke, such as a completion, always sees the new text; starting
//! servers, which needs the window, is queued.

use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::rc::Rc;
use std::sync::Arc;
use std::time::Instant;

use futures::StreamExt as _;
use futures::channel::mpsc::{UnboundedSender, unbounded};
use futures::future::{Either, select};
use lsp::lsp_types::{
    self, Diagnostic, DidChangeTextDocumentParams, DidCloseTextDocumentParams,
    DidOpenTextDocumentParams, DidSaveTextDocumentParams, PublishDiagnosticsParams,
    TextDocumentContentChangeEvent, TextDocumentIdentifier, TextDocumentItem,
    TextDocumentSyncCapability, TextDocumentSyncKind, TextDocumentSyncSaveOptions,
    VersionedTextDocumentIdentifier,
    notification::{
        DidChangeTextDocument, DidCloseTextDocument, DidOpenTextDocument, DidSaveTextDocument,
    },
};
use lsp::prerequisites::{self, Prerequisite};
use lsp::servers::{self, ServerSpec};
use lsp::{LanguageServer, ResponseError, ServerEvent};

use super::panel_views::OutputChannel;
use super::*;

actions!(keel, [RestartLanguageServers]);

/// A server that stops more often than this within [`RESTART_WINDOW`] is
/// left stopped, as VS Code does.
const MAX_RESTARTS: usize = 4;
const RESTART_WINDOW: Duration = Duration::from_secs(180);
/// How long a server may take to answer `initialize`.
const INITIALIZE_TIMEOUT: Duration = Duration::from_secs(60);

/// What the language servers' queue carries.
pub(super) enum LspInput {
    Opened {
        uri: String,
        language_id: String,
    },
    /// A file changed for a server that wants its diagnostics asked for.
    Pull {
        id: &'static str,
        uri: String,
    },
    Server {
        id: &'static str,
        generation: u64,
        event: ServerEvent,
    },
    /// A server cannot start without something the user installs.
    Missing {
        server: &'static str,
        prerequisite: Prerequisite,
    },
}

/// A file as the servers know it: the text last sent and its version.
struct SyncedDocument {
    language_id: String,
    version: i32,
    text: String,
}

enum Slot {
    Starting {
        generation: u64,
        _task: Task<()>,
    },
    Running(Rc<RunningServer>),
    /// It failed to start or stopped too often: left alone until
    /// "Restart Language Servers".
    Stopped,
}

/// A server that answered `initialize`.
pub(super) struct RunningServer {
    pub(super) spec: &'static ServerSpec,
    pub(super) server: LanguageServer,
    generation: u64,
    /// The documents it was told about with `didOpen`.
    open: RefCell<HashSet<String>>,
    /// The files it asked to hear about when they change on disk.
    watchers: RefCell<Vec<FileWatcher>>,
    /// It wants its diagnostics asked for (`textDocument/diagnostic`)
    /// instead of sending them, as ESLint does.
    pulls_diagnostics: std::cell::Cell<bool>,
    /// Commands it registered while running (`workspace/executeCommand`).
    commands: RefCell<HashSet<String>>,
}

impl RunningServer {
    /// Whether it runs `command`, declared at the start or registered later.
    fn runs_command(&self, command: &str) -> bool {
        self.commands.borrow().contains(command)
            || self
                .server
                .capabilities()
                .and_then(|capabilities| capabilities.execute_command_provider.as_ref())
                .is_some_and(|provider| provider.commands.iter().any(|known| known == command))
    }
}

/// A `workspace/didChangeWatchedFiles` registration: a pattern and the
/// kinds of change it wants (1 created, 2 changed, 4 deleted).
struct FileWatcher {
    registration: String,
    pattern: globset::GlobMatcher,
    /// For a relative pattern, the folder it starts from.
    base: Option<PathBuf>,
    kinds: u64,
}

impl FileWatcher {
    fn matches(&self, path: &Path, kind: u64) -> bool {
        if self.kinds & kind == 0 {
            return false;
        }
        match &self.base {
            Some(base) => path
                .strip_prefix(base)
                .is_ok_and(|rest| self.pattern.is_match(rest)),
            None => self.pattern.is_match(path),
        }
    }
}

/// Reads the watchers of a `client/registerCapability` registration.
fn file_watchers(registration: &Value) -> Vec<FileWatcher> {
    let id = registration["id"].as_str().unwrap_or_default().to_owned();
    registration["registerOptions"]["watchers"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|watcher| {
            let (pattern, base) = match &watcher["globPattern"] {
                Value::String(pattern) => (pattern.clone(), None),
                relative => {
                    let base = match &relative["baseUri"] {
                        Value::String(uri) => Uri::parse(uri)?.file_path()?,
                        folder => Uri::parse(folder["uri"].as_str()?)?.file_path()?,
                    };
                    (relative["pattern"].as_str()?.to_owned(), Some(base))
                }
            };
            let glob = globset::GlobBuilder::new(&pattern)
                .literal_separator(true)
                .build()
                .ok()?;
            Some(FileWatcher {
                registration: id.clone(),
                pattern: glob.compile_matcher(),
                base,
                kinds: watcher["kind"].as_u64().unwrap_or(7),
            })
        })
        .collect()
}

impl RunningServer {
    fn sync_kind(&self) -> TextDocumentSyncKind {
        match self
            .server
            .capabilities()
            .and_then(|capabilities| capabilities.text_document_sync.as_ref())
        {
            Some(TextDocumentSyncCapability::Kind(kind)) => *kind,
            Some(TextDocumentSyncCapability::Options(options)) => {
                options.change.unwrap_or(TextDocumentSyncKind::NONE)
            }
            None => TextDocumentSyncKind::NONE,
        }
    }

    /// Whether the server wants `didSave`, and with the text.
    fn save_options(&self) -> Option<bool> {
        let sync = self
            .server
            .capabilities()
            .and_then(|capabilities| capabilities.text_document_sync.as_ref())?;
        match sync {
            TextDocumentSyncCapability::Kind(_) => None,
            TextDocumentSyncCapability::Options(options) => match options.save.as_ref()? {
                TextDocumentSyncSaveOptions::Supported(true) => Some(false),
                TextDocumentSyncSaveOptions::Supported(false) => None,
                TextDocumentSyncSaveOptions::SaveOptions(save) => {
                    Some(save.include_text.unwrap_or(false))
                }
            },
        }
    }
}

/// What the editors' providers share with the window: the servers that
/// are running and the documents they know. Providers read it while the
/// window itself may be busy, so it lives apart.
#[derive(Default)]
pub(super) struct LanguageState {
    running: RefCell<HashMap<&'static str, Rc<RunningServer>>>,
    documents: RefCell<HashMap<String, SyncedDocument>>,
    /// A copy of the diagnostics, for the quick fixes the servers are asked.
    diagnostics: RefCell<HashMap<String, BTreeMap<&'static str, Vec<Diagnostic>>>>,
    /// The window's queue, for pulling diagnostics after a change.
    sender: RefCell<Option<UnboundedSender<LspInput>>>,
}

/// The language servers of a window.
pub(super) struct Languages {
    sender: UnboundedSender<LspInput>,
    servers: HashMap<&'static str, Slot>,
    pub(super) state: Rc<LanguageState>,
    /// The latest diagnostics of each file, by server.
    pub(super) diagnostics: BTreeMap<String, BTreeMap<&'static str, Vec<Diagnostic>>>,
    /// Work in progress the servers report, such as indexing.
    progress: BTreeMap<(&'static str, String), String>,
    /// Diagnostics being asked for, one request per server and file: a
    /// newer change replaces the waiting one.
    pulls: HashMap<(&'static str, String), Task<()>>,
    /// The last answer of each pull, so the server can say "unchanged".
    pull_results: HashMap<(&'static str, String), String>,
    restarts: HashMap<&'static str, Vec<Instant>>,
    /// Prerequisites already offered to install, once per window.
    offered: HashSet<Prerequisite>,
    next_generation: u64,
    /// Tests run the fake server in a thread when this is set, and no
    /// server at all otherwise.
    #[cfg(test)]
    pub(super) fake: bool,
    _driver: Task<()>,
}

impl LanguageState {
    /// What server `server` reported about `uri`.
    pub(super) fn diagnostics_of(&self, uri: &str, server: &str) -> Vec<Diagnostic> {
        self.diagnostics
            .borrow()
            .get(uri)
            .and_then(|by_server| by_server.get(server))
            .cloned()
            .unwrap_or_default()
    }

    /// Every problem reported about `uri`, by servers and extensions.
    pub(super) fn all_diagnostics(&self, uri: &str) -> Vec<Diagnostic> {
        self.diagnostics
            .borrow()
            .get(uri)
            .map(|by_owner| by_owner.values().flatten().cloned().collect())
            .unwrap_or_default()
    }

    pub(super) fn mirror_diagnostics(
        &self,
        uri: &str,
        by_server: Option<&BTreeMap<&'static str, Vec<Diagnostic>>>,
    ) {
        let mut diagnostics = self.diagnostics.borrow_mut();
        match by_server {
            Some(by_server) => {
                diagnostics.insert(uri.to_owned(), by_server.clone());
            }
            None => {
                diagnostics.remove(uri);
            }
        }
    }

    /// Tells the servers that know `uri` its text is now `text`, if it
    /// changed, as one small edit. Requests call this first with the text
    /// they are about.
    pub(super) fn sync(&self, uri: &str, text: &str) {
        let mut documents = self.documents.borrow_mut();
        let Some(document) = documents.get_mut(uri) else {
            return;
        };
        let Some(change) = lsp::text::content_change(&document.text, text) else {
            return;
        };
        document.version += 1;
        document.text = text.to_owned();
        let version = document.version;
        let Ok(parsed) = uri.parse::<lsp_types::Uri>() else {
            return;
        };
        for running in self.servers_for(uri) {
            self.queue_pull(&running, uri);
            let change = match running.sync_kind() {
                TextDocumentSyncKind::INCREMENTAL => change.clone(),
                TextDocumentSyncKind::FULL => TextDocumentContentChangeEvent {
                    range: None,
                    range_length: None,
                    text: text.to_owned(),
                },
                _ => continue,
            };
            running
                .server
                .notify::<DidChangeTextDocument>(DidChangeTextDocumentParams {
                    text_document: VersionedTextDocumentIdentifier::new(parsed.clone(), version),
                    content_changes: vec![change],
                });
        }
    }

    pub(super) fn saved(&self, uri: &str) {
        let text = self
            .documents
            .borrow()
            .get(uri)
            .map(|document| document.text.clone());
        for running in self.servers_for(uri) {
            let Some(include_text) = running.save_options() else {
                continue;
            };
            let Ok(parsed) = uri.parse() else { continue };
            running
                .server
                .notify::<DidSaveTextDocument>(DidSaveTextDocumentParams {
                    text_document: TextDocumentIdentifier::new(parsed),
                    text: if include_text { text.clone() } else { None },
                });
        }
    }

    pub(super) fn closed(&self, uri: &str) {
        for running in self.servers_for(uri) {
            running.open.borrow_mut().remove(uri);
            let Ok(parsed) = uri.parse() else { continue };
            running
                .server
                .notify::<DidCloseTextDocument>(DidCloseTextDocumentParams {
                    text_document: TextDocumentIdentifier::new(parsed),
                });
        }
        self.documents.borrow_mut().remove(uri);
    }

    /// Asks for the diagnostics of `uri` from a server that wants them pulled.
    fn queue_pull(&self, running: &RunningServer, uri: &str) {
        if !running.pulls_diagnostics.get() {
            return;
        }
        if let Some(sender) = self.sender.borrow().as_ref() {
            let _ = sender.unbounded_send(LspInput::Pull {
                id: running.spec.id,
                uri: uri.to_owned(),
            });
        }
    }

    /// Tells `running` about `uri`, once.
    pub(super) fn open_in_server(&self, running: &RunningServer, uri: &str) {
        let documents = self.documents.borrow();
        let Some(document) = documents.get(uri) else {
            return;
        };
        if !running.open.borrow_mut().insert(uri.to_owned()) {
            return;
        }
        let Ok(parsed) = uri.parse() else { return };
        running
            .server
            .notify::<DidOpenTextDocument>(DidOpenTextDocumentParams {
                text_document: TextDocumentItem::new(
                    parsed,
                    document.language_id.clone(),
                    document.version,
                    document.text.clone(),
                ),
            });
        drop(documents);
        self.queue_pull(running, uri);
    }

    /// The running servers that know `uri`.
    pub(super) fn servers_for(&self, uri: &str) -> Vec<Rc<RunningServer>> {
        self.running
            .borrow()
            .values()
            .filter(|running| running.open.borrow().contains(uri))
            .cloned()
            .collect()
    }
}

impl Languages {
    pub(super) fn new(window: &mut Window, cx: &mut Context<Workbench>) -> Self {
        let (sender, mut receiver) = unbounded::<LspInput>();
        let driver = cx.spawn_in(window, async move |this, cx| {
            while let Some(input) = receiver.next().await {
                if this
                    .update_in(cx, |this, window, cx| this.lsp_input(input, window, cx))
                    .is_err()
                {
                    break;
                }
            }
        });
        Self {
            sender: sender.clone(),
            servers: HashMap::new(),
            state: Rc::new(LanguageState {
                sender: RefCell::new(Some(sender.clone())),
                ..Default::default()
            }),
            pulls: HashMap::new(),
            pull_results: HashMap::new(),
            diagnostics: BTreeMap::new(),
            progress: BTreeMap::new(),
            restarts: HashMap::new(),
            offered: HashSet::new(),
            next_generation: 1,
            #[cfg(test)]
            fake: false,
            _driver: driver,
        }
    }

    /// Follows what an extension-host notification says about a document.
    /// Only files on disk go to language servers.
    pub(super) fn document_event(&self, method: &str, params: &Value) {
        let uri = params["uri"].as_str().unwrap_or_default();
        if !uri.starts_with("file:") {
            return;
        }
        match method {
            "documents/opened" => {
                let language_id = params["languageId"].as_str().unwrap_or_default().to_owned();
                let text = params["text"].as_str().unwrap_or_default().to_owned();
                self.state.documents.borrow_mut().insert(
                    uri.to_owned(),
                    SyncedDocument {
                        language_id: language_id.clone(),
                        version: 1,
                        text,
                    },
                );
                let _ = self.sender.unbounded_send(LspInput::Opened {
                    uri: uri.to_owned(),
                    language_id,
                });
            }
            "documents/changed" => {
                if let Some(text) = params["text"].as_str() {
                    self.sync(uri, text);
                }
            }
            "documents/saved" => self.saved(uri),
            "documents/closed" => self.closed(uri),
            _ => {}
        }
    }

    pub(super) fn sync(&self, uri: &str, text: &str) {
        self.state.sync(uri, text);
    }

    fn saved(&self, uri: &str) {
        self.state.saved(uri);
    }

    fn closed(&self, uri: &str) {
        self.state.closed(uri);
    }

    pub(super) fn open_in_server(&self, running: &RunningServer, uri: &str) {
        self.state.open_in_server(running, uri);
    }

    pub(super) fn servers_for(&self, uri: &str) -> Vec<Rc<RunningServer>> {
        self.state.servers_for(uri)
    }

    /// What the servers are busy with, for the status bar.
    pub(super) fn progress_text(&self) -> Option<&str> {
        self.progress.values().next().map(String::as_str)
    }

    #[cfg(test)]
    pub(super) fn is_running(&self, id: &str) -> bool {
        matches!(self.servers.get(id), Some(Slot::Running(_)))
    }

    #[cfg(test)]
    pub(super) fn running_pid(&self, id: &str) -> Option<(u64, Option<u32>)> {
        match self.servers.get(id) {
            Some(Slot::Running(running)) => Some((running.generation, running.server.pid())),
            _ => None,
        }
    }
}

impl Workbench {
    pub(super) fn lsp_input(
        &mut self,
        input: LspInput,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match input {
            LspInput::Opened { uri, language_id } => {
                if !self.languages.state.documents.borrow().contains_key(&uri) {
                    return;
                }
                let Some(root) = self.server_root(&uri) else {
                    return;
                };
                for spec in servers::servers_for(&language_id, &root) {
                    if self.server_disabled(spec) {
                        continue;
                    }
                    match self.languages.servers.get(spec.id) {
                        Some(Slot::Running(running)) => {
                            let running = running.clone();
                            self.languages.open_in_server(&running, &uri);
                        }
                        Some(_) => {}
                        None => self.start_server(spec, root.clone(), cx),
                    }
                }
            }
            LspInput::Pull { id, uri } => self.pull_diagnostics(id, uri, cx),
            LspInput::Server {
                id,
                generation,
                event,
            } => self.server_event(id, generation, event, window, cx),
            LspInput::Missing {
                server,
                prerequisite,
            } => self.offer_prerequisite(server, prerequisite, window, cx),
        }
    }

    /// Runs a command that belongs to the editor rather than to a server.
    /// Returns whether it knew the command.
    pub(super) fn client_command(
        &mut self,
        command: &str,
        arguments: &[Value],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        match command {
            // jdtls's quick fixes and refactorings carry their edit here.
            "java.apply.workspaceEdit" => {
                if let Some(edit) = arguments.first().and_then(|edit| {
                    serde_json::from_value::<lsp_types::WorkspaceEdit>(edit.clone()).ok()
                }) {
                    self.apply_workspace_edit(&edit, window, cx);
                }
                true
            }
            // jdtls found a Spring Boot project: VS Code's extension then
            // has Spring Boot's server follow the classpath.
            "vscode-spring-boot.ls.start" => {
                self.enable_spring_boot_classpath(cx);
                true
            }
            // VS Code's Java extension reloads extension bundles; Keel's
            // are loaded at the start.
            "_java.reloadBundles.command" => true,
            _ => false,
        }
    }

    /// Tells Spring Boot's server to follow the project's classpath, which
    /// it then asks jdtls for. Only once both run.
    fn enable_spring_boot_classpath(&self, cx: &mut Context<Self>) {
        let running = self.languages.state.running.borrow();
        if let (Some(spring), true) = (running.get("spring-boot"), running.contains_key("jdtls")) {
            let request = spring.server.request_raw(
                "workspace/executeCommand",
                json!({ "command": "sts.vscode-spring-boot.enableClasspathListening", "arguments": [true] }),
            );
            cx.background_spawn(async move {
                let _ = request.await;
            })
            .detach();
        }
    }

    /// Shows a class of a library, as jdtls gives it: its source when the
    /// library comes with one, or what jdtls rebuilds from the class.
    pub(super) fn open_class_file(
        &mut self,
        uri: String,
        range: lsp_types::Range,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let column = self.active.min(self.panes.len().saturating_sub(1));
        if self.documents.contains_key(&uri) {
            self.show_tab(&uri, column, OpenAs::Normal, true, window, cx);
            self.select_lsp_range(range, window, cx);
            return;
        }
        let Some(Slot::Running(running)) = self.languages.servers.get("jdtls") else {
            return;
        };
        let request = running
            .server
            .request_raw("java/classFileContents", json!({ "uri": uri }));
        cx.spawn_in(window, async move |this, cx| {
            let answer = request.await;
            this.update_in(cx, |this, window, cx| {
                let text = match answer {
                    Ok(Value::String(text)) if !text.is_empty() => text,
                    Ok(_) => return,
                    Err(error) => {
                        this.report(format!("Java: {error:#}"));
                        cx.notify();
                        return;
                    }
                };
                if !this.documents.contains_key(&uri) {
                    this.add_document(uri.clone(), Backing::Library, "java".to_owned(), text);
                }
                this.show_tab(&uri, column, OpenAs::Normal, true, window, cx);
                this.select_lsp_range(range, window, cx);
            })
            .ok();
        })
        .detach();
    }

    /// Asks once whether to install what a server needs, in a terminal.
    fn offer_prerequisite(
        &mut self,
        server: &str,
        prerequisite: Prerequisite,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.languages.offered.insert(prerequisite) {
            return;
        }
        let strings = self.strings();
        let command = prerequisite.install_command();
        let message = strings.prerequisite_missing(server, &prerequisite.name());
        let detail = strings.prerequisite_detail(&command);
        let buttons = [strings.install_in_terminal, strings.cancel];
        let answer = window.prompt(PromptLevel::Warning, &message, Some(&detail), &buttons, cx);
        let name = strings.install_terminal_name(&prerequisite.name());
        cx.spawn_in(window, async move |this, cx| {
            if answer.await.ok() != Some(0) {
                return;
            }
            this.update_in(cx, |this, window, cx| {
                let spawn = terminal_view::shell_spawn(this.terminal_folder(), cx);
                if let Some(view) = this.add_terminal(spawn, None, true, window, cx) {
                    view.read(cx).run(&command);
                    this.name_terminal(&view, name);
                }
            })
            .ok();
        })
        .detach();
    }

    /// The folder a server for `uri` works in: the project, or the file's
    /// own folder in a window without one.
    fn server_root(&self, uri: &str) -> Option<PathBuf> {
        let path = Uri::parse(uri)?.file_path()?;
        let folder = path.parent()?;
        // A real project: `.git`, `.vscode` or `.keel` at its top.
        let marked = |root: &Path| {
            [".git", ".vscode", ".keel"]
                .iter()
                .any(|name| root.join(name).exists())
        };
        match &self.project.root {
            // A file opened alone, outside any project, is its own
            // "project": servers work from the nearest build file instead,
            // such as a `pom.xml` above `src/main/java/...`.
            Some(root) if root == folder && !marked(root) => {
                Some(servers::build_root(folder).unwrap_or_else(|| root.clone()))
            }
            Some(root) => Some(root.clone()),
            None => Some(servers::build_root(folder).unwrap_or_else(|| folder.to_path_buf())),
        }
    }

    /// `keel.languageServers.disabled` lists servers by id, such as `"eslint"`.
    fn server_disabled(&self, spec: &ServerSpec) -> bool {
        self.settings
            .get("keel.languageServers.disabled")
            .and_then(Value::as_array)
            .is_some_and(|ids| ids.iter().any(|id| id.as_str() == Some(spec.id)))
    }

    fn start_server(&mut self, spec: &'static ServerSpec, root: PathBuf, cx: &mut Context<Self>) {
        let generation = self.languages.next_generation;
        self.languages.next_generation += 1;
        let sender = self.languages.sender.clone();
        #[cfg_attr(test, allow(unused_variables))]
        let sender_for_missing = sender.clone();
        let on_event = move |event| {
            let _ = sender.unbounded_send(LspInput::Server {
                id: spec.id,
                generation,
                event,
            });
        };
        let servers_dir = self.data_dir.as_deref().map(crate::paths::servers_dir);
        let auto_install = self
            .settings
            .get("keel.languageServers.autoInstall")
            .and_then(Value::as_bool)
            .unwrap_or(true);
        // Tests never start real servers; some use the fake one.
        #[cfg(test)]
        let fake = self.languages.fake;
        #[cfg(test)]
        if !fake {
            return;
        }
        #[cfg(not(test))]
        let fake = false;
        let user_settings = lsp::nested_settings(&self.settings.merged());
        // jdtls loads Spring Boot's parts when it starts: in its projects
        // they are installed first, unless Spring Boot's server is off.
        let spring_first = spec.id == "jdtls"
            && auto_install
            && servers::spec("spring-boot")
                .is_some_and(|spring| spring.wanted_in(&root) && !self.server_disabled(spring));
        let options_dir = servers_dir.clone().unwrap_or_default();
        let task = cx.spawn(async move |this, cx| {
            let started: anyhow::Result<LanguageServer> = if fake {
                lsp::fake::spawn()
                    .map_err(anyhow::Error::from)
                    .and_then(|(reader, writer)| {
                        LanguageServer::connect(spec.name, reader, writer, Arc::new(on_event), None)
                    })
            } else {
                // What the server needs comes first: no point installing
                // a Java server without Java.
                let checked = cx
                    .background_spawn(async move {
                        prerequisites::check_all(spec.needs, servers::login_environment())
                    })
                    .await;
                let mut extra_env = match checked {
                    Ok(extra_env) => extra_env,
                    Err(missing) => {
                        let _ = sender_for_missing.unbounded_send(LspInput::Missing {
                            server: spec.name,
                            prerequisite: missing.prerequisite,
                        });
                        this.update(cx, |this, cx| {
                            this.server_failed(spec, generation, missing.into(), cx)
                        })
                        .ok();
                        return;
                    }
                };
                // Programs the server runs, such as ShellCheck for Bash.
                if !spec.helpers.is_empty() {
                    let (dir, root) = (servers_dir.clone().unwrap_or_default(), root.clone());
                    let install = auto_install && servers_dir.is_some();
                    let (folders, problems) = cx
                        .background_spawn(async move {
                            servers::prepare_helpers(
                                spec,
                                &dir,
                                &root,
                                servers::login_environment(),
                                install,
                            )
                        })
                        .await;
                    servers::with_path_first(
                        &folders,
                        servers::login_environment(),
                        &mut extra_env,
                    );
                    this.update(cx, |this, _| {
                        for problem in problems {
                            this.log_to(
                                OutputChannel::Languages,
                                format!("[{}] {problem}", spec.name),
                            );
                        }
                    })
                    .ok();
                }
                if spring_first
                    && let (Some(spring), Some(dir)) =
                        (servers::spec("spring-boot"), servers_dir.clone())
                {
                    let root = root.clone();
                    let installed = cx
                        .background_spawn(async move {
                            let env = servers::login_environment();
                            match servers::find(spring, &dir, &root, env) {
                                Some(_) => Ok(String::new()),
                                None => servers::install(spring, &dir, &root, env),
                            }
                        })
                        .await;
                    if let Err(error) = installed {
                        this.update(cx, |this, _| {
                            this.log_to(
                                OutputChannel::Languages,
                                format!("[{}] {error:#}", spring.name),
                            );
                        })
                        .ok();
                    }
                }
                let found = {
                    let (servers_dir, root) = (servers_dir.clone(), root.clone());
                    cx.background_spawn(async move {
                        let env = servers::login_environment();
                        let dir = servers_dir.unwrap_or_default();
                        servers::find(spec, &dir, &root, env)
                    })
                    .await
                };
                let program = match (found, servers_dir) {
                    (Some(program), _) => Ok(program),
                    (None, Some(servers_dir)) if auto_install => {
                        this.update(cx, |this, cx| {
                            let strings = this.strings();
                            this.log_to(
                                OutputChannel::Languages,
                                format!("[{}] {}", spec.name, strings.lsp_installing(spec.name)),
                            );
                            this.status_message = Some(strings.lsp_installing(spec.name));
                            cx.notify();
                        })
                        .ok();
                        let root = root.clone();
                        cx.background_spawn(async move {
                            let env = servers::login_environment();
                            servers::install(spec, &servers_dir, &root, env)?;
                            servers::find(spec, &servers_dir, &root, env).ok_or_else(|| {
                                anyhow::anyhow!("installed, but {} is still missing", spec.program)
                            })
                        })
                        .await
                    }
                    (None, _) => Err(anyhow::anyhow!("{} is not installed", spec.program)),
                };
                match program {
                    Ok(program) => {
                        let root = root.clone();
                        cx.background_spawn(async move {
                            let command = servers::command(
                                spec,
                                program,
                                &root,
                                servers::login_environment(),
                                &extra_env,
                            );
                            LanguageServer::start(spec.name, &command, on_event)
                        })
                        .await
                    }
                    Err(error) => Err(error),
                }
            };
            let server = match started {
                Ok(server) => server,
                Err(error) => {
                    this.update(cx, |this, cx| {
                        this.server_failed(spec, generation, error, cx)
                    })
                    .ok();
                    return;
                }
            };
            // After the install: jdtls names files of Spring Boot's.
            let options =
                servers::initialization_options(spec, &root, &user_settings, &options_dir);
            let initialized = {
                let initialize = server.initialize(&root, options);
                let timeout = cx.background_executor().timer(INITIALIZE_TIMEOUT);
                futures::pin_mut!(initialize);
                match select(initialize, timeout).await {
                    Either::Left((result, _)) => result.map(|_| ()),
                    Either::Right(_) => Err(anyhow::anyhow!("no answer to initialize")),
                }
            };
            this.update(cx, |this, cx| match initialized {
                Ok(()) => this.server_ready(spec, generation, server, cx),
                Err(error) => this.server_failed(spec, generation, error, cx),
            })
            .ok();
        });
        self.languages.servers.insert(
            spec.id,
            Slot::Starting {
                generation,
                _task: task,
            },
        );
    }

    fn server_ready(
        &mut self,
        spec: &'static ServerSpec,
        generation: u64,
        server: LanguageServer,
        cx: &mut Context<Self>,
    ) {
        if !matches!(self.languages.servers.get(spec.id), Some(Slot::Starting { generation: current, .. }) if *current == generation)
        {
            return;
        }
        server.set_settings(self.server_settings(spec));
        let pulls = server
            .capabilities()
            .is_some_and(|capabilities| capabilities.diagnostic_provider.is_some());
        let pid = server
            .pid()
            .map(|pid| format!(" (pid {pid})"))
            .unwrap_or_default();
        self.log_to(
            OutputChannel::Languages,
            format!("[{}] {}{pid}", spec.name, self.strings().lsp_started),
        );
        let running = Rc::new(RunningServer {
            spec,
            server,
            generation,
            open: RefCell::new(HashSet::new()),
            watchers: RefCell::new(Vec::new()),
            pulls_diagnostics: std::cell::Cell::new(pulls),
            commands: RefCell::new(HashSet::new()),
        });
        self.languages
            .servers
            .insert(spec.id, Slot::Running(running.clone()));
        self.languages
            .state
            .running
            .borrow_mut()
            .insert(spec.id, running.clone());
        let uris: Vec<String> = self
            .languages
            .state
            .documents
            .borrow()
            .iter()
            .filter(|(_, document)| spec.serves(&document.language_id))
            .map(|(uri, _)| uri.clone())
            .collect();
        for uri in uris {
            self.languages.open_in_server(&running, &uri);
        }
        if matches!(spec.id, "jdtls" | "spring-boot") {
            self.enable_spring_boot_classpath(cx);
        }
        if self
            .status_message
            .as_deref()
            .is_some_and(|message| message == self.strings().lsp_installing(spec.name))
        {
            self.status_message = None;
        }
        cx.notify();
    }

    fn server_failed(
        &mut self,
        spec: &'static ServerSpec,
        generation: u64,
        error: anyhow::Error,
        cx: &mut Context<Self>,
    ) {
        if !matches!(self.languages.servers.get(spec.id), Some(Slot::Starting { generation: current, .. }) if *current == generation)
        {
            return;
        }
        self.languages.servers.insert(spec.id, Slot::Stopped);
        let message = self.strings().lsp_failed(spec.name, &format!("{error:#}"));
        self.log_to(
            OutputChannel::Languages,
            format!("[{}] {message}", spec.name),
        );
        self.status_message = Some(message);
        cx.notify();
    }

    fn server_event(
        &mut self,
        id: &'static str,
        generation: u64,
        event: ServerEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let current = match self.languages.servers.get(id) {
            Some(Slot::Running(running)) if running.generation == generation => {
                Some(running.clone())
            }
            Some(Slot::Starting {
                generation: starting,
                ..
            }) if *starting == generation => None,
            // A server that was replaced or stopped.
            _ => return,
        };
        let Some(spec) = servers::spec(id) else {
            return;
        };
        match event {
            ServerEvent::Notification { method, params } => {
                self.server_notification(spec, &method, params, cx)
            }
            ServerEvent::Request { id, method, params } => {
                let Some(running) = current else { return };
                self.server_request(&running, id, &method, params, window, cx);
            }
            ServerEvent::Stderr(line) => {
                self.log_to(OutputChannel::Languages, format!("[{}] {line}", spec.name));
            }
            ServerEvent::Exited(code) => self.server_exited(spec, code, cx),
        }
    }

    fn server_notification(
        &mut self,
        spec: &'static ServerSpec,
        method: &str,
        params: Value,
        cx: &mut Context<Self>,
    ) {
        match method {
            "textDocument/publishDiagnostics" => {
                let Ok(params) = serde_json::from_value::<PublishDiagnosticsParams>(params) else {
                    return;
                };
                let uri = normalize_uri(params.uri.as_str());
                self.set_diagnostics(spec, &uri, params.diagnostics, cx);
            }
            // A pull server says its answers are stale: ask again.
            "workspace/diagnostic/refresh" => {
                let Some(running) = self.languages.state.running.borrow().get(spec.id).cloned()
                else {
                    return;
                };
                running.pulls_diagnostics.set(true);
                let uris: Vec<String> = running.open.borrow().iter().cloned().collect();
                for uri in uris {
                    self.pull_diagnostics(spec.id, uri, cx);
                }
            }
            "window/logMessage" | "window/showMessage" => {
                let message = params["message"].as_str().unwrap_or_default();
                self.log_to(
                    OutputChannel::Languages,
                    format!("[{}] {message}", spec.name),
                );
                // Errors and warnings shown to the user, as VS Code does.
                let kind = params["type"].as_i64().unwrap_or(4);
                if method == "window/showMessage" && kind <= 2 {
                    self.status_message = Some(format!("{}: {message}", spec.name));
                    cx.notify();
                }
            }
            "client/registerCapability" => {
                let Some(running) = self.languages.state.running.borrow().get(spec.id).cloned()
                else {
                    return;
                };
                for registration in params["registrations"].as_array().into_iter().flatten() {
                    match registration["method"].as_str() {
                        Some("workspace/didChangeWatchedFiles") => {
                            running
                                .watchers
                                .borrow_mut()
                                .extend(file_watchers(registration));
                        }
                        // Diagnostics to ask for rather than wait for.
                        Some("textDocument/diagnostic") => {
                            running.pulls_diagnostics.set(true);
                            let uris: Vec<String> = running.open.borrow().iter().cloned().collect();
                            for uri in uris {
                                self.languages.state.queue_pull(&running, &uri);
                            }
                        }
                        Some("workspace/executeCommand") => {
                            running.commands.borrow_mut().extend(
                                registration["registerOptions"]["commands"]
                                    .as_array()
                                    .into_iter()
                                    .flatten()
                                    .filter_map(|command| command.as_str().map(str::to_owned)),
                            );
                        }
                        // VS Code answers this registration with the
                        // settings, and servers such as ESLint wait for it
                        // before checking anything.
                        Some("workspace/didChangeConfiguration") => {
                            running.server.notify_raw(
                                "workspace/didChangeConfiguration",
                                json!({ "settings": self.server_settings(spec) }),
                            );
                        }
                        _ => {}
                    }
                }
            }
            "client/unregisterCapability" => {
                let Some(running) = self.languages.state.running.borrow().get(spec.id).cloned()
                else {
                    return;
                };
                for gone in params["unregisterations"].as_array().into_iter().flatten() {
                    let id = gone["id"].as_str().unwrap_or_default();
                    running
                        .watchers
                        .borrow_mut()
                        .retain(|watcher| watcher.registration != id);
                }
            }
            "$/progress" => {
                let token = match &params["token"] {
                    Value::String(token) => token.clone(),
                    other => other.to_string(),
                };
                let value = &params["value"];
                let key = (spec.id, token);
                match value["kind"].as_str() {
                    Some("begin") | Some("report") => {
                        let title = match value["title"].as_str() {
                            Some(title) => title.to_owned(),
                            None => self
                                .languages
                                .progress
                                .get(&key)
                                .and_then(|text| text.split(": ").nth(1))
                                .map(|text| text.split(" — ").next().unwrap_or(text).to_owned())
                                .unwrap_or_default(),
                        };
                        let mut text = format!("{}: {title}", spec.name);
                        if let Some(message) = value["message"].as_str() {
                            text.push_str(&format!(" — {message}"));
                        }
                        if let Some(percentage) = value["percentage"].as_u64() {
                            text.push_str(&format!(" {percentage}%"));
                        }
                        self.languages.progress.insert(key, text);
                    }
                    Some("end") => {
                        self.languages.progress.remove(&key);
                        // Work such as loading the project changes the
                        // answers, and servers like csharp-ls do not say
                        // so: ask again for the open files' diagnostics.
                        let running = self.languages.state.running.borrow().get(spec.id).cloned();
                        if let Some(running) =
                            running.filter(|running| running.pulls_diagnostics.get())
                        {
                            let uris: Vec<String> = running.open.borrow().iter().cloned().collect();
                            for uri in uris {
                                self.pull_diagnostics(spec.id, uri, cx);
                            }
                        }
                    }
                    _ => {}
                }
                cx.notify();
            }
            _ => {}
        }
    }

    fn server_request(
        &mut self,
        running: &RunningServer,
        id: Value,
        method: &str,
        params: Value,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let answer = match method {
            // Edits a server makes itself, such as after a quick fix.
            "workspace/applyEdit" => {
                match serde_json::from_value::<lsp_types::WorkspaceEdit>(params["edit"].clone()) {
                    Ok(edit) => {
                        self.apply_workspace_edit(&edit, window, cx);
                        Ok(json!({ "applied": true }))
                    }
                    Err(error) => {
                        Ok(json!({ "applied": false, "failureReason": error.to_string() }))
                    }
                }
            }
            // The server wants a file shown, such as rust-analyzer's.
            "window/showDocument" => {
                let uri = normalize_uri(params["uri"].as_str().unwrap_or_default());
                let range = serde_json::from_value::<lsp_types::Range>(params["selection"].clone())
                    .unwrap_or_default();
                if params["external"].as_bool() == Some(true) {
                    cx.open_url(&uri);
                } else {
                    self.open_lsp_range(&uri, range, OpenAs::Normal, window, cx);
                }
                Ok(json!({ "success": true }))
            }
            // Older ESLint servers ask before running the project's ESLint.
            "eslint/confirmESLintExecution" => Ok(json!(4)),
            "eslint/openDoc" => {
                if let Some(url) = params["url"].as_str() {
                    cx.open_url(url);
                }
                Ok(json!({}))
            }
            // Spring Boot's server asks about the project's Java through
            // the editor: jdtls answers, with the parts it loaded for it.
            method if method.starts_with("sts/") => {
                let jdtls = self.languages.state.running.borrow().get("jdtls").cloned();
                match (servers::spring_boot_relay(method, &params), jdtls) {
                    (Some((command, arguments)), Some(jdtls)) => {
                        let asker = self
                            .languages
                            .state
                            .running
                            .borrow()
                            .get(running.spec.id)
                            .cloned();
                        let request = jdtls.server.request_raw(
                            "workspace/executeCommand",
                            json!({ "command": command, "arguments": arguments }),
                        );
                        cx.spawn(async move |_, _| {
                            let answer = request.await.map_err(|error| {
                                ResponseError::new(
                                    ResponseError::INTERNAL_ERROR,
                                    format!("{error:#}"),
                                )
                            });
                            if let Some(asker) = asker {
                                asker.server.respond(id, answer);
                            }
                        })
                        .detach();
                        return;
                    }
                    // Moving the cursor and the like: nothing to do.
                    (None, _) => Ok(Value::Null),
                    (Some(_), None) => Err(ResponseError::new(
                        ResponseError::INTERNAL_ERROR,
                        "the Java server is not running",
                    )),
                }
            }
            // A command the editor runs, as VS Code's Java extension does
            // for jdtls; one Keel does not know goes to the server that
            // registered it, as Spring Boot's classpath callbacks do.
            "workspace/executeClientCommand" => {
                let command = params["command"].as_str().unwrap_or_default().to_owned();
                let arguments = params["arguments"].as_array().cloned().unwrap_or_default();
                if self.client_command(&command, &arguments, window, cx) {
                    Ok(Value::Null)
                } else if let Some(target) = {
                    let servers = self.languages.state.running.borrow();
                    servers
                        .values()
                        .find(|other| {
                            other.spec.id != running.spec.id && other.runs_command(&command)
                        })
                        // Spring Boot's classpath callbacks (`sts4.classpath.…`).
                        .or_else(|| {
                            command
                                .starts_with("sts")
                                .then(|| servers.get("spring-boot"))
                                .flatten()
                        })
                        .cloned()
                } {
                    let asker = self
                        .languages
                        .state
                        .running
                        .borrow()
                        .get(running.spec.id)
                        .cloned();
                    let request = target.server.request_raw(
                        "workspace/executeCommand",
                        json!({ "command": command, "arguments": arguments }),
                    );
                    cx.spawn(async move |_, _| {
                        let answer = request.await.map_err(|error| {
                            ResponseError::new(ResponseError::INTERNAL_ERROR, format!("{error:#}"))
                        });
                        if let Some(asker) = asker {
                            asker.server.respond(id, answer);
                        }
                    })
                    .detach();
                    return;
                } else {
                    Err(ResponseError::new(
                        ResponseError::METHOD_NOT_FOUND,
                        format!("Keel does not know the command {command}"),
                    ))
                }
            }
            "window/showMessageRequest" => {
                let message = params["message"].as_str().unwrap_or_default();
                self.log_to(
                    OutputChannel::Languages,
                    format!("[{}] {message}", running.spec.name),
                );
                Ok(Value::Null)
            }
            _ => Err(ResponseError::new(
                ResponseError::METHOD_NOT_FOUND,
                format!("Keel does not handle {method}"),
            )),
        };
        running.server.respond(id, answer);
    }

    fn server_exited(
        &mut self,
        spec: &'static ServerSpec,
        code: Option<i32>,
        cx: &mut Context<Self>,
    ) {
        self.languages.servers.remove(spec.id);
        self.languages.state.running.borrow_mut().remove(spec.id);
        self.languages.progress.retain(|(id, _), _| *id != spec.id);
        // Its diagnostics are gone with it.
        let uris: Vec<String> = self
            .languages
            .diagnostics
            .iter_mut()
            .filter_map(|(uri, by_server)| by_server.remove(spec.id).map(|_| uri.clone()))
            .collect();
        self.languages
            .diagnostics
            .retain(|_, by_server| !by_server.is_empty());
        for uri in &uris {
            self.diagnostics_changed(uri, cx);
        }
        let code = code.map_or_else(|| "?".to_owned(), |code| code.to_string());
        let now = Instant::now();
        let restarts = self.languages.restarts.entry(spec.id).or_default();
        restarts.retain(|at| now.duration_since(*at) < RESTART_WINDOW);
        restarts.push(now);
        let too_many = restarts.len() > MAX_RESTARTS;
        let strings = self.strings();
        if too_many {
            self.languages.servers.insert(spec.id, Slot::Stopped);
            let message = strings.lsp_stopped(spec.name);
            self.log_to(
                OutputChannel::Languages,
                format!("[{}] {message} ({code})", spec.name),
            );
            self.status_message = Some(message);
        } else {
            self.log_to(
                OutputChannel::Languages,
                format!("[{}] {} ({code})", spec.name, strings.lsp_restarting),
            );
            self.restart_for_open_documents(spec, cx);
        }
        cx.notify();
    }

    /// Starts `spec` again if a document still needs it.
    fn restart_for_open_documents(&mut self, spec: &'static ServerSpec, cx: &mut Context<Self>) {
        let uri = self
            .languages
            .state
            .documents
            .borrow()
            .iter()
            .find(|(_, document)| spec.serves(&document.language_id))
            .map(|(uri, _)| uri.clone());
        if let Some(root) = uri.and_then(|uri| self.server_root(&uri)) {
            self.start_server(spec, root, cx);
        }
    }

    /// Tells the servers that asked about files changed on disk: created
    /// when Quick Open did not list them yet, deleted when they are gone.
    pub(super) fn watched_files_changed(&self, paths: &[PathBuf]) {
        let running: Vec<Rc<RunningServer>> = self
            .languages
            .state
            .running
            .borrow()
            .values()
            .cloned()
            .collect();
        if running
            .iter()
            .all(|server| server.watchers.borrow().is_empty())
        {
            return;
        }
        let known = |path: &Path| {
            let (Some(root), Some(index)) = (&self.project.root, &self.file_index) else {
                return true;
            };
            path.strip_prefix(root)
                .ok()
                .and_then(|relative| relative.to_str())
                .is_some_and(|relative| index.iter().any(|file| file == relative))
        };
        let changes: Vec<(PathBuf, u64)> = paths
            .iter()
            .filter(|path| !path.components().any(|part| part.as_os_str() == ".git"))
            .map(|path| {
                let kind = if !path.exists() {
                    4
                } else if path.is_file() && !known(path) {
                    1
                } else {
                    2
                };
                (path.clone(), kind)
            })
            .collect();
        for server in running {
            let watchers = server.watchers.borrow();
            let wanted: Vec<Value> = changes
                .iter()
                .filter(|(path, kind)| watchers.iter().any(|watcher| watcher.matches(path, *kind)))
                .map(|(path, kind)| {
                    // LSP numbers the kinds 1, 2, 3.
                    let kind = match kind {
                        1 => 1,
                        2 => 2,
                        _ => 3,
                    };
                    json!({ "uri": Uri::file(path).to_string(), "type": kind })
                })
                .collect();
            if !wanted.is_empty() {
                server.server.notify_raw(
                    "workspace/didChangeWatchedFiles",
                    json!({ "changes": wanted }),
                );
            }
        }
    }

    /// Replaces what server `spec` says about `uri`.
    fn set_diagnostics(
        &mut self,
        spec: &'static ServerSpec,
        uri: &str,
        diagnostics: Vec<Diagnostic>,
        cx: &mut Context<Self>,
    ) {
        let entry = self
            .languages
            .diagnostics
            .entry(uri.to_owned())
            .or_default();
        if diagnostics.is_empty() {
            entry.remove(spec.id);
        } else {
            entry.insert(spec.id, diagnostics);
        }
        if entry.is_empty() {
            self.languages.diagnostics.remove(uri);
        }
        self.diagnostics_changed(uri, cx);
    }

    /// Asks a pull server for the diagnostics of `uri`, a moment after the
    /// last change, as VS Code does.
    fn pull_diagnostics(&mut self, id: &'static str, uri: String, cx: &mut Context<Self>) {
        let Some(running) = self.languages.state.running.borrow().get(id).cloned() else {
            return;
        };
        let Some(spec) = servers::spec(id) else {
            return;
        };
        let key = (id, uri.clone());
        let previous = self.languages.pull_results.get(&key).cloned();
        let identifier = running
            .server
            .capabilities()
            .and_then(|capabilities| capabilities.diagnostic_provider.as_ref())
            .and_then(|provider| match provider {
                lsp_types::DiagnosticServerCapabilities::Options(options) => {
                    options.identifier.clone()
                }
                lsp_types::DiagnosticServerCapabilities::RegistrationOptions(options) => {
                    options.diagnostic_options.identifier.clone()
                }
            });
        let task = cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(200))
                .await;
            let request = running.server.request_raw(
                "textDocument/diagnostic",
                json!({
                    "textDocument": { "uri": uri },
                    "identifier": identifier,
                    "previousResultId": previous,
                }),
            );
            let Ok(report) = request.await else { return };
            this.update(cx, |this, cx| {
                let key = (id, uri.clone());
                if let Some(result) = report["resultId"].as_str() {
                    this.languages
                        .pull_results
                        .insert(key.clone(), result.to_owned());
                }
                this.languages.pulls.remove(&key);
                if report["kind"] == "full" {
                    let items = serde_json::from_value::<Vec<Diagnostic>>(report["items"].clone())
                        .unwrap_or_default();
                    this.set_diagnostics(spec, &normalize_uri(&uri), items, cx);
                }
            })
            .ok();
        });
        self.languages.pulls.insert(key, task);
    }

    /// The user's settings changed: every server reads them again.
    pub(super) fn language_settings_changed(&self) {
        let running: Vec<Rc<RunningServer>> = self
            .languages
            .state
            .running
            .borrow()
            .values()
            .cloned()
            .collect();
        for server in running {
            let settings = self.server_settings(server.spec);
            server.server.set_settings(settings.clone());
            server.server.notify_raw(
                "workspace/didChangeConfiguration",
                json!({ "settings": settings }),
            );
        }
    }

    /// "Restart Language Servers": stops every server and starts again the
    /// ones the open files need.
    pub(super) fn restart_language_servers(&mut self, cx: &mut Context<Self>) {
        let stopped: Vec<&'static str> = self.languages.servers.keys().copied().collect();
        for id in stopped {
            if let Some(Slot::Running(running)) = self.languages.servers.remove(id) {
                // Asked to stop politely; the process is stopped anyway when
                // the server goes, after two seconds at most.
                cx.spawn(async move |_, cx| {
                    let shutdown = running.server.shutdown();
                    let timeout = cx.background_executor().timer(Duration::from_secs(2));
                    futures::pin_mut!(shutdown);
                    let _ = select(shutdown, timeout).await;
                })
                .detach();
            }
        }
        self.languages.servers.clear();
        self.languages.state.running.borrow_mut().clear();
        self.languages.restarts.clear();
        self.languages.progress.clear();
        let uris: Vec<String> = self.languages.diagnostics.keys().cloned().collect();
        self.languages.diagnostics.clear();
        for uri in &uris {
            self.diagnostics_changed(uri, cx);
        }
        let documents: Vec<(String, String)> = self
            .languages
            .state
            .documents
            .borrow()
            .iter()
            .map(|(uri, document)| (uri.clone(), document.language_id.clone()))
            .collect();
        for (uri, language_id) in documents {
            let Some(root) = self.server_root(&uri) else {
                continue;
            };
            for spec in servers::servers_for(&language_id, &root) {
                if !self.server_disabled(spec) && !self.languages.servers.contains_key(spec.id) {
                    self.start_server(spec, root.clone(), cx);
                }
            }
        }
        self.log_to(
            OutputChannel::Languages,
            self.strings().lsp_restarted.to_owned(),
        );
        cx.notify();
    }

    /// The settings a server reads with `workspace/configuration`.
    fn server_settings(&self, spec: &ServerSpec) -> Value {
        let root = self.project.root.clone().unwrap_or_default();
        servers::settings_for(spec, &lsp::nested_settings(&self.settings.merged()), &root)
    }

    pub(super) fn language_server_actions(
        &self,
        root: gpui::Div,
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        root.on_action(
            cx.listener(|this, _: &RestartLanguageServers, _, cx| {
                this.restart_language_servers(cx)
            }),
        )
    }
}

/// Servers and Keel may spell the same file differently, for example
/// `%3A` against `:`. Diagnostics are kept under Keel's spelling.
pub(super) fn normalize_uri(uri: &str) -> String {
    Uri::parse(uri)
        .and_then(|parsed| parsed.file_path())
        .map(|path| Uri::file(&path).to_string())
        .unwrap_or_else(|| uri.to_owned())
}

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use gpui::{TestAppContext, VisualTestContext};

    use super::*;

    fn window(
        cx: &mut TestAppContext,
    ) -> (
        tempfile::TempDir,
        PathBuf,
        Entity<Workbench>,
        &mut VisualTestContext,
    ) {
        cx.executor().allow_parking();
        cx.update(|cx| {
            gpui_component::init(cx);
            language::register_grammars();
            crate::workbench_window::init(cx);
        });
        let folder = tempfile::tempdir().unwrap();
        let base = std::fs::canonicalize(folder.path()).unwrap();
        std::fs::write(base.join("app.ts"), "const a = 1;\n").unwrap();
        let settings = Settings::load(base.join("settings.json"), None);
        let opening = Opening::new(Some(base.clone()), None);
        let (view, cx) = cx.add_window_view(|window, cx| {
            let mut workbench = Workbench::new(opening, settings, None, window, cx);
            workbench.languages.fake = true;
            workbench
        });
        (folder, base, view, cx)
    }

    fn wait_until(
        cx: &mut VisualTestContext,
        what: &str,
        check: impl Fn(&mut VisualTestContext) -> bool,
    ) {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            cx.run_until_parked();
            if check(cx) {
                return;
            }
            assert!(Instant::now() < deadline, "gave up waiting for {what}");
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    fn todo_count(view: &Entity<Workbench>, uri: &str, cx: &mut VisualTestContext) -> usize {
        view.read_with(cx, |view, _| {
            view.languages
                .diagnostics
                .get(uri)
                .and_then(|by_server| by_server.get("vtsls"))
                .map_or(0, Vec::len)
        })
    }

    #[gpui::test]
    fn opening_and_typing_keep_the_server_in_step(cx: &mut TestAppContext) {
        let (_folder, base, view, cx) = window(cx);
        let path = base.join("app.ts");
        let uri = Uri::file(&path).to_string();
        view.update_in(cx, |view, window, cx| view.open_file(&path, window, cx));
        wait_until(cx, "the server to start", |cx| {
            view.read_with(cx, |view, _| view.languages.is_running("vtsls"))
        });
        assert_eq!(todo_count(&view, &uri, cx), 0);

        // Typing reaches the server as small changes, and it reports back.
        view.update_in(cx, |view, window, cx| {
            let editor = view.active_editor().unwrap();
            editor.update(cx, |editor, cx| {
                editor.set_value("const a = 1; // TODO\n// TODO again\n", window, cx)
            });
            // As if typed: the editor tells the window.
            view.sync_document_from(&uri, None, window, cx);
        });
        wait_until(cx, "two diagnostics", |cx| todo_count(&view, &uri, cx) == 2);

        // Closing the file clears what the server said about it.
        view.update_in(cx, |view, window, cx| {
            view.close_pane(0, window, cx);
        });
        wait_until(cx, "the diagnostics to go", |cx| {
            todo_count(&view, &uri, cx) == 0
        });
    }

    #[gpui::test]
    fn a_crashed_server_starts_again(cx: &mut TestAppContext) {
        let (_folder, base, view, cx) = window(cx);
        let path = base.join("app.ts");
        view.update_in(cx, |view, window, cx| view.open_file(&path, window, cx));
        wait_until(cx, "the server to start", |cx| {
            view.read_with(cx, |view, _| view.languages.is_running("vtsls"))
        });
        let first = view.read_with(cx, |view, _| view.languages.running_pid("vtsls").unwrap().0);
        view.read_with(cx, |view, _| {
            for running in view.languages.servers_for(&Uri::file(&path).to_string()) {
                running.server.notify_raw("fake/crash", Value::Null);
            }
        });
        wait_until(cx, "a new server", |cx| {
            view.read_with(cx, |view, _| {
                view.languages
                    .running_pid("vtsls")
                    .is_some_and(|(generation, _)| generation != first)
            })
        });
        let logged = view.read_with(cx, |view, _| {
            view.output.iter().any(|(channel, line)| {
                *channel == OutputChannel::Languages && line.contains("TypeScript")
            })
        });
        assert!(logged);
    }

    #[gpui::test]
    fn a_missing_prerequisite_is_offered_once_in_a_terminal(cx: &mut TestAppContext) {
        let (_folder, _base, view, cx) = window(cx);
        let missing = |view: &Entity<Workbench>, cx: &mut VisualTestContext| {
            view.update_in(cx, |view, window, cx| {
                view.lsp_input(
                    LspInput::Missing {
                        server: "Java",
                        prerequisite: Prerequisite::Java { min: 21 },
                    },
                    window,
                    cx,
                )
            });
        };
        missing(&view, cx);
        assert!(cx.has_pending_prompt());
        cx.simulate_prompt_answer("Installa nel terminale");
        cx.run_until_parked();
        let names: Vec<Option<String>> = view.read_with(cx, |view, _| {
            view.terminals.iter().map(|tab| tab.name.clone()).collect()
        });
        assert_eq!(names, [Some("Installa Java 21".to_owned())]);
        // Once per window: a second server needing Java asks nothing.
        missing(&view, cx);
        assert!(!cx.has_pending_prompt());
    }

    #[gpui::test]
    fn pulled_diagnostics_are_asked_again_when_a_server_ends_its_work(cx: &mut TestAppContext) {
        let (_folder, base, view, cx) = window(cx);
        let path = base.join("app.ts");
        let uri = Uri::file(&path).to_string();
        view.update_in(cx, |view, window, cx| view.open_file(&path, window, cx));
        wait_until(cx, "the server to start", |cx| {
            view.read_with(cx, |view, _| view.languages.is_running("vtsls"))
        });
        let pulled = |view: &Entity<Workbench>, cx: &mut VisualTestContext, message: &str| {
            view.read_with(cx, |view, _| {
                view.languages
                    .diagnostics
                    .get(&uri)
                    .and_then(|by_server| by_server.get("vtsls"))
                    .is_some_and(|found| {
                        found.iter().any(|diagnostic| diagnostic.message == message)
                    })
            })
        };
        let notify = |view: &Entity<Workbench>, cx: &mut VisualTestContext, method: &str| {
            view.read_with(cx, |view, _| {
                view.languages.servers_for(&uri)[0]
                    .server
                    .notify_raw(method, Value::Null)
            })
        };
        // csharp-ls registers its diagnostics while it loads the project.
        notify(&view, cx, "fake/registerPull");
        // Pulls wait a moment after the last change.
        let advance = |cx: &mut VisualTestContext| {
            cx.executor().advance_clock(Duration::from_millis(300));
        };
        wait_until(cx, "the first pull", |cx| {
            advance(cx);
            pulled(&view, cx, "pulled 1")
        });
        // Loading done, it says nothing more: Keel asks again.
        notify(&view, cx, "fake/progressEnd");
        wait_until(cx, "the second pull", |cx| {
            advance(cx);
            pulled(&view, cx, "pulled 2")
        });
    }

    #[gpui::test]
    fn makefiles_indent_with_tabs(cx: &mut TestAppContext) {
        let (_folder, _base, view, cx) = window(cx);
        view.read_with(cx, |view, _| {
            assert!(view.tab_size_for("makefile").hard_tabs);
            assert!(!view.tab_size_for("python").hard_tabs);
            assert_eq!(view.tab_size_for("python").tab_size, 2);
        });
    }

    #[gpui::test]
    fn restarting_starts_fresh_servers(cx: &mut TestAppContext) {
        let (_folder, base, view, cx) = window(cx);
        let path = base.join("app.ts");
        view.update_in(cx, |view, window, cx| view.open_file(&path, window, cx));
        wait_until(cx, "the server to start", |cx| {
            view.read_with(cx, |view, _| view.languages.is_running("vtsls"))
        });
        let first = view.read_with(cx, |view, _| view.languages.running_pid("vtsls").unwrap().0);
        view.update(cx, |view, cx| view.restart_language_servers(cx));
        wait_until(cx, "the server to start again", |cx| {
            view.read_with(cx, |view, _| {
                view.languages
                    .running_pid("vtsls")
                    .is_some_and(|(generation, _)| generation != first)
            })
        });
    }

    #[test]
    fn file_watchers_match_their_patterns_and_kinds() {
        let registration = json!({
            "id": "watch-1",
            "method": "workspace/didChangeWatchedFiles",
            "registerOptions": { "watchers": [
                { "globPattern": "**/*.{ts,tsx}" },
                { "globPattern": { "baseUri": "file:///progetto", "pattern": "prisma/*.prisma" }, "kind": 4 },
            ] },
        });
        let watchers = file_watchers(&registration);
        assert_eq!(watchers.len(), 2);
        assert!(watchers[0].matches(Path::new("/progetto/src/app/page.tsx"), 2));
        assert!(!watchers[0].matches(Path::new("/progetto/README.md"), 2));
        // The second one only wants deletions, under its folder.
        assert!(watchers[1].matches(Path::new("/progetto/prisma/schema.prisma"), 4));
        assert!(!watchers[1].matches(Path::new("/progetto/prisma/schema.prisma"), 2));
        assert!(!watchers[1].matches(Path::new("/altro/prisma/schema.prisma"), 4));
    }

    #[test]
    fn uris_are_compared_by_path() {
        let keel = Uri::file(Path::new("/Users/m/progetto con spazi/app.ts")).to_string();
        assert_eq!(
            normalize_uri("file:///Users/m/progetto%20con%20spazi/app.ts"),
            keel
        );
        assert_eq!(normalize_uri("untitled:Untitled-1"), "untitled:Untitled-1");
    }
}
