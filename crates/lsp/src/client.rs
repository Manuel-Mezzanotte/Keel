//! A running language server: its process, the messages going both ways,
//! and the requests waiting for an answer.

use std::collections::HashMap;
use std::fmt;
use std::future::Future;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::PathBuf;
use std::pin::Pin;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::{Arc, Mutex, OnceLock, Weak, mpsc};
use std::task::{Context, Poll};
use std::thread;

use anyhow::{Context as _, Result, anyhow};
use futures::channel::oneshot;
use lsp_types::{InitializeResult, ServerCapabilities};
use serde_json::{Value, json};

use crate::rpc::{read_message, write_message};

/// How to start a server.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ServerCommand {
    pub program: PathBuf,
    pub args: Vec<String>,
    /// Added to Keel's own environment, replacing variables with the same name.
    pub env: Vec<(String, String)>,
    pub cwd: PathBuf,
}

/// Something the server said on its own.
#[derive(Clone, Debug, PartialEq)]
pub enum ServerEvent {
    /// Diagnostics, log and show messages, progress... Requests Keel answers
    /// by itself, such as `client/registerCapability`, come here too, so the
    /// editor can follow them.
    Notification { method: String, params: Value },
    /// A request the editor must answer with [`LanguageServer::respond`],
    /// such as `workspace/applyEdit`.
    Request {
        id: Value,
        method: String,
        params: Value,
    },
    /// A line the server wrote on its standard error.
    Stderr(String),
    /// The server has stopped, with its exit code when it had one.
    Exited(Option<i32>),
}

/// An error a server answered a request with.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResponseError {
    pub code: i64,
    pub message: String,
}

impl ResponseError {
    pub const METHOD_NOT_FOUND: i64 = -32601;
    pub const REQUEST_CANCELLED: i64 = -32800;
    pub const CONTENT_MODIFIED: i64 = -32801;

    pub fn new(code: i64, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    /// The request was cancelled or outdated: nothing to report.
    pub fn is_cancellation(&self) -> bool {
        matches!(self.code, Self::REQUEST_CANCELLED | Self::CONTENT_MODIFIED)
    }
}

impl fmt::Display for ResponseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} ({})", self.message, self.code)
    }
}

impl std::error::Error for ResponseError {}

type Pending = HashMap<i64, oneshot::Sender<std::result::Result<Value, ResponseError>>>;

/// What the client's threads share.
struct Shared {
    pending: Mutex<Pending>,
    outgoing: Mutex<Option<mpsc::Sender<Value>>>,
    /// The settings `workspace/configuration` reads, nested by section.
    settings: Mutex<Value>,
    folders: Mutex<Value>,
    closed: AtomicBool,
}

impl Shared {
    fn send(&self, message: Value) -> bool {
        self.outgoing
            .lock()
            .unwrap()
            .as_ref()
            .is_some_and(|outgoing| outgoing.send(message).is_ok())
    }

    /// Ends every request still waiting: their futures fail.
    fn close(&self) {
        self.closed.store(true, Ordering::SeqCst);
        self.outgoing.lock().unwrap().take();
        self.pending.lock().unwrap().clear();
    }
}

/// A language server Keel talks to.
pub struct LanguageServer {
    name: String,
    shared: Arc<Shared>,
    next_id: AtomicI64,
    process: Option<Arc<Mutex<Child>>>,
    pid: Option<u32>,
    capabilities: OnceLock<ServerCapabilities>,
}

