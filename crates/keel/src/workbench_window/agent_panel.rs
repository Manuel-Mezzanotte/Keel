//! The Claude panel: Claude Code as a chat on the right, through the Agent
//! Client Protocol, next to Claude Code in a terminal. It is the user's own
//! Claude Code, with its login, its skills and its chats: Keel installs the
//! `claude-agent-acp` adapter once and starts it in the project only when
//! the chat is used, and shows Claude Code's past chats at once, read from
//! where Claude Code keeps them. The chats are Claude Code's own files, so
//! one goes on in the terminal and comes back here up to date.
//!
//! This file holds the conversation's state and its talk with the agent;
//! `agent_view` draws it, `agent_session` handles the account and options.

use std::collections::HashSet;

use ai::acp::{self, Agent, AgentCommand, AgentEvent};
use ai::history::Chat;
use ai::thread::{Entry, Thread, ToolContent};
use futures::StreamExt as _;
use gpui::{ScrollHandle, Subscription};
use gpui_component::input::{InputEvent, InputState, TextareaState};
use gpui_component::text::TextViewState;

pub(super) use super::agent_view::spinner;
use super::agent_view::{AgentAccount, ComposerMenus, ContinueInTerminal, SetAgentOption};
use super::*;

actions!(keel, [OpenAgent, NewAgentConversation, AddToAgent]);

/// How many lines the message box grows to.
const INPUT_ROWS: usize = 8;

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

/// How long a message written during a turn waits after the turn ends,
/// for the turn's last updates to come in before it.
const QUEUE_SETTLE: std::time::Duration = std::time::Duration::from_millis(150);

/// Whether the Mac stays awake while Claude works (on by default).
pub(super) const KEEP_AWAKE_SETTING: &str = "keel.claude.keepAwake";

/// The Claude app's chats: those of the project in `cwd`, and those of the
/// other projects of its account, by project, as chats of the list.
fn read_app_chats(cwd: &Path) -> (Vec<ai::history::AppChat>, Vec<(PathBuf, Vec<Chat>)>) {
    let Some(store) = ai::history::app_sessions_dir() else {
        return (Vec::new(), Vec::new());
    };
    let all = ai::history::app_chats_everywhere(&store);
    let here_text = cwd.to_string_lossy();
    let here_text = here_text.trim_end_matches('/');
    let here = all
        .iter()
        .filter(|chat| chat.cwd.to_string_lossy() == here_text)
        .cloned()
        .collect();
    let elsewhere = ai::history::app_chats_elsewhere(&all, cwd)
        .into_iter()
        .map(|(folder, chats)| {
            let chats = chats
                .into_iter()
                .map(|chat| Chat {
                    id: chat.session,
                    title: chat.title,
                    updated: chat.updated,
                    app: true,
                })
                .collect();
            (folder, chats)
        })
        .collect();
    (here, elsewhere)
}

/// Adds a message to one that waits to go: the texts joined in the first
/// block, as if written together, then the files of both. A block of text
/// that is only spaces is never sent: the API refuses it.
fn join_message(into: &mut Vec<Value>, blocks: Vec<Value>) {
    let mut blocks = blocks.into_iter();
    let text = blocks.next();
    match (into.first_mut(), text) {
        (Some(first), Some(text)) if first["type"] == "text" => {
            let joined = [first["text"].as_str(), text["text"].as_str()]
                .into_iter()
                .flatten()
                .filter(|part| !part.trim().is_empty())
                .collect::<Vec<_>>()
                .join("\n\n");
            first["text"] = json!(joined);
        }
        (_, Some(text)) => into.push(text),
        _ => {}
    }
    into.extend(blocks);
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
    /// The plan, when Claude asks to leave Plan mode with it ("Ready to
    /// code?"): it opens in a tab, to read, change and annotate.
    pub(super) plan: Option<String>,
}

