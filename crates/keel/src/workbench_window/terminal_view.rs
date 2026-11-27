//! A terminal drawn in the window: the cells of its screen, the cursor,
//! the selection, and keys, mouse and clipboard sent to the shell.

use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use futures::StreamExt as _;
use gpui::prelude::FluentBuilder as _;
use gpui::{
    App, AppContext as _, Bounds, ClipboardEntry, ClipboardItem, Context, EventEmitter,
    FocusHandle, Focusable, Font, FontStyle, FontWeight, Hsla, InteractiveElement as _,
    IntoElement, KeyBinding, KeyDownEvent, MouseButton, MouseDownEvent, MouseMoveEvent,
    MouseUpEvent, NoAction, ParentElement as _, Pixels, Point, Render, Rgba, ScrollEvent,
    SharedString, StatefulInteractiveElement as _, StrikethroughStyle, Styled as _, Task,
    TextAlign, TextRun, UnderlineStyle, Window, actions, canvas, div, fill, outline, point, px,
    relative, size,
};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::input::{Input, InputEvent, InputState};
use gpui_component::{ActiveTheme as _, Icon, Selectable as _, Sizable as _};
use gpui_kit_assets::IconName;

use super::notices::{ago, fill as fill_text};
use super::terminal_fixes::{QuickFix, quick_fix};
use crate::i18n::Strings;
use terminal::{
    CellPoint, CellView, CommandState, CursorShape, Found, Key, LinkTarget, Modifiers, MouseAction,
    Palette, Progress, PromptMark, Rgb, SelectionKind, ShellCommand, Side, Snapshot, Spawn,
    TermMode, Terminal, TerminalEvent,
};

/// The lines of a terminal kept for the next run, as VS Code's
/// `terminal.integrated.persistentSessionScrollback`.
const RESTORED_LINES: usize = 1000;

/// How long the prompt Cmd+↑ went to stays lit.
const FLASH: Duration = Duration::from_millis(1200);
/// How long a command waits for a prompt the shell never marks.
const PROMPT_PATIENCE: Duration = Duration::from_secs(4);

/// The key context of a terminal, for its own shortcuts.
pub(crate) const TERMINAL_CONTEXT: &str = "Terminal";

actions!(
    terminal,
    [
        Copy,
        Paste,
        SelectAll,
        Clear,
        ScrollPageUp,
        ScrollPageDown,
        Find
    ]
);

/// The terminal's own shortcuts, and keys taken back from the window so
/// they reach the shell: Tab instead of moving the focus, Ctrl+R for the
/// shell's history instead of Open Recent…
pub(crate) fn bindings(window_keys_for_shell: &[String], cx: &App) -> Vec<KeyBinding> {
    let context = Some(TERMINAL_CONTEXT);
    let mut bindings = vec![
        KeyBinding::new("cmd-c", Copy, context),
        KeyBinding::new("cmd-v", Paste, context),
        KeyBinding::new("cmd-a", SelectAll, context),
        KeyBinding::new("cmd-k", Clear, context),
        KeyBinding::new("cmd-f", Find, context),
        KeyBinding::new("shift-pageup", ScrollPageUp, context),
        KeyBinding::new("shift-pagedown", ScrollPageDown, context),
        KeyBinding::new("tab", NoAction, context),
        KeyBinding::new("shift-tab", NoAction, context),
        KeyBinding::new("escape", NoAction, context),
    ];
    let predicate = gpui::KeyBindingContextPredicate::parse(TERMINAL_CONTEXT)
        .ok()
        .map(Rc::new);
    for keys in window_keys_for_shell {
        if let Ok(binding) =
            KeyBinding::load(keys, Box::new(NoAction), predicate.clone(), true, None, cx)
        {
            bindings.push(binding);
        }
    }
    bindings
}

/// What a terminal tells the window.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum TerminalViewEvent {
    /// The shell ended: the terminal closes, as in VS Code.
    Exited,
    TitleChanged,
    /// The terminal took the keyboard.
    Focused,
    /// Cmd+click on a path such as `src/main.rs:12:5`.
    OpenFile {
        path: PathBuf,
        line: Option<u32>,
        column: Option<u32>,
    },
    /// A command ended (shell integration).
    CommandFinished(ShellCommand),
    /// A question for Claude Code, ready to send.
    AskClaude(String),
    /// A short message for the status bar.
    Status(String),
    /// A program asked for a notification (OSC 9 or 777).
    Notify(String),
    /// A program rang the bell.
    Bell,
}

/// How a terminal's tab looks.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct TabStatus {
    /// The last command failed.
    pub failed: bool,
    pub progress: Option<Progress>,
}

/// The menu a command's dot opens.
struct DotMenu {
    id: u64,
    row: usize,
}

/// What the dot's menu does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DotAction {
    CopyCommand,
    CopyOutput,
    Rerun,
    AskClaude,
}

/// The bar after a command failed, with its fix if one is known.
struct Failure {
    command: ShellCommand,
    fix: Option<QuickFix>,
}

/// Font and size of the cells, measured when the terminal is drawn.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Metrics {
    origin: Point<Pixels>,
    cell_width: Pixels,
    line_height: Pixels,
}

impl Metrics {
    /// The cell under a point, and the half of it, for selections.
    fn cell_at(&self, position: Point<Pixels>, snapshot_size: (usize, usize)) -> (CellPoint, Side) {
        let (columns, lines) = snapshot_size;
        let x = ((position.x - self.origin.x) / self.cell_width).max(0.);
        let y = ((position.y - self.origin.y) / self.line_height).max(0.);
        let column = (x as usize).min(columns.saturating_sub(1));
        let line = (y as usize).min(lines.saturating_sub(1));
        let side = if x.fract() < 0.5 {
            Side::Left
        } else {
            Side::Right
        };
        (CellPoint { line, column }, side)
    }
}

pub(crate) struct TerminalView {
    terminal: Arc<Terminal>,
    focus_handle: FocusHandle,
    /// The folder the terminal started in.
    cwd: PathBuf,
    /// The title a program set, if any.
    title: Option<String>,
    font_family: SharedString,
    font_size: Pixels,
    option_is_meta: bool,
    metrics: Rc<Cell<Option<Metrics>>>,
    /// The size of the screen in cells, from the last drawing.
    grid_size: Rc<Cell<(usize, usize)>>,
    /// Lines of a trackpad scroll not yet worth a whole line.
    scroll_remainder: f32,
    /// Points of a Cmd+scroll not yet worth a size of the text.
    zoom_remainder: f32,
    /// A drag that selects text, rather than going to the program.
    selecting: bool,
    exited: bool,
    find: Option<FindBar>,
    /// How far the program in front says it is (OSC 9;4).
    progress: Option<Progress>,
    /// The command Cmd+↑ went to, lit for a moment.
    flash: Option<(u64, Instant)>,
    failure: Option<Failure>,
    dot_menu: Option<DotMenu>,
    /// The prompts drawn last, for clicks on their dots.
    prompts: Rc<RefCell<Vec<PromptMark>>>,
    /// A command to type once the shell draws its prompt.
    waiting_run: Option<String>,
    strings: &'static Strings,
    _events: Task<()>,
    _subscriptions: Vec<gpui::Subscription>,
}

impl EventEmitter<TerminalViewEvent> for TerminalView {}

impl Focusable for TerminalView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

/// How a terminal looks: the theme's colors and the terminal's font.
pub(crate) struct TerminalLook {
    pub font_family: SharedString,
    pub font_size: f32,
    pub option_is_meta: bool,
    pub strings: &'static Strings,
}

/// Cmd+F in a terminal: the field and its switches.
struct FindBar {
    input: gpui::Entity<InputState>,
    case_sensitive: bool,
    regex: bool,
    /// Whether the last search found something.
    found: bool,
    /// The text last searched, so only a new text starts over.
    searched: String,
    _subscription: gpui::Subscription,
}

/// The theme's colors for terminals: the editors' background and text,
/// with VS Code's ANSI colors for dark or light themes.
pub(crate) fn palette(cx: &App) -> Palette {
    let theme = cx.theme();
    let background = to_rgb(theme.background);
    let foreground = to_rgb(theme.foreground);
    if theme.mode.is_dark() {
        Palette::dark(foreground, background)
    } else {
        Palette::light(foreground, background)
    }
}

fn to_rgb(color: Hsla) -> Rgb {
    let color = color.to_rgb();
    let channel = |value: f32| (value.clamp(0., 1.) * 255.).round() as u8;
    Rgb {
        r: channel(color.r),
        g: channel(color.g),
        b: channel(color.b),
    }
}

fn to_hsla(color: Rgb) -> Hsla {
    Rgba {
        r: color.r as f32 / 255.,
        g: color.g as f32 / 255.,
        b: color.b as f32 / 255.,
        a: 1.,
    }
    .into()
}

/// A terminal whose shell is running, before its view exists.
pub(crate) struct Started {
    terminal: Terminal,
    events: futures::channel::mpsc::UnboundedReceiver<TerminalEvent>,
    cwd: PathBuf,
}

