//! What can be debugged: the configurations of `.vscode/launch.json`, read
//! as VS Code reads them, and the ones Keel finds itself (the open Python
//! or JavaScript file, a Rust program, an npm script). Here is only the
//! reading and what each adapter is asked; `debug.rs` runs them.

use std::path::{Path, PathBuf};

use dap::adapters::Adapter;
use serde_json::{Map, Value, json};

use super::task_config::{TaskFile, TaskInput, Variables};

/// A configuration to launch or attach with.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct DebugConfig {
    pub(super) name: String,
    pub(super) kind: String,
    pub(super) adapter: Adapter,
    /// `launch` or `attach`.
    pub(super) request: String,
    /// What it says, as it says it.
    pub(super) body: Map<String, Value>,
    /// Written in `launch.json`, rather than found by Keel.
    pub(super) written: bool,
}

/// Configurations started together.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Compound {
    pub(super) name: String,
    pub(super) configurations: Vec<String>,
    pub(super) before: Option<String>,
}

#[derive(Clone, Debug, Default)]
pub(super) struct LaunchFile {
    pub(super) configs: Vec<DebugConfig>,
    pub(super) compounds: Vec<Compound>,
    pub(super) inputs: Vec<TaskInput>,
    /// Types of configurations Keel has no debugger for.
    pub(super) unknown: Vec<String>,
}

/// Reads a `launch.json`.
pub(super) fn read(text: &str) -> Result<LaunchFile, String> {
    let file = settings::jsonc::parse(text).map_err(|error| error.to_string())?;
    let mut launch = LaunchFile {
        // Inputs are written as in tasks.json.
        inputs: super::task_config::read(
            &json!({ "inputs": file["inputs"] }).to_string(),
            Path::new("/"),
        )
        .unwrap_or_else(|_| TaskFile::default())
        .inputs,
        ..LaunchFile::default()
    };
    for value in file["configurations"].as_array().into_iter().flatten() {
        let Some(mut body) = value.as_object().cloned() else {
            continue;
        };
        // What it says for macOS wins.
        if let Some(Value::Object(own)) = body.remove("osx") {
            body.extend(own);
        }
        let text = |key: &str| body.get(key).and_then(Value::as_str).map(str::to_owned);
        let (Some(name), Some(kind)) = (text("name"), text("type")) else {
            continue;
        };
        let request = text("request").unwrap_or_else(|| "launch".to_owned());
        match Adapter::of_type(&kind) {
            Some(adapter) => launch.configs.push(DebugConfig {
                name,
                kind,
                adapter,
                request,
                body,
                written: true,
            }),
            None => launch.unknown.push(kind),
        }
    }
    for value in file["compounds"].as_array().into_iter().flatten() {
        let Some(name) = value["name"].as_str() else {
            continue;
        };
        launch.compounds.push(Compound {
            name: name.to_owned(),
            configurations: value["configurations"]
                .as_array()
                .into_iter()
                .flatten()
                // A name, or an object with the folder it is in.
                .filter_map(|value| value.as_str().or(value["name"].as_str()))
                .map(str::to_owned)
                .collect(),
            before: value["preLaunchTask"].as_str().map(str::to_owned),
        });
    }
    Ok(launch)
}

fn config(name: String, kind: &str, adapter: Adapter, body: Value) -> DebugConfig {
    let body = match body {
        Value::Object(body) => body,
        _ => Map::new(),
    };
    DebugConfig {
        name,
        kind: kind.to_owned(),
        adapter,
        request: "launch".to_owned(),
        body,
        written: false,
    }
}

/// What can be debugged without a `launch.json`: the open file first.
pub(super) fn found(root: &Path, file: Option<(&Path, &str)>) -> Vec<DebugConfig> {
    let mut configs = Vec::new();
    if let Some((path, language)) = file {
        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        match language {
            "python" => configs.push(config(
                format!("Python: {name}"),
                "debugpy",
                Adapter::Python,
                json!({ "program": "${file}", "console": "integratedTerminal", "justMyCode": true }),
            )),
            "javascript" | "typescript" => configs.push(config(
                format!("Node: {name}"),
                "pwa-node",
                Adapter::Node,
                json!({ "program": "${file}", "skipFiles": ["<node_internals>/**"] }),
            )),
            _ => {}
        }
    }
    if root.join("Cargo.toml").is_file() {
        configs.push(config(
            "Rust: cargo run".to_owned(),
            "lldb-dap",
            Adapter::Lldb,
            json!({ "cargo": { "args": ["build"] } }),
        ));
    }
    if let Some(scripts) = super::npm_scripts::NpmScripts::read(root) {
        for (name, _) in &scripts.scripts {
            configs.push(config(
                format!("{}: {name}", scripts.manager),
                "pwa-node",
                Adapter::Node,
                json!({
                    "runtimeExecutable": scripts.manager,
                    "runtimeArgs": ["run", name],
                    "skipFiles": ["<node_internals>/**"],
                }),
            ));
        }
    }
    configs
}

