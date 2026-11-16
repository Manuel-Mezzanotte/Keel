//! Claude Code in a terminal of Keel, connected to the window as it is to
//! VS Code and Cursor: Keel announces itself in `~/.claude/ide/`, gives its
//! terminals `CLAUDE_CODE_SSE_PORT`, answers Claude Code's tools (open
//! editors, selection, problems, open and save files) and tells it where
//! the cursor is. Cmd+Alt+K sends the file and lines to the conversation.

use std::time::Duration;

use ai::ide::{self, IdeEvent, IdeOptions, IdeServer, json_result, text_result};
use futures::StreamExt as _;
use futures::channel::oneshot;
use lsp::lsp_types::DiagnosticSeverity;

use super::*;

actions!(keel, [InsertAtMention]);

/// How often the cursor is looked at while Claude Code is connected.
const SELECTION_POLL: Duration = Duration::from_millis(250);

/// The connection of this window.
pub(super) struct ClaudeIde {
    pub(super) server: IdeServer,
    pub(super) connected: bool,
    /// The selection told last, sent again to a new connection and
    /// answered by `getLatestSelection`.
    pub(super) latest: Option<Value>,
    /// What the selection was made of, to tell only changes.
    seen: Option<(String, Range<usize>)>,
}

/// An error result, as MCP marks it.
fn error_result(text: impl Into<String>) -> Value {
    let mut result = text_result(text);
    result["isError"] = json!(true);
    result
}

impl Workbench {
    /// Starts the connection: called for real windows, with `~/.claude/ide`.
    pub(crate) fn start_claude_ide(
        &mut self,
        ide_dir: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let options = IdeOptions {
            ide_dir,
            ide_name: "Keel".to_owned(),
            workspace_folders: self.project.root.iter().cloned().collect(),
            pid: std::process::id(),
        };
        let (server, mut events) = match IdeServer::start(options) {
            Ok(started) => started,
            Err(error) => {
                self.log(format!("Claude Code: {error}"));
                return;
            }
        };
        self.claude_ide = Some(ClaudeIde {
            server,
            connected: false,
            latest: None,
            seen: None,
        });
        cx.spawn_in(window, async move |this, cx| {
            while let Some(event) = events.next().await {
                let alive =
                    this.update_in(cx, |this, window, cx| this.on_ide_event(event, window, cx));
                if alive.is_err() {
                    break;
                }
            }
        })
        .detach();
    }

    /// Removes the announcement in `~/.claude/ide` and stops the agent's
    /// adapter: the window or Keel is closing.
    pub(super) fn stop_claude(&mut self) {
        self.claude_ide = None;
        self.agent_panel.agent = None;
    }

    /// The port Claude Code started in Keel's terminals connects to.
    pub(super) fn claude_ide_env(&self) -> Vec<(String, String)> {
        match &self.claude_ide {
            Some(ide) => vec![
                (ide::PORT_VARIABLE.to_owned(), ide.server.port().to_string()),
                ("ENABLE_IDE_INTEGRATION".to_owned(), "true".to_owned()),
            ],
            None => Vec::new(),
        }
    }

    fn on_ide_event(&mut self, event: IdeEvent, window: &mut Window, cx: &mut Context<Self>) {
        match event {
            IdeEvent::Connected => {
                let Some(ide) = self.claude_ide.as_mut() else {
                    return;
                };
                ide.connected = true;
                ide.seen = None;
                self.log("Claude Code: connected".to_owned());
                self.watch_selection_for_claude(window, cx);
            }
            IdeEvent::Disconnected => {
                if let Some(ide) = self.claude_ide.as_mut() {
                    ide.connected = false;
                }
                self.log("Claude Code: disconnected".to_owned());
            }
            IdeEvent::ToolCall {
                name,
                arguments,
                reply,
            } => self.answer_claude_tool(&name, &arguments, reply, window, cx),
        }
        cx.notify();
    }