/// How to start the user's shell in a folder.
pub(crate) fn shell_spawn(cwd: PathBuf, cx: &App) -> Spawn {
    let language = terminal::lang_for(sys_locale::get_locale().as_deref());
    let size = terminal::Size {
        columns: 80,
        lines: 24,
        cell_width: 8,
        cell_height: 16,
    };
    #[allow(unused_mut)]
    let mut spawn = Spawn::shell(cwd, size, palette(cx), &language);
    #[cfg(test)]
    TEST_HOME.with(|home| {
        if let Some(home) = home.borrow().as_ref() {
            spawn.command = Some(("/bin/zsh".into(), vec!["-i".into()]));
            spawn
                .env
                .insert("HOME".into(), home.to_string_lossy().into_owned());
            spawn.integrate_zsh(home).unwrap();
        }
    });
    spawn
}

#[cfg(test)]
thread_local! {
    /// A test's own home: its shells are a zsh that reads nothing of the
    /// user's.
    pub(crate) static TEST_HOME: RefCell<Option<PathBuf>> = const { RefCell::new(None) };
}

/// Starts a terminal; tests run a script instead of the login shell.
pub(crate) fn start(mut spawn: Spawn) -> std::io::Result<Started> {
    spawn.integrate_shell();
    let cwd = spawn.cwd.clone();
    let (sender, events) = futures::channel::mpsc::unbounded();
    let terminal = Terminal::spawn(spawn, move |event| {
        let _ = sender.unbounded_send(event);
    })?;
    Ok(Started {
        terminal,
        events,
        cwd,
    })
}

impl TerminalView {
    pub(crate) fn new(
        started: Started,
        look: TerminalLook,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let Started {
            terminal,
            mut events,
            cwd,
        } = started;
        let pump = cx.spawn(async move |this, cx| {
            while let Some(event) = events.next().await {
                let mut batch = vec![event];
                while let Ok(event) = events.try_recv() {
                    batch.push(event);
                }
                if this
                    .update(cx, |this, cx| this.handle_events(batch, cx))
                    .is_err()
                {
                    break;
                }
            }
        });
        let focus_handle = cx.focus_handle();
        let subscriptions = vec![
            cx.on_focus_in(&focus_handle, window, |this, _, cx| {
                this.send_focus(true);
                cx.emit(TerminalViewEvent::Focused);
                cx.notify();
            }),
            cx.on_focus_out(&focus_handle, window, |this, _, _, cx| {
                this.send_focus(false);
                cx.notify();
            }),
        ];
        Self {
            terminal: Arc::new(terminal),
            focus_handle,
            cwd,
            title: None,
            font_family: look.font_family,
            font_size: px(look.font_size),
            option_is_meta: look.option_is_meta,
            metrics: Rc::new(Cell::new(None)),
            grid_size: Rc::new(Cell::new((80, 24))),
            scroll_remainder: 0.,
            zoom_remainder: 0.,
            selecting: false,
            exited: false,
            find: None,
            progress: None,
            flash: None,
            failure: None,
            dot_menu: None,
            prompts: Rc::new(RefCell::new(Vec::new())),
            waiting_run: None,
            strings: look.strings,
            _events: pump,
            _subscriptions: subscriptions,
        }
    }

    pub(crate) fn terminal(&self) -> &Arc<Terminal> {
        &self.terminal
    }

    /// The tab's name: the program in front (`zsh`, `node`, `claude`), or
    /// the title a program set.
    pub(crate) fn name(&self) -> String {
        if let Some(title) = &self.title {
            return title.clone();
        }
        self.terminal
            .foreground_name()
            .unwrap_or_else(|| "zsh".to_owned())
    }

    /// The process the terminal started, for finding its ports.
    pub(crate) fn pid(&self) -> u32 {
        self.terminal.pid()
    }

    /// Types a command and runs it, as if typed at the prompt.
    pub(crate) fn run(&self, command: &str) {
        self.terminal.write(format!("{command}\r").into_bytes());
    }

