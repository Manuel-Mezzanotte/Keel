//! The connection Claude Code makes to an editor, as the Claude Code
//! extension for VS Code and Cursor offers it:
//!
//! - a WebSocket server on 127.0.0.1, announced by `~/.claude/ide/<port>.lock`
//!   with the window's folders and a secret token;
//! - terminals of the window get `CLAUDE_CODE_SSE_PORT`, so Claude Code
//!   started in them connects by itself (elsewhere `/ide` lists it);
//! - Claude Code speaks MCP over it: it calls the editor's tools
//!   (`getOpenEditors`, `openDiff`...) and receives `selection_changed`,
//!   `at_mentioned` and `diagnostics_changed`.
//!
//! The server only carries messages: the window answers the tools.

use std::io::{self, ErrorKind};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::thread::JoinHandle;
use std::time::Duration;

use futures::channel::{mpsc as async_mpsc, oneshot};
use serde_json::{Value, json};
use tungstenite::Message;
use tungstenite::handshake::server::{ErrorResponse, Request, Response};

/// The header carrying the token of the lock file.
pub const AUTH_HEADER: &str = "x-claude-code-ide-authorization";
/// The variable that tells Claude Code which port to connect to.
pub const PORT_VARIABLE: &str = "CLAUDE_CODE_SSE_PORT";

/// How often the threads look at their channels and the stop flag.
const POLL: Duration = Duration::from_millis(25);

/// Where editors announce themselves: `~/.claude/ide`.
pub fn default_ide_dir() -> Option<PathBuf> {
    std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".claude/ide"))
}

/// Something Claude Code did.
#[derive(Debug)]
pub enum IdeEvent {
    /// Claude Code connected (a new one replaces the one before).
    Connected,
    Disconnected,
    /// A tool call; the window sends the MCP result (`{ "content": [...] }`)
    /// through `reply`, now or later (`openDiff` waits for the user).
    ToolCall {
        name: String,
        arguments: Value,
        reply: oneshot::Sender<Value>,
    },
}

pub struct IdeOptions {
    /// Usually `~/.claude/ide`.
    pub ide_dir: PathBuf,
    /// Shown by Claude Code, as "Cursor" or "Visual Studio Code".
    pub ide_name: String,
    pub workspace_folders: Vec<PathBuf>,
    /// Claude Code checks this process is alive.
    pub pid: u32,
}

/// What the threads share.
struct Shared {
    stop: AtomicBool,
    /// Messages for the connected Claude Code, if any.
    outgoing: Mutex<Option<mpsc::Sender<String>>>,
    /// Which connection is current; older ones close themselves.
    current: AtomicU64,
}

/// A running server. Dropping it removes the lock file and closes it.
pub struct IdeServer {
    port: u16,
    token: String,
    lock_file: PathBuf,
    options: Mutex<IdeOptions>,
    shared: Arc<Shared>,
    thread: Option<JoinHandle<()>>,
}

fn lock_contents(options: &IdeOptions, token: &str) -> String {
    json!({
        "pid": options.pid,
        "workspaceFolders": options.workspace_folders,
        "ideName": options.ide_name,
        "transport": "ws",
        "runningInWindows": false,
        "authToken": token,
    })
    .to_string()
}

/// Writes a file readable only by the user, as the lock file must be.
fn write_private(path: &Path, text: &str) -> io::Result<()> {
    use std::io::Write as _;
    use std::os::unix::fs::OpenOptionsExt as _;
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)?;
    file.write_all(text.as_bytes())
}

impl IdeServer {
    pub fn start(
        options: IdeOptions,
    ) -> io::Result<(Self, async_mpsc::UnboundedReceiver<IdeEvent>)> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        listener.set_nonblocking(true)?;
        let port = listener.local_addr()?.port();
        let token = uuid::Uuid::new_v4().to_string();
        {
            use std::os::unix::fs::DirBuilderExt as _;
            std::fs::DirBuilder::new()
                .recursive(true)
                .mode(0o700)
                .create(&options.ide_dir)?;
        }
        let lock_file = options.ide_dir.join(format!("{port}.lock"));
        write_private(&lock_file, &lock_contents(&options, &token))?;