    /// Looks at the cursor while Claude Code is connected, and tells it
    /// when it moves.
    fn watch_selection_for_claude(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        cx.spawn_in(window, async move |this, cx| {
            loop {
                cx.background_executor().timer(SELECTION_POLL).await;
                let connected = this.update(cx, |this, cx| {
                    let connected = this.claude_ide.as_ref().is_some_and(|ide| ide.connected);
                    if connected {
                        this.send_selection_to_claude(cx);
                    }
                    connected
                });
                if !matches!(connected, Ok(true)) {
                    break;
                }
            }
        })
        .detach();
    }

    /// The selection of the active editor, as Claude Code reads it.
    fn claude_selection(&self, cx: &App) -> Option<Value> {
        let pane = self.panes.get(self.active)?;
        let path = Uri::parse(&pane.uri)?.file_path()?;
        let editor = pane.editor.read(cx);
        let text = editor.value().to_string();
        let range = editor.selected_range();
        let start = position::position_of(&text, range.start);
        let end = position::position_of(&text, range.end);
        Some(json!({
            "text": text.get(range.clone()).unwrap_or_default(),
            "filePath": path,
            "fileUrl": pane.uri,
            "selection": {
                "start": { "line": start.line, "character": start.character },
                "end": { "line": end.line, "character": end.character },
                "isEmpty": range.is_empty(),
            },
        }))
    }

    /// `selection_changed`, when the file or the selection changed.
    pub(super) fn send_selection_to_claude(&mut self, cx: &mut Context<Self>) {
        let Some(pane) = self.panes.get(self.active) else {
            return;
        };
        let now = (pane.uri.clone(), pane.editor.read(cx).selected_range());
        if self
            .claude_ide
            .as_ref()
            .is_none_or(|ide| ide.seen.as_ref() == Some(&now))
        {
            return;
        }
        let selection = self.claude_selection(cx);
        let Some(ide) = self.claude_ide.as_mut() else {
            return;
        };
        ide.seen = Some(now);
        if let Some(selection) = selection {
            ide.server.notify("selection_changed", selection.clone());
            ide.latest = Some(selection);
        }
    }

    /// Cmd+Alt+K: puts the file, and the selected lines, in Claude Code's
    /// prompt.
    fn insert_at_mention(&mut self, cx: &mut Context<Self>) {
        let Some(pane) = self.panes.get(self.active) else {
            return;
        };
        let Some(path) = Uri::parse(&pane.uri).and_then(|uri| uri.file_path()) else {
            return;
        };
        let editor = pane.editor.read(cx);
        let text = editor.value().to_string();
        let range = editor.selected_range();
        let mut params = json!({ "filePath": path });
        if !range.is_empty() {
            params["lineStart"] = json!(position::position_of(&text, range.start).line);
            params["lineEnd"] = json!(position::position_of(&text, range.end).line);
        }
        match &self.claude_ide {
            Some(ide) if ide.connected => ide.server.notify("at_mentioned", params),
            _ => self.status_message = Some(self.strings().claude_not_connected.to_owned()),
        }
        cx.notify();
    }

    /// Tells Claude Code that the problems of `uri` changed.
    pub(super) fn claude_diagnostics_changed(&self, uri: &str) {
        if let Some(ide) = self.claude_ide.as_ref().filter(|ide| ide.connected) {
            ide.server
                .notify("diagnostics_changed", json!({ "uris": [uri] }));
        }
    }

    /// The path of a tool argument: absolute, or inside the project.
    fn claude_path(&self, value: &Value) -> Option<PathBuf> {
        let path = PathBuf::from(value.as_str().filter(|path| !path.is_empty())?);
        if path.is_absolute() {
            return Some(path);
        }
        Some(self.project.root.as_ref()?.join(path))
    }

