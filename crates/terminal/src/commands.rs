//! The commands a terminal ran, from the marks its shell printed: where
//! each prompt, output and end is on the screen, and how each command
//! ended.
//!
//! A place is kept as a line counted from the first line the terminal ever
//! had, so it stays with its text while output scrolls. To know how far
//! the text moved, a hidden tag sits on the newest line of the history:
//! lines there never change, so where the tag is now says how many lines
//! went up since. A resize, which rewraps lines, tags every place instead
//! and looks for the tags afterwards.

use std::collections::{HashMap, VecDeque};
use std::time::{Duration, Instant};

use alacritty_terminal::event::EventListener;
use alacritty_terminal::grid::{Dimensions as _, Scroll};
use alacritty_terminal::index::{Column, Line, Point};
use alacritty_terminal::term::cell::Hyperlink;
use alacritty_terminal::term::{Term, TermMode};

use crate::TerminalEvent;
use crate::marks::{Mark, Progress};

/// How many commands a terminal remembers.
const KEPT: usize = 500;

/// The start of the hidden links Keel puts on cells; they are never shown.
pub(crate) const TAG: &str = "keel-place:";

/// A place in the output that stays with its text: the line counted from
/// the first one the terminal ever had, and the column.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Spot {
    pub line: u64,
    pub column: usize,
}

/// How a command is doing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CommandState {
    Running,
    Succeeded,
    Failed(i32),
    /// It ended, but the shell did not say how.
    Ended,
}

/// A command the shell ran.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ShellCommand {
    /// Stays the same while older commands are forgotten.
    pub id: u64,
    /// The command line, when the shell said it.
    pub text: Option<String>,
    pub prompt: Option<Spot>,
    /// Where the command line starts, after the prompt.
    pub input: Option<Spot>,
    pub output: Option<Spot>,
    pub end: Option<Spot>,
    pub exit: Option<i32>,
    pub started: Option<Instant>,
    pub duration: Option<Duration>,
    pub finished: bool,
}

impl ShellCommand {
    /// `None` for a prompt waiting for a command.
    pub fn state(&self) -> Option<CommandState> {
        if self.finished {
            Some(match self.exit {
                Some(0) => CommandState::Succeeded,
                Some(code) => CommandState::Failed(code),
                None => CommandState::Ended,
            })
        } else if self.output.is_some() {
            Some(CommandState::Running)
        } else {
            None
        }
    }

    fn places(&mut self) -> [&mut Option<Spot>; 4] {
        [
            &mut self.prompt,
            &mut self.input,
            &mut self.output,
            &mut self.end,
        ]
    }

    fn forget_places(&mut self) {
        for place in self.places() {
            *place = None;
        }
    }
}

/// A prompt on the screen, with how its command did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PromptMark {
    /// The row of the screen, from the top.
    pub row: usize,
    pub id: u64,
    pub state: Option<CommandState>,
}

/// The command whose output fills the top of the screen while its own line
/// has gone above it, as VS Code keeps it in sight.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StickyCommand {
    pub id: u64,
    pub text: String,
    pub state: Option<CommandState>,
}

/// A cell tagged while the terminal resizes.
struct Tagged {
    uri: String,
    saved: Option<Hyperlink>,
    found: Option<Point>,
}

/// The tag on the newest line of the history.
struct Anchor {
    line: u64,
    uri: String,
    /// The link the cell had before, put back when the tag moves on.
    saved: Option<Hyperlink>,
}

#[derive(Default)]
pub(crate) struct Tracker {
    pub(crate) commands: VecDeque<ShellCommand>,
    /// Lines that went into the history since the terminal started.
    scrolled: u64,
    /// The history's size when last looked at.
    history: usize,
    anchor: Option<Anchor>,
    next_tag: u64,
    next_id: u64,
    /// The nonce the shell proves its command lines with.
    pub(crate) nonce: Option<String>,
    pub(crate) progress: Option<Progress>,
    /// The command Cmd+↑ and Cmd+↓ last went to.
    pub(crate) jumped: Option<u64>,
}

