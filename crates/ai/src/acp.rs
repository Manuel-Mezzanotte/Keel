//! The Agent Client Protocol (ACP), the open protocol Zed made for coding
//! agents: Keel is the client, the agent a program speaking JSON-RPC, one
//! message per line, on its standard input and output. For Claude Code the
//! agent is `@agentclientprotocol/claude-agent-acp`, which uses the Claude
//! Code login already on the Mac: no key in Keel.

use std::collections::HashMap;
use std::io::{self, BufRead as _, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use anyhow::{Context as _, Result, anyhow, bail};
use futures::channel::{mpsc, oneshot};
use serde_json::{Value, json};

/// The protocol version Keel speaks.
pub const PROTOCOL_VERSION: u64 = 1;

/// The adapter for Claude Code, pinned like the language servers.
pub const ADAPTER_PACKAGE: &str = "@agentclientprotocol/claude-agent-acp";
pub const ADAPTER_VERSION: &str = "0.107.3";
const ADAPTER_PROGRAM: &str = "claude-agent-acp";

/// Where Keel installs the adapter, inside its `servers` folder.
pub fn adapter_dir(servers_dir: &Path) -> PathBuf {
    servers_dir.join("agentclientprotocol-claude-agent-acp")
}

/// The adapter's program, if Keel installed it at the pinned version.
pub fn find_adapter(servers_dir: &Path) -> Option<PathBuf> {
    let dir = adapter_dir(servers_dir);
    let manifest = dir
        .join("node_modules")
        .join(ADAPTER_PACKAGE)
        .join("package.json");
    let version = std::fs::read_to_string(manifest)
        .ok()
        .and_then(|text| serde_json::from_str::<Value>(&text).ok())
        .and_then(|manifest| manifest["version"].as_str().map(str::to_owned))?;
    let program = dir.join("node_modules/.bin").join(ADAPTER_PROGRAM);
    (version == ADAPTER_VERSION && program.exists()).then_some(program)
}

/// Installs the adapter with `npm`, found on the `PATH` of `env` (the
/// login shell's). Slow: run it away from the interface.
pub fn install_adapter(
    servers_dir: &Path,
    npm: &Path,
    env: &HashMap<String, String>,
) -> Result<String> {
    let dir = adapter_dir(servers_dir);
    std::fs::create_dir_all(&dir)?;
    let output = Command::new(npm)
        .args(["install", "--prefix"])
        .arg(&dir)
        .args(["--no-audit", "--no-fund", "--loglevel=error"])
        .arg(format!("{ADAPTER_PACKAGE}@{ADAPTER_VERSION}"))
        .current_dir(&dir)
        .envs(env)
        .stdin(Stdio::null())
        .output()
        .with_context(|| format!("running {}", npm.display()))?;
    let printed = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    if !output.status.success() {
        bail!("npm failed: {}", printed.trim());
    }
    Ok(printed)
}

/// Something the agent said or asked.
#[derive(Debug)]
pub enum AgentEvent {
    /// `session/update` and the like.
    Notification { method: String, params: Value },
    /// The agent asks the client (`fs/write_text_file`,
    /// `session/request_permission`...): answer with [`Agent::respond`].
    Request {
        id: Value,
        method: String,
        params: Value,
    },
    /// The agent's program ended; the last lines it wrote to stderr.
    Exited { stderr: String },
}

/// An error the agent answered with.
#[derive(Clone, Debug, PartialEq)]
pub struct AgentError {
    pub code: i64,
    pub message: String,
    pub data: Value,
}

impl std::fmt::Display for AgentError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{} ({})", self.message, self.code)
    }
}

impl std::error::Error for AgentError {}

type Pending = Arc<Mutex<HashMap<u64, oneshot::Sender<Result<Value, AgentError>>>>>;

/// A running agent. Dropping it stops the program.
pub struct Agent {
    writer: Mutex<Box<dyn Write + Send>>,
    pending: Pending,
    next_id: AtomicU64,
    child: Mutex<Option<Child>>,
}

/// How to start the agent's program.
#[derive(Clone, Debug)]
pub struct AgentCommand {
    pub program: PathBuf,
    pub args: Vec<String>,
    pub env: HashMap<String, String>,
    pub cwd: PathBuf,
}