        let (events, receiver) = async_mpsc::unbounded();
        let shared = Arc::new(Shared {
            stop: AtomicBool::new(false),
            outgoing: Mutex::new(None),
            current: AtomicU64::new(0),
        });
        let thread = std::thread::Builder::new()
            .name("keel-ide-server".to_owned())
            .spawn({
                let shared = shared.clone();
                let token = token.clone();
                move || accept_loop(listener, token, shared, events)
            })?;
        Ok((
            Self {
                port,
                token,
                lock_file,
                options: Mutex::new(options),
                shared,
                thread: Some(thread),
            },
            receiver,
        ))
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    pub fn lock_file(&self) -> &Path {
        &self.lock_file
    }

    /// Whether Claude Code is connected.
    pub fn is_connected(&self) -> bool {
        self.shared.outgoing.lock().unwrap().is_some()
    }

    /// Sends a notification (`selection_changed`...) to Claude Code, if it
    /// is connected.
    pub fn notify(&self, method: &str, params: Value) {
        let message = json!({ "jsonrpc": "2.0", "method": method, "params": params }).to_string();
        if let Some(outgoing) = self.shared.outgoing.lock().unwrap().as_ref() {
            let _ = outgoing.send(message);
        }
    }

    /// The window's folders changed: the lock file follows.
    pub fn set_workspace_folders(&self, folders: Vec<PathBuf>) -> io::Result<()> {
        let mut options = self.options.lock().unwrap();
        options.workspace_folders = folders;
        write_private(&self.lock_file, &lock_contents(&options, &self.token))
    }
}

impl Drop for IdeServer {
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::SeqCst);
        let _ = std::fs::remove_file(&self.lock_file);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn accept_loop(
    listener: TcpListener,
    token: String,
    shared: Arc<Shared>,
    events: async_mpsc::UnboundedSender<IdeEvent>,
) {
    let mut connections = Vec::new();
    while !shared.stop.load(Ordering::SeqCst) {
        match listener.accept() {
            Ok((stream, _)) => {
                let id = shared.current.fetch_add(1, Ordering::SeqCst) + 1;
                let shared = shared.clone();
                let token = token.clone();
                let events = events.clone();
                let thread = std::thread::Builder::new()
                    .name("keel-ide-connection".to_owned())
                    .spawn(move || serve(stream, id, &token, &shared, &events));
                if let Ok(thread) = thread {
                    connections.push(thread);
                }
            }
            Err(error) if error.kind() == ErrorKind::WouldBlock => std::thread::sleep(POLL),
            Err(_) => std::thread::sleep(POLL),
        }
        connections.retain(|thread| !thread.is_finished());
    }
    for thread in connections {
        let _ = thread.join();
    }
}

