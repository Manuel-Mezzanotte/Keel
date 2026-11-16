//! The extension host: a V8 runtime on its own thread that runs extensions.
//!
//! Keel and the host talk through JSON messages. Either side can send a
//! notification (no answer) or a call (answered by a reply with the same id).

use std::cell::RefCell;
use std::collections::HashMap;
use std::fs;
use std::future::Future;
use std::io;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use anyhow::{Context as _, Result, anyhow};
use deno_core::{JsRuntime, OpState, RuntimeOptions, extension, op2};
use deno_error::JsErrorBox;
use futures::channel::{mpsc, oneshot};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest as _, Sha256};

use crate::vsix::InstalledExtension;

const RUNTIME_JS: &str = include_str!("js/runtime.js");
const VSCODE_JS: &str = include_str!("js/vscode.js");

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
enum Message {
    Notify {
        method: String,
        #[serde(default)]
        params: Value,
    },
    Call {
        id: u64,
        method: String,
        #[serde(default)]
        params: Value,
    },
    Reply {
        id: u64,
        #[serde(default)]
        result: Value,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        error: Option<String>,
    },
}

/// Something the extension host tells or asks Keel.
#[derive(Clone, Debug, PartialEq)]
pub enum HostEvent {
    /// No answer expected.
    Notify { method: String, params: Value },
    /// Keel must answer with [`ExtensionHost::reply`].
    Call {
        id: u64,
        method: String,
        params: Value,
    },
}

/// The calls waiting for the host's answer, with what each one asked.
type Pending = Arc<Mutex<HashMap<u64, (oneshot::Sender<Result<Value, String>>, String)>>>;

/// What the host thread's operations share, kept in V8's op state.
struct HostChannels {
    inbox: Rc<tokio::sync::Mutex<tokio::sync::mpsc::UnboundedReceiver<String>>>,
    events: mpsc::UnboundedSender<HostEvent>,
    pending: Pending,
    readable_roots: Arc<Mutex<Vec<PathBuf>>>,
}

/// A running extension host.
pub struct ExtensionHost {
    inbox: tokio::sync::mpsc::UnboundedSender<String>,
    pending: Pending,
    next_id: AtomicU64,
    readable_roots: Arc<Mutex<Vec<PathBuf>>>,
    shutdown: Option<tokio::sync::oneshot::Sender<()>>,
    thread: Option<JoinHandle<()>>,
    /// Stops JavaScript that never returns, such as an endless loop.
    isolate: deno_core::v8::IsolateHandle,
    /// Signals that the host's thread has finished.
    finished: std::sync::mpsc::Receiver<()>,
}

/// How long dropping a host waits for its thread before leaving it.
const STOP_WAIT: std::time::Duration = std::time::Duration::from_secs(2);