    /// Types a command once the shell's prompt is drawn, as a person does:
    /// typed earlier, the terminal itself echoes it above the prompt.
    pub(crate) fn run_at_prompt(&mut self, command: &str, cx: &mut Context<Self>) {
        if self.terminal.at_prompt() {
            self.run(command);
            return;
        }
        self.waiting_run = Some(command.to_owned());
        // A shell that never marks its prompt gets the command anyway.
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(PROMPT_PATIENCE).await;
            this.update(cx, |this, _| this.run_waiting()).ok();
        })
        .detach();
    }

    fn run_waiting(&mut self) {
        if let Some(command) = self.waiting_run.take() {
            self.run(&command);
        }
    }

    /// The folder of the program in front, which follows `cd`.
    pub(crate) fn current_dir(&self) -> PathBuf {
        self.terminal
            .current_dir()
            .unwrap_or_else(|| self.cwd.clone())
    }

    fn handle_events(&mut self, events: Vec<TerminalEvent>, cx: &mut Context<Self>) {
        for event in events {
            match event {
                TerminalEvent::Changed => {}
                TerminalEvent::Bell => cx.emit(TerminalViewEvent::Bell),
                TerminalEvent::Notify(text) => cx.emit(TerminalViewEvent::Notify(text)),
                TerminalEvent::Title(title) => {
                    self.title = title.filter(|title| !title.trim().is_empty());
                    cx.emit(TerminalViewEvent::TitleChanged);
                }
                TerminalEvent::Copy(text) => cx.write_to_clipboard(ClipboardItem::new_string(text)),
                TerminalEvent::Exited(_) => {
                    self.exited = true;
                    cx.emit(TerminalViewEvent::Exited);
                }
                TerminalEvent::CommandStarted(_) => {
                    self.failure = None;
                    self.dot_menu = None;
                    cx.emit(TerminalViewEvent::TitleChanged);
                }
                TerminalEvent::CommandFinished(command) => {
                    self.failure = match command.state() {
                        // Ctrl+C is not a failure to fix.
                        Some(CommandState::Failed(code)) if code != 130 => {
                            let output = self.terminal.output_of(command.id).unwrap_or_default();
                            let fix = command
                                .text
                                .as_deref()
                                .and_then(|text| quick_fix(text, &output));
                            Some(Failure {
                                command: command.clone(),
                                fix,
                            })
                        }
                        _ => None,
                    };
                    cx.emit(TerminalViewEvent::CommandFinished(command));
                    cx.emit(TerminalViewEvent::TitleChanged);
                }
                TerminalEvent::Progress(progress) => {
                    self.progress = progress;
                    cx.emit(TerminalViewEvent::TitleChanged);
                }
            }
        }
        if self.waiting_run.is_some() && self.terminal.at_prompt() {
            self.run_waiting();
        }
        cx.notify();
    }

    // -----------------------------------------------------------------
    // Commands (shell integration)

    /// Cmd+↑ and Cmd+↓: to the command before or after the one shown.
    pub(crate) fn jump_to_command(&mut self, older: bool, cx: &mut Context<Self>) {
        let id = self.terminal.scroll_to_command(older);
        self.flash(id, cx);
    }

    fn flash(&mut self, id: Option<u64>, cx: &mut Context<Self>) {
        self.flash = id.map(|id| (id, Instant::now()));
        cx.notify();
    }

    /// Whether the shell itself is in front, at its prompt, rather than a
    /// program it started.
    pub(crate) fn runs_shell(&self) -> bool {
        self.terminal.foreground_name().is_none_or(|name| {
            matches!(
                name.trim_start_matches('-'),
                "zsh" | "bash" | "fish" | "sh" | "login"
            )
        })
    }

    /// What the terminal shows, to write back after a restart.
    pub(crate) fn contents(&self) -> Option<Vec<u8>> {
        self.terminal.contents(RESTORED_LINES)
    }

    /// The menu of the last command's dot, for the snapshots.
    #[cfg(feature = "snapshots")]
    pub(crate) fn open_last_dot_menu(&mut self, cx: &mut Context<Self>) {
        let id = self.terminal.last_command().map(|command| command.id);
        let row = self
            .prompts
            .borrow()
            .iter()
            .find(|prompt| Some(prompt.id) == id)
            .map(|prompt| prompt.row);
        if let (Some(id), Some(row)) = (id, row) {
            self.dot_menu = Some(DotMenu { id, row });
            cx.notify();
        }
    }

    /// Scrolls up into the output, for the snapshots.
    #[cfg(feature = "snapshots")]
    pub(crate) fn scroll_for_snapshot(&mut self, lines: i32, cx: &mut Context<Self>) {
        self.terminal.scroll(lines);
        cx.notify();
    }

    /// Shows a command at the top of the screen, lit for a moment.
    pub(crate) fn show_command(&mut self, id: u64, cx: &mut Context<Self>) {
        self.terminal.scroll_to(id);
        self.flash(Some(id), cx);
    }

    pub(crate) fn last_command(&self) -> Option<ShellCommand> {
        self.terminal.last_command()
    }

    pub(crate) fn commands(&self) -> Vec<ShellCommand> {
        self.terminal.commands()
    }

    pub(crate) fn output_of(&self, id: u64) -> Option<String> {
        self.terminal.output_of(id)
    }

    /// Puts text at the prompt of the program in front, as a paste, and
    /// sends it when asked.
    pub(crate) fn send(&self, text: &str, submit: bool) {
        self.terminal.paste(text);
        if submit {
            self.terminal.write(&b"\r"[..]);
        }
    }

    pub(crate) fn status(&self) -> TabStatus {
        TabStatus {
            failed: self.failure.is_some(),
            progress: self.progress,
        }
    }

    /// What to ask Claude about a command: why it failed, or what its
    /// output means.
    pub(crate) fn claude_prompt_for(&self, command: &ShellCommand) -> String {
        let output = tail(&self.terminal.output_of(command.id).unwrap_or_default());
        let text = command.text.clone().unwrap_or_default();
        match command.state() {
            Some(CommandState::Failed(code)) => fill_text(
                self.strings.terminal_claude_failed,
                &[
                    ("command", &text),
                    ("code", &code.to_string()),
                    ("output", &output),
                ],
            ),
            _ => fill_text(
                self.strings.terminal_claude_command,
                &[("command", &text), ("output", &output)],
            ),
        }
    }

    /// Cmd+L: the selected text, else the last command, for Claude Code.
    pub(crate) fn claude_prompt(&self) -> Option<String> {
        if let Some(text) = self.terminal.selected_text() {
            return Some(fill_text(
                self.strings.terminal_claude_selection,
                &[("text", &tail(&text))],
            ));
        }
        let command = self.terminal.last_command()?;
        Some(self.claude_prompt_for(&command))
    }

    fn dot_action(&mut self, id: u64, action: DotAction, cx: &mut Context<Self>) {
        self.dot_menu = None;
        cx.notify();
        let Some(command) = self
            .terminal
            .commands()
            .into_iter()
            .find(|command| command.id == id)
        else {
            return;
        };
        match action {
            DotAction::CopyCommand => {
                if let Some(text) = command.text {
                    cx.write_to_clipboard(ClipboardItem::new_string(text));
                    cx.emit(TerminalViewEvent::Status(
                        self.strings.terminal_command_copied.to_owned(),
                    ));
                }
            }
            DotAction::CopyOutput => {
                if let Some(output) = self.terminal.output_of(id) {
                    cx.write_to_clipboard(ClipboardItem::new_string(output));
                    cx.emit(TerminalViewEvent::Status(
                        self.strings.terminal_output_copied.to_owned(),
                    ));
                }
            }
            DotAction::Rerun => {
                if let Some(text) = command.text {
                    self.run(&text);
                }
            }
            DotAction::AskClaude => {
                let prompt = self.claude_prompt_for(&command);
                cx.emit(TerminalViewEvent::AskClaude(prompt));
            }
        }
    }

    /// The bar's fix: runs what the program suggested, or frees the port
    /// and runs the command again.
    fn apply_fix(&mut self, cx: &mut Context<Self>) {
        let Some(failure) = self.failure.take() else {
            return;
        };
        cx.notify();
        match failure.fix {
            Some(QuickFix::Run(command)) => self.run(&command),
            Some(QuickFix::FreePort { port }) => {
                let rerun = failure.command.text.clone();
                let strings = self.strings;
                cx.spawn(async move |this, cx| {
                    let background = cx.background_executor().clone();
                    let free = background
                        .spawn(async move {
                            for pid in terminal::listeners_on(port) {
                                terminal::end_process(pid);
                            }
                            // The port is free once the program has let go.
                            for _ in 0..30 {
                                if terminal::listeners_on(port).is_empty() {
                                    return true;
                                }
                                std::thread::sleep(Duration::from_millis(100));
                            }
                            false
                        })
                        .await;
                    this.update(cx, |this, cx| {
                        let port = port.to_string();
                        if free {
                            if let Some(rerun) = &rerun {
                                this.run(rerun);
                            }
                            cx.emit(TerminalViewEvent::Status(fill_text(
                                strings.terminal_port_freed,
                                &[("port", &port)],
                            )));
                        } else {
                            cx.emit(TerminalViewEvent::Status(fill_text(
                                strings.terminal_port_still_taken,
                                &[("port", &port)],
                            )));
                        }
                    })
                    .ok();
                })
                .detach();
            }
            None => {}
        }
    }

    fn send_focus(&self, focused: bool) {
        if let Some(bytes) = terminal::focus_bytes(focused, self.terminal.mode()) {
            self.terminal.write(bytes);
        }
    }

    // -----------------------------------------------------------------
    // Keys and clipboard

    fn key_down(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        // Keys typed in the find field are not for the shell.
        if !self.focus_handle.is_focused(window) {
            return;
        }
        if self.dot_menu.take().is_some() {
            cx.notify();
        }
        let keystroke = &event.keystroke;
        let modifiers = Modifiers {
            shift: keystroke.modifiers.shift,
            alt: keystroke.modifiers.alt,
            control: keystroke.modifiers.control,
            command: keystroke.modifiers.platform,
        };
        let key = match keystroke.key.as_str() {
            "enter" => Key::Enter,
            "tab" => Key::Tab,
            "backspace" => Key::Backspace,
            "escape" => Key::Escape,
            "up" => Key::Up,
            "down" => Key::Down,
            "left" => Key::Left,
            "right" => Key::Right,
            "home" => Key::Home,
            "end" => Key::End,
            "pageup" => Key::PageUp,
            "pagedown" => Key::PageDown,
            "insert" => Key::Insert,
            "delete" => Key::Delete,
            name if name.len() > 1 && name.starts_with('f') && name[1..].parse::<u8>().is_ok() => {
                Key::Function(name[1..].parse().unwrap_or(0))
            }
            name => Key::Character {
                key: name,
                text: keystroke.key_char.as_deref(),
            },
        };
        let Some(bytes) =
            terminal::key_bytes(key, modifiers, self.terminal.mode(), self.option_is_meta)
        else {
            return;
        };
        self.terminal.clear_selection();
        self.terminal.write(bytes);
        cx.stop_propagation();
        cx.notify();
    }

    fn copy(&mut self, _: &Copy, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(text) = self.terminal.selected_text() {
            cx.write_to_clipboard(ClipboardItem::new_string(text));
        }
    }

    /// Pastes text. An image, which only Claude Code can take, is left for
    /// it to read from the clipboard: it does so on Ctrl+V.
    fn paste(&mut self, _: &Paste, _: &mut Window, cx: &mut Context<Self>) {
        let Some(item) = cx.read_from_clipboard() else {
            return;
        };
        if let Some(text) = item.text() {
            self.terminal.paste(&text);
        } else if item
            .entries()
            .iter()
            .any(|entry| matches!(entry, ClipboardEntry::Image(_)))
        {
            self.terminal.write(&[0x16][..]);
        }
        cx.notify();
    }

    fn select_all(&mut self, _: &SelectAll, _: &mut Window, cx: &mut Context<Self>) {
        self.terminal.select_all();
        cx.notify();
    }

    fn clear(&mut self, _: &Clear, _: &mut Window, cx: &mut Context<Self>) {
        self.terminal.clear();
        cx.notify();
    }

    fn scroll_page(&mut self, up: bool, cx: &mut Context<Self>) {
        let lines = self.grid_size.get().1.max(2) as i32 - 1;
        self.terminal.scroll(if up { lines } else { -lines });
        cx.notify();
    }

    // -----------------------------------------------------------------
    // Find

    /// Cmd+F: shows the find field, with the selected text in it.
    fn open_find(&mut self, _: &Find, window: &mut Window, cx: &mut Context<Self>) {
        if self.find.is_none() {
            let placeholder = self.strings.find_in_terminal;
            let input = cx.new(|cx| InputState::new(window, cx).placeholder(placeholder));
            let subscription = cx.subscribe_in(
                &input,
                window,
                |this, _, event: &InputEvent, _, cx| match event {
                    InputEvent::Change => this.find_again(None, cx),
                    InputEvent::PressEnter { shift, .. } => this.find_again(Some(!*shift), cx),
                    _ => {}
                },
            );
            self.find = Some(FindBar {
                input,
                case_sensitive: false,
                regex: false,
                found: true,
                searched: String::new(),
                _subscription: subscription,
            });
        }
        let selected = self
            .terminal
            .selected_text()
            .filter(|text| !text.contains('\n'));
        if let Some(find) = &self.find {
            find.input.update(cx, |input, cx| {
                if let Some(text) = selected {
                    input.set_value(text, window, cx);
                }
                input.focus(window, cx);
                input.select_all(window, cx);
            });
        }
        self.find_again(None, cx);
    }

    /// Searches for what the field holds: from the newest output when the
    /// text changed, or to the next match toward older output (`Some(true)`)
    /// or newer output (`Some(false)`).
    fn find_again(&mut self, older: Option<bool>, cx: &mut Context<Self>) {
        let Some(find) = &mut self.find else {
            return;
        };
        let query = find.input.read(cx).value().to_string();
        if older.is_none() && query == find.searched {
            return;
        }
        find.searched = query.clone();
        let found = self.terminal.find(
            &query,
            find.regex,
            find.case_sensitive,
            older.unwrap_or(true),
        );
        find.found = found || query.is_empty();
        cx.notify();
    }

    fn toggle_find_option(
        &mut self,
        option: fn(&mut FindBar) -> &mut bool,
        cx: &mut Context<Self>,
    ) {
        if let Some(find) = &mut self.find {
            let value = option(find);
            *value = !*value;
            find.searched.clear();
        }
        self.terminal.stop_finding();
        self.find_again(None, cx);
    }

    fn close_find(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.find = None;
        self.terminal.stop_finding();
        self.focus_handle.focus(window, cx);
        cx.notify();
    }

    fn render_find(&self, cx: &mut Context<Self>) -> Option<gpui::AnyElement> {
        let find = self.find.as_ref()?;
        let strings = self.strings;
        let theme = cx.theme();
        let switch = |id: &'static str, icon: IconName, on: bool, tooltip: &'static str| {
            Button::new(id)
                .ghost()
                .small()
                .icon(Icon::new(icon))
                .selected(on)
                .tooltip(tooltip)
        };
        Some(
            div()
                .absolute()
                .top_1()
                .right_3()
                .p_1()
                .flex()
                .items_center()
                .gap_0p5()
                .rounded(px(8.))
                .border_1()
                .border_color(if find.found {
                    theme.border
                } else {
                    theme.danger
                })
                .bg(theme.popover)
                .shadow_md()
                .on_action(
                    cx.listener(|this, _: &gpui_component::input::Escape, window, cx| {
                        this.close_find(window, cx)
                    }),
                )
                .child(div().w(px(200.)).child(Input::new(&find.input).xsmall()))
                .child(
                    switch(
                        "terminal-find-case",
                        IconName::CaseSensitive,
                        find.case_sensitive,
                        strings.match_case,
                    )
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.toggle_find_option(|find| &mut find.case_sensitive, cx)
                    })),
                )
                .child(
                    switch(
                        "terminal-find-regex",
                        IconName::Regex,
                        find.regex,
                        strings.use_regex,
                    )
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.toggle_find_option(|find| &mut find.regex, cx)
                    })),
                )
                .child(
                    Button::new("terminal-find-older")
                        .ghost()
                        .small()
                        .icon(Icon::new(IconName::ArrowUp))
                        .tooltip(strings.find_older)
                        .on_click(cx.listener(|this, _, _, cx| this.find_again(Some(true), cx))),
                )
                .child(
                    Button::new("terminal-find-newer")
                        .ghost()
                        .small()
                        .icon(Icon::new(IconName::ArrowDown))
                        .tooltip(strings.find_newer)
                        .on_click(cx.listener(|this, _, _, cx| this.find_again(Some(false), cx))),
                )
                .child(
                    Button::new("terminal-find-close")
                        .ghost()
                        .small()
                        .icon(Icon::new(IconName::X))
                        .tooltip(strings.close_find)
                        .on_click(cx.listener(|this, _, window, cx| this.close_find(window, cx))),
                )
                .into_any_element(),
        )
    }

    // -----------------------------------------------------------------
    // Mouse

    fn cell_at(&self, position: Point<Pixels>) -> Option<(CellPoint, Side)> {
        let metrics = self.metrics.get()?;
        Some(metrics.cell_at(position, self.grid_size.get()))
    }

    /// Whether the program takes the mouse. Shift keeps it for selecting,
    /// as in every terminal.
    fn mouse_goes_to_program(&self, modifiers: gpui::Modifiers) -> bool {
        !modifiers.shift && self.terminal.mode().intersects(TermMode::MOUSE_MODE)
    }

    fn report_mouse(
        &self,
        button: Option<terminal::MouseButton>,
        action: MouseAction,
        position: Point<Pixels>,
        modifiers: gpui::Modifiers,
    ) -> bool {
        let Some((cell, _)) = self.cell_at(position) else {
            return false;
        };
        let modifiers = Modifiers {
            shift: modifiers.shift,
            alt: modifiers.alt,
            control: modifiers.control,
            command: false,
        };
        let Some(bytes) = terminal::mouse_bytes(
            button,
            action,
            cell.column,
            cell.line,
            modifiers,
            self.terminal.mode(),
        ) else {
            return false;
        };
        self.terminal.write(bytes);
        true
    }

    fn mouse_down(&mut self, event: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        self.focus_handle.focus(window, cx);
        if self.dot_menu.take().is_some() {
            cx.notify();
        }
        // A click on a command's dot, in the margin, opens its menu.
        if event.button == MouseButton::Left
            && let Some(metrics) = self.metrics.get()
            && event.position.x < metrics.origin.x
            && event.position.y >= metrics.origin.y
        {
            let row = ((event.position.y - metrics.origin.y) / metrics.line_height) as usize;
            let prompt = self
                .prompts
                .borrow()
                .iter()
                .find(|prompt| prompt.row == row && prompt.state.is_some())
                .cloned();
            if let Some(prompt) = prompt {
                self.dot_menu = Some(DotMenu { id: prompt.id, row });
                cx.notify();
                return;
            }
        }
        let button = match event.button {
            MouseButton::Left => terminal::MouseButton::Left,
            MouseButton::Middle => terminal::MouseButton::Middle,
            MouseButton::Right => terminal::MouseButton::Right,
            _ => return,
        };
        if self.mouse_goes_to_program(event.modifiers) {
            self.report_mouse(
                Some(button),
                MouseAction::Press,
                event.position,
                event.modifiers,
            );
            return;
        }
        if event.button != MouseButton::Left {
            return;
        }
        let Some((cell, side)) = self.cell_at(event.position) else {
            return;
        };
        if event.modifiers.platform {
            self.open_link_at(cell, cx);
            return;
        }
        let kind = match event.click_count {
            0 | 1 => SelectionKind::Simple,
            2 => SelectionKind::Word,
            _ => SelectionKind::Line,
        };
        self.terminal.start_selection(cell, side, kind);
        self.selecting = true;
        cx.notify();
    }

    /// Cmd+click: opens the web address in the browser, or the file in the
    /// editor at its line, as VS Code does.
    fn open_link_at(&mut self, cell: CellPoint, cx: &mut Context<Self>) {
        let snapshot = self.terminal.snapshot();
        if let Some(url) = snapshot
            .rows
            .get(cell.line)
            .and_then(|row| row.get(cell.column))
            .and_then(|cell| cell.link.clone())
        {
            cx.open_url(&url);
            return;
        }
        let text = snapshot.line_text(cell.line);
        let columns = snapshot.line_columns(cell.line);
        let link = terminal::find_links(&text).into_iter().find(|link| {
            let first = columns.get(link.start).copied();
            let last = columns.get(link.end.saturating_sub(1)).copied();
            matches!((first, last), (Some(first), Some(last)) if (first..=last).contains(&cell.column))
        });
        match link.map(|link| link.target) {
            Some(LinkTarget::Url(url)) => cx.open_url(&url),
            Some(LinkTarget::File { path, line, column }) => {
                if let Some(path) = self.resolve_path(&path) {
                    cx.emit(TerminalViewEvent::OpenFile { path, line, column });
                }
            }
            None => {}
        }
    }

    /// A path printed in the terminal, as a file that exists.
    fn resolve_path(&self, path: &str) -> Option<PathBuf> {
        let path = match path.strip_prefix("~/") {
            Some(rest) => std::env::home_dir()?.join(rest),
            None => PathBuf::from(path),
        };
        let path = if path.is_absolute() {
            path
        } else {
            self.current_dir().join(path)
        };
        path.is_file().then_some(path)
    }

    fn mouse_move(&mut self, event: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        if self.selecting {
            if event.pressed_button == Some(MouseButton::Left)
                && let Some((cell, side)) = self.cell_at(event.position)
            {
                self.terminal.extend_selection(cell, side);
                cx.notify();
            }
            return;
        }
        if self.mouse_goes_to_program(event.modifiers) {
            let button = match event.pressed_button {
                Some(MouseButton::Left) => Some(terminal::MouseButton::Left),
                Some(MouseButton::Middle) => Some(terminal::MouseButton::Middle),
                Some(MouseButton::Right) => Some(terminal::MouseButton::Right),
                _ => None,
            };
            self.report_mouse(button, MouseAction::Move, event.position, event.modifiers);
        }
    }

    fn mouse_up(&mut self, event: &MouseUpEvent, _: &mut Window, cx: &mut Context<Self>) {
        if self.selecting {
            self.selecting = false;
            // A click without a drag leaves nothing selected.
            if self.terminal.selected_text().is_none() {
                self.terminal.clear_selection();
            }
            cx.notify();
            return;
        }
        if self.mouse_goes_to_program(event.modifiers) {
            let button = match event.button {
                MouseButton::Left => terminal::MouseButton::Left,
                MouseButton::Middle => terminal::MouseButton::Middle,
                MouseButton::Right => terminal::MouseButton::Right,
                _ => return,
            };
            self.report_mouse(
                Some(button),
                MouseAction::Release,
                event.position,
                event.modifiers,
            );
        }
    }

    /// Makes the text `steps` points bigger (or smaller, below zero), once
    /// a whole point has gathered; between 8 and 40 points.
    fn zoom_by(&mut self, steps: f32, cx: &mut Context<Self>) {
        self.zoom_remainder += steps;
        let whole = self.zoom_remainder.trunc();
        if whole == 0. {
            return;
        }
        self.zoom_remainder -= whole;
        let size = (self.font_size.as_f32() + whole).clamp(8., 40.);
        if size != self.font_size.as_f32() {
            self.font_size = px(size);
            cx.notify();
        }
    }

    #[cfg(test)]
    fn font_size(&self) -> f32 {
        self.font_size.as_f32()
    }

    fn scroll(&mut self, event: &ScrollEvent, _: &mut Window, cx: &mut Context<Self>) {
        let Some(metrics) = self.metrics.get() else {
            return;
        };
        let delta = event.delta.pixel_delta(metrics.line_height).y;
        // Cmd with the wheel makes the text bigger or smaller, as in Zed.
        if event.modifiers.platform {
            self.zoom_by(delta / metrics.line_height, cx);
            return;
        }
        self.scroll_remainder += delta / metrics.line_height;
        let lines = self.scroll_remainder.trunc() as i32;
        self.scroll_remainder -= lines as f32;
        if lines == 0 {
            return;
        }
        let mode = self.terminal.mode();
        if self.mouse_goes_to_program(event.modifiers) {
            let button = if lines > 0 {
                terminal::MouseButton::WheelUp
            } else {
                terminal::MouseButton::WheelDown
            };
            for _ in 0..lines.unsigned_abs() {
                self.report_mouse(
                    Some(button),
                    MouseAction::Press,
                    event.position,
                    event.modifiers,
                );
            }
        } else if mode.contains(TermMode::ALT_SCREEN) && mode.contains(TermMode::ALTERNATE_SCROLL) {
            // Full-screen programs without the mouse (less, man) get arrows.
            let key = if lines > 0 { Key::Up } else { Key::Down };
            if let Some(bytes) = terminal::key_bytes(key, Modifiers::default(), mode, false) {
                self.terminal
                    .write(bytes.repeat(lines.unsigned_abs() as usize));
            }
        } else {
            self.terminal.scroll(lines);
        }
        cx.notify();
    }
}

