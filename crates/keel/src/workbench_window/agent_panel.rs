//! The agent panel: Claude Code as a conversation on the right, through
//! the Agent Client Protocol, next to Claude Code in a terminal. Keel
//! installs the `claude-agent-acp` adapter once, starts it in the project,
//! and shows the conversation: messages, reasoning, tools with their file
//! differences, the plan, permission requests, the mode and past
//! conversations. Files and selections go along with a message.

use std::collections::HashSet;

use ai::acp::{self, Agent, AgentCommand, AgentEvent};
use ai::thread::{Entry, Thread, ToolCall, ToolContent, ToolStatus};
use futures::StreamExt as _;
use gpui::{AnyElement, ScrollHandle, StatefulInteractiveElement as _, Subscription};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::input::{InputEvent, Textarea, TextareaState};
use gpui_component::menu::DropdownMenu as _;
use gpui_component::text::{TextView, TextViewState};
use gpui_component::{ActiveTheme as _, Disableable as _, Icon, Sizable as _};
use gpui_kit_assets::IconName;

use super::shell::Colors;
use super::*;

actions!(keel, [OpenAgent, NewAgentConversation, AddToAgent]);

/// How many lines the message box grows to.
const INPUT_ROWS: usize = 8;
/// How many lines of a file difference a tool shows before "…".
const DIFF_LINES: usize = 14;

/// What the panel on the right shows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum AgentView {
    #[default]
    Terminal,
    Agent,
}

#[derive(Clone, Debug, PartialEq)]
pub(super) enum AgentStatus {
    Off,
    Installing,
    Starting,
    Ready,
    Failed(String),
}

/// A permission the agent waits for.
pub(super) struct Permission {
    id: Value,
    pub(super) title: String,
    pub(super) content: Vec<ToolContent>,
    /// Option id, name and kind (`allow_once`, `reject_once`...).
    pub(super) options: Vec<(String, String, String)>,
}

/// A file, or part of one, sent along with the next message.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Attachment {
    pub(super) path: PathBuf,
    /// The lines, from 1, and their text, for a selection.
    pub(super) selection: Option<((u32, u32), String)>,
}

pub(super) struct AgentPanel {
    pub(super) view: AgentView,
    pub(super) status: AgentStatus,
    pub(super) agent: Option<Rc<Agent>>,
    pub(super) session: Option<String>,
    pub(super) thread: Thread,
    pub(super) running: bool,
    pub(super) permissions: Vec<Permission>,
    pub(super) input: Entity<TextareaState>,
    pub(super) attachments: Vec<Attachment>,
    /// The rendered text of each agent message, by entry, and how much of
    /// it is in.
    markdown: HashMap<usize, (Entity<TextViewState>, usize)>,
    /// Modes the agent offers (value, name), and the current one.
    pub(super) modes: Vec<(String, String)>,
    pub(super) mode: Option<String>,
    /// Past conversations in the project: id and title.
    pub(super) history: Vec<(String, String)>,
    open_thoughts: HashSet<usize>,
    scroll: ScrollHandle,
    /// A message written before the session was ready.
    waiting: Option<Vec<Value>>,
    /// A notice under the conversation, such as an error.
    pub(super) notice: Option<String>,
    /// Tests talk to the fake agent instead of the adapter.
    pub(super) fake: bool,
    /// The text files had before the agent first changed them in this
    /// conversation (none for a file it created), in order.
    pub(super) snapshots: Vec<(PathBuf, Option<String>)>,
    /// The kept files that differ now, for the review.
    pub(super) changes: Vec<super::agent_review::AgentChange>,
}

impl AgentPanel {
    pub(super) fn new(
        strings: &'static Strings,
        window: &mut Window,
        cx: &mut Context<Workbench>,
    ) -> (Self, Subscription) {
        let input = cx.new(|cx| {
            TextareaState::new(window, cx)
                .placeholder(strings.agent_placeholder)
                .rows(2)
                .submit_on_enter(true)
        });
        let events = cx.subscribe_in(
            &input,
            window,
            |this, input, event: &InputEvent, window, cx| match event {
                InputEvent::PressEnter { shift: false, .. } => this.send_to_agent(window, cx),
                InputEvent::Change => {
                    let lines = input
                        .read(cx)
                        .value()
                        .split('\n')
                        .count()
                        .clamp(2, INPUT_ROWS);
                    input.update(cx, |input, cx| input.set_rows(lines, cx));
                }
                _ => {}
            },
        );
        let panel = Self {
            view: AgentView::default(),
            status: AgentStatus::Off,
            agent: None,
            session: None,
            thread: Thread::default(),
            running: false,
            permissions: Vec::new(),
            input,
            attachments: Vec::new(),
            markdown: HashMap::new(),
            modes: Vec::new(),
            mode: None,
            history: Vec::new(),
            open_thoughts: HashSet::new(),
            scroll: ScrollHandle::new(),
            waiting: None,
            notice: None,
            fake: false,
            snapshots: Vec::new(),
            changes: Vec::new(),
        };
        (panel, events)
    }

    /// Brings the rendered messages up to date with the thread.
    fn sync_markdown(&mut self, cx: &mut Context<Workbench>) {
        for (index, entry) in self.thread.entries.iter().enumerate() {
            let Entry::Agent(text) = entry else { continue };
            match self.markdown.get_mut(&index) {
                Some((state, shown)) if *shown < text.len() => {
                    let added = text[*shown..].to_owned();
                    state.update(cx, |state, cx| state.push_str(&added, cx));
                    *shown = text.len();
                }
                Some(_) => {}
                None => {
                    let state = cx.new(|cx| TextViewState::markdown(text, cx).selectable(true));
                    self.markdown.insert(index, (state, text.len()));
                }
            }
        }
    }

    fn reset_thread(&mut self) {
        self.thread = Thread::default();
        self.markdown.clear();
        self.open_thoughts.clear();
        self.permissions.clear();
        self.notice = None;
    }
}

