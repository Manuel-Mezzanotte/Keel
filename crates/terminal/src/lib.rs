//! Terminals: a shell in a pseudo-terminal, the screen its programs draw,
//! and the bytes keys, the mouse and pasted text send to them. The
//! emulation is Alacritty's; the window draws a [`Snapshot`].

mod commands;
mod event_loop;
mod input;
mod links;
mod marks;
mod palette;
mod ports;
mod process;
mod shell_integration;

use std::borrow::Cow;
use std::collections::HashMap;
use std::fs::File;
use std::io;
use std::os::fd::AsRawFd;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};

use alacritty_terminal::event::{Event, EventListener, WindowSize};
use alacritty_terminal::event_loop::Msg;
use alacritty_terminal::grid::{Dimensions, Scroll};
use alacritty_terminal::index::{Boundary, Column, Direction, Line, Point};
use alacritty_terminal::selection::{Selection, SelectionType};
use alacritty_terminal::sync::FairMutex;
use alacritty_terminal::term::cell::{Cell, Flags};
use alacritty_terminal::term::search::{Match, RegexIter, RegexSearch};
use alacritty_terminal::term::{Config, Term, point_to_viewport, viewport_to_point};
use alacritty_terminal::tty;
use alacritty_terminal::vte::ansi::{self, CursorShape as AlacrittyCursor};

pub use alacritty_terminal::index::Side;
pub use alacritty_terminal::term::TermMode;
pub use commands::{CommandState, PromptMark, ShellCommand, Spot, StickyCommand};
pub use input::{
    Key, Modifiers, MouseAction, MouseButton, focus_bytes, key_bytes, mouse_bytes, paste_bytes,
};
pub use links::{Link, LinkTarget, find_links};
pub use marks::Progress;
pub use palette::{Palette, Rgb};
pub use ports::{ListeningPort, listening_ports};

/// The lines a terminal keeps above the screen, as Alacritty does.
pub const SCROLLBACK: usize = 10_000;

/// The size of a terminal in cells, and of a cell in pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Size {
    pub columns: u16,
    pub lines: u16,
    pub cell_width: u16,
    pub cell_height: u16,
}

impl Size {
    fn window_size(self) -> WindowSize {
        WindowSize {
            num_lines: self.lines,
            num_cols: self.columns,
            cell_width: self.cell_width,
            cell_height: self.cell_height,
        }
    }
}

impl Dimensions for Size {
    fn total_lines(&self) -> usize {
        self.lines as usize
    }

    fn screen_lines(&self) -> usize {
        self.lines as usize
    }

    fn columns(&self) -> usize {
        self.columns as usize
    }
}

/// What happened in a terminal. Sent from the terminal's own thread.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TerminalEvent {
    /// The screen changed and needs drawing.
    Changed,
    /// A program set the title, or reset it with `None`.
    Title(Option<String>),
    Bell,
    /// A program asked to copy text (OSC 52).
    Copy(String),
    /// The shell ended, with its exit code when it has one.
    Exited(Option<i32>),
    /// The shell started running a command (shell integration).
    CommandStarted(ShellCommand),
    /// A command ended, with its exit code when the shell said it.
    CommandFinished(ShellCommand),
    /// A program reported how far it is (OSC 9;4), or stopped saying.
    Progress(Option<Progress>),
}

/// How to start a terminal.
#[derive(Clone, Debug)]
pub struct Spawn {
    pub cwd: PathBuf,
    /// The program and its arguments; `None` starts the user's shell as a
    /// login shell, as the Terminal of macOS does.
    pub command: Option<(String, Vec<String>)>,
    pub env: HashMap<String, String>,
    pub size: Size,
    pub palette: Palette,
    /// The nonce the shell integration proves command lines with.
    pub nonce: Option<String>,
    /// What the terminal showed before Keel quit, written back first.
    pub restore: Option<Vec<u8>>,
}

impl Spawn {
    /// The user's shell in a folder, with the variables programs look at
    /// to know what the terminal can do.
    pub fn shell(cwd: PathBuf, size: Size, palette: Palette, language: &str) -> Self {
        let mut env = HashMap::new();
        env.insert("TERM".to_owned(), "xterm-256color".to_owned());
        env.insert("COLORTERM".to_owned(), "truecolor".to_owned());
        env.insert("TERM_PROGRAM".to_owned(), "Keel".to_owned());
        env.insert(
            "TERM_PROGRAM_VERSION".to_owned(),
            env!("CARGO_PKG_VERSION").to_owned(),
        );
        // Apps opened from the Finder have no LANG, and without it zsh
        // shows accented letters as codes.
        if std::env::var_os("LANG").is_none() {
            env.insert("LANG".to_owned(), language.to_owned());
        }
        Self {
            cwd,
            command: None,
            env,
            size,
            palette,
            nonce: None,
            restore: None,
        }
    }

    /// Marks each prompt, command and output when the user's shell is zsh,
    /// as VS Code's shell integration does: the dots by the commands, the
    /// jumps between them and their output come from these marks.
    pub fn integrate_shell(&mut self) {
        if self.command.is_some()
            || !shell_integration::login_shell().is_some_and(|shell| shell.ends_with("/zsh"))
        {
            return;
        }
        let user = std::env::var_os("ZDOTDIR")
            .filter(|folder| !folder.is_empty())
            .map(PathBuf::from)
            .or_else(std::env::home_dir);
        if let Some(user) = user
            && let Err(error) = self.integrate_zsh(&user)
        {
            eprintln!("keel: no shell integration: {error}");
        }
    }