impl Render for TerminalView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = palette(cx);
        self.terminal.set_palette(palette);
        let theme = cx.theme();
        let flash = self.flash.and_then(|(id, at)| {
            let gone = at.elapsed().as_secs_f32() / FLASH.as_secs_f32();
            (gone < 1.).then_some((id, 0.22 * (1. - gone)))
        });
        if flash.is_some() {
            window.request_animation_frame();
        } else {
            self.flash = None;
        }
        let mut font = gpui::font(self.font_family.clone());
        font.fallbacks = symbol_fonts(window);
        let style = PaintStyle {
            font,
            font_size: self.font_size,
            palette,
            selection: theme.selection,
            found: theme.warning.opacity(0.35),
            found_current: theme.warning.opacity(0.8),
            focused: self.focus_handle.is_focused(window),
            dots: DotColors::new(cx),
            flash,
            flash_color: theme.info,
        };
        let terminal = self.terminal.clone();
        let metrics = self.metrics.clone();
        let grid_size = self.grid_size.clone();
        let prompts = self.prompts.clone();
        let find = self.render_find(cx);
        let sticky = self.render_sticky(cx);
        let failure = self.render_failure(cx);
        let dot_menu = self.render_dot_menu(cx);
        div()
            .id("terminal")
            .relative()
            .key_context(TERMINAL_CONTEXT)
            .track_focus(&self.focus_handle)
            // A file dragged from the Explorer types its path, quoted when
            // it needs it, as Terminal does with Finder's files.
            .on_drop(
                cx.listener(|this, dragged: &super::explorer::DraggedPath, window, cx| {
                    let text = dragged
                        .all()
                        .iter()
                        .map(|path| shell_quote(&path.to_string_lossy()))
                        .collect::<Vec<_>>()
                        .join(" ");
                    this.terminal.paste(&format!("{text} "));
                    this.focus_handle.focus(window, cx);
                }),
            )
            .on_drop(
                cx.listener(|this, paths: &gpui::ExternalPaths, window, cx| {
                    let text = paths
                        .paths()
                        .iter()
                        .map(|path| shell_quote(&path.to_string_lossy()))
                        .collect::<Vec<_>>()
                        .join(" ");
                    this.terminal.paste(&format!("{text} "));
                    this.focus_handle.focus(window, cx);
                }),
            )
            .size_full()
            .pl_3()
            .pr_1()
            .pt_1()
            .bg(to_hsla(palette.background))
            .cursor_text()
            .on_action(cx.listener(Self::copy))
            .on_action(cx.listener(Self::paste))
            .on_action(cx.listener(Self::select_all))
            .on_action(cx.listener(Self::clear))
            .on_action(cx.listener(Self::open_find))
            .on_action(cx.listener(|this, _: &ScrollPageUp, _, cx| this.scroll_page(true, cx)))
            .on_action(cx.listener(|this, _: &ScrollPageDown, _, cx| this.scroll_page(false, cx)))
            .on_key_down(cx.listener(Self::key_down))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::mouse_down))
            .on_mouse_down(MouseButton::Middle, cx.listener(Self::mouse_down))
            .on_mouse_down(MouseButton::Right, cx.listener(Self::mouse_down))
            .on_mouse_move(cx.listener(Self::mouse_move))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::mouse_up))
            .on_mouse_up(MouseButton::Middle, cx.listener(Self::mouse_up))
            .on_mouse_up(MouseButton::Right, cx.listener(Self::mouse_up))
            .on_scroll(cx.listener(Self::scroll))
            .when(self.exited, |this| this.opacity(0.6))
            .child(
                canvas(
                    move |bounds, window, _cx| {
                        let measured = measure(&style, bounds.origin, window);
                        let columns = (bounds.size.width / measured.cell_width).floor().max(2.);
                        let lines = (bounds.size.height / measured.line_height).floor().max(1.);
                        terminal.resize(terminal::Size {
                            columns: columns as u16,
                            lines: lines as u16,
                            cell_width: f32::from(measured.cell_width).round() as u16,
                            cell_height: f32::from(measured.line_height).round() as u16,
                        });
                        let snapshot = terminal.snapshot();
                        metrics.set(Some(measured));
                        grid_size.set((snapshot.columns, snapshot.lines));
                        *prompts.borrow_mut() = snapshot.prompts.clone();
                        (snapshot, measured, style)
                    },
                    move |_bounds, (snapshot, measured, style), window, cx| {
                        paint_snapshot(&snapshot, measured, &style, window, cx);
                    },
                )
                .size_full(),
            )
            .children(sticky)
            .children(failure)
            .children(dot_menu)
            .children(find)
    }
}