/// The modes of `session/new`: `configOptions` with category `mode`, or
/// the older `modes`.
fn modes_of(session: &Value) -> (Vec<(String, String)>, Option<String>) {
    if let Some(option) = session["configOptions"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|option| option["category"] == "mode" || option["id"] == "mode")
    {
        let modes = option["options"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|mode| {
                Some((
                    mode["value"].as_str()?.to_owned(),
                    mode["name"].as_str()?.to_owned(),
                ))
            })
            .collect();
        return (modes, option["currentValue"].as_str().map(str::to_owned));
    }
    let modes = session["modes"]["availableModes"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|mode| {
            Some((
                mode["id"].as_str()?.to_owned(),
                mode["name"].as_str()?.to_owned(),
            ))
        })
        .collect();
    (
        modes,
        session["modes"]["currentModeId"]
            .as_str()
            .map(str::to_owned),
    )
}

impl Workbench {
    /// The folder the agent works in.
    fn agent_folder(&self) -> PathBuf {
        self.project
            .root
            .clone()
            .or_else(|| std::env::var_os("HOME").map(PathBuf::from))
            .unwrap_or_else(|| PathBuf::from("/"))
    }

    /// Shows the agent and starts it if needed.
    pub(super) fn open_agent(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.layout.agent = true;
        self.agent_panel.view = AgentView::Agent;
        self.ensure_agent(window, cx);
        self.agent_panel
            .input
            .update(cx, |input, cx| input.focus(window, cx));
        self.session_changed(cx);
        cx.notify();
    }

