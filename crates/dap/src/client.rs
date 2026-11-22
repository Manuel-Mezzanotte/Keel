//! The connection to a debug adapter: requests and their answers, the
//! events it sends, and what it asks of the editor.

use std::collections::HashMap;
use std::future::Future;
use std::io::{BufRead as _, BufReader, Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::os::unix::process::CommandExt as _;
use std::path::PathBuf;
use std::pin::Pin;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::task::{Context, Poll};
use std::time::{Duration, Instant};

use anyhow::{Context as _, Result, anyhow, bail};
use futures::channel::oneshot;
use lsp::rpc::{read_message, write_message};
use serde_json::{Value, json};

/// What an adapter says on its own.
#[derive(Clone, Debug, PartialEq)]
pub enum ClientEvent {
    /// `stopped`, `output`, `terminated`…
    Event { event: String, body: Value },
    /// Something it asks: `runInTerminal`, `startDebugging`.
    Request {
        seq: u64,
        command: String,
        arguments: Value,
    },
    /// A line of the adapter's own errors.
    Stderr(String),
    /// The connection is over.
    Closed,
}

pub type Handler = Arc<dyn Fn(ClientEvent) + Send + Sync>;

/// How to start an adapter.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AdapterCommand {
    pub program: PathBuf,
    pub args: Vec<String>,
    pub cwd: Option<PathBuf>,
    pub env: Vec<(String, String)>,
}

impl AdapterCommand {
    fn command(&self) -> Command {
        let mut command = Command::new(&self.program);
        command
            .args(&self.args)
            .envs(self.env.iter().map(|(key, value)| (key, value)))
            // A group of its own, to end it with what it started.
            .process_group(0);
        if let Some(cwd) = &self.cwd {
            command.current_dir(cwd);
        }
        command
    }
}

type Waiting = HashMap<u64, oneshot::Sender<Result<Value>>>;

pub struct Client {
    outgoing: mpsc::Sender<Value>,
    waiting: Arc<Mutex<Option<Waiting>>>,
    seq: AtomicU64,
    child: Mutex<Option<Child>>,
}

impl std::fmt::Debug for Client {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.debug_struct("Client").finish_non_exhaustive()
    }
}

/// The answer to a request, when it comes.
pub struct Response(oneshot::Receiver<Result<Value>>);

impl Future for Response {
    type Output = Result<Value>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        Pin::new(&mut self.0)
            .poll(cx)
            .map(|answer| answer.unwrap_or_else(|_| Err(anyhow!("the debug adapter closed"))))
    }
}

impl Client {
    /// Talks over `reader` and `writer`; `handler` hears what comes on its
    /// own, from another thread.
    pub fn start(
        reader: impl Read + Send + 'static,
        writer: impl Write + Send + 'static,
        handler: Handler,
    ) -> Arc<Self> {
        Self::with_child(reader, writer, None, handler)
    }

    fn with_child(
        reader: impl Read + Send + 'static,
        mut writer: impl Write + Send + 'static,
        child: Option<Child>,
        handler: Handler,
    ) -> Arc<Self> {
        let (outgoing, messages) = mpsc::channel::<Value>();
        // Written from a thread of its own: an adapter that does not read
        // never holds the window.
        std::thread::spawn(move || {
            for message in messages {
                if write_message(&mut writer, &message).is_err() {
                    break;
                }
            }
        });
        let waiting: Arc<Mutex<Option<Waiting>>> = Arc::new(Mutex::new(Some(HashMap::new())));
        let client = Arc::new(Self {
            outgoing,
            waiting: waiting.clone(),
            seq: AtomicU64::new(1),
            child: Mutex::new(child),
        });
        std::thread::spawn(move || {
            let mut reader = BufReader::new(reader);
            while let Ok(Some(message)) = read_message(&mut reader) {
                take(&message, &waiting, &handler);
            }
            // Whoever waits for an answer hears that none will come.
            waiting.lock().unwrap().take();
            handler(ClientEvent::Closed);
        });
        client
    }

