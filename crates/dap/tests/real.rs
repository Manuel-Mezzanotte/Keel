//! The real adapters, when they are on this Mac:
//! `cargo test -p dap --test real -- --ignored`.

use std::path::Path;
use std::process::Command;
use std::sync::{Arc, mpsc};
use std::time::Duration;

use dap::adapters;
use dap::client::AdapterCommand;
use dap::{Client, ClientEvent};
use futures::executor::block_on;
use serde_json::{Value, json};

struct Session {
    client: Arc<Client>,
    events: mpsc::Receiver<ClientEvent>,
}

impl Session {
    fn start(command: &AdapterCommand) -> Self {
        let (sender, events) = mpsc::channel();
        let client = Client::spawn(
            command,
            Arc::new(move |event| {
                let _ = sender.send(event);
            }),
        )
        .unwrap();
        Self { client, events }
    }

    fn ask(&self, command: &str, arguments: Value) -> Value {
        block_on(self.client.request(command, arguments))
            .unwrap_or_else(|error| panic!("{command}: {error}"))
    }

    fn wait(&self, name: &str) -> Value {
        loop {
            match self.events.recv_timeout(Duration::from_secs(90)) {
                Ok(ClientEvent::Event { event, body }) if event == name => return body,
                Ok(event) => eprintln!("{event:?}"),
                Err(error) => panic!("no {name}: {error}"),
            }
        }
    }

    /// Runs `launch` to the breakpoint at `line`; the top frame's locals.
    fn stop_at(
        &self,
        adapter: &str,
        launch: Value,
        source: &Path,
        line: u32,
    ) -> Vec<(String, String)> {
        let capabilities = self.ask(
            "initialize",
            json!({ "clientID": "keel", "adapterID": adapter, "linesStartAt1": true, "columnsStartAt1": true, "pathFormat": "path", "supportsRunInTerminalRequest": false }),
        );
        eprintln!("{capabilities}");
        let launched = self.client.request("launch", launch);
        self.wait("initialized");
        let set = self.ask(
            "setBreakpoints",
            json!({ "source": { "path": source }, "breakpoints": [{ "line": line }] }),
        );
        eprintln!("{set}");
        self.ask("configurationDone", json!({}));
        block_on(launched).unwrap();
        let stopped = self.wait("stopped");
        eprintln!("{stopped}");
        let frames = self.ask(
            "stackTrace",
            json!({ "threadId": stopped["threadId"], "startFrame": 0, "levels": 20 }),
        );
        let frame = &frames["stackFrames"][0];
        eprintln!("{frame}");
        assert_eq!(frame["line"], line);
        let scopes = self.ask("scopes", json!({ "frameId": frame["id"] }));
        eprintln!("{scopes}");
        let variables = self.ask(
            "variables",
            json!({ "variablesReference": scopes["scopes"][0]["variablesReference"] }),
        );
        variables["variables"]
            .as_array()
            .unwrap()
            .iter()
            .map(|variable| {
                (
                    variable["name"].as_str().unwrap().to_owned(),
                    variable["value"].as_str().unwrap().to_owned(),
                )
            })
            .collect()
    }
}

#[test]
#[ignore = "needs lldb-dap and rustc"]
fn lldb_stops_in_a_rust_program() {
    let env = lsp::servers::login_environment();
    let folder = tempfile::tempdir().unwrap();
    let root = std::fs::canonicalize(folder.path()).unwrap();
    let source = root.join("prova.rs");
    std::fs::write(
        &source,
        "fn main() {\n    let nomi = vec![\"uno\".to_string(), \"due\".to_string()];\n    let totale = nomi.len();\n    println!(\"{totale} {nomi:?}\");\n}\n",
    )
    .unwrap();
    let status = Command::new("rustc")
        .args(["-g", "-o", "prova", "prova.rs"])
        .current_dir(&root)
        .status()
        .unwrap();
    assert!(status.success());
    let command = adapters::find_lldb(env).unwrap();
    let session = Session::start(&command);
    let locals = session.stop_at(
        "lldb-dap",
        json!({
            "name": "prova", "type": "lldb-dap", "request": "launch",
            "program": root.join("prova"), "cwd": root,
            "initCommands": adapters::rust_formatters(&root, env),
        }),
        &source,
        4,
    );
    eprintln!("{locals:?}");
    let value = |name: &str| {
        locals
            .iter()
            .find(|(known, _)| known == name)
            .unwrap()
            .1
            .clone()
    };
    assert_eq!(value("totale"), "2");
    assert!(value("nomi").contains("size=2"), "{}", value("nomi"));
    // The line can have more than one place to stop at.
    let code = 'ran: loop {
        session.ask("continue", json!({ "threadId": 1 }));
        loop {
            match session.events.recv_timeout(Duration::from_secs(20)) {
                Ok(ClientEvent::Event { event, body }) if event == "exited" => break 'ran body,
                Ok(ClientEvent::Event { event, .. }) if event == "stopped" => break,
                Ok(_) => {}
                Err(error) => panic!("no exited: {error}"),
            }
        }
    };
    assert_eq!(code["exitCode"], 0);
    session.ask("disconnect", json!({ "terminateDebuggee": true }));
}

#[test]
#[ignore = "needs python3 and debugpy"]
fn debugpy_stops_in_a_python_script() {
    let env = lsp::servers::login_environment();
    let folder = tempfile::tempdir().unwrap();
    let root = std::fs::canonicalize(folder.path()).unwrap();
    let source = root.join("prova.py");
    std::fs::write(
        &source,
        "nomi = ['uno', 'due']\ntotale = len(nomi)\nprint(totale)\n",
    )
    .unwrap();
    let python = adapters::python_of(&root, env).unwrap();
    let home = std::env::home_dir();
    let command =
        adapters::find_debugpy(&python, &root.join("adapters"), home.as_deref(), env).unwrap();
    eprintln!("{command:?}");
    let session = Session::start(&command);
    let locals = session.stop_at(
        "debugpy",
        json!({
            "name": "prova", "type": "debugpy", "request": "launch",
            "program": source, "cwd": root, "console": "internalConsole",
            "python": [python], "justMyCode": true,
        }),
        &source,
        3,
    );
    eprintln!("{locals:?}");
    assert!(locals.contains(&("totale".into(), "2".into())));
    session.ask("continue", json!({ "threadId": 1 }));
    assert_eq!(session.wait("exited")["exitCode"], 0);
    session.ask("disconnect", json!({ "terminateDebuggee": true }));
}