    fn answer_claude_tool(
        &mut self,
        name: &str,
        arguments: &Value,
        reply: oneshot::Sender<Value>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let result = match name {
            "getWorkspaceFolders" => {
                let folders: Vec<Value> = self
                    .project
                    .root
                    .iter()
                    .enumerate()
                    .map(|(index, root)| {
                        json!({
                            "name": root.file_name().map(|name| name.to_string_lossy().into_owned()),
                            "uri": Uri::file(root).to_string(),
                            "path": root,
                            "index": index,
                        })
                    })
                    .collect();
                json_result(&json!({
                    "success": true,
                    "folders": folders,
                    "rootPath": self.project.root,
                    "workspaceFile": null,
                }))
            }
            "getOpenEditors" => json_result(&json!({ "tabs": self.claude_open_editors(cx) })),
            "getCurrentSelection" => match self.claude_selection(cx) {
                Some(selection) => {
                    let mut answer = json!({ "success": true });
                    answer
                        .as_object_mut()
                        .unwrap()
                        .extend(selection.as_object().cloned().unwrap_or_default());
                    json_result(&answer)
                }
                None => {
                    json_result(&json!({ "success": false, "message": "No active editor found" }))
                }
            },
            "getLatestSelection" => {
                match self.claude_ide.as_ref().and_then(|ide| ide.latest.clone()) {
                    Some(selection) => json_result(&selection),
                    None => json_result(
                        &json!({ "success": false, "message": "No selection available" }),
                    ),
                }
            }
            "getDiagnostics" => json_result(&self.claude_diagnostics(arguments["uri"].as_str())),
            "checkDocumentDirty" => match self.claude_path(&arguments["filePath"]) {
                Some(path) => {
                    let uri = Uri::file(&path).to_string();
                    match self.documents.get(&uri) {
                        Some(document) => json_result(&json!({
                            "success": true,
                            "filePath": path,
                            "isDirty": document.modified,
                            "isUntitled": matches!(document.backing, Backing::Untitled),
                        })),
                        None => json_result(
                            &json!({ "success": false, "message": format!("Document not open: {}", path.display()) }),
                        ),
                    }
                }
                None => error_result("File path is required"),
            },
            "saveDocument" => match self.claude_path(&arguments["filePath"]) {
                Some(path) => {
                    let uri = Uri::file(&path).to_string();
                    match self.documents.get(&uri).map(|document| document.modified) {
                        Some(modified) => {
                            if modified {
                                self.save_document(&uri, None, window, cx);
                            }
                            json_result(&json!({
                                "success": true,
                                "filePath": path,
                                "saved": modified,
                                "message": if modified { "Document saved successfully" } else { "Document was not dirty or save failed" },
                            }))
                        }
                        None => json_result(
                            &json!({ "success": false, "message": format!("Document not open: {}", path.display()) }),
                        ),
                    }
                }
                None => error_result("File path is required"),
            },
            "openFile" => self.claude_open_file(arguments, window, cx),
            "openDiff" | "close_tab" | "closeAllDiffTabs" => {
                self.answer_claude_diff_tool(name, arguments, reply, window, cx);
                return;
            }
            _ => error_result(format!("Keel does not offer {name}")),
        };
        let _ = reply.send(result);
    }

    /// The tabs of every group, as `getOpenEditors` lists them.
    fn claude_open_editors(&self, cx: &App) -> Vec<Value> {
        let active_uri = self.panes.get(self.active).map(|pane| pane.uri.clone());
        let mut tabs = Vec::new();
        for (group_index, group) in self.groups.iter().enumerate() {
            for (tab_index, tab) in group.tabs.iter().enumerate() {
                let Some(document) = self.documents.get(&tab.uri) else {
                    continue;
                };
                if !matches!(document.backing, Backing::File(_) | Backing::Untitled) {
                    continue;
                }
                let path = Uri::parse(&tab.uri).and_then(|uri| uri.file_path());
                let label = path
                    .as_ref()
                    .and_then(|path| path.file_name())
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_else(|| document.name());
                let mut entry = json!({
                    "uri": tab.uri,
                    "isActive": tab_index == group.active,
                    "isPinned": tab.pinned,
                    "isPreview": tab.preview,
                    "isDirty": document.modified,
                    "label": label,
                    "groupIndex": group_index,
                    "viewColumn": group_index + 1,
                    "isGroupActive": group_index == self.active,
                    "fileName": path,
                    "languageId": document.language_id,
                    "lineCount": document.synced_text.lines().count().max(1),
                    "isUntitled": matches!(document.backing, Backing::Untitled),
                });
                if active_uri.as_deref() == Some(tab.uri.as_str())
                    && group_index == self.active
                    && let Some(selection) = self.claude_selection(cx)
                {
                    entry["selection"] = selection["selection"].clone();
                }
                tabs.push(entry);
            }
        }
        tabs
    }

