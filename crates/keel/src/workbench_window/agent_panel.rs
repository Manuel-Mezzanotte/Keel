//! The Claude panel: Claude Code as a chat on the right, through the Agent
//! Client Protocol, next to Claude Code in a terminal. It is the user's own
//! Claude Code, with its login and its chats: Keel installs the
//! `claude-agent-acp` adapter once and starts it in the project only when
//! the chat is used, and shows Claude Code's past chats at once, read from
//! where Claude Code keeps them. A chat shows its messages, reasoning,
//! tools with their file differences, the plan, permission requests and
//! the mode; files and selections go along with a message.

use std::collections::HashSet;

use ai::acp::{self, Agent, AgentCommand, AgentEvent};
use ai::history::Chat;
use ai::thread::{Entry, Thread, ToolCall, ToolContent, ToolStatus};
use futures::StreamExt as _;
use gpui::{
    Animation, AnimationExt as _, AnyElement, ScrollHandle, StatefulInteractiveElement as _,
    Subscription,
};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::input::{Input, InputEvent, InputState, Textarea, TextareaState};
use gpui_component::menu::DropdownMenu as _;
use gpui_component::text::{TextView, TextViewState};
use gpui_component::{ActiveTheme as _, Disableable as _, Icon, Selectable as _, Sizable as _};
use gpui_kit_assets::IconName;

use super::shell::{Colors, chevron_turn, empty_state, heading, keys_for, tool_row};
use super::*;

actions!(keel, [OpenAgent, NewAgentConversation, AddToAgent]);

/// How many lines the message box grows to.
const INPUT_ROWS: usize = 8;
/// How many lines of a file difference a tool shows before "…".
const DIFF_LINES: usize = 14;

/// What the panel on the right shows: the chat at first, Claude Code in
/// a terminal on Cmd+Esc.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum AgentView {
    Terminal,
    #[default]
    Agent,
}