    /// zsh with Keel's integration, reading the user's own files from
    /// `user_zdotdir`.
    pub fn integrate_zsh(&mut self, user_zdotdir: &Path) -> io::Result<()> {
        let folder = shell_integration::zsh_folder()?;
        let nonce = shell_integration::nonce();
        let folder = folder.to_string_lossy().into_owned();
        self.env.insert("ZDOTDIR".to_owned(), folder.clone());
        self.env.insert("KEEL_ZDOTDIR".to_owned(), folder);
        self.env.insert(
            "KEEL_USER_ZDOTDIR".to_owned(),
            user_zdotdir.to_string_lossy().into_owned(),
        );
        self.env.insert("KEEL_NONCE".to_owned(), nonce.clone());
        self.nonce = Some(nonce);
        Ok(())
    }
}

/// The LANG for a language such as `it-IT`: `it_IT.UTF-8`.
pub fn lang_for(locale: Option<&str>) -> String {
    let locale = locale.unwrap_or("en-US").replace('-', "_");
    let locale = if locale.contains('_') {
        locale
    } else {
        format!("{locale}_{}", locale.to_uppercase())
    };
    format!("{locale}.UTF-8")
}

struct Shared {
    sender: OnceLock<event_loop::LoopSender>,
    size: Mutex<Size>,
    palette: Mutex<Palette>,
    exit_code: Mutex<Option<i32>>,
    /// The commands the shell ran. Taken while the screen is locked, never
    /// the other way round.
    tracker: Mutex<commands::Tracker>,
    on_event: Box<dyn Fn(TerminalEvent) + Send + Sync>,
}

impl Shared {
    fn tracker(&self) -> MutexGuard<'_, commands::Tracker> {
        self.tracker.lock().expect("tracker")
    }

    fn write(&self, bytes: Cow<'static, [u8]>) {
        if let Some(sender) = self.sender.get() {
            // The loop is gone once the shell ended; nothing to write to.
            let _ = sender.send(Msg::Input(bytes));
        }
    }
}

#[derive(Clone)]
struct Listener(Arc<Shared>);

impl EventListener for Listener {
    fn send_event(&self, event: Event) {
        let shared = &self.0;
        let event = match event {
            Event::Wakeup | Event::MouseCursorDirty | Event::CursorBlinkingChange => {
                TerminalEvent::Changed
            }
            Event::Title(title) => TerminalEvent::Title(Some(title)),
            Event::ResetTitle => TerminalEvent::Title(None),
            Event::Bell => TerminalEvent::Bell,
            Event::ClipboardStore(_, text) => TerminalEvent::Copy(text),
            // Programs never read the clipboard: a command in a file could
            // otherwise take a password copied a moment before.
            Event::ClipboardLoad(..) => return,
            Event::ColorRequest(index, format) => {
                let color = shared.palette.lock().expect("palette").indexed(index);
                shared.write(format(color.into()).into_bytes().into());
                return;
            }
            Event::TextAreaSizeRequest(format) => {
                let size = *shared.size.lock().expect("size");
                shared.write(format(size.window_size()).into_bytes().into());
                return;
            }
            Event::PtyWrite(text) => {
                shared.write(text.into_bytes().into());
                return;
            }
            Event::ChildExit(status) => {
                *shared.exit_code.lock().expect("exit code") = status.code();
                return;
            }
            Event::Exit => TerminalEvent::Exited(*shared.exit_code.lock().expect("exit code")),
        };
        (shared.on_event)(event);
    }
}

/// How a selection grows from where the mouse went down.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SelectionKind {
    /// Cell by cell: a drag.
    Simple,
    /// Word by word: a double click.
    Word,
    /// Line by line: a triple click.
    Line,
}

/// A place on the screen: the line from the top of what is shown, and the
/// column.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CellPoint {
    pub line: usize,
    pub column: usize,
}

struct Search {
    pattern: String,
    regex: RegexSearch,
    current: Option<Match>,
}

/// A running terminal. Dropping it ends the shell.
pub struct Terminal {
    term: Arc<FairMutex<Term<Listener>>>,
    shared: Arc<Shared>,
    pty: File,
    pid: u32,
    search: Mutex<Option<Search>>,
}

impl Terminal {
    /// Starts the program in a new pseudo-terminal. `on_event` is called
    /// from the terminal's thread.
    pub fn spawn(
        spawn: Spawn,
        on_event: impl Fn(TerminalEvent) + Send + Sync + 'static,
    ) -> io::Result<Self> {
        let mut tracker = commands::Tracker::default();
        tracker.nonce = spawn.nonce.clone();
        let shared = Arc::new(Shared {
            sender: OnceLock::new(),
            size: Mutex::new(spawn.size),
            palette: Mutex::new(spawn.palette),
            exit_code: Mutex::new(None),
            tracker: Mutex::new(tracker),
            on_event: Box::new(on_event),
        });
        let config = Config {
            scrolling_history: SCROLLBACK,
            ..Config::default()
        };
        let mut term = Term::new(config, &spawn.size, Listener(shared.clone()));
        if let Some(bytes) = &spawn.restore {
            let mut parser: ansi::Processor = ansi::Processor::new();
            parser.advance(&mut term, bytes);
        }
        let term = Arc::new(FairMutex::new(term));
        let options = tty::Options {
            shell: spawn
                .command
                .map(|(program, args)| tty::Shell::new(program, args)),
            working_directory: Some(spawn.cwd),
            drain_on_exit: true,
            env: spawn.env,
        };
        let pty = tty::new(&options, spawn.size.window_size(), 0)?;
        let pid = pty.child().id();
        let pty_file = pty.file().try_clone()?;
        let event_loop = event_loop::EventLoop::new(term.clone(), shared.clone(), pty)?;
        shared.sender.set(event_loop.channel()).ok();
        event_loop.spawn()?;
        Ok(Self {
            term,
            shared,
            pty: pty_file,
            pid,
            search: Mutex::new(None),
        })
    }