impl TerminalView {
    fn line_height(&self) -> Pixels {
        self.metrics
            .get()
            .map(|metrics| metrics.line_height)
            .unwrap_or((self.font_size * 1.25).round())
    }

    /// The command whose output fills the screen, kept at its top; a click
    /// goes to it.
    fn render_sticky(&self, cx: &mut Context<Self>) -> Option<gpui::AnyElement> {
        let sticky = self.terminal.sticky()?;
        let theme = cx.theme();
        let dots = DotColors::new(cx);
        let id = sticky.id;
        let go_to = self.strings.terminal_go_to_command;
        let background = to_hsla(palette(cx).background);
        Some(
            div()
                .id("terminal-sticky")
                .absolute()
                .top_0()
                .left_0()
                .right_0()
                .h(self.line_height() + px(6.))
                .pl(px(3.5))
                .pr_3()
                .flex()
                .items_center()
                .gap(px(1.5))
                .bg(background)
                .border_b_1()
                .border_color(theme.border)
                .shadow_sm()
                .font_family(self.font_family.clone())
                .text_size(self.font_size)
                .text_color(theme.foreground)
                .cursor_pointer()
                .hover(|row| row.bg(theme.secondary))
                .tooltip(move |window, cx| {
                    gpui_component::tooltip::Tooltip::new(go_to).build(window, cx)
                })
                .occlude()
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, _, _, cx| {
                        this.terminal.scroll_to(id);
                        this.flash(Some(id), cx);
                        cx.stop_propagation();
                    }),
                )
                .child(dot(sticky.state, dots))
                .child(div().flex_1().min_w_0().truncate().child(sticky.text))
                .into_any_element(),
        )
    }

    /// After a failed command: its fix when one is known, and Claude.
    fn render_failure(&self, cx: &mut Context<Self>) -> Option<gpui::AnyElement> {
        let failure = self.failure.as_ref()?;
        let strings = self.strings;
        let theme = cx.theme();
        let name = failure.command.text.clone().unwrap_or_default();
        let code = match failure.command.state() {
            Some(CommandState::Failed(code)) => code,
            _ => 1,
        };
        let title = format!(
            "{} · {}",
            fill_text(
                strings.terminal_fix_failed_title,
                &[("command", &format!("«{}»", short(&name, 32)))]
            ),
            fill_text(strings.terminal_exit_code, &[("code", &code.to_string())]),
        );
        let fix = failure.fix.as_ref().map(|fix| {
            let label = match fix {
                QuickFix::FreePort { port } => {
                    fill_text(strings.terminal_free_port, &[("port", &port.to_string())])
                }
                QuickFix::Run(command) => fill_text(
                    strings.terminal_run_fix,
                    &[("command", &short(command, 40))],
                ),
            };
            Button::new("terminal-fix")
                .primary()
                .xsmall()
                .icon(Icon::new(IconName::Wrench))
                .label(label)
                .on_click(cx.listener(|this, _, _, cx| this.apply_fix(cx)))
        });
        let id = failure.command.id;
        Some(
            div()
                .id("terminal-failure")
                .absolute()
                .bottom_2()
                .right_3()
                .max_w(relative(0.94))
                .p_1()
                .pl_2()
                .flex()
                .items_center()
                .gap_1()
                .rounded(px(8.))
                .border_1()
                .border_color(theme.danger.opacity(0.45))
                .bg(theme.popover)
                .shadow_md()
                .text_xs()
                .occlude()
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .child(
                    Icon::new(IconName::CircleX)
                        .xsmall()
                        .text_color(theme.danger),
                )
                .child(
                    div()
                        .min_w_0()
                        .truncate()
                        .text_color(theme.muted_foreground)
                        .child(title),
                )
                .children(fix)
                .child(
                    Button::new("terminal-ask-claude")
                        .ghost()
                        .xsmall()
                        .icon(Icon::new(IconName::Sparkles))
                        .label(strings.terminal_ask_claude)
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.failure = None;
                            this.dot_action(id, DotAction::AskClaude, cx);
                        })),
                )
                .child(
                    Button::new("terminal-failure-close")
                        .ghost()
                        .xsmall()
                        .icon(Icon::new(IconName::X))
                        .tooltip(strings.terminal_dismiss)
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.failure = None;
                            cx.notify();
                        })),
                )
                .into_any_element(),
        )
    }

    /// The menu of a command's dot: how it went, and what to do with it.
    fn render_dot_menu(&self, cx: &mut Context<Self>) -> Option<gpui::AnyElement> {
        let menu = self.dot_menu.as_ref()?;
        let command = self
            .terminal
            .commands()
            .into_iter()
            .find(|command| command.id == menu.id)?;
        let strings = self.strings;
        let theme = cx.theme();
        let state = match command.state()? {
            CommandState::Succeeded => strings.terminal_command_ok.to_owned(),
            CommandState::Failed(code) => fill_text(
                strings.terminal_command_failed,
                &[("code", &code.to_string())],
            ),
            CommandState::Running => strings.terminal_command_running.to_owned(),
            CommandState::Ended => strings.terminal_command_ended.to_owned(),
        };
        let mut summary = vec![state];
        if let Some(started) = command.started {
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|time| time.as_secs() as i64)
                .unwrap_or_default();
            summary.push(ago(now - started.elapsed().as_secs() as i64, now, strings));
        }
        if let Some(duration) = command.duration {
            summary.push(duration_text(duration));
        }
        let id = command.id;
        let item = |key: &'static str, icon: IconName, label: &'static str, action: DotAction| {
            div()
                .id((key, id))
                .px_2()
                .py(px(5.))
                .rounded(px(5.))
                .flex()
                .items_center()
                .gap_2()
                .text_sm()
                .cursor_pointer()
                .hover(|row| row.bg(theme.secondary))
                .on_click(cx.listener(move |this, _, _, cx| this.dot_action(id, action, cx)))
                .child(Icon::new(icon).xsmall().text_color(theme.muted_foreground))
                .child(label)
        };
        let metrics = self.metrics.get()?;
        // Under the dot, over everything else: a low panel does not cut it,
        // and near the window's edge it moves to fit.
        let at = point(
            metrics.origin.x - px(10.),
            metrics.origin.y + metrics.line_height * (menu.row as f32 + 1.) + px(2.),
        );
        let has_text = command.text.is_some();
        let menu_box = div().id("terminal-dot-menu").w(px(280.));
        Some(
            gpui::deferred(
                gpui::anchored()
                    .position(at)
                    .snap_to_window_with_margin(px(8.))
                    .child(
                        menu_box
                            .p_1()
                            .flex()
                            .flex_col()
                            .rounded(px(8.))
                            .border_1()
                            .border_color(theme.border)
                            .bg(theme.popover)
                            .shadow_lg()
                            .occlude()
                            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                            .child(
                                div()
                                    .px_2()
                                    .pt_1()
                                    .text_xs()
                                    .truncate()
                                    .text_color(theme.muted_foreground)
                                    .child(summary.join(" · ")),
                            )
                            .children(command.text.clone().map(|text| {
                                div()
                                    .px_2()
                                    .pb_1()
                                    .text_sm()
                                    .truncate()
                                    .font_family(self.font_family.clone())
                                    .child(text)
                            }))
                            .when(has_text, |menu_box| {
                                menu_box.child(item(
                                    "dot-copy-command",
                                    IconName::Copy,
                                    strings.terminal_copy_command,
                                    DotAction::CopyCommand,
                                ))
                            })
                            .child(item(
                                "dot-copy-output",
                                IconName::Copy,
                                strings.terminal_copy_output,
                                DotAction::CopyOutput,
                            ))
                            .when(has_text, |menu_box| {
                                menu_box.child(item(
                                    "dot-rerun",
                                    IconName::RotateCcw,
                                    strings.terminal_rerun,
                                    DotAction::Rerun,
                                ))
                            })
                            .child(item(
                                "dot-ask-claude",
                                IconName::Sparkles,
                                strings.terminal_ask_claude,
                                DotAction::AskClaude,
                            )),
                    ),
            )
            .with_priority(1)
            .into_any_element(),
        )
    }
}

