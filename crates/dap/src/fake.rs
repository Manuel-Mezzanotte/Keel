//! A tiny debug adapter for the tests, in Keel and in this crate. It
//! "runs" a text file: `name = value` sets a variable, `print words`
//! writes them, `call name` enters a function `name:` further down until
//! its `return`, and `crash words` stops as an exception does. Anything
//! else is a line that does nothing.
//!
//! It stops at breakpoints (with a condition `name == value`, or a log
//! message with `{name}` in it), steps over, into and out of calls,
//! evaluates a variable's name, and changes one. `launch` takes `program`
//! and `stopOnEntry`; with `console` `integratedTerminal` it first asks
//! the editor to run a command in its terminal.

use std::collections::{BTreeMap, HashMap};
use std::io::{self, BufReader, Read, Write};
use std::os::unix::net::UnixStream;

use lsp::rpc::{read_message, write_message};
use serde_json::{Value, json};

/// The two ends of a fake adapter running in a thread.
pub fn spawn() -> io::Result<(UnixStream, UnixStream)> {
    let (ours, theirs) = UnixStream::pair()?;
    let reader = theirs.try_clone()?;
    std::thread::spawn(move || run(reader, theirs));
    Ok((ours.try_clone()?, ours))
}

struct Program {
    path: String,
    lines: Vec<String>,
    /// The line each function starts at.
    functions: HashMap<String, usize>,
    /// The line running, and for each call where it came from.
    line: usize,
    calls: Vec<(String, usize)>,
    variables: BTreeMap<String, String>,
    breakpoints: Vec<Value>,
    ended: bool,
}

enum Stop {
    At(&'static str, Option<String>),
    Ended,
}

impl Program {
    fn load(path: &str) -> Self {
        let text = std::fs::read_to_string(path).unwrap_or_default();
        let lines: Vec<String> = text.lines().map(|line| line.trim().to_owned()).collect();
        let functions = lines
            .iter()
            .enumerate()
            .filter_map(|(index, line)| Some((line.strip_suffix(':')?.to_owned(), index)))
            .collect();
        Self {
            path: path.to_owned(),
            lines,
            functions,
            line: 0,
            calls: Vec::new(),
            variables: BTreeMap::new(),
            breakpoints: Vec::new(),
            ended: false,
        }
    }

    /// Runs the current line; returns what it printed, or why it stopped.
    fn step(&mut self, out: &mut Vec<String>) -> Option<Stop> {
        let Some(line) = self.lines.get(self.line).cloned() else {
            self.ended = true;
            return Some(Stop::Ended);
        };
        // A function's text is only for calls.
        if line.ends_with(':') && self.calls.is_empty() {
            self.ended = true;
            return Some(Stop::Ended);
        }
        if let Some(words) = line.strip_prefix("crash ") {
            return Some(Stop::At("exception", Some(words.to_owned())));
        }
        if let Some(name) = line.strip_prefix("call ")
            && let Some(start) = self.functions.get(name)
        {
            self.calls.push((name.to_owned(), self.line));
            self.line = start + 1;
            return None;
        }
        if line == "return" {
            match self.calls.pop() {
                Some((_, from)) => self.line = from + 1,
                None => {
                    self.ended = true;
                    return Some(Stop::Ended);
                }
            }
            return None;
        }
        if let Some(words) = line.strip_prefix("print ") {
            out.push(words.to_owned());
        } else if let Some((name, value)) = line.split_once('=') {
            self.variables
                .insert(name.trim().to_owned(), value.trim().to_owned());
        }
        self.line += 1;
        if self.line >= self.lines.len() {
            self.ended = true;
            return Some(Stop::Ended);
        }
        None
    }

    /// Whether to stop at the line now; a log message only writes.
    fn breakpoint(&self, out: &mut Vec<String>) -> bool {
        let here = self
            .breakpoints
            .iter()
            .find(|breakpoint| breakpoint["line"].as_u64() == Some(self.line as u64 + 1));
        let Some(breakpoint) = here else {
            return false;
        };
        if let Some(condition) = breakpoint["condition"].as_str() {
            let (name, value) = condition.split_once("==").unwrap_or((condition, ""));
            if self.variables.get(name.trim()).map(String::as_str) != Some(value.trim()) {
                return false;
            }
        }
        if let Some(message) = breakpoint["logMessage"].as_str() {
            let filled = self
                .variables
                .iter()
                .fold(message.to_owned(), |text, (name, value)| {
                    text.replace(&format!("{{{name}}}"), value)
                });
            out.push(filled);
            return false;
        }
        true
    }

    /// Runs until `stop` says so, a breakpoint or the end.
    fn run_until(&mut self, out: &mut Vec<String>, stop: impl Fn(&Self) -> bool) -> Stop {
        loop {
            if let Some(stop) = self.step(out) {
                return stop;
            }
            if self.breakpoint(out) {
                return Stop::At("breakpoint", None);
            }
            if stop(self) {
                return Stop::At("step", None);
            }
        }
    }