    /// The process the terminal started: `login`, which runs the shell.
    pub fn pid(&self) -> u32 {
        self.pid
    }

    /// Sends bytes to the program, as typed keys do. Typing brings the
    /// screen back to the bottom.
    pub fn write(&self, bytes: impl Into<Cow<'static, [u8]>>) {
        {
            let mut term = self.term.lock();
            if term.grid().display_offset() != 0 {
                term.scroll_display(Scroll::Bottom);
            }
            self.shared.tracker().jumped = None;
        }
        self.shared.write(bytes.into());
    }

    /// Pastes text the way the program asked for.
    pub fn paste(&self, text: &str) {
        let bytes = paste_bytes(text, self.mode());
        self.write(bytes);
    }

    pub fn size(&self) -> Size {
        *self.shared.size.lock().expect("size")
    }

    /// Fits the terminal to new room. Programs redraw for the new size.
    pub fn resize(&self, size: Size) {
        let size = Size {
            columns: size.columns.max(2),
            lines: size.lines.max(1),
            ..size
        };
        {
            let mut current = self.shared.size.lock().expect("size");
            if *current == size {
                return;
            }
            *current = size;
        }
        {
            let mut term = self.term.lock();
            self.shared
                .tracker()
                .resize(&mut term, |term| term.resize(size));
        }
        if let Some(sender) = self.shared.sender.get() {
            let _ = sender.send(Msg::Resize(size.window_size()));
        }
    }

    pub fn set_palette(&self, palette: Palette) {
        *self.shared.palette.lock().expect("palette") = palette;
    }

    pub fn mode(&self) -> TermMode {
        *self.term.lock().mode()
    }

    /// Scrolls through the lines above the screen: up with a positive
    /// number.
    pub fn scroll(&self, lines: i32) {
        self.term.lock().scroll_display(Scroll::Delta(lines));
    }

    pub fn scroll_to_bottom(&self) {
        self.term.lock().scroll_display(Scroll::Bottom);
    }

    /// Clears the screen and the lines above it, as Cmd+K does.
    pub fn clear(&self) {
        let mut term = self.term.lock();
        term.grid_mut().clear_history();
        term.selection = None;
        self.shared.tracker().clear();
        drop(term);
        // Ctrl+L asks the shell to redraw its prompt on a clean screen.
        self.shared.write(Cow::Borrowed(b"\x0c"));
    }

    /// The program in front: the shell, or the command it runs.
    pub fn foreground_name(&self) -> Option<String> {
        process::foreground(self.pty.as_raw_fd()).and_then(process::name)
    }

    /// The folder of the program in front, which follows `cd`.
    pub fn current_dir(&self) -> Option<PathBuf> {
        process::foreground(self.pty.as_raw_fd()).and_then(process::current_dir)
    }

    fn grid_point(term: &Term<Listener>, point: CellPoint) -> Point {
        let lines = term.screen_lines().saturating_sub(1);
        let columns = term.columns().saturating_sub(1);
        let point = Point::new(point.line.min(lines), Column(point.column.min(columns)));
        viewport_to_point(term.grid().display_offset(), point)
    }

    pub fn start_selection(&self, point: CellPoint, side: Side, kind: SelectionKind) {
        let mut term = self.term.lock();
        let point = Self::grid_point(&term, point);
        let kind = match kind {
            SelectionKind::Simple => SelectionType::Simple,
            SelectionKind::Word => SelectionType::Semantic,
            SelectionKind::Line => SelectionType::Lines,
        };
        term.selection = Some(Selection::new(kind, point, side));
    }

    pub fn extend_selection(&self, point: CellPoint, side: Side) {
        let mut term = self.term.lock();
        let point = Self::grid_point(&term, point);
        if let Some(selection) = term.selection.as_mut() {
            selection.update(point, side);
        }
    }

    pub fn select_all(&self) {
        let mut term = self.term.lock();
        let start = Point::new(term.topmost_line(), Column(0));
        let end = Point::new(term.bottommost_line(), term.last_column());
        let mut selection = Selection::new(SelectionType::Simple, start, Side::Left);
        selection.update(end, Side::Right);
        term.selection = Some(selection);
    }

    pub fn clear_selection(&self) {
        self.term.lock().selection = None;
    }

    /// The selected text, or `None` when nothing is selected.
    pub fn selected_text(&self) -> Option<String> {
        let term = self.term.lock();
        let selection = term.selection.as_ref()?;
        if selection.is_empty() {
            return None;
        }
        term.selection_to_string().filter(|text| !text.is_empty())
    }