/// The debugging of a test run by its language's runner.
pub(super) fn test_of(label: &str, kind: &str, body: Value) -> DebugConfig {
    let adapter = Adapter::of_type(kind).unwrap_or(Adapter::Node);
    config(label.to_owned(), kind, adapter, body)
}

/// The debugging of one Rust test, or of all of a file's.
pub(super) fn rust_test(
    label: &str,
    cargo_args: Vec<String>,
    program_args: Vec<String>,
    source: &Path,
) -> DebugConfig {
    config(
        label.to_owned(),
        "lldb-dap",
        Adapter::Lldb,
        json!({
            "cargo": { "args": cargo_args, "source": source },
            "args": program_args,
        }),
    )
}

/// Every text of a value with its variables filled in, or the one that is
/// unknown.
pub(super) fn expanded(value: &Value, variables: &Variables) -> Result<Value, String> {
    Ok(match value {
        Value::String(text) => Value::String(variables.expand(text)?),
        Value::Array(values) => Value::Array(
            values
                .iter()
                .map(|value| expanded(value, variables))
                .collect::<Result<_, _>>()?,
        ),
        Value::Object(map) => Value::Object(
            map.iter()
                .map(|(key, value)| Ok((key.clone(), expanded(value, variables)?)))
                .collect::<Result<_, String>>()?,
        ),
        other => other.clone(),
    })
}

/// The ids of the inputs a value asks for.
pub(super) fn inputs_in(value: &Value) -> Vec<String> {
    let text = value.to_string();
    let mut ids: Vec<String> = Vec::new();
    for rest in text.split("${input:").skip(1) {
        if let Some((id, _)) = rest.split_once('}')
            && !ids.iter().any(|known| known == id)
        {
            ids.push(id.to_owned());
        }
    }
    ids
}

/// A list of words: written as one, or as a line to split as a shell does.
pub(super) fn words(value: &Value) -> Vec<String> {
    match value {
        Value::Array(values) => values
            .iter()
            .map(|value| match value {
                Value::String(text) => text.clone(),
                other => other.to_string(),
            })
            .collect(),
        Value::String(line) => {
            let mut words = Vec::new();
            let mut word: Option<String> = None;
            let mut quote = None;
            let mut chars = line.chars();
            while let Some(char) = chars.next() {
                match (quote, char) {
                    (None, '\'' | '"') => {
                        quote = Some(char);
                        word.get_or_insert_default();
                    }
                    (Some(open), char) if open == char => quote = None,
                    (None | Some('"'), '\\') => {
                        word.get_or_insert_default().extend(chars.next());
                    }
                    (None, char) if char.is_whitespace() => words.extend(word.take()),
                    (_, char) => word.get_or_insert_default().push(char),
                }
            }
            words.extend(word);
            words
        }
        _ => Vec::new(),
    }
}

/// The variables of a `.env` file.
pub(super) fn env_file(text: &str) -> Vec<(String, String)> {
    text.lines()
        .filter_map(|line| {
            let line = line.trim();
            if line.starts_with('#') {
                return None;
            }
            let line = line.strip_prefix("export ").unwrap_or(line);
            let (name, value) = line.split_once('=')?;
            let value = value.trim();
            let unquoted = ['"', '\'']
                .iter()
                .find_map(|quote| value.strip_prefix(*quote)?.strip_suffix(*quote));
            let value = match unquoted {
                Some(value) => value,
                // A comment after the value.
                None => value.split(" #").next().unwrap_or(value).trim(),
            };
            Some((name.trim().to_owned(), value.to_owned()))
        })
        .collect()
}