    fn frames(&self) -> Vec<Value> {
        let source = json!({ "name": "program", "path": self.path });
        let mut frames = Vec::new();
        let mut line = self.line;
        for (depth, (name, from)) in self.calls.iter().enumerate().rev() {
            frames.push(json!({
                "id": depth + 2, "name": name, "line": line + 1, "column": 1, "source": source,
            }));
            line = *from;
        }
        frames.push(
            json!({ "id": 1, "name": "main", "line": line + 1, "column": 1, "source": source }),
        );
        frames
    }
}

pub fn run(input: impl Read, mut output: impl Write) {
    let mut reader = BufReader::new(input);
    let mut program: Option<Program> = None;
    let mut breakpoints: Vec<Value> = Vec::new();
    let mut launch: Option<Value> = None;
    let mut seq = 5000;
    let mut send = move |mut message: Value| {
        seq += 1;
        message["seq"] = json!(seq);
        let _ = write_message(&mut output, &message);
    };
    while let Ok(Some(message)) = read_message(&mut reader) {
        let command = message["command"].as_str().unwrap_or_default().to_owned();
        let arguments = &message["arguments"];
        let answer = |body: Value| {
            json!({
                "type": "response", "request_seq": message["seq"], "command": command,
                "success": true, "body": body,
            })
        };
        let refuse = |text: &str| {
            json!({
                "type": "response", "request_seq": message["seq"], "command": command,
                "success": false, "message": text,
            })
        };
        let event =
            |name: &str, body: Value| json!({ "type": "event", "event": name, "body": body });
        // What a run of the program says when it stops.
        let mut after = Vec::new();
        let mut stopped = |stop: Stop, out: Vec<String>| {
            for line in out {
                after.push(event(
                    "output",
                    json!({ "category": "stdout", "output": format!("{line}\n") }),
                ));
            }
            match stop {
                Stop::At(reason, text) => after.push(event(
                    "stopped",
                    json!({ "reason": reason, "threadId": 1, "allThreadsStopped": true, "text": text }),
                )),
                Stop::Ended => {
                    after.push(event("exited", json!({ "exitCode": 0 })));
                    after.push(event("terminated", json!({})));
                }
            }
        };
        match (message["type"].as_str(), command.as_str()) {
            (Some("response"), _) => {}
            (_, "initialize") => {
                send(answer(json!({
                    "supportsConfigurationDoneRequest": true,
                    "supportsConditionalBreakpoints": true,
                    "supportsLogPoints": true,
                    "supportsSetVariable": true,
                    "supportsEvaluateForHovers": true,
                    "supportTerminateDebuggee": true,
                    "exceptionBreakpointFilters": [{ "filter": "all", "label": "Every Exception", "default": true }],
                })));
            }
            (_, "launch") => {
                if arguments["program"].as_str().is_none_or(str::is_empty) {
                    send(refuse("no program"));
                    continue;
                }
                launch = Some(message.clone());
                send(event("initialized", json!({})));
            }
            (_, "setBreakpoints") => {
                breakpoints = arguments["breakpoints"]
                    .as_array()
                    .cloned()
                    .unwrap_or_default();
                if let Some(program) = &mut program {
                    program.breakpoints = breakpoints.clone();
                }
                let set: Vec<Value> = breakpoints
                    .iter()
                    .enumerate()
                    .map(|(id, breakpoint)| json!({ "id": id + 1, "verified": true, "line": breakpoint["line"] }))
                    .collect();
                send(answer(json!({ "breakpoints": set })));
            }
            (_, "configurationDone") => {
                send(answer(json!({})));
                let Some(asked) = launch.take() else {
                    continue;
                };
                send(json!({
                    "type": "response", "request_seq": asked["seq"], "command": "launch",
                    "success": true, "body": {},
                }));
                let settings = &asked["arguments"];
                if settings["console"] == "integratedTerminal" {
                    send(json!({
                        "type": "request", "command": "runInTerminal",
                        "arguments": { "kind": "integrated", "title": "fake", "cwd": settings["cwd"], "args": ["echo", "dal debugger"], "env": { "A": "b c" } },
                    }));
                }
                let mut started = Program::load(settings["program"].as_str().unwrap_or_default());
                started.breakpoints = breakpoints.clone();
                let mut out = Vec::new();
                let stop = if settings["stopOnEntry"].as_bool() == Some(true) {
                    Stop::At("entry", None)
                } else if started.breakpoint(&mut out) {
                    Stop::At("breakpoint", None)
                } else {
                    started.run_until(&mut out, |_| false)
                };
                program = Some(started);
                stopped(stop, out);
            }
            (_, "threads") => send(answer(
                json!({ "threads": [{ "id": 1, "name": "principale" }] }),
            )),
            (_, "stackTrace") => {
                let frames = program.as_ref().map(Program::frames).unwrap_or_default();
                send(answer(
                    json!({ "stackFrames": frames, "totalFrames": frames.len() }),
                ));
            }
            (_, "scopes") => send(answer(json!({ "scopes": [
                { "name": "Locals", "variablesReference": 10, "expensive": false },
            ] }))),
            (_, "variables") => {
                let variables: Vec<Value> = match (&program, arguments["variablesReference"].as_u64()) {
                    (Some(program), Some(10)) => program
                        .variables
                        .iter()
                        .map(|(name, value)| {
                            let holds = value.contains(',');
                            json!({ "name": name, "value": value, "type": "text", "evaluateName": name, "variablesReference": if holds { 20 } else { 0 } })
                        })
                        .collect(),
                    (Some(program), Some(20)) => program
                        .variables
                        .values()
                        .filter(|value| value.contains(','))
                        .flat_map(|value| value.split(','))
                        .enumerate()
                        .map(|(index, value)| json!({ "name": format!("[{index}]"), "value": value.trim(), "variablesReference": 0 }))
                        .collect(),
                    _ => Vec::new(),
                };
                send(answer(json!({ "variables": variables })));
            }
            (_, "evaluate") => {
                let name = arguments["expression"].as_str().unwrap_or_default().trim();
                match program
                    .as_ref()
                    .and_then(|program| program.variables.get(name))
                {
                    Some(value) => send(answer(
                        json!({ "result": value, "type": "text", "variablesReference": 0 }),
                    )),
                    None => send(refuse(&format!("{name} is not there"))),
                }
            }
            (_, "setVariable") => {
                let name = arguments["name"].as_str().unwrap_or_default().to_owned();
                let value = arguments["value"].as_str().unwrap_or_default().to_owned();
                if let Some(program) = &mut program {
                    program.variables.insert(name, value.clone());
                }
                send(answer(json!({ "value": value })));
            }
            (_, "continue" | "next" | "stepIn" | "stepOut") => {
                send(answer(json!({ "allThreadsContinued": true })));
                let Some(program) = program.as_mut().filter(|program| !program.ended) else {
                    continue;
                };
                // A line that crashes is left behind, as a caught one.
                if program
                    .lines
                    .get(program.line)
                    .is_some_and(|line| line.starts_with("crash "))
                {
                    program.line += 1;
                }
                let depth = program.calls.len();
                let mut out = Vec::new();
                let stop = match command.as_str() {
                    "next" => program.run_until(&mut out, |program| program.calls.len() <= depth),
                    "stepIn" => program.run_until(&mut out, |_| true),
                    "stepOut" => program.run_until(&mut out, |program| program.calls.len() < depth),
                    _ => program.run_until(&mut out, |_| false),
                };
                stopped(stop, out);
            }
            (_, "disconnect" | "terminate") => {
                send(answer(json!({})));
                send(event("terminated", json!({})));
                if command == "disconnect" {
                    return;
                }
            }
            (_, "setExceptionBreakpoints" | "pause") => send(answer(json!({}))),
            _ => send(refuse("unknown")),
        }
        for message in after {
            send(message);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, mpsc};
    use std::time::Duration;

    use futures::executor::block_on;

    use super::*;
    use crate::client::{Client, ClientEvent};

    #[test]
    fn a_program_stops_steps_and_ends() {
        let folder = tempfile::tempdir().unwrap();
        let path = folder.path().join("prova.txt");
        std::fs::write(
            &path,
            "a = 1\nb = due\ncall f\nprint fine\nend:\nf:\nc = 3\nreturn\n",
        )
        .unwrap();
        let (sender, events) = mpsc::channel();
        let (reader, writer) = spawn().unwrap();
        let client = Client::start(
            reader,
            writer,
            Arc::new(move |event| {
                let _ = sender.send(event);
            }),
        );
        let wait = |name: &str| loop {
            match events.recv_timeout(Duration::from_secs(5)).unwrap() {
                ClientEvent::Event { event, body } if event == name => break body,
                _ => {}
            }
        };
        let ask =
            |command: &str, arguments: Value| block_on(client.request(command, arguments)).unwrap();
        let top = || {
            let frames = ask("stackTrace", json!({ "threadId": 1 }));
            frames["stackFrames"][0]["line"].as_u64().unwrap()
        };
        assert_eq!(ask("initialize", json!({}))["supportsLogPoints"], true);
        let launched = client.request("launch", json!({ "program": path }));
        wait("initialized");
        ask(
            "setBreakpoints",
            json!({ "source": { "path": path }, "breakpoints": [{ "line": 3 }] }),
        );
        ask("configurationDone", Value::Null);
        block_on(launched).unwrap();
        assert_eq!(wait("stopped")["reason"], "breakpoint");
        assert_eq!(top(), 3);
        let variables = ask("variables", json!({ "variablesReference": 10 }));
        assert_eq!(variables["variables"][1]["value"], "due");
        ask("stepIn", json!({ "threadId": 1 }));
        wait("stopped");
        assert_eq!(top(), 7);
        ask("stepOut", json!({ "threadId": 1 }));
        wait("stopped");
        assert_eq!(top(), 4);
        ask("continue", json!({ "threadId": 1 }));
        assert_eq!(wait("output")["output"], "fine\n");
        wait("terminated");
        let refused = block_on(client.request("evaluate", json!({ "expression": "z" })));
        assert_eq!(refused.unwrap_err().to_string(), "z is not there");
    }
}
