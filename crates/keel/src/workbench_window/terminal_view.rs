//! A terminal drawn in the window: the cells of its screen, the cursor,
//! the selection, and keys, mouse and clipboard sent to the shell.

use std::cell::Cell;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;

use futures::StreamExt as _;
use gpui::prelude::FluentBuilder as _;
use gpui::{
    App, AppContext as _, Bounds, ClipboardEntry, ClipboardItem, Context, EventEmitter,
    FocusHandle, Focusable, Font, FontStyle, FontWeight, Hsla, InteractiveElement as _,
    IntoElement, KeyBinding, KeyDownEvent, MouseButton, MouseDownEvent, MouseMoveEvent,
    MouseUpEvent, NoAction, ParentElement as _, Pixels, Point, Render, Rgba, ScrollEvent,
    SharedString, StrikethroughStyle, Styled as _, Task, TextAlign, TextRun, UnderlineStyle,
    Window, actions, canvas, div, fill, outline, point, px, size,
};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::input::{Input, InputEvent, InputState};
use gpui_component::{ActiveTheme as _, Icon, Selectable as _, Sizable as _};
use gpui_kit_assets::IconName;

use crate::i18n::Strings;
use terminal::{
    CellPoint, CellView, CursorShape, Found, Key, LinkTarget, Modifiers, MouseAction, Palette, Rgb,
    SelectionKind, Side, Snapshot, Spawn, TermMode, Terminal, TerminalEvent,
};

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
    /// A drag that selects text, rather than going to the program.
    selecting: bool,
    exited: bool,
    find: Option<FindBar>,
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
    Spawn::shell(cwd, size, palette(cx), &language)
}

/// Starts a terminal; tests run a script instead of the login shell.
pub(crate) fn start(spawn: Spawn) -> std::io::Result<Started> {
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
            selecting: false,
            exited: false,
            find: None,
            strings: look.strings,
            _events: pump,
            _subscriptions: subscriptions,
        }
    }

    #[cfg(test)]
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

    /// The folder of the program in front, which follows `cd`.
    pub(crate) fn current_dir(&self) -> PathBuf {
        self.terminal
            .current_dir()
            .unwrap_or_else(|| self.cwd.clone())
    }

    fn handle_events(&mut self, events: Vec<TerminalEvent>, cx: &mut Context<Self>) {
        for event in events {
            match event {
                TerminalEvent::Changed | TerminalEvent::Bell => {}
                TerminalEvent::Title(title) => {
                    self.title = title.filter(|title| !title.trim().is_empty());
                    cx.emit(TerminalViewEvent::TitleChanged);
                }
                TerminalEvent::Copy(text) => cx.write_to_clipboard(ClipboardItem::new_string(text)),
                TerminalEvent::Exited(_) => {
                    self.exited = true;
                    cx.emit(TerminalViewEvent::Exited);
                }
            }
        }
        cx.notify();
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
                .xsmall()
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
                        .xsmall()
                        .icon(Icon::new(IconName::ArrowUp))
                        .tooltip(strings.find_older)
                        .on_click(cx.listener(|this, _, _, cx| this.find_again(Some(true), cx))),
                )
                .child(
                    Button::new("terminal-find-newer")
                        .ghost()
                        .xsmall()
                        .icon(Icon::new(IconName::ArrowDown))
                        .tooltip(strings.find_newer)
                        .on_click(cx.listener(|this, _, _, cx| this.find_again(Some(false), cx))),
                )
                .child(
                    Button::new("terminal-find-close")
                        .ghost()
                        .xsmall()
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

    fn scroll(&mut self, event: &ScrollEvent, _: &mut Window, cx: &mut Context<Self>) {
        let Some(metrics) = self.metrics.get() else {
            return;
        };
        let delta = event.delta.pixel_delta(metrics.line_height).y;
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
        let style = PaintStyle {
            font: gpui::font(self.font_family.clone()),
            font_size: self.font_size,
            palette,
            selection: theme.selection,
            found: theme.warning.opacity(0.35),
            found_current: theme.warning.opacity(0.8),
            focused: self.focus_handle.is_focused(window),
        };
        let terminal = self.terminal.clone();
        let metrics = self.metrics.clone();
        let grid_size = self.grid_size.clone();
        let find = self.render_find(cx);
        div()
            .id("terminal")
            .relative()
            .key_context(TERMINAL_CONTEXT)
            .track_focus(&self.focus_handle)
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
                        (snapshot, measured, style)
                    },
                    move |_bounds, (snapshot, measured, style), window, cx| {
                        paint_snapshot(&snapshot, measured, &style, window, cx);
                    },
                )
                .size_full(),
            )
            .children(find)
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

    for (line, row) in snapshot.rows.iter().enumerate() {
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