/// What the adapter is asked to launch or attach to; `config.body` has its
/// variables filled in.
pub(super) fn arguments(
    config: &DebugConfig,
    root: &Path,
    python: Option<&Path>,
    formatters: &[String],
    no_debug: bool,
) -> Value {
    let mut body = config.body.clone();
    for key in [
        "preLaunchTask",
        "postDebugTask",
        "presentation",
        "internalConsoleOptions",
    ] {
        body.remove(key);
    }
    let cwd = body
        .get("cwd")
        .and_then(Value::as_str)
        .map(|cwd| root.join(cwd))
        .unwrap_or_else(|| root.to_path_buf());
    body.insert("cwd".to_owned(), json!(cwd));
    // Programs are written from the project's folder.
    if let Some(program) = body.get("program").and_then(Value::as_str) {
        let program = root.join(program);
        body.insert("program".to_owned(), json!(program));
    }
    if no_debug {
        body.insert("noDebug".to_owned(), json!(true));
    }
    match config.adapter {
        Adapter::Python => {
            if let Some(text) = body
                .remove("envFile")
                .as_ref()
                .and_then(Value::as_str)
                .and_then(|path| std::fs::read_to_string(root.join(path)).ok())
            {
                let mut env: Map<String, Value> = env_file(&text)
                    .into_iter()
                    .map(|(name, value)| (name, Value::String(value)))
                    .collect();
                if let Some(Value::Object(own)) = body.get("env") {
                    env.extend(own.clone());
                }
                body.insert("env".to_owned(), Value::Object(env));
            }
            if !body.contains_key("python") {
                // The older name of it.
                let named = body.remove("pythonPath");
                let python = named
                    .as_ref()
                    .and_then(Value::as_str)
                    .map(PathBuf::from)
                    .or_else(|| python.map(Path::to_path_buf));
                if let Some(python) = python {
                    body.insert("python".to_owned(), json!([python]));
                }
            }
            body.entry("console").or_insert(json!("integratedTerminal"));
            body.insert("type".to_owned(), json!("debugpy"));
        }
        Adapter::Node => {
            let kind = match config.kind.as_str() {
                "node" | "extensionHost" => "pwa-node",
                "chrome" => "pwa-chrome",
                "msedge" => "pwa-msedge",
                kind => kind,
            };
            body.insert("type".to_owned(), json!(kind));
            body.insert("__workspaceFolder".to_owned(), json!(root));
            body.entry("console").or_insert(json!("internalConsole"));
            body.entry("sourceMaps").or_insert(json!(true));
        }
        Adapter::Lldb => {
            body.remove("cargo");
            body.remove("sourceLanguages");
            if let Some(args) = body.get("args") {
                let args = words(args);
                body.insert("args".to_owned(), json!(args));
            }
            // `["A=1"]` is what every version takes.
            let env: Vec<String> = match body.get("env").or(body.get("environment")) {
                Some(Value::Object(env)) => env
                    .iter()
                    .map(|(name, value)| match value {
                        Value::String(value) => format!("{name}={value}"),
                        value => format!("{name}={value}"),
                    })
                    .collect(),
                Some(Value::Array(env)) => env
                    .iter()
                    .filter_map(|entry| match entry {
                        Value::String(entry) => Some(entry.clone()),
                        // cppdbg's { "name": "A", "value": "1" }.
                        entry => Some(format!(
                            "{}={}",
                            entry["name"].as_str()?,
                            entry["value"].as_str().unwrap_or_default()
                        )),
                    })
                    .collect(),
                _ => Vec::new(),
            };
            body.remove("environment");
            body.insert("env".to_owned(), json!(env));
            if body.remove("stopAtEntry") == Some(json!(true)) {
                body.insert("stopOnEntry".to_owned(), json!(true));
            }
            if let Some(pid) = body.remove("processId") {
                body.insert("pid".to_owned(), pid);
            }
            // The process's number, asked as a text.
            if let Some(pid) = body
                .get("pid")
                .and_then(Value::as_str)
                .and_then(|pid| pid.trim().parse::<i64>().ok())
            {
                body.insert("pid".to_owned(), json!(pid));
            }
            let in_terminal = body.get("terminal") == Some(&json!("integrated"))
                || body.get("console") == Some(&json!("integratedTerminal"))
                || body.get("externalConsole") == Some(&json!(true));
            if in_terminal {
                body.insert("runInTerminal".to_owned(), json!(true));
                body.insert("console".to_owned(), json!("integratedTerminal"));
            }
            body.remove("terminal");
            let mut init: Vec<Value> = formatters.iter().map(|line| json!(line)).collect();
            init.extend(
                body.get("initCommands")
                    .and_then(Value::as_array)
                    .cloned()
                    .into_iter()
                    .flatten(),
            );
            body.insert("initCommands".to_owned(), Value::Array(init));
        }
    }
    body.insert("name".to_owned(), json!(config.name));
    body.insert("request".to_owned(), json!(config.request));
    Value::Object(body)
}

