//! What the window does with the commands its terminals run: Cmd+↑ and
//! Cmd+↓ between them, copying the last one or its output, running a
//! recent one again, asking Claude Code about one, and a notice when a
//! long one ends with Keel in the background.

use std::collections::HashSet;
use std::time::Duration;

use gpui::Div;

use super::notices::{Notice, fill};
use super::terminal_view::{TerminalView, duration_text};
use super::*;
use crate::quick_pick::{self, PickItem};
use terminal::{CommandState, ShellCommand};

actions!(
    keel,
    [
        ScrollToPreviousCommand,
        ScrollToNextCommand,
        CopyLastCommandOutput,
        CopyLastCommand,
        RunRecentCommand,
        AskClaudeAboutTerminal
    ]
);

/// A command that took this long gets a notice when it ends with Keel in
/// the background, as iTerm and Ghostty do.
const LONG_COMMAND: Duration = Duration::from_secs(10);
/// How many lines of the shell's own history «Run Recent Command» lists.
const HISTORY_SHOWN: usize = 500;

impl Workbench {
    /// The terminal with the keyboard, else the panel's.
    pub(super) fn focused_terminal(
        &self,
        window: &Window,
        cx: &App,
    ) -> Option<Entity<TerminalView>> {
        let mut all = self
            .terminals
            .iter()
            .map(|terminal| &terminal.view)
            .chain(self.agent_terminal.as_ref().map(|agent| &agent.view));
        all.find(|view| view.read(cx).focus_handle(cx).contains_focused(window, cx))
            .or_else(|| self.active_terminal_view())
            .cloned()
    }

    pub(super) fn terminal_command_actions(&self, root: Div, cx: &mut Context<Self>) -> Div {
        root.on_action(
            cx.listener(|this, _: &ScrollToPreviousCommand, window, cx| {
                this.jump_to_command(true, window, cx)
            }),
        )
        .on_action(cx.listener(|this, _: &ScrollToNextCommand, window, cx| {
            this.jump_to_command(false, window, cx)
        }))
        .on_action(cx.listener(|this, _: &CopyLastCommandOutput, window, cx| {
            this.copy_last_command(true, window, cx)
        }))
        .on_action(cx.listener(|this, _: &CopyLastCommand, window, cx| {
            this.copy_last_command(false, window, cx)
        }))
        .on_action(
            cx.listener(|this, _: &RunRecentCommand, window, cx| {
                this.run_recent_command(window, cx)
            }),
        )
        .on_action(cx.listener(|this, _: &AskClaudeAboutTerminal, window, cx| {
            this.ask_claude_about_terminal(window, cx)
        }))
    }

    fn jump_to_command(&mut self, older: bool, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(view) = self.focused_terminal(window, cx) {
            view.update(cx, |view, cx| view.jump_to_command(older, cx));
        }
    }

    /// Copies the last command, or what it printed.
    fn copy_last_command(&mut self, output: bool, window: &mut Window, cx: &mut Context<Self>) {
        let strings = self.strings();
        let Some(view) = self.focused_terminal(window, cx) else {
            return;
        };
        let view = view.read(cx);
        let Some(command) = view.last_command() else {
            self.status_message = Some(strings.terminal_no_command.to_owned());
            cx.notify();
            return;
        };
        let (text, message) = if output {
            (view.output_of(command.id), strings.terminal_output_copied)
        } else {
            (command.text.clone(), strings.terminal_command_copied)
        };
        if let Some(text) = text {
            cx.write_to_clipboard(gpui::ClipboardItem::new_string(text));
            self.status_message = Some(message.to_owned());
        }
        cx.notify();
    }