impl ExtensionHost {
    /// Starts V8 on a new thread. The receiver gets everything extensions
    /// tell or ask Keel; calls must be answered with [`Self::reply`].
    pub fn start() -> Result<(Self, mpsc::UnboundedReceiver<HostEvent>)> {
        let (inbox_sender, inbox) = tokio::sync::mpsc::unbounded_channel::<String>();
        let (events, event_receiver) = mpsc::unbounded();
        let pending = Pending::default();
        let readable_roots = Arc::new(Mutex::new(Vec::new()));
        let (shutdown, shutdown_signal) = tokio::sync::oneshot::channel::<()>();
        let (started, started_signal) =
            std::sync::mpsc::channel::<Result<deno_core::v8::IsolateHandle>>();
        let (done, finished) = std::sync::mpsc::channel::<()>();

        let thread = std::thread::Builder::new()
            .name("keel-extension-host".to_owned())
            .spawn({
                let pending = pending.clone();
                let readable_roots = readable_roots.clone();
                move || {
                    // Tells the window the thread is over, however it ends.
                    struct Finished(std::sync::mpsc::Sender<()>);
                    impl Drop for Finished {
                        fn drop(&mut self) {
                            let _ = self.0.send(());
                        }
                    }
                    let _finished = Finished(done);
                    let runtime = match tokio::runtime::Builder::new_current_thread()
                        .enable_all()
                        .build()
                    {
                        Ok(runtime) => runtime,
                        Err(error) => {
                            let _ = started.send(Err(error.into()));
                            return;
                        }
                    };
                    runtime.block_on(async move {
                        let mut js = JsRuntime::new(RuntimeOptions {
                            extensions: vec![keel_host::init()],
                            ..Default::default()
                        });
                        js.op_state().borrow_mut().put(HostChannels {
                            inbox: Rc::new(tokio::sync::Mutex::new(inbox)),
                            events,
                            pending,
                            readable_roots,
                        });
                        let setup = [
                            ("keel:runtime.js", RUNTIME_JS),
                            ("keel:vscode.js", VSCODE_JS),
                            ("keel:start.js", "globalThis.__keel.messageLoop();"),
                        ]
                        .into_iter()
                        .try_for_each(|(name, source)| {
                            js.execute_script(name, source.to_owned())
                                .map(|_| ())
                                .map_err(|error| anyhow!("{name}: {error}"))
                        });
                        let failed = setup.is_err();
                        let _ = started.send(setup.map(|()| js.v8_isolate().thread_safe_handle()));
                        if failed {
                            return;
                        }
                        tokio::select! {
                            result = js.run_event_loop(Default::default()) => {
                                if let Err(error) = result {
                                    eprintln!("keel: the extension host stopped: {error}");
                                }
                            }
                            _ = shutdown_signal => {}
                        }
                    });
                }
            })?;

        let isolate = started_signal
            .recv()
            .context("the extension host thread ended while starting")??;

        Ok((
            Self {
                inbox: inbox_sender,
                pending,
                next_id: AtomicU64::new(1),
                readable_roots,
                shutdown: Some(shutdown),
                thread: Some(thread),
                isolate,
                finished,
            },
            event_receiver,
        ))
    }

    fn send(&self, message: &Message) -> Result<()> {
        let text = serde_json::to_string(message)?;
        self.inbox
            .send(text)
            .map_err(|_| anyhow!("the extension host has stopped"))
    }

    /// Tells the host something, without waiting for an answer.
    pub fn notify(&self, method: &str, params: Value) -> Result<()> {
        self.send(&Message::Notify {
            method: method.to_owned(),
            params,
        })
    }

    /// Asks the host something and waits for the answer.
    pub fn call(&self, method: &str, params: Value) -> impl Future<Output = Result<Value>> + use<> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let (answer, answered) = oneshot::channel();
        // What the call is about, to name what keeps a stuck host busy.
        let subject = ["command", "id", "extension"]
            .iter()
            .find_map(|key| params[*key].as_str())
            .map_or_else(
                || method.to_owned(),
                |subject| format!("{method} {subject}"),
            );
        self.pending.lock().unwrap().insert(id, (answer, subject));
        let sent = self.send(&Message::Call {
            id,
            method: method.to_owned(),
            params,
        });
        async move {
            sent?;
            match answered.await {
                Ok(Ok(value)) => Ok(value),
                Ok(Err(error)) => Err(anyhow!(error)),
                Err(_) => Err(anyhow!("the extension host stopped before answering")),
            }
        }
    }

    /// Answers a [`HostEvent::Call`].
    pub fn reply(&self, id: u64, result: Result<Value, String>) -> Result<()> {
        let (result, error) = match result {
            Ok(value) => (value, None),
            Err(error) => (Value::Null, Some(error)),
        };
        self.send(&Message::Reply { id, result, error })
    }

    /// Makes an unpacked extension known to the host, without activating it.
    /// `storage` is the folder for its global and workspace storage.
    pub fn load_extension(
        &self,
        extension: &InstalledExtension,
        storage: &Path,
    ) -> impl Future<Output = Result<()>> + use<> {
        let prepared = (|| -> Result<Value> {
            let root = fs::canonicalize(&extension.root)
                .with_context(|| format!("cannot find {}", extension.root.display()))?;
            self.readable_roots.lock().unwrap().push(root.clone());
            Ok(json!({
                "id": extension.id(),
                "root": root,
                "main": extension.manifest.main,
                "packageJson": extension.manifest.raw(),
                "storagePath": storage,
                "configurationDefaults": extension.manifest.configuration_defaults(),
            }))
        })();
        let call = prepared.map(|params| self.call("extensions/load", params));
        async move {
            call?.await?;
            Ok(())
        }
    }

    /// Runs the extension's `activate` function.
    pub fn activate(&self, id: &str) -> impl Future<Output = Result<()>> + use<> {
        let call = self.call("extensions/activate", json!({ "id": id }));
        async move { call.await.map(|_| ()) }
    }

    /// Runs a command registered by an extension, activating the extension
    /// first if one declares `onCommand:` for it.
    pub fn execute_command(
        &self,
        command: &str,
        args: Vec<Value>,
    ) -> impl Future<Output = Result<Value>> + use<> {
        self.call(
            "commands/execute",
            json!({ "command": command, "args": args }),
        )
    }
}