/// The programs a `cargo build --message-format=json` made: name, path,
/// the file it starts from, and whether it is a test.
pub(super) fn cargo_programs(output: &str) -> Vec<(String, PathBuf, PathBuf, bool)> {
    output
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .filter(|message| message["reason"] == "compiler-artifact")
        .filter_map(|message| {
            Some((
                message["target"]["name"].as_str()?.to_owned(),
                PathBuf::from(message["executable"].as_str()?),
                PathBuf::from(message["target"]["src_path"].as_str()?),
                message["profile"]["test"].as_bool().unwrap_or(false),
            ))
        })
        .collect()
}

/// The test program a source file is part of: the integration test that
/// is that file, or the package's library, or its program.
pub(super) fn program_of_source<'a>(
    programs: &'a [(String, PathBuf, PathBuf, bool)],
    source: &Path,
) -> Option<&'a PathBuf> {
    programs
        .iter()
        .find(|(_, _, from, _)| from == source)
        .or_else(|| {
            programs
                .iter()
                .filter(|(_, _, from, _)| {
                    from.parent()
                        .is_some_and(|folder| source.starts_with(folder))
                })
                // The deepest folder, and in it the library before a program.
                .max_by_key(|(_, _, from, _)| (from.components().count(), from.ends_with("lib.rs")))
        })
        .map(|(_, program, _, _)| program)
}

