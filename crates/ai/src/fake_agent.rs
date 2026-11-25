//! A small ACP agent for the tests, run in a thread, that behaves like
//! `claude-agent-acp` without a model:
//!
//! - a prompt with `edit <path> <old> <new>` reads the file through the
//!   client, shows a plan and a tool call with the difference, asks
//!   permission, writes the file on disk itself (as Claude Code does) and
//!   says "Done.";
//! - any other prompt is answered with "You said: <text>";
//! - a prompt with `wait` waits until `session/cancel`;
//! - `session/list` and `session/load` know the sessions it made.

use std::io::{self, BufRead as _, BufReader, Write as _};
use std::sync::mpsc;

use serde_json::{Value, json};

/// Runs the fake agent; returns what it writes and where to write to it.
pub fn spawn() -> io::Result<(io::PipeReader, io::PipeWriter)> {
    let (client_reads, agent_writes) = io::pipe()?;
    let (agent_reads, client_writes) = io::pipe()?;
    std::thread::Builder::new()
        .name("fake agent".to_owned())
        .spawn(move || run(agent_reads, agent_writes))?;
    Ok((client_reads, client_writes))
}

/// A session: its id, folder, and what was said (prompt, answer).
type Session = (String, String, Vec<(String, String)>);

struct Fake {
    out: io::PipeWriter,
    incoming: mpsc::Receiver<Value>,
    next_id: u64,
    /// Sessions: id, cwd and what was said, for `session/load`.
    sessions: Vec<Session>,
    /// Messages read while waiting for an answer, handled after.
    queued: Vec<Value>,
}

impl Fake {
    fn send(&mut self, message: Value) {
        let _ = writeln!(self.out, "{message}");
        let _ = self.out.flush();
    }

    fn update(&mut self, session: &str, update: Value) {
        self.send(json!({ "jsonrpc": "2.0", "method": "session/update", "params": { "sessionId": session, "update": update } }));
    }

    fn text(&mut self, session: &str, kind: &str, text: &str) {
        self.update(
            session,
            json!({ "sessionUpdate": kind, "content": { "type": "text", "text": text } }),
        );
    }

