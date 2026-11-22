//! The debugger: F5 starts the file or a configuration of `launch.json`
//! with the adapter for it, and the sessions stop, step, show their
//! variables and end. `breakpoints.rs` has the dots, `debug_config.rs`
//! the configurations, `debug_view.rs` what is drawn.

use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::time::Instant;

use dap::adapters::{self, Adapter, Missing};
use dap::client::AdapterCommand;
use dap::protocol::{Breakpoint, Capabilities, Evaluated, Scope, StackFrame, Thread, Variable};
use dap::{Client, ClientEvent, Handler};
use futures::StreamExt as _;
use futures::channel::{mpsc, oneshot};
use futures::future::{Either, select};
use gpui::{AsyncWindowContext, Div, PromptLevel, WeakEntity};
use gpui_component::ActiveTheme as _;
use gpui_component::RopeExt as _;
use gpui_component::input::{
    InputEvent, InputState, RangeDecoration, RangeDecorationCollection, RangeDecorationStyle,
};

use super::debug_config::{self, DebugConfig, LaunchFile};
use super::task_config::{TaskFile, TaskInput};
use super::*;
use crate::quick_pick::{self, PickItem};

actions!(
    keel,
    [
        DebugStart,
        DebugRun,
        DebugSelectAndStart,
        DebugStop,
        DebugRestart,
        DebugPause,
        DebugStepOver,
        DebugStepInto,
        DebugStepOut,
        DebugRunToCursor,
        DebugConfigure,
        ShowDebug,
        ToggleDebugConsole,
        DebugEvaluateSelection,
        DebugWatchSelection
    ]
);

/// Lines the console keeps.
const CONSOLE_LINES: usize = 5000;
/// How long an adapter asked to end has before it is ended.
const END_PATIENCE: Duration = Duration::from_millis(1500);
/// How long js-debug's server has to say where it listens.
const SERVER_PATIENCE: Duration = Duration::from_secs(15);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum SessionState {
    Starting,
    Running,
    Stopped,
}

/// A breakpoint as a debugger took it.
#[derive(Clone, Debug)]
struct Taken {
    id: Option<i64>,
    verified: bool,
    message: Option<String>,
}

pub(super) struct Session {
    pub(super) id: u64,
    parent: Option<u64>,
    pub(super) name: String,
    config: DebugConfig,
    no_debug: bool,
    client: Arc<Client>,
    /// js-debug's address, for the sessions it opens.
    server: Option<String>,
    pub(super) capabilities: Capabilities,
    pub(super) state: SessionState,
    initialized: Option<oneshot::Sender<()>>,
    /// Initialized: it takes breakpoints.
    ready: bool,
    pub(super) threads: Vec<Thread>,
    pub(super) thread: Option<i64>,
    pub(super) frames: Vec<StackFrame>,
    pub(super) frame: usize,
    pub(super) scopes: Vec<Scope>,
    pub(super) variables: HashMap<i64, Vec<Variable>>,
    /// The variables asked and not come yet.
    coming: HashSet<i64>,
    /// Why it stopped.
    pub(super) stop: Option<String>,
    /// Counts the stops: what was asked before the last is thrown away.
    epoch: u64,
    taken: HashMap<(PathBuf, u32), Taken>,
    ending: bool,
    _events: Task<()>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum LineKind {
    Output,
    Error,
    /// What Keel or the adapter says.
    Note,
    /// What was typed, and its answer.
    Typed,
    Answer,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct ConsoleLine {
    pub(super) kind: LineKind,
    pub(super) text: String,
}

/// The Debug Console's lines, and the field to type in.
#[derive(Default)]
pub(super) struct Console {
    pub(super) lines: Vec<ConsoleLine>,
    /// The last line has not ended yet.
    open: bool,
    pub(super) input: Option<Entity<InputState>>,
    history: Vec<String>,
    pub(super) scroll: gpui::UniformListScrollHandle,
    _input: Option<Subscription>,
}

impl Console {
    pub(super) fn write(&mut self, kind: LineKind, text: &str) {
        for (index, piece) in text.split('\n').enumerate() {
            let piece = piece.trim_end_matches('\r');
            if index == 0
                && self.open
                && let Some(last) = self.lines.last_mut().filter(|last| last.kind == kind)
            {
                last.text.push_str(&without_escapes(piece));
                continue;
            }
            // The end of a text that ended its line.
            if index > 0 && piece.is_empty() && text.ends_with('\n') && {
                index == text.matches('\n').count()
            } {
                break;
            }
            self.lines.push(ConsoleLine {
                kind,
                text: without_escapes(piece),
            });
        }
        self.open = !text.ends_with('\n') && matches!(kind, LineKind::Output | LineKind::Error);
        if self.lines.len() > CONSOLE_LINES {
            self.lines.drain(..self.lines.len() - CONSOLE_LINES);
        }
        let last = self.lines.len().saturating_sub(1);
        self.scroll
            .scroll_to_item(last, gpui::ScrollStrategy::Bottom);
    }
}

/// A text without the terminal's colours.
fn without_escapes(text: &str) -> String {
    static ESCAPE: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
        regex::Regex::new(r"\x1b(?:\[[0-?]*[ -/]*[@-~]|\][^\x07\x1b]*(?:\x07|\x1b\\)|[@-Z\\-_])")
            .expect("the escapes")
    });
    ESCAPE.replace_all(text, "").into_owned()
}

/// An expression always evaluated.
#[derive(Clone, Debug)]
pub(super) struct Watch {
    pub(super) expression: String,
    pub(super) value: Option<Result<Evaluated, String>>,
}

#[derive(Default)]
pub(super) struct Debugging {
    pub(super) breakpoints: breakpoints::Breakpoints,
    pub(super) sessions: Vec<Session>,
    /// The session the toolbar and the view are about.
    pub(super) active: Option<u64>,
    next_id: u64,
    pub(super) console: Console,
    pub(super) watches: Vec<Watch>,
    /// The variables opened, by where they are: `Locals/nomi/[0]`.
    pub(super) open: HashSet<String>,
    /// Scopes and sections closed by hand.
    pub(super) closed: HashSet<String>,
    /// The configuration F5 starts.
    pub(super) chosen: Option<String>,
    /// What is being started, for the status bar.
    pub(super) starting: Option<String>,
    /// The name in the view, and when it was read.
    #[allow(clippy::type_complexity)]
    shown: std::cell::RefCell<Option<(Instant, Option<String>, Option<String>)>>,
    /// The exceptions to stop at, where the user said otherwise.
    exceptions: HashMap<String, bool>,
    decor: HashMap<gpui::EntityId, RangeDecorationCollection>,
    /// The values beside the lines of the frame's file.
    pub(super) inline: Option<(String, HashMap<usize, Vec<String>>)>,
    #[cfg(test)]
    pub(super) fake: bool,
}

impl Debugging {
    pub(super) fn session(&self) -> Option<&Session> {
        let active = self.active?;
        self.sessions.iter().find(|session| session.id == active)
    }

    fn session_mut(&mut self, id: u64) -> Option<&mut Session> {
        self.sessions.iter_mut().find(|session| session.id == id)
    }

    /// The session the user started, above those it opened.
    fn root_of(&self, id: u64) -> u64 {
        let mut id = id;
        while let Some(parent) = self
            .sessions
            .iter()
            .find(|session| session.id == id)
            .and_then(|session| session.parent)
        {
            id = parent;
        }
        id
    }
}

impl Session {
    pub(super) fn top_frame(&self) -> Option<&StackFrame> {
        self.frames.get(self.frame)
    }

    /// The variables that are to be shown and are not read yet.
    fn unread(&self, open: &HashSet<String>, closed: &HashSet<String>) -> Vec<i64> {
        let mut wanted = Vec::new();
        for scope in &self.scopes {
            let path = scope.name.clone();
            let shown = if scope.expensive {
                open.contains(&path)
            } else {
                !closed.contains(&path)
            };
            if shown {
                self.walk(scope.variables_reference, &path, open, &mut wanted);
            }
        }
        wanted
    }

    fn walk(&self, reference: i64, path: &str, open: &HashSet<String>, wanted: &mut Vec<i64>) {
        if reference == 0 {
            return;
        }
        let Some(variables) = self.variables.get(&reference) else {
            if !self.coming.contains(&reference) {
                wanted.push(reference);
            }
            return;
        };
        for variable in variables {
            let path = format!("{path}/{}", variable.name);
            if open.contains(&path) {
                self.walk(variable.variables_reference, &path, open, wanted);
            }
        }
    }
}

/// A list of an answer, without what cannot be read in it.
fn list<T: serde::de::DeserializeOwned>(value: &Value) -> Vec<T> {
    value
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|value| serde_json::from_value(value.clone()).ok())
        .collect()
}

/// What hears an adapter, from its thread to the window's.
fn channel() -> (Handler, mpsc::UnboundedReceiver<ClientEvent>) {
    let (sender, receiver) = mpsc::unbounded();
    let handler: Handler = Arc::new(move |event| {
        let _ = sender.unbounded_send(event);
    });
    (handler, receiver)
}

/// What starts a session.
struct Opening {
    config: DebugConfig,
    no_debug: bool,
    request: String,
    arguments: Value,
    client: Arc<Client>,
    events: mpsc::UnboundedReceiver<ClientEvent>,
    server: Option<String>,
    parent: Option<u64>,
}

type Cancelled = Result<(), Option<String>>;

impl Workbench {
    pub(super) fn debug_actions(&self, root: Div, cx: &mut Context<Self>) -> Div {
        root.on_action(
            cx.listener(|this, _: &DebugStart, window, cx| this.start_or_continue(window, cx)),
        )
        .on_action(
            cx.listener(|this, _: &DebugRun, window, cx| this.start_chosen(true, window, cx)),
        )
        .on_action(cx.listener(|this, _: &DebugSelectAndStart, window, cx| {
            this.choose_configuration(true, window, cx)
        }))
        .on_action(cx.listener(|this, _: &DebugStop, window, cx| this.stop_debugging(window, cx)))
        .on_action(
            cx.listener(|this, _: &DebugRestart, window, cx| this.restart_debugging(window, cx)),
        )
        .on_action(cx.listener(|this, _: &DebugPause, _, cx| this.pause_debugging(cx)))
        .on_action(cx.listener(|this, _: &DebugStepOver, _, cx| this.debug_step("next", cx)))
        .on_action(cx.listener(|this, _: &DebugStepInto, _, cx| this.debug_step("stepIn", cx)))
        .on_action(cx.listener(|this, _: &DebugStepOut, _, cx| this.debug_step("stepOut", cx)))
        .on_action(cx.listener(|this, _: &DebugRunToCursor, _, cx| this.run_to_cursor(cx)))
        .on_action(
            cx.listener(|this, _: &DebugConfigure, window, cx| {
                this.configure_debugging(window, cx)
            }),
        )
        .on_action(
            cx.listener(|this, _: &ShowDebug, _, cx| this.show_side_view(SideView::Debug, cx)),
        )
        .on_action(cx.listener(|this, _: &ToggleDebugConsole, window, cx| {
            let shown = this.layout.panel && this.layout.panel_view == PanelView::DebugConsole;
            if shown {
                this.layout.panel = false;
            } else {
                this.layout.show_panel_view(PanelView::DebugConsole);
                let input = this.console_input(window, cx);
                input.update(cx, |input, cx| input.focus(window, cx));
            }
            cx.notify();
        }))
        .on_action(cx.listener(|this, _: &DebugEvaluateSelection, _, cx| {
            if let Some(text) = this.selected_or_word(cx) {
                this.evaluate_in_console(text, cx);
                this.layout.show_panel_view(PanelView::DebugConsole);
            }
        }))
        .on_action(cx.listener(|this, _: &DebugWatchSelection, _, cx| {
            if let Some(text) = this.selected_or_word(cx) {
                this.add_watch(text, cx);
            }
        }))
    }