impl Agent {
    /// Starts the program and talks to it.
    pub fn start(
        command: &AgentCommand,
    ) -> io::Result<(Self, mpsc::UnboundedReceiver<AgentEvent>)> {
        let mut child = Command::new(&command.program)
            .args(&command.args)
            .envs(&command.env)
            .current_dir(&command.cwd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| io::Error::other("no stdin"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| io::Error::other("no stdout"))?;
        let stderr = child.stderr.take();
        Self::over(stdout, stdin, stderr, Some(child))
    }

    /// Talks to an agent through `reader` and `writer`: a program's pipes,
    /// or the fake agent of the tests.
    pub fn over(
        reader: impl Read + Send + 'static,
        writer: impl Write + Send + 'static,
        stderr: Option<impl Read + Send + 'static>,
        child: Option<Child>,
    ) -> io::Result<(Self, mpsc::UnboundedReceiver<AgentEvent>)> {
        let (events, receiver) = mpsc::unbounded();
        let pending = Pending::default();
        let tail = Arc::new(Mutex::new(Vec::<String>::new()));
        if let Some(stderr) = stderr {
            let tail = tail.clone();
            std::thread::Builder::new()
                .name("keel-agent-stderr".to_owned())
                .spawn(move || {
                    for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                        let mut tail = tail.lock().unwrap();
                        tail.push(line);
                        if tail.len() > 40 {
                            tail.remove(0);
                        }
                    }
                })?;
        }
        std::thread::Builder::new()
            .name("keel-agent".to_owned())
            .spawn({
                let pending = pending.clone();
                move || {
                    for line in BufReader::new(reader).lines().map_while(Result::ok) {
                        let Ok(message) = serde_json::from_str::<Value>(&line) else {
                            continue;
                        };
                        let method = message["method"].as_str().map(str::to_owned);
                        match (method, message.get("id").cloned()) {
                            (Some(method), Some(id)) => {
                                let params = message.get("params").cloned().unwrap_or(Value::Null);
                                let _ = events.unbounded_send(AgentEvent::Request {
                                    id,
                                    method,
                                    params,
                                });
                            }
                            (Some(method), None) => {
                                let params = message.get("params").cloned().unwrap_or(Value::Null);
                                let _ = events
                                    .unbounded_send(AgentEvent::Notification { method, params });
                            }
                            (None, Some(id)) => {
                                let Some(id) = id.as_u64() else { continue };
                                let Some(answer) = pending.lock().unwrap().remove(&id) else {
                                    continue;
                                };
                                let result = match message.get("error") {
                                    Some(error) => Err(AgentError {
                                        code: error["code"].as_i64().unwrap_or(-32603),
                                        message: error["message"]
                                            .as_str()
                                            .unwrap_or_default()
                                            .to_owned(),
                                        data: error.get("data").cloned().unwrap_or(Value::Null),
                                    }),
                                    None => {
                                        Ok(message.get("result").cloned().unwrap_or(Value::Null))
                                    }
                                };
                                let _ = answer.send(result);
                            }
                            (None, None) => {}
                        }
                    }
                    // Calls still waiting will never be answered.
                    pending.lock().unwrap().clear();
                    let stderr = tail.lock().unwrap().join("\n");
                    let _ = events.unbounded_send(AgentEvent::Exited { stderr });
                }
            })?;
        Ok((
            Self {
                writer: Mutex::new(Box::new(writer)),
                pending,
                next_id: AtomicU64::new(1),
                child: Mutex::new(child),
            },
            receiver,
        ))
    }

    fn write(&self, message: &Value) -> io::Result<()> {
        let mut writer = self.writer.lock().unwrap();
        writeln!(writer, "{message}")?;
        writer.flush()
    }