    /// Starts the adapter, installing it the first time, then opens a
    /// session.
    pub(super) fn ensure_agent(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if matches!(
            self.agent_panel.status,
            AgentStatus::Installing | AgentStatus::Starting | AgentStatus::Ready
        ) {
            return;
        }
        self.agent_panel.status = AgentStatus::Starting;
        self.agent_panel.notice = None;
        let cwd = self.agent_folder();
        if self.agent_panel.fake {
            match ai::fake_agent::spawn().and_then(|(reader, writer)| {
                Agent::over(reader, writer, None::<std::io::Empty>, None)
            }) {
                Ok((agent, events)) => self.agent_started(agent, events, cwd, window, cx),
                Err(error) => self.agent_panel.status = AgentStatus::Failed(error.to_string()),
            }
            return;
        }
        let Some(servers) = self.data_dir.as_deref().map(crate::paths::servers_dir) else {
            self.agent_panel.status = AgentStatus::Failed(self.strings().agent_no_data.to_owned());
            return;
        };
        let installed = acp::find_adapter(&servers).is_some();
        if !installed {
            self.agent_panel.status = AgentStatus::Installing;
            self.log(format!(
                "Claude: npm install {}@{}",
                acp::ADAPTER_PACKAGE,
                acp::ADAPTER_VERSION
            ));
        }
        let folder = cwd.clone();
        let start = cx.background_spawn(async move {
            let env = lsp::servers::login_environment().clone();
            let program = match acp::find_adapter(&servers) {
                Some(program) => program,
                None => {
                    let npm = lsp::servers::which_in("npm", &env).ok_or_else(|| {
                        anyhow::anyhow!("npm is not installed: install Node.js first")
                    })?;
                    acp::install_adapter(&servers, &npm, &env)?;
                    acp::find_adapter(&servers)
                        .ok_or_else(|| anyhow::anyhow!("the adapter did not install"))?
                }
            };
            let command = AgentCommand {
                program,
                args: Vec::new(),
                env,
                cwd: folder,
            };
            anyhow::Ok(Agent::start(&command)?)
        });
        cx.spawn_in(window, async move |this, cx| {
            let started = start.await;
            this.update_in(cx, |this, window, cx| match started {
                Ok((agent, events)) => this.agent_started(agent, events, cwd, window, cx),
                Err(error) => {
                    this.log(format!("Claude: {error:#}"));
                    this.agent_panel.status = AgentStatus::Failed(format!("{error:#}"));
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    fn agent_started(
        &mut self,
        agent: Agent,
        mut events: futures::channel::mpsc::UnboundedReceiver<AgentEvent>,
        cwd: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let agent = Rc::new(agent);
        self.agent_panel.agent = Some(agent.clone());
        self.agent_panel.status = AgentStatus::Starting;
        cx.spawn_in(window, async move |this, cx| {
            while let Some(event) = events.next().await {
                if this
                    .update_in(cx, |this, window, cx| {
                        this.on_agent_event(event, window, cx)
                    })
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();
        let initialize = agent.initialize();
        cx.spawn_in(window, async move |this, cx| {
            let ready = initialize.await;
            let session = match ready {
                Ok(_) => agent.new_session(&cwd).await,
                Err(error) => Err(error),
            };
            this.update_in(cx, |this, window, cx| {
                match session {
                    Ok(session) => {
                        this.agent_panel.session = session["sessionId"].as_str().map(str::to_owned);
                        let (modes, mode) = modes_of(&session);
                        this.agent_panel.modes = modes;
                        this.agent_panel.mode = mode;
                        this.agent_panel.status = AgentStatus::Ready;
                        if let Some(blocks) = this.agent_panel.waiting.take() {
                            this.prompt_agent(blocks, window, cx);
                        }
                        this.refresh_agent_history(window, cx);
                    }
                    Err(error) => {
                        this.log(format!("Claude: {error:#}"));
                        this.agent_panel.status = AgentStatus::Failed(format!("{error:#}"));
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn on_agent_event(&mut self, event: AgentEvent, window: &mut Window, cx: &mut Context<Self>) {
        match event {
            AgentEvent::Notification { method, params } => {
                if method == "session/update"
                    && params["sessionId"].as_str() == self.agent_panel.session.as_deref()
                {
                    let update = &params["update"];
                    self.before_agent_update(update, cx);
                    self.agent_panel.thread.apply(update);
                    if update["sessionUpdate"] == "config_option_update" {
                        let (modes, mode) = modes_of(update);
                        if !modes.is_empty() {
                            self.agent_panel.modes = modes;
                            self.agent_panel.mode = mode;
                        }
                    }
                    if let Some(mode) = &self.agent_panel.thread.mode
                        && update["sessionUpdate"] == "current_mode_update"
                    {
                        self.agent_panel.mode = Some(mode.clone());
                    }
                    self.agent_panel.sync_markdown(cx);
                    if update["sessionUpdate"] == "tool_call_update" {
                        self.refresh_agent_changes();
                    }
                    self.agent_panel.scroll.scroll_to_bottom();
                }
            }
            AgentEvent::Request { id, method, params } => {
                self.answer_agent(id, &method, &params, window, cx)
            }
            AgentEvent::Exited { stderr } => {
                let tail: String = stderr
                    .lines()
                    .rev()
                    .take(3)
                    .collect::<Vec<_>>()
                    .into_iter()
                    .rev()
                    .collect::<Vec<_>>()
                    .join("\n");
                self.log(format!("Claude: the agent stopped. {tail}"));
                self.agent_panel.agent = None;
                self.agent_panel.session = None;
                self.agent_panel.running = false;
                self.agent_panel.permissions.clear();
                self.agent_panel.status =
                    AgentStatus::Failed(self.strings().agent_stopped.to_owned());
            }
        }
        cx.notify();
    }

    /// What the agent asks of Keel: files and permissions.
    fn answer_agent(
        &mut self,
        id: Value,
        method: &str,
        params: &Value,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(agent) = self.agent_panel.agent.clone() else {
            return;
        };
        match method {
            "fs/read_text_file" => {
                let path = PathBuf::from(params["path"].as_str().unwrap_or_default());
                let uri = Uri::file(&path).to_string();
                // What the editor holds, with unsaved changes.
                let text = match self.documents.get(&uri) {
                    Some(document) if matches!(document.backing, Backing::File(_)) => Ok(self
                        .panes
                        .iter()
                        .find(|pane| pane.uri == uri)
                        .map(|pane| pane.editor.read(cx).value().to_string())
                        .unwrap_or_else(|| document.synced_text.clone())),
                    _ => std::fs::read_to_string(&path).map_err(|error| error.to_string()),
                };
                let answer = text
                    .map(|text| {
                        let line = params["line"].as_u64().map(|line| line.max(1) as usize);
                        let limit = params["limit"].as_u64().map(|limit| limit as usize);
                        let content = match (line, limit) {
                            (None, None) => text,
                            (line, limit) => text
                                .split_inclusive('\n')
                                .skip(line.unwrap_or(1) - 1)
                                .take(limit.unwrap_or(usize::MAX))
                                .collect(),
                        };
                        json!({ "content": content })
                    })
                    .map_err(|error| (-32603, error));
                let _ = agent.respond(id, answer);
            }
            "fs/write_text_file" => {
                let path = PathBuf::from(params["path"].as_str().unwrap_or_default());
                self.remember_before_agent_edit(&path);
                let written = path
                    .parent()
                    .map(std::fs::create_dir_all)
                    .transpose()
                    .and_then(|_| {
                        std::fs::write(&path, params["content"].as_str().unwrap_or_default())
                    });
                let _ = agent.respond(
                    id,
                    written
                        .map(|()| Value::Null)
                        .map_err(|error| (-32603, error.to_string())),
                );
                self.refresh_agent_changes();
            }
            "session/request_permission" => {
                let tool = &params["toolCall"];
                let content: Vec<ToolContent> = {
                    let mut thread = Thread::default();
                    thread.apply(
                        &json!({ "sessionUpdate": "tool_call", "toolCallId": "permission",
                        "title": tool["title"], "content": tool["content"] }),
                    );
                    thread
                        .tool("permission")
                        .map(|tool| tool.content.clone())
                        .unwrap_or_default()
                };
                let title = tool["title"]
                    .as_str()
                    .map(str::to_owned)
                    .or_else(|| {
                        tool["toolCallId"]
                            .as_str()
                            .and_then(|call| self.agent_panel.thread.tool(call))
                            .map(|call| call.title.clone())
                    })
                    .unwrap_or_default();
                let options = params["options"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|option| {
                        Some((
                            option["optionId"].as_str()?.to_owned(),
                            option["name"].as_str().unwrap_or_default().to_owned(),
                            option["kind"].as_str().unwrap_or_default().to_owned(),
                        ))
                    })
                    .collect();
                self.agent_panel.permissions.push(Permission {
                    id,
                    title,
                    content,
                    options,
                });
                self.agent_panel.scroll.scroll_to_bottom();
            }
            _ => {
                let _ = agent.respond(id, Err((-32601, format!("Keel does not offer {method}"))));
            }
        }
    }

    /// Keeps the text `path` has now, the first time the agent is about to
    /// change it, so the change can be undone.
    pub(super) fn remember_before_agent_edit(&mut self, path: &Path) {
        if self
            .agent_panel
            .snapshots
            .iter()
            .any(|(known, _)| known == path)
        {
            return;
        }
        let before = std::fs::read_to_string(path).ok();
        self.agent_panel
            .snapshots
            .push((path.to_path_buf(), before));
    }

    /// A tool about to change files: Keel keeps their text first.
    fn before_agent_update(&mut self, update: &Value, _cx: &mut Context<Self>) {
        if !matches!(
            update["sessionUpdate"].as_str(),
            Some("tool_call" | "tool_call_update")
        ) {
            return;
        }
        let paths: Vec<PathBuf> = update["content"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|item| item["type"] == "diff")
            .filter_map(|item| item["path"].as_str().map(PathBuf::from))
            .collect();
        for path in paths {
            self.remember_before_agent_edit(&path);
        }
    }

    /// Answers a permission with one of its options.
    pub(super) fn answer_permission(&mut self, index: usize, option: &str, cx: &mut Context<Self>) {
        if index >= self.agent_panel.permissions.len() {
            return;
        }
        let permission = self.agent_panel.permissions.remove(index);
        if let Some(agent) = &self.agent_panel.agent {
            let _ = agent.respond(
                permission.id,
                Ok(json!({ "outcome": { "outcome": "selected", "optionId": option } })),
            );
        }
        cx.notify();
    }

    /// Enter in the message box: sends the text and the attachments.
    pub(super) fn send_to_agent(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let text = self.agent_panel.input.read(cx).value().trim().to_owned();
        if (text.is_empty() && self.agent_panel.attachments.is_empty()) || self.agent_panel.running
        {
            return;
        }
        let mut blocks = vec![acp::text_block(&text)];
        let mut shown = text.clone();
        for attachment in std::mem::take(&mut self.agent_panel.attachments) {
            let name = attachment
                .path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default();
            match attachment.selection {
                Some((lines, selected)) => {
                    blocks.push(acp::selection_block(&attachment.path, lines, &selected));
                    shown.push_str(&format!(" @{name}:{}-{}", lines.0, lines.1));
                }
                None => {
                    blocks.push(acp::file_block(&attachment.path, None));
                    shown.push_str(&format!(" @{name}"));
                }
            }
        }
        self.agent_panel.input.update(cx, |input, cx| {
            input.set_value("", window, cx);
            input.set_rows(2, cx);
        });
        self.agent_panel.thread.push_user(shown.trim());
        self.agent_panel.notice = None;
        if self.agent_panel.session.is_none() {
            self.agent_panel.waiting = Some(blocks);
            self.ensure_agent(window, cx);
        } else {
            self.prompt_agent(blocks, window, cx);
        }
        self.agent_panel.scroll.scroll_to_bottom();
        cx.notify();
    }

    fn prompt_agent(&mut self, blocks: Vec<Value>, window: &mut Window, cx: &mut Context<Self>) {
        let (Some(agent), Some(session)) = (
            self.agent_panel.agent.clone(),
            self.agent_panel.session.clone(),
        ) else {
            return;
        };
        self.agent_panel.running = true;
        let turn = agent.prompt(&session, blocks);
        cx.spawn_in(window, async move |this, cx| {
            let result = turn.await;
            this.update_in(cx, |this, window, cx| {
                this.agent_panel.running = false;
                this.refresh_agent_changes();
                match result {
                    Ok(answer) => {
                        let stop = answer["stopReason"].as_str().unwrap_or("end_turn");
                        if stop != "end_turn" {
                            this.agent_panel.notice = Some(this.strings().agent_stop_reason(stop));
                        }
                    }
                    Err(error) => this.agent_panel.notice = Some(format!("{error:#}")),
                }
                this.refresh_agent_history(window, cx);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// The stop button: the turn ends, and permissions waiting are
    /// cancelled, as the protocol wants.
    pub(super) fn cancel_agent(&mut self, cx: &mut Context<Self>) {
        let (Some(agent), Some(session)) = (
            self.agent_panel.agent.clone(),
            self.agent_panel.session.clone(),
        ) else {
            return;
        };
        for permission in std::mem::take(&mut self.agent_panel.permissions) {
            let _ = agent.respond(
                permission.id,
                Ok(json!({ "outcome": { "outcome": "cancelled" } })),
            );
        }
        let _ = agent.cancel(&session);
        cx.notify();
    }

    /// A new conversation in the same agent.
    pub(super) fn new_agent_conversation(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(agent) = self.agent_panel.agent.clone() else {
            self.open_agent(window, cx);
            return;
        };
        if self.agent_panel.running {
            self.cancel_agent(cx);
        }
        self.agent_panel.reset_thread();
        self.agent_panel.session = None;
        let opened = agent.new_session(&self.agent_folder());
        cx.spawn_in(window, async move |this, cx| {
            let session = opened.await;
            this.update(cx, |this, cx| {
                match session {
                    Ok(session) => {
                        this.agent_panel.session = session["sessionId"].as_str().map(str::to_owned);
                        let (modes, mode) = modes_of(&session);
                        this.agent_panel.modes = modes;
                        this.agent_panel.mode = mode;
                    }
                    Err(error) => this.agent_panel.notice = Some(format!("{error:#}")),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    /// Reads the list of past conversations in the project.
    fn refresh_agent_history(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(agent) = self.agent_panel.agent.clone() else {
            return;
        };
        let listed = agent.list_sessions(&self.agent_folder());
        cx.spawn_in(window, async move |this, cx| {
            let Ok(listed) = listed.await else { return };
            let history: Vec<(String, String)> = listed["sessions"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|session| {
                    let id = session["sessionId"].as_str()?.to_owned();
                    let title = session["title"]
                        .as_str()
                        .filter(|title| !title.is_empty())
                        .unwrap_or(&id)
                        .to_owned();
                    Some((id, title))
                })
                .take(30)
                .collect();
            this.update(cx, |this, cx| {
                this.agent_panel.history = history;
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Opens a past conversation: the agent replays it.
    pub(super) fn load_agent_conversation(
        &mut self,
        id: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(agent) = self.agent_panel.agent.clone() else {
            return;
        };
        if self.agent_panel.running {
            self.cancel_agent(cx);
        }
        self.agent_panel.reset_thread();
        self.agent_panel.session = Some(id.clone());
        let loaded = agent.load_session(&id, &self.agent_folder());
        cx.spawn_in(window, async move |this, cx| {
            let loaded = loaded.await;
            this.update(cx, |this, cx| {
                match loaded {
                    Ok(session) => {
                        let (modes, mode) = modes_of(&session);
                        if !modes.is_empty() {
                            this.agent_panel.modes = modes;
                            this.agent_panel.mode = mode;
                        }
                    }
                    Err(error) => this.agent_panel.notice = Some(format!("{error:#}")),
                }
                this.agent_panel.scroll.scroll_to_bottom();
                cx.notify();
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    /// Changes how the agent asks permission.
    pub(super) fn set_agent_mode(&mut self, mode: String, cx: &mut Context<Self>) {
        let (Some(agent), Some(session)) = (
            self.agent_panel.agent.clone(),
            self.agent_panel.session.clone(),
        ) else {
            return;
        };
        self.agent_panel.mode = Some(mode.clone());
        let changed = agent.set_config_option(&session, "mode", &mode);
        cx.spawn(async move |this, cx| {
            if let Err(error) = changed.await {
                this.update(cx, |this, cx| {
                    this.agent_panel.notice = Some(format!("{error:#}"));
                    cx.notify();
                })
                .ok();
            }
        })
        .detach();
        cx.notify();
    }

    /// The active file, or its selection, goes with the next message.
    pub(super) fn add_to_agent(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(pane) = self.panes.get(self.active) else {
            return;
        };
        let Some(path) = Uri::parse(&pane.uri).and_then(|uri| uri.file_path()) else {
            return;
        };
        let editor = pane.editor.read(cx);
        let range = editor.selected_range();
        let selection = (!range.is_empty()).then(|| {
            let text = editor.value().to_string();
            let start = position::position_of(&text, range.start).line + 1;
            let end = position::position_of(&text, range.end).line + 1;
            ((start, end), text[range.clone()].to_owned())
        });
        let attachment = Attachment { path, selection };
        if !self.agent_panel.attachments.contains(&attachment) {
            self.agent_panel.attachments.push(attachment);
        }
        self.layout.agent = true;
        self.agent_panel.view = AgentView::Agent;
        self.ensure_agent(window, cx);
        self.agent_panel
            .input
            .update(cx, |input, cx| input.focus(window, cx));
        cx.notify();
    }

    pub(super) fn agent_panel_actions(&self, root: gpui::Div, cx: &mut Context<Self>) -> gpui::Div {
        root.on_action(cx.listener(|this, _: &OpenAgent, window, cx| this.open_agent(window, cx)))
            .on_action(cx.listener(|this, _: &NewAgentConversation, window, cx| {
                this.new_agent_conversation(window, cx)
            }))
            .on_action(
                cx.listener(|this, _: &AddToAgent, window, cx| this.add_to_agent(window, cx)),
            )
            .on_action(
                cx.listener(|this, action: &LoadAgentConversation, window, cx| {
                    this.load_agent_conversation(action.id.clone(), window, cx)
                }),
            )
            .on_action(cx.listener(|this, action: &SetAgentMode, _, cx| {
                this.set_agent_mode(action.mode.clone(), cx)
            }))
    }

    // -----------------------------------------------------------------
    // Drawing

    /// The buttons of the panel's header while the agent is shown.
    pub(super) fn render_agent_header_buttons(&self, cx: &mut Context<Self>) -> AnyElement {
        let strings = self.strings();
        let history = self.agent_panel.history.clone();
        div()
            .flex()
            .items_center()
            .gap_0p5()
            .child(
                Button::new("agent-history")
                    .ghost()
                    .small()
                    .icon(Icon::new(IconName::Clock))
                    .tooltip(strings.agent_history)
                    .disabled(history.is_empty())
                    .dropdown_menu(move |mut menu, _, _| {
                        for (id, title) in &history {
                            let id = id.clone();
                            menu = menu.item(
                                gpui_component::menu::PopupMenuItem::new(title.clone()).on_click(
                                    move |_, window, cx| {
                                        let id = id.clone();
                                        window.dispatch_action(
                                            Box::new(LoadAgentConversation { id }),
                                            cx,
                                        );
                                    },
                                ),
                            );
                        }
                        menu
                    }),
            )
            .child(
                Button::new("agent-new")
                    .ghost()
                    .small()
                    .icon(Icon::new(IconName::Plus))
                    .tooltip(strings.agent_new)
                    .on_click(
                        cx.listener(|this, _, window, cx| this.new_agent_conversation(window, cx)),
                    ),
            )
            .into_any_element()
    }

    pub(super) fn render_agent_conversation(
        &self,
        colors: Colors,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let strings = self.strings();
        let panel = &self.agent_panel;
        let mut body = div()
            .id("agent-conversation")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .track_scroll(&panel.scroll)
            .px_3()
            .py_2()
            .flex()
            .flex_col()
            .gap_2();
        if panel.thread.entries.is_empty() {
            let message = match &panel.status {
                AgentStatus::Installing => strings.agent_installing.to_owned(),
                AgentStatus::Starting => strings.agent_starting.to_owned(),
                AgentStatus::Failed(error) => error.clone(),
                AgentStatus::Off | AgentStatus::Ready => strings.agent_empty.to_owned(),
            };
            body = body.child(
                div()
                    .flex_1()
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .gap_3()
                    .py_8()
                    .text_color(colors.faint)
                    .child(Icon::new(IconName::Sparkles).large())
                    .child(
                        div()
                            .text_xs()
                            .text_center()
                            .text_color(colors.muted)
                            .child(message),
                    )
                    .when(
                        matches!(panel.status, AgentStatus::Failed(_) | AgentStatus::Off),
                        |empty| {
                            empty.child(
                                Button::new("agent-start")
                                    .primary()
                                    .small()
                                    .label(strings.agent_start)
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.open_agent(window, cx)
                                    })),
                            )
                        },
                    ),
            );
        }
        for (index, entry) in panel.thread.entries.iter().enumerate() {
            body = body.child(self.render_agent_entry(index, entry, colors, cx));
        }
        if panel.running && panel.permissions.is_empty() {
            body = body.child(
                div()
                    .text_xs()
                    .text_color(colors.faint)
                    .child(strings.agent_working),
            );
        }
        if let Some(notice) = &panel.notice {
            body = body.child(
                div()
                    .text_xs()
                    .text_color(cx.theme().warning)
                    .child(notice.clone()),
            );
        }
        div()
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .child(body)
            .children(self.render_agent_plan(colors))
            .children(self.render_agent_changes(colors, cx))
            .children(
                panel
                    .permissions
                    .iter()
                    .enumerate()
                    .map(|(index, permission)| {
                        self.render_permission(index, permission, colors, cx)
                    }),
            )
            .child(self.render_agent_input(colors, cx))
            .into_any_element()
    }

    fn render_agent_entry(
        &self,
        index: usize,
        entry: &Entry,
        colors: Colors,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        match entry {
            Entry::User(text) => div()
                .flex()
                .justify_end()
                .child(
                    div()
                        .max_w(px(self.agent_width * 0.85))
                        .px_3()
                        .py_1p5()
                        .rounded(px(10.))
                        .bg(colors.hover)
                        .text_sm()
                        .text_color(colors.text)
                        .child(text.clone()),
                )
                .into_any_element(),
            Entry::Agent(_) => match self.agent_panel.markdown.get(&index) {
                Some((state, _)) => div()
                    .text_sm()
                    .child(TextView::new(state))
                    .into_any_element(),
                None => div().into_any_element(),
            },
            Entry::Thought(text) => {
                let open = self.agent_panel.open_thoughts.contains(&index);
                let first = text.lines().next().unwrap_or_default().to_owned();
                div()
                    .id(("agent-thought", index))
                    .flex()
                    .flex_col()
                    .gap_0p5()
                    .text_xs()
                    .text_color(colors.faint)
                    .cursor_pointer()
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if !this.agent_panel.open_thoughts.remove(&index) {
                            this.agent_panel.open_thoughts.insert(index);
                        }
                        cx.notify();
                    }))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_1()
                            .child(
                                Icon::new(if open {
                                    IconName::ChevronDown
                                } else {
                                    IconName::ChevronRight
                                })
                                .xsmall(),
                            )
                            .child(self.strings().agent_thinking)
                            .when(!open, |line| line.child(div().truncate().child(first))),
                    )
                    .when(open, |thought| {
                        thought.child(div().pl_4().italic().child(text.clone()))
                    })
                    .into_any_element()
            }
            Entry::Tool(tool) => self.render_tool(index, tool, colors, cx),
        }
    }

    fn render_tool(
        &self,
        index: usize,
        tool: &ToolCall,
        colors: Colors,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = cx.theme();
        let icon = match tool.kind.as_str() {
            "edit" | "delete" | "move" => IconName::Pencil,
            "read" => IconName::FileText,
            "search" => IconName::Search,
            "execute" => IconName::SquareTerminal,
            "fetch" => IconName::Globe,
            "think" => IconName::Sparkles,
            _ => IconName::Blocks,
        };
        let (status, status_color) = match tool.status {
            ToolStatus::Pending | ToolStatus::InProgress => ("…", colors.faint),
            ToolStatus::Completed => ("✓", theme.success),
            ToolStatus::Failed => ("✗", theme.danger),
        };
        let location = tool.locations.first().cloned();
        let mut card = div()
            .id(("agent-tool", index))
            .flex()
            .flex_col()
            .gap_1()
            .px_2()
            .py_1p5()
            .rounded(px(8.))
            .border_1()
            .border_color(colors.border)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_1p5()
                    .text_xs()
                    .child(Icon::new(icon).xsmall().text_color(colors.muted))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_color(colors.text)
                            .child(tool.title.clone()),
                    )
                    .child(div().flex_none().text_color(status_color).child(status)),
            );
        if let Some((path, line)) = location {
            card = card
                .cursor_pointer()
                .on_click(cx.listener(move |this, _, window, cx| {
                    if path.is_file() {
                        this.open_file(&path, window, cx);
                        if let Some(line) = line
                            && let Some(editor) = this.active_editor()
                        {
                            editor.update(cx, |editor, cx| {
                                editor.set_cursor_position(
                                    gpui_component::input::Position::new(line.saturating_sub(1), 0),
                                    window,
                                    cx,
                                );
                            });
                        }
                    }
                }));
        }
        for content in &tool.content {
            card = card.child(render_tool_content(
                content,
                colors,
                theme.success,
                theme.danger,
            ));
        }
        card.into_any_element()
    }

    fn render_agent_plan(&self, colors: Colors) -> Option<AnyElement> {
        let plan = &self.agent_panel.thread.plan;
        if plan.is_empty() {
            return None;
        }
        let done = plan
            .iter()
            .filter(|item| item.status == "completed")
            .count();
        Some(
            div()
                .flex_none()
                .mx_3()
                .mb_1()
                .px_2()
                .py_1p5()
                .rounded(px(8.))
                .bg(colors.hover)
                .flex()
                .flex_col()
                .gap_0p5()
                .text_xs()
                .child(
                    div()
                        .text_color(colors.muted)
                        .child(self.strings().agent_plan(done, plan.len())),
                )
                .children(plan.iter().map(|item| {
                    let mark = match item.status.as_str() {
                        "completed" => "✓",
                        "in_progress" => "›",
                        _ => "·",
                    };
                    div()
                        .flex()
                        .gap_1p5()
                        .text_color(if item.status == "completed" {
                            colors.faint
                        } else {
                            colors.text
                        })
                        .child(div().flex_none().w(px(10.)).child(mark))
                        .child(div().flex_1().min_w_0().child(item.content.clone()))
                }))
                .into_any_element(),
        )
    }

    fn render_permission(
        &self,
        index: usize,
        permission: &Permission,
        colors: Colors,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = cx.theme();
        let mut card = div()
            .flex_none()
            .mx_3()
            .mb_1()
            .px_2()
            .py_2()
            .rounded(px(8.))
            .border_1()
            .border_color(colors.accent)
            .flex()
            .flex_col()
            .gap_1p5()
            .child(
                div()
                    .text_xs()
                    .text_color(colors.text)
                    .child(self.strings().agent_permission(&permission.title)),
            );
        for content in &permission.content {
            card = card.child(render_tool_content(
                content,
                colors,
                theme.success,
                theme.danger,
            ));
        }
        let mut buttons = div().flex().flex_wrap().gap_1();
        for (number, (id, name, kind)) in permission.options.iter().enumerate() {
            let option = id.clone();
            let button = Button::new(("agent-permission", index * 16 + number))
                .small()
                .label(name.clone());
            let button = if kind.starts_with("allow") && number == 0 {
                button.primary()
            } else {
                button.ghost()
            };
            buttons = buttons.child(button.on_click(
                cx.listener(move |this, _, _, cx| this.answer_permission(index, &option, cx)),
            ));
        }
        card.child(buttons).into_any_element()
    }

    fn render_agent_input(&self, colors: Colors, cx: &mut Context<Self>) -> AnyElement {
        let strings = self.strings();
        let panel = &self.agent_panel;
        let chips = panel
            .attachments
            .iter()
            .enumerate()
            .map(|(index, attachment)| {
                let name = attachment
                    .path
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_default();
                let label = match &attachment.selection {
                    Some(((start, end), _)) => format!("{name}:{start}-{end}"),
                    None => name,
                };
                div()
                    .flex()
                    .items_center()
                    .gap_1()
                    .px_1p5()
                    .rounded(px(6.))
                    .bg(colors.hover)
                    .text_xs()
                    .text_color(colors.text)
                    .child(label)
                    .child(
                        Button::new(("agent-attachment", index))
                            .ghost()
                            .small()
                            .icon(Icon::new(IconName::X).xsmall())
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if index < this.agent_panel.attachments.len() {
                                    this.agent_panel.attachments.remove(index);
                                }
                                cx.notify();
                            })),
                    )
            });
        let modes = panel.modes.clone();
        let mode_name = panel
            .mode
            .as_ref()
            .and_then(|mode| {
                modes
                    .iter()
                    .find(|(value, _)| value == mode)
                    .map(|(_, name)| name.clone())
            })
            .unwrap_or_else(|| strings.agent_mode.to_owned());
        let send: AnyElement = if panel.running {
            Button::new("agent-stop")
                .ghost()
                .small()
                .icon(Icon::new(IconName::Square))
                .tooltip(strings.agent_stop)
                .on_click(cx.listener(|this, _, _, cx| this.cancel_agent(cx)))
                .into_any_element()
        } else {
            Button::new("agent-send")
                .primary()
                .small()
                .icon(Icon::new(IconName::ArrowUp))
                .tooltip(strings.agent_send)
                .on_click(cx.listener(|this, _, window, cx| this.send_to_agent(window, cx)))
                .into_any_element()
        };
        div()
            .flex_none()
            .m_2()
            .p_1p5()
            .rounded(px(10.))
            .border_1()
            .border_color(colors.border)
            .bg(colors.editor)
            .flex()
            .flex_col()
            .gap_1()
            .when(!panel.attachments.is_empty(), |input| {
                input.child(div().flex().flex_wrap().gap_1().children(chips))
            })
            .child(Textarea::new(&panel.input).appearance(false))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_1()
                    .child(
                        Button::new("agent-add-file")
                            .ghost()
                            .small()
                            .label("@")
                            .tooltip(strings.agent_add_file)
                            .on_click(
                                cx.listener(|this, _, window, cx| this.add_to_agent(window, cx)),
                            ),
                    )
                    .when(!modes.is_empty(), |row| {
                        row.child(
                            Button::new("agent-mode")
                                .ghost()
                                .small()
                                .label(mode_name)
                                .dropdown_menu(move |mut menu, _, _| {
                                    for (value, name) in &modes {
                                        let value = value.clone();
                                        menu = menu.item(
                                            gpui_component::menu::PopupMenuItem::new(name.clone())
                                                .on_click(move |_, window, cx| {
                                                    window.dispatch_action(
                                                        Box::new(SetAgentMode {
                                                            mode: value.clone(),
                                                        }),
                                                        cx,
                                                    );
                                                }),
                                        );
                                    }
                                    menu
                                }),
                        )
                    })
                    .child(div().flex_1())
                    .child(send),
            )
            .into_any_element()
    }
}

/// Opens a past conversation, from the history menu.
#[derive(Clone, Debug, PartialEq, gpui::Action)]
#[action(namespace = agent, no_json)]
pub struct LoadAgentConversation {
    pub id: String,
}

/// Picks a mode, from the mode menu.
#[derive(Clone, Debug, PartialEq, gpui::Action)]
#[action(namespace = agent, no_json)]
pub struct SetAgentMode {
    pub mode: String,
}

/// A tool's text, or a file difference with its removed lines in red and
/// added lines in green.
fn render_tool_content(
    content: &ToolContent,
    colors: Colors,
    added: gpui::Hsla,
    removed: gpui::Hsla,
) -> AnyElement {
    match content {
        ToolContent::Text(text) => {
            let shown: String = text.lines().take(6).collect::<Vec<_>>().join("\n");
            div()
                .text_xs()
                .font_family("Menlo")
                .text_color(colors.muted)
                .child(shown)
                .into_any_element()
        }
        ToolContent::Diff { path, old, new } => {
            let name = path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default();
            let mut lines: Vec<(char, String)> = Vec::new();
            for line in old.as_deref().unwrap_or_default().lines() {
                lines.push(('-', line.to_owned()));
            }
            for line in new.lines() {
                lines.push(('+', line.to_owned()));
            }
            let more = lines.len().saturating_sub(DIFF_LINES);
            div()
                .flex()
                .flex_col()
                .rounded(px(6.))
                .bg(colors.editor)
                .px_1p5()
                .py_1()
                .text_xs()
                .font_family("Menlo")
                .child(div().text_color(colors.faint).child(name))
                .children(lines.into_iter().take(DIFF_LINES).map(|(mark, line)| {
                    let color = if mark == '-' { removed } else { added };
                    div()
                        .text_color(color)
                        .bg(color.opacity(0.08))
                        .child(format!("{mark} {line}"))
                }))
                .when(more > 0, |diff| {
                    diff.child(div().text_color(colors.faint).child(format!("… +{more}")))
                })
                .into_any_element()
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use gpui::{TestAppContext, VisualTestContext};

    use super::super::tests::test_settings;
    use super::*;

    fn wait_until(
        cx: &mut VisualTestContext,
        what: &str,
        check: impl Fn(&mut VisualTestContext) -> bool,
    ) {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            cx.executor().advance_clock(Duration::from_millis(100));
            cx.run_until_parked();
            if check(cx) {
                return;
            }
            assert!(Instant::now() < deadline, "timed out waiting for {what}");
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    fn send(view: &Entity<Workbench>, cx: &mut VisualTestContext, text: &str) {
        view.update_in(cx, |view, window, cx| {
            view.agent_panel
                .input
                .update(cx, |input, cx| input.set_value(text, window, cx));
            view.send_to_agent(window, cx);
        });
    }

    fn last_answer(view: &Entity<Workbench>, cx: &mut VisualTestContext) -> Option<String> {
        view.read_with(cx, |view, _| match view.agent_panel.thread.entries.last() {
            Some(Entry::Agent(text)) if !view.agent_panel.running => Some(text.clone()),
            _ => None,
        })
    }

    #[gpui::test]
    fn the_agent_panel_talks_asks_and_remembers(cx: &mut TestAppContext) {
        cx.executor().allow_parking();
        cx.update(|cx| {
            gpui_component::init(cx);
            language::register_grammars();
            init(cx);
        });
        let project = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(project.path().join(".git")).unwrap();
        let path = std::path::absolute(project.path().join("booking.ts")).unwrap();
        std::fs::write(&path, "const price = 90;\n").unwrap();
        let document = Document::open(&path).unwrap();
        let (view, cx) = cx.add_window_view(|window, cx| {
            let mut workbench = Workbench::new(Some(document), test_settings(), None, window, cx);
            workbench.agent_panel.fake = true;
            workbench
        });

        // A message written before the agent is ready waits for it.
        view.update_in(cx, |view, window, cx| view.open_agent(window, cx));
        send(&view, cx, "ciao");
        wait_until(cx, "the answer", |cx| last_answer(&view, cx).is_some());
        assert_eq!(last_answer(&view, cx).unwrap(), "You said: ciao");
        view.read_with(cx, |view, _| {
            assert_eq!(view.agent_panel.status, AgentStatus::Ready);
            assert_eq!(view.agent_panel.view, AgentView::Agent);
            assert_eq!(view.agent_panel.modes.len(), 2);
            assert_eq!(
                view.agent_panel.thread.entries[0],
                Entry::User("ciao".into())
            );
        });

        // The selection goes along; the edit asks permission first.
        view.update_in(cx, |view, window, cx| {
            view.active_editor()
                .unwrap()
                .update(cx, |editor, cx| editor.set_selected_range(6..11, cx));
            view.add_to_agent(window, cx);
            assert_eq!(
                view.agent_panel.attachments[0].selection,
                Some(((1, 1), "price".to_owned()))
            );
        });
        send(&view, cx, &format!("edit {} 90 95", path.display()));
        wait_until(cx, "the permission", |cx| {
            view.read_with(cx, |view, _| !view.agent_panel.permissions.is_empty())
        });
        view.update(cx, |view, cx| {
            let permission = &view.agent_panel.permissions[0];
            assert_eq!(permission.options.len(), 3);
            assert!(matches!(&permission.content[0], ToolContent::Diff { new, .. } if new == "95"));
            // Keel kept the file as it was before the change.
            assert_eq!(
                view.agent_panel.snapshots,
                [(path.clone(), Some("const price = 90;\n".to_owned()))]
            );
            assert!(view.agent_panel.attachments.is_empty());
            view.answer_permission(0, "allow", cx);
        });
        wait_until(cx, "the edit", |cx| {
            last_answer(&view, cx).as_deref() == Some("Done.")
        });
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "const price = 95;\n"
        );
        view.read_with(cx, |view, _| {
            assert_eq!(view.agent_panel.thread.plan.len(), 2);
            assert_eq!(
                view.agent_panel.thread.tool("edit-1").unwrap().status,
                ToolStatus::Completed
            );
        });

        // The mode changes; past conversations are listed and come back.
        view.update(cx, |view, cx| view.set_agent_mode("plan".to_owned(), cx));
        wait_until(cx, "the history", |cx| {
            view.read_with(cx, |view, _| !view.agent_panel.history.is_empty())
        });
        view.update_in(cx, |view, window, cx| {
            view.new_agent_conversation(window, cx)
        });
        wait_until(cx, "a new session", |cx| {
            view.read_with(cx, |view, _| {
                view.agent_panel.session.as_deref() == Some("session-2")
            })
        });
        view.read_with(cx, |view, _| {
            assert!(view.agent_panel.thread.entries.is_empty())
        });
        view.update_in(cx, |view, window, cx| {
            view.load_agent_conversation("session-1".to_owned(), window, cx)
        });
        wait_until(cx, "the old conversation", |cx| {
            view.read_with(cx, |view, _| view.agent_panel.thread.entries.len() >= 4)
        });
        view.read_with(cx, |view, _| {
            assert_eq!(
                view.agent_panel.thread.entries[0],
                Entry::User("ciao".into())
            );
            assert_eq!(
                view.agent_panel.thread.entries[1],
                Entry::Agent("You said: ciao".into())
            );
        });

        // The panel draws: messages, tools, plan and the message box.
        view.update_in(cx, |view, _, cx| {
            view.agent_panel.open_thoughts.insert(2);
            cx.notify();
        });
        cx.update(|window, cx| {
            let _ = window.draw(cx);
        });

        // A long turn can be stopped.
        send(&view, cx, "wait");
        wait_until(cx, "the turn to start", |cx| {
            view.read_with(cx, |view, _| view.agent_panel.running)
        });
        view.update(cx, |view, cx| view.cancel_agent(cx));
        wait_until(cx, "the stop", |cx| {
            view.read_with(cx, |view, _| !view.agent_panel.running)
        });
        view.read_with(cx, |view, _| {
            assert_eq!(
                view.agent_panel.notice.as_deref(),
                Some(view.strings().agent_stop_reason("cancelled").as_str())
            );
        });
    }
}