/// The tools Claude Code may call, with their arguments, as the VS Code
/// extension describes them.
pub fn tools() -> Value {
    let text = |description: &str| json!({ "type": "string", "description": description });
    let flag = |description: &str, default: bool| json!({ "type": "boolean", "description": description, "default": default });
    let tool = |name: &str, description: &str, properties: Value, required: &[&str]| {
        json!({
            "name": name,
            "description": description,
            "inputSchema": { "type": "object", "properties": properties, "required": required },
        })
    };
    json!([
        tool(
            "openDiff",
            "Open a git diff for the file",
            json!({
                "old_file_path": text("Path to the file to show diff for."),
                "new_file_path": text("Path to the file to show diff for."),
                "new_file_contents": text("Contents of the new file."),
                "tab_name": text("Name of the tab with the diff."),
            }),
            &[
                "old_file_path",
                "new_file_path",
                "new_file_contents",
                "tab_name"
            ]
        ),
        tool(
            "getDiagnostics",
            "Get language diagnostics from the editor",
            json!({
                "uri": text("Optional file URI to get diagnostics for. If not provided, gets diagnostics for all files."),
            }),
            &[]
        ),
        tool(
            "close_tab",
            "Close a tab",
            json!({ "tab_name": text("Name of the tab") }),
            &["tab_name"]
        ),
        tool(
            "closeAllDiffTabs",
            "Close all diff tabs in the editor",
            json!({}),
            &[]
        ),
        tool(
            "openFile",
            "Open a file in the editor and optionally select a range of text",
            json!({
                "filePath": text("Path to the file to open"),
                "preview": flag("Whether to open the file in preview mode", false),
                "startText": text("Text pattern to find the start of the selection range. Selects from the beginning of this match."),
                "endText": text("Text pattern to find the end of the selection range. Selects up to the end of this match. If not provided, only the startText match will be selected."),
                "selectToEndOfLine": flag("If true, selection will extend to the end of the line containing the endText match.", false),
                "makeFrontmost": flag("Whether to make the file the active editor tab.", true),
            }),
            &["filePath"]
        ),
        tool(
            "getOpenEditors",
            "Get information about currently open editors",
            json!({}),
            &[]
        ),
        tool(
            "getWorkspaceFolders",
            "Get all workspace folders currently open in the IDE",
            json!({}),
            &[]
        ),
        tool(
            "getCurrentSelection",
            "Get the current text selection in the active editor",
            json!({}),
            &[]
        ),
        tool(
            "getLatestSelection",
            "Get the most recent text selection (even if not in the active editor)",
            json!({}),
            &[]
        ),
        tool(
            "checkDocumentDirty",
            "Check if a document has unsaved changes (is dirty)",
            json!({
                "filePath": text("Path to the file to check"),
            }),
            &["filePath"]
        ),
        tool(
            "saveDocument",
            "Save a document with unsaved changes",
            json!({
                "filePath": text("Path to the file to save"),
            }),
            &["filePath"]
        ),
    ])
}

/// A tool result with one text, as MCP wants it.
pub fn text_result(text: impl Into<String>) -> Value {
    json!({ "content": [{ "type": "text", "text": text.into() }] })
}

/// A tool result with a JSON value as text, as the VS Code extension answers.
pub fn json_result(value: &Value) -> Value {
    text_result(serde_json::to_string_pretty(value).unwrap_or_default())
}

