//! The client against the fake server: in a thread, and as a real process.

use std::sync::{Arc, Mutex, mpsc};
use std::time::Duration;

use futures::executor::block_on;
use lsp::lsp_types::{
    DidChangeTextDocumentParams, DidOpenTextDocumentParams, Hover, HoverContents, HoverParams,
    MarkupContent, Position, TextDocumentIdentifier, TextDocumentItem, TextDocumentPositionParams,
    Uri, VersionedTextDocumentIdentifier,
    notification::{DidChangeTextDocument, DidOpenTextDocument},
    request::HoverRequest,
};
use lsp::{LanguageServer, ServerCommand, ServerEvent, text::content_change};
use serde_json::{Value, json};

const WAIT: Duration = Duration::from_secs(5);

fn events() -> (
    Arc<dyn Fn(ServerEvent) + Send + Sync>,
    mpsc::Receiver<ServerEvent>,
) {
    let (sender, receiver) = mpsc::channel();
    let sender = Mutex::new(sender);
    (
        Arc::new(move |event| {
            let _ = sender.lock().unwrap().send(event);
        }),
        receiver,
    )
}

/// The next notification with `method`, skipping the others.
fn next(receiver: &mpsc::Receiver<ServerEvent>, method: &str) -> Value {
    loop {
        match receiver.recv_timeout(WAIT).expect("an event in time") {
            ServerEvent::Notification {
                method: got,
                params,
            } if got == method => return params,
            _ => {}
        }
    }
}

fn in_thread() -> (LanguageServer, mpsc::Receiver<ServerEvent>) {
    let (reader, writer) = lsp::fake::spawn().unwrap();
    let (on_event, receiver) = events();
    let server = LanguageServer::connect("fake", reader, writer, on_event, None).unwrap();
    (server, receiver)
}

#[test]
fn initialize_open_change_and_hover() {
    let (server, events) = in_thread();
    server.set_settings(json!({ "fake": { "level": "all" } }));
    let root = tempfile::tempdir().unwrap();
    let result = block_on(server.initialize(root.path(), None)).unwrap();
    assert_eq!(result.server_info.unwrap().name, "fake");
    assert!(server.capabilities().unwrap().hover_provider.is_some());
    // The server asked for its settings, and Keel answered by itself.
    assert_eq!(
        next(&events, "window/logMessage")["message"],
        r#"settings [{"level":"all"}]"#
    );

    let uri: Uri = "file:///progetto/app.ts".parse().unwrap();
    let text = "const a = 1;\n";
    server.notify::<DidOpenTextDocument>(DidOpenTextDocumentParams {
        text_document: TextDocumentItem::new(uri.clone(), "typescript".into(), 1, text.into()),
    });
    let diagnostics = next(&events, "textDocument/publishDiagnostics");
    assert_eq!(diagnostics["diagnostics"], json!([]));

    let new_text = "const a = 1; // TODO: perché?\n";
    server.notify::<DidChangeTextDocument>(DidChangeTextDocumentParams {
        text_document: VersionedTextDocumentIdentifier::new(uri.clone(), 2),
        content_changes: vec![content_change(text, new_text).unwrap()],
    });
    let diagnostics = next(&events, "textDocument/publishDiagnostics");
    assert_eq!(diagnostics["version"], 2);
    assert_eq!(
        diagnostics["diagnostics"][0]["range"]["start"]["character"],
        16
    );

    let hover: Option<Hover> = block_on(server.request::<HoverRequest>(HoverParams {
        text_document_position_params: TextDocumentPositionParams::new(
            TextDocumentIdentifier::new(uri),
            Position::new(0, 0),
        ),
        work_done_progress_params: Default::default(),
    }))
    .unwrap();
    // The server rebuilt the text from the change alone.
    assert_eq!(
        hover.unwrap().contents,
        HoverContents::Markup(MarkupContent {
            kind: lsp::lsp_types::MarkupKind::Markdown,
            value: format!("{} bytes", new_text.len()),
        })
    );
}

#[test]
fn errors_and_unknown_methods() {
    let (server, _events) = in_thread();
    let error = block_on(server.request_raw("fake/unknown", Value::Null)).unwrap_err();
    let error = error.downcast::<lsp::ResponseError>().unwrap();
    assert_eq!(error.code, lsp::ResponseError::METHOD_NOT_FOUND);
}

#[test]
fn dropping_a_request_cancels_it() {
    let (server, events) = in_thread();
    let slow = server.request_raw("fake/slow", Value::Null);
    let id = slow.id();
    drop(slow);
    assert_eq!(
        next(&events, "window/logMessage")["message"],
        format!("cancelled {id}")
    );
    // The server still works.
    let error = block_on(server.request_raw("fake/unknown", Value::Null)).unwrap_err();
    assert!(error.to_string().contains("unknown method"));
}

#[test]
fn a_real_process_reports_its_stderr_and_its_exit() {
    let root = tempfile::tempdir().unwrap();
    let command = ServerCommand {
        program: env!("CARGO_BIN_EXE_keel-fake-lsp").into(),
        args: vec![],
        env: vec![],
        cwd: root.path().to_path_buf(),
    };
    let (sender, events) = mpsc::channel();
    let sender = Mutex::new(sender);
    let server = LanguageServer::start("fake", &command, move |event| {
        let _ = sender.lock().unwrap().send(event);
    })
    .unwrap();
    assert!(server.pid().is_some());
    block_on(server.initialize(root.path(), None)).unwrap();
    let mut saw_stderr = false;
    // The crash ends the process with code 3; waiting requests fail.
    let waiting = server.request_raw("fake/slow", Value::Null);
    server.notify_raw("fake/crash", Value::Null);
    loop {
        match events.recv_timeout(WAIT).expect("an event in time") {
            ServerEvent::Stderr(line) => saw_stderr |= line == "fake server ready",
            ServerEvent::Exited(code) => {
                assert_eq!(code, Some(3));
                break;
            }
            _ => {}
        }
    }
    assert!(saw_stderr);
    assert!(!server.is_running());
    let error = block_on(waiting).unwrap_err();
    assert!(error.to_string().contains("stopped"), "{error}");
}

#[test]
fn shutdown_ends_the_process() {
    let root = tempfile::tempdir().unwrap();
    let command = ServerCommand {
        program: env!("CARGO_BIN_EXE_keel-fake-lsp").into(),
        args: vec![],
        env: vec![],
        cwd: root.path().to_path_buf(),
    };
    let (sender, events) = mpsc::channel();
    let sender = Mutex::new(sender);
    let server = LanguageServer::start("fake", &command, move |event| {
        let _ = sender.lock().unwrap().send(event);
    })
    .unwrap();
    block_on(server.shutdown());
    loop {
        if let ServerEvent::Exited(code) = events.recv_timeout(WAIT).expect("exits in time") {
            assert_eq!(code, Some(0));
            break;
        }
    }
}