/// The colors of the dots by the commands, as VS Code's: blue when it
/// went well, red when it failed, a ring while it runs.
#[derive(Clone, Copy)]
struct DotColors {
    ok: Hsla,
    failed: Hsla,
    running: Hsla,
}

impl DotColors {
    fn new(cx: &App) -> Self {
        let theme = cx.theme();
        Self {
            ok: theme.info.opacity(0.85),
            failed: theme.danger,
            running: theme.muted_foreground,
        }
    }

    fn of(self, state: Option<CommandState>) -> Option<Hsla> {
        match state? {
            CommandState::Succeeded | CommandState::Ended => Some(self.ok),
            CommandState::Failed(_) => Some(self.failed),
            CommandState::Running => Some(self.running),
        }
    }
}

/// A dot as an element, for the command at the top.
fn dot(state: Option<CommandState>, dots: DotColors) -> gpui::Div {
    let color = dots.of(state).unwrap_or(dots.running);
    let size = px(7.);
    let dot = div().flex_none().size(size).rounded_full();
    if state == Some(CommandState::Running) || state.is_none() {
        dot.border_1().border_color(color)
    } else {
        dot.bg(color)
    }
}

/// A command line cut to fit a button, with an ellipsis.
fn short(text: &str, most: usize) -> String {
    let text = text.trim();
    if text.chars().count() <= most {
        return text.to_owned();
    }
    let cut: String = text.chars().take(most.saturating_sub(1)).collect();
    format!("{cut}…")
}

/// The end of a long output, which says what went wrong: its last lines.
fn tail(text: &str) -> String {
    const LINES: usize = 120;
    const CHARS: usize = 8000;
    let lines: Vec<&str> = text.lines().collect();
    let start = lines.len().saturating_sub(LINES);
    let mut tail = lines[start..].join("\n");
    let mut cut = start > 0;
    if tail.len() > CHARS {
        let from = (tail.len() - CHARS..tail.len())
            .find(|&index| tail.is_char_boundary(index))
            .unwrap_or(0);
        tail = tail[from..].to_owned();
        cut = true;
    }
    if cut { format!("…\n{tail}") } else { tail }
}