    /// The problems of one file or of all, as `getDiagnostics` answers.
    fn claude_diagnostics(&self, uri: Option<&str>) -> Value {
        let severity = |severity: Option<DiagnosticSeverity>| match severity {
            Some(DiagnosticSeverity::WARNING) => "Warning",
            Some(DiagnosticSeverity::INFORMATION) => "Information",
            Some(DiagnosticSeverity::HINT) => "Hint",
            _ => "Error",
        };
        let files: Vec<Value> = self
            .languages
            .diagnostics
            .iter()
            .filter(|(file, _)| uri.is_none_or(|uri| uri == file.as_str()))
            .map(|(file, by_owner)| {
                let diagnostics: Vec<Value> = by_owner
                    .values()
                    .flatten()
                    .map(|diagnostic| {
                        json!({
                            "message": diagnostic.message,
                            "severity": severity(diagnostic.severity),
                            "range": diagnostic.range,
                            "source": diagnostic.source,
                            "code": diagnostic.code,
                        })
                    })
                    .collect();
                let lines = self
                    .documents
                    .get(file)
                    .map(|document| document.synced_text.lines().count());
                json!({ "uri": file, "linesInFile": lines, "diagnostics": diagnostics })
            })
            .collect();
        Value::Array(files)
    }

    /// `openFile`: opens the file and selects from `startText` to `endText`.
    fn claude_open_file(
        &mut self,
        arguments: &Value,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Value {
        let Some(path) = self.claude_path(&arguments["filePath"]) else {
            return error_result("File path is required");
        };
        if !path.is_file() {
            return error_result(format!("File not found: {}", path.display()));
        }
        self.open_file(&path, window, cx);
        let Some(start_text) = arguments["startText"]
            .as_str()
            .filter(|text| !text.is_empty())
        else {
            return text_result(format!("Opened file: {}", path.display()));
        };
        let Some(editor) = self.active_editor() else {
            return text_result(format!("Opened file: {}", path.display()));
        };
        let text = editor.read(cx).value().to_string();
        let Some(start) = text.find(start_text) else {
            return text_result(format!(
                "Opened file: {} (text not found: {start_text})",
                path.display()
            ));
        };
        let mut end = start + start_text.len();
        if let Some(end_text) = arguments["endText"]
            .as_str()
            .filter(|text| !text.is_empty())
            && let Some(found) = text[end..].find(end_text)
        {
            end += found + end_text.len();
        }
        if arguments["selectToEndOfLine"].as_bool() == Some(true) {
            end = text[end..].find('\n').map_or(text.len(), |at| end + at);
        }
        editor.update(cx, |editor, cx| {
            editor.set_selected_range(start..end, cx);
            if arguments["makeFrontmost"].as_bool() != Some(false) {
                editor.focus(window, cx);
            }
        });
        self.send_selection_to_claude(cx);
        text_result(format!("Opened file and selected text: {}", path.display()))
    }

    pub(super) fn claude_ide_actions(&self, root: gpui::Div, cx: &mut Context<Self>) -> gpui::Div {
        root.on_action(cx.listener(|this, _: &InsertAtMention, _, cx| this.insert_at_mention(cx)))
    }
}

/// For tests: the token of a lock file.
#[cfg(test)]
pub(super) fn token_of(lock: &std::path::Path) -> String {
    let lock: Value = serde_json::from_str(&std::fs::read_to_string(lock).unwrap()).unwrap();
    lock["authToken"].as_str().unwrap().to_owned()
}

#[cfg(test)]
mod tests {
    use std::net::TcpStream;
    use std::time::Instant;

    use gpui::{TestAppContext, VisualTestContext};
    use tungstenite::client::IntoClientRequest as _;
    use tungstenite::stream::MaybeTlsStream;
    use tungstenite::{Message, WebSocket};

    use super::super::tests::test_settings;
    use super::*;

    type Socket = WebSocket<MaybeTlsStream<TcpStream>>;