impl LanguageServer {
    /// Starts the server's process. `on_event` is called from the client's
    /// threads.
    pub fn start(
        name: impl Into<String>,
        command: &ServerCommand,
        on_event: impl Fn(ServerEvent) + Send + Sync + 'static,
    ) -> Result<Self> {
        use std::os::unix::process::CommandExt as _;

        let name = name.into();
        let mut child = Command::new(&command.program)
            .args(&command.args)
            .envs(command.env.iter().map(|(key, value)| (key, value)))
            .current_dir(&command.cwd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            // Its own process group, so stopping it also stops the programs
            // it starts, such as `tsserver`.
            .process_group(0)
            .spawn()
            .with_context(|| format!("starting {}", command.program.display()))?;
        let stdin = child.stdin.take().context("no stdin")?;
        let stdout = child.stdout.take().context("no stdout")?;
        let stderr = child.stderr.take().context("no stderr")?;
        let pid = child.id();
        let process = Arc::new(Mutex::new(child));
        let on_event: Arc<dyn Fn(ServerEvent) + Send + Sync> = Arc::new(on_event);

        let events = on_event.clone();
        thread::Builder::new()
            .name(format!("{name} stderr"))
            .spawn(move || {
                for line in BufReader::new(stderr).lines() {
                    let Ok(line) = line else { break };
                    events(ServerEvent::Stderr(line));
                }
            })?;

        let mut server = Self::connect(name, stdout, stdin, on_event, Some(process.clone()))?;
        server.pid = Some(pid);
        server.process = Some(process);
        Ok(server)
    }

    /// Talks to a server over any pair of streams, such as the fake server
    /// the tests run in a thread. With `process`, the reading thread waits
    /// for it to exit before reporting [`ServerEvent::Exited`].
    pub fn connect(
        name: impl Into<String>,
        reader: impl Read + Send + 'static,
        writer: impl Write + Send + 'static,
        on_event: Arc<dyn Fn(ServerEvent) + Send + Sync>,
        process: Option<Arc<Mutex<Child>>>,
    ) -> Result<Self> {
        let name = name.into();
        let (outgoing, queue) = mpsc::channel::<Value>();
        let shared = Arc::new(Shared {
            pending: Mutex::new(HashMap::new()),
            outgoing: Mutex::new(Some(outgoing)),
            settings: Mutex::new(Value::Null),
            folders: Mutex::new(Value::Null),
            closed: AtomicBool::new(false),
        });

        thread::Builder::new()
            .name(format!("{name} writer"))
            .spawn(move || {
                let mut writer = writer;
                while let Ok(message) = queue.recv() {
                    if write_message(&mut writer, &message).is_err() {
                        break;
                    }
                }
            })?;

        let reading = shared.clone();
        thread::Builder::new()
            .name(format!("{name} reader"))
            .spawn(move || {
                let mut reader = BufReader::new(reader);
                while let Ok(Some(message)) = read_message(&mut reader) {
                    dispatch(&reading, message, &*on_event);
                }
                reading.close();
                let code = process.and_then(|process| {
                    let status = process.lock().unwrap().wait().ok()?;
                    status.code()
                });
                on_event(ServerEvent::Exited(code));
            })?;

        Ok(Self {
            name,
            shared,
            next_id: AtomicI64::new(1),
            process: None,
            pid: None,
            capabilities: OnceLock::new(),
        })
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    /// The server's process id, for the tests and the Output view.
    pub fn pid(&self) -> Option<u32> {
        self.pid
    }

    pub fn is_running(&self) -> bool {
        !self.shared.closed.load(Ordering::SeqCst)
    }

    /// What the server can do, once it has been initialized.
    pub fn capabilities(&self) -> Option<&ServerCapabilities> {
        self.capabilities.get()
    }

    /// The settings `workspace/configuration` answers from, as one nested
    /// object (see [`crate::nested_settings`]).
    pub fn set_settings(&self, settings: Value) {
        *self.shared.settings.lock().unwrap() = settings;
    }

    /// Sends a request; the answer comes from the returned future. Dropping
    /// the future before it ends cancels the request.
    pub fn request_raw(&self, method: &str, params: Value) -> Response {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let (sender, receiver) = oneshot::channel();
        self.shared.pending.lock().unwrap().insert(id, sender);
        let sent = self.shared.send(json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        }));
        if !sent {
            self.shared.pending.lock().unwrap().remove(&id);
        }
        Response {
            id,
            receiver,
            shared: Arc::downgrade(&self.shared),
            done: false,
        }
    }

    /// A request with the types of `lsp_types`.
    pub fn request<R: lsp_types::request::Request>(
        &self,
        params: R::Params,
    ) -> impl Future<Output = Result<R::Result>> + use<R> {
        let params = serde_json::to_value(params);
        let response = params.map(|params| self.request_raw(R::METHOD, params));
        async move {
            let value = response?.await?;
            serde_json::from_value(value)
                .with_context(|| format!("reading the answer to {}", R::METHOD))
        }
    }