    /// Starts an adapter that talks over its standard input and output.
    pub fn spawn(command: &AdapterCommand, handler: Handler) -> Result<Arc<Self>> {
        let mut child = command
            .command()
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .with_context(|| format!("starting {}", command.program.display()))?;
        let stdin = child.stdin.take().context("the adapter's input")?;
        let stdout = child.stdout.take().context("the adapter's output")?;
        if let Some(stderr) = child.stderr.take() {
            let handler = handler.clone();
            std::thread::spawn(move || lines(stderr, &|line| handler(ClientEvent::Stderr(line))));
        }
        Ok(Self::with_child(stdout, stdin, Some(child), handler))
    }

    /// Starts an adapter that listens on a port and says which one, as
    /// js-debug does: `Debug server listening at 127.0.0.1:8123`. The
    /// process is the first client's, and ends with it.
    pub fn spawn_server(
        command: &AdapterCommand,
        patience: Duration,
        handler: Handler,
    ) -> Result<(Arc<Self>, String)> {
        let mut child = command
            .command()
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .with_context(|| format!("starting {}", command.program.display()))?;
        let stdout = child.stdout.take().context("the adapter's output")?;
        let (said, address) = mpsc::channel();
        let out = handler.clone();
        std::thread::spawn(move || {
            lines(stdout, &|line| {
                if let Some(at) = line.split("listening at ").nth(1) {
                    let _ = said.send(at.trim().to_owned());
                }
                out(ClientEvent::Stderr(line));
            });
        });
        if let Some(stderr) = child.stderr.take() {
            let handler = handler.clone();
            std::thread::spawn(move || lines(stderr, &|line| handler(ClientEvent::Stderr(line))));
        }
        let connected = address
            .recv_timeout(patience)
            .map_err(|_| anyhow!("{} did not say where it listens", command.program.display()))
            .and_then(|address| Ok((connect(&address, patience)?, address)));
        match connected {
            Ok((stream, address)) => {
                let reader = stream.try_clone()?;
                Ok((
                    Self::with_child(reader, stream, Some(child), handler),
                    address,
                ))
            }
            Err(error) => {
                end(&mut child);
                Err(error)
            }
        }
    }

    /// Another session with a server already started.
    pub fn connect(address: &str, handler: Handler) -> Result<Arc<Self>> {
        let stream = connect(address, Duration::from_secs(5))?;
        let reader = stream.try_clone()?;
        Ok(Self::start(reader, stream, handler))
    }

    pub fn request(&self, command: &str, arguments: Value) -> Response {
        let seq = self.seq.fetch_add(1, Ordering::SeqCst);
        let (sender, receiver) = oneshot::channel();
        // Closed: the sender is dropped, and the answer says so.
        if let Some(waiting) = self.waiting.lock().unwrap().as_mut() {
            waiting.insert(seq, sender);
        }
        let mut message = json!({ "seq": seq, "type": "request", "command": command });
        if !arguments.is_null() {
            message["arguments"] = arguments;
        }
        let _ = self.outgoing.send(message);
        Response(receiver)
    }

    /// Answers something the adapter asked.
    pub fn respond(&self, request: u64, command: &str, answer: Result<Value, String>) {
        let seq = self.seq.fetch_add(1, Ordering::SeqCst);
        let mut message = json!({
            "seq": seq,
            "type": "response",
            "request_seq": request,
            "command": command,
            "success": answer.is_ok(),
        });
        match answer {
            Ok(body) => message["body"] = body,
            Err(error) => message["message"] = Value::String(error),
        }
        let _ = self.outgoing.send(message);
    }

    /// Ends the adapter's process, with what it started.
    pub fn kill(&self) {
        if let Some(mut child) = self.child.lock().unwrap().take() {
            // Asked at once; waited for away from the window.
            ask_to_end(&child);
            std::thread::spawn(move || end(&mut child));
        }
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        self.kill();
    }
}

fn ask_to_end(child: &Child) {
    // SAFETY: a signal to the group of a child not yet waited for.
    unsafe { libc::kill(-(child.id() as i32), libc::SIGTERM) };
}