impl Tracker {
    /// Counts the lines that went up since the last look, and moves the tag
    /// to the newest line of the history.
    pub(crate) fn catch_up<L: EventListener>(&mut self, term: &mut Term<L>) {
        // A full-screen program has its own screen; the shell's waits.
        if term.mode().contains(TermMode::ALT_SCREEN) {
            return;
        }
        let history = term.history_size();
        let moved = match self.anchor.take() {
            Some(anchor) => {
                let was = anchor.line as i64 - self.scrolled as i64;
                let top = -(history as i64);
                let grid = term.grid_mut();
                let mut found = None;
                // Lines only go up, so the tag is where it was or above.
                let mut line = was;
                while line >= top && line < 0 {
                    let cell = &mut grid[Line(line as i32)][Column(0)];
                    if cell
                        .hyperlink()
                        .is_some_and(|link| link.uri() == anchor.uri)
                    {
                        cell.set_hyperlink(anchor.saved.clone());
                        found = Some((was - line) as u64);
                        break;
                    }
                    line -= 1;
                }
                found
            }
            None => history.checked_sub(self.history).map(|grown| grown as u64),
        };
        match moved {
            Some(lines) => self.scrolled += lines,
            None => {
                // The history was cleared, or more went by than it holds:
                // the places from before are gone.
                for command in &mut self.commands {
                    command.forget_places();
                }
                self.scrolled += (history + term.screen_lines()) as u64;
            }
        }
        self.history = history;
        self.anchor = self.tag_newest_history_line(term);
    }

    fn tag_newest_history_line<L: EventListener>(&mut self, term: &mut Term<L>) -> Option<Anchor> {
        if term.history_size() == 0 {
            return None;
        }
        self.next_tag += 1;
        let uri = format!("{TAG}{}", self.next_tag);
        let cell = &mut term.grid_mut()[Line(-1)][Column(0)];
        let saved = cell.hyperlink();
        cell.set_hyperlink(Some(Hyperlink::new(None::<String>, uri.clone())));
        Some(Anchor {
            line: self.scrolled.saturating_sub(1),
            uri,
            saved,
        })
    }

    fn drop_anchor<L: EventListener>(&mut self, term: &mut Term<L>) {
        let Some(anchor) = self.anchor.take() else {
            return;
        };
        if let Some(line) = self.line_of(anchor.line, term) {
            let cell = &mut term.grid_mut()[line][Column(0)];
            if cell
                .hyperlink()
                .is_some_and(|link| link.uri() == anchor.uri)
            {
                cell.set_hyperlink(anchor.saved);
            }
        }
    }

    /// The grid's line for an absolute one, while it is still kept.
    fn line_of<L: EventListener>(&self, line: u64, term: &Term<L>) -> Option<Line> {
        let line = line as i64 - self.scrolled as i64;
        let top = -(term.history_size() as i64);
        (line >= top && line < term.screen_lines() as i64).then_some(Line(line as i32))
    }

    fn spot_at(&self, point: Point) -> Spot {
        Spot {
            line: (self.scrolled as i64 + point.line.0 as i64).max(0) as u64,
            column: point.column.0,
        }
    }

    /// Where a place is on the grid now.
    pub(crate) fn point_of<L: EventListener>(&self, spot: Spot, term: &Term<L>) -> Option<Point> {
        let line = self.line_of(spot.line, term)?;
        Some(Point::new(
            line,
            Column(spot.column.min(term.last_column().0)),
        ))
    }

    /// The command the shell is at: the last one, unless it has ended.
    fn current(&mut self) -> &mut ShellCommand {
        if self
            .commands
            .back()
            .is_none_or(|command| command.output.is_some() || command.finished)
        {
            self.push(ShellCommand::default());
        }
        self.commands.back_mut().expect("just pushed")
    }

    fn push(&mut self, mut command: ShellCommand) {
        self.next_id += 1;
        command.id = self.next_id;
        self.commands.push_back(command);
        while self.commands.len() > KEPT {
            self.commands.pop_front();
        }
    }

    /// Whether the shell drew its prompt and waits for a command line.
    pub(crate) fn at_prompt<L: EventListener>(&mut self, term: &mut Term<L>) -> bool {
        if term.mode().contains(TermMode::ALT_SCREEN) {
            return false;
        }
        self.catch_up(term);
        let cursor = self.spot_at(term.grid().cursor.point);
        self.commands.back().is_some_and(|last| {
            last.output.is_none()
                && !last.finished
                && last.prompt.is_some_and(|prompt| cursor > prompt)
        })
    }

