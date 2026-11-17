//! jdtls and Spring Boot's server together on a real Spring Boot project,
//! with this test passing their questions on as Keel does. Slow and needs
//! the network, so it only runs when asked:
//!
//! ```text
//! KEEL_TEST_SERVERS_DIR=/tmp/servers KEEL_TEST_PROJECT=/tmp/spring-maven \
//!   cargo test -p lsp --test real_spring -- --ignored --nocapture
//! ```
//! `KEEL_TEST_SETTINGS` is JSON for the user settings, such as a Maven
//! `userSettings` file with its own repository.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

use futures::executor::block_on;
use lsp::servers::{self, spec};
use lsp::{LanguageServer, ResponseError, ServerEvent};
use serde_json::{Value, json};

/// Starts a server, installing it first if needed.
fn start(
    id: &str,
    servers_dir: &Path,
    root: &Path,
    user: &Value,
) -> (Arc<LanguageServer>, mpsc::Receiver<ServerEvent>) {
    let spec = spec(id).unwrap();
    let env = servers::login_environment();
    let program = match servers::find(spec, servers_dir, root, env) {
        Some(program) => program,
        None => {
            let started = Instant::now();
            servers::install(spec, servers_dir, root, env).unwrap();
            println!("[{id}] installed in {:?}", started.elapsed());
            servers::find(spec, servers_dir, root, env).unwrap()
        }
    };
    let extra = lsp::prerequisites::check_all(spec.needs, env).unwrap();
    let command = servers::command(spec, program, root, env, &extra);
    let (sender, events) = mpsc::channel();
    let sender = Mutex::new(sender);
    let server = LanguageServer::start(spec.name, &command, move |event| {
        let _ = sender.lock().unwrap().send(event);
    })
    .unwrap();
    server.set_settings(servers::settings_for(spec, user, root));
    let started = Instant::now();
    block_on(server.initialize(
        root,
        servers::initialization_options(spec, root, user, servers_dir),
    ))
    .unwrap();
    println!("[{id}] initialized in {:?}", started.elapsed());
    server.notify_raw(
        "workspace/didChangeConfiguration",
        json!({ "settings": servers::settings_for(spec, user, root) }),
    );
    (Arc::new(server), events)
}