fn serve(
    stream: TcpStream,
    id: u64,
    token: &str,
    shared: &Arc<Shared>,
    events: &async_mpsc::UnboundedSender<IdeEvent>,
) {
    let _ = stream.set_nonblocking(false);
    // The error type of the handshake callback is set by tungstenite.
    #[allow(clippy::result_large_err)]
    let check = |request: &Request, response: Response| -> Result<Response, ErrorResponse> {
        if std::env::var_os("KEEL_IDE_TRACE").is_some() {
            let headers: Vec<String> = request
                .headers()
                .iter()
                .filter(|(name, _)| name.as_str() != AUTH_HEADER)
                .map(|(name, value)| format!("{name}: {value:?}"))
                .collect();
            eprintln!("keel ide: handshake {} {headers:?}", request.uri());
        }
        let given = request
            .headers()
            .get(AUTH_HEADER)
            .and_then(|value| value.to_str().ok());
        if given == Some(token) {
            let mut response = response;
            // Claude Code asks for the `mcp` subprotocol and drops a
            // server that does not confirm it.
            let asks_mcp = request
                .headers()
                .get("sec-websocket-protocol")
                .and_then(|value| value.to_str().ok())
                .is_some_and(|protocols| {
                    protocols
                        .split(',')
                        .any(|protocol| protocol.trim() == "mcp")
                });
            if asks_mcp {
                response.headers_mut().insert(
                    "sec-websocket-protocol",
                    tungstenite::http::HeaderValue::from_static("mcp"),
                );
            }
            Ok(response)
        } else {
            let mut refused = ErrorResponse::new(Some("Unauthorized".to_owned()));
            *refused.status_mut() = tungstenite::http::StatusCode::UNAUTHORIZED;
            Err(refused)
        }
    };
    let Ok(mut socket) = tungstenite::accept_hdr(stream, check) else {
        return;
    };
    let _ = socket.get_ref().set_read_timeout(Some(POLL));
    let (outgoing, queue) = mpsc::channel::<String>();
    *shared.outgoing.lock().unwrap() = Some(outgoing.clone());
    let _ = events.unbounded_send(IdeEvent::Connected);

    loop {
        if shared.stop.load(Ordering::SeqCst) || shared.current.load(Ordering::SeqCst) != id {
            let _ = socket.close(None);
            let _ = socket.flush();
            break;
        }
        while let Ok(text) = queue.try_recv() {
            if socket.send(Message::text(text)).is_err() {
                break;
            }
        }
        match socket.read() {
            Ok(Message::Text(text)) => handle(&text, &outgoing, events),
            Ok(Message::Close(frame)) => {
                if std::env::var_os("KEEL_IDE_TRACE").is_some() {
                    eprintln!("keel ide: closed by Claude Code {frame:?}");
                }
                break;
            }
            Ok(_) => {}
            Err(tungstenite::Error::Io(error))
                if matches!(error.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {}
            Err(error) => {
                if std::env::var_os("KEEL_IDE_TRACE").is_some() {
                    eprintln!("keel ide: {error}");
                }
                break;
            }
        }
    }
    // Only the current connection clears the queue and says goodbye.
    if shared.current.load(Ordering::SeqCst) == id {
        *shared.outgoing.lock().unwrap() = None;
        let _ = events.unbounded_send(IdeEvent::Disconnected);
    }
}

fn respond(outgoing: &mpsc::Sender<String>, id: &Value, result: Result<Value, (i64, String)>) {
    let message = match result {
        Ok(result) => json!({ "jsonrpc": "2.0", "id": id, "result": result }),
        Err((code, message)) => {
            json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
        }
    };
    let _ = outgoing.send(message.to_string());
}

/// One message from Claude Code: MCP over JSON-RPC.
fn handle(
    text: &str,
    outgoing: &mpsc::Sender<String>,
    events: &async_mpsc::UnboundedSender<IdeEvent>,
) {
    if std::env::var_os("KEEL_IDE_TRACE").is_some() {
        eprintln!("keel ide <- {}", text.chars().take(400).collect::<String>());
    }
    let Ok(message) = serde_json::from_str::<Value>(text) else {
        return;
    };
    let Some(id) = message.get("id").cloned() else {
        // Notifications, such as `notifications/initialized`.
        return;
    };
    let method = message["method"].as_str().unwrap_or_default();
    let params = &message["params"];
    match method {
        "initialize" => respond(
            outgoing,
            &id,
            Ok(json!({
                "protocolVersion": params["protocolVersion"].as_str().unwrap_or("2025-06-18"),
                "capabilities": { "tools": { "listChanged": true } },
                "serverInfo": { "name": "Keel", "version": env!("CARGO_PKG_VERSION") },
            })),
        ),
        "ping" => respond(outgoing, &id, Ok(json!({}))),
        "tools/list" => respond(outgoing, &id, Ok(json!({ "tools": tools() }))),
        "tools/call" => {
            let (reply, answer) = oneshot::channel();
            let call = IdeEvent::ToolCall {
                name: params["name"].as_str().unwrap_or_default().to_owned(),
                arguments: params
                    .get("arguments")
                    .cloned()
                    .unwrap_or_else(|| json!({})),
                reply,
            };
            if events.unbounded_send(call).is_err() {
                respond(outgoing, &id, Err((-32603, "Keel is closing".to_owned())));
                return;
            }
            // The answer may take long (a diff waits for the user): wait
            // apart, so other calls go on.
            let outgoing = outgoing.clone();
            std::thread::spawn(move || {
                let result = futures::executor::block_on(answer)
                    .map_err(|_| (-32603, "the window closed before answering".to_owned()));
                respond(&outgoing, &id, result);
            });
        }
        _ => respond(
            outgoing,
            &id,
            Err((-32601, format!("Method not found: {method}"))),
        ),
    }
}

#[cfg(test)]
mod tests {
    use futures::StreamExt as _;
    use tungstenite::WebSocket;
    use tungstenite::client::IntoClientRequest as _;

    use super::*;

    fn options(dir: &Path) -> IdeOptions {
        IdeOptions {
            ide_dir: dir.join("ide"),
            ide_name: "Keel".to_owned(),
            workspace_folders: vec![PathBuf::from("/progetti/Hospitality")],
            pid: std::process::id(),
        }
    }

    /// Connects as Claude Code does, asking for the `mcp` subprotocol.
    fn connect(
        port: u16,
        token: &str,
    ) -> tungstenite::Result<WebSocket<tungstenite::stream::MaybeTlsStream<TcpStream>>> {
        let mut request = format!("ws://127.0.0.1:{port}")
            .into_client_request()
            .unwrap();
        request
            .headers_mut()
            .insert(AUTH_HEADER, token.parse().unwrap());
        request
            .headers_mut()
            .insert("sec-websocket-protocol", "mcp".parse().unwrap());
        let (socket, response) = tungstenite::connect(request)?;
        assert_eq!(
            response.headers().get("sec-websocket-protocol").unwrap(),
            "mcp"
        );
        Ok(socket)
    }

    fn call(
        socket: &mut WebSocket<tungstenite::stream::MaybeTlsStream<TcpStream>>,
        message: Value,
    ) -> Value {
        socket.send(Message::text(message.to_string())).unwrap();
        loop {
            if let Message::Text(text) = socket.read().unwrap() {
                let answer: Value = serde_json::from_str(&text).unwrap();
                if answer.get("id") == message.get("id") {
                    return answer;
                }
            }
        }
    }

    fn token_of(lock: &Path) -> String {
        let lock: Value = serde_json::from_str(&std::fs::read_to_string(lock).unwrap()).unwrap();
        lock["authToken"].as_str().unwrap().to_owned()
    }

    #[test]
    fn claude_code_finds_connects_and_calls_tools() {
        let dir = tempfile::tempdir().unwrap();
        let (server, mut events) = IdeServer::start(options(dir.path())).unwrap();

        // The lock file announces the window, readable only by the user.
        let lock = server.lock_file().to_path_buf();
        assert_eq!(lock, dir.path().join(format!("ide/{}.lock", server.port())));
        let announced: Value =
            serde_json::from_str(&std::fs::read_to_string(&lock).unwrap()).unwrap();
        assert_eq!(announced["ideName"], "Keel");
        assert_eq!(announced["transport"], "ws");
        assert_eq!(
            announced["workspaceFolders"],
            json!(["/progetti/Hospitality"])
        );
        use std::os::unix::fs::PermissionsExt as _;
        assert_eq!(
            std::fs::metadata(&lock).unwrap().permissions().mode() & 0o777,
            0o600
        );
        let token = token_of(&lock);

        // Without the token the door stays shut.
        assert!(connect(server.port(), "wrong").is_err());

        let mut socket = connect(server.port(), &token).unwrap();
        let initialized = call(
            &mut socket,
            json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize",
            "params": { "protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": { "name": "claude-code" } } }),
        );
        assert_eq!(initialized["result"]["serverInfo"]["name"], "Keel");
        let listed = call(
            &mut socket,
            json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/list" }),
        );
        let names: Vec<&str> = listed["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|tool| tool["name"].as_str().unwrap())
            .collect();
        assert!(
            names.contains(&"openDiff") && names.contains(&"getDiagnostics"),
            "{names:?}"
        );

        assert!(matches!(
            futures::executor::block_on(events.next()),
            Some(IdeEvent::Connected)
        ));
        // A tool call goes to the window, and its answer comes back.
        socket
            .send(Message::text(
                json!({ "jsonrpc": "2.0", "id": 3, "method": "tools/call",
                "params": { "name": "getWorkspaceFolders", "arguments": {} } })
                .to_string(),
            ))
            .unwrap();
        match futures::executor::block_on(events.next()) {
            Some(IdeEvent::ToolCall { name, reply, .. }) => {
                assert_eq!(name, "getWorkspaceFolders");
                reply
                    .send(json_result(&json!({ "success": true })))
                    .unwrap();
            }
            other => panic!("{other:?}"),
        }
        let answer = loop {
            if let Message::Text(text) = socket.read().unwrap() {
                break serde_json::from_str::<Value>(&text).unwrap();
            }
        };
        assert_eq!(answer["id"], 3);
        assert!(
            answer["result"]["content"][0]["text"]
                .as_str()
                .unwrap()
                .contains("\"success\": true")
        );

        // Notifications reach Claude Code.
        server.notify(
            "at_mentioned",
            json!({ "filePath": "/progetti/a.ts", "lineStart": 2, "lineEnd": 4 }),
        );
        let notified = loop {
            if let Message::Text(text) = socket.read().unwrap() {
                break serde_json::from_str::<Value>(&text).unwrap();
            }
        };
        assert_eq!(notified["method"], "at_mentioned");
        assert_eq!(notified["params"]["lineEnd"], 4);

        server
            .set_workspace_folders(vec![PathBuf::from("/progetti/Salon")])
            .unwrap();
        assert!(
            std::fs::read_to_string(&lock)
                .unwrap()
                .contains("/progetti/Salon")
        );

        // Closing the window removes the announcement.
        drop(server);
        assert!(!lock.exists());
    }

    #[test]
    fn a_new_connection_replaces_the_old_one() {
        let dir = tempfile::tempdir().unwrap();
        let (server, mut events) = IdeServer::start(options(dir.path())).unwrap();
        let token = token_of(server.lock_file());
        let mut first = connect(server.port(), &token).unwrap();
        assert!(matches!(
            futures::executor::block_on(events.next()),
            Some(IdeEvent::Connected)
        ));
        let _second = connect(server.port(), &token).unwrap();
        assert!(matches!(
            futures::executor::block_on(events.next()),
            Some(IdeEvent::Connected)
        ));
        // The first one is closed by Keel.
        let closed = loop {
            match first.read() {
                Ok(Message::Close(_)) | Err(_) => break true,
                Ok(_) => {}
            }
        };
        assert!(closed);
        assert!(server.is_connected());
    }
}