impl ExtensionHost {
    /// Asks the host to answer at once. A host stuck in an extension's code
    /// never answers.
    pub fn ping(&self) -> impl Future<Output = Result<()>> + use<> {
        let call = self.call("host/ping", Value::Null);
        async move { call.await.map(|_| ()) }
    }

    /// What the oldest unanswered calls asked, such as
    /// `commands/execute vsauto.start`: the likely cause of a stuck host.
    pub fn waiting_for(&self) -> Vec<String> {
        let pending = self.pending.lock().unwrap();
        let mut calls: Vec<(&u64, &String)> = pending
            .iter()
            .filter(|(_, (_, subject))| subject != "host/ping")
            .map(|(id, (_, subject))| (id, subject))
            .collect();
        calls.sort();
        calls
            .into_iter()
            .map(|(_, subject)| subject.clone())
            .collect()
    }

    /// Whether the host's thread is still running.
    pub fn is_running(&self) -> bool {
        matches!(
            self.finished.try_recv(),
            Err(std::sync::mpsc::TryRecvError::Empty)
        )
    }
}

impl Drop for ExtensionHost {
    fn drop(&mut self) {
        if let Some(shutdown) = self.shutdown.take() {
            let _ = shutdown.send(());
        }
        // JavaScript busy in a loop never sees the shutdown: stop it.
        self.isolate.terminate_execution();
        let Some(thread) = self.thread.take() else {
            return;
        };
        match self.finished.recv_timeout(STOP_WAIT) {
            Ok(()) | Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                let _ = thread.join();
            }
            // Still stuck: leave the thread rather than freeze Keel.
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
        }
    }
}

// ---------------------------------------------------------------------------
// Operations JavaScript can call

#[op2(fast)]
fn op_keel_post(state: &mut OpState, #[string] message: String) -> Result<(), JsErrorBox> {
    let channels = state.borrow::<HostChannels>();
    let message: Message = serde_json::from_str(&message)
        .map_err(|error| JsErrorBox::type_error(error.to_string()))?;
    match message {
        Message::Reply { id, result, error } => {
            if let Some((answer, _)) = channels.pending.lock().unwrap().remove(&id) {
                let _ = answer.send(match error {
                    Some(error) => Err(error),
                    None => Ok(result),
                });
            }
        }
        Message::Notify { method, params } => {
            let _ = channels
                .events
                .unbounded_send(HostEvent::Notify { method, params });
        }
        Message::Call { id, method, params } => {
            let _ = channels
                .events
                .unbounded_send(HostEvent::Call { id, method, params });
        }
    }
    Ok(())
}

#[op2]
#[string]
async fn op_keel_next(state: Rc<RefCell<OpState>>) -> Option<String> {
    let inbox = state.borrow().borrow::<HostChannels>().inbox.clone();
    let mut inbox = inbox.lock().await;
    inbox.recv().await
}

/// Reads a module source, but only from inside a loaded extension.
#[op2]
#[string]
fn op_keel_read_module(state: &mut OpState, #[string] path: String) -> Option<String> {
    let channels = state.borrow::<HostChannels>();
    let file = fs::canonicalize(path).ok()?;
    let allowed = channels
        .readable_roots
        .lock()
        .unwrap()
        .iter()
        .any(|root| file.starts_with(root));
    if !allowed || !file.is_file() {
        return None;
    }
    fs::read_to_string(file).ok()
}