    /// The commands of this session in every terminal, the newest first,
    /// then the shell's own history; the one chosen runs in the terminal
    /// with the keyboard.
    fn run_recent_command(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(target) = self.focused_terminal(window, cx) else {
            return;
        };
        let strings = self.strings();
        let mut views: Vec<Entity<TerminalView>> = vec![target.clone()];
        views.extend(
            self.terminals
                .iter()
                .map(|terminal| terminal.view.clone())
                .filter(|view| *view != target),
        );
        let mut seen = HashSet::new();
        let mut entries: Vec<(String, String)> = Vec::new();
        let mut session: Vec<ShellCommand> = views
            .iter()
            .flat_map(|view| view.read(cx).commands())
            .filter(|command| command.output.is_some())
            .collect();
        session.sort_by_key(|command| std::cmp::Reverse(command.started));
        for command in session {
            let Some(text) = command.text.clone() else {
                continue;
            };
            if seen.insert(text.clone()) {
                let detail = match command.state() {
                    Some(CommandState::Failed(code)) => {
                        fill(strings.terminal_exit_code, &[("code", &code.to_string())])
                    }
                    _ => command.duration.map(duration_text).unwrap_or_default(),
                };
                entries.push((text, detail));
            }
        }
        for text in shell_history(HISTORY_SHOWN) {
            if seen.insert(text.clone()) {
                entries.push((text, strings.terminal_from_history.to_owned()));
            }
        }
        if entries.is_empty() {
            self.status_message = Some(strings.terminal_no_command.to_owned());
            cx.notify();
            return;
        }
        let items = entries
            .iter()
            .map(|(text, detail)| {
                let mut item = PickItem::new(text.replace('\n', " ⏎ "));
                if !detail.is_empty() {
                    item.detail = Some(detail.clone());
                }
                item
            })
            .collect();
        let chosen = quick_pick::show(
            Some(strings.terminal_recent_commands.to_owned()),
            None,
            items,
            window,
            cx,
        );
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Some(picked)) = chosen.await else {
                return;
            };
            this.update_in(cx, |_, window, cx| {
                let view = target.read(cx);
                view.run(&entries[picked].0);
                view.focus_handle(cx).focus(window, cx);
            })
            .ok();
        })
        .detach();
    }

    /// Cmd+L in a terminal: the selection, or the last command with what
    /// it printed, goes to Claude Code.
    fn ask_claude_about_terminal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(view) = self.focused_terminal(window, cx) else {
            return;
        };
        match view.read(cx).claude_prompt() {
            Some(prompt) => self.ask_claude(prompt, window, cx),
            None => {
                self.status_message = Some(self.strings().terminal_no_command.to_owned());
                cx.notify();
            }
        }
    }

    /// A command ended: with Keel in the background, a long one says so.
    pub(super) fn terminal_command_finished(
        &mut self,
        view: &Entity<TerminalView>,
        command: &ShellCommand,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if window.is_window_active() || command.duration.is_none_or(|took| took < LONG_COMMAND) {
            return;
        }
        let strings = self.strings();
        let name = format!("«{}»", command.text.clone().unwrap_or_default().trim());
        let time = command.duration.map(duration_text).unwrap_or_default();
        let notice = match command.state() {
            // Stopped with Ctrl+C: nothing to say.
            Some(CommandState::Failed(130)) => return,
            Some(CommandState::Failed(code)) => Notice::warning(fill(
                strings.terminal_failed_after,
                &[("command", &name), ("time", &time)],
            ))
            .message(fill(
                strings.terminal_exit_code,
                &[("code", &code.to_string())],
            )),
            _ => Notice::success(fill(
                strings.terminal_finished_in,
                &[("command", &name), ("time", &time)],
            )),
        };
        let view = view.downgrade();
        let notice = notice.action(strings.terminal_show, move |this, window, cx| {
            if let Some(view) = view.upgrade() {
                this.reveal_terminal(&view, window, cx);
            }
        });
        self.notify_user(notice, window, cx);
    }

    /// A program in a terminal asked for you, with a notification or the
    /// bell: with Keel in the background, a notice and the Dock say so, as
    /// iTerm and Ghostty do for Claude Code waiting for input.
    pub(super) fn terminal_wants_you(
        &mut self,
        view: &Entity<TerminalView>,
        text: Option<&String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if window.is_window_active() {
            return;
        }
        crate::mac::needs_attention();
        // A bell only counts on the Dock: programs ring it for many small
        // things. A notification, which a program asks for on purpose, is
        // a notice too.
        let Some(title) = text.cloned() else {
            return;
        };
        let strings = self.strings();
        let view = view.downgrade();
        let notice = Notice::info(title).action(strings.terminal_show, move |this, window, cx| {
            if let Some(view) = view.upgrade() {
                this.reveal_terminal(&view, window, cx);
            }
        });
        self.notify_user(notice, window, cx);
    }

    /// Brings a terminal to the front and gives it the keyboard.
    pub(super) fn reveal_terminal(
        &mut self,
        view: &Entity<TerminalView>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(index) = self
            .terminals
            .iter()
            .position(|terminal| terminal.view == *view)
        {
            self.active_terminal = index;
            self.layout.show_panel_view(PanelView::Terminal);
        } else {
            self.layout.agent = true;
        }
        view.read(cx).focus_handle(cx).focus(window, cx);
        self.session_changed(cx);
        cx.notify();
    }
}