    /// Reads what Keel sends until `wanted` matches, while the window runs.
    fn receive(
        cx: &mut VisualTestContext,
        socket: &mut Socket,
        wanted: impl Fn(&Value) -> bool,
    ) -> Value {
        let mut seen = Vec::new();
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            cx.executor().advance_clock(Duration::from_millis(100));
            cx.run_until_parked();
            match socket.read() {
                Ok(Message::Text(text)) => {
                    let message: Value = serde_json::from_str(&text).unwrap();
                    if wanted(&message) {
                        return message;
                    }
                    seen.push(message);
                }
                Ok(_) => {}
                Err(tungstenite::Error::Io(error))
                    if matches!(
                        error.kind(),
                        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                    ) => {}
                Err(error) => panic!("{error}"),
            }
            assert!(
                Instant::now() < deadline,
                "Keel did not answer; it sent {seen:?}"
            );
        }
    }

    fn call_tool(
        cx: &mut VisualTestContext,
        socket: &mut Socket,
        id: u64,
        name: &str,
        arguments: Value,
    ) -> Value {
        let request = json!({ "jsonrpc": "2.0", "id": id, "method": "tools/call", "params": { "name": name, "arguments": arguments } });
        socket.send(Message::text(request.to_string())).unwrap();
        let answer = receive(cx, socket, |message| message["id"] == json!(id));
        let text = answer["result"]["content"][0]["text"]
            .as_str()
            .unwrap_or_default()
            .to_owned();
        serde_json::from_str(&text).unwrap_or(Value::String(text))
    }

    #[gpui::test]
    fn claude_code_sees_the_window_and_is_told_where_the_cursor_is(cx: &mut TestAppContext) {
        cx.executor().allow_parking();
        cx.update(|cx| {
            gpui_component::init(cx);
            language::register_grammars();
            init(cx);
        });
        let project = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(project.path().join(".git")).unwrap();
        let path = project.path().join("booking.ts");
        std::fs::write(&path, "const nights = 3;\nconst price = nights * 90;\n").unwrap();
        let ide_dir = tempfile::tempdir().unwrap();
        let document = Document::open(&path).unwrap();
        let (view, cx) = cx.add_window_view(|window, cx| {
            let mut workbench = Workbench::new(Some(document), test_settings(), None, window, cx);
            workbench.start_claude_ide(ide_dir.path().to_path_buf(), window, cx);
            workbench
        });

        // The terminals carry the port; the lock file the token.
        let (port, lock) = view.read_with(cx, |view, _| {
            let ide = view.claude_ide.as_ref().unwrap();
            assert!(
                view.claude_ide_env()
                    .contains(&(ide::PORT_VARIABLE.to_owned(), ide.server.port().to_string()))
            );
            (ide.server.port(), ide.server.lock_file().to_path_buf())
        });
        let announced: Value =
            serde_json::from_str(&std::fs::read_to_string(&lock).unwrap()).unwrap();
        assert_eq!(
            announced["workspaceFolders"][0].as_str().unwrap(),
            std::path::absolute(project.path())
                .unwrap()
                .to_str()
                .unwrap()
        );
        let mut request = format!("ws://127.0.0.1:{port}")
            .into_client_request()
            .unwrap();
        request
            .headers_mut()
            .insert(ide::AUTH_HEADER, token_of(&lock).parse().unwrap());
        let (mut socket, _) = tungstenite::connect(request).unwrap();
        if let MaybeTlsStream::Plain(stream) = socket.get_ref() {
            stream
                .set_read_timeout(Some(Duration::from_millis(20)))
                .unwrap();
        }

        let folders = call_tool(cx, &mut socket, 1, "getWorkspaceFolders", json!({}));
        assert_eq!(folders["success"], true);
        let editors = call_tool(cx, &mut socket, 2, "getOpenEditors", json!({}));
        assert_eq!(editors["tabs"][0]["label"], "booking.ts");
        assert_eq!(editors["tabs"][0]["languageId"], "typescript");
        assert_eq!(editors["tabs"][0]["isActive"], true);

        // Moving the selection is told to Claude Code.
        view.update_in(cx, |view, window, cx| {
            view.active_editor().unwrap().update(cx, |editor, cx| {
                editor.set_selected_range(6..12, cx);
                let _ = window;
            });
        });
        // The first one may be the cursor where the file opened.
        let changed = receive(cx, &mut socket, |message| {
            message["method"] == "selection_changed" && message["params"]["text"] != ""
        });
        assert_eq!(changed["params"]["text"], "nights");
        assert_eq!(
            changed["params"]["selection"]["start"],
            json!({ "line": 0, "character": 6 })
        );
        let selection = call_tool(cx, &mut socket, 3, "getCurrentSelection", json!({}));
        assert_eq!(selection["success"], true);
        assert_eq!(selection["text"], "nights");

        // Cmd+Alt+K: the file and the lines.
        view.update_in(cx, |view, _, cx| view.insert_at_mention(cx));
        let mention = receive(cx, &mut socket, |message| {
            message["method"] == "at_mentioned"
        });
        assert_eq!(mention["params"]["lineStart"], 0);

        // openFile selects the text asked for; the document is not dirty.
        let opened = call_tool(
            cx,
            &mut socket,
            4,
            "openFile",
            json!({ "filePath": "booking.ts", "startText": "price", "endText": "90" }),
        );
        assert!(
            opened
                .as_str()
                .unwrap()
                .starts_with("Opened file and selected text"),
            "{opened}"
        );
        view.read_with(cx, |view, cx| {
            let editor = view.active_editor().unwrap().read(cx);
            assert_eq!(
                &editor.value()[editor.selected_range()],
                "price = nights * 90"
            );
        });
        let dirty = call_tool(
            cx,
            &mut socket,
            5,
            "checkDocumentDirty",
            json!({ "filePath": path }),
        );
        assert_eq!(dirty["isDirty"], false);
        let problems = call_tool(cx, &mut socket, 6, "getDiagnostics", json!({}));
        assert_eq!(problems, json!([]));

        // Closing the window takes the announcement away.
        view.update(cx, |view, _| view.claude_ide = None);
        assert!(!lock.exists());
    }