/// A first `launch.json`, from what Keel found.
pub(super) fn template(found: &[DebugConfig]) -> String {
    let mut configs: Vec<Value> = Vec::new();
    for config in found.iter().take(3) {
        let mut body = Map::new();
        body.insert("name".to_owned(), json!(config.name));
        body.insert("type".to_owned(), json!(config.kind));
        body.insert("request".to_owned(), json!(config.request));
        body.extend(config.body.clone());
        configs.push(Value::Object(body));
    }
    if configs.is_empty() {
        configs.push(json!({
            "name": "Programma",
            "type": "lldb-dap",
            "request": "launch",
            "program": "${workspaceFolder}/a.out",
        }));
    }
    let file = json!({ "version": "0.2.0", "configurations": configs });
    let text = serde_json::to_string_pretty(&file).unwrap_or_default();
    format!("// https://code.visualstudio.com/docs/debugtest/debugging-configuration\n{text}\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn variables() -> Variables {
        Variables {
            root: "/p/app".into(),
            file: Some("/p/app/src/menu.py".into()),
            ..Variables::default()
        }
    }

    #[test]
    fn a_launch_file_is_read_as_vs_code_reads_it() {
        let file = read(
            r#"{
                // Prove
                "version": "0.2.0",
                "configurations": [
                    { "name": "Server", "type": "node", "request": "launch", "program": "${workspaceFolder}/server.js", "preLaunchTask": "compila" },
                    { "name": "Prova", "type": "debugpy", "request": "launch", "program": "${file}", "osx": { "args": ["${input:cosa}"] } },
                    { "name": "Attacca", "type": "lldb", "request": "attach", "pid": "${command:pickProcess}" },
                    { "name": "Go", "type": "go", "request": "launch" },
                ],
                "compounds": [{ "name": "Tutto", "configurations": ["Server", { "name": "Prova" }], "preLaunchTask": "prima" }],
                "inputs": [{ "id": "cosa", "type": "promptString", "description": "Cosa?" }],
            }"#,
        )
        .unwrap();
        let names: Vec<&str> = file
            .configs
            .iter()
            .map(|config| config.name.as_str())
            .collect();
        assert_eq!(names, ["Server", "Prova", "Attacca"]);
        assert_eq!(file.unknown, ["go"]);
        assert_eq!(file.compounds[0].configurations, ["Server", "Prova"]);
        assert_eq!(file.compounds[0].before.as_deref(), Some("prima"));
        assert_eq!(file.inputs[0].description, "Cosa?");
        let body = Value::Object(file.configs[1].body.clone());
        assert_eq!(inputs_in(&body), ["cosa"]);
        let mut known = variables();
        known.inputs.insert("cosa".into(), "due parole".into());
        let filled = expanded(&body, &known).unwrap();
        assert_eq!(filled["program"], "/p/app/src/menu.py");
        assert_eq!(filled["args"][0], "due parole");
        assert!(expanded(&Value::Object(file.configs[2].body.clone()), &known).is_err());
    }

    #[test]
    fn each_adapter_is_asked_its_own_way() {
        let root = Path::new("/p/app");
        let python = config(
            "Py".into(),
            "python",
            Adapter::Python,
            json!({ "program": "a.py", "preLaunchTask": "x" }),
        );
        let asked = arguments(&python, root, Some(Path::new("/v/bin/python")), &[], false);
        assert_eq!(asked["python"], json!(["/v/bin/python"]));
        assert_eq!(asked["program"], "/p/app/a.py");
        assert_eq!(
            (&asked["cwd"], &asked["console"]),
            (&json!("/p/app"), &json!("integratedTerminal"))
        );
        assert!(asked.get("preLaunchTask").is_none());

        let node = config(
            "Node".into(),
            "node",
            Adapter::Node,
            json!({ "program": "/x/a.ts", "cwd": "sub" }),
        );
        let asked = arguments(&node, root, None, &[], true);
        assert_eq!(
            (&asked["type"], &asked["cwd"]),
            (&json!("pwa-node"), &json!("/p/app/sub"))
        );
        assert_eq!(asked["noDebug"], true);

        let lldb = config(
            "Rust".into(),
            "lldb",
            Adapter::Lldb,
            json!({ "program": "target/debug/app", "args": "--nome 'due parole' a\\ b", "env": { "A": "1" }, "terminal": "integrated", "cargo": {}, "initCommands": ["x"], "stopAtEntry": true }),
        );
        let asked = arguments(&lldb, root, None, &["formatta".into()], false);
        assert_eq!(asked["args"], json!(["--nome", "due parole", "a b"]));
        assert_eq!(asked["env"], json!(["A=1"]));
        assert_eq!(asked["initCommands"], json!(["formatta", "x"]));
        assert_eq!(asked["runInTerminal"], true);
        assert_eq!(asked["stopOnEntry"], true);
        assert!(asked.get("cargo").is_none());
        let attach = config("A".into(), "lldb", Adapter::Lldb, json!({ "pid": "42" }));
        assert_eq!(arguments(&attach, root, None, &[], false)["pid"], 42);
    }

    #[test]
    fn env_files_and_cargos_programs() {
        assert_eq!(
            env_file("# commento\nexport A=1\nB=\"due parole\" \nC=tre # no\n\nD='x=y'\n"),
            [("A", "1"), ("B", "due parole"), ("C", "tre"), ("D", "x=y")]
                .map(|(name, value)| (name.to_owned(), value.to_owned()))
        );
        let line = |name: &str, src: &str, executable: Value| {
            json!({ "reason": "compiler-artifact", "target": { "name": name, "src_path": src }, "executable": executable, "profile": { "test": true } }).to_string()
        };
        let output = [
            line("app", "/p/src/main.rs", json!("/p/target/app")),
            line("app", "/p/src/lib.rs", json!("/p/target/lib")),
            line("casa", "/p/tests/casa.rs", json!("/p/target/casa")),
            line("dep", "/d/src/lib.rs", Value::Null),
            "non è json".to_owned(),
        ]
        .join("\n");
        let programs = cargo_programs(&output);
        assert_eq!(programs.len(), 3);
        let of = |source: &str| {
            program_of_source(&programs, Path::new(source))
                .unwrap()
                .display()
                .to_string()
        };
        assert_eq!(of("/p/tests/casa.rs"), "/p/target/casa");
        assert_eq!(of("/p/src/menu/voci.rs"), "/p/target/lib");
        assert_eq!(of("/p/src/main.rs"), "/p/target/app");
    }

    #[test]
    fn what_is_found_without_a_file() {
        let folder = tempfile::tempdir().unwrap();
        std::fs::write(folder.path().join("Cargo.toml"), "").unwrap();
        std::fs::write(
            folder.path().join("package.json"),
            r#"{ "scripts": { "dev": "vite" } }"#,
        )
        .unwrap();
        let names: Vec<String> = found(folder.path(), Some((Path::new("/p/a.py"), "python")))
            .into_iter()
            .map(|config| config.name)
            .collect();
        assert_eq!(names, ["Python: a.py", "Rust: cargo run", "npm: dev"]);
        let written = template(&found(folder.path(), None));
        let read = read(&written).unwrap();
        assert_eq!(read.configs.len(), 2);
        assert!(read.configs[0].body.contains_key("cargo"));
    }
}