/// Ends a process and its group: asked first, then by force.
fn end(child: &mut Child) {
    let group = -(child.id() as i32);
    ask_to_end(child);
    let deadline = Instant::now() + Duration::from_millis(300);
    while Instant::now() < deadline {
        if matches!(child.try_wait(), Ok(Some(_))) {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    // SAFETY: as above; the child has not been waited for.
    unsafe { libc::kill(group, libc::SIGKILL) };
    let _ = child.wait();
}

fn connect(address: &str, patience: Duration) -> Result<TcpStream> {
    let deadline = Instant::now() + patience;
    let place = address
        .to_socket_addrs()?
        .next()
        .with_context(|| format!("no address in {address}"))?;
    // An adapter is on this Mac; anywhere else is not one Keel started.
    if !place.ip().is_loopback() {
        bail!("{address} is not on this Mac");
    }
    loop {
        match TcpStream::connect_timeout(&place, Duration::from_secs(1)) {
            Ok(stream) => {
                stream.set_nodelay(true).ok();
                return Ok(stream);
            }
            Err(error) if Instant::now() >= deadline => return Err(error.into()),
            Err(_) => std::thread::sleep(Duration::from_millis(50)),
        }
    }
}

fn lines(reader: impl Read, each: &dyn Fn(String)) {
    for line in BufReader::new(reader).lines() {
        match line {
            Ok(line) => each(line),
            Err(_) => break,
        }
    }
}

fn take(message: &Value, waiting: &Mutex<Option<Waiting>>, handler: &Handler) {
    match message["type"].as_str() {
        Some("response") => {
            let seq = message["request_seq"].as_u64().unwrap_or_default();
            let sender = waiting
                .lock()
                .unwrap()
                .as_mut()
                .and_then(|waiting| waiting.remove(&seq));
            let Some(sender) = sender else {
                return;
            };
            let answer = if message["success"].as_bool() == Some(true) {
                Ok(message["body"].clone())
            } else {
                Err(anyhow!("{}", error_text(message)))
            };
            let _ = sender.send(answer);
        }
        Some("event") => handler(ClientEvent::Event {
            event: message["event"].as_str().unwrap_or_default().to_owned(),
            body: message["body"].clone(),
        }),
        Some("request") => handler(ClientEvent::Request {
            seq: message["seq"].as_u64().unwrap_or_default(),
            command: message["command"].as_str().unwrap_or_default().to_owned(),
            arguments: message["arguments"].clone(),
        }),
        _ => {}
    }
}

/// Why a request failed: the message for people, with its variables, or
/// the short one.
fn error_text(message: &Value) -> String {
    let error = &message["body"]["error"];
    let mut text = error["format"].as_str().unwrap_or_default().to_owned();
    for (name, value) in error["variables"].as_object().into_iter().flatten() {
        let value = value.as_str().unwrap_or_default();
        text = text.replace(&format!("{{{name}}}"), value);
    }
    if text.is_empty() {
        text = message["message"]
            .as_str()
            .unwrap_or("the debug adapter refused")
            .to_owned();
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn errors_are_filled_in() {
        let message = json!({
            "message": "short",
            "body": { "error": { "format": "No {what} at {where}", "variables": { "what": "file", "where": "/a" } } },
        });
        assert_eq!(error_text(&message), "No file at /a");
        assert_eq!(error_text(&json!({ "message": "short" })), "short");
    }

    #[test]
    fn a_process_that_ends_closes_the_client() {
        let (sender, events) = mpsc::channel();
        let command = AdapterCommand {
            program: "/bin/sh".into(),
            args: vec!["-c".into(), "echo no >&2; exit 3".into()],
            ..AdapterCommand::default()
        };
        let handler: Handler = Arc::new(move |event| {
            let _ = sender.send(event);
        });
        let client = Client::spawn(&command, handler).unwrap();
        let mut seen = Vec::new();
        while let Ok(event) = events.recv_timeout(Duration::from_secs(5)) {
            let closed = event == ClientEvent::Closed;
            seen.push(event);
            if closed && seen.len() > 1 {
                break;
            }
        }
        assert!(seen.contains(&ClientEvent::Stderr("no".into())), "{seen:?}");
        let answer = futures::executor::block_on(client.request("initialize", Value::Null));
        assert!(answer.is_err());
    }

    #[test]
    fn only_this_mac_is_connected_to() {
        assert!(connect("192.0.2.1:80", Duration::from_millis(10)).is_err());
    }
}