    /// Takes a mark the shell printed, with the cursor where it was then.
    pub(crate) fn mark<L: EventListener>(
        &mut self,
        term: &mut Term<L>,
        mark: Mark,
    ) -> Vec<TerminalEvent> {
        if let Mark::Progress(progress) = mark {
            self.progress = progress;
            return vec![TerminalEvent::Progress(progress)];
        }
        if let Mark::Notify(text) = mark {
            return vec![TerminalEvent::Notify(text)];
        }
        if term.mode().contains(TermMode::ALT_SCREEN) {
            return Vec::new();
        }
        self.catch_up(term);
        let cursor = term.grid().cursor.point;
        let spot = self.spot_at(cursor);
        match mark {
            Mark::PromptStart => {
                // After output without a final newline, zsh draws a mark
                // and starts the prompt on the next line.
                let spot = if spot.column > 0 {
                    Spot {
                        line: spot.line + 1,
                        column: 0,
                    }
                } else {
                    spot
                };
                match self.commands.back_mut() {
                    // The prompt drawn again, with nothing run from it.
                    Some(last) if last.output.is_none() && !last.finished => {
                        last.prompt = Some(spot);
                        last.input = None;
                    }
                    _ => self.push(ShellCommand {
                        prompt: Some(spot),
                        ..ShellCommand::default()
                    }),
                }
                Vec::new()
            }
            Mark::CommandStart => {
                self.current().input = Some(spot);
                Vec::new()
            }
            Mark::CommandLine { text, nonce } => {
                if self.nonce.is_some() && nonce != self.nonce {
                    return Vec::new();
                }
                let text = text.trim().to_owned();
                if !text.is_empty() {
                    self.current().text = Some(text);
                }
                Vec::new()
            }
            Mark::OutputStart => {
                let scrolled = self.scrolled;
                let command = self.current();
                command.output = Some(spot);
                command.started = Some(Instant::now());
                // Without the command line from the shell, it is what was
                // typed after the prompt.
                if command.text.is_none()
                    && let Some(input) = command.input
                    && cursor.line.0 > 0
                {
                    let start = Line((input.line as i64 - scrolled as i64) as i32);
                    let end = Point::new(cursor.line - 1, term.last_column());
                    if start <= end.line && start >= term.topmost_line() {
                        let typed = term
                            .bounds_to_string(Point::new(start, Column(input.column)), end)
                            .trim()
                            .to_owned();
                        if !typed.is_empty() {
                            command.text = Some(typed);
                        }
                    }
                }
                let command = command.clone();
                vec![TerminalEvent::CommandStarted(command)]
            }
            Mark::CommandEnd(exit) => {
                let Some(command) = self
                    .commands
                    .back_mut()
                    .filter(|command| command.output.is_some() && !command.finished)
                else {
                    return Vec::new();
                };
                command.end = Some(spot);
                command.exit = exit;
                command.finished = true;
                command.duration = command.started.map(|started| started.elapsed());
                let command = command.clone();
                let mut events = vec![TerminalEvent::CommandFinished(command)];
                // A program that ended without taking its progress away.
                if self.progress.take().is_some() {
                    events.push(TerminalEvent::Progress(None));
                }
                events
            }
            Mark::Progress(_) | Mark::Notify(_) => Vec::new(),
        }
    }