#[test]
#[ignore]
fn spring_boot_completes_properties_with_jdtls() {
    let servers_dir =
        PathBuf::from(std::env::var("KEEL_TEST_SERVERS_DIR").expect("KEEL_TEST_SERVERS_DIR"));
    let root = PathBuf::from(std::env::var("KEEL_TEST_PROJECT").expect("KEEL_TEST_PROJECT"));
    let user: Value = std::env::var("KEEL_TEST_SETTINGS")
        .map(|text| serde_json::from_str(&text).unwrap())
        .unwrap_or_else(|_| json!({}));

    // Spring Boot first: jdtls loads its parts when it starts.
    let (spring, spring_events) = start("spring-boot", &servers_dir, &root, &user);
    let (jdtls, jdtls_events) = start("jdtls", &servers_dir, &root, &user);

    // Spring Boot asks jdtls through the editor.
    let relayed = Arc::new(Mutex::new(Vec::<String>::new()));
    {
        let (spring, jdtls, relayed) = (spring.clone(), jdtls.clone(), relayed.clone());
        std::thread::spawn(move || {
            while let Ok(event) = spring_events.recv() {
                match event {
                    ServerEvent::Request { id, method, params } => {
                        let answer = match servers::spring_boot_relay(&method, &params) {
                            Some((command, arguments)) => block_on(jdtls.request_raw(
                                "workspace/executeCommand",
                                json!({ "command": command, "arguments": arguments }),
                            ))
                            .map_err(|error| ResponseError::new(-32603, format!("{error:#}"))),
                            None => Ok(Value::Null),
                        };
                        relayed.lock().unwrap().push(method);
                        spring.respond(id, answer);
                    }
                    ServerEvent::Stderr(line) if std::env::var("KEEL_TEST_VERBOSE").is_ok() => {
                        println!("    [spring] {line}");
                    }
                    ServerEvent::Exited(code) => panic!("Spring Boot exited with {code:?}"),
                    _ => {}
                }
            }
        });
    }
    // jdtls calls back Spring Boot when the classpath changes.
    {
        let (spring, jdtls, relayed) = (spring.clone(), jdtls.clone(), relayed.clone());
        std::thread::spawn(move || {
            while let Ok(event) = jdtls_events.recv() {
                match event {
                    ServerEvent::Request { id, method, params }
                        if method == "workspace/executeClientCommand" =>
                    {
                        relayed
                            .lock()
                            .unwrap()
                            .push(format!("callback {}", params["command"]));
                        // Commands of VS Code's extensions themselves.
                        if params["command"] == "vscode-spring-boot.ls.start"
                            || params["command"] == "_java.reloadBundles.command"
                        {
                            jdtls.respond(id, Ok(Value::Null));
                            continue;
                        }
                        let answer = block_on(spring.request_raw(
                            "workspace/executeCommand",
                            json!({ "command": params["command"], "arguments": params["arguments"] }),
                        ))
                        .map_err(|error| ResponseError::new(-32603, format!("{error:#}")));
                        jdtls.respond(id, answer);
                    }
                    ServerEvent::Request { id, .. } => jdtls.respond(id, Ok(Value::Null)),
                    ServerEvent::Notification { method, params }
                        if method == "language/status"
                            && std::env::var("KEEL_TEST_VERBOSE").is_ok() =>
                    {
                        println!("    [jdtls] {}", params["message"]);
                    }
                    _ => {}
                }
            }
        });
    }

    // What VS Code's Spring Boot extension does once jdtls runs in full.
    let enable = block_on(spring.request_raw(
        "workspace/executeCommand",
        json!({ "command": "sts.vscode-spring-boot.enableClasspathListening", "arguments": [true] }),
    ));
    println!("enableClasspathListening: {enable:?}");
    let path = root.join("src/main/resources/application.properties");
    let text = std::fs::read_to_string(&path).unwrap() + "server.po";
    let uri = workbench::uri::Uri::file(&path).to_string();
    spring.notify_raw(
        "textDocument/didOpen",
        json!({ "textDocument": { "uri": uri, "languageId": "spring-boot-properties", "version": 1, "text": text } }),
    );
    let line = text.lines().count() - 1;
    let character = text.lines().last().unwrap().len();
    let started = Instant::now();
    let deadline = started + Duration::from_secs(300);
    loop {
        let answer = block_on(spring.request_raw(
            "textDocument/completion",
            json!({ "textDocument": { "uri": uri }, "position": { "line": line, "character": character } }),
        ))
        .unwrap();
        let items = answer["items"]
            .as_array()
            .or(answer.as_array())
            .cloned()
            .unwrap_or_default();
        let labels: Vec<&str> = items
            .iter()
            .filter_map(|item| item["label"].as_str())
            .collect();
        if labels.iter().any(|label| label.starts_with("server.port")) {
            println!(
                "completion with server.port after {:?}: {} items, first {:?}",
                started.elapsed(),
                labels.len(),
                &labels[..labels.len().min(5)]
            );
            break;
        }
        assert!(
            Instant::now() < deadline,
            "no server.port in {labels:?}; relayed {:?}",
            relayed.lock().unwrap()
        );
        std::thread::sleep(Duration::from_secs(2));
    }
    // Warm, as when typing the next letter.
    let warm = Instant::now();
    block_on(spring.request_raw(
        "textDocument/completion",
        json!({ "textDocument": { "uri": uri }, "position": { "line": line, "character": character } }),
    ))
    .unwrap();
    println!("completion again in {:?}", warm.elapsed());
    let mut relayed: Vec<String> = relayed.lock().unwrap().clone();
    relayed.sort();
    relayed.dedup();
    println!("relayed: {relayed:?}");
}