    pub fn notify_raw(&self, method: &str, params: Value) {
        self.shared.send(json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
        }));
    }

    pub fn notify<N: lsp_types::notification::Notification>(&self, params: N::Params) {
        if let Ok(params) = serde_json::to_value(params) {
            self.notify_raw(N::METHOD, params);
        }
    }

    /// Answers a request the server sent ([`ServerEvent::Request`]).
    pub fn respond(&self, id: Value, result: std::result::Result<Value, ResponseError>) {
        let message = match result {
            Ok(result) => json!({ "jsonrpc": "2.0", "id": id, "result": result }),
            Err(error) => json!({
                "jsonrpc": "2.0",
                "id": id,
                "error": { "code": error.code, "message": error.message },
            }),
        };
        self.shared.send(message);
    }

    /// The `initialize` handshake, then `initialized`.
    pub async fn initialize(
        &self,
        root: &std::path::Path,
        initialization_options: Option<Value>,
    ) -> Result<InitializeResult> {
        let root_uri = workbench::uri::Uri::file(root).to_string();
        let folder_name = root
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        let folders = json!([{ "uri": root_uri, "name": folder_name }]);
        *self.shared.folders.lock().unwrap() = folders.clone();
        let mut params = json!({
            "processId": std::process::id(),
            "clientInfo": { "name": "Keel", "version": env!("CARGO_PKG_VERSION") },
            "locale": "en",
            "rootPath": root,
            "rootUri": root_uri,
            "workspaceFolders": folders,
            "capabilities": client_capabilities(),
        });
        if let Some(options) = initialization_options {
            params["initializationOptions"] = options;
        }
        let value = self.request_raw("initialize", params).await?;
        let result: InitializeResult =
            serde_json::from_value(value).context("reading the server's capabilities")?;
        let _ = self.capabilities.set(result.capabilities.clone());
        self.notify_raw("initialized", json!({}));
        Ok(result)
    }

    /// Asks the server to stop: `shutdown`, then `exit`. The process is
    /// stopped anyway when the server is dropped.
    pub async fn shutdown(&self) {
        let _ = self.request_raw("shutdown", Value::Null).await;
        self.notify_raw("exit", Value::Null);
    }

    fn kill(&self) {
        self.shared.close();
        if let Some(pid) = self.pid {
            // SAFETY: a plain signal to the server's own process group.
            unsafe {
                libc::killpg(pid as libc::pid_t, libc::SIGTERM);
            }
        }
        if let Some(process) = &self.process {
            let _ = process.lock().unwrap().kill();
        }
    }
}

impl Drop for LanguageServer {
    fn drop(&mut self) {
        self.kill();
    }
}

/// The answer to a request, still to come.
pub struct Response {
    id: i64,
    receiver: oneshot::Receiver<std::result::Result<Value, ResponseError>>,
    shared: Weak<Shared>,
    done: bool,
}

impl Response {
    pub fn id(&self) -> i64 {
        self.id
    }
}

impl Future for Response {
    type Output = Result<Value>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        match Pin::new(&mut self.receiver).poll(cx) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(result) => {
                self.done = true;
                Poll::Ready(match result {
                    Ok(Ok(value)) => Ok(value),
                    Ok(Err(error)) => Err(error.into()),
                    Err(_) => Err(anyhow!("the language server has stopped")),
                })
            }
        }
    }
}

impl Drop for Response {
    fn drop(&mut self) {
        if self.done {
            return;
        }
        let Some(shared) = self.shared.upgrade() else {
            return;
        };
        if shared.pending.lock().unwrap().remove(&self.id).is_some() {
            shared.send(json!({
                "jsonrpc": "2.0",
                "method": "$/cancelRequest",
                "params": { "id": self.id },
            }));
        }
    }
}

/// Requests Keel answers without asking the editor.
fn dispatch(shared: &Shared, message: Value, on_event: &(dyn Fn(ServerEvent) + Send + Sync)) {
    let method = message["method"].as_str().map(str::to_owned);
    let id = message.get("id").cloned();
    match (method, id) {
        // A response to one of Keel's requests.
        (None, Some(id)) => {
            let Some(id) = id.as_i64() else { return };
            let Some(sender) = shared.pending.lock().unwrap().remove(&id) else {
                return;
            };
            let result = match message.get("error") {
                Some(error) => Err(ResponseError::new(
                    error["code"].as_i64().unwrap_or_default(),
                    error["message"].as_str().unwrap_or_default(),
                )),
                None => Ok(message.get("result").cloned().unwrap_or(Value::Null)),
            };
            let _ = sender.send(result);
        }
        // A request from the server.
        (Some(method), Some(id)) => {
            let params = message.get("params").cloned().unwrap_or(Value::Null);
            let answer = match method.as_str() {
                "workspace/configuration" => {
                    let settings = shared.settings.lock().unwrap();
                    let items = params["items"].as_array().cloned().unwrap_or_default();
                    Some(Value::Array(
                        items
                            .iter()
                            .map(|item| section_of(&settings, item["section"].as_str()))
                            .collect(),
                    ))
                }
                "workspace/workspaceFolders" => Some(shared.folders.lock().unwrap().clone()),
                "window/workDoneProgress/create"
                | "client/registerCapability"
                | "client/unregisterCapability"
                | "workspace/semanticTokens/refresh"
                | "workspace/inlayHint/refresh"
                | "workspace/codeLens/refresh"
                | "workspace/diagnostic/refresh"
                | "workspace/inlineValue/refresh"
                | "workspace/foldingRange/refresh" => Some(Value::Null),
                _ => None,
            };
            match answer {
                Some(result) => {
                    shared.send(json!({ "jsonrpc": "2.0", "id": id, "result": result }));
                    on_event(ServerEvent::Notification { method, params });
                }
                None => on_event(ServerEvent::Request { id, method, params }),
            }
        }
        (Some(method), None) => {
            let params = message.get("params").cloned().unwrap_or(Value::Null);
            on_event(ServerEvent::Notification { method, params });
        }
        (None, None) => {}
    }
}