/// A file as it was before Claude first changed it in a turn: the turn is
/// the entry of the message that started it. Bringing the code back to a
/// message puts back the oldest copy of each file changed from there on.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Checkpoint {
    pub(super) turn: usize,
    pub(super) path: PathBuf,
    /// None for a file Claude made.
    pub(super) before: Option<String>,
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
    pub(super) markdown: HashMap<usize, (Entity<TextViewState>, usize)>,
    /// The session's options: mode, model, effort, fast mode...
    pub(super) options: Vec<ai::session_meta::ConfigOption>,
    /// The `/` commands, the user's skills among them.
    pub(super) commands: Vec<ai::session_meta::SlashCommand>,
    /// How the agent can log in, and who it works as.
    pub(super) auth_methods: Vec<ai::session_meta::AuthMethod>,
    pub(super) account: Option<ai::session_meta::Account>,
    /// The agent wants a login before anything else.
    pub(super) needs_login: bool,
    /// A login running in a terminal: its method, and the wait for it.
    pub(super) login: Option<(String, Task<()>)>,
    /// The agent runs with the user's own API key, from the keychain.
    pub(super) uses_api_key: bool,
    /// Typing a key to keep in the keychain.
    pub(super) key_input: Option<Entity<InputState>>,
    /// The project's chats in Claude Code, the latest first: read from its
    /// files at first, then from the agent.
    pub(super) chats: Vec<Chat>,
    /// The Claude app's chats of other projects, by project: they open
    /// here, and Claude goes on working in their folder.
    pub(super) other_chats: Vec<(PathBuf, Vec<Chat>)>,
    /// The folder of the open chat when it is another project's: Claude
    /// works there, where the chat began. None for this project's chats.
    pub(super) chat_folder: Option<PathBuf>,
    /// The projects whose chats are folded in the lists, by folder.
    pub(super) folded_projects: HashSet<PathBuf>,
    /// The list of chats is shown in place of the chat.
    pub(super) show_chats: bool,
    /// What the list of chats is filtered by.
    pub(super) chat_filter: Entity<InputState>,
    /// A chat to open as soon as the agent is ready.
    pending_load: Option<String>,
    pub(super) open_thoughts: HashSet<usize>,
    /// Tools opened, or edits closed, by hand.
    pub(super) open_tools: HashSet<usize>,
    pub(super) scroll: ScrollHandle,
    /// The `/` and `@` menus of the message box.
    pub(super) menus: ComposerMenus,
    /// When the open chat's file last changed as Keel knows it: a later
    /// change came from Claude Code elsewhere, and the chat is read again.
    pub(super) synced: Option<std::time::SystemTime>,
    /// How full the open chat's context is, and what it cost.
    pub(super) context: Option<ai::session_meta::ContextUsage>,
    /// The plan's limits, from `/usage` and from each turn, and when
    /// `/usage` last answered.
    pub(super) plan_usage: Option<ai::session_meta::PlanUsage>,
    pub(super) usage_read: Option<std::time::Instant>,
    /// The throwaway session `/usage` runs in, and what it answered so far.
    pub(super) usage_session: Option<(String, String)>,
    pub(super) reading_usage: bool,
    /// The card open over the message box: the mode, the model, the usage.
    pub(super) popover: Option<super::agent_cards::ComposerPopover>,
    /// A card closed by a click outside it, and when: the click on its own
    /// button must not open it again.
    pub(super) popover_closed: Option<(super::agent_cards::ComposerPopover, std::time::Instant)>,
    /// The model card shows the older models too.
    pub(super) more_models: bool,
    /// The chat whose deletion waits for a second click.
    pub(super) deleting: Option<String>,
    /// Chats to delete once the agent runs.
    pub(super) pending_deletes: Vec<String>,
    /// Chats to rename once the agent runs: id and title.
    pub(super) pending_renames: Vec<(String, String)>,
    /// Chats deleted here: a list asked for before stays without them.
    pub(super) deleted_chats: HashSet<String>,
    /// A message written before the session was ready.
    waiting: Option<Vec<Value>>,
    /// Keeps the Mac awake while a turn runs.
    awake: Option<crate::mac::Awake>,
    /// Messages written while Claude was at work, and what they show: they
    /// go as one as soon as the turn ends.
    pub(super) queued: Option<(Vec<Value>, String)>,
    /// The chat the window had open when it closed, and its folder for a
    /// chat of another project: it opens again once the panel shows.
    pub(super) restored: Option<(String, Option<PathBuf>)>,
    /// A notice under the conversation, such as an error.
    pub(super) notice: Option<String>,
    /// Tests talk to the fake agent instead of the adapter.
    pub(super) fake: bool,
    /// The tab of the plan Claude proposed, and the plan as it came.
    pub(super) plan_doc: Option<(String, String)>,
    /// The text files had before the agent first changed them in this
    /// conversation (none for a file it created), in order.
    pub(super) snapshots: Vec<(PathBuf, Option<String>)>,
    /// The kept files that differ now, for the review.
    pub(super) changes: Vec<super::agent_review::AgentChange>,
    /// The files before each turn changed them, to bring the code back.
    pub(super) checkpoints: Vec<Checkpoint>,
    /// The entry of the message whose turn runs or ran last.
    pub(super) turn: Option<usize>,
    /// The message whose code is to come back, after a second click.
    pub(super) rewinding: Option<usize>,
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
                    this.agent_panel.menus.text_changed();
                    this.agent_panel.popover = None;
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
            options: Vec::new(),
            commands: Vec::new(),
            auth_methods: Vec::new(),
            account: None,
            needs_login: false,
            login: None,
            uses_api_key: false,
            key_input: None,
            chats: Vec::new(),
            other_chats: Vec::new(),
            chat_folder: None,
            folded_projects: HashSet::new(),
            show_chats: false,
            chat_filter,
            pending_load: None,
            open_thoughts: HashSet::new(),
            open_tools: HashSet::new(),
            scroll: ScrollHandle::new(),
            menus: ComposerMenus::default(),
            synced: None,
            context: None,
            plan_usage: None,
            usage_read: None,
            usage_session: None,
            reading_usage: false,
            popover: None,
            popover_closed: None,
            more_models: false,
            deleting: None,
            pending_deletes: Vec::new(),
            pending_renames: Vec::new(),
            deleted_chats: HashSet::new(),
            waiting: None,
            awake: None,
            queued: None,
            restored: None,
            notice: None,
            fake: false,
            snapshots: Vec::new(),
            plan_doc: None,
            changes: Vec::new(),
            checkpoints: Vec::new(),
            turn: None,
            rewinding: None,
        };
        (panel, vec![events, filtered])
    }

    /// Brings the rendered messages up to date with the thread.
    pub(super) fn sync_markdown(&mut self, cx: &mut Context<Workbench>) {
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
        self.context = None;
        self.queued = None;
        self.checkpoints.clear();
        self.turn = None;
        self.rewinding = None;
    }
}

impl Workbench {
    /// The folder the agent works in.
    pub(super) fn agent_folder(&self) -> PathBuf {
        self.project
            .root
            .clone()
            .or_else(|| std::env::var_os("HOME").map(PathBuf::from))
            .unwrap_or_else(|| PathBuf::from("/"))
    }

    /// The folder the open chat works in: its own project's for a chat of
    /// another project, otherwise this one's.
    pub(super) fn agent_chat_folder(&self) -> PathBuf {
        self.agent_panel
            .chat_folder
            .clone()
            .unwrap_or_else(|| self.agent_folder())
    }