    /// Finds the next match of a search, toward older output when
    /// `backward`, as the terminal of VS Code does with Enter. Shows it and
    /// returns whether there is one.
    pub fn find(&self, query: &str, regex: bool, case_sensitive: bool, backward: bool) -> bool {
        if query.is_empty() {
            self.stop_finding();
            return false;
        }
        let body = if regex {
            query.to_owned()
        } else {
            regex::escape(query)
        };
        let pattern = if case_sensitive {
            format!("(?-i){body}")
        } else {
            format!("(?i){body}")
        };
        let mut search = self.search.lock().expect("search");
        if search
            .as_ref()
            .is_none_or(|search| search.pattern != pattern)
        {
            let Ok(compiled) = RegexSearch::new(&pattern) else {
                *search = None;
                return false;
            };
            *search = Some(Search {
                pattern,
                regex: compiled,
                current: None,
            });
        }
        let search = search.as_mut().expect("just set");
        let mut term = self.term.lock();
        let direction = if backward {
            Direction::Left
        } else {
            Direction::Right
        };
        let origin = match &search.current {
            Some(current) if backward => current.start().sub(&*term, Boundary::None, 1),
            Some(current) => current.end().add(&*term, Boundary::None, 1),
            None if backward => Point::new(term.bottommost_line(), term.last_column()),
            None => Point::new(term.topmost_line(), Column(0)),
        };
        let found = term.search_next(&mut search.regex, origin, direction, Side::Left, None);
        if let Some(found) = &found {
            term.scroll_to_point(*found.start());
        }
        search.current = found;
        search.current.is_some()
    }

    /// Forgets the search, so the screen shows no matches.
    pub fn stop_finding(&self) {
        *self.search.lock().expect("search") = None;
    }

    /// What the screen shows now, with colors resolved from the palette.
    pub fn snapshot(&self) -> Snapshot {
        let palette = *self.shared.palette.lock().expect("palette");
        let mut search = self.search.lock().expect("search");
        let mut term = self.term.lock();
        let (prompts, sticky) = {
            let mut tracker = self.shared.tracker();
            tracker.catch_up(&mut term);
            tracker.on_screen(&term)
        };
        let content = term.renderable_content();
        let offset = content.display_offset;
        let columns = term.columns();
        let lines = term.screen_lines();

        // The matches on the screen, to mark them.
        let mut matches: Vec<(Match, bool)> = Vec::new();
        if let Some(search) = search.as_mut() {
            let top = viewport_to_point(offset, Point::new(0, Column(0)));
            let bottom = viewport_to_point(offset, Point::new(lines - 1, Column(columns - 1)));
            let current = search.current.clone();
            for found in
                RegexIter::new(top, bottom, Direction::Right, &term, &mut search.regex).take(1000)
            {
                let is_current = current.as_ref() == Some(&found);
                matches.push((found, is_current));
            }
        }

        let mut rows: Vec<Vec<CellView>> =
            (0..lines).map(|_| Vec::with_capacity(columns)).collect();
        for indexed in content.display_iter {
            let Some(viewport) = point_to_viewport(offset, indexed.point) else {
                continue;
            };
            let Some(row) = rows.get_mut(viewport.line) else {
                continue;
            };
            let cell = indexed.cell;
            let flags = cell.flags;
            let mut fg = palette.resolve(cell.fg, content.colors);
            let mut bg = palette.resolve(cell.bg, content.colors);
            let mut default_bg = palette::Palette::is_default_background(cell.bg);
            if flags.contains(Flags::DIM) {
                fg = fg.dim();
            }
            if flags.contains(Flags::INVERSE) {
                std::mem::swap(&mut fg, &mut bg);
                default_bg = false;
            }
            if flags.contains(Flags::HIDDEN) {
                fg = bg;
            }
            let found = matches
                .iter()
                .find(|(found, _)| found.contains(&indexed.point))
                .map(|(_, current)| {
                    if *current {
                        Found::Current
                    } else {
                        Found::Match
                    }
                })
                .unwrap_or(Found::None);
            row.push(CellView {
                char: cell.c,
                zerowidth: cell.zerowidth().map(|chars| chars.iter().collect()),
                fg,
                bg: (!default_bg).then_some(bg),
                bold: flags.contains(Flags::BOLD),
                italic: flags.contains(Flags::ITALIC),
                underline: flags.intersects(Flags::ALL_UNDERLINES),
                strikeout: flags.contains(Flags::STRIKEOUT),
                wide: flags.contains(Flags::WIDE_CHAR),
                spacer: flags.intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER),
                selected: content
                    .selection
                    .is_some_and(|selection| selection.contains(indexed.point)),
                found,
                link: cell
                    .hyperlink()
                    .map(|link| link.uri().to_owned())
                    .filter(|uri| !uri.starts_with(commands::TAG)),
            });
        }

        let cursor = point_to_viewport(offset, content.cursor.point).and_then(|point| {
            let shape = match content.cursor.shape {
                AlacrittyCursor::Hidden => return None,
                AlacrittyCursor::Block => CursorShape::Block,
                AlacrittyCursor::Underline => CursorShape::Underline,
                AlacrittyCursor::Beam => CursorShape::Beam,
                AlacrittyCursor::HollowBlock => CursorShape::Hollow,
            };
            (point.line < lines).then_some(Cursor {
                line: point.line,
                column: point.column.0,
                shape,
            })
        });