    fn send_tool(socket: &mut Socket, id: u64, name: &str, arguments: Value) {
        let request = json!({ "jsonrpc": "2.0", "id": id, "method": "tools/call", "params": { "name": name, "arguments": arguments } });
        socket.send(Message::text(request.to_string())).unwrap();
    }

    fn texts(answer: &Value) -> Vec<String> {
        answer["result"]["content"]
            .as_array()
            .unwrap()
            .iter()
            .map(|item| item["text"].as_str().unwrap().to_owned())
            .collect()
    }

    #[gpui::test]
    fn proposed_edits_are_accepted_or_rejected_in_keel(cx: &mut TestAppContext) {
        cx.executor().allow_parking();
        cx.update(|cx| {
            gpui_component::init(cx);
            language::register_grammars();
            init(cx);
        });
        let project = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(project.path().join(".git")).unwrap();
        let path = project.path().join("booking.ts");
        std::fs::write(&path, "const nights = 3;\nconst price = nights * 90;\n").unwrap();
        let ide_dir = tempfile::tempdir().unwrap();
        let document = Document::open(&path).unwrap();
        let (view, cx) = cx.add_window_view(|window, cx| {
            let mut workbench = Workbench::new(Some(document), test_settings(), None, window, cx);
            workbench.start_claude_ide(ide_dir.path().to_path_buf(), window, cx);
            workbench
        });
        let (port, lock) = view.read_with(cx, |view, _| {
            let ide = view.claude_ide.as_ref().unwrap();
            (ide.server.port(), ide.server.lock_file().to_path_buf())
        });
        let mut request = format!("ws://127.0.0.1:{port}")
            .into_client_request()
            .unwrap();
        request
            .headers_mut()
            .insert(ide::AUTH_HEADER, token_of(&lock).parse().unwrap());
        let (mut socket, _) = tungstenite::connect(request).unwrap();
        if let MaybeTlsStream::Plain(stream) = socket.get_ref() {
            stream
                .set_read_timeout(Some(Duration::from_millis(20)))
                .unwrap();
        }
        let proposed = "const nights = 3;\nconst price = nights * 95;\n";
        let open = |name: &str| {
            json!({
                "old_file_path": path, "new_file_path": path,
                "new_file_contents": proposed, "tab_name": name,
            })
        };

        // The proposal opens as differences, and Claude Code waits.
        send_tool(&mut socket, 1, "openDiff", open("✻ booking.ts"));
        let deadline = Instant::now() + Duration::from_secs(10);
        let uri = loop {
            cx.executor().advance_clock(Duration::from_millis(100));
            cx.run_until_parked();
            let active = view.read_with(cx, |view, _| view.panes[view.active].uri.clone());
            if active.starts_with(super::super::claude_diff::PROPOSED_SCHEME) {
                break active;
            }
            assert!(Instant::now() < deadline, "the proposal did not open");
            std::thread::sleep(Duration::from_millis(20));
        };
        view.read_with(cx, |view, cx| {
            let Some(Backing::Diff(tab)) =
                view.documents.get(&uri).map(|document| &document.backing)
            else {
                panic!("not a diff");
            };
            assert_eq!(tab.title, "✻ booking.ts");
            let files = tab.state.read(cx).files();
            assert_eq!((files[0].additions(), files[0].deletions()), (1, 1));
        });

        // Accept: Claude Code gets the text, the tab goes away.
        view.update_in(cx, |view, window, cx| {
            window.dispatch_action(Box::new(super::super::claude_diff::AcceptProposedDiff), cx);
            if view.claude_diffs.iter().any(|edit| edit.uri == uri) {
                view.answer_claude_diff_for_tests(&uri, true, window, cx);
            }
        });
        let accepted = receive(cx, &mut socket, |message| message["id"] == 1);
        assert_eq!(texts(&accepted), ["FILE_SAVED", proposed]);
        view.read_with(cx, |view, _| {
            assert!(!view.documents.contains_key(&uri));
            assert!(view.claude_diffs.is_empty());
        });

        // Closing the tab is a no.
        send_tool(&mut socket, 2, "openDiff", open("second"));
        let deadline = Instant::now() + Duration::from_secs(10);
        while view.read_with(cx, |view, _| view.claude_diffs.is_empty()) {
            assert!(Instant::now() < deadline);
            cx.executor().advance_clock(Duration::from_millis(100));
            cx.run_until_parked();
            std::thread::sleep(Duration::from_millis(20));
        }
        view.update_in(cx, |view, window, cx| {
            let (group, tab) = view
                .find_tab(&view.claude_diffs[0].uri.clone(), view.active)
                .unwrap();
            view.close_tab_now(group, tab, window, cx);
        });
        let rejected = receive(cx, &mut socket, |message| message["id"] == 2);
        assert_eq!(texts(&rejected), ["DIFF_REJECTED", "second"]);

        // closeAllDiffTabs rejects what still waits.
        send_tool(&mut socket, 3, "openDiff", open("third"));
        let deadline = Instant::now() + Duration::from_secs(10);
        while view.read_with(cx, |view, _| view.claude_diffs.is_empty()) {
            assert!(Instant::now() < deadline);
            cx.executor().advance_clock(Duration::from_millis(100));
            cx.run_until_parked();
            std::thread::sleep(Duration::from_millis(20));
        }
        send_tool(&mut socket, 4, "closeAllDiffTabs", json!({}));
        // Both answers come from their own threads, in either order.
        let mut answers: Vec<Value> = Vec::new();
        while answers.len() < 2 {
            let answer = receive(cx, &mut socket, |message| {
                message["id"] == 3 || message["id"] == 4
            });
            answers.push(answer);
        }
        let answer = |id: u64| answers.iter().find(|answer| answer["id"] == id).unwrap();
        assert_eq!(texts(answer(3))[0], "DIFF_REJECTED");
        assert_eq!(texts(answer(4)), ["CLOSED_1_DIFF_TABS"]);
        // The file itself is untouched: Claude Code writes it.
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "const nights = 3;\nconst price = nights * 90;\n"
        );
    }
}