    /// Asks the agent and waits for the answer.
    pub fn request(
        &self,
        method: &str,
        params: Value,
    ) -> impl Future<Output = Result<Value>> + use<> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let (answer, answered) = oneshot::channel();
        self.pending.lock().unwrap().insert(id, answer);
        let sent =
            self.write(&json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }));
        let method = method.to_owned();
        async move {
            sent.with_context(|| format!("the agent stopped before {method}"))?;
            match answered.await {
                Ok(result) => result.map_err(anyhow::Error::from),
                Err(_) => Err(anyhow!("the agent stopped before answering {method}")),
            }
        }
    }

    pub fn notify(&self, method: &str, params: Value) -> io::Result<()> {
        self.write(&json!({ "jsonrpc": "2.0", "method": method, "params": params }))
    }

    /// Answers an [`AgentEvent::Request`].
    pub fn respond(&self, id: Value, result: Result<Value, (i64, String)>) -> io::Result<()> {
        let message = match result {
            Ok(result) => json!({ "jsonrpc": "2.0", "id": id, "result": result }),
            Err((code, message)) => {
                json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
            }
        };
        self.write(&message)
    }

    // -----------------------------------------------------------------
    // The calls Keel makes

    /// `initialize`: Keel reads and writes files for the agent, so its edits
    /// pass through the editor; no terminal of its own for now.
    pub fn initialize(&self) -> impl Future<Output = Result<Value>> + use<> {
        self.request(
            "initialize",
            json!({
                "protocolVersion": PROTOCOL_VERSION,
                "clientCapabilities": { "fs": { "readTextFile": true, "writeTextFile": true }, "terminal": false },
                "clientInfo": { "name": "keel", "title": "Keel", "version": env!("CARGO_PKG_VERSION") },
            }),
        )
    }

    pub fn new_session(&self, cwd: &Path) -> impl Future<Output = Result<Value>> + use<> {
        self.request("session/new", json!({ "cwd": cwd, "mcpServers": [] }))
    }

    /// Resumes a past conversation: the agent replays it as updates.
    pub fn load_session(
        &self,
        session: &str,
        cwd: &Path,
    ) -> impl Future<Output = Result<Value>> + use<> {
        self.request(
            "session/load",
            json!({ "sessionId": session, "cwd": cwd, "mcpServers": [] }),
        )
    }

    /// The past conversations in `cwd`.
    pub fn list_sessions(&self, cwd: &Path) -> impl Future<Output = Result<Value>> + use<> {
        self.request("session/list", json!({ "cwd": cwd }))
    }

    /// Sends a message; the answer comes as updates, and this ends with the
    /// reason the turn stopped.
    pub fn prompt(
        &self,
        session: &str,
        blocks: Vec<Value>,
    ) -> impl Future<Output = Result<Value>> + use<> {
        self.request(
            "session/prompt",
            json!({ "sessionId": session, "prompt": blocks }),
        )
    }

    pub fn cancel(&self, session: &str) -> io::Result<()> {
        self.notify("session/cancel", json!({ "sessionId": session }))
    }

    /// Changes a session option, such as `mode` (how the agent asks
    /// permission), as `claude-agent-acp` offers them.
    pub fn set_config_option(
        &self,
        session: &str,
        option: &str,
        value: &str,
    ) -> impl Future<Output = Result<Value>> + use<> {
        self.request(
            "session/set_config_option",
            json!({ "sessionId": session, "configId": option, "value": value }),
        )
    }

    pub fn set_mode(
        &self,
        session: &str,
        mode: &str,
    ) -> impl Future<Output = Result<Value>> + use<> {
        self.request(
            "session/set_mode",
            json!({ "sessionId": session, "modeId": mode }),
        )
    }
}

