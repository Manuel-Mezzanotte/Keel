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
//!
//! A definition is the `function` of that name in any open document.
//! `fake/applyEdit` makes the server ask the editor to insert its `text`
//! at the start of its `uri`; `fake/clientCommand` makes it ask the editor
//! to run its `command` with its `arguments`, as jdtls does.
//!
//! `fake/registerPull` registers diagnostics to ask for, as csharp-ls does;
//! each `textDocument/diagnostic` then answers "pulled N", counting the
//! requests. `fake/progressEnd` ends a piece of work (`$/progress`).
//!
//! Each `TODO` has a quick fix, "Replace with DONE", whose edit comes with
//! `codeAction/resolve`; "Organize" (`source.organizeImports`) adds a
//! `// organized` line at the top, and formatting a `// formatted` one.

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
    let mut pulls = 0;
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
                        "definitionProvider": true,
                        "codeActionProvider": { "resolveProvider": true },
                        "documentFormattingProvider": true,
                        "executeCommandProvider": { "commands": ["fake.command"] },
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
            "textDocument/definition" => {
                let uri = params["textDocument"]["uri"].as_str().unwrap_or_default();
                let text = documents.get(uri).cloned().unwrap_or_default();
                let offset = offset_at(&text, &params["position"]);
                let word = word_at(&text, offset);
                let wanted = format!("function {word}");
                let found = documents.iter().find_map(|(uri, text)| {
                    let at = text.find(&wanted)? + "function ".len();
                    let start = workbench::position::position_of(text, at);
                    let end = workbench::position::position_of(text, at + word.len());
                    Some(json!({
                        "uri": uri,
                        "range": {
                            "start": { "line": start.line, "character": start.character },
                            "end": { "line": end.line, "character": end.character },
                        },
                    }))
                });
                send(json!({ "jsonrpc": "2.0", "id": id, "result": found }));
            }
            "textDocument/codeAction" => {
                let uri = params["textDocument"]["uri"].as_str().unwrap_or_default();
                let only: Vec<&str> = params["context"]["only"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_str)
                    .collect();
                let mut actions = Vec::new();
                if only.is_empty() || only.contains(&"source.organizeImports") {
                    actions.push(json!({
                        "title": "Organize",
                        "kind": "source.organizeImports",
                        "edit": { "changes": { uri: [top_line("// organized\n")] } },
                    }));
                }
                if only.is_empty() {
                    for diagnostic in params["context"]["diagnostics"]
                        .as_array()
                        .into_iter()
                        .flatten()
                    {
                        actions.push(json!({
                            "title": "Replace with DONE",
                            "kind": "quickfix",
                            "isPreferred": true,
                            "data": { "uri": uri, "range": diagnostic["range"] },
                        }));
                    }
                    actions.push(json!({
                        "title": "Run a command",
                        "command": "fake.command",
                        "arguments": [1],
                    }));
                }
                send(json!({ "jsonrpc": "2.0", "id": id, "result": actions }));
            }
            "codeAction/resolve" => {
                let mut action = params.clone();
                let data = &params["data"];
                if let Some(uri) = data["uri"].as_str() {
                    action["edit"] = json!({ "changes": { uri: [{ "range": data["range"], "newText": "DONE" }] } });
                }
                send(json!({ "jsonrpc": "2.0", "id": id, "result": action }));
            }
            "textDocument/formatting" => send(json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": [top_line("// formatted\n")],
            })),
            "workspace/executeCommand" => {
                send(json!({
                    "jsonrpc": "2.0",
                    "method": "window/logMessage",
                    "params": { "type": 3, "message": format!("executed {} {}", params["command"].as_str().unwrap_or_default(), params["arguments"]) },
                }));
                send(json!({ "jsonrpc": "2.0", "id": id, "result": null }));
            }
            "fake/applyEdit" => {
                send(json!({
                    "jsonrpc": "2.0",
                    "id": next_id,
                    "method": "workspace/applyEdit",
                    "params": { "edit": { "changes": { params["uri"].as_str().unwrap_or_default(): [{
                        "range": { "start": { "line": 0, "character": 0 }, "end": { "line": 0, "character": 0 } },
                        "newText": params["text"],
                    }] } } },
                }));
                next_id += 1;
            }
            "fake/clientCommand" => {
                send(json!({
                    "jsonrpc": "2.0",
                    "id": next_id,
                    "method": "workspace/executeClientCommand",
                    "params": { "command": params["command"], "arguments": params["arguments"] },
                }));
                next_id += 1;
            }
            "fake/registerPull" => {
                send(json!({
                    "jsonrpc": "2.0",
                    "id": next_id,
                    "method": "client/registerCapability",
                    "params": { "registrations": [{ "id": "pull", "method": "textDocument/diagnostic", "registerOptions": {} }] },
                }));
                next_id += 1;
            }
            "textDocument/diagnostic" => {
                pulls += 1;
                send(json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": { "kind": "full", "items": [{
                        "range": { "start": { "line": 0, "character": 0 }, "end": { "line": 0, "character": 1 } },
                        "severity": 2,
                        "message": format!("pulled {pulls}"),
                    }] },
                }));
            }
            "fake/progressEnd" => send(json!({
                "jsonrpc": "2.0",
                "method": "$/progress",
                "params": { "token": "load", "value": { "kind": "end" } },
            })),
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

/// An edit inserting `text` at the very start.
fn top_line(text: &str) -> Value {
    json!({
        "range": { "start": { "line": 0, "character": 0 }, "end": { "line": 0, "character": 0 } },
        "newText": text,
    })
}

fn offset_at(text: &str, position: &Value) -> usize {
    workbench::position::offset_of(
        text,
        workbench::position::Position::new(
            position["line"].as_u64().unwrap_or_default() as u32,
            position["character"].as_u64().unwrap_or_default() as u32,
        ),
    )
}

fn word_at(text: &str, offset: usize) -> &str {
    let is_word = |character: char| character.is_alphanumeric() || character == '_';
    let start = text[..offset]
        .char_indices()
        .rev()
        .take_while(|(_, character)| is_word(*character))
        .last()
        .map_or(offset, |(at, _)| at);
    let end = text[offset..]
        .char_indices()
        .find(|(_, character)| !is_word(*character))
        .map_or(text.len(), |(at, _)| offset + at);
    &text[start..end]
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