/// Starts the real `claude` against a Keel server for a few seconds,
/// without typing anything, and prints what it asked:
/// `cargo test -p ai -- --ignored real_claude --nocapture`.
#[cfg(test)]
mod real {
    use futures::StreamExt as _;

    use super::*;

    #[test]
    #[ignore]
    fn real_claude_connects() {
        // A folder Claude Code already trusts, or it stops to ask.
        let project = PathBuf::from(std::env::var("KEEL_CLAUDE_PROJECT").unwrap());
        let (server, mut events) = IdeServer::start(IdeOptions {
            ide_dir: default_ide_dir().unwrap(),
            ide_name: "Keel".to_owned(),
            workspace_folders: vec![project.canonicalize().unwrap()],
            pid: std::process::id(),
        })
        .unwrap();
        let screen = std::env::var("KEEL_CLAUDE_SCREEN").unwrap_or_else(|_| "/dev/null".to_owned());
        let extra: Vec<String> = std::env::var("KEEL_CLAUDE_ARGS")
            .unwrap_or_default()
            .split_whitespace()
            .map(str::to_owned)
            .collect();
        let helper = std::env::var("KEEL_PTY_RUN").unwrap();
        let mut child = std::process::Command::new("/usr/bin/python3")
            .args(["-I", &helper, "12", &screen, "claude"])
            .args(&extra)
            .current_dir(&project)
            .env(PORT_VARIABLE, server.port().to_string())
            .env("ENABLE_IDE_INTEGRATION", "true")
            .spawn()
            .unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(12);
        while std::time::Instant::now() < deadline {
            let next = futures::executor::block_on(async {
                futures::select! {
                    event = events.next() => event,
                    _ = futures::FutureExt::fuse(async { std::thread::sleep(Duration::from_millis(200)) }) => None,
                }
            });
            match next {
                Some(IdeEvent::ToolCall {
                    name,
                    arguments,
                    reply,
                }) => {
                    println!("tool {name} {arguments}");
                    let _ = reply.send(json_result(
                        &json!({ "success": true, "tabs": [], "folders": [] }),
                    ));
                }
                Some(other) => println!("{other:?}"),
                None => {}
            }
        }
        let _ = child.kill();
        let _ = child.wait();
        println!("connected at the end: {}", server.is_connected());
    }
}