/// How many chats the list keeps.
const MOST_CHATS: usize = 60;
/// How many chats the panel shows before the first message.
const RECENT_CHATS: usize = 5;

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
    /// The project's chats in Claude Code, the latest first: read from its
    /// files at first, then from the agent.
    pub(super) chats: Vec<Chat>,
    /// The list of chats is shown in place of the chat.
    pub(super) show_chats: bool,
    /// What the list of chats is filtered by.
    pub(super) chat_filter: Entity<InputState>,
    /// A chat to open as soon as the agent is ready.
    pending_load: Option<String>,
    open_thoughts: HashSet<usize>,
    /// Tools opened to show what they did.
    open_tools: HashSet<usize>,
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
    ) -> (Self, Vec<Subscription>) {
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
        let chat_filter =
            cx.new(|cx| InputState::new(window, cx).placeholder(strings.agent_search_chats));
        let filtered = cx.subscribe(&chat_filter, |_, _, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) {
                cx.notify();
            }
        });
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
            chats: Vec::new(),
            show_chats: false,
            chat_filter,
            pending_load: None,
            open_thoughts: HashSet::new(),
            open_tools: HashSet::new(),
            scroll: ScrollHandle::new(),
            waiting: None,
            notice: None,
            fake: false,
            snapshots: Vec::new(),
            changes: Vec::new(),
        };
        (panel, vec![events, filtered])
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

    pub(super) fn reset_thread(&mut self) {
        self.thread = Thread::default();
        self.markdown.clear();
        self.open_thoughts.clear();
        self.open_tools.clear();
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
        // A chat chosen before the agent ran: its updates are this
        // session's from the first one.
        let resume = self.agent_panel.pending_load.take();
        if let Some(id) = &resume {
            self.agent_panel.session = Some(id.clone());
        }
        cx.spawn_in(window, async move |this, cx| {
            let ready = initialize.await;
            let session = match (ready, resume) {
                (Ok(_), Some(id)) => agent.load_session(&id, &cwd).await.map(|mut loaded| {
                    loaded["sessionId"] = json!(id);
                    loaded
                }),
                (Ok(_), None) => agent.new_session(&cwd).await,
                (Err(error), _) => Err(error),
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
        window: &mut Window,
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
                    title: title.clone(),
                    content,
                    options,
                });
                self.agent_panel.scroll.scroll_to_bottom();
                self.agent_needs_you(self.strings().agent_waiting_permission, title, window, cx);
            }
            _ => {
                let _ = agent.respond(id, Err((-32601, format!("Keel does not offer {method}"))));
            }
        }
    }

    /// Claude finished or asks something with Keel in the background: a
    /// notice in macOS's Notification Center and a number on the Dock, as
    /// Cursor does.
    fn agent_needs_you(
        &mut self,
        title: &str,
        message: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if window.is_window_active() {
            return;
        }
        crate::mac::needs_attention();
        let notice = super::notices::Notice::info(title)
            .message(message)
            .action(self.strings().terminal_show, |this, window, cx| {
                this.open_agent(window, cx)
            });
        self.notify_user(notice, window, cx);
    }

    /// Keeps the text `path` has now, the first time the agent is about to
    /// change it, so the change can be undone.
    pub(super) fn remember_before_agent_edit(&mut self, path: &Path) {
        self.claude_touched.insert(path.to_path_buf());
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

    /// Asks the chat something: written in its box and sent, or left
    /// there to send while Claude is still at work.
    pub(super) fn ask_claude_in_chat(
        &mut self,
        prompt: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_agent(window, cx);
        self.agent_panel
            .input
            .update(cx, |input, cx| input.set_value(prompt, window, cx));
        if !self.agent_panel.running {
            self.send_to_agent(window, cx);
        }
    }

    /// Files and folders dropped on the chat's box are attached to the
    /// next message.
    pub(super) fn attach_to_agent(
        &mut self,
        paths: Vec<PathBuf>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        for path in paths {
            let attachment = Attachment {
                path,
                selection: None,
            };
            if !self.agent_panel.attachments.contains(&attachment) {
                self.agent_panel.attachments.push(attachment);
            }
        }
        self.agent_panel
            .input
            .update(cx, |input, cx| input.focus(window, cx));
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
                let said = this.agent_panel.notice.clone().unwrap_or_default();
                this.agent_needs_you(this.strings().agent_done_title, said, window, cx);
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
        self.agent_panel.show_chats = false;
        self.agent_panel.session = None;
        let opened = agent.new_session(&self.agent_folder());
        cx.spawn_in(window, async move |this, cx| {
            let session = opened.await;
            this.update_in(cx, |this, window, cx| {
                match session {
                    Ok(session) => {
                        this.agent_panel.session = session["sessionId"].as_str().map(str::to_owned);
                        let (modes, mode) = modes_of(&session);
                        this.agent_panel.modes = modes;
                        this.agent_panel.mode = mode;
                        // A message written while the chat was opening.
                        if let Some(blocks) = this.agent_panel.waiting.take() {
                            this.prompt_agent(blocks, window, cx);
                        }
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

    /// Reads the project's chats from Claude Code's files, away from the
    /// interface: they show before the agent starts.
    pub(super) fn read_claude_chats(&mut self, cx: &mut Context<Self>) {
        if self.agent_panel.fake || self.agent_panel.agent.is_some() {
            return;
        }
        let Some(claude) = ai::history::claude_dir() else {
            return;
        };
        let cwd = self.agent_folder();
        let read =
            cx.background_spawn(async move { ai::history::chats(&claude, &cwd, MOST_CHATS) });
        cx.spawn(async move |this, cx| {
            let chats = read.await;
            this.update(cx, |this, cx| {
                // The agent's list wins once it runs.
                if this.agent_panel.agent.is_none() {
                    this.agent_panel.chats = chats;
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
    }

    /// Reads the project's chats from the agent, which knows them best;
    /// without it, from Claude Code's files.
    fn refresh_agent_history(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(agent) = self.agent_panel.agent.clone() else {
            self.read_claude_chats(cx);
            return;
        };
        let listed = agent.list_sessions(&self.agent_folder());
        cx.spawn_in(window, async move |this, cx| {
            let Ok(listed) = listed.await else { return };
            let chats: Vec<Chat> = listed["sessions"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|session| {
                    let id = session["sessionId"].as_str()?.to_owned();
                    let title = session["title"]
                        .as_str()
                        .map(str::trim)
                        .filter(|title| !title.is_empty())
                        .unwrap_or(&id)
                        .to_owned();
                    let updated = session["updatedAt"]
                        .as_str()
                        .and_then(ai::history::iso_seconds)
                        .unwrap_or(0);
                    Some(Chat { id, title, updated })
                })
                .take(MOST_CHATS)
                .collect();
            this.update(cx, |this, cx| {
                if !chats.is_empty() || this.agent_panel.chats.is_empty() {
                    this.agent_panel.chats = chats;
                }
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
        self.agent_panel.show_chats = false;
        self.agent_panel.view = AgentView::Agent;
        let Some(agent) = self.agent_panel.agent.clone() else {
            // The agent starts, then opens the chat instead of a new one.
            self.agent_panel.reset_thread();
            self.agent_panel.pending_load = Some(id.clone());
            self.agent_panel.session = Some(id);
            self.ensure_agent(window, cx);
            cx.notify();
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

    /// What the chat is called: Claude Code's title, the one in the list of
    /// chats, or its first message; "Claude Code" before there is one.
    pub(super) fn agent_title(&self) -> String {
        let panel = &self.agent_panel;
        if let Some(title) = panel
            .thread
            .title
            .as_ref()
            .filter(|title| !title.is_empty())
        {
            return title.clone();
        }
        if let Some(chat) = panel
            .session
            .as_ref()
            .and_then(|id| panel.chats.iter().find(|chat| &chat.id == id))
        {
            return chat.title.clone();
        }
        panel
            .thread
            .entries
            .iter()
            .find_map(|entry| match entry {
                Entry::User(text) => text.lines().next().map(str::to_owned),
                _ => None,
            })
            .unwrap_or_else(|| "Claude Code".to_owned())
    }

    /// The buttons of the panel's header while the chat is shown: the
    /// list of chats and a new one.
    pub(super) fn render_agent_header_buttons(&self, cx: &mut Context<Self>) -> AnyElement {
        let strings = self.strings();
        div()
            .flex()
            .items_center()
            .gap_0p5()
            .child(
                Button::new("agent-history")
                    .ghost()
                    .small()
                    .icon(Icon::new(IconName::Clock))
                    .selected(self.agent_panel.show_chats)
                    .tooltip(strings.agent_all_chats)
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.agent_panel.show_chats = !this.agent_panel.show_chats;
                        if this.agent_panel.show_chats {
                            this.refresh_agent_history(window, cx);
                            this.agent_panel
                                .chat_filter
                                .update(cx, |input, cx| input.focus(window, cx));
                        }
                        cx.notify();
                    })),
            )
            .child(
                Button::new("agent-new")
                    .ghost()
                    .small()
                    .icon(Icon::new(IconName::Plus))
                    .tooltip(strings.agent_new_chat)
                    .on_click(
                        cx.listener(|this, _, window, cx| this.new_agent_conversation(window, cx)),
                    ),
            )
            .into_any_element()
    }

    pub(super) fn render_agent_conversation(
        &self,
        colors: Colors,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let strings = self.strings();
        let panel = &self.agent_panel;
        if panel.show_chats {
            return div()
                .flex_1()
                .min_h_0()
                .flex()
                .flex_col()
                .child(self.render_agent_chats(colors, cx))
                .child(self.render_agent_input(colors, window, cx))
                .into_any_element();
        }
        let body: AnyElement = if panel.thread.entries.is_empty() {
            self.render_agent_home(colors, window, cx)
        } else {
            let mut body = div()
                .id("agent-conversation")
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .track_scroll(&panel.scroll)
                .px_3()
                .py_3()
                .flex()
                .flex_col()
                .gap_3();
            let mut after_user = true;
            for (index, entry) in panel.thread.entries.iter().enumerate() {
                // Claude's name over the first block of each answer.
                let starts_answer = after_user && !matches!(entry, Entry::User(_));
                after_user = matches!(entry, Entry::User(_));
                if starts_answer {
                    body = body.child(claude_label(strings.agent_claude, colors));
                }
                body = body.child(self.render_agent_entry(index, entry, colors, window, cx));
            }
            if panel.running && panel.permissions.is_empty() {
                body = body.child(working(strings.agent_working, colors, self.look.motion));
            }
            if let Some(notice) = &panel.notice {
                body = body.child(
                    div()
                        .px_2()
                        .py_1p5()
                        .rounded(px(8.))
                        .bg(cx.theme().warning.opacity(0.1))
                        .text_xs()
                        .text_color(cx.theme().warning)
                        .child(notice.clone()),
                );
            }
            body.into_any_element()
        };
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
            .child(self.render_agent_input(colors, window, cx))
            .into_any_element()
    }

    /// Before the first message: who Claude is here, how it is doing, and
    /// the latest chats to pick up.
    fn render_agent_home(
        &self,
        colors: Colors,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let strings = self.strings();
        let panel = &self.agent_panel;
        let status: Option<(String, bool)> = match &panel.status {
            AgentStatus::Installing => Some((strings.agent_installing.to_owned(), true)),
            AgentStatus::Starting if panel.session.is_some() => {
                Some((strings.agent_opening_chat.to_owned(), true))
            }
            AgentStatus::Starting => Some((strings.agent_starting.to_owned(), true)),
            AgentStatus::Failed(error) => Some((error.clone(), false)),
            AgentStatus::Off | AgentStatus::Ready => None,
        };
        let failed = matches!(panel.status, AgentStatus::Failed(_));
        let recent: Vec<AnyElement> = panel
            .chats
            .iter()
            .take(RECENT_CHATS)
            .enumerate()
            .map(|(index, chat)| self.render_chat_row(("agent-recent", index), chat, colors, cx))
            .collect();
        // What else the panel does, with the keys.
        let tool = |id: &'static str,
                    icon: IconName,
                    label: &'static str,
                    action: Box<dyn gpui::Action>| {
            let keys = keys_for(action.as_ref(), window);
            tool_row(id, icon, label, keys, colors)
                .on_click(move |_, window, cx| window.dispatch_action(action.boxed_clone(), cx))
        };
        let tools = [
            tool(
                "agent-tool-new",
                IconName::Plus,
                strings.agent_new_chat,
                Box::new(NewAgentConversation),
            ),
            tool(
                "agent-tool-file",
                IconName::AtSign,
                strings.agent_add_file,
                Box::new(AddToAgent),
            ),
            tool(
                "agent-tool-terminal",
                IconName::SquareTerminal,
                strings.open_claude_code,
                Box::new(super::OpenClaudeCode),
            ),
        ];
        div()
            .id("agent-home")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .px_2()
            .py_4()
            .flex()
            .flex_col()
            .gap_3()
            .child(empty_state(
                IconName::Sparkles,
                "Claude Code",
                strings.agent_home_hint,
                colors,
            ))
            .when_some(status, |home, (text, busy)| {
                home.child(
                    div()
                        .mx_2()
                        .px_2p5()
                        .py_2()
                        .flex()
                        .items_center()
                        .gap_2()
                        .rounded(px(9.))
                        .bg(if busy {
                            colors.hover
                        } else {
                            cx.theme().warning.opacity(0.1)
                        })
                        .text_xs()
                        .text_color(if busy {
                            colors.muted
                        } else {
                            cx.theme().warning
                        })
                        .when(busy, |row| {
                            row.child(spinner(colors.accent, self.look.motion))
                        })
                        .child(div().flex_1().min_w_0().child(text))
                        .when(failed, |row| {
                            row.child(
                                Button::new("agent-start")
                                    .primary()
                                    .small()
                                    .label(strings.agent_start)
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.open_agent(window, cx)
                                    })),
                            )
                        }),
                )
            })
            .when(!recent.is_empty(), |home| {
                home.child(
                    div()
                        .flex()
                        .flex_col()
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .child(
                                    div()
                                        .flex_1()
                                        .child(heading(strings.agent_recent_chats, colors)),
                                )
                                .child(
                                    Button::new("agent-all-chats")
                                        .ghost()
                                        .small()
                                        .label(strings.agent_all_chats)
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            this.agent_panel.show_chats = true;
                                            this.refresh_agent_history(window, cx);
                                            cx.notify();
                                        })),
                                ),
                        )
                        .children(recent),
                )
            })
            .child(
                div()
                    .flex()
                    .flex_col()
                    .child(heading(strings.extensions_tools, colors))
                    .children(tools),
            )
            .into_any_element()
    }

    /// A chat in a list: its symbol, its title and how long ago it changed.
    fn render_chat_row(
        &self,
        id: impl Into<gpui::ElementId>,
        chat: &Chat,
        colors: Colors,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let italian = self.language == UiLanguage::Italian;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |elapsed| elapsed.as_secs() as i64);
        let current = self.agent_panel.session.as_deref() == Some(chat.id.as_str());
        let chat_id = chat.id.clone();
        let when =
            (chat.updated > 0).then(|| super::scm_graph::relative_time(chat.updated, now, italian));
        div()
            .id(id)
            .h(px(32.))
            .px_2()
            .flex()
            .items_center()
            .gap_2()
            .rounded(px(7.))
            .text_sm()
            .cursor_pointer()
            .when(current, |row| row.bg(colors.selected))
            .when(!current, |row| row.hover(|row| row.bg(colors.hover)))
            .active(|row| row.bg(colors.selected))
            .on_click(cx.listener(move |this, _, window, cx| {
                this.load_agent_conversation(chat_id.clone(), window, cx)
            }))
            .child(
                Icon::new(IconName::MessageSquare)
                    .small()
                    .text_color(colors.faint),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_color(colors.text)
                    .child(chat.title.clone()),
            )
            .children(when.map(|when| {
                div()
                    .flex_none()
                    .text_xs()
                    .text_color(colors.faint)
                    .child(when)
            }))
            .into_any_element()
    }

    /// Every chat of the project, by day, with a field to find one.
    fn render_agent_chats(&self, colors: Colors, cx: &mut Context<Self>) -> AnyElement {
        let strings = self.strings();
        let panel = &self.agent_panel;
        let filter = panel.chat_filter.read(cx).value().to_lowercase();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |elapsed| elapsed.as_secs() as i64);
        let midnight = local_midnight(now);
        let groups = [
            (strings.agent_today, midnight),
            (strings.agent_yesterday, midnight - 86_400),
            (strings.agent_this_week, midnight - 6 * 86_400),
            (strings.agent_older, i64::MIN),
        ];
        let shown: Vec<&Chat> = panel
            .chats
            .iter()
            .filter(|chat| filter.is_empty() || chat.title.to_lowercase().contains(&filter))
            .collect();
        let mut list = div()
            .id("agent-chats")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .px_2()
            .pb_2()
            .flex()
            .flex_col();
        if shown.is_empty() {
            list = list.child(
                div()
                    .px_2()
                    .py_6()
                    .text_sm()
                    .text_center()
                    .text_color(colors.muted)
                    .child(if panel.chats.is_empty() {
                        strings.agent_no_chats
                    } else {
                        strings.agent_no_match
                    }),
            );
        }
        let mut row = 0usize;
        let mut newer = i64::MAX;
        for (label, since) in groups {
            let chats: Vec<&Chat> = shown
                .iter()
                .copied()
                .filter(|chat| chat.updated >= since && chat.updated < newer)
                .collect();
            newer = since;
            if chats.is_empty() {
                continue;
            }
            list = list.child(div().pt_2().child(heading(label, colors)));
            for chat in chats {
                list = list.child(self.render_chat_row(("agent-chat", row), chat, colors, cx));
                row += 1;
            }
        }
        div()
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .child(
                div().px_3().pt_2p5().pb_1().child(
                    Input::new(&panel.chat_filter)
                        .small()
                        .prefix(Icon::new(IconName::Search).small().text_color(colors.faint))
                        .cleanable(true),
                ),
            )
            .child(list)
            .into_any_element()
    }

    fn render_agent_entry(
        &self,
        index: usize,
        entry: &Entry,
        colors: Colors,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        match entry {
            // Your messages on the right, in a bubble of the accent's tint.
            Entry::User(text) => div()
                .flex()
                .justify_end()
                .child(
                    div()
                        .max_w(px(self.agent_width * 0.86))
                        .px_3()
                        .py_2()
                        .rounded(px(14.))
                        .rounded_br(px(5.))
                        .bg(colors.accent.opacity(0.16))
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
                let turn = chevron_turn(("agent-thought-turn", index), open, self.look, window, cx);
                let first = text.lines().next().unwrap_or_default().to_owned();
                div()
                    .id(("agent-thought", index))
                    .flex()
                    .flex_col()
                    .gap_1()
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
                            .gap_1p5()
                            .child(
                                Icon::new(IconName::ChevronRight)
                                    .xsmall()
                                    .rotate(gpui::radians(turn * std::f32::consts::FRAC_PI_2)),
                            )
                            .child(
                                div()
                                    .flex_none()
                                    .font_weight(FontWeight::MEDIUM)
                                    .child(self.strings().agent_thinking),
                            )
                            .when(!open, |line| {
                                line.child(div().min_w_0().truncate().child(first))
                            }),
                    )
                    .when(open, |thought| {
                        thought.child(
                            div()
                                .ml(px(5.))
                                .pl_3()
                                .border_l_2()
                                .border_color(colors.border)
                                .italic()
                                .child(text.clone()),
                        )
                    })
                    .into_any_element()
            }
            Entry::Tool(tool) => self.render_tool(index, tool, colors, window, cx),
        }
    }

    /// A tool Claude used: a row with its symbol, its title and its state,
    /// that opens on what it did. A file's edit is open at first.
    fn render_tool(
        &self,
        index: usize,
        tool: &ToolCall,
        colors: Colors,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = cx.theme();
        let (success, danger) = (theme.success, theme.danger);
        let icon = match tool.kind.as_str() {
            "edit" | "delete" | "move" => IconName::Pencil,
            "read" => IconName::FileText,
            "search" => IconName::Search,
            "execute" => IconName::SquareTerminal,
            "fetch" => IconName::Globe,
            "think" => IconName::Sparkles,
            _ => IconName::Blocks,
        };
        let has_diff = tool
            .content
            .iter()
            .any(|content| matches!(content, ToolContent::Diff { .. }));
        let toggled = self.agent_panel.open_tools.contains(&index);
        // An edit shows its difference until it is closed; the others
        // open on a click.
        let open = !tool.content.is_empty() && (has_diff != toggled);
        let turn = chevron_turn(("agent-tool-turn", index), open, self.look, window, cx);
        let state: AnyElement = match tool.status {
            ToolStatus::Pending | ToolStatus::InProgress => {
                spinner(colors.accent, self.look.motion).into_any_element()
            }
            ToolStatus::Completed => Icon::new(IconName::Check)
                .xsmall()
                .text_color(success)
                .into_any_element(),
            ToolStatus::Failed => Icon::new(IconName::X)
                .xsmall()
                .text_color(danger)
                .into_any_element(),
        };
        let location = tool.locations.first().cloned();
        let open_file = location.map(|(path, line)| {
            Button::new(("agent-tool-open", index))
                .ghost()
                .xsmall()
                .icon(Icon::new(IconName::ExternalLink))
                .tooltip(self.strings().agent_open_file)
                .on_click(cx.listener(move |this, _, window, cx| {
                    cx.stop_propagation();
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
                }))
        });
        let expandable = !tool.content.is_empty();
        let mut card = div()
            .flex()
            .flex_col()
            .rounded(px(10.))
            .border_1()
            .border_color(colors.border)
            .bg(colors.editor.opacity(0.6))
            .overflow_hidden()
            .child(
                div()
                    .id(("agent-tool", index))
                    .h(px(30.))
                    .pl_1p5()
                    .pr_1()
                    .flex()
                    .items_center()
                    .gap_1p5()
                    .text_xs()
                    .when(expandable, |row| {
                        row.cursor_pointer()
                            .hover(|row| row.bg(colors.hover.opacity(0.5)))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if !this.agent_panel.open_tools.remove(&index) {
                                    this.agent_panel.open_tools.insert(index);
                                }
                                cx.notify();
                            }))
                    })
                    .child(
                        div()
                            .size(px(20.))
                            .flex_none()
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded(px(5.))
                            .bg(colors.hover)
                            .child(Icon::new(icon).xsmall().text_color(colors.muted)),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_color(colors.text)
                            .child(tool.title.clone()),
                    )
                    .children(open_file)
                    .child(
                        div()
                            .flex_none()
                            .w(px(16.))
                            .flex()
                            .justify_center()
                            .child(state),
                    )
                    .when(expandable, |row| {
                        row.child(
                            Icon::new(IconName::ChevronRight)
                                .xsmall()
                                .rotate(gpui::radians(turn * std::f32::consts::FRAC_PI_2))
                                .text_color(colors.faint),
                        )
                    }),
            );
        if open {
            card = card.child(
                div()
                    .px_2()
                    .pb_2()
                    .pt_0p5()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .children(
                        tool.content
                            .iter()
                            .map(|content| render_tool_content(content, colors, success, danger)),
                    ),
            );
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
        let progress = done as f32 / plan.len() as f32;
        Some(
            div()
                .flex_none()
                .mx_2()
                .mb_1p5()
                .px_2p5()
                .py_2()
                .rounded(px(10.))
                .border_1()
                .border_color(colors.border)
                .bg(colors.editor.opacity(0.6))
                .flex()
                .flex_col()
                .gap_1()
                .text_xs()
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(
                            div()
                                .flex_none()
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(colors.muted)
                                .child(self.strings().agent_plan(done, plan.len())),
                        )
                        // How far along, as a thin bar.
                        .child(
                            div()
                                .flex_1()
                                .h(px(4.))
                                .rounded_full()
                                .bg(colors.hover)
                                .child(
                                    div()
                                        .h_full()
                                        .w(gpui::relative(progress))
                                        .rounded_full()
                                        .bg(colors.accent),
                                ),
                        ),
                )
                .children(plan.iter().map(|item| {
                    let (mark, color) = match item.status.as_str() {
                        "completed" => (IconName::CircleCheck, colors.accent),
                        "in_progress" => (IconName::CircleDot, colors.text),
                        _ => (IconName::Circle, colors.faint),
                    };
                    div()
                        .flex()
                        .items_start()
                        .gap_1p5()
                        .text_color(if item.status == "completed" {
                            colors.faint
                        } else {
                            colors.text
                        })
                        .child(
                            div()
                                .pt(px(1.))
                                .child(Icon::new(mark).xsmall().text_color(color)),
                        )
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
            .mx_2()
            .mb_1p5()
            .p_2p5()
            .rounded(px(12.))
            .border_1()
            .border_color(colors.accent.opacity(0.6))
            .bg(colors.accent.opacity(0.07))
            .flex()
            .flex_col()
            .gap_2()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_1p5()
                    .text_xs()
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(colors.text)
                    .child(
                        Icon::new(IconName::ShieldCheck)
                            .small()
                            .text_color(colors.accent),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .child(self.strings().agent_permission(&permission.title)),
                    ),
            );
        for content in &permission.content {
            card = card.child(render_tool_content(
                content,
                colors,
                theme.success,
                theme.danger,
            ));
        }
        let mut buttons = div().flex().flex_wrap().gap_1p5();
        for (number, (id, name, kind)) in permission.options.iter().enumerate() {
            let option = id.clone();
            let button = Button::new(("agent-permission", index * 16 + number))
                .small()
                .label(name.clone());
            let button = if kind.starts_with("allow") && number == 0 {
                button.primary()
            } else {
                button.outline()
            };
            buttons = buttons.child(button.on_click(
                cx.listener(move |this, _, _, cx| this.answer_permission(index, &option, cx)),
            ));
        }
        card.child(buttons).into_any_element()
    }

    /// The message box, as a composer: what is attached, the text, and
    /// under it @, the mode and the button that sends or stops.
    fn render_agent_input(
        &self,
        colors: Colors,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
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
                    .h(px(24.))
                    .pl_2()
                    .flex()
                    .items_center()
                    .gap_1()
                    .rounded(px(7.))
                    .border_1()
                    .border_color(colors.border)
                    .bg(colors.hover.opacity(0.6))
                    .text_xs()
                    .text_color(colors.text)
                    .child(
                        Icon::new(IconName::FileText)
                            .xsmall()
                            .text_color(colors.muted),
                    )
                    .child(label)
                    .child(
                        Button::new(("agent-attachment", index))
                            .ghost()
                            .xsmall()
                            .icon(Icon::new(IconName::X))
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
                .icon(Icon::new(IconName::Square))
                .tooltip(strings.agent_stop)
                .rounded(px(16.))
                .on_click(cx.listener(|this, _, _, cx| this.cancel_agent(cx)))
                .into_any_element()
        } else {
            let empty =
                panel.input.read(cx).value().trim().is_empty() && panel.attachments.is_empty();
            Button::new("agent-send")
                .primary()
                .icon(Icon::new(IconName::ArrowUp))
                .tooltip(strings.agent_send)
                .rounded(px(16.))
                .disabled(empty)
                .on_click(cx.listener(|this, _, window, cx| this.send_to_agent(window, cx)))
                .into_any_element()
        };
        let typing = panel.input.read(cx).focus_handle(cx).is_focused(window);
        div()
            .id("agent-input-box")
            // Files from the Explorer or the Finder become attachments, as in
            // Cursor's chat.
            .drag_over::<super::explorer::DraggedPath>(move |style, _, _, _| {
                style
                    .border_color(colors.accent)
                    .bg(colors.accent.opacity(0.06))
            })
            .drag_over::<gpui::ExternalPaths>(move |style, _, _, _| {
                style
                    .border_color(colors.accent)
                    .bg(colors.accent.opacity(0.06))
            })
            .on_drop(
                cx.listener(|this, dragged: &super::explorer::DraggedPath, window, cx| {
                    this.attach_to_agent(dragged.all(), window, cx)
                }),
            )
            .on_drop(
                cx.listener(|this, paths: &gpui::ExternalPaths, window, cx| {
                    this.attach_to_agent(paths.paths().to_vec(), window, cx)
                }),
            )
            // A pasted picture, such as a screenshot, is attached too.
            .capture_action(
                cx.listener(|this, _: &gpui_component::input::Paste, window, cx| {
                    if this.paste_image_into_chat(window, cx) {
                        cx.stop_propagation();
                    }
                }),
            )
            .flex_none()
            .m_2()
            .pt_1()
            .pb_1p5()
            .px_1p5()
            .rounded(px(14.))
            .border_1()
            .border_color(if typing {
                colors.accent.opacity(0.7)
            } else {
                colors.border
            })
            .bg(colors.editor)
            .flex()
            .flex_col()
            .gap_1()
            .when(!panel.attachments.is_empty(), |input| {
                input.child(
                    div()
                        .px_1()
                        .pt_1()
                        .flex()
                        .flex_wrap()
                        .gap_1()
                        .children(chips),
                )
            })
            .child(
                div()
                    .text_sm()
                    .child(Textarea::new(&panel.input).appearance(false)),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_1()
                    .child(
                        Button::new("agent-add-file")
                            .ghost()
                            .small()
                            .icon(Icon::new(IconName::AtSign))
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
                                .icon(Icon::new(IconName::ShieldCheck))
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

/// Claude's name and mark over its answer.
fn claude_label(name: &'static str, colors: Colors) -> gpui::Div {
    div()
        .flex()
        .items_center()
        .gap_1p5()
        .mb(px(-6.))
        .text_xs()
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(colors.muted)
        .child(
            div()
                .size(px(18.))
                .rounded_full()
                .flex()
                .items_center()
                .justify_center()
                .bg(colors.accent.opacity(0.16))
                .child(
                    Icon::new(IconName::Sparkles)
                        .xsmall()
                        .text_color(colors.accent),
                ),
        )
        .child(name)
}

/// A small circle turning while something runs; still without movements.
pub(super) fn spinner(color: Hsla, motion: bool) -> AnyElement {
    let icon = Icon::new(IconName::LoaderCircle).xsmall().text_color(color);
    if !motion {
        return icon.into_any_element();
    }
    icon.with_animation(
        "spinner",
        Animation::new(std::time::Duration::from_millis(900)).repeat(),
        |icon, delta| icon.rotate(gpui::radians(delta * std::f32::consts::TAU)),
    )
    .into_any_element()
}

/// "Claude is working…" with three dots that breathe in turn.
fn working(text: &'static str, colors: Colors, motion: bool) -> gpui::Div {
    let dot = |index: usize| {
        let dot = div().size(px(5.)).rounded_full().bg(colors.accent);
        if !motion {
            return dot.into_any_element();
        }
        dot.with_animation(
            ("working-dot", index),
            Animation::new(std::time::Duration::from_millis(1100)).repeat(),
            move |dot, delta| {
                let phase = (delta - index as f32 * 0.18).rem_euclid(1.);
                let lift = (phase * std::f32::consts::TAU).sin().max(0.);
                dot.opacity(0.3 + 0.7 * lift)
            },
        )
        .into_any_element()
    };
    div()
        .flex()
        .items_center()
        .gap_2()
        .text_xs()
        .text_color(colors.faint)
        .child(div().flex().gap_1().children((0..3).map(dot)))
        .child(text)
}

/// Seconds since 1970 of the last local midnight before `now`.
fn local_midnight(now: i64) -> i64 {
    let time = now as libc::time_t;
    // SAFETY: localtime_r writes only into the tm it is given.
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    let found = unsafe { !libc::localtime_r(&time, &mut tm).is_null() };
    if !found {
        return now - now.rem_euclid(86_400);
    }
    now - (tm.tm_hour as i64 * 3600 + tm.tm_min as i64 * 60 + tm.tm_sec as i64)
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
    fn files_claude_changed_are_marked_until_opened_and_drops_attach(cx: &mut TestAppContext) {
        cx.update(|cx| {
            gpui_component::init(cx);
            init(cx);
        });
        let project = tempfile::tempdir().unwrap();
        let path = std::path::absolute(project.path().join("booking.ts")).unwrap();
        std::fs::write(&path, "const price = 90;\n").unwrap();
        let (view, cx) = cx
            .add_window_view(|window, cx| Workbench::new(None, test_settings(), None, window, cx));
        view.update_in(cx, |view, window, cx| {
            view.remember_before_agent_edit(&path);
            assert!(view.claude_touched.contains(&path));
            view.attach_to_agent(vec![path.clone(), path.clone()], window, cx);
            assert_eq!(view.agent_panel.attachments.len(), 1);
            view.open_file(&path, window, cx);
            assert!(!view.claude_touched.contains(&path));
        });
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
            view.read_with(cx, |view, _| !view.agent_panel.chats.is_empty())
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
        // A message written while a new chat opens goes once it is open.
        view.update_in(cx, |view, window, cx| {
            view.new_agent_conversation(window, cx);
            view.agent_panel
                .input
                .update(cx, |input, cx| input.set_value("di nuovo", window, cx));
            view.send_to_agent(window, cx);
        });
        wait_until(cx, "the answer in the new chat", |cx| {
            last_answer(&view, cx).as_deref() == Some("You said: di nuovo")
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