    /// Resizes the terminal, keeping each place on its text although lines
    /// rewrap.
    pub(crate) fn resize<L: EventListener>(
        &mut self,
        term: &mut Term<L>,
        resize: impl FnOnce(&mut Term<L>),
    ) {
        self.catch_up(term);
        if term.mode().contains(TermMode::ALT_SCREEN) {
            resize(term);
            return;
        }
        self.drop_anchor(term);
        // One tag for each cell a place is on: a command's end and the
        // next prompt share theirs.
        let mut tagged: Vec<Tagged> = Vec::new();
        let mut at: HashMap<(i32, usize), usize> = HashMap::new();
        // Each place: its command, which place it is, its tag, and how many
        // columns the tag is after it.
        let mut followers: Vec<(usize, usize, usize, usize)> = Vec::new();
        let last_column = term.last_column();
        for index in 0..self.commands.len() {
            for which in 0..4 {
                let Some(spot) = *self.commands[index].places()[which] else {
                    continue;
                };
                let Some(line) = self.line_of(spot.line, term) else {
                    *self.commands[index].places()[which] = None;
                    continue;
                };
                let column = Column(spot.column.min(last_column.0));
                // A blank cell can be dropped as lines rewrap: the tag goes
                // on the first written one from there.
                let row = &term.grid()[line];
                let cell_column = (column.0..=last_column.0)
                    .map(Column)
                    .find(|&column| row[column].c != ' ')
                    .unwrap_or(column);
                let point = Point::new(line, cell_column);
                let tag = match at.get(&(point.line.0, point.column.0)) {
                    Some(&tag) => tag,
                    None => {
                        self.next_tag += 1;
                        let uri = format!("{TAG}{}", self.next_tag);
                        let cell = &mut term.grid_mut()[line][cell_column];
                        let saved = cell.hyperlink();
                        cell.set_hyperlink(Some(Hyperlink::new(None::<String>, uri.clone())));
                        tagged.push(Tagged {
                            uri,
                            saved,
                            found: None,
                        });
                        at.insert((point.line.0, point.column.0), tagged.len() - 1);
                        tagged.len() - 1
                    }
                };
                followers.push((index, which, tag, cell_column.0 - column.0));
            }
        }

        resize(term);

        // Find the tags where the text went, and put the old links back.
        if !tagged.is_empty() {
            let by_uri: HashMap<String, usize> = tagged
                .iter()
                .enumerate()
                .map(|(index, tag)| (tag.uri.clone(), index))
                .collect();
            let top = term.topmost_line();
            let bottom = term.bottommost_line();
            let columns = term.columns();
            let grid = term.grid_mut();
            let mut line = top;
            while line <= bottom {
                for column in 0..columns {
                    let cell = &mut grid[line][Column(column)];
                    let Some(link) = cell.hyperlink() else {
                        continue;
                    };
                    if let Some(&index) = by_uri.get(link.uri()) {
                        cell.set_hyperlink(tagged[index].saved.clone());
                        tagged[index].found = Some(Point::new(line, Column(column)));
                    }
                }
                line += 1;
            }
        }
        // Rewrapping can put lines into the history: every place is worked
        // out again from where its tag is, so the count only has to cover
        // the history.
        self.scrolled = self.scrolled.max(term.history_size() as u64);
        let scrolled = self.scrolled as i64;
        for (index, which, tag, offset) in followers {
            let spot = tagged[tag].found.map(|point| Spot {
                line: (scrolled + point.line.0 as i64).max(0) as u64,
                column: point.column.0.saturating_sub(offset),
            });
            *self.commands[index].places()[which] = spot;
        }
        self.history = term.history_size();
        self.anchor = self.tag_newest_history_line(term);
    }

    /// Forgets every place, as Cmd+K clears the screen.
    pub(crate) fn clear(&mut self) {
        for command in &mut self.commands {
            command.forget_places();
        }
        self.anchor = None;
        self.history = 0;
        self.jumped = None;
    }

    /// The grid line of a place, even when it has gone from the history.
    fn grid_line(&self, spot: Spot) -> i64 {
        spot.line as i64 - self.scrolled as i64
    }