/// The shell's own history, the newest first and each line once: zsh's
/// `~/.zsh_history`, in its extended form or not.
fn shell_history(most: usize) -> Vec<String> {
    let file = std::env::var_os("HISTFILE")
        .map(std::path::PathBuf::from)
        .or_else(|| std::env::home_dir().map(|home| home.join(".zsh_history")));
    let Some(bytes) = file.and_then(|file| std::fs::read(file).ok()) else {
        return Vec::new();
    };
    parse_zsh_history(&bytes, most)
}

/// zsh keeps the history "metafied": a byte it would choke on is 0x83
/// followed by the byte xor 32. A line ending in a backslash goes on.
fn parse_zsh_history(bytes: &[u8], most: usize) -> Vec<String> {
    let mut plain = Vec::with_capacity(bytes.len());
    let mut iter = bytes.iter();
    while let Some(&byte) = iter.next() {
        if byte == 0x83 {
            if let Some(&next) = iter.next() {
                plain.push(next ^ 32);
            }
        } else {
            plain.push(byte);
        }
    }
    let text = String::from_utf8_lossy(&plain);
    let mut commands: Vec<String> = Vec::new();
    let mut current = String::new();
    for line in text.lines() {
        let line = if current.is_empty() {
            // `: 1700000000:0;command` in the extended form.
            match line
                .strip_prefix(": ")
                .and_then(|rest| rest.split_once(';'))
            {
                Some((_, command)) => command,
                None => line,
            }
        } else {
            line
        };
        if let Some(open) = line.strip_suffix('\\') {
            current.push_str(open);
            current.push('\n');
            continue;
        }
        current.push_str(line);
        let command = std::mem::take(&mut current).trim().to_owned();
        if !command.is_empty() {
            commands.push(command);
        }
    }
    let mut seen = HashSet::new();
    commands
        .into_iter()
        .rev()
        .filter(|command| seen.insert(command.clone()))
        .take(most)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_zsh_history_newest_first() {
        let mut file = b": 1700000000:0;ls -la\n: 1700000001:0;echo a \\\n  b\n".to_vec();
        // «città», metafied: 0x83 then each byte that needs it xor 32.
        file.extend_from_slice(b": 1700000002:0;cd citt");
        file.extend_from_slice("à".as_bytes());
        file.extend_from_slice(b"\n: 1700000003:0;ls -la\ngit status\n");
        assert_eq!(
            parse_zsh_history(&file, 10),
            vec!["git status", "ls -la", "cd città", "echo a \n  b"]
        );
        assert_eq!(parse_zsh_history(&file, 1), vec!["git status"]);
        let metafied = [b'e', b'c', b'h', b'o', b' ', 0x83, 0xa3 ^ 32];
        assert_eq!(parse_zsh_history(&metafied, 1), vec!["echo \u{fffd}"]);
    }
}