/// The part of the settings a `workspace/configuration` item asks for:
/// `typescript` gives the `typescript.*` settings as an object.
pub fn section_of(settings: &Value, section: Option<&str>) -> Value {
    let Some(section) = section.filter(|section| !section.is_empty()) else {
        return settings.clone();
    };
    let mut value = settings;
    for part in section.split('.') {
        match value.get(part) {
            Some(inner) => value = inner,
            None => return Value::Null,
        }
    }
    value.clone()
}

/// What Keel can do, told to every server.
fn client_capabilities() -> Value {
    let symbol_kinds: Vec<u32> = (1..=26).collect();
    let completion_kinds: Vec<u32> = (1..=25).collect();
    json!({
        "general": {
            "positionEncodings": ["utf-16"],
            "markdown": { "parser": "marked", "version": "1.1.0" },
        },
        "window": {
            "workDoneProgress": true,
            "showMessage": { "messageActionItem": { "additionalPropertiesSupport": false } },
            "showDocument": { "support": true },
        },
        "workspace": {
            "applyEdit": true,
            "workspaceEdit": {
                "documentChanges": true,
                "resourceOperations": ["create", "rename", "delete"],
                "failureHandling": "abort",
            },
            "configuration": true,
            "workspaceFolders": true,
            "didChangeConfiguration": { "dynamicRegistration": true },
            "didChangeWatchedFiles": { "dynamicRegistration": true, "relativePatternSupport": true },
            "symbol": { "symbolKind": { "valueSet": symbol_kinds } },
            "executeCommand": {},
            "fileOperations": { "willRename": true, "didRename": true },
        },
        "textDocument": {
            "synchronization": { "didSave": true, "willSave": false, "willSaveWaitUntil": false },
            "completion": {
                "contextSupport": true,
                "completionItem": {
                    "snippetSupport": true,
                    "commitCharactersSupport": true,
                    "documentationFormat": ["markdown", "plaintext"],
                    "deprecatedSupport": true,
                    "preselectSupport": true,
                    "insertReplaceSupport": true,
                    "labelDetailsSupport": true,
                    "tagSupport": { "valueSet": [1] },
                    "resolveSupport": { "properties": ["documentation", "detail", "additionalTextEdits"] },
                    "insertTextModeSupport": { "valueSet": [1, 2] },
                },
                "completionItemKind": { "valueSet": completion_kinds },
                "completionList": { "itemDefaults": ["commitCharacters", "editRange", "insertTextFormat", "insertTextMode", "data"] },
            },
            "hover": { "contentFormat": ["markdown", "plaintext"] },
            "signatureHelp": {
                "contextSupport": true,
                "signatureInformation": {
                    "documentationFormat": ["markdown", "plaintext"],
                    "parameterInformation": { "labelOffsetSupport": true },
                    "activeParameterSupport": true,
                },
            },
            "definition": { "linkSupport": true },
            "declaration": { "linkSupport": true },
            "typeDefinition": { "linkSupport": true },
            "implementation": { "linkSupport": true },
            "references": {},
            "documentHighlight": {},
            "documentSymbol": {
                "hierarchicalDocumentSymbolSupport": true,
                "symbolKind": { "valueSet": symbol_kinds },
            },
            "codeAction": {
                "codeActionLiteralSupport": {
                    "codeActionKind": {
                        "valueSet": [
                            "", "quickfix", "refactor", "refactor.extract", "refactor.inline",
                            "refactor.rewrite", "source", "source.organizeImports", "source.fixAll",
                        ],
                    },
                },
                "isPreferredSupport": true,
                "disabledSupport": true,
                "dataSupport": true,
                "resolveSupport": { "properties": ["edit"] },
            },
            "formatting": {},
            "rangeFormatting": {},
            "rename": { "prepareSupport": true },
            "colorProvider": {},
            "publishDiagnostics": {
                "relatedInformation": true,
                "versionSupport": true,
                "tagSupport": { "valueSet": [1, 2] },
                "codeDescriptionSupport": true,
                "dataSupport": true,
            },
        },
    })
}