        Snapshot {
            rows,
            cursor,
            mode: content.mode,
            display_offset: offset,
            history_size: term.history_size(),
            columns,
            lines,
            prompts,
            sticky,
        }
    }

    // -----------------------------------------------------------------
    // Shell integration

    /// The commands the shell ran, oldest first, as far as they are kept.
    pub fn commands(&self) -> Vec<ShellCommand> {
        let _term = self.term.lock();
        self.shared.tracker().commands.iter().cloned().collect()
    }

    /// The newest command that ran.
    pub fn last_command(&self) -> Option<ShellCommand> {
        let _term = self.term.lock();
        self.shared
            .tracker()
            .commands
            .iter()
            .rev()
            .find(|command| command.output.is_some() || command.finished)
            .cloned()
    }

    /// What a command printed, while it is still kept.
    pub fn output_of(&self, id: u64) -> Option<String> {
        let mut term = self.term.lock();
        self.shared.tracker().output_of(&mut term, id)
    }

    /// Cmd+↑ and Cmd+↓: the prompt of the command before or after the one
    /// shown goes to the top of the screen; returns that command.
    pub fn scroll_to_command(&self, older: bool) -> Option<u64> {
        let mut term = self.term.lock();
        self.shared.tracker().scroll_to_command(&mut term, older)
    }

    /// Shows a command's prompt at the top of the screen.
    pub fn scroll_to(&self, id: u64) {
        let mut term = self.term.lock();
        self.shared.tracker().scroll_to(&mut term, id);
    }

    /// How far the program in front says it is.
    pub fn progress(&self) -> Option<Progress> {
        self.shared.tracker().progress
    }

    /// The shell's screen and the history above it, at most `lines` of
    /// them, as text with its colors: what [`Spawn::restore`] writes back
    /// after a restart. `None` while a full-screen program shows its own.
    pub fn contents(&self, lines: usize) -> Option<Vec<u8>> {
        let mut term = self.term.lock();
        if term.mode().contains(TermMode::ALT_SCREEN) {
            return None;
        }
        // The prompt the shell waits at comes back by itself.
        let waiting = {
            let mut tracker = self.shared.tracker();
            tracker.catch_up(&mut term);
            tracker
                .commands
                .back()
                .filter(|command| command.state().is_none())
                .and_then(|command| command.prompt)
                .and_then(|prompt| tracker.point_of(prompt, &term))
                .map(|point| point.line)
        };
        let cursor = term.grid().cursor.point.line;
        let mut last = match waiting {
            Some(prompt) => prompt - 1,
            None => cursor,
        };
        let top = term.topmost_line();
        let blank = |line: Line| {
            term.grid()[line].into_iter().all(|cell| {
                cell.c == ' ' && cell.bg == ansi::Color::Named(ansi::NamedColor::Background)
            })
        };
        while last >= top && blank(last) {
            last -= 1;
        }
        if last < top {
            return None;
        }
        let first = (last - (lines.max(1) as i32 - 1)).max(top);
        Some(serialize(&term, first, last).into_bytes())
    }
}

/// Lines of the screen as text and SGR colors, wrapped lines joined so
/// they wrap again at any width.
fn serialize(term: &Term<Listener>, first: Line, last: Line) -> String {
    let mut out = String::new();
    let mut style = String::new();
    let columns = term.columns();
    let mut line = first;
    while line <= last {
        let row = &term.grid()[line];
        let wraps = row[Column(columns - 1)].flags.contains(Flags::WRAPLINE);
        let end = if wraps {
            columns
        } else {
            (0..columns)
                .rev()
                .find(|&column| {
                    let cell = &row[Column(column)];
                    cell.c != ' '
                        || cell.bg != ansi::Color::Named(ansi::NamedColor::Background)
                        || cell
                            .flags
                            .intersects(Flags::INVERSE | Flags::ALL_UNDERLINES)
                })
                .map_or(0, |column| column + 1)
        };
        for column in 0..end {
            let cell = &row[Column(column)];
            if cell
                .flags
                .intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER)
            {
                continue;
            }
            let wanted = sgr(cell);
            if wanted != style {
                out.push_str(&wanted);
                style = wanted;
            }
            out.push(cell.c);
            if let Some(extra) = cell.zerowidth() {
                out.extend(extra.iter());
            }
        }
        if !wraps && line < last {
            out.push_str("\x1b[0m\r\n");
            style.clear();
        }
        line += 1;
    }
    out.push_str("\x1b[0m\r\n");
    out
}

/// The SGR sequence that gives a cell its look.
fn sgr(cell: &Cell) -> String {
    let mut codes = vec!["0".to_owned()];
    for (flag, code) in [
        (Flags::BOLD, "1"),
        (Flags::DIM, "2"),
        (Flags::ITALIC, "3"),
        (Flags::ALL_UNDERLINES, "4"),
        (Flags::INVERSE, "7"),
        (Flags::HIDDEN, "8"),
        (Flags::STRIKEOUT, "9"),
    ] {
        if cell.flags.intersects(flag) {
            codes.push(code.to_owned());
        }
    }
    codes.extend(color_code(cell.fg, true));
    codes.extend(color_code(cell.bg, false));
    format!("\x1b[{}m", codes.join(";"))
}

fn color_code(color: ansi::Color, foreground: bool) -> Option<String> {
    let (base, bright, extended) = if foreground {
        (30, 90, 38)
    } else {
        (40, 100, 48)
    };
    match color {
        ansi::Color::Named(named) => {
            let index = named as usize;
            let dim = ansi::NamedColor::DimBlack as usize;
            match index {
                0..=7 => Some((base + index).to_string()),
                8..=15 => Some((bright + index - 8).to_string()),
                _ if (dim..dim + 8).contains(&index) => Some((base + index - dim).to_string()),
                _ => None,
            }
        }
        ansi::Color::Indexed(index) => Some(format!("{extended};5;{index}")),
        ansi::Color::Spec(rgb) => Some(format!("{extended};2;{};{};{}", rgb.r, rgb.g, rgb.b)),
    }
}