#[op2]
#[string]
fn op_keel_sha256(#[string] text: String) -> String {
    Sha256::digest(text.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// Turns an I/O error into the codes `vscode.FileSystemError` uses.
fn fs_error(error: io::Error) -> JsErrorBox {
    let code = match error.kind() {
        io::ErrorKind::NotFound => "FileNotFound",
        io::ErrorKind::AlreadyExists => "FileExists",
        io::ErrorKind::PermissionDenied => "NoPermissions",
        io::ErrorKind::NotADirectory => "FileNotADirectory",
        io::ErrorKind::IsADirectory => "FileIsADirectory",
        _ => "Unknown",
    };
    JsErrorBox::generic(format!("{code}: {error}"))
}

#[op2]
#[buffer]
fn op_keel_fs_read(#[string] path: String) -> Result<Vec<u8>, JsErrorBox> {
    fs::read(path).map_err(fs_error)
}

#[op2(fast)]
fn op_keel_fs_write(#[string] path: String, #[buffer] data: &[u8]) -> Result<(), JsErrorBox> {
    let path = PathBuf::from(path);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(fs_error)?;
    }
    fs::write(path, data).map_err(fs_error)
}

#[derive(Serialize)]
struct FileStat {
    #[serde(rename = "type")]
    kind: u32,
    ctime: f64,
    mtime: f64,
    size: u64,
}

fn file_type(kind: fs::FileType) -> u32 {
    // Values of `vscode.FileType`.
    if kind.is_symlink() {
        64
    } else if kind.is_dir() {
        2
    } else if kind.is_file() {
        1
    } else {
        0
    }
}

fn milliseconds(time: io::Result<std::time::SystemTime>) -> f64 {
    time.ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0.0, |duration| duration.as_millis() as f64)
}

#[op2]
#[serde]
fn op_keel_fs_stat(#[string] path: String) -> Result<FileStat, JsErrorBox> {
    let metadata = fs::metadata(path).map_err(fs_error)?;
    Ok(FileStat {
        kind: file_type(metadata.file_type()),
        ctime: milliseconds(metadata.created()),
        mtime: milliseconds(metadata.modified()),
        size: metadata.len(),
    })
}

#[op2]
#[serde]
fn op_keel_fs_read_directory(#[string] path: String) -> Result<Vec<(String, u32)>, JsErrorBox> {
    let mut entries = Vec::new();
    for entry in fs::read_dir(path).map_err(fs_error)? {
        let entry = entry.map_err(fs_error)?;
        let kind = entry.file_type().map_err(fs_error)?;
        entries.push((
            entry.file_name().to_string_lossy().into_owned(),
            file_type(kind),
        ));
    }
    Ok(entries)
}

#[op2(fast)]
fn op_keel_fs_create_directory(#[string] path: String) -> Result<(), JsErrorBox> {
    fs::create_dir_all(path).map_err(fs_error)
}

#[op2(fast)]
fn op_keel_fs_delete(#[string] path: String, recursive: bool) -> Result<(), JsErrorBox> {
    let metadata = fs::symlink_metadata(&path).map_err(fs_error)?;
    if metadata.is_dir() {
        if recursive {
            fs::remove_dir_all(path).map_err(fs_error)
        } else {
            fs::remove_dir(path).map_err(fs_error)
        }
    } else {
        fs::remove_file(path).map_err(fs_error)
    }
}

#[op2(fast)]
fn op_keel_fs_rename(
    #[string] from: String,
    #[string] to: String,
    overwrite: bool,
) -> Result<(), JsErrorBox> {
    if !overwrite && Path::new(&to).exists() {
        return Err(fs_error(io::Error::from(io::ErrorKind::AlreadyExists)));
    }
    fs::rename(from, to).map_err(fs_error)
}

extension!(
    keel_host,
    ops = [
        op_keel_post,
        op_keel_next,
        op_keel_read_module,
        op_keel_sha256,
        op_keel_fs_read,
        op_keel_fs_write,
        op_keel_fs_stat,
        op_keel_fs_read_directory,
        op_keel_fs_create_directory,
        op_keel_fs_delete,
        op_keel_fs_rename,
    ],
);