/// A moment as the day and the time where Keel runs: `19/11/2026 14:32`.
pub(crate) fn local_time_text(at: Option<SystemTime>) -> String {
    let seconds = at
        .unwrap_or_else(SystemTime::now)
        .duration_since(UNIX_EPOCH)
        .map(|time| time.as_secs() as libc::time_t)
        .unwrap_or_default();
    // SAFETY: localtime_r fills the struct given; nothing is kept.
    let local = unsafe {
        let mut local: libc::tm = std::mem::zeroed();
        libc::localtime_r(&seconds, &mut local);
        local
    };
    format!(
        "{:02}/{:02}/{} {:02}:{:02}",
        local.tm_mday,
        local.tm_mon + 1,
        local.tm_year + 1900,
        local.tm_hour,
        local.tm_min
    )
}

/// How long a command took: `850 ms`, `12 s`, `1 min 12 s`, `2 h 5 min`.
pub(crate) fn duration_text(duration: Duration) -> String {
    let seconds = duration.as_secs();
    if seconds < 1 {
        format!("{} ms", duration.as_millis())
    } else if seconds < 60 {
        format!("{seconds} s")
    } else if seconds < 3600 {
        format!("{} min {} s", seconds / 60, seconds % 60)
    } else {
        format!("{} h {} min", seconds / 3600, seconds % 3600 / 60)
    }
}

/// What the drawing needs besides the screen.
struct PaintStyle {
    font: Font,
    font_size: Pixels,
    palette: Palette,
    selection: Hsla,
    found: Hsla,
    found_current: Hsla,
    focused: bool,
    dots: DotColors,
    /// The prompt Cmd+↑ went to, and how strongly it is still lit.
    flash: Option<(u64, f32)>,
    flash_color: Hsla,
}

/// The Nerd Fonts installed, for the symbols themes such as Powerlevel10k
/// and Starship draw in the prompt, which Menlo and SF Mono do not have.
fn symbol_fonts(window: &Window) -> Option<gpui::FontFallbacks> {
    static FONTS: std::sync::OnceLock<Vec<String>> = std::sync::OnceLock::new();
    let fonts = FONTS.get_or_init(|| {
        let mut fonts: Vec<String> = window
            .text_system()
            .all_font_names()
            .into_iter()
            .filter(|name| {
                name.contains("Nerd Font") || name.ends_with(" NF") || name.contains("Powerline")
            })
            .collect();
        // The ones made for terminals, one cell wide, first.
        fonts.sort_by_key(|name| !(name.contains("Mono") || name.ends_with(" NF")));
        fonts
    });
    (!fonts.is_empty()).then(|| gpui::FontFallbacks::from_fonts(fonts.clone()))
}

fn measure(style: &PaintStyle, origin: Point<Pixels>, window: &mut Window) -> Metrics {
    let text_system = window.text_system();
    let font_id = text_system.resolve_font(&style.font);
    let cell_width = text_system
        .advance(font_id, style.font_size, 'm')
        .map(|advance| advance.width)
        .unwrap_or(style.font_size * 0.6);
    Metrics {
        origin,
        cell_width,
        line_height: (style.font_size * 1.25).round(),
    }
}

/// The background a cell shows: a match, the selection, or its own color.
fn cell_background(cell: &CellView, style: &PaintStyle) -> Option<Hsla> {
    match cell.found {
        Found::Current => return Some(style.found_current),
        Found::Match => return Some(style.found),
        Found::None => {}
    }
    if cell.selected {
        return Some(style.selection);
    }
    cell.bg.map(to_hsla)
}

/// Text that looks the same, drawn in one go.
#[derive(PartialEq)]
struct TextStyle {
    fg: Rgb,
    bold: bool,
    italic: bool,
    underline: bool,
    strikeout: bool,
}

fn text_style(cell: &CellView) -> TextStyle {
    TextStyle {
        fg: cell.fg,
        bold: cell.bold,
        italic: cell.italic,
        underline: cell.underline,
        strikeout: cell.strikeout,
    }
}

fn paint_snapshot(
    snapshot: &Snapshot,
    metrics: Metrics,
    style: &PaintStyle,
    window: &mut Window,
    cx: &mut App,
) {
    let Metrics {
        origin,
        cell_width,
        line_height,
    } = metrics;
    let cell_origin = |line: usize, column: usize| {
        point(
            origin.x + cell_width * column as f32,
            origin.y + line_height * line as f32,
        )
    };

    let flashed = style.flash.and_then(|(id, strength)| {
        snapshot
            .prompts
            .iter()
            .find(|prompt| prompt.id == id)
            .map(|prompt| (prompt.row, strength))
    });
    for (line, row) in snapshot.rows.iter().enumerate() {
        if let Some((_, strength)) = flashed.filter(|(row, _)| *row == line) {
            let band = Bounds::new(
                point(origin.x - px(12.), origin.y + line_height * line as f32),
                size(cell_width * row.len() as f32 + px(12.), line_height),
            );
            window.paint_quad(fill(band, style.flash_color.opacity(strength)));
        }
        // Backgrounds, joined while the color stays the same.
        let mut column = 0;
        while column < row.len() {
            let Some(color) = cell_background(&row[column], style) else {
                column += 1;
                continue;
            };
            let start = column;
            while column < row.len() && cell_background(&row[column], style) == Some(color) {
                column += 1;
            }
            let bounds = Bounds::new(
                cell_origin(line, start),
                size(cell_width * (column - start) as f32, line_height),
            );
            window.paint_quad(fill(bounds, color));
        }

        // Text, in runs of cells that look the same. A wide character is
        // drawn on its own, so the cells after it stay on the grid.
        let mut column = 0;
        while column < row.len() {
            let cell = &row[column];
            if cell.spacer || (cell.char == ' ' && !cell.underline && !cell.strikeout) {
                column += 1;
                continue;
            }
            let start = column;
            let look = text_style(cell);
            let mut text = String::new();
            loop {
                let cell = &row[column];
                text.push(cell.char);
                if let Some(extra) = &cell.zerowidth {
                    text.push_str(extra);
                }
                column += 1;
                if cell.wide || column >= row.len() {
                    break;
                }
                let next = &row[column];
                if next.spacer || next.wide || text_style(next) != look {
                    break;
                }
            }
            paint_text(
                &text,
                &look,
                cell_origin(line, start),
                metrics,
                style,
                window,
                cx,
            );
        }
    }

    // Each command's dot in the margin, by how it went.
    for prompt in &snapshot.prompts {
        let Some(color) = style.dots.of(prompt.state) else {
            continue;
        };
        let diameter = px(7.);
        let middle = origin.y + line_height * (prompt.row as f32 + 0.5);
        let bounds = Bounds::new(
            point(origin.x - px(9.5), middle - diameter / 2.),
            size(diameter, diameter),
        );
        let quad = if prompt.state == Some(CommandState::Running) {
            outline(bounds, color, gpui::BorderStyle::Solid)
        } else {
            fill(bounds, color)
        };
        window.paint_quad(quad.corner_radii(diameter / 2.));
    }

    let Some(cursor) = snapshot.cursor else {
        return;
    };
    let cursor_color = to_hsla(style.palette.cursor);
    let at = cell_origin(cursor.line, cursor.column);
    let cell = snapshot
        .rows
        .get(cursor.line)
        .and_then(|row| row.get(cursor.column));
    let width = if cell.is_some_and(|cell| cell.wide) {
        cell_width * 2.
    } else {
        cell_width
    };
    let block = Bounds::new(at, size(width, line_height));
    match (cursor.shape, style.focused) {
        (CursorShape::Block, true) => {
            window.paint_quad(fill(block, cursor_color));
            // The letter under the cursor, in the background's color.
            if let Some(cell) = cell.filter(|cell| cell.char != ' ') {
                let look = TextStyle {
                    fg: style.palette.background,
                    ..text_style(cell)
                };
                paint_text(
                    &cell.char.to_string(),
                    &look,
                    at,
                    metrics,
                    style,
                    window,
                    cx,
                );
            }
        }
        (CursorShape::Beam, _) => {
            window.paint_quad(fill(
                Bounds::new(at, size(px(2.), line_height)),
                cursor_color,
            ));
        }
        (CursorShape::Underline, _) => {
            let bar = Bounds::new(
                point(at.x, at.y + line_height - px(2.)),
                size(width, px(2.)),
            );
            window.paint_quad(fill(bar, cursor_color));
        }
        (CursorShape::Hollow, _) | (CursorShape::Block, false) => {
            window.paint_quad(outline(block, cursor_color, gpui::BorderStyle::Solid));
        }
    }
}