    // -----------------------------------------------------------------
    // Configurations

    /// The project's `launch.json`, as it is in its tab when it is open.
    fn read_launch_file(&self) -> Result<LaunchFile, String> {
        let Some(root) = self.project.root.clone() else {
            return Ok(LaunchFile::default());
        };
        let path = root.join(".vscode/launch.json");
        let uri = Uri::file(&path).to_string();
        let text = match self.documents.get(&uri) {
            Some(document) => Some(document.synced_text.clone()),
            None => std::fs::read_to_string(&path).ok(),
        };
        text.map_or_else(
            || Ok(LaunchFile::default()),
            |text| debug_config::read(&text),
        )
    }

    fn launch_file(&mut self, cx: &mut Context<Self>) -> LaunchFile {
        match self.read_launch_file() {
            Ok(file) => file,
            Err(error) => {
                let message = notices::fill(self.strings().debug_bad_file, &[("error", &error)]);
                self.report(message);
                cx.notify();
                LaunchFile::default()
            }
        }
    }

    /// What can be started: written, then found.
    fn configurations(&mut self, cx: &mut Context<Self>) -> (LaunchFile, Vec<DebugConfig>) {
        let file = self.launch_file(cx);
        let all = self.with_found(&file);
        (file, all)
    }

    fn with_found(&self, file: &LaunchFile) -> Vec<DebugConfig> {
        let root = self.terminal_folder();
        let document = self
            .panes
            .get(self.active)
            .and_then(|pane| self.documents.get(&pane.uri));
        let active = document.and_then(|document| {
            Some((
                document.disk_path()?.to_path_buf(),
                document.language_id.clone(),
            ))
        });
        let found = debug_config::found(
            &root,
            active
                .as_ref()
                .map(|(path, language)| (path.as_path(), language.as_str())),
        );
        let mut all = file.configs.clone();
        for config in found {
            if !all.iter().any(|known| known.name == config.name) {
                all.push(config);
            }
        }
        all
    }

    /// The name of what F5 starts now, for the view: read again now and
    /// then, not at each drawing.
    pub(super) fn shown_configuration(&self) -> Option<String> {
        const FRESH: Duration = Duration::from_secs(1);
        let mut shown = self.debug.shown.borrow_mut();
        if let Some((at, chosen, name)) = shown.as_ref()
            && at.elapsed() < FRESH
            && *chosen == self.debug.chosen
        {
            return name.clone();
        }
        let file = self.read_launch_file().unwrap_or_default();
        let name = self.chosen_among(&file, &self.with_found(&file));
        *shown = Some((Instant::now(), self.debug.chosen.clone(), name.clone()));
        name
    }

    /// The name of what F5 starts now.
    pub(super) fn chosen_configuration(&mut self, cx: &mut Context<Self>) -> Option<String> {
        let (file, all) = self.configurations(cx);
        self.chosen_among(&file, &all)
    }

    fn chosen_among(&self, file: &LaunchFile, all: &[DebugConfig]) -> Option<String> {
        let named = |name: &String| {
            all.iter().any(|config| config.name == *name)
                || file.compounds.iter().any(|compound| compound.name == *name)
        };
        if let Some(chosen) = self.debug.chosen.clone().filter(named) {
            return Some(chosen);
        }
        all.first().map(|config| config.name.clone())
    }

    /// F5: goes on when stopped, or starts.
    fn start_or_continue(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match self.debug.session().map(|session| session.state) {
            Some(SessionState::Stopped) => self.debug_step("continue", cx),
            Some(_) => {}
            None => self.start_chosen(false, window, cx),
        }
    }

    pub(super) fn start_chosen(
        &mut self,
        no_debug: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Several found and none ever chosen: asked once.
        let (file, all) = self.configurations(cx);
        let known = self.debug.chosen.is_some() || file.configs.len() == 1 || all.len() == 1;
        match self.chosen_configuration(cx) {
            Some(name) if known => self.start_named(&name, no_debug, window, cx),
            Some(_) => self.choose_configuration(true, window, cx),
            None => {
                let strings = self.strings();
                self.notify_user(
                    notices::Notice::info(strings.debug_nothing)
                        .message(strings.debug_nothing_detail),
                    window,
                    cx,
                );
            }
        }
    }

    /// Starts a configuration, or those of a compound.
    pub(super) fn start_named(
        &mut self,
        name: &str,
        no_debug: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (file, all) = self.configurations(cx);
        self.debug.chosen = Some(name.to_owned());
        let compound = file
            .compounds
            .iter()
            .find(|compound| compound.name == name)
            .cloned();
        let Some(compound) = compound else {
            if let Some(config) = all.into_iter().find(|config| config.name == name) {
                self.start_debugging(config, no_debug, window, cx);
            }
            return;
        };
        let configs: Vec<DebugConfig> = all
            .into_iter()
            .filter(|config| compound.configurations.contains(&config.name))
            .collect();
        cx.spawn_in(window, async move |this, cx| {
            if let Some(label) = compound.before {
                let Ok(task) =
                    this.update_in(cx, |this, window, cx| this.task_outcome(&label, window, cx))
                else {
                    return;
                };
                if !task.await {
                    return;
                }
            }
            this.update_in(cx, |this, window, cx| {
                for config in configs {
                    this.start_debugging(config, no_debug, window, cx);
                }
            })
            .ok();
        })
        .detach();
    }

    /// Lists what can be started; the one chosen is F5's from now on.
    pub(super) fn choose_configuration(
        &mut self,
        start: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (file, all) = self.configurations(cx);
        let strings = self.strings();
        let mut names: Vec<String> = Vec::new();
        let mut items = Vec::new();
        for config in &all {
            let mut item = PickItem::new(config.name.clone());
            item.detail = Some(if config.written {
                format!("launch.json · {}", config.kind)
            } else {
                strings.debug_found.to_owned()
            });
            names.push(config.name.clone());
            items.push(item);
        }
        for compound in &file.compounds {
            let mut item = PickItem::new(compound.name.clone());
            item.detail = Some(compound.configurations.join(" + "));
            names.push(compound.name.clone());
            items.push(item);
        }
        if items.is_empty() {
            return self.start_chosen(false, window, cx);
        }
        let chosen = quick_pick::show(
            None,
            Some(strings.debug_choose.to_owned()),
            items,
            window,
            cx,
        );
        cx.spawn_in(window, async move |this, cx| {
            let Some(index) = chosen.await.ok().flatten() else {
                return;
            };
            this.update_in(cx, |this, window, cx| {
                this.debug.chosen = Some(names[index].clone());
                cx.notify();
                if start {
                    this.start_named(&names[index], false, window, cx);
                }
            })
            .ok();
        })
        .detach();
    }

    /// Opens `launch.json`, written from what Keel finds when there is none.
    pub(super) fn configure_debugging(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(root) = self.project.root.clone() else {
            return;
        };
        let path = root.join(".vscode/launch.json");
        if !path.exists() {
            let (_, all) = self.configurations(cx);
            let written = path
                .parent()
                .map_or(Ok(()), std::fs::create_dir_all)
                .and_then(|()| std::fs::write(&path, debug_config::template(&all)));
            if let Err(error) = written {
                self.report(format!("{}: {error}", path.display()));
                cx.notify();
                return;
            }
        }
        self.open_file(&path, window, cx);
    }

    // -----------------------------------------------------------------
    // Starting

