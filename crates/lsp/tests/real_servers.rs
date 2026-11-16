//! Real servers, installed into a scratch folder: slow and needs the
//! network, so it only runs when asked.
//!
//! ```text
//! KEEL_TEST_SERVERS_DIR=/tmp/servers KEEL_TEST_SERVERS=vtsls,yaml \
//!   cargo test -p lsp --test real_servers -- --ignored --nocapture
//! ```
//! `KEEL_TEST_PROJECT` picks the project and `KEEL_TEST_FILE` the file to
//! open in it; otherwise a small sample is written for each server.

use std::path::{Path, PathBuf};
use std::sync::{Mutex, mpsc};
use std::time::{Duration, Instant};

use futures::executor::block_on;
use lsp::lsp_types::{
    DidOpenTextDocumentParams, TextDocumentItem, notification::DidOpenTextDocument,
};
use lsp::servers::{self, spec};
use lsp::{LanguageServer, ServerEvent};
use serde_json::{Value, json};

fn sample(id: &str, dir: &Path) -> (PathBuf, &'static str) {
    let (name, language, text) = match id {
        "vtsls" | "eslint" | "emmet" => ("app.ts", "typescript", "const total: number = 'uno';\n"),
        "html" => ("index.html", "html", "<div><span></div>\n"),
        "css" => ("style.css", "css", ".a { colr: red; }\n"),
        "json" => ("data.json", "json", "{ \"a\": 1, }\n"),
        "yaml" => ("config.yaml", "yaml", "a: [1, 2\n"),
        "taplo" => ("Cargo.toml", "toml", "[package\nname = 1\n"),
        "marksman" => ("README.md", "markdown", "# Titolo\n\n[link](#manca)\n"),
        "prisma" => (
            "schema.prisma",
            "prisma",
            "model User {\n  id Int @id\n  name Strin\n}\n",
        ),
        "rust-analyzer" => (
            "src/main.rs",
            "rust",
            "fn main() { let x: u32 = \"no\"; }\n",
        ),
        other => panic!("no sample for {other}"),
    };
    if id == "rust-analyzer" {
        std::fs::write(
            dir.join("Cargo.toml"),
            "[package]\nname = \"prova\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        )
        .unwrap();
    }
    let path = dir.join(name);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, text).unwrap();
    (path, language)
}

#[test]
#[ignore]
fn real_servers_start_and_report() {
    let servers_dir =
        PathBuf::from(std::env::var("KEEL_TEST_SERVERS_DIR").expect("KEEL_TEST_SERVERS_DIR"));
    let wanted = std::env::var("KEEL_TEST_SERVERS").unwrap_or_else(|_| "vtsls".to_owned());
    let env = servers::login_environment();
    for id in wanted.split(',') {
        let spec = spec(id).unwrap_or_else(|| panic!("unknown server {id}"));
        let scratch = tempfile::tempdir().unwrap();
        let (root, path, language) = match std::env::var("KEEL_TEST_PROJECT") {
            Ok(project) => {
                let root = PathBuf::from(project);
                let path = root.join(std::env::var("KEEL_TEST_FILE").expect("KEEL_TEST_FILE"));
                let language = lsp_language(&path);
                (root, path, language)
            }
            Err(_) => {
                let root = std::fs::canonicalize(scratch.path()).unwrap();
                let (path, language) = sample(id, &root);
                (root, path, language)
            }
        };
        let started = Instant::now();
        let program = match servers::find(spec, &servers_dir, &root, env) {
            Some(program) => program,
            None => {
                println!("[{id}] installing…");
                servers::install(spec, &servers_dir, &root, env).unwrap();
                servers::find(spec, &servers_dir, &root, env).expect("found after installing")
            }
        };
        let found = started.elapsed();
        let (sender, events) = mpsc::channel();
        let sender = Mutex::new(sender);
        let command = servers::command(spec, program.clone(), &root, env);
        let server = LanguageServer::start(spec.name, &command, move |event| {
            let _ = sender.lock().unwrap().send(event);
        })
        .unwrap();
        server.set_settings(servers::default_settings(spec));
        let start = Instant::now();
        block_on(server.initialize(&root, None)).unwrap();
        let initialized = start.elapsed();
        let text = std::fs::read_to_string(&path).unwrap();
        let uri = workbench::uri::Uri::file(&path).to_string();
        server.notify::<DidOpenTextDocument>(DidOpenTextDocumentParams {
            text_document: TextDocumentItem::new(uri.parse().unwrap(), language.into(), 1, text),
        });
        let deadline = Instant::now() + Duration::from_secs(120);
        let mut diagnostics: Option<Value> = None;
        while Instant::now() < deadline {
            match events.recv_timeout(Duration::from_millis(200)) {
                Ok(ServerEvent::Notification { method, params })
                    if method == "textDocument/publishDiagnostics" =>
                {
                    let empty = params["diagnostics"].as_array().is_none_or(Vec::is_empty);
                    if params["uri"] == json!(uri) || !empty {
                        diagnostics = Some(params);
                        if !empty {
                            break;
                        }
                    }
                }
                Ok(ServerEvent::Exited(code)) => panic!("[{id}] exited with {code:?}"),
                _ => {}
            }
        }
        let first = start.elapsed();
        println!(
            "[{id}] {} — found in {found:?}, initialized in {initialized:?}, diagnostics after {first:?}",
            program.display()
        );
        for diagnostic in diagnostics
            .as_ref()
            .and_then(|params| params["diagnostics"].as_array())
            .into_iter()
            .flatten()
            .take(5)
        {
            println!(
                "    {} {}",
                diagnostic["range"]["start"], diagnostic["message"]
            );
        }
        block_on(server.shutdown());
    }
}

fn lsp_language(path: &Path) -> &'static str {
    match path.extension().and_then(|extension| extension.to_str()) {
        Some("ts") => "typescript",
        Some("tsx") => "typescriptreact",
        Some("rs") => "rust",
        _ => "plaintext",
    }
}