fn paint_text(
    text: &str,
    look: &TextStyle,
    origin: Point<Pixels>,
    metrics: Metrics,
    style: &PaintStyle,
    window: &mut Window,
    cx: &mut App,
) {
    let color = to_hsla(look.fg);
    let font = Font {
        weight: if look.bold {
            FontWeight::BOLD
        } else {
            FontWeight::NORMAL
        },
        style: if look.italic {
            FontStyle::Italic
        } else {
            FontStyle::Normal
        },
        ..style.font.clone()
    };
    let run = TextRun {
        len: text.len(),
        font,
        color,
        background_color: None,
        underline: look.underline.then(|| UnderlineStyle {
            thickness: px(1.),
            color: Some(color),
            wavy: false,
        }),
        strikethrough: look.strikeout.then(|| StrikethroughStyle {
            thickness: px(1.),
            color: Some(color),
        }),
    };
    let shaped = window.text_system().shape_line(
        SharedString::from(text.to_owned()),
        style.font_size,
        &[run],
        Some(metrics.cell_width),
    );
    let _ = shaped.paint(
        origin,
        metrics.line_height,
        TextAlign::Left,
        None,
        window,
        cx,
    );
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use gpui::{Entity, TestAppContext, VisualTestContext};

    use super::*;

    fn terminal<'a>(
        cx: &'a mut TestAppContext,
        script: &str,
    ) -> (Entity<TerminalView>, &'a mut VisualTestContext) {
        cx.executor().allow_parking();
        cx.update(|cx| {
            gpui_component::init(cx);
            crate::workbench_window::init(cx);
        });
        let script = script.to_owned();
        let (view, cx) = cx.add_window_view(|window, cx| {
            let mut spawn = shell_spawn(std::env::temp_dir(), cx);
            spawn.command = Some(("/bin/sh".into(), vec!["-c".into(), script]));
            let look = TerminalLook {
                font_family: "Menlo".into(),
                font_size: 12.,
                option_is_meta: false,
                strings: &crate::i18n::ENGLISH,
            };
            TerminalView::new(start(spawn).unwrap(), look, window, cx)
        });
        view.update_in(cx, |view, window, cx| view.focus_handle.focus(window, cx));
        (view, cx)
    }

    #[gpui::test]
    fn cmd_and_the_wheel_change_the_text_size(cx: &mut TestAppContext) {
        let (view, cx) = terminal(cx, "sleep 30");
        view.update(cx, |view, cx| {
            // Less than a point waits; a whole one changes the size.
            view.zoom_by(0.6, cx);
            assert_eq!(view.font_size(), 12.);
            view.zoom_by(0.6, cx);
            assert_eq!(view.font_size(), 13.);
            // The 0.2 left over counts with the next scroll.
            view.zoom_by(-3., cx);
            assert_eq!(view.font_size(), 11.);
            // Never below 8 or above 40.
            view.zoom_by(-20., cx);
            assert_eq!(view.font_size(), 8.);
            view.zoom_by(100., cx);
            assert_eq!(view.font_size(), 40.);
        });
    }

    fn wait_until(cx: &mut VisualTestContext, check: impl Fn(&mut VisualTestContext) -> bool) {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            cx.run_until_parked();
            if check(cx) {
                return;
            }
            assert!(Instant::now() < deadline, "gave up waiting");
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    fn current_match_line(
        view: &Entity<TerminalView>,
        cx: &mut VisualTestContext,
    ) -> Option<String> {
        view.read_with(cx, |view, _| {
            let snapshot = view.terminal.snapshot();
            (0..snapshot.lines)
                .find(|&line| {
                    snapshot.rows[line]
                        .iter()
                        .any(|cell| cell.found == Found::Current)
                })
                .map(|line| snapshot.line_text(line))
        })
    }

    /// zsh with Keel's shell integration, in a home of its own.
    fn zsh<'a>(
        cx: &'a mut TestAppContext,
        home: &std::path::Path,
    ) -> (Entity<TerminalView>, &'a mut VisualTestContext) {
        cx.executor().allow_parking();
        cx.update(|cx| {
            gpui_component::init(cx);
            crate::workbench_window::init(cx);
        });
        std::fs::write(home.join(".zshrc"), "PS1='$ '\n").unwrap();
        let home = home.to_path_buf();
        let (view, cx) = cx.add_window_view(|window, cx| {
            let mut spawn = shell_spawn(home.clone(), cx);
            spawn.command = Some(("/bin/zsh".into(), vec!["-i".into()]));
            spawn
                .env
                .insert("HOME".into(), home.to_string_lossy().into_owned());
            spawn.integrate_zsh(&home).unwrap();
            let look = TerminalLook {
                font_family: "Menlo".into(),
                font_size: 12.,
                option_is_meta: false,
                strings: &crate::i18n::ENGLISH,
            };
            TerminalView::new(start(spawn).unwrap(), look, window, cx)
        });
        view.update_in(cx, |view, window, cx| view.focus_handle.focus(window, cx));
        (view, cx)
    }

    fn at_prompt(view: &Entity<TerminalView>, cx: &mut VisualTestContext) {
        wait_until(cx, |cx| {
            view.read_with(cx, |view, _| {
                let snapshot = view.terminal.snapshot();
                snapshot
                    .cursor
                    .is_some_and(|cursor| snapshot.line_text(cursor.line) == "$")
            })
        });
    }

    #[gpui::test]
    fn a_failed_command_gets_its_fix_its_dot_and_a_question_for_claude(cx: &mut TestAppContext) {
        let home = tempfile::tempdir().unwrap();
        let (view, cx) = zsh(cx, home.path());
        at_prompt(&view, cx);
        view.read_with(cx, |view, _| {
            view.terminal.write(
                b"echo 'Error: listen EADDRINUSE: address already in use :::3999'; false\r"
                    .to_vec(),
            )
        });
        wait_until(cx, |cx| {
            view.read_with(cx, |view, _| view.failure.is_some())
        });
        let (fix, failed, prompt, id) = view.read_with(cx, |view, _| {
            let failure = view.failure.as_ref().unwrap();
            (
                failure.fix.clone(),
                view.status().failed,
                view.claude_prompt().unwrap(),
                failure.command.id,
            )
        });
        assert_eq!(fix, Some(QuickFix::FreePort { port: 3999 }));
        assert!(failed);
        assert!(
            prompt.contains("`echo 'Error: listen EADDRINUSE"),
            "{prompt}"
        );
        assert!(
            prompt.contains("(exit code 1)") && prompt.contains("\n```\nError: listen"),
            "{prompt}"
        );

        // Its dot is red, and Cmd+↑ goes to it and lights it.
        at_prompt(&view, cx);
        view.update(cx, |view, cx| view.jump_to_command(true, cx));
        view.read_with(cx, |view, _| {
            assert_eq!(view.flash.map(|(flashed, _)| flashed), Some(id));
            let prompts = view.terminal.snapshot().prompts;
            let prompt = prompts.iter().find(|prompt| prompt.id == id).unwrap();
            assert_eq!(prompt.state, Some(CommandState::Failed(1)));
        });

        // The next command takes the bar away.
        view.read_with(cx, |view, _| view.terminal.write(b"true\r".to_vec()));
        wait_until(cx, |cx| {
            view.read_with(cx, |view, _| view.failure.is_none())
        });
    }

    #[gpui::test]
    fn cmd_f_finds_in_the_output_and_escape_goes_back_to_the_shell(cx: &mut TestAppContext) {
        let (view, cx) = terminal(
            cx,
            "for i in $(seq 1 40); do echo riga$i; done; echo fine; sleep 30",
        );
        wait_until(cx, |cx| {
            view.read_with(cx, |view, _| {
                view.terminal.snapshot().text().contains("fine")
            })
        });

        cx.simulate_keystrokes("cmd-f");
        assert!(view.read_with(cx, |view, _| view.find.is_some()));
        cx.simulate_input("riga2");
        cx.run_until_parked();
        // The newest match first, then older ones with Enter.
        assert_eq!(current_match_line(&view, cx).as_deref(), Some("riga29"));
        cx.simulate_keystrokes("enter");
        cx.run_until_parked();
        assert_eq!(current_match_line(&view, cx).as_deref(), Some("riga28"));
        cx.simulate_keystrokes("shift-enter");
        cx.run_until_parked();
        assert_eq!(current_match_line(&view, cx).as_deref(), Some("riga29"));

        cx.simulate_keystrokes("escape");
        let (open, focused) = view.update_in(cx, |view, window, _| {
            (view.find.is_some(), view.focus_handle.is_focused(window))
        });
        assert!(!open && focused);
        assert_eq!(current_match_line(&view, cx), None);
    }
}

/// A path as a shell reads it: as it is when it holds only safe
/// characters, else in single quotes.
pub(super) fn shell_quote(path: &str) -> String {
    let safe = path
        .chars()
        .all(|c| c.is_alphanumeric() || "/._-+@%:,=".contains(c));
    if safe && !path.is_empty() {
        path.to_owned()
    } else {
        format!("'{}'", path.replace('\'', "'\\''"))
    }
}

#[cfg(test)]
mod quote_tests {
    #[test]
    fn paths_are_quoted_when_needed() {
        assert_eq!(
            super::shell_quote("/Users/me/app/a.ts"),
            "/Users/me/app/a.ts"
        );
        assert_eq!(
            super::shell_quote("/Users/me/My App/a.ts"),
            "'/Users/me/My App/a.ts'"
        );
        assert_eq!(super::shell_quote("/tmp/it's"), "'/tmp/it'\\''s'");
    }
}
