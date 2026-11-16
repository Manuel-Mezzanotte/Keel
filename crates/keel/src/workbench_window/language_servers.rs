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
    Server {
        id: &'static str,
        generation: u64,
        event: ServerEvent,
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
    restarts: HashMap<&'static str, Vec<Instant>>,
    next_generation: u64,
    /// Tests run the fake server in a thread when this is set, and no
    /// server at all otherwise.
    #[cfg(test)]
    pub(super) fake: bool,
    _driver: Task<()>,
}

impl LanguageState {
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
    pub(super) fn new(cx: &mut Context<Workbench>) -> Self {
        let (sender, mut receiver) = unbounded::<LspInput>();
        let driver = cx.spawn(async move |this, cx| {
            while let Some(input) = receiver.next().await {
                if this
                    .update(cx, |this, cx| this.lsp_input(input, cx))
                    .is_err()
                {
                    break;
                }
            }
        });
        Self {
            sender,
            servers: HashMap::new(),
            state: Rc::default(),
            diagnostics: BTreeMap::new(),
            progress: BTreeMap::new(),
            restarts: HashMap::new(),
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
    pub(super) fn lsp_input(&mut self, input: LspInput, cx: &mut Context<Self>) {
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
            LspInput::Server {
                id,
                generation,
                event,
            } => self.server_event(id, generation, event, cx),
        }
    }

    /// The folder a server for `uri` works in: the project, or the file's
    /// own folder in a window without one.
    fn server_root(&self, uri: &str) -> Option<PathBuf> {
        if let Some(root) = &self.project.root {
            return Some(root.clone());
        }
        let path = Uri::parse(uri)?.file_path()?;
        path.parent().map(Path::to_path_buf)
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
        let options = initialization_options(spec, &self.settings);
        let task = cx.spawn(async move |this, cx| {
            let started: anyhow::Result<LanguageServer> = if fake {
                lsp::fake::spawn()
                    .map_err(anyhow::Error::from)
                    .and_then(|(reader, writer)| {
                        LanguageServer::connect(spec.name, reader, writer, Arc::new(on_event), None)
                    })
            } else {
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
                self.server_request(&running, id, &method, params, cx);
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
                let entry = self.languages.diagnostics.entry(uri.clone()).or_default();
                if params.diagnostics.is_empty() {
                    entry.remove(spec.id);
                } else {
                    entry.insert(spec.id, params.diagnostics);
                }
                if entry.is_empty() {
                    self.languages.diagnostics.remove(&uri);
                }
                self.diagnostics_changed(&uri, cx);
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
        _cx: &mut Context<Self>,
    ) {
        let answer = match method {
            // Edits from the server arrive with the rename and quick fix
            // work of later sub-phases.
            "workspace/applyEdit" => Ok(json!({ "applied": false })),
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
        servers::settings_for(spec, &lsp::nested_settings(&self.settings.merged()))
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

/// What a server gets in `initializationOptions`.
fn initialization_options(spec: &ServerSpec, _settings: &Settings) -> Option<Value> {
    let _ = spec;
    None
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
    fn uris_are_compared_by_path() {
        let keel = Uri::file(Path::new("/Users/m/progetto con spazi/app.ts")).to_string();
        assert_eq!(
            normalize_uri("file:///Users/m/progetto%20con%20spazi/app.ts"),
            keel
        );
        assert_eq!(normalize_uri("untitled:Untitled-1"), "untitled:Untitled-1");
    }
}