impl Drop for Agent {
    fn drop(&mut self) {
        if let Some(mut child) = self.child.lock().unwrap().take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

/// A selection the user points the agent to, with its text.
pub fn selection_block(path: &Path, lines: (u32, u32), text: &str) -> Value {
    json!({
        "type": "resource",
        "resource": {
            "uri": format!("file://{}#L{}:{}", path.display(), lines.0, lines.1),
            "mimeType": "text/plain",
            "text": text,
        },
    })
}

/// A text block of a prompt.
pub fn text_block(text: &str) -> Value {
    json!({ "type": "text", "text": text })
}

/// A file (or part of one) the user points the agent to: a link to it, as
/// Zed sends `@` mentions.
pub fn file_block(path: &Path, lines: Option<(u32, u32)>) -> Value {
    let mut uri = format!("file://{}", path.display());
    if let Some((start, end)) = lines {
        uri.push_str(&format!("#L{start}:{end}"));
    }
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    json!({ "type": "resource_link", "name": name, "uri": uri })
}

#[cfg(test)]
mod tests {
    use futures::{FutureExt as _, StreamExt as _};

    use super::*;
    use crate::thread::{Entry, Thread, ToolStatus};

    /// Runs `work` while answering the agent: files are read from disk and
    /// permissions get `option`.
    fn drive<T>(
        agent: &Agent,
        events: &mut mpsc::UnboundedReceiver<AgentEvent>,
        thread: &mut Thread,
        option: &str,
        work: impl Future<Output = T>,
    ) -> T {
        futures::executor::block_on(async {
            let mut work = std::pin::pin!(work.fuse());
            loop {
                futures::select! {
                    result = work => {
                        // Updates sent just before the answer.
                        while let Ok(event) = events.try_recv() {
                            if let AgentEvent::Notification { params, .. } = event {
                                thread.apply(&params["update"]);
                            }
                        }
                        return result;
                    }
                    event = events.next() => match event {
                        Some(AgentEvent::Notification { method, params }) if method == "session/update" => {
                            thread.apply(&params["update"]);
                        }
                        Some(AgentEvent::Request { id, method, params }) => {
                            let answer = match method.as_str() {
                                "fs/read_text_file" => Ok(json!({
                                    "content": std::fs::read_to_string(params["path"].as_str().unwrap()).unwrap()
                                })),
                                "session/request_permission" => Ok(json!({ "outcome": { "outcome": "selected", "optionId": option } })),
                                _ => Err((-32601, "unknown".to_owned())),
                            };
                            agent.respond(id, answer).unwrap();
                        }
                        _ => {}
                    },
                }
            }
        })
    }

    #[test]
    fn a_conversation_with_permissions_and_history() {
        let (reader, writer) = crate::fake_agent::spawn().unwrap();
        let (agent, mut events) = Agent::over(reader, writer, None::<io::Empty>, None).unwrap();
        let mut thread = Thread::default();
        let folder = tempfile::tempdir().unwrap();
        let file = folder.path().join("booking.ts");
        std::fs::write(&file, "const price = 90;\n").unwrap();

        let initialized = drive(
            &agent,
            &mut events,
            &mut thread,
            "allow",
            agent.initialize(),
        )
        .unwrap();
        assert_eq!(initialized["agentCapabilities"]["loadSession"], true);
        let session = drive(
            &agent,
            &mut events,
            &mut thread,
            "allow",
            agent.new_session(folder.path()),
        )
        .unwrap();
        let session = session["sessionId"].as_str().unwrap().to_owned();
        assert_eq!(thread.commands[0].0, "review");

        thread.push_user("ciao");
        let answer = drive(
            &agent,
            &mut events,
            &mut thread,
            "allow",
            agent.prompt(&session, vec![text_block("ciao")]),
        )
        .unwrap();
        assert_eq!(answer["stopReason"], "end_turn");
        assert_eq!(
            thread.entries.last(),
            Some(&Entry::Agent("You said: ciao".into()))
        );

        // An edit: read through Keel, permission asked, file written.
        let edit = format!("edit {} 90 95", file.display());
        let answer = drive(
            &agent,
            &mut events,
            &mut thread,
            "allow",
            agent.prompt(&session, vec![text_block(&edit)]),
        )
        .unwrap();
        assert_eq!(answer["stopReason"], "end_turn");
        assert_eq!(
            std::fs::read_to_string(&file).unwrap(),
            "const price = 95;\n"
        );
        let tool = thread.tool("edit-1").unwrap();
        assert_eq!(tool.status, ToolStatus::Completed);
        assert_eq!(thread.plan.len(), 2);
        assert_eq!(thread.changed_files(), std::slice::from_ref(&file));

        // Rejected: the file stays.
        let edit = format!("edit {} 95 99", file.display());
        drive(
            &agent,
            &mut events,
            &mut thread,
            "reject",
            agent.prompt(&session, vec![text_block(&edit)]),
        )
        .unwrap();
        assert_eq!(
            std::fs::read_to_string(&file).unwrap(),
            "const price = 95;\n"
        );

        // History: listed, and replayed when loaded.
        let listed = drive(
            &agent,
            &mut events,
            &mut thread,
            "allow",
            agent.list_sessions(folder.path()),
        )
        .unwrap();
        assert_eq!(listed["sessions"][0]["title"], "ciao");
        let mut replayed = Thread::default();
        drive(
            &agent,
            &mut events,
            &mut replayed,
            "allow",
            agent.load_session(&session, folder.path()),
        )
        .unwrap();
        assert_eq!(replayed.entries[0], Entry::User("ciao".into()));
        assert_eq!(replayed.entries[1], Entry::Agent("You said: ciao".into()));
    }

    #[test]
    fn a_turn_can_be_cancelled() {
        let (reader, writer) = crate::fake_agent::spawn().unwrap();
        let (agent, mut events) = Agent::over(reader, writer, None::<io::Empty>, None).unwrap();
        let mut thread = Thread::default();
        drive(
            &agent,
            &mut events,
            &mut thread,
            "allow",
            agent.initialize(),
        )
        .unwrap();
        let session = drive(
            &agent,
            &mut events,
            &mut thread,
            "allow",
            agent.new_session(Path::new("/tmp")),
        )
        .unwrap();
        let session = session["sessionId"].as_str().unwrap().to_owned();
        let turn = agent.prompt(&session, vec![text_block("wait")]);
        agent.cancel(&session).unwrap();
        let answer = drive(&agent, &mut events, &mut thread, "allow", turn).unwrap();
        assert_eq!(answer["stopReason"], "cancelled");
    }

    #[test]
    fn blocks_point_to_files_and_lines() {
        let block = file_block(Path::new("/p/booking.ts"), Some((3, 7)));
        assert_eq!(block["type"], "resource_link");
        assert_eq!(block["name"], "booking.ts");
        assert_eq!(block["uri"], "file:///p/booking.ts#L3:7");
    }

    /// The real adapter, installed with npm into `$KEEL_ACP_SERVERS`:
    /// starts, opens a session and lists the old ones, without a prompt.
    /// `KEEL_ACP_SERVERS=... cargo test -p ai -- --ignored real_adapter --nocapture`.
    #[test]
    #[ignore]
    fn real_adapter_starts_and_opens_a_session() {
        let servers = PathBuf::from(std::env::var("KEEL_ACP_SERVERS").unwrap());
        let npm = PathBuf::from(
            std::env::var("KEEL_NPM").unwrap_or_else(|_| "/opt/homebrew/bin/npm".to_owned()),
        );
        let env: HashMap<String, String> = std::env::vars().collect();
        let program = match find_adapter(&servers) {
            Some(program) => program,
            None => {
                let started = std::time::Instant::now();
                println!("{}", install_adapter(&servers, &npm, &env).unwrap());
                println!("installed in {:?}", started.elapsed());
                find_adapter(&servers).unwrap()
            }
        };
        let cwd = PathBuf::from(std::env::var("KEEL_ACP_CWD").unwrap());
        let started = std::time::Instant::now();
        let (agent, mut events) = Agent::start(&AgentCommand {
            program,
            args: Vec::new(),
            env,
            cwd: cwd.clone(),
        })
        .unwrap();
        let mut thread = Thread::default();
        let initialized = drive(
            &agent,
            &mut events,
            &mut thread,
            "reject",
            agent.initialize(),
        )
        .unwrap();
        println!(
            "initialize in {:?}: {}",
            started.elapsed(),
            initialized
                .to_string()
                .chars()
                .take(600)
                .collect::<String>()
        );
        let session = drive(
            &agent,
            &mut events,
            &mut thread,
            "reject",
            agent.new_session(&cwd),
        );
        println!(
            "session/new in {:?}: {}",
            started.elapsed(),
            format!("{session:?}").chars().take(800).collect::<String>()
        );
        let listed = drive(
            &agent,
            &mut events,
            &mut thread,
            "reject",
            agent.list_sessions(&cwd),
        );
        println!(
            "session/list: {}",
            format!("{listed:?}").chars().take(600).collect::<String>()
        );
        println!(
            "commands: {:?}",
            thread
                .commands
                .iter()
                .map(|(name, _)| name)
                .collect::<Vec<_>>()
        );
    }
}