    pub(super) fn start_debugging(
        &mut self,
        config: DebugConfig,
        no_debug: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.save_all_quietly(window, cx);
        self.debug.starting = Some(config.name.clone());
        cx.notify();
        cx.spawn_in(window, async move |this, cx| {
            let name = config.name.clone();
            let started = start(&this, config, no_debug, cx).await;
            this.update_in(cx, |this, window, cx| {
                this.debug.starting = None;
                if let Err(Some(error)) = started {
                    let title = notices::fill(this.strings().debug_failed, &[("name", &name)]);
                    this.debug
                        .console
                        .write(LineKind::Error, &format!("{title}\n{error}\n"));
                    this.notify_user(notices::Notice::error(title).message(error), window, cx);
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn adapters_dir(&self) -> PathBuf {
        self.data_dir
            .as_deref()
            .map(crate::paths::servers_dir)
            .unwrap_or_else(|| std::env::temp_dir().join("keel-adapters"))
    }

    /// Puts a session in the list and hears its adapter.
    fn add_session(
        &mut self,
        opening: Opening,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> (u64, Arc<Client>, oneshot::Receiver<()>) {
        self.debug.next_id += 1;
        let id = self.debug.next_id;
        let (initialized, heard) = oneshot::channel();
        let mut events = opening.events;
        let pump = cx.spawn_in(window, async move |this, cx| {
            while let Some(event) = events.next().await {
                let heard = this.update_in(cx, |this, window, cx| {
                    this.debug_event(id, event, window, cx)
                });
                if heard.is_err() {
                    break;
                }
            }
        });
        let client = opening.client.clone();
        self.debug.sessions.push(Session {
            id,
            parent: opening.parent,
            name: opening.config.name.clone(),
            config: opening.config,
            no_debug: opening.no_debug,
            client: opening.client,
            server: opening.server,
            capabilities: Capabilities::default(),
            state: SessionState::Starting,
            initialized: Some(initialized),
            ready: false,
            threads: Vec::new(),
            thread: None,
            frames: Vec::new(),
            frame: 0,
            scopes: Vec::new(),
            variables: HashMap::new(),
            coming: HashSet::new(),
            stop: None,
            epoch: 0,
            taken: HashMap::new(),
            ending: false,
            _events: pump,
        });
        if self.debug.session().is_none() {
            self.debug.active = Some(id);
        }
        cx.notify();
        (id, client, heard)
    }

    /// Breakpoints and exceptions, once an adapter is ready for them.
    fn configure_session(&mut self, id: u64, cx: &mut Context<Self>) {
        let files = self.debug.breakpoints.files();
        let Some(session) = self.debug.session_mut(id) else {
            return;
        };
        session.ready = true;
        for path in files {
            self.send_breakpoints_to(id, &path, cx);
        }
        self.send_exceptions(id);
    }

    fn send_exceptions(&mut self, id: u64) {
        let wanted = self.debug.exceptions.clone();
        let Some(session) = self.debug.session_mut(id) else {
            return;
        };
        let known = &session.capabilities.exception_breakpoint_filters;
        if known.is_empty() {
            return;
        }
        let filters: Vec<&str> = known
            .iter()
            .filter(|filter| *wanted.get(&filter.filter).unwrap_or(&filter.default))
            .map(|filter| filter.filter.as_str())
            .collect();
        drop(
            session
                .client
                .request("setExceptionBreakpoints", json!({ "filters": filters })),
        );
    }

    /// Whether a kind of exception is to be stopped at.
    pub(super) fn exception_wanted(&self, filter: &str, default: bool) -> bool {
        *self.debug.exceptions.get(filter).unwrap_or(&default)
    }

    pub(super) fn want_exception(&mut self, filter: &str, on: bool, cx: &mut Context<Self>) {
        self.debug.exceptions.insert(filter.to_owned(), on);
        let ids: Vec<u64> = self
            .debug
            .sessions
            .iter()
            .filter(|session| session.ready)
            .map(|session| session.id)
            .collect();
        for id in ids {
            self.send_exceptions(id);
        }
        cx.notify();
    }

    /// Shows another thread's stack.
    pub(super) fn select_thread(
        &mut self,
        id: u64,
        thread: i64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.debug.active = Some(id);
        if let Some(session) = self.debug.session_mut(id) {
            session.thread = Some(thread);
        }
        self.load_stack(id, true, window, cx);
    }

    // -----------------------------------------------------------------
    // Breakpoints

    /// Tells every debugger a file's breakpoints.
    pub(super) fn send_breakpoints(&mut self, path: &Path, cx: &mut Context<Self>) {
        let ids: Vec<u64> = self
            .debug
            .sessions
            .iter()
            .filter(|session| session.ready)
            .map(|session| session.id)
            .collect();
        for id in ids {
            self.send_breakpoints_to(id, path, cx);
        }
    }

    fn send_breakpoints_to(&mut self, id: u64, path: &Path, cx: &mut Context<Self>) {
        let asked = self.debug.breakpoints.asked(path);
        let Some(session) = self.debug.session_mut(id) else {
            return;
        };
        let lines: Vec<u32> = asked.iter().map(|breakpoint| breakpoint.line).collect();
        let answer = session.client.request(
            "setBreakpoints",
            json!({
                "source": { "name": path.file_name().map(|name| name.to_string_lossy()), "path": path },
                "breakpoints": asked,
                "lines": lines,
                "sourceModified": false,
            }),
        );
        let path = path.to_path_buf();
        cx.spawn(async move |this, cx| {
            let answer = answer.await;
            this.update(cx, |this, cx| {
                let Some(session) = this.debug.session_mut(id) else {
                    return;
                };
                session.taken.retain(|(known, _), _| *known != path);
                let taken: Vec<Breakpoint> = match &answer {
                    Ok(answer) => list(&answer["breakpoints"]),
                    Err(_) => Vec::new(),
                };
                for (line, taken) in lines.iter().zip(taken) {
                    session.taken.insert(
                        (path.clone(), line - 1),
                        Taken {
                            id: taken.id,
                            verified: taken.verified,
                            message: taken.message,
                        },
                    );
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Why no debugger took a breakpoint, when one is running and none did.
    pub(super) fn breakpoint_refused(&self, path: &Path, line: u32) -> Option<String> {
        let key = (path.to_path_buf(), line);
        let said: Vec<&Taken> = self
            .debug
            .sessions
            .iter()
            .filter_map(|session| session.taken.get(&key))
            .collect();
        if said.is_empty() || said.iter().any(|taken| taken.verified) {
            return None;
        }
        Some(
            said.iter()
                .find_map(|taken| taken.message.clone())
                .unwrap_or_default(),
        )
    }

    // -----------------------------------------------------------------
    // What the adapter says

    fn debug_event(
        &mut self,
        id: u64,
        event: ClientEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            ClientEvent::Event { event, body } => match event.as_str() {
                "initialized" => {
                    let session = self.debug.session_mut(id);
                    if let Some(sender) = session.and_then(|session| session.initialized.take()) {
                        let _ = sender.send(());
                    }
                }
                "stopped" => self.debug_stopped(id, &body, window, cx),
                "continued" => self.debug_continued(id, cx),
                "output" => {
                    let kind = match body["category"].as_str() {
                        Some("stderr" | "important") => LineKind::Error,
                        Some("console") => LineKind::Note,
                        Some("telemetry") => return,
                        _ => LineKind::Output,
                    };
                    let text = body["output"].as_str();
                    // LLDB says it at each run; Rust's own formatters are
                    // there for it.
                    let usual = text.is_some_and(|text| {
                        text.contains("has no plugin for the language \"rust\"")
                    });
                    if let Some(text) = text.filter(|_| !usual) {
                        self.debug.console.write(kind, text);
                        cx.notify();
                    }
                }
                "exited" => {
                    let code = body["exitCode"].as_i64().unwrap_or_default();
                    let text =
                        notices::fill(self.strings().debug_exited, &[("code", &code.to_string())]);
                    self.debug
                        .console
                        .write(LineKind::Note, &format!("{text}\n"));
                    cx.notify();
                }
                "terminated" => self.end_session(id, window, cx),
                "breakpoint" => {
                    let changed: Breakpoint =
                        serde_json::from_value(body["breakpoint"].clone()).unwrap_or_default();
                    if let Some(session) = self.debug.session_mut(id)
                        && changed.id.is_some()
                        && let Some(taken) = session
                            .taken
                            .values_mut()
                            .find(|taken| taken.id == changed.id)
                    {
                        taken.verified = changed.verified;
                        taken.message = changed.message;
                        cx.notify();
                    }
                }
                "thread" => {
                    if body["reason"] == "exited"
                        && let Some(session) = self.debug.session_mut(id)
                    {
                        let gone = body["threadId"].as_i64();
                        session.threads.retain(|thread| Some(thread.id) != gone);
                    }
                }
                "capabilities" => {
                    if let Some(session) = self.debug.session_mut(id)
                        && let Ok(changed) =
                            serde_json::from_value::<Capabilities>(body["capabilities"].clone())
                    {
                        let filters = changed.exception_breakpoint_filters.clone();
                        if !filters.is_empty() {
                            session.capabilities.exception_breakpoint_filters = filters;
                        }
                    }
                }
                "invalidated" => self.reload_variables(id, cx),
                _ => {}
            },
            ClientEvent::Request {
                seq,
                command,
                arguments,
            } => self.debug_request(id, seq, &command, &arguments, window, cx),
            ClientEvent::Stderr(line) => {
                let name = self
                    .debug
                    .session_mut(id)
                    .map(|session| session.name.clone());
                self.log(format!("[{}] {line}", name.unwrap_or_default()));
            }
            ClientEvent::Closed => self.end_session(id, window, cx),
        }
    }

    /// What an adapter asks of the editor.
    fn debug_request(
        &mut self,
        id: u64,
        seq: u64,
        command: &str,
        arguments: &Value,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(session) = self.debug.session_mut(id) else {
            return;
        };
        let client = session.client.clone();
        match command {
            "runInTerminal" => {
                let title = arguments["title"]
                    .as_str()
                    .map(str::to_owned)
                    .unwrap_or_else(|| session.name.clone());
                let answer = self.run_in_terminal(&title, arguments, window, cx);
                client.respond(seq, command, answer);
            }
            // js-debug opens a session for each program it finds.
            "startDebugging" => {
                let Some(server) = session.server.clone() else {
                    client.respond(seq, command, Err("no server to connect to".to_owned()));
                    return;
                };
                let mut config = session.config.clone();
                let no_debug = session.no_debug;
                if let Some(name) = arguments["configuration"]["name"].as_str() {
                    config.name = name.to_owned();
                }
                client.respond(seq, command, Ok(json!({})));
                let request = arguments["request"].as_str().unwrap_or("launch").to_owned();
                let arguments = arguments["configuration"].clone();
                cx.spawn_in(window, async move |this, cx| {
                    let (handler, events) = channel();
                    let address = server.clone();
                    let connected = cx
                        .background_executor()
                        .spawn(async move { Client::connect(&address, handler) })
                        .await;
                    let Ok(client) = connected else {
                        return;
                    };
                    let opening = Opening {
                        config,
                        no_debug,
                        request,
                        arguments,
                        client,
                        events,
                        server: Some(server),
                        parent: Some(id),
                    };
                    let _ = open(&this, opening, cx).await;
                })
                .detach();
            }
            _ => client.respond(seq, command, Err(format!("{command} is not known here"))),
        }
    }

    /// Runs what an adapter asks in a terminal of Keel's, as if typed.
    fn run_in_terminal(
        &mut self,
        title: &str,
        arguments: &Value,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<Value, String> {
        let words = debug_config::words(&arguments["args"]);
        if words.is_empty() {
            return Err("nothing to run".to_owned());
        }
        let cwd = arguments["cwd"]
            .as_str()
            .filter(|cwd| !cwd.is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| self.terminal_folder());
        let mut spawn = terminal_view::shell_spawn(cwd, cx);
        for (name, value) in arguments["env"].as_object().into_iter().flatten() {
            match value {
                Value::String(value) => {
                    spawn.env.insert(name.clone(), value.clone());
                }
                _ => {
                    spawn.env.remove(name);
                }
            }
        }
        let command: Vec<String> = words
            .iter()
            .map(|word| terminal_view::shell_quote(word))
            .collect();
        let view = self
            .add_terminal(spawn, None, false, window, cx)
            .ok_or("no terminal")?;
        self.name_terminal(&view, title.to_owned());
        let command = super::tasks::one_line(&command.join(" "));
        view.update(cx, |view, cx| view.run_at_prompt(&command, cx));
        Ok(json!({}))
    }

    fn debug_stopped(
        &mut self,
        id: u64,
        body: &Value,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // The line run to has been reached, or something before it.
        if let Some((path, _)) = self.debug.breakpoints.once.take() {
            self.send_breakpoints(&path, cx);
        }
        let reason = body["reason"].as_str().unwrap_or_default().to_owned();
        let said = body["text"]
            .as_str()
            .or(body["description"].as_str())
            .filter(|said| !said.is_empty())
            .map(str::to_owned);
        let Some(session) = self.debug.session_mut(id) else {
            return;
        };
        session.state = SessionState::Stopped;
        session.epoch += 1;
        session.thread = body["threadId"].as_i64().or(session.thread);
        session.stop = said.clone().or(Some(reason.clone()));
        self.debug.active = Some(id);
        if reason == "exception" {
            let text = said.unwrap_or_default();
            let text = notices::fill(self.strings().debug_exception, &[("text", &text)]);
            self.debug
                .console
                .write(LineKind::Error, &format!("{text}\n"));
        }
        // Another app is in front: the Dock says the program stopped.
        if !window.is_window_active() {
            crate::mac::needs_attention();
        }
        self.load_stack(id, true, window, cx);
        cx.notify();
    }

    fn debug_continued(&mut self, id: u64, cx: &mut Context<Self>) {
        if let Some(session) = self.debug.session_mut(id) {
            session.state = SessionState::Running;
            session.epoch += 1;
            session.frames.clear();
            session.stop = None;
        }
        self.debug.inline = None;
        self.show_debug_line(cx);
        cx.notify();
    }

    /// Reads the threads and the stack of the one that stopped.
    pub(super) fn load_stack(
        &mut self,
        id: u64,
        open_file: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(session) = self.debug.session_mut(id) else {
            return;
        };
        let (client, epoch, wanted) = (session.client.clone(), session.epoch, session.thread);
        cx.spawn_in(window, async move |this, cx| {
            let threads: Vec<Thread> = match client.request("threads", Value::Null).await {
                Ok(answer) => list(&answer["threads"]),
                Err(_) => Vec::new(),
            };
            let Some(thread) = wanted.or(threads.first().map(|thread| thread.id)) else {
                return;
            };
            let frames: Vec<StackFrame> = match client
                .request(
                    "stackTrace",
                    json!({ "threadId": thread, "startFrame": 0, "levels": 100 }),
                )
                .await
            {
                Ok(answer) => list(&answer["stackFrames"]),
                Err(_) => Vec::new(),
            };
            this.update_in(cx, |this, window, cx| {
                let Some(session) = this.debug.session_mut(id) else {
                    return;
                };
                if session.epoch != epoch {
                    return;
                }
                session.threads = threads;
                session.thread = Some(thread);
                // The first that is a file of the disk's.
                let first = frames.iter().position(|frame| {
                    frame
                        .presentation_hint
                        .as_deref()
                        .is_none_or(|hint| hint == "normal")
                        && frame_file(frame).is_some()
                });
                session.frames = frames;
                this.select_frame(id, first.unwrap_or(0), open_file, window, cx);
            })
            .ok();
        })
        .detach();
    }

    /// A frame of the stack: its line in front, its variables in the view.
    pub(super) fn select_frame(
        &mut self,
        id: u64,
        frame: usize,
        open_file: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.debug.active = Some(id);
        let Some(session) = self.debug.session_mut(id) else {
            return;
        };
        session.frame = frame;
        let chosen = session.frames.get(frame).cloned();
        if let Some(chosen) = &chosen
            && open_file
            && let Some(path) = frame_file(chosen)
        {
            self.open_file(&path, window, cx);
            self.go_to(chosen.line, 1, window, cx);
        }
        self.debug.inline = None;
        self.show_debug_line(cx);
        self.load_scopes(id, cx);
        self.evaluate_watches(cx);
        cx.notify();
    }

    /// Reads the scopes of the frame chosen, and what is open in them.
    fn load_scopes(&mut self, id: u64, cx: &mut Context<Self>) {
        let Some(session) = self.debug.session_mut(id) else {
            return;
        };
        let Some(frame) = session.top_frame().map(|frame| frame.id) else {
            session.scopes.clear();
            session.variables.clear();
            return;
        };
        let (client, epoch) = (session.client.clone(), session.epoch);
        cx.spawn(async move |this, cx| {
            let answer = client.request("scopes", json!({ "frameId": frame })).await;
            let mut scopes: Vec<Scope> = match answer {
                Ok(answer) => list(&answer["scopes"]),
                Err(_) => Vec::new(),
            };
            // Long and seldom wanted: closed until they are opened.
            for scope in &mut scopes {
                let name = scope.name.to_lowercase();
                scope.expensive |= name.contains("register") || name.contains("global");
            }
            this.update(cx, |this, cx| {
                let Some(session) = this.debug.session_mut(id) else {
                    return;
                };
                let same = session.top_frame().map(|frame| frame.id) == Some(frame);
                if session.epoch != epoch || !same {
                    return;
                }
                session.scopes = scopes;
                session.variables.clear();
                session.coming.clear();
                this.load_variables(id, cx);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Reads the variables shown and not read yet, one layer after the
    /// other.
    pub(super) fn load_variables(&mut self, id: u64, cx: &mut Context<Self>) {
        let (open, closed) = (self.debug.open.clone(), self.debug.closed.clone());
        let Some(session) = self.debug.session_mut(id) else {
            return;
        };
        let wanted = session.unread(&open, &closed);
        if wanted.is_empty() {
            if session.coming.is_empty() {
                self.show_inline_values(cx);
            }
            return;
        }
        let (client, epoch) = (session.client.clone(), session.epoch);
        // Not asked twice while they come.
        session.coming.extend(&wanted);
        cx.spawn(async move |this, cx| {
            let answers = futures::future::join_all(wanted.iter().map(|reference| {
                client.request("variables", json!({ "variablesReference": reference }))
            }))
            .await;
            this.update(cx, |this, cx| {
                let Some(session) = this.debug.session_mut(id) else {
                    return;
                };
                if session.epoch != epoch {
                    return;
                }
                for (reference, answer) in wanted.iter().zip(answers) {
                    session.coming.remove(reference);
                    let variables = match answer {
                        Ok(answer) => list(&answer["variables"]),
                        Err(_) => Vec::new(),
                    };
                    session.variables.insert(*reference, variables);
                }
                this.load_variables(id, cx);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn reload_variables(&mut self, id: u64, cx: &mut Context<Self>) {
        self.load_scopes(id, cx);
        self.evaluate_watches(cx);
    }

    /// Opens or closes a scope or a variable of the view.
    pub(super) fn toggle_variable(&mut self, path: &str, scope: bool, cx: &mut Context<Self>) {
        let set = if scope {
            let expensive = self.debug.session().is_some_and(|session| {
                session
                    .scopes
                    .iter()
                    .any(|known| known.name == path && known.expensive)
            });
            if expensive {
                &mut self.debug.open
            } else {
                &mut self.debug.closed
            }
        } else {
            &mut self.debug.open
        };
        if !set.remove(path) {
            set.insert(path.to_owned());
        }
        if let Some(id) = self.debug.active {
            self.load_variables(id, cx);
        }
        cx.notify();
    }

    /// Changes a variable's value.
    pub(super) fn set_variable(
        &mut self,
        container: i64,
        name: String,
        value: String,
        cx: &mut Context<Self>,
    ) {
        let Some(session) = self.debug.session() else {
            return;
        };
        let (id, client) = (session.id, session.client.clone());
        let answer = client.request(
            "setVariable",
            json!({ "variablesReference": container, "name": name, "value": value }),
        );
        cx.spawn(async move |this, cx| {
            let answer = answer.await;
            this.update(cx, |this, cx| {
                if let Err(error) = answer {
                    this.report(format!("{name}: {error}"));
                }
                this.reload_variables(id, cx);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    // -----------------------------------------------------------------
    // Going on

    /// Continue, step over, into and out.
    pub(super) fn debug_step(&mut self, command: &'static str, cx: &mut Context<Self>) {
        let Some(session) = self.debug.session() else {
            return;
        };
        if session.state != SessionState::Stopped {
            return;
        }
        let (id, client, epoch) = (session.id, session.client.clone(), session.epoch);
        let thread = session.thread.unwrap_or(1);
        let answer = client.request(command, json!({ "threadId": thread }));
        cx.spawn(async move |this, cx| {
            let answer = answer.await;
            this.update(cx, |this, cx| match answer {
                // The next stop may have come first.
                Ok(_) => {
                    let same = this
                        .debug
                        .session_mut(id)
                        .is_some_and(|session| session.epoch == epoch);
                    if same {
                        this.debug_continued(id, cx);
                    }
                }
                Err(error) => {
                    this.report(error.to_string());
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
    }

    fn pause_debugging(&mut self, cx: &mut Context<Self>) {
        let Some(session) = self.debug.session() else {
            return;
        };
        if session.state != SessionState::Running {
            return;
        }
        let client = session.client.clone();
        let known = session
            .thread
            .or(session.threads.first().map(|thread| thread.id));
        cx.spawn(async move |_, _| {
            let thread = match known {
                Some(thread) => thread,
                None => {
                    let answer = client.request("threads", Value::Null).await;
                    let threads: Vec<Thread> = answer
                        .map(|answer| list(&answer["threads"]))
                        .unwrap_or_default();
                    threads.first().map_or(1, |thread| thread.id)
                }
            };
            let _ = client.request("pause", json!({ "threadId": thread })).await;
        })
        .detach();
    }

    /// Goes on until the cursor's line, once.
    fn run_to_cursor(&mut self, cx: &mut Context<Self>) {
        let Some((path, line)) = self.debug_cursor(cx) else {
            return;
        };
        if self
            .debug
            .session()
            .is_none_or(|session| session.state != SessionState::Stopped)
        {
            return;
        }
        self.debug.breakpoints.once = Some((path.clone(), line));
        self.send_breakpoints(&path, cx);
        self.debug_step("continue", cx);
    }

    /// Ends the session in front, with those it opened.
    pub(super) fn stop_debugging(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(active) = self.debug.active else {
            return;
        };
        let root = self.debug.root_of(active);
        let family: Vec<u64> = self
            .debug
            .sessions
            .iter()
            .map(|session| session.id)
            .filter(|id| self.debug.root_of(*id) == root)
            .collect();
        for id in family {
            let Some(session) = self.debug.session_mut(id) else {
                continue;
            };
            if session.ending {
                // Asked twice: at once.
                session.client.kill();
                continue;
            }
            session.ending = true;
            let end = session.config.request != "attach";
            let client = session.client.clone();
            let answer = client.request(
                "disconnect",
                json!({ "restart": false, "terminateDebuggee": end }),
            );
            cx.spawn_in(window, async move |this, cx| {
                let patience = cx.background_executor().timer(END_PATIENCE);
                select(Box::pin(answer), Box::pin(patience)).await;
                client.kill();
                this.update_in(cx, |this, window, cx| this.end_session(id, window, cx))
                    .ok();
            })
            .detach();
        }
        cx.notify();
    }

    fn restart_debugging(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(active) = self.debug.active else {
            return self.start_chosen(false, window, cx);
        };
        let root = self.debug.root_of(active);
        let Some(session) = self.debug.session_mut(root) else {
            return;
        };
        let (config, no_debug) = (session.config.clone(), session.no_debug);
        self.stop_debugging(window, cx);
        // Started again once it is over.
        cx.spawn_in(window, async move |this, cx| {
            for _ in 0..60 {
                cx.background_executor()
                    .timer(Duration::from_millis(50))
                    .await;
                let Ok(over) = this.update(cx, |this, _| {
                    !this.debug.sessions.iter().any(|session| session.id == root)
                }) else {
                    return;
                };
                if over {
                    break;
                }
            }
            this.update_in(cx, |this, window, cx| {
                // Its variables are read again, in the project as it is.
                let (_, all) = this.configurations(cx);
                let config = all
                    .into_iter()
                    .find(|known| known.name == config.name)
                    .unwrap_or(config);
                this.start_debugging(config, no_debug, window, cx)
            })
            .ok();
        })
        .detach();
    }

    /// A session is over: it leaves, and what it showed with it.
    fn end_session(&mut self, id: u64, window: &mut Window, cx: &mut Context<Self>) {
        let Some(index) = self
            .debug
            .sessions
            .iter()
            .position(|session| session.id == id)
        else {
            return;
        };
        let session = self.debug.sessions.remove(index);
        // The sessions it opened go with it.
        let children: Vec<u64> = self
            .debug
            .sessions
            .iter()
            .filter(|child| child.parent == Some(id))
            .map(|child| child.id)
            .collect();
        for child in children {
            self.end_session(child, window, cx);
        }
        if !session.ending {
            // It ended by itself: the adapter hears it may go.
            let end = session.config.request != "attach";
            drop(
                session
                    .client
                    .request("disconnect", json!({ "terminateDebuggee": end })),
            );
        }
        let client = session.client.clone();
        cx.background_executor()
            .spawn({
                let timer = cx.background_executor().timer(END_PATIENCE);
                async move {
                    timer.await;
                    client.kill();
                }
            })
            .detach();
        if self.debug.active == Some(id) {
            // The one stopped, or any other.
            let next = self
                .debug
                .sessions
                .iter()
                .find(|other| other.state == SessionState::Stopped)
                .or(self.debug.sessions.last());
            self.debug.active = next.map(|next| next.id);
        }
        if self.debug.sessions.is_empty() {
            self.debug.inline = None;
            for watch in &mut self.debug.watches {
                watch.value = None;
            }
            if session.parent.is_none()
                && let Some(label) = session
                    .config
                    .body
                    .get("postDebugTask")
                    .and_then(Value::as_str)
            {
                let label = label.to_owned();
                self.run_task(&label, window, cx);
            }
        }
        self.show_debug_line(cx);
        cx.notify();
    }

    // -----------------------------------------------------------------
    // Expressions

    /// What is selected, or the word at the cursor.
    fn selected_or_word(&self, cx: &App) -> Option<String> {
        let pane = self.panes.get(self.active)?;
        let editor = pane.editor.read(cx);
        let text = editor.value().to_string();
        let selected = text.get(editor.selected_range()).unwrap_or_default().trim();
        if !selected.is_empty() {
            return Some(selected.to_owned());
        }
        let at = editor.selected_range().start;
        let word = |char: char| char.is_alphanumeric() || char == '_';
        let start = text[..at]
            .char_indices()
            .rev()
            .take_while(|(_, char)| word(*char))
            .last()
            .map_or(at, |(start, _)| start);
        let end = text[at..]
            .char_indices()
            .find(|(_, char)| !word(*char))
            .map_or(text.len(), |(end, _)| at + end);
        (start < end).then(|| text[start..end].to_owned())
    }

    /// Asks the session in front what an expression is worth.
    pub(super) fn evaluate(
        &self,
        expression: &str,
        context: &str,
    ) -> Option<impl Future<Output = Result<Evaluated, String>> + use<>> {
        let session = self.debug.session()?;
        let mut arguments = json!({ "expression": expression, "context": context });
        if let Some(frame) = session.top_frame() {
            arguments["frameId"] = json!(frame.id);
        }
        let answer = session.client.request("evaluate", arguments);
        Some(async move {
            match answer.await {
                Ok(answer) => Ok(serde_json::from_value(answer).unwrap_or_default()),
                Err(error) => Err(error.to_string()),
            }
        })
    }

    pub(super) fn add_watch(&mut self, expression: String, cx: &mut Context<Self>) {
        if !self
            .debug
            .watches
            .iter()
            .any(|watch| watch.expression == expression)
        {
            self.debug.watches.push(Watch {
                expression,
                value: None,
            });
        }
        self.session_changed(cx);
        self.evaluate_watches(cx);
        cx.notify();
    }

    pub(super) fn evaluate_watches(&mut self, cx: &mut Context<Self>) {
        let stopped = self
            .debug
            .session()
            .is_some_and(|session| session.state == SessionState::Stopped);
        if !stopped {
            return;
        }
        for index in 0..self.debug.watches.len() {
            let expression = self.debug.watches[index].expression.clone();
            let Some(answer) = self.evaluate(&expression, "watch") else {
                return;
            };
            cx.spawn(async move |this, cx| {
                let answer = answer.await;
                this.update(cx, |this, cx| {
                    let watch = this
                        .debug
                        .watches
                        .iter_mut()
                        .find(|watch| watch.expression == expression);
                    if let Some(watch) = watch {
                        watch.value = Some(answer);
                        cx.notify();
                    }
                })
                .ok();
            })
            .detach();
        }
    }

    /// The console's field, made the first time it is wanted.
    pub(super) fn console_input(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<InputState> {
        if let Some(input) = &self.debug.console.input {
            return input.clone();
        }
        let placeholder = self.strings().debug_console_placeholder;
        let input = cx.new(|cx| InputState::new(window, cx).placeholder(placeholder));
        let subscription = cx.subscribe_in(
            &input,
            window,
            |this, input, event: &InputEvent, window, cx| {
                if !matches!(event, InputEvent::PressEnter { .. }) {
                    return;
                }
                let text = input.read(cx).value().trim().to_owned();
                if text.is_empty() {
                    return;
                }
                input.update(cx, |input, cx| input.set_value("", window, cx));
                this.evaluate_in_console(text, cx);
            },
        );
        self.debug.console._input = Some(subscription);
        self.debug.console.input = Some(input.clone());
        input
    }

    pub(super) fn evaluate_in_console(&mut self, text: String, cx: &mut Context<Self>) {
        let console = &mut self.debug.console;
        console.write(LineKind::Typed, &format!("{text}\n"));
        console.history.retain(|known| *known != text);
        console.history.push(text.clone());
        let Some(answer) = self.evaluate(&text, "repl") else {
            let strings = self.strings();
            self.debug
                .console
                .write(LineKind::Note, &format!("{}\n", strings.debug_not_running));
            cx.notify();
            return;
        };
        cx.notify();
        cx.spawn(async move |this, cx| {
            let answer = answer.await;
            this.update(cx, |this, cx| {
                match answer {
                    Ok(value) if value.result.is_empty() => {}
                    Ok(value) => {
                        let text = format!("{}\n", value.result.trim_end());
                        this.debug.console.write(LineKind::Answer, &text);
                    }
                    Err(error) => this
                        .debug
                        .console
                        .write(LineKind::Error, &format!("{error}\n")),
                }
                // It may have changed something.
                if let Some(id) = this.debug.active {
                    this.reload_variables(id, cx);
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    // -----------------------------------------------------------------
    // In the editors

    /// The line a file is stopped at, and whether it is the running one.
    pub(super) fn stopped_line(&self, path: &Path) -> Option<(u32, bool)> {
        let session = self.debug.session()?;
        if session.state != SessionState::Stopped {
            return None;
        }
        let frame = session.top_frame()?;
        (frame_file(frame)?.as_path() == path)
            .then(|| (frame.line.saturating_sub(1), session.frame == 0))
    }

    /// Lights the line the program is stopped at, in every editor.
    pub(super) fn show_debug_line(&mut self, cx: &mut Context<Self>) {
        let theme = cx.theme();
        let (top, other) = (theme.warning.opacity(0.22), theme.success.opacity(0.16));
        let editors: Vec<(Entity<EditorState>, Option<PathBuf>)> = self
            .panes
            .iter()
            .map(|pane| {
                let path = self
                    .documents
                    .get(&pane.uri)
                    .and_then(|document| document.disk_path())
                    .map(Path::to_path_buf);
                (pane.editor.clone(), path)
            })
            .collect();
        for (editor, path) in editors {
            let line = path.and_then(|path| self.stopped_line(&path));
            let known = self.debug.decor.contains_key(&editor.entity_id());
            if line.is_none() && !known {
                continue;
            }
            let decorations = match line {
                Some((line, is_top)) => {
                    let state = editor.read(cx);
                    let text = state.text();
                    let row = (line as usize).min(text.lines_len().saturating_sub(1));
                    let start = text.line_start_offset(row);
                    let end = (text.line_end_offset(row) + 1).min(text.len());
                    vec![
                        RangeDecoration::new(start..end)
                            .with_style(RangeDecorationStyle::Fill)
                            .with_color(if is_top { top } else { other }),
                    ]
                }
                None => Vec::new(),
            };
            match self.debug.decor.get(&editor.entity_id()) {
                Some(decor) => decor.set(decorations, cx),
                None => {
                    let decor = editor.update(cx, |editor, cx| {
                        editor.create_range_decorations_collection(decorations, cx)
                    });
                    self.debug.decor.insert(editor.entity_id(), decor);
                }
            }
        }
    }

    /// The values of the variables beside the lines that name them, up to
    /// the one stopped at.
    fn show_inline_values(&mut self, cx: &mut Context<Self>) {
        let wanted = self.settings.string("debug.inlineValues") != Some("off")
            && self.settings.get("debug.inlineValues") != Some(&Value::Bool(false));
        self.debug.inline = wanted.then(|| self.inline_values()).flatten();
        cx.notify();
    }

    fn inline_values(&self) -> Option<(String, HashMap<usize, Vec<String>>)> {
        let session = self.debug.session()?;
        if session.state != SessionState::Stopped {
            return None;
        }
        let frame = session.top_frame()?;
        let uri = Uri::file(&frame_file(frame)?).to_string();
        let text = &self.documents.get(&uri)?.synced_text;
        // Only the frame's own: globals would be on every line.
        let values: Vec<(&str, &str)> = session
            .scopes
            .iter()
            .filter(|scope| {
                let name = scope.name.to_lowercase();
                let hint = scope.presentation_hint.as_deref();
                !scope.expensive
                    && (matches!(hint, Some("locals" | "arguments"))
                        || name.contains("local")
                        || name.contains("argument")
                        || name.contains("block"))
            })
            .filter_map(|scope| session.variables.get(&scope.variables_reference))
            .flatten()
            .filter(|variable| !variable.value.is_empty())
            .map(|variable| (variable.name.as_str(), variable.value.as_str()))
            .collect();
        Some((uri, beside_lines(text, frame.line as usize, &values)))
    }

    /// The debugger's part of the status bar.
    pub(super) fn debug_status(&self) -> Option<String> {
        let strings = self.strings();
        if let Some(starting) = &self.debug.starting {
            return Some(notices::fill(strings.debug_starting, &[("name", starting)]));
        }
        let session = self.debug.session()?;
        let text = match session.state {
            SessionState::Starting => strings.debug_starting,
            SessionState::Running => strings.debug_running,
            SessionState::Stopped => strings.debug_stopped,
        };
        Some(notices::fill(text, &[("name", &session.name)]))
    }
}

/// The values to write beside each line, from 0: those it names, up to
/// the line stopped at (from 1) and no further up than its block.
fn beside_lines(
    text: &str,
    stopped: usize,
    values: &[(&str, &str)],
) -> HashMap<usize, Vec<String>> {
    const MOST_LINES: usize = 200;
    const MOST_CHARS: usize = 60;
    let lines: Vec<&str> = text.lines().collect();
    let mut beside: HashMap<usize, Vec<String>> = HashMap::new();
    let last = stopped.min(lines.len());
    let is_word = |char: char| char.is_alphanumeric() || char == '_';
    let at_margin = |line: &str| {
        line.chars()
            .next()
            .is_some_and(|first| !first.is_whitespace())
    };
    // In a script, every line is at the margin.
    let inside = last > 0 && !at_margin(lines[last - 1]);
    for row in (last.saturating_sub(MOST_LINES)..last).rev() {
        let line = lines[row];
        let mut words: Vec<&str> = Vec::new();
        for word in line.split(|char| !is_word(char)) {
            if !word.is_empty() && !words.contains(&word) {
                words.push(word);
            }
        }
        for word in words {
            if let Some((name, value)) = values.iter().find(|(name, _)| *name == word) {
                let value = value.lines().next().unwrap_or_default();
                let mut short: String = value.chars().take(MOST_CHARS).collect();
                if value.chars().count() > MOST_CHARS {
                    short.push('…');
                }
                beside
                    .entry(row)
                    .or_default()
                    .push(format!("{name} = {short}"));
            }
        }
        // Above a line that starts at the margin is another function.
        if inside && at_margin(line) && !line.starts_with(['}', ')', ']', '#', '@']) {
            break;
        }
    }
    beside
}

/// A frame's file, when it is on the disk.
pub(super) fn frame_file(frame: &StackFrame) -> Option<PathBuf> {
    let path = PathBuf::from(frame.source.as_ref()?.path.as_ref()?);
    path.is_file().then_some(path)
}

/// Starts what a configuration says; the error is for the user, `None`
/// when they changed their mind.
async fn start(
    this: &WeakEntity<Workbench>,
    mut config: DebugConfig,
    no_debug: bool,
    cx: &mut AsyncWindowContext,
) -> Cancelled {
    let gone = |_| None;
    // What has to run before.
    if let Some(label) = config.body.get("preLaunchTask").and_then(Value::as_str) {
        let label = label.to_owned();
        let task = this
            .update_in(cx, |this, window, cx| this.task_outcome(&label, window, cx))
            .map_err(gone)?;
        if !task.await {
            let text = this
                .update(cx, |this, _| {
                    notices::fill(this.strings().debug_task_failed, &[("task", &label)])
                })
                .map_err(gone)?;
            return Err(Some(text));
        }
    }
    // What it asks.
    let body = Value::Object(config.body.clone());
    let mut inputs = HashMap::new();
    for id in debug_config::inputs_in(&body) {
        let answer = this
            .update_in(cx, |this, window, cx| {
                let file = this.launch_file(cx);
                let known = file.inputs.into_iter().find(|input| input.id == id);
                let input = known.unwrap_or(TaskInput {
                    description: id.clone(),
                    id: id.clone(),
                    default: None,
                    options: Vec::new(),
                });
                this.ask_task_input(&input, window, cx)
            })
            .map_err(gone)?;
        let Some(answer) = answer.await.ok().flatten() else {
            return Err(None);
        };
        inputs.insert(id, answer);
    }
    let mut body = body;
    if body.to_string().contains("${command:pick") {
        let Some(pid) = pick_process(this, cx).await else {
            return Err(None);
        };
        body = replaced(
            &body,
            &["${command:pickProcess}", "${command:pickMyProcess}"],
            &pid,
        );
    }
    let (variables, root, adapters_dir) = this
        .update(cx, |this, cx| {
            (
                this.task_variables(&TaskFile::default(), inputs, cx),
                this.terminal_folder(),
                this.adapters_dir(),
            )
        })
        .map_err(gone)?;
    match debug_config::expanded(&body, &variables) {
        Ok(Value::Object(filled)) => config.body = filled,
        Ok(_) => {}
        Err(variable) => {
            let text = this
                .update(cx, |this, _| {
                    notices::fill(
                        this.strings().tasks_unknown_variable,
                        &[("task", &config.name), ("variable", &variable)],
                    )
                })
                .map_err(gone)?;
            return Err(Some(text));
        }
    }

    #[cfg(test)]
    if this.update(cx, |this, _| this.debug.fake).map_err(gone)? {
        let (handler, events) = channel();
        let (reader, writer) = dap::fake::spawn().map_err(|error| Some(error.to_string()))?;
        let client = Client::start(reader, writer, handler);
        let arguments = debug_config::arguments(&config, &root, None, &[], no_debug);
        let request = config.request.clone();
        return open(
            this,
            Opening {
                config,
                no_debug,
                request,
                arguments,
                client,
                events,
                server: None,
                parent: None,
            },
            cx,
        )
        .await;
    }

    // Its debugger, installed if the user agrees.
    let env = cx
        .background_executor()
        .spawn(async { lsp::servers::login_environment() })
        .await;
    let adapter = config.adapter;
    let python = {
        let root = root.clone();
        cx.background_executor()
            .spawn(async move { adapters::python_of(&root, env) })
            .await
    };
    let command = loop {
        let found = {
            let (python, dir) = (python.clone(), adapters_dir.clone());
            cx.background_executor()
                .spawn(async move { find(adapter, python.as_deref(), &dir, env) })
                .await
        };
        match found {
            Ok(command) => break command,
            Err(missing) => {
                if !install(this, adapter, &missing, python.clone(), &adapters_dir, cx).await? {
                    return Err(None);
                }
            }
        }
    };
    if adapter == Adapter::Lldb && config.request == "launch" {
        let program = cargo_program(this, &config, &root, env, cx).await?;
        if let Some(program) = program {
            config.body.insert("program".to_owned(), json!(program));
        }
    }
    let formatters = if adapter == Adapter::Lldb {
        let root = root.clone();
        cx.background_executor()
            .spawn(async move { adapters::rust_formatters(&root, env) })
            .await
    } else {
        Vec::new()
    };
    let arguments =
        debug_config::arguments(&config, &root, python.as_deref(), &formatters, no_debug);
    let (handler, events) = channel();
    let started = {
        let mut command = command;
        command.cwd = Some(root.clone());
        command.env = env
            .iter()
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect();
        cx.background_executor()
            .spawn(async move {
                match adapter {
                    Adapter::Node => Client::spawn_server(&command, SERVER_PATIENCE, handler)
                        .map(|(client, address)| (client, Some(address))),
                    _ => Client::spawn(&command, handler).map(|client| (client, None)),
                }
            })
            .await
    };
    let (client, server) = started.map_err(|error| Some(format!("{error:#}")))?;
    let request = config.request.clone();
    open(
        this,
        Opening {
            config,
            no_debug,
            request,
            arguments,
            client,
            events,
            server,
            parent: None,
        },
        cx,
    )
    .await
}

/// Adds a session and brings it to where the program runs.
async fn open(
    this: &WeakEntity<Workbench>,
    opening: Opening,
    cx: &mut AsyncWindowContext,
) -> Cancelled {
    let adapter = match opening.config.adapter {
        Adapter::Lldb => "lldb-dap",
        Adapter::Python => "debugpy",
        Adapter::Node => "pwa-node",
    };
    let (request, arguments) = (opening.request.clone(), opening.arguments.clone());
    let language = if opening.config.name.is_empty() {
        "en"
    } else {
        "en-us"
    };
    let (id, client, heard) = this
        .update_in(cx, |this, window, cx| this.add_session(opening, window, cx))
        .map_err(|_| None)?;
    let shaken = handshake(
        this, id, &client, adapter, language, &request, arguments, heard, cx,
    )
    .await;
    if let Err(error) = shaken {
        // Stopped by the user while it started: nothing to say.
        let ending = this
            .update_in(cx, |this, window, cx| {
                let ending = this
                    .debug
                    .session_mut(id)
                    .is_none_or(|session| session.ending);
                client.kill();
                this.end_session(id, window, cx);
                ending
            })
            .unwrap_or(true);
        return Err((!ending).then(|| format!("{error:#}")));
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn handshake(
    this: &WeakEntity<Workbench>,
    id: u64,
    client: &Arc<Client>,
    adapter: &str,
    locale: &str,
    request: &str,
    arguments: Value,
    heard: oneshot::Receiver<()>,
    cx: &mut AsyncWindowContext,
) -> anyhow::Result<()> {
    let answer = client
        .request(
            "initialize",
            json!({
                "clientID": "keel",
                "clientName": "Keel",
                "adapterID": adapter,
                "locale": locale,
                "linesStartAt1": true,
                "columnsStartAt1": true,
                "pathFormat": "path",
                "supportsVariableType": true,
                "supportsVariablePaging": false,
                "supportsRunInTerminalRequest": true,
                "supportsMemoryReferences": false,
                "supportsProgressReporting": false,
                "supportsInvalidatedEvent": true,
                "supportsStartDebuggingRequest": true,
            }),
        )
        .await?;
    let capabilities: Capabilities = serde_json::from_value(answer).unwrap_or_default();
    let configures = capabilities.supports_configuration_done_request;
    this.update(cx, |this, _| {
        if let Some(session) = this.debug.session_mut(id) {
            session.capabilities = capabilities;
        }
    })?;
    // Some adapters answer the launch first, some only once configured.
    let launched = client.request(request, arguments);
    let waiting = match select(launched, heard).await {
        Either::Left((answer, heard)) => {
            answer?;
            heard
                .await
                .map_err(|_| anyhow::anyhow!("the debug adapter closed"))?;
            None
        }
        Either::Right((heard, launched)) => {
            heard.map_err(|_| anyhow::anyhow!("the debug adapter closed"))?;
            Some(launched)
        }
    };
    this.update(cx, |this, cx| this.configure_session(id, cx))?;
    if configures {
        client.request("configurationDone", json!({})).await?;
    }
    if let Some(launched) = waiting {
        launched.await?;
    }
    this.update(cx, |this, cx| {
        if let Some(session) = this.debug.session_mut(id)
            && session.state == SessionState::Starting
        {
            session.state = SessionState::Running;
        }
        cx.notify();
    })?;
    Ok(())
}

fn find(
    adapter: Adapter,
    python: Option<&Path>,
    dir: &Path,
    env: &HashMap<String, String>,
) -> Result<AdapterCommand, Missing> {
    match adapter {
        Adapter::Lldb => adapters::find_lldb(env),
        Adapter::Python => match python {
            Some(python) => {
                adapters::find_debugpy(python, dir, std::env::home_dir().as_deref(), env)
            }
            None => Err(Missing::ByHand {
                name: "Python",
                how: "https://www.python.org",
            }),
        },
        Adapter::Node => adapters::find_js_debug(dir, env),
    }
}

/// Asks to install a debugger, and installs it.
async fn install(
    this: &WeakEntity<Workbench>,
    adapter: Adapter,
    missing: &Missing,
    python: Option<PathBuf>,
    dir: &Path,
    cx: &mut AsyncWindowContext,
) -> Result<bool, Option<String>> {
    let gone = |_| None;
    let (name, from) = match missing {
        Missing::Installable { name, from } => (*name, *from),
        Missing::ByHand { name, how } => {
            let text = this
                .update(cx, |this, _| {
                    notices::fill(
                        this.strings().debug_missing,
                        &[("name", name), ("how", how)],
                    )
                })
                .map_err(gone)?;
            return Err(Some(text));
        }
    };
    let answer = this
        .update_in(cx, |this, window, cx| {
            let strings = this.strings();
            let question = notices::fill(strings.debug_install, &[("name", name), ("from", from)]);
            window.prompt(
                PromptLevel::Info,
                &question,
                Some(strings.debug_install_detail),
                &[strings.debug_install_yes, strings.cancel],
                cx,
            )
        })
        .map_err(gone)?;
    if answer.await != Ok(0) {
        return Ok(false);
    }
    this.update(cx, |this, cx| {
        this.status_message = Some(notices::fill(
            this.strings().debug_installing,
            &[("name", name)],
        ));
        cx.notify();
    })
    .map_err(gone)?;
    let dir = dir.to_path_buf();
    let installed = cx
        .background_executor()
        .spawn(async move {
            adapters::install(
                adapter,
                python.as_deref(),
                &dir,
                lsp::servers::login_environment(),
            )
        })
        .await;
    this.update(cx, |this, cx| {
        this.status_message = None;
        cx.notify();
    })
    .map_err(gone)?;
    match installed {
        Ok(text) => {
            this.update(cx, |this, _| this.log(text)).map_err(gone)?;
            Ok(true)
        }
        Err(error) => Err(Some(format!("{error:#}"))),
    }
}

/// Builds what a configuration's `cargo` says; its program, when it has
/// one and no `program` of its own.
async fn cargo_program(
    this: &WeakEntity<Workbench>,
    config: &DebugConfig,
    root: &Path,
    env: &'static HashMap<String, String>,
    cx: &mut AsyncWindowContext,
) -> Result<Option<PathBuf>, Option<String>> {
    let gone = |_| None;
    let Some(cargo) = config.body.get("cargo") else {
        return Ok(None);
    };
    let mut args = debug_config::words(&cargo["args"]);
    if args.is_empty() {
        args.push("build".to_owned());
    }
    // Before a `--`, which is for the program.
    let at = args
        .iter()
        .position(|arg| arg == "--")
        .unwrap_or(args.len());
    args.insert(at, "--message-format=json-render-diagnostics".to_owned());
    this.update(cx, |this, cx| {
        this.debug
            .console
            .write(LineKind::Note, &format!("cargo {}\n", args.join(" ")));
        this.layout.show_panel_view(PanelView::DebugConsole);
        cx.notify();
    })
    .map_err(gone)?;
    let cwd = cargo["cwd"]
        .as_str()
        .map_or_else(|| root.to_path_buf(), |cwd| root.join(cwd));
    let output = cx
        .background_executor()
        .spawn(async move {
            Command::new(lsp::servers::which_in("cargo", env).unwrap_or_else(|| "cargo".into()))
                .args(&args)
                .current_dir(cwd)
                .envs(env)
                .stdin(Stdio::null())
                .output()
        })
        .await
        .map_err(|error| Some(error.to_string()))?;
    let said = String::from_utf8_lossy(&output.stderr).into_owned();
    this.update(cx, |this, cx| {
        this.debug.console.write(LineKind::Output, &said);
        this.build_problems(&said, cx);
        cx.notify();
    })
    .map_err(gone)?;
    if !output.status.success() {
        let text = this
            .update(cx, |this, _| this.strings().debug_build_failed.to_owned())
            .map_err(gone)?;
        return Err(Some(text));
    }
    if config.body.contains_key("program") {
        return Ok(None);
    }
    let programs = debug_config::cargo_programs(&String::from_utf8_lossy(&output.stdout));
    if let Some(source) = cargo["source"].as_str()
        && let Some(program) = debug_config::program_of_source(&programs, Path::new(source))
    {
        return Ok(Some(program.clone()));
    }
    let wanted = cargo["filter"]["name"].as_str();
    let programs: Vec<(String, PathBuf)> = programs
        .into_iter()
        .filter(|(name, ..)| wanted.is_none_or(|wanted| wanted == name))
        .map(|(name, program, ..)| (name, program))
        .collect();
    match programs.as_slice() {
        [] => {
            let text = this
                .update(cx, |this, _| this.strings().debug_no_program.to_owned())
                .map_err(gone)?;
            Err(Some(text))
        }
        [(_, program)] => Ok(Some(program.clone())),
        _ => {
            let items = programs
                .iter()
                .map(|(name, _)| PickItem::new(name.clone()))
                .collect();
            let chosen = this
                .update_in(cx, |this, window, cx| {
                    let title = this.strings().debug_choose_program.to_owned();
                    quick_pick::show(None, Some(title), items, window, cx)
                })
                .map_err(gone)?;
            match chosen.await.ok().flatten() {
                Some(index) => Ok(Some(programs[index].1.clone())),
                None => Err(None),
            }
        }
    }
}

/// Asks for a running process; its number.
async fn pick_process(this: &WeakEntity<Workbench>, cx: &mut AsyncWindowContext) -> Option<String> {
    let listed = cx
        .background_executor()
        .spawn(async {
            Command::new("/bin/ps")
                .args(["-xo", "pid=,comm="])
                .stdin(Stdio::null())
                .output()
        })
        .await
        .ok()?;
    let mut processes: Vec<(String, String)> = String::from_utf8_lossy(&listed.stdout)
        .lines()
        .filter_map(|line| {
            let (pid, command) = line.trim().split_once(' ')?;
            Some((pid.to_owned(), command.trim().to_owned()))
        })
        .collect();
    // The newest first.
    processes.reverse();
    let items = processes
        .iter()
        .map(|(pid, command)| {
            let name = command.rsplit('/').next().unwrap_or(command);
            let mut item = PickItem::new(format!("{name}  {pid}"));
            item.detail = Some(command.clone());
            item
        })
        .collect();
    let chosen = this
        .update_in(cx, |this, window, cx| {
            let title = this.strings().debug_choose_process.to_owned();
            quick_pick::show(None, Some(title), items, window, cx)
        })
        .ok()?;
    let index = chosen.await.ok().flatten()?;
    Some(processes[index].0.clone())
}

/// A value with some texts in it replaced.
fn replaced(value: &Value, from: &[&str], to: &str) -> Value {
    match value {
        Value::String(text) => Value::String(
            from.iter()
                .fold(text.clone(), |text, from| text.replace(from, to)),
        ),
        Value::Array(values) => Value::Array(
            values
                .iter()
                .map(|value| replaced(value, from, to))
                .collect(),
        ),
        Value::Object(map) => Value::Object(
            map.iter()
                .map(|(key, value)| (key.clone(), replaced(value, from, to)))
                .collect(),
        ),
        other => other.clone(),
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::time::Instant;

    use gpui::{TestAppContext, VisualTestContext};

    use super::*;

    const PROGRAM: &str = "a = 1\nnomi = uno, due\ncall saluta\nprint fine di tutto\nend:\nsaluta:\nb = ciao\nreturn\n";

    /// A project with a program for the fake adapter and a launch.json.
    fn project<'a>(
        cx: &'a mut TestAppContext,
        program: &str,
    ) -> (
        tempfile::TempDir,
        PathBuf,
        Entity<Workbench>,
        &'a mut VisualTestContext,
    ) {
        cx.executor().allow_parking();
        cx.update(|cx| {
            gpui_component::init(cx);
            crate::workbench_window::init(cx);
        });
        let folder = tempfile::tempdir().unwrap();
        let base = fs::canonicalize(folder.path()).unwrap();
        let home = base.join("home");
        fs::create_dir(&home).unwrap();
        fs::write(home.join(".zshrc"), "PS1='$ '\n").unwrap();
        terminal_view::TEST_HOME.with(|test| *test.borrow_mut() = Some(home));
        let root = base.join("app");
        fs::create_dir_all(root.join(".vscode")).unwrap();
        let path = root.join("prova.txt");
        fs::write(&path, program).unwrap();
        fs::write(
            root.join(".vscode/launch.json"),
            r#"{ "configurations": [
                { "name": "Prova", "type": "debugpy", "request": "launch", "program": "${workspaceFolder}/prova.txt", "console": "internalConsole" },
            ] }"#,
        )
        .unwrap();
        let settings = Settings::load(base.join("settings.json"), None);
        let opening = super::super::Opening::new(Some(root), None);
        let (view, cx) = cx.add_window_view(|window, cx| {
            let mut workbench = Workbench::new(opening, settings, None, window, cx);
            workbench.debug.fake = true;
            workbench
        });
        view.update_in(cx, |view, window, cx| view.open_file(&path, window, cx));
        (folder, path, view, cx)
    }

    fn wait_until(
        cx: &mut VisualTestContext,
        what: &str,
        check: impl Fn(&mut VisualTestContext) -> bool,
    ) {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            cx.executor().advance_clock(Duration::from_millis(50));
            cx.run_until_parked();
            if check(cx) {
                return;
            }
            assert!(Instant::now() < deadline, "gave up waiting for {what}");
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    /// The line stopped at, from 1, once its variables are there.
    fn stopped_at(view: &Entity<Workbench>, cx: &mut VisualTestContext) -> Option<u32> {
        view.read_with(cx, |view, _| {
            let session = view.debug.session()?;
            let read = session
                .scopes
                .iter()
                .all(|scope| session.variables.contains_key(&scope.variables_reference))
                && !session.scopes.is_empty();
            (session.state == SessionState::Stopped && read)
                .then(|| session.top_frame().map(|frame| frame.line))
                .flatten()
        })
    }

    fn values(view: &Entity<Workbench>, cx: &mut VisualTestContext) -> Vec<(String, String)> {
        view.read_with(cx, |view, _| {
            let session = view.debug.session().unwrap();
            session.variables[&session.scopes[0].variables_reference]
                .iter()
                .map(|variable| (variable.name.clone(), variable.value.clone()))
                .collect()
        })
    }

    fn console(view: &Entity<Workbench>, cx: &mut VisualTestContext) -> String {
        view.read_with(cx, |view, _| {
            let lines = view.debug.console.lines.iter();
            lines
                .map(|line| line.text.clone())
                .collect::<Vec<_>>()
                .join("\n")
        })
    }

    #[gpui::test]
    fn a_program_stops_steps_and_ends(cx: &mut TestAppContext) {
        let (_folder, path, view, cx) = project(cx, PROGRAM);
        view.update(cx, |view, cx| view.toggle_breakpoint(&path, 2, cx));
        cx.simulate_keystrokes("f5");
        wait_until(cx, "the breakpoint", |cx| stopped_at(&view, cx) == Some(3));
        assert_eq!(
            values(&view, cx),
            [("a", "1"), ("nomi", "uno, due")]
                .map(|(name, value)| (name.to_owned(), value.to_owned()))
        );
        view.update_in(cx, |view, _, cx| {
            // The line is lit and in front, and the values beside the lines.
            assert_eq!(view.stopped_line(&path), Some((2, true)));
            let pane = &view.panes[view.active];
            assert_eq!(pane.editor.read(cx).cursor_position().line, 2);
            let (uri, beside) = view.debug.inline.clone().unwrap();
            assert_eq!(uri, pane.uri);
            assert_eq!(beside[&0], ["a = 1"]);
            assert_eq!(beside[&1], ["nomi = uno, due"]);
            assert_eq!(view.breakpoint_refused(&path, 2), None);
            // What holds something opens.
            view.toggle_variable("Locals/nomi", false, cx);
        });
        wait_until(cx, "what is in it", |cx| {
            view.read_with(cx, |view, _| {
                let session = view.debug.session().unwrap();
                session
                    .variables
                    .get(&20)
                    .is_some_and(|inside| inside.len() == 2)
            })
        });

        // Into the call: another frame on top, and what is open stays so.
        cx.simulate_keystrokes("f11");
        wait_until(cx, "the call", |cx| stopped_at(&view, cx) == Some(7));
        view.read_with(cx, |view, _| {
            let names: Vec<_> = view
                .debug
                .session()
                .unwrap()
                .frames
                .iter()
                .map(|frame| frame.name.clone())
                .collect();
            assert_eq!(names, ["saluta", "main"]);
        });
        wait_until(cx, "what was open", |cx| {
            view.read_with(cx, |view, _| {
                view.debug.session().unwrap().variables.contains_key(&20)
            })
        });
        // The caller's line, from the stack.
        let id = view.read_with(cx, |view, _| view.debug.active.unwrap());
        view.update_in(cx, |view, window, cx| {
            view.select_frame(id, 1, true, window, cx)
        });
        assert_eq!(
            view.read_with(cx, |view, _| view.stopped_line(&path)),
            Some((2, false))
        );
        cx.simulate_keystrokes("shift-f11");
        wait_until(cx, "the way out", |cx| stopped_at(&view, cx) == Some(4));
        // Over a line, to the end.
        cx.simulate_keystrokes("f5");
        wait_until(cx, "the end", |cx| {
            view.read_with(cx, |view, _| view.debug.sessions.is_empty())
        });
        let said = console(&view, cx);
        assert!(
            said.contains("fine di tutto") && said.contains('0'),
            "{said}"
        );
        view.read_with(cx, |view, _| {
            assert!(view.debug.inline.is_none() && view.debug.active.is_none());
            assert_eq!(view.stopped_line(&path), None);
        });
    }

    #[gpui::test]
    fn expressions_values_and_the_console(cx: &mut TestAppContext) {
        let (_folder, path, view, cx) = project(cx, PROGRAM);
        view.update(cx, |view, cx| {
            view.toggle_breakpoint(&path, 3, cx);
            view.add_watch("a".to_owned(), cx);
            view.add_watch("z".to_owned(), cx);
        });
        cx.simulate_keystrokes("f5");
        wait_until(cx, "the breakpoint", |cx| stopped_at(&view, cx) == Some(4));
        let watched = |cx: &mut VisualTestContext| {
            view.read_with(cx, |view, _| {
                view.debug
                    .watches
                    .iter()
                    .map(|watch| {
                        watch
                            .value
                            .clone()
                            .map(|value| value.map(|value| value.result))
                    })
                    .collect::<Vec<_>>()
            })
        };
        wait_until(cx, "the expressions", |cx| {
            watched(cx).iter().all(Option::is_some)
        });
        assert_eq!(
            watched(cx),
            [
                Some(Ok("1".to_owned())),
                Some(Err("z is not there".to_owned()))
            ]
        );
        // Changed from the view: the expressions follow.
        view.update(cx, |view, cx| {
            let holder = view.debug.session().unwrap().scopes[0].variables_reference;
            view.set_variable(holder, "a".to_owned(), "42".to_owned(), cx);
        });
        wait_until(cx, "the new value", |cx| {
            watched(cx)[0] == Some(Ok("42".to_owned()))
        });
        wait_until(cx, "the variables again", |cx| {
            stopped_at(&view, cx).is_some()
                && values(&view, cx).contains(&("a".to_owned(), "42".to_owned()))
        });
        // Typed in the console.
        view.update_in(cx, |view, window, cx| {
            view.layout.show_panel_view(PanelView::DebugConsole);
            let input = view.console_input(window, cx);
            input.update(cx, |input, cx| input.focus(window, cx));
        });
        cx.simulate_input("b");
        cx.simulate_keystrokes("enter");
        wait_until(cx, "the answer", |cx| console(&view, cx).contains("ciao"));
        // Stopped by hand.
        cx.simulate_keystrokes("shift-f5");
        wait_until(cx, "the stop", |cx| {
            view.read_with(cx, |view, _| view.debug.sessions.is_empty())
        });
        assert!(!console(&view, cx).contains("fine di tutto"));
        assert_eq!(watched(cx), [None, None]);
    }

    #[gpui::test]
    fn conditions_messages_and_run_to_cursor(cx: &mut TestAppContext) {
        let (_folder, path, view, cx) = project(cx, "a = 1\nb = 2\nc = 3\nprint ecco\nd = 4\n");
        view.update(cx, |view, cx| {
            let ask = breakpoints::Ask::Condition;
            view.set_breakpoint_detail(&path, 1, ask, "a == 9".to_owned(), cx);
            let ask = breakpoints::Ask::Log;
            view.set_breakpoint_detail(&path, 2, ask, "a vale {a}".to_owned(), cx);
            view.toggle_breakpoint(&path, 3, cx);
        });
        cx.simulate_keystrokes("f5");
        wait_until(cx, "the third breakpoint", |cx| {
            stopped_at(&view, cx) == Some(4)
        });
        assert!(console(&view, cx).contains("a vale 1"));
        // To the cursor's line, once: no dot stays there.
        view.update_in(cx, |view, window, cx| view.go_to(5, 1, window, cx));
        view.update(cx, |view, cx| view.run_to_cursor(cx));
        wait_until(cx, "the cursor's line", |cx| {
            stopped_at(&view, cx) == Some(5)
        });
        view.read_with(cx, |view, _| {
            assert!(view.debug.breakpoints.once.is_none());
            assert_eq!(view.debug.breakpoints.all.len(), 3);
        });
        cx.simulate_keystrokes("f5");
        wait_until(cx, "the end", |cx| {
            view.read_with(cx, |view, _| view.debug.sessions.is_empty())
        });
    }

    #[gpui::test]
    fn a_crash_is_stopped_at_and_restarting_starts_again(cx: &mut TestAppContext) {
        let (_folder, _path, view, cx) = project(cx, "a = 1\ncrash tutto rotto\nprint dopo\n");
        cx.simulate_keystrokes("f5");
        wait_until(cx, "the exception", |cx| stopped_at(&view, cx) == Some(2));
        assert!(console(&view, cx).contains("tutto rotto"));
        let first = view.read_with(cx, |view, _| view.debug.active);
        cx.simulate_keystrokes("cmd-shift-f5");
        wait_until(cx, "the second run", |cx| {
            stopped_at(&view, cx) == Some(2)
                && view.read_with(cx, |view, _| view.debug.active) != first
        });
        assert_eq!(view.read_with(cx, |view, _| view.debug.sessions.len()), 1);
    }

    #[gpui::test]
    fn the_adapter_runs_a_command_in_a_terminal(cx: &mut TestAppContext) {
        let (_folder, path, view, cx) = project(cx, "a = 1\nb = 2\n");
        view.update_in(cx, |view, window, cx| {
            view.toggle_breakpoint(&path, 1, cx);
            let (_, all) = view.configurations(cx);
            let mut config = all[0].clone();
            config
                .body
                .insert("console".to_owned(), json!("integratedTerminal"));
            view.start_debugging(config, false, window, cx);
        });
        wait_until(cx, "the breakpoint", |cx| stopped_at(&view, cx) == Some(2));
        wait_until(cx, "the command", |cx| {
            view.read_with(cx, |view, cx| {
                view.terminals.iter().any(|tab| {
                    let text = tab.view.read(cx).terminal().snapshot().text();
                    tab.name.as_deref() == Some("fake")
                        && text.lines().any(|line| line.trim() == "dal debugger")
                })
            })
        });
    }

    #[gpui::test]
    fn what_cannot_start_says_why(cx: &mut TestAppContext) {
        let (_folder, path, view, cx) = project(cx, PROGRAM);
        let root = path.parent().unwrap().to_path_buf();
        fs::write(
            root.join(".vscode/launch.json"),
            r#"{ "configurations": [
                { "name": "Prima", "type": "python", "request": "launch", "program": "", "preLaunchTask": "non c'è" },
                { "name": "Chissà", "type": "python", "request": "launch", "program": "${chissà}" },
                { "name": "Vuota", "type": "python", "request": "launch" },
            ] }"#,
        )
        .unwrap();
        for (name, said) in [
            ("Prima", "non c'è"),
            ("Chissà", "${chissà}"),
            ("Vuota", "no program"),
        ] {
            view.update_in(cx, |view, window, cx| {
                view.start_named(name, false, window, cx)
            });
            wait_until(cx, name, |cx| console(&view, cx).contains(said));
            wait_until(cx, "its end", |cx| {
                view.read_with(cx, |view, _| {
                    view.debug.sessions.is_empty() && view.debug.starting.is_none()
                })
            });
        }
    }

    #[test]
    fn the_console_joins_what_comes_in_pieces() {
        let mut console = Console::default();
        console.write(LineKind::Output, "una ");
        console.write(LineKind::Output, "riga\ne un'altra\n");
        console.write(LineKind::Error, "\x1b[31mrosso\x1b[0m\n");
        console.write(LineKind::Output, "\n");
        let texts: Vec<&str> = console
            .lines
            .iter()
            .map(|line| line.text.as_str())
            .collect();
        assert_eq!(texts, ["una riga", "e un'altra", "rosso", ""]);
    }

    #[test]
    fn values_are_written_beside_their_lines() {
        let text = "fn prima() {\n    let a = 0;\n}\n\nfn dopo(b: i32) {\n    let a = b;\n    let c = a + b;\n    c\n}\n";
        let values = [("a", "1"), ("b", "2"), ("c", "x\ny")];
        let beside = beside_lines(text, 7, &values);
        assert_eq!(beside[&4], ["b = 2"]);
        assert_eq!(beside[&5], ["a = 1", "b = 2"]);
        assert_eq!(beside[&6], ["c = x", "a = 1", "b = 2"]);
        // Not in the function above, nor below the line stopped at.
        assert!(!beside.contains_key(&1) && !beside.contains_key(&7));
    }
}