    /// Shows the agent and starts it if needed.
    pub(super) fn open_agent(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.layout.agent = true;
        self.agent_panel.view = AgentView::Agent;
        if !self.resume_restored_chat(window, cx) {
            self.ensure_agent(window, cx);
        }
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
        self.agent_panel.uses_api_key = false;
        let start = cx.background_spawn(async move {
            let mut env = lsp::servers::login_environment().clone();
            // The user's own key, kept in the keychain, wins over a login.
            let key = super::agent_session::saved_api_key();
            let uses_key = key.is_some();
            if let Some(key) = key {
                env.insert("ANTHROPIC_API_KEY".to_owned(), key);
            }
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
            anyhow::Ok((Agent::start(&command)?, uses_key))
        });
        cx.spawn_in(window, async move |this, cx| {
            let started = start.await;
            this.update_in(cx, |this, window, cx| match started {
                Ok(((agent, events), uses_key)) => {
                    this.agent_panel.uses_api_key = uses_key;
                    this.agent_started(agent, events, cwd, window, cx)
                }
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
        } else {
            self.agent_panel.chat_folder = None;
        }
        // A chat of another project goes on in its own folder.
        let chat_cwd = self.agent_chat_folder();
        cx.spawn_in(window, async move |this, cx| {
            let ready = initialize.await;
            if let Ok(answer) = &ready {
                let methods = ai::session_meta::auth_methods(answer);
                this.update(cx, |this, cx| {
                    this.agent_panel.auth_methods = methods;
                    for id in std::mem::take(&mut this.agent_panel.pending_deletes) {
                        this.delete_agent_chat(id, cx);
                    }
                    for (id, title) in std::mem::take(&mut this.agent_panel.pending_renames) {
                        this.rename_agent_chat_to(id, title, cx);
                    }
                })
                .ok();
            }
            let session = match (ready, resume) {
                (Ok(_), Some(id)) => agent.load_session(&id, &chat_cwd).await.map(|mut loaded| {
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
                        this.apply_agent_options(&session);
                        this.mark_agent_chat_synced();
                        this.agent_panel.needs_login = false;
                        this.agent_panel.status = AgentStatus::Ready;
                        if let Some(blocks) = this.agent_panel.waiting.take() {
                            this.prompt_agent(blocks, window, cx);
                        }
                        this.refresh_agent_history(window, cx);
                    }
                    Err(error) => {
                        this.log(format!("Claude: {error:#}"));
                        this.agent_failed_to_open(&error);
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
                if method == "auth/status_update" {
                    self.agent_account_changed(ai::session_meta::account(&params), window, cx);
                }
                // The commands come while the session opens, before Keel
                // knows its id: they are the same for every session here.
                if method == "session/update"
                    && params["update"]["sessionUpdate"] == "available_commands_update"
                {
                    self.agent_panel.commands = ai::session_meta::slash_commands(&params["update"]);
                }
                // What `/usage` answers, in its own session.
                if method == "session/update"
                    && let Some((usage, text)) = &mut self.agent_panel.usage_session
                    && params["sessionId"].as_str() == Some(usage.as_str())
                {
                    if params["update"]["sessionUpdate"] == "agent_message_chunk" {
                        text.push_str(
                            params["update"]["content"]["text"]
                                .as_str()
                                .unwrap_or_default(),
                        );
                    }
                    return;
                }
                if method == "session/update"
                    && params["sessionId"].as_str() == self.agent_panel.session.as_deref()
                {
                    let update = &params["update"];
                    self.before_agent_update(update, cx);
                    self.agent_panel.thread.apply(update);
                    if update["sessionUpdate"] == "config_option_update" {
                        self.apply_agent_options(update);
                    }
                    if update["sessionUpdate"] == "usage_update" {
                        self.agent_usage_update(update);
                    }
                    if let Some(mode) = self.agent_panel.thread.mode.clone()
                        && update["sessionUpdate"] == "current_mode_update"
                    {
                        self.agent_option_changed("mode", mode);
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
                self.agent_panel.chat_folder = None;
                self.agent_panel.running = false;
                self.agent_panel.awake = None;
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
                let options: Vec<(String, String, String)> = params["options"]
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
                // The plan of "Ready to code?": in `rawInput`, or the text
                // of the request when the options are the plan's.
                let plan = tool["rawInput"]["plan"]
                    .as_str()
                    .map(str::to_owned)
                    .or_else(|| {
                        options
                            .iter()
                            .any(|(option, _, _)| option.starts_with("exit-plan"))
                            .then(|| {
                                content.iter().find_map(|item| match item {
                                    ToolContent::Text(text) => Some(text.clone()),
                                    _ => None,
                                })
                            })
                            .flatten()
                    })
                    .filter(|plan| !plan.trim().is_empty());
                if let Some(plan) = &plan {
                    self.open_agent_plan(plan.clone(), window, cx);
                }
                self.agent_panel.permissions.push(Permission {
                    id,
                    title: title.clone(),
                    content,
                    options,
                    plan,
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
        // The copy of this turn, the first time the turn changes the file.
        if let Some(turn) = self.agent_panel.turn
            && !self
                .agent_panel
                .checkpoints
                .iter()
                .any(|known| known.turn == turn && known.path == path)
        {
            self.agent_panel.checkpoints.push(Checkpoint {
                turn,
                path: path.to_path_buf(),
                before: std::fs::read_to_string(path).ok(),
            });
        }
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

    /// Files and folders of the Explorer, with "Add to Claude": they go
    /// with the next message, and the chat shows.
    pub(super) fn add_paths_to_agent(
        &mut self,
        paths: Vec<PathBuf>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.layout.agent = true;
        self.agent_panel.view = AgentView::Agent;
        self.agent_panel.show_chats = false;
        self.attach_to_agent(paths, window, cx);
        self.session_changed(cx);
    }

    /// Enter in the message box: sends the text and the attachments.
    pub(super) fn send_to_agent(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let text = self.agent_panel.input.read(cx).value().trim().to_owned();
        if text.is_empty() && self.agent_panel.attachments.is_empty() {
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
        // Claude is at work: the message waits for the turn to end, as in
        // Claude Code, and more of them go together.
        if self.agent_panel.running {
            let shown = shown.trim().to_owned();
            match &mut self.agent_panel.queued {
                Some((queued, said)) => {
                    join_message(queued, blocks);
                    said.push_str("\n\n");
                    said.push_str(&shown);
                }
                None => self.agent_panel.queued = Some((blocks, shown)),
            }
            cx.notify();
            return;
        }
        self.agent_panel.thread.push_user(shown.trim());
        self.agent_panel.notice = None;
        if self.agent_panel.session.is_none() {
            // Several written while Claude starts go together.
            match &mut self.agent_panel.waiting {
                Some(waiting) => join_message(waiting, blocks),
                None => self.agent_panel.waiting = Some(blocks),
            }
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
        // The turn belongs to the message that starts it, the last one sent.
        self.agent_panel.turn = self
            .agent_panel
            .thread
            .entries
            .iter()
            .rposition(|entry| matches!(entry, Entry::User(_)));
        // The Mac stays awake while Claude works, unless told otherwise.
        let keep_awake = !cfg!(test)
            && self
                .settings
                .get(KEEP_AWAKE_SETTING)
                .and_then(Value::as_bool)
                .unwrap_or(true);
        if keep_awake && self.agent_panel.awake.is_none() {
            self.agent_panel.awake = Some(crate::mac::stay_awake("Claude is working in Keel"));
        }
        let turn = agent.prompt(&session, blocks);
        cx.spawn_in(window, async move |this, cx| {
            let result = turn.await;
            this.update_in(cx, |this, window, cx| {
                this.agent_panel.running = false;
                this.agent_panel.awake = None;
                this.mark_agent_chat_synced();
                this.refresh_agent_changes();
                this.session_changed(cx);
                // What was written meanwhile goes now, in the same chat. The
                // turn's last updates may still be on their way, apart from
                // its end: it goes once they are in, so it shows after them.
                if this.agent_panel.queued.is_some() {
                    this.agent_panel.notice = None;
                    cx.spawn_in(window, async move |this, cx| {
                        cx.background_executor().timer(QUEUE_SETTLE).await;
                        this.update_in(cx, |this, window, cx| this.send_queued(window, cx))
                            .ok();
                    })
                    .detach();
                    cx.notify();
                    return;
                }
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

    /// Whether the code can come back to the message at `entry`: a turn
    /// from there on changed files.
    pub(super) fn can_rewind_to(&self, entry: usize) -> bool {
        self.agent_panel
            .checkpoints
            .iter()
            .any(|checkpoint| checkpoint.turn >= entry)
    }

    /// Brings the code back to how it was when the message at `entry` was
    /// sent: each file changed from that turn on gets its oldest copy back,
    /// and a file Claude made goes to the Trash. Commands Claude ran (an
    /// install, a `sed`) are not undone, as in Claude Code.
    pub(super) fn rewind_code_to(&mut self, entry: usize, cx: &mut Context<Self>) {
        self.agent_panel.rewinding = None;
        let mut oldest: Vec<(PathBuf, Option<String>)> = Vec::new();
        let mut later: Vec<&Checkpoint> = self
            .agent_panel
            .checkpoints
            .iter()
            .filter(|checkpoint| checkpoint.turn >= entry)
            .collect();
        later.sort_by_key(|checkpoint| checkpoint.turn);
        for checkpoint in later {
            if !oldest.iter().any(|(path, _)| *path == checkpoint.path) {
                oldest.push((checkpoint.path.clone(), checkpoint.before.clone()));
            }
        }
        let mut failed = Vec::new();
        for (path, before) in &oldest {
            let done = match before {
                Some(text) => std::fs::write(path, text).map_err(|error| error.to_string()),
                None if path.exists() => files::trash(path).map_err(|error| error.to_string()),
                None => Ok(()),
            };
            if let Err(error) = done {
                failed.push(format!("{}: {error}", path.display()));
            }
        }
        self.agent_panel
            .checkpoints
            .retain(|checkpoint| checkpoint.turn < entry);
        self.refresh_agent_changes();
        let strings = self.strings();
        self.status_message = Some(if failed.is_empty() {
            strings.agent_rewound(oldest.len())
        } else {
            failed.join("; ")
        });
        cx.notify();
    }

    /// A new chat that goes on from the answer at `entry`: Claude Code copies
    /// the chat up to it (`session/fork`), and the panel moves there. From a
    /// message of yours, the copy stops before it and the message waits in
    /// the box, to be sent again or changed.
    pub(super) fn fork_agent_chat(
        &mut self,
        entry: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (Some(agent), Some(session)) = (
            self.agent_panel.agent.clone(),
            self.agent_panel.session.clone(),
        ) else {
            return;
        };
        let thread = &self.agent_panel.thread;
        let (answer, rewrite) = match thread.entries.get(entry) {
            Some(Entry::User(text)) => (
                thread.entries[..entry]
                    .iter()
                    .rposition(|known| matches!(known, Entry::Agent(_))),
                Some(text.clone()),
            ),
            Some(Entry::Agent(_)) => (Some(entry), None),
            _ => return,
        };
        let last_answer = thread
            .entries
            .iter()
            .rposition(|known| matches!(known, Entry::Agent(_)));
        let message = answer.and_then(|answer| thread.message_ids.get(&answer).cloned());
        // Without the id, only the whole chat can be copied.
        if message.is_none() && answer.is_some() && answer != last_answer {
            return;
        }
        if self.agent_panel.running {
            self.cancel_agent(cx);
        }
        let forked = match (answer, &message) {
            // Before the first answer: nothing to copy, a new chat.
            (None, _) => None,
            (_, message) => {
                Some(agent.fork_session(&session, &self.agent_chat_folder(), message.as_deref()))
            }
        };
        let Some(forked) = forked else {
            self.new_agent_conversation(window, cx);
            if let Some(text) = rewrite {
                self.agent_panel
                    .input
                    .update(cx, |input, cx| input.set_value(text, window, cx));
            }
            return;
        };
        cx.spawn_in(window, async move |this, cx| {
            let forked = forked.await;
            this.update_in(cx, |this, window, cx| {
                match forked {
                    Ok(answer) => {
                        let Some(id) = answer["sessionId"].as_str().map(str::to_owned) else {
                            return;
                        };
                        this.load_agent_conversation(id, window, cx);
                        if let Some(text) = rewrite {
                            this.agent_panel
                                .input
                                .update(cx, |input, cx| input.set_value(text, window, cx));
                        }
                        this.refresh_agent_history(window, cx);
                        this.session_changed(cx);
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

    /// Opens the plan Claude proposed in a tab of the editor, as Markdown,
    /// in place of an older one: to read it whole, change it and write
    /// notes in it before saying yes.
    pub(super) fn open_agent_plan(
        &mut self,
        plan: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.close_agent_plan(window, cx);
        let uri = format!("untitled:{}", self.strings().agent_plan_file);
        let uri = self.add_document(uri, Backing::Untitled, "markdown".to_owned(), plan.clone());
        let column = self.active.min(self.panes.len().saturating_sub(1));
        self.open_in_pane(&uri, column, false, window, cx);
        self.agent_panel.plan_doc = Some((uri, plan));
        cx.notify();
    }

    /// Closes the plan's tab without asking: it was Claude's proposal.
    pub(super) fn close_agent_plan(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some((uri, _)) = self.agent_panel.plan_doc.take() {
            self.close_document_tabs(&uri, window, cx);
            self.documents.remove(&uri);
        }
    }

    /// The plan as it reads now in its tab, with the changes and notes.
    fn agent_plan_text(&self, cx: &App) -> Option<String> {
        let (uri, _) = self.agent_panel.plan_doc.as_ref()?;
        let open = self.panes.iter().find(|pane| &pane.uri == uri);
        Some(match open {
            Some(pane) => pane.editor.read(cx).value().to_string(),
            None => self.documents.get(uri)?.synced_text.clone(),
        })
    }

    /// "Revise with my notes": the plan, changed and annotated in its tab,
    /// goes back to Claude, which keeps planning with it.
    pub(super) fn revise_agent_plan(
        &mut self,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(permission) = self.agent_panel.permissions.get(index) else {
            return;
        };
        let original = permission.plan.clone().unwrap_or_default();
        let Some(keep_planning) = permission
            .options
            .iter()
            .find(|(_, _, kind)| kind.starts_with("reject"))
            .map(|(option, _, _)| option.clone())
        else {
            return;
        };
        let edited = self.agent_plan_text(cx).unwrap_or_else(|| original.clone());
        if edited.trim() == original.trim() {
            self.agent_panel.notice = Some(self.strings().agent_plan_unchanged.to_owned());
            cx.notify();
            return;
        }
        self.answer_permission(index, &keep_planning, cx);
        self.close_agent_plan(window, cx);
        let feedback = format!(
            "{}\n\n{}",
            self.strings().agent_plan_feedback,
            edited.trim()
        );
        self.agent_panel
            .input
            .update(cx, |input, cx| input.set_value(feedback, window, cx));
        self.send_to_agent(window, cx);
    }

    /// Sends what was written while Claude worked; with a turn running
    /// again, it waits for that one.
    fn send_queued(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.agent_panel.running {
            return;
        }
        let Some((blocks, said)) = self.agent_panel.queued.take() else {
            return;
        };
        self.agent_panel.thread.push_user(&said);
        self.prompt_agent(blocks, window, cx);
        self.agent_panel.scroll.scroll_to_bottom();
        cx.notify();
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
        // A new chat is always this project's.
        self.agent_panel.chat_folder = None;
        let opened = agent.new_session(&self.agent_folder());
        cx.spawn_in(window, async move |this, cx| {
            let session = opened.await;
            this.update_in(cx, |this, window, cx| {
                match session {
                    Ok(session) => {
                        this.agent_panel.session = session["sessionId"].as_str().map(str::to_owned);
                        this.apply_agent_options(&session);
                        this.agent_panel.needs_login = false;
                        // A message written while the chat was opening.
                        if let Some(blocks) = this.agent_panel.waiting.take() {
                            this.prompt_agent(blocks, window, cx);
                        }
                    }
                    Err(error) => this.agent_failed_to_open(&error),
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
        let read = cx.background_spawn(async move {
            let chats = ai::history::chats(&claude, &cwd, MOST_CHATS);
            let (app, elsewhere) = read_app_chats(&cwd);
            (
                ai::history::with_app_chats(chats, &app, MOST_CHATS),
                elsewhere,
            )
        });
        cx.spawn(async move |this, cx| {
            let (mut chats, elsewhere) = read.await;
            this.update(cx, |this, cx| {
                this.agent_panel.other_chats = elsewhere;
                // The agent's list wins once it runs.
                if this.agent_panel.agent.is_none() {
                    let deleted = &this.agent_panel.deleted_chats;
                    chats.retain(|chat| !deleted.contains(&chat.id));
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
    pub(super) fn refresh_agent_history(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(agent) = self.agent_panel.agent.clone() else {
            self.read_claude_chats(cx);
            return;
        };
        let listed = agent.list_sessions(&self.agent_folder());
        let cwd = self.agent_folder();
        let fake = self.agent_panel.fake;
        let app = cx.background_spawn(async move {
            if fake {
                (Vec::new(), Vec::new())
            } else {
                read_app_chats(&cwd)
            }
        });
        cx.spawn_in(window, async move |this, cx| {
            let (app, elsewhere) = app.await;
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
                    Some(Chat {
                        id,
                        title,
                        updated,
                        app: false,
                    })
                })
                .collect();
            let mut chats = ai::history::with_app_chats(chats, &app, MOST_CHATS);
            this.update(cx, |this, cx| {
                this.agent_panel.other_chats = elsewhere;
                let deleted = &this.agent_panel.deleted_chats;
                chats.retain(|chat| !deleted.contains(&chat.id));
                this.agent_panel
                    .chats
                    .retain(|chat| !deleted.contains(&chat.id));
                if !chats.is_empty() || this.agent_panel.chats.is_empty() {
                    this.agent_panel.chats = chats;
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Opens a past conversation: the agent replays it, in the folder the
    /// chat works in (`agent_chat_folder`).
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
            self.note_app_chat(&id);
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
        self.note_app_chat(&id);
        self.agent_panel.session = Some(id.clone());
        let loaded = agent.load_session(&id, &self.agent_chat_folder());
        cx.spawn_in(window, async move |this, cx| {
            let loaded = loaded.await;
            this.update(cx, |this, cx| {
                match loaded {
                    Ok(session) => {
                        this.apply_agent_options(&session);
                        this.mark_agent_chat_synced();
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
        self.set_agent_option("mode".to_owned(), mode, cx);
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
                    this.open_agent_chat(action.id.clone(), None, window, cx)
                }),
            )
            .on_action(cx.listener(|this, action: &SetAgentMode, _, cx| {
                this.set_agent_mode(action.mode.clone(), cx)
            }))
            .on_action(cx.listener(|this, action: &SetAgentOption, _, cx| {
                this.set_agent_option(action.id.clone(), action.value.clone(), cx)
            }))
            .on_action(
                cx.listener(|this, action: &ContinueInTerminal, window, cx| {
                    this.continue_chat_in_terminal(action.session.clone(), window, cx)
                }),
            )
            .on_action(
                cx.listener(|this, action: &AgentAccount, window, cx| match action {
                    AgentAccount::UseKey => this.ask_agent_api_key(window, cx),
                    AgentAccount::ForgetKey => this.forget_agent_api_key(window, cx),
                    AgentAccount::Logout => this.logout_agent(window, cx),
                }),
            )
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

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use gpui::{TestAppContext, VisualTestContext};

    use super::super::tests::test_settings;
    use ai::thread::ToolStatus;

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

            // "Add to Claude" in the Explorer attaches and shows the chat.
            view.agent_panel.attachments.clear();
            view.layout.agent = false;
            view.add_paths_to_agent(vec![path.clone(), project.path().to_path_buf()], window, cx);
            assert!(view.layout.agent);
            assert_eq!(view.agent_panel.attachments.len(), 2);

            // An answer is copied as Claude wrote it.
            view.copy_agent_answer("**Fatto.**\n".to_owned(), cx);
        });

        // ⌘D dictates in Claude's message box, and only there.
        let cmd_d = gpui::Keystroke::parse("cmd-d").unwrap();
        view.update_in(cx, |view, window, cx| {
            view.agent_panel
                .input
                .update(cx, |input, cx| input.focus(window, cx));
        });
        view.update_in(cx, |view, window, cx| {
            assert!(view.dictation_key(&cmd_d, window, cx));
            let plain_d = gpui::Keystroke::parse("d").unwrap();
            assert!(!view.dictation_key(&plain_d, window, cx));
            view.focus_pane(view.active, window, cx);
        });
        view.update_in(cx, |view, window, cx| {
            assert!(!view.dictation_key(&cmd_d, window, cx));
        });
        let copied = cx.read_from_clipboard().and_then(|item| item.text());
        assert_eq!(copied.as_deref(), Some("**Fatto.**"));
    }

    #[gpui::test]
    fn a_chat_of_another_project_opens_here_in_its_folder(cx: &mut TestAppContext) {
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
        let other = tempfile::tempdir().unwrap();
        let other = std::fs::canonicalize(other.path()).unwrap();
        let document = Document::open(&path).unwrap();
        let (view, cx) = cx.add_window_view(|window, cx| {
            let mut workbench = Workbench::new(Some(document), test_settings(), None, window, cx);
            workbench.agent_panel.fake = true;
            workbench
        });
        let here = view.read_with(cx, |view, _| view.agent_folder());
        let windows = |cx: &mut VisualTestContext| cx.update(|_, cx| cx.windows().len());
        let before = windows(cx);

        // A chat, then a new one: the first is now listed with another
        // project, as the Claude app's chats of other folders are.
        view.update_in(cx, |view, window, cx| view.open_agent(window, cx));
        send(&view, cx, "ciao");
        wait_until(cx, "the answer", |cx| last_answer(&view, cx).is_some());
        view.update_in(cx, |view, window, cx| {
            view.new_agent_conversation(window, cx)
        });
        wait_until(cx, "a new session", |cx| {
            view.read_with(cx, |view, _| {
                view.agent_panel.session.as_deref() == Some("session-2")
            })
        });
        view.update_in(cx, |view, window, cx| {
            view.agent_panel.other_chats = vec![(
                other.clone(),
                vec![Chat {
                    id: "session-1".into(),
                    title: "Documentazione".into(),
                    updated: 0,
                    app: true,
                }],
            )];
            view.open_agent_chat("session-1".into(), Some(other.clone()), window, cx);
        });
        wait_until(cx, "the chat of the other project", |cx| {
            view.read_with(cx, |view, _| view.agent_panel.thread.entries.len() >= 2)
        });

        // It opened here, with no window of its own, and works in its folder.
        assert_eq!(windows(cx), before);
        view.read_with(cx, |view, _| {
            assert_eq!(
                view.agent_panel.chat_folder.as_deref(),
                Some(other.as_path())
            );
            assert_eq!(view.agent_panel.session.as_deref(), Some("session-1"));
            assert_eq!(view.agent_title(), "Documentazione");
        });
        send(&view, cx, "pwd");
        wait_until(cx, "the folder", |cx| {
            last_answer(&view, cx).is_some_and(|answer| answer.starts_with("cwd:"))
        });
        assert_eq!(
            last_answer(&view, cx).unwrap(),
            format!("cwd: {}", other.display())
        );
        // The panel says where Claude works.
        cx.update(|window, cx| {
            let _ = window.draw(cx);
        });
        assert!(cx.debug_bounds("agent-chat-folder").is_some());

        // A new chat is this project's again.
        view.update_in(cx, |view, window, cx| {
            view.new_agent_conversation(window, cx)
        });
        view.read_with(cx, |view, _| assert_eq!(view.agent_panel.chat_folder, None));
        send(&view, cx, "pwd");
        wait_until(cx, "the folder of the new chat", |cx| {
            last_answer(&view, cx).is_some_and(|answer| answer.starts_with("cwd:"))
        });
        assert_eq!(
            last_answer(&view, cx).unwrap(),
            format!("cwd: {}", here.display())
        );
    }

    #[gpui::test]
    fn the_code_comes_back_to_a_message_and_a_chat_forks(cx: &mut TestAppContext) {
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
        // The project without the file open: the fake agent reads the disk.
        let root = path.parent().unwrap().to_path_buf();
        let (view, cx) = cx.add_window_view(|window, cx| {
            let mut workbench = Workbench::new(
                Opening::new(Some(root), None),
                test_settings(),
                None,
                window,
                cx,
            );
            workbench.agent_panel.fake = true;
            workbench
        });
        let users =
            |view: &Entity<Workbench>, cx: &mut VisualTestContext| -> Vec<(usize, String)> {
                view.read_with(cx, |view, _| {
                    view.agent_panel
                        .thread
                        .entries
                        .iter()
                        .enumerate()
                        .filter_map(|(index, entry)| match entry {
                            Entry::User(text) => Some((index, text.clone())),
                            _ => None,
                        })
                        .collect()
                })
            };
        // Two turns, each changing the file.
        view.update_in(cx, |view, window, cx| view.open_agent(window, cx));
        for (old, new) in [("90", "95"), ("95", "99")] {
            send(&view, cx, &format!("edit {} {old} {new}", path.display()));
            wait_until(cx, "the permission", |cx| {
                view.read_with(cx, |view, _| !view.agent_panel.permissions.is_empty())
            });
            view.update(cx, |view, cx| view.answer_permission(0, "allow", cx));
            wait_until(cx, "the edit", |cx| {
                last_answer(&view, cx).as_deref() == Some("Done.")
            });
        }
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "const price = 99;\n"
        );
        let said = users(&view, cx);
        let (first, second) = (said[0].0, said[1].0);

        // Back to the second message: the file as it was when it was sent.
        view.update(cx, |view, cx| {
            assert!(view.can_rewind_to(second));
            view.rewind_code_to(second, cx);
            assert!(!view.can_rewind_to(second));
            assert!(view.can_rewind_to(first));
        });
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "const price = 95;\n"
        );
        // Back to the first: as before Claude touched it.
        view.update(cx, |view, cx| view.rewind_code_to(first, cx));
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "const price = 90;\n"
        );

        // A new chat from an answer: the chat up to it, and no further.
        send(&view, cx, "uno");
        wait_until(cx, "the first answer", |cx| {
            last_answer(&view, cx).as_deref() == Some("You said: uno")
        });
        send(&view, cx, "due");
        wait_until(cx, "the second answer", |cx| {
            last_answer(&view, cx).as_deref() == Some("You said: due")
        });
        let before = view.read_with(cx, |view, _| view.agent_panel.session.clone());
        let answer = view.read_with(cx, |view, _| {
            view.agent_panel
                .thread
                .entries
                .iter()
                .position(|entry| *entry == Entry::Agent("You said: uno".into()))
                .unwrap()
        });
        view.update_in(cx, |view, window, cx| {
            view.fork_agent_chat(answer, window, cx)
        });
        wait_until(cx, "the fork", |cx| {
            view.read_with(cx, |view, _| {
                view.agent_panel.session != before
                    && view
                        .agent_panel
                        .thread
                        .entries
                        .contains(&Entry::Agent("You said: uno".into()))
            })
        });
        let texts: Vec<String> = users(&view, cx).into_iter().map(|(_, text)| text).collect();
        assert!(texts.contains(&"uno".to_owned()));
        assert!(!texts.contains(&"due".to_owned()));

        // From a message of yours: the chat before it, the message in the box.
        let forked = view.read_with(cx, |view, _| view.agent_panel.session.clone());
        let uno = users(&view, cx)
            .into_iter()
            .find(|(_, text)| text == "uno")
            .unwrap()
            .0;
        view.update_in(cx, |view, window, cx| view.fork_agent_chat(uno, window, cx));
        wait_until(cx, "the fork before the message", |cx| {
            view.read_with(cx, |view, cx| {
                view.agent_panel.session != forked
                    && view.agent_panel.input.read(cx).value() == "uno"
            })
        });
        let texts: Vec<String> = users(&view, cx).into_iter().map(|(_, text)| text).collect();
        assert!(!texts.contains(&"uno".to_owned()), "{texts:?}");
    }

    #[gpui::test]
    fn the_plan_opens_in_a_tab_and_goes_back_with_notes(cx: &mut TestAppContext) {
        cx.executor().allow_parking();
        cx.update(|cx| {
            gpui_component::init(cx);
            language::register_grammars();
            init(cx);
        });
        let (view, cx) = cx.add_window_view(|window, cx| {
            let mut workbench = Workbench::new(None, test_settings(), None, window, cx);
            workbench.agent_panel.fake = true;
            workbench
        });
        view.update_in(cx, |view, window, cx| view.open_agent(window, cx));
        send(&view, cx, "plan");
        wait_until(cx, "the plan to approve", |cx| {
            view.read_with(cx, |view, _| !view.agent_panel.permissions.is_empty())
        });
        // The plan is a Markdown tab of its own.
        let uri = view.read_with(cx, |view, _| {
            let permission = &view.agent_panel.permissions[0];
            assert!(permission.plan.as_deref().unwrap().starts_with("# Piano"));
            let (uri, _) = view.agent_panel.plan_doc.clone().unwrap();
            let document = &view.documents[&uri];
            assert_eq!(document.language_id, "markdown");
            assert!(view.panes.iter().any(|pane| pane.uri == uri));
            uri
        });
        // Unchanged, there is nothing to send back.
        view.update_in(cx, |view, window, cx| {
            view.revise_agent_plan(0, window, cx);
            assert_eq!(view.agent_panel.permissions.len(), 1);
            assert!(view.agent_panel.notice.is_some());
        });
        // A note in the plan, then "Revise with my notes".
        view.update_in(cx, |view, window, cx| {
            let text = "# Piano\n\n1. Leggi il file\n2. Cambia il prezzo\n> nota: a 95, non a 99";
            view.set_document_text(&uri, text, window, cx);
            view.revise_agent_plan(0, window, cx);
            assert!(view.agent_panel.permissions.is_empty());
            assert!(view.agent_panel.plan_doc.is_none());
            assert!(!view.documents.contains_key(&uri));
        });
        // Claude keeps planning, then reads the notes.
        wait_until(cx, "the notes to reach Claude", |cx| {
            last_answer(&view, cx).is_some_and(|answer| answer.contains("> nota: a 95"))
        });
        assert!(
            last_answer(&view, cx)
                .unwrap()
                .contains(view.read_with(cx, |view, _| view.strings().agent_plan_feedback))
        );
    }

    #[gpui::test]
    fn messages_written_while_claude_starts_all_go(cx: &mut TestAppContext) {
        cx.executor().allow_parking();
        cx.update(|cx| {
            gpui_component::init(cx);
            init(cx);
        });
        let (view, cx) = cx.add_window_view(|window, cx| {
            let mut workbench = Workbench::new(None, test_settings(), None, window, cx);
            workbench.agent_panel.fake = true;
            workbench
        });
        view.update_in(cx, |view, window, cx| view.open_agent(window, cx));
        send(&view, cx, "uno");
        send(&view, cx, "due");
        wait_until(cx, "the answer", |cx| last_answer(&view, cx).is_some());
        assert_eq!(last_answer(&view, cx).unwrap(), "You said: uno\n\ndue");
        view.read_with(cx, |view, _| {
            assert_eq!(
                view.agent_panel.thread.entries[..2],
                [Entry::User("uno".into()), Entry::User("due".into())]
            );
        });
    }

    #[test]
    fn a_message_joins_the_one_that_waits() {
        let mut waiting = vec![
            acp::text_block("prima"),
            json!({ "type": "resource_link", "uri": "file:///a.rs" }),
        ];
        join_message(
            &mut waiting,
            vec![
                acp::text_block("poi"),
                json!({ "type": "resource_link", "uri": "file:///b.rs" }),
            ],
        );
        assert_eq!(waiting.len(), 3);
        assert_eq!(waiting[0]["text"], "prima\n\npoi");
        // Only files: no empty text joins it.
        let mut files = vec![
            acp::text_block(""),
            json!({ "type": "resource_link", "uri": "file:///a.rs" }),
        ];
        join_message(&mut files, vec![acp::text_block("ecco")]);
        assert_eq!(files[0]["text"], "ecco");
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
            let mode = view.agent_option("mode").unwrap();
            assert_eq!(mode.values.len(), 2);
            assert_eq!(
                view.agent_option("model").unwrap().current.as_deref(),
                Some("opus")
            );
            assert_eq!(view.agent_panel.commands.len(), 3);
            assert!(
                view.agent_panel
                    .account
                    .as_ref()
                    .is_some_and(|account| account.signed_in)
            );
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

        // A chat takes a title of yours: at once in the list and the
        // header, and in Claude Code, which lists it so from then on.
        view.update(cx, |view, cx| {
            view.rename_agent_chat_to("session-1".to_owned(), "  Prenotazioni ".to_owned(), cx);
            assert_eq!(view.agent_title(), "Prenotazioni");
            assert!(
                view.agent_panel
                    .chats
                    .iter()
                    .any(|chat| chat.id == "session-1" && chat.title == "Prenotazioni")
            );
            view.agent_panel.chats.clear();
        });
        view.update_in(cx, |view, window, cx| {
            view.refresh_agent_history(window, cx)
        });
        wait_until(cx, "the renamed chat in the list", |cx| {
            view.read_with(cx, |view, _| {
                view.agent_panel
                    .chats
                    .iter()
                    .any(|chat| chat.id == "session-1" && chat.title == "Prenotazioni")
            })
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

        // Written while Claude works, a message waits for the turn to end;
        // Esc in the message box stops Claude, and the message goes then.
        send(&view, cx, "wait");
        wait_until(cx, "the second long turn", |cx| {
            view.read_with(cx, |view, _| view.agent_panel.running)
        });
        // Two of them go as one message, their texts joined.
        send(&view, cx, "dopo");
        send(&view, cx, "anche questo");
        view.read_with(cx, |view, _| {
            let (blocks, said) = view.agent_panel.queued.as_ref().unwrap();
            assert_eq!(said, "dopo\n\nanche questo");
            assert_eq!(blocks[0]["text"], "dopo\n\nanche questo");
            assert!(
                !view
                    .agent_panel
                    .thread
                    .entries
                    .contains(&Entry::User("dopo".into()))
            );
        });
        cx.update(|window, cx| {
            let _ = window.draw(cx);
        });
        assert!(cx.debug_bounds("agent-queued").is_some());
        view.update_in(cx, |view, window, cx| {
            view.agent_panel
                .input
                .update(cx, |input, cx| input.focus(window, cx));
            assert!(view.composer_key("escape", window, cx));
        });
        wait_until(cx, "the answer to the queued message", |cx| {
            last_answer(&view, cx).as_deref() == Some("You said: dopo\n\nanche questo")
        });
        view.read_with(cx, |view, _| {
            assert!(view.agent_panel.queued.is_none());
            let said: Vec<&str> = view
                .agent_panel
                .thread
                .entries
                .iter()
                .filter_map(|entry| match entry {
                    Entry::User(text) => Some(text.as_str()),
                    _ => None,
                })
                .collect();
            assert_eq!(said[said.len() - 2..], ["wait", "dopo\n\nanche questo"]);
        });
        // With Claude idle, Esc is left to the message box.
        view.update_in(cx, |view, window, cx| {
            assert!(!view.composer_key("escape", window, cx));
        });
    }
}