    /// Asks the client and waits for its answer; a `session/cancel` meanwhile
    /// makes it `None`.
    fn ask(&mut self, method: &str, params: Value) -> Option<Value> {
        self.next_id += 1;
        let id = format!("agent-{}", self.next_id);
        self.send(json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }));
        while let Ok(message) = self.incoming.recv() {
            if message["id"] == json!(id) && message.get("method").is_none() {
                return message.get("result").cloned();
            }
            if message["method"] == "session/cancel" {
                return None;
            }
            self.queued.push(message);
        }
        None
    }

    fn prompt(&mut self, id: Value, params: &Value) {
        let session = params["sessionId"].as_str().unwrap_or_default().to_owned();
        let text: String = params["prompt"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|block| match block["type"].as_str() {
                Some("text") => block["text"].as_str().unwrap_or_default().to_owned(),
                Some("resource_link") => {
                    format!(" [{}]", block["uri"].as_str().unwrap_or_default())
                }
                Some("resource") => format!(
                    " [{}]",
                    block["resource"]["uri"].as_str().unwrap_or_default()
                ),
                _ => String::new(),
            })
            .collect();
        let words: Vec<&str> = text.split_whitespace().collect();
        let reply = if words.first() == Some(&"wait") {
            // Until the client cancels.
            loop {
                match self.incoming.recv() {
                    Ok(message) if message["method"] == "session/cancel" => break,
                    Ok(message) => self.queued.push(message),
                    Err(_) => return,
                }
            }
            self.send(
                json!({ "jsonrpc": "2.0", "id": id, "result": { "stopReason": "cancelled" } }),
            );
            return;
        } else if words.first() == Some(&"edit") && words.len() >= 4 {
            let (path, old, new) = (words[1], words[2], words[3]);
            self.text(&session, "agent_thought_chunk", "Reading the file.");
            let read = self.ask(
                "fs/read_text_file",
                json!({ "sessionId": session, "path": path }),
            );
            let current = read
                .and_then(|read| read["content"].as_str().map(str::to_owned))
                .unwrap_or_default();
            self.update(
                &session,
                json!({ "sessionUpdate": "plan", "entries": [
                { "content": "Read the file", "priority": "high", "status": "completed" },
                { "content": "Change it", "priority": "high", "status": "in_progress" } ] }),
            );
            let diff = json!([{ "type": "diff", "path": path, "oldText": old, "newText": new }]);
            self.update(&session, json!({ "sessionUpdate": "tool_call", "toolCallId": "edit-1", "title": format!("Edit {path}"),
                "kind": "edit", "status": "pending", "content": diff, "locations": [{ "path": path }] }));
            let answer = self.ask("session/request_permission", json!({
                "sessionId": session,
                "toolCall": { "toolCallId": "edit-1", "title": format!("Edit {path}"), "kind": "edit", "content": diff },
                "options": [
                    { "optionId": "allow", "name": "Allow", "kind": "allow_once" },
                    { "optionId": "always", "name": "Always allow", "kind": "allow_always" },
                    { "optionId": "reject", "name": "Reject", "kind": "reject_once" } ] }));
            let allowed = answer.is_some_and(|answer| {
                answer["outcome"]["outcome"] == "selected"
                    && answer["outcome"]["optionId"] != "reject"
            });
            if allowed {
                let _ = std::fs::write(path, current.replacen(old, new, 1));
                self.update(&session, json!({ "sessionUpdate": "tool_call_update", "toolCallId": "edit-1", "status": "completed" }));
                "Done.".to_owned()
            } else {
                self.update(&session, json!({ "sessionUpdate": "tool_call_update", "toolCallId": "edit-1", "status": "failed" }));
                "Not changed.".to_owned()
            }
        } else if words.first() == Some(&"run") {
            // Reasoning, a command of several lines with its output, and
            // an empty message between them, as replayed chats have.
            self.text(&session, "agent_thought_chunk", "Guardo come è fatto il pannello. Prima leggo il file, poi provo il comando, poi controllo che i test passino ancora.\n\nIl comando cambia una riga sola.");
            self.text(&session, "agent_message_chunk", "\n\n");
            self.update(&session, json!({ "sessionUpdate": "tool_call", "toolCallId": "run-1", "kind": "execute",
                "title": "python3 - <<'EOF'\np='crates/keel/src/workbench_window/agent_view.rs'\nprint(open(p).read()[:10])\nEOF",
                "status": "completed", "content": [{ "type": "content", "content": { "type": "text", "text": "//! How Cla" } }] }));
            self.update(
                &session,
                json!({ "sessionUpdate": "tool_call", "toolCallId": "run-2", "kind": "search",
                "title": "grep -n \"fn render\" crates/keel/src", "status": "completed" }),
            );
            "Fatto: il comando ha letto il file.".to_owned()
        } else if text.trim() == "/usage" {
            // Claude Code's own `/usage`, as the adapter writes it.
            "## Usage\n\n> Claude pro subscription usage\n\n### Limits\n\n**5-hour limit** — **74%** · Resets Nov 25, 2:10 PM GMT+1\n\n`███████████████░░░░░`\n\n**Weekly · all models** — **86%** · Resets Nov 29, 2:00 AM GMT+1\n\n---\n\n### This session\n\n| Cost | API time | Active |\n|:--|:--|:--|\n| $0.00 | 0s | 3s |\n".to_owned()
        } else {
            format!("You said: {}", text.trim())
        };
        for chunk in reply.split_inclusive(' ') {
            self.text(&session, "agent_message_chunk", chunk);
        }
        if let Some((_, _, said)) = self
            .sessions
            .iter_mut()
            .find(|(known, _, _)| *known == session)
        {
            said.push((text.trim().to_owned(), reply));
        }
        self.update(
            &session,
            json!({ "sessionUpdate": "usage_update", "used": 313_000, "size": 1_000_000,
            "cost": { "amount": 0.42, "currency": "USD" } }),
        );
        self.send(json!({ "jsonrpc": "2.0", "id": id, "result": { "stopReason": "end_turn" } }));
    }

    fn handle(&mut self, message: Value) {
        let method = message["method"].as_str().unwrap_or_default().to_owned();
        let Some(id) = message.get("id").cloned() else {
            return;
        };
        let params = message["params"].clone();
        let result = match method.as_str() {
            "initialize" => {
                self.send(json!({ "jsonrpc": "2.0", "method": "auth/status_update", "params": {
                    "state": "authenticated", "accounts": [{ "label": "Claude Pro", "active": true,
                    "email": "fake@example.com", "_meta": { "claudeCode": { "kind": "account" } } }] } }));
                json!({
                "protocolVersion": 1,
                "agentCapabilities": { "loadSession": true, "promptCapabilities": { "embeddedContext": true },
                    "sessionCapabilities": { "list": {}, "close": {} }, "auth": { "logout": {}, "status": {} } },
                "agentInfo": { "name": "fake-agent", "title": "Fake Agent", "version": "1.0.0" },
                "authMethods": [
                    { "id": "claude-ai-login", "name": "Claude Subscription", "description": "Use Claude subscription",
                      "type": "terminal", "_meta": { "terminal-auth": { "command": "/usr/bin/true", "args": ["login"] } } },
                    { "id": "console-login", "name": "Anthropic Console", "description": "Use Anthropic Console (API usage billing)",
                      "type": "terminal", "_meta": { "terminal-auth": { "command": "/usr/bin/true", "args": ["console"] } } } ],
                })
            }
            "auth/status" => {
                json!({ "state": "authenticated", "accounts": [{ "label": "Claude Pro", "active": true,
                "email": "fake@example.com", "_meta": { "claudeCode": { "kind": "account" } } }] })
            }
            "logout" => {
                self.send(
                    json!({ "jsonrpc": "2.0", "method": "auth/status_update", "params": {
                    "state": "unauthenticated", "accounts": [] } }),
                );
                json!({})
            }
            "authenticate" | "session/close" => json!({}),
            "session/delete" => {
                let session = params["sessionId"].as_str().unwrap_or_default();
                self.sessions.retain(|(known, _, _)| known != session);
                json!({})
            }
            "session/new" => {
                let session = format!("session-{}", self.sessions.len() + 1);
                let cwd = params["cwd"].as_str().unwrap_or_default().to_owned();
                self.sessions.push((session.clone(), cwd, Vec::new()));
                self.update(&session, json!({ "sessionUpdate": "available_commands_update",
                    "availableCommands": [{ "name": "review", "description": "Review the changes" },
                        { "name": "frontend-design", "description": "Design pages that look designed" },
                        { "name": "compact", "description": "Make the conversation shorter", "input": { "hint": "[instructions]" } }] }));
                json!({ "sessionId": session, "configOptions": [
                    { "id": "mode", "name": "Mode", "category": "mode", "type": "select", "currentValue": "default",
                      "options": [{ "value": "default", "name": "Default" }, { "value": "plan", "name": "Plan" }] },
                    { "id": "model", "name": "Model", "category": "model", "type": "select", "currentValue": "opus",
                      "options": [{ "value": "opus", "name": "Opus 5.5" }, { "value": "sonnet", "name": "Sonnet 5.5" }] },
                    { "id": "effort", "name": "Effort", "category": "thought_level", "type": "select", "currentValue": "high",
                      "options": [{ "value": "low", "name": "Low" }, { "value": "high", "name": "High" }] } ] })
            }
            "session/list" => {
                let cwd = params["cwd"].as_str().unwrap_or_default();
                let sessions: Vec<Value> = self
                    .sessions
                    .iter()
                    .filter(|(_, known, said)| known == cwd && !said.is_empty())
                    .map(|(session, cwd, said)| json!({ "sessionId": session, "cwd": cwd, "title": said[0].0 }))
                    .collect();
                json!({ "sessions": sessions })
            }
            "session/load" => {
                let session = params["sessionId"].as_str().unwrap_or_default().to_owned();
                let said = self
                    .sessions
                    .iter()
                    .find(|(known, _, _)| *known == session)
                    .map(|(_, _, said)| said.clone())
                    .unwrap_or_default();
                for (user, agent) in said {
                    self.text(&session, "user_message_chunk", &user);
                    self.text(&session, "agent_message_chunk", &agent);
                }
                json!({})
            }
            "session/set_mode" => {
                let session = params["sessionId"].as_str().unwrap_or_default().to_owned();
                let mode = params["modeId"].clone();
                self.update(
                    &session,
                    json!({ "sessionUpdate": "current_mode_update", "currentModeId": mode }),
                );
                json!({})
            }
            "session/set_config_option" => {
                let session = params["sessionId"].as_str().unwrap_or_default().to_owned();
                let option = params["configId"].as_str().unwrap_or_default().to_owned();
                let value = params["value"].clone();
                if option == "mode" {
                    self.update(
                        &session,
                        json!({ "sessionUpdate": "current_mode_update", "currentModeId": value }),
                    );
                }
                json!({ "configOptions": [{ "id": option, "name": option, "currentValue": value,
                    "options": [{ "value": value, "name": value }] }] })
            }
            "session/prompt" => {
                self.prompt(id, &params);
                return;
            }
            _ => {
                self.send(json!({ "jsonrpc": "2.0", "id": id, "error": { "code": -32601, "message": format!("unknown {method}") } }));
                return;
            }
        };
        self.send(json!({ "jsonrpc": "2.0", "id": id, "result": result }));
    }
}

fn run(input: io::PipeReader, out: io::PipeWriter) {
    let (sender, incoming) = mpsc::channel();
    std::thread::spawn(move || {
        for line in BufReader::new(input).lines().map_while(Result::ok) {
            if let Ok(message) = serde_json::from_str::<Value>(&line)
                && sender.send(message).is_err()
            {
                break;
            }
        }
    });
    let mut fake = Fake {
        out,
        incoming,
        next_id: 0,
        sessions: Vec::new(),
        queued: Vec::new(),
    };
    loop {
        let message = if fake.queued.is_empty() {
            match fake.incoming.recv() {
                Ok(message) => message,
                Err(_) => break,
            }
        } else {
            fake.queued.remove(0)
        };
        fake.handle(message);
    }
}