impl Drop for Terminal {
    fn drop(&mut self) {
        if let Some(sender) = self.shared.sender.get() {
            let _ = sender.send(Msg::Shutdown);
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Found {
    None,
    Match,
    /// The match the search is on.
    Current,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CellView {
    pub char: char,
    /// Accents and joiners drawn on the same cell.
    pub zerowidth: Option<String>,
    pub fg: Rgb,
    /// `None` for the terminal's own background.
    pub bg: Option<Rgb>,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strikeout: bool,
    /// A character two cells wide; the next cell is its spacer.
    pub wide: bool,
    /// The second half of a wide character: nothing to draw.
    pub spacer: bool,
    pub selected: bool,
    pub found: Found,
    /// The address of a link a program printed (OSC 8).
    pub link: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CursorShape {
    Block,
    Underline,
    Beam,
    Hollow,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cursor {
    pub line: usize,
    pub column: usize,
    pub shape: CursorShape,
}

/// The screen of a terminal at one moment.
#[derive(Clone, Debug)]
pub struct Snapshot {
    pub rows: Vec<Vec<CellView>>,
    pub cursor: Option<Cursor>,
    pub mode: TermMode,
    /// How many lines the screen is scrolled up into the history.
    pub display_offset: usize,
    pub history_size: usize,
    pub columns: usize,
    pub lines: usize,
    /// The prompts on the screen (shell integration).
    pub prompts: Vec<PromptMark>,
    /// The command whose output fills the top of the screen.
    pub sticky: Option<StickyCommand>,
}

impl Snapshot {
    /// A line as text, one character per cell, without trailing spaces.
    pub fn line_text(&self, line: usize) -> String {
        let Some(row) = self.rows.get(line) else {
            return String::new();
        };
        let text: String = row
            .iter()
            .filter(|cell| !cell.spacer)
            .map(|cell| cell.char)
            .collect();
        text.trim_end().to_owned()
    }

    /// The whole screen as text.
    pub fn text(&self) -> String {
        (0..self.lines)
            .map(|line| self.line_text(line))
            .collect::<Vec<_>>()
            .join("\n")
            .trim_end()
            .to_owned()
    }

    /// The column of each character of [`Self::line_text`], so a link found
    /// in the text can be drawn on the right cells.
    pub fn line_columns(&self, line: usize) -> Vec<usize> {
        self.rows
            .get(line)
            .map(|row| {
                row.iter()
                    .enumerate()
                    .filter(|(_, cell)| !cell.spacer)
                    .map(|(column, _)| column)
                    .collect()
            })
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;
    use std::time::{Duration, Instant};

    use super::*;

    fn size(columns: u16, lines: u16) -> Size {
        Size {
            columns,
            lines,
            cell_width: 8,
            cell_height: 16,
        }
    }

    fn palette() -> Palette {
        Palette::dark(Rgb::hex(0xdddddd), Rgb::hex(0x101010))
    }

    /// Starts `sh -c script` and returns the terminal and its events.
    fn run(script: &str, cwd: PathBuf) -> (Terminal, mpsc::Receiver<TerminalEvent>) {
        let (sender, events) = mpsc::channel();
        let mut spawn = Spawn::shell(cwd, size(120, 6), palette(), "en_US.UTF-8");
        spawn.command = Some(("/bin/sh".into(), vec!["-c".into(), script.into()]));
        let terminal = Terminal::spawn(spawn, move |event| {
            let _ = sender.send(event);
        })
        .expect("the shell starts");
        (terminal, events)
    }

    fn wait_for(terminal: &Terminal, check: impl Fn(&Snapshot) -> bool) -> Snapshot {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let snapshot = terminal.snapshot();
            if check(&snapshot) {
                return snapshot;
            }
            assert!(
                Instant::now() < deadline,
                "gave up on:\n{}",
                snapshot.text()
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    #[test]
    fn runs_in_the_folder_with_colors_and_utf8() {
        let folder = tempfile::tempdir().unwrap();
        let (terminal, _events) = run(
            r#"pwd; printf '\033[31mrosso\033[0m città\n'; echo "$TERM $COLORTERM"; sleep 5"#,
            folder.path().to_path_buf(),
        );
        let snapshot = wait_for(&terminal, |snapshot| snapshot.text().contains("truecolor"));
        let folder = folder.path().canonicalize().unwrap();
        assert_eq!(snapshot.line_text(0), folder.display().to_string());
        let text = snapshot.text();
        assert!(text.contains("rosso città"), "{text}");
        assert!(text.contains("xterm-256color truecolor"), "{text}");
        let line = (0..snapshot.lines)
            .find(|&line| snapshot.line_text(line).starts_with("rosso"))
            .unwrap();
        assert_eq!(snapshot.rows[line][0].fg, palette().ansi[1]);
        assert_eq!(snapshot.rows[line][6].fg, palette().foreground);
    }

    #[test]
    fn typed_keys_reach_the_program_and_exit_is_reported() {
        let folder = tempfile::tempdir().unwrap();
        let (terminal, events) = run(
            "read line; echo \"got:$line\"; exit 3",
            folder.path().to_path_buf(),
        );
        terminal.write(b"ciao\r".as_slice());
        wait_for(&terminal, |snapshot| snapshot.text().contains("got:ciao"));
        let exited = events
            .iter()
            .find(|event| matches!(event, TerminalEvent::Exited(_)));
        assert_eq!(exited, Some(TerminalEvent::Exited(Some(3))));
    }

    #[test]
    fn resizing_tells_the_program() {
        let folder = tempfile::tempdir().unwrap();
        let (terminal, _events) = run("read go; stty size; sleep 5", folder.path().to_path_buf());
        terminal.resize(size(60, 9));
        terminal.write(b"\r".as_slice());
        let snapshot = wait_for(&terminal, |snapshot| snapshot.text().contains("9 60"));
        assert_eq!((snapshot.columns, snapshot.lines), (60, 9));
    }

    #[test]
    fn selection_search_and_scrollback() {
        let folder = tempfile::tempdir().unwrap();
        let (terminal, _events) = run(
            "for i in $(seq 1 30); do echo riga$i; done; echo fine; sleep 5",
            folder.path().to_path_buf(),
        );
        let snapshot = wait_for(&terminal, |snapshot| snapshot.text().contains("fine"));
        assert!(snapshot.history_size > 0);

        // Double click selects a word.
        terminal.start_selection(
            CellPoint { line: 0, column: 2 },
            Side::Left,
            SelectionKind::Word,
        );
        let first = snapshot.line_text(0);
        assert_eq!(terminal.selected_text().as_deref(), Some(first.as_str()));
        terminal.clear_selection();
        assert_eq!(terminal.selected_text(), None);

        // Searching up finds riga30 on the screen, then riga3 above it.
        assert!(terminal.find("riga3", false, false, true));
        assert_eq!(terminal.snapshot().display_offset, 0);
        assert!(terminal.find("riga3", false, false, true));
        let snapshot = terminal.snapshot();
        assert!(snapshot.display_offset > 0);
        assert!(
            snapshot
                .rows
                .iter()
                .flatten()
                .any(|cell| cell.found == Found::Current)
        );
        assert!(!terminal.find("assente", false, false, true));

        terminal.scroll_to_bottom();
        assert_eq!(terminal.snapshot().display_offset, 0);
        terminal.select_all();
        assert!(terminal.selected_text().unwrap().contains("riga1\n"));
    }

    /// zsh with Keel's integration and a home of its own, so the files of
    /// whoever runs the tests play no part.
    fn zsh(home: &Path) -> (Terminal, mpsc::Receiver<TerminalEvent>) {
        std::fs::write(home.join(".zshrc"), "PS1='$ '\n").unwrap();
        let (sender, events) = mpsc::channel();
        let mut spawn = Spawn::shell(home.to_path_buf(), size(80, 8), palette(), "en_US.UTF-8");
        spawn.command = Some(("/bin/zsh".into(), vec!["-i".into()]));
        spawn
            .env
            .insert("HOME".into(), home.to_string_lossy().into_owned());
        spawn.integrate_zsh(home).unwrap();
        let terminal = Terminal::spawn(spawn, move |event| {
            let _ = sender.send(event);
        })
        .expect("zsh starts");
        (terminal, events)
    }

    /// The next command to finish, and the progress reported meanwhile.
    fn finished(events: &mpsc::Receiver<TerminalEvent>) -> (ShellCommand, Vec<Option<Progress>>) {
        let mut progress = Vec::new();
        loop {
            match events.recv_timeout(Duration::from_secs(10)) {
                Ok(TerminalEvent::CommandFinished(command)) => return (command, progress),
                Ok(TerminalEvent::Progress(reported)) => progress.push(reported),
                Ok(_) => {}
                Err(error) => panic!("no command finished: {error}"),
            }
        }
    }

    /// Types a command once the prompt is drawn, as a person does: typed
    /// earlier, the terminal itself would echo it before the prompt.
    fn type_at_prompt(terminal: &Terminal, command: &[u8]) {
        wait_for(terminal, |snapshot| {
            snapshot
                .cursor
                .is_some_and(|cursor| snapshot.line_text(cursor.line) == "$")
        });
        terminal.write(command.to_vec());
    }

    #[test]
    fn shell_integration_marks_each_command() {
        let home = tempfile::tempdir().unwrap();
        let (terminal, events) = zsh(home.path());

        type_at_prompt(&terminal, b"echo uno\r");
        let (echo, _) = finished(&events);
        assert_eq!(
            (echo.text.as_deref(), echo.exit),
            (Some("echo uno"), Some(0))
        );
        type_at_prompt(&terminal, b"false\r");
        let (failed, _) = finished(&events);
        assert_eq!(failed.state(), Some(CommandState::Failed(1)));
        type_at_prompt(&terminal, b"for i in $(seq 1 30); do echo riga$i; done\r");
        let (lines, _) = finished(&events);
        // A program printing the marks cannot pass a command off as the
        // shell's: it does not know the nonce.
        type_at_prompt(
            &terminal,
            b"printf '\\e]633;E;rm -rf x;guess\\a\\e]9;4;1;40\\a'\r",
        );
        let (printed, mut progress) = finished(&events);
        assert!(printed.text.clone().unwrap().starts_with("printf"));
        std::thread::sleep(Duration::from_millis(100));
        progress.extend(events.try_iter().filter_map(|event| match event {
            TerminalEvent::Progress(progress) => Some(progress),
            _ => None,
        }));
        assert_eq!(progress, vec![Some(Progress::Normal(40)), None]);

        assert_eq!(terminal.output_of(echo.id).as_deref(), Some("uno"));
        let output = terminal.output_of(lines.id).unwrap();
        assert!(
            output.starts_with("riga1\n") && output.ends_with("riga30"),
            "{output}"
        );
        // Four commands, and the prompt waiting for the next.
        type_at_prompt(&terminal, b"");
        let commands = terminal.commands();
        assert_eq!(commands.len(), 5);
        assert_eq!(commands[4].state(), None);

        // Cmd+↑ goes up one command at a time, Cmd+↓ back to the end.
        assert_eq!(terminal.scroll_to_command(true), Some(printed.id));
        assert_eq!(terminal.scroll_to_command(true), Some(lines.id));
        let snapshot = terminal.snapshot();
        assert!(snapshot.display_offset > 0);
        let prompt = snapshot.prompts.iter().find(|prompt| prompt.id == lines.id);
        assert_eq!(prompt.map(|prompt| prompt.row), Some(0));
        assert!(snapshot.line_text(0).starts_with("$ for i in"));
        // Scrolled into its output, the command stays at the top.
        terminal.scroll(-3);
        let sticky = terminal.snapshot().sticky.unwrap();
        assert_eq!(sticky.id, lines.id);
        assert!(sticky.text.starts_with("for i in"));
        // From inside its output, Cmd+↑ goes to its start first.
        assert_eq!(terminal.scroll_to_command(true), Some(lines.id));
        assert_eq!(terminal.scroll_to_command(true), Some(failed.id));
        assert_eq!(terminal.scroll_to_command(true), Some(echo.id));
        assert_eq!(terminal.scroll_to_command(false), Some(failed.id));
        assert_eq!(terminal.scroll_to_command(false), Some(lines.id));
        assert_eq!(terminal.scroll_to_command(false), Some(printed.id));
        assert_eq!(terminal.scroll_to_command(false), None);
        assert_eq!(terminal.snapshot().display_offset, 0);

        // Narrower lines rewrap; every place stays on its text.
        terminal.resize(size(30, 6));
        assert_eq!(terminal.output_of(echo.id).as_deref(), Some("uno"));
        assert!(terminal.output_of(lines.id).unwrap().ends_with("riga30"));
        assert_eq!(terminal.scroll_to_command(true), Some(printed.id));
        assert_eq!(terminal.scroll_to_command(true), Some(lines.id));
        assert!(terminal.snapshot().line_text(0).starts_with("$ for i in"));

        // What comes back after a restart: the output, without the prompt
        // waiting at the end.
        let contents = String::from_utf8(terminal.contents(1000).unwrap()).unwrap();
        assert!(contents.contains("riga30"), "{contents}");
        assert!(!contents.trim_end().ends_with("$"), "{contents}");
    }

    #[test]
    fn restored_text_comes_first() {
        let folder = tempfile::tempdir().unwrap();
        let (sender, _events) = mpsc::channel();
        let mut spawn = Spawn::shell(
            folder.path().to_path_buf(),
            size(40, 6),
            palette(),
            "en_US.UTF-8",
        );
        spawn.command = Some((
            "/bin/sh".into(),
            vec!["-c".into(), "echo dopo; sleep 5".into()],
        ));
        spawn.restore = Some(b"\x1b[0;31mprima\x1b[0m\r\n".to_vec());
        let terminal = Terminal::spawn(spawn, move |event| {
            let _ = sender.send(event);
        })
        .unwrap();
        let snapshot = wait_for(&terminal, |snapshot| snapshot.text().contains("dopo"));
        assert_eq!(snapshot.line_text(0), "prima");
        assert_eq!(snapshot.rows[0][0].fg, palette().ansi[1]);
        assert_eq!(snapshot.line_text(1), "dopo");
    }

    #[test]
    fn languages_become_lang() {
        assert_eq!(lang_for(Some("it-IT")), "it_IT.UTF-8");
        assert_eq!(lang_for(Some("it")), "it_IT.UTF-8");
        assert_eq!(lang_for(None), "en_US.UTF-8");
    }
}

/// How fast the emulator takes output and how long a snapshot of the
/// screen takes; run with `cargo test --release -p terminal timing --
/// --ignored --nocapture`.
#[cfg(test)]
mod timing {
    use std::time::{Duration, Instant};

    use super::*;

    #[test]
    #[ignore]
    fn throughput_and_snapshot() {
        let lines = 500_000;
        let mut spawn = Spawn::shell(
            std::env::temp_dir(),
            Size {
                columns: 200,
                lines: 50,
                cell_width: 8,
                cell_height: 16,
            },
            Palette::dark(Rgb::hex(0xdddddd), Rgb::hex(0x101010)),
            "en_US.UTF-8",
        );
        spawn.command = Some((
            "/bin/sh".into(),
            vec![
                "-c".into(),
                format!(
                    "seq 1 {lines} | sed 's/$/ \x1b[32mok\x1b[0m lorem ipsum dolor sit amet/'; echo fine; sleep 5"
                ),
            ],
        ));
        let start = Instant::now();
        let terminal = Terminal::spawn(spawn, |_| {}).unwrap();
        loop {
            if terminal.snapshot().text().contains("fine") {
                break;
            }
            assert!(start.elapsed() < Duration::from_secs(60));
            std::thread::sleep(Duration::from_millis(5));
        }
        eprintln!(
            "{lines} lines through the emulator in {:?}",
            start.elapsed()
        );

        let start = Instant::now();
        for _ in 0..100 {
            std::hint::black_box(terminal.snapshot());
        }
        eprintln!("snapshot of 200x50: {:?} each", start.elapsed() / 100);
    }
}