    /// The prompts on the screen and the command kept at its top.
    pub(crate) fn on_screen<L: EventListener>(
        &self,
        term: &Term<L>,
    ) -> (Vec<PromptMark>, Option<StickyCommand>) {
        let offset = term.grid().display_offset() as i64;
        let lines = term.screen_lines() as i64;
        let row_of = |spot: Spot| self.grid_line(spot) + offset;
        let prompts = self
            .commands
            .iter()
            .filter_map(|command| {
                let row = row_of(command.prompt?);
                (0..lines).contains(&row).then(|| PromptMark {
                    row: row as usize,
                    id: command.id,
                    state: command.state(),
                })
            })
            .collect();
        let sticky = self
            .commands
            .iter()
            .rev()
            .find(|command| command.prompt.is_some_and(|prompt| row_of(prompt) < 0))
            .filter(|command| {
                command.output.is_some()
                    && command.end.is_none_or(|end| row_of(end) > 0)
                    && command.prompt.is_some_and(|prompt| {
                        self.grid_line(prompt) >= -(term.history_size() as i64)
                    })
            })
            .map(|command| {
                let text = command.text.clone().unwrap_or_else(|| {
                    let line = Line(self.grid_line(command.prompt.expect("filtered")) as i32);
                    term.bounds_to_string(
                        Point::new(line, Column(0)),
                        Point::new(line, term.last_column()),
                    )
                    .trim()
                    .to_owned()
                });
                StickyCommand {
                    id: command.id,
                    text,
                    state: command.state(),
                }
            });
        (prompts, sticky)
    }

    /// Cmd+↑ and Cmd+↓: shows the prompt of the command before or after
    /// the one shown, at the top of the screen, and says which it is. Past
    /// the newest, the screen goes back to the end.
    pub(crate) fn scroll_to_command<L: EventListener>(
        &mut self,
        term: &mut Term<L>,
        older: bool,
    ) -> Option<u64> {
        self.catch_up(term);
        let offset = term.grid().display_offset() as i64;
        let lines = term.screen_lines() as i64;
        let history = term.history_size() as i64;
        let top = -offset;
        let ran: Vec<(i64, u64)> = self
            .commands
            .iter()
            .filter(|command| command.output.is_some() || command.finished)
            .filter_map(|command| Some((self.grid_line(command.prompt?), command.id)))
            .filter(|(line, _)| *line >= -history)
            .collect();
        // From the command gone to last while it is in sight, else from the
        // top of the screen, or from the end when the screen is there.
        let from = self
            .jumped
            .and_then(|id| ran.iter().find(|(_, command)| *command == id))
            .map(|(line, _)| *line)
            .filter(|line| (top..top + lines).contains(line))
            .unwrap_or(if offset == 0 { i64::MAX } else { top });
        let target = if older {
            ran.iter().rev().find(|(line, _)| *line < from)
        } else {
            ran.iter().find(|(line, _)| *line > from)
        };
        match target {
            Some(&(line, id)) => {
                let wanted = (-line).clamp(0, history);
                term.scroll_display(Scroll::Delta((wanted - offset) as i32));
                self.jumped = Some(id);
                Some(id)
            }
            None if !older => {
                term.scroll_display(Scroll::Bottom);
                self.jumped = None;
                None
            }
            None => self.jumped,
        }
    }

    /// Shows a command's prompt at the top of the screen.
    pub(crate) fn scroll_to<L: EventListener>(&mut self, term: &mut Term<L>, id: u64) {
        self.catch_up(term);
        let Some(prompt) = self
            .commands
            .iter()
            .find(|command| command.id == id)
            .and_then(|command| command.prompt)
        else {
            return;
        };
        let offset = term.grid().display_offset() as i64;
        let wanted = (-self.grid_line(prompt)).clamp(0, term.history_size() as i64);
        term.scroll_display(Scroll::Delta((wanted - offset) as i32));
        self.jumped = Some(id);
    }

    /// What a command printed, while it is still on the screen or in the
    /// history.
    pub(crate) fn output_of<L: EventListener>(
        &mut self,
        term: &mut Term<L>,
        id: u64,
    ) -> Option<String> {
        self.catch_up(term);
        let command = self.commands.iter().find(|command| command.id == id)?;
        let start = self.point_of(command.output?, term)?;
        let end = match command.end {
            Some(end) => self.point_of(end, term)?,
            None => term.grid().cursor.point,
        };
        // To the end of the line before the end, or into the end's own line
        // when the output did not end with a newline.
        let last = if end.column.0 > 0 {
            Point::new(end.line, end.column - 1)
        } else if end.line > start.line {
            Point::new(end.line - 1, term.last_column())
        } else {
            return Some(String::new());
        };
        if last < start {
            return Some(String::new());
        }
        Some(term.bounds_to_string(start, last).trim_end().to_owned())
    }
}
