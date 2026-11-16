//! A tiny language server for the tests, in Keel and in this crate.
//!
//! It keeps the text of open documents, reports a warning for every `TODO`,
//! answers hovers with the document's length, asks for its `fake` settings
//! after starting, and has two extra methods: `fake/slow`, which only ends
//! when cancelled, and `fake/crash`, which stops the server.
//!
//! Completions offer `console`, a `log($1)$0` snippet and `useState`, whose
//! automatic import comes with `completionItem/resolve`. The signature is
//! always `saluta(nome: string, volte: number)`, with the parameter counted
//! from the commas before the cursor.

use std::collections::HashMap;
use std::io::{self, BufReader, Read, Write};
use std::sync::{Arc, Mutex};

use serde_json::{Value, json};

use crate::rpc::{read_message, write_message};
use crate::text::apply_change;

/// Serves one client until `exit`, `fake/crash` or the end of the input.
/// Returns the exit code the server's process should use.
pub fn run(input: impl Read, output: impl Write + Send + 'static) -> i32 {
    let output = Arc::new(Mutex::new(output));
    let send = |message: Value| {
        let _ = write_message(&mut *output.lock().unwrap(), &message);
    };
    let mut documents: HashMap<String, String> = HashMap::new();
    let mut slow: Vec<Value> = Vec::new();
    let mut reader = BufReader::new(input);
    let mut next_id = 1000;
    while let Ok(Some(message)) = read_message(&mut reader) {
        let method = message["method"].as_str().unwrap_or_default();
        let id = message.get("id").cloned();
        let params = &message["params"];
        match method {
            "initialize" => send(json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": {
                    "capabilities": {
                        "textDocumentSync": { "openClose": true, "change": 2, "save": true },
                        "hoverProvider": true,
                        "completionProvider": { "triggerCharacters": ["."], "resolveProvider": true },
                        "signatureHelpProvider": { "triggerCharacters": ["(", ","] },
                    },
                    "serverInfo": { "name": "fake", "version": "1" },
                },
            })),
            "initialized" => {
                send(json!({
                    "jsonrpc": "2.0",
                    "id": next_id,
                    "method": "workspace/configuration",
                    "params": { "items": [{ "section": "fake" }] },
                }));
                next_id += 1;
            }
            "textDocument/didOpen" => {
                let uri = params["textDocument"]["uri"].as_str().unwrap_or_default();
                let text = params["textDocument"]["text"].as_str().unwrap_or_default();
                documents.insert(uri.to_owned(), text.to_owned());
                send(diagnostics(uri, text, &params["textDocument"]["version"]));
            }
            "textDocument/didChange" => {
                let uri = params["textDocument"]["uri"].as_str().unwrap_or_default();
                let Some(text) = documents.get_mut(uri) else {
                    continue;
                };
                for change in params["contentChanges"].as_array().into_iter().flatten() {
                    if let Ok(change) = serde_json::from_value(change.clone()) {
                        *text = apply_change(text, &change);
                    }
                }
                send(diagnostics(uri, text, &params["textDocument"]["version"]));
            }
            "textDocument/didSave" => send(json!({
                "jsonrpc": "2.0",
                "method": "window/logMessage",
                "params": { "type": 4, "message": format!("saved {}", params["textDocument"]["uri"].as_str().unwrap_or_default()) },
            })),
            "textDocument/didClose" => {
                let uri = params["textDocument"]["uri"].as_str().unwrap_or_default();
                documents.remove(uri);
                send(json!({
                    "jsonrpc": "2.0",
                    "method": "textDocument/publishDiagnostics",
                    "params": { "uri": uri, "diagnostics": [] },
                }));
            }
            "textDocument/hover" => {
                let uri = params["textDocument"]["uri"].as_str().unwrap_or_default();
                let length = documents.get(uri).map_or(0, String::len);
                send(json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": { "contents": { "kind": "markdown", "value": format!("{length} bytes") } },
                }));
            }
            "textDocument/completion" => send(json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": { "isIncomplete": false, "items": [
                    { "label": "console", "kind": 6 },
                    { "label": "log", "kind": 2, "insertText": "log($1)$0", "insertTextFormat": 2 },
                    { "label": "useState", "kind": 3, "detail": "Auto import from 'react'", "data": { "import": "react" } },
                ] },
            })),
            "completionItem/resolve" => {
                let mut item = params.clone();
                if let Some(module) = params["data"]["import"].as_str() {
                    item["additionalTextEdits"] = json!([{
                        "range": { "start": { "line": 0, "character": 0 }, "end": { "line": 0, "character": 0 } },
                        "newText": format!("import {{ {} }} from '{module}';\n", params["label"].as_str().unwrap_or_default()),
                    }]);
                }
                send(json!({ "jsonrpc": "2.0", "id": id, "result": item }));
            }
            "textDocument/signatureHelp" => {
                let uri = params["textDocument"]["uri"].as_str().unwrap_or_default();
                let position = &params["position"];
                let text = documents.get(uri).cloned().unwrap_or_default();
                let offset = workbench::position::offset_of(
                    &text,
                    workbench::position::Position::new(
                        position["line"].as_u64().unwrap_or_default() as u32,
                        position["character"].as_u64().unwrap_or_default() as u32,
                    ),
                );
                let before = &text[..offset];
                let result = match before.rfind('(') {
                    Some(open) if !before[open..].contains(')') => json!({
                        "signatures": [{
                            "label": "saluta(nome: string, volte: number)",
                            "parameters": [{ "label": "nome: string" }, { "label": "volte: number" }],
                        }],
                        "activeSignature": 0,
                        "activeParameter": before[open..].matches(',').count(),
                    }),
                    _ => Value::Null,
                };
                send(json!({ "jsonrpc": "2.0", "id": id, "result": result }));
            }
            "fake/slow" => slow.extend(id),
            "$/cancelRequest" => {
                let cancelled = params["id"].clone();
                if let Some(index) = slow.iter().position(|id| *id == cancelled) {
                    slow.remove(index);
                    send(json!({
                        "jsonrpc": "2.0",
                        "id": cancelled,
                        "error": { "code": -32800, "message": "cancelled" },
                    }));
                    send(json!({
                        "jsonrpc": "2.0",
                        "method": "window/logMessage",
                        "params": { "type": 3, "message": format!("cancelled {cancelled}") },
                    }));
                }
            }
            "fake/crash" => return 3,
            "shutdown" => send(json!({ "jsonrpc": "2.0", "id": id, "result": null })),
            "exit" => return 0,
            // The answer to `workspace/configuration`.
            "" if id.is_some() => send(json!({
                "jsonrpc": "2.0",
                "method": "window/logMessage",
                "params": { "type": 3, "message": format!("settings {}", message["result"]) },
            })),
            _ => {
                if let Some(id) = id {
                    send(json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "error": { "code": -32601, "message": format!("unknown method {method}") },
                    }));
                }
            }
        }
    }
    0
}

fn diagnostics(uri: &str, text: &str, version: &Value) -> Value {
    let found: Vec<Value> = text
        .match_indices("TODO")
        .map(|(offset, _)| {
            let start = workbench::position::position_of(text, offset);
            json!({
                "range": {
                    "start": { "line": start.line, "character": start.character },
                    "end": { "line": start.line, "character": start.character + 4 },
                },
                "severity": 2,
                "source": "fake",
                "message": "todo found",
            })
        })
        .collect();
    json!({
        "jsonrpc": "2.0",
        "method": "textDocument/publishDiagnostics",
        "params": { "uri": uri, "version": version, "diagnostics": found },
    })
}

/// Runs the fake server in a thread and returns the two ends a
/// [`crate::LanguageServer`] connects to: what the server writes, and where
/// to write to it.
pub fn spawn() -> io::Result<(io::PipeReader, io::PipeWriter)> {
    let (client_reads, server_writes) = io::pipe()?;
    let (server_reads, client_writes) = io::pipe()?;
    std::thread::Builder::new()
        .name("fake language server".to_owned())
        .spawn(move || run(server_reads, server_writes))?;
    Ok((client_reads, client_writes))
}
