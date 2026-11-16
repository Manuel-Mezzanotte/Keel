//! The terminals of the bottom panel: opening them in the project, showing
//! the one in front, and closing them when their shell ends.

use gpui::{AnyElement, Entity, Subscription};
use terminal::Spawn;

use super::terminal_view::{self, TerminalLook, TerminalView, TerminalViewEvent};
use super::*;

/// A terminal of the panel.
pub(super) struct TerminalTab {
    pub(super) view: Entity<TerminalView>,
    _subscription: Subscription,
}

/// The font size of terminals when the settings do not say, as in VS Code
/// on the Mac.
const TERMINAL_FONT_SIZE: f32 = 12.;

impl Workbench {
    /// The terminals' font and keys, from the `terminal.integrated.*`
    /// settings VS Code uses.
    pub(super) fn terminal_look(&self, cx: &App) -> TerminalLook {
        let font_family = self
            .settings
            .string("terminal.integrated.fontFamily")
            .filter(|family| !family.trim().is_empty())
            .map(|family| {
                // VS Code takes a CSS list; the first font is the one.
                let first = family.split(',').next().unwrap_or(family);
                first.trim().trim_matches(['\'', '"']).to_owned().into()
            })
            .unwrap_or_else(|| cx.theme().mono_font_family.clone());
        let font_size = self
            .settings
            .number("terminal.integrated.fontSize")
            .map_or(TERMINAL_FONT_SIZE, |size| size.clamp(6., 72.) as f32);
        let option_is_meta = self
            .settings
            .get("terminal.integrated.macOptionIsMeta")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        TerminalLook {
            font_family,
            font_size,
            option_is_meta,
        }
    }

    /// Where new terminals start: the project, or the home folder.
    fn terminal_folder(&self) -> PathBuf {
        self.project
            .root
            .clone()
            .or_else(std::env::home_dir)
            .unwrap_or_else(|| PathBuf::from("/"))
    }

    /// Opens a terminal in the project and puts it in front.
    pub(super) fn new_terminal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let spawn = terminal_view::shell_spawn(self.terminal_folder(), cx);
        self.add_terminal(spawn, window, cx);
    }

    /// Starts a terminal as described and puts it in front. Returns false,
    /// with a message, when it cannot start.
    pub(super) fn add_terminal(
        &mut self,
        spawn: Spawn,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let started = match terminal_view::start(spawn) {
            Ok(started) => started,
            Err(error) => {
                let message = self.strings().terminal_failed(&error.to_string());
                self.report(message);
                cx.notify();
                return false;
            }
        };
        let look = self.terminal_look(cx);
        let view = cx.new(|cx| TerminalView::new(started, look, window, cx));
        let subscription =
            cx.subscribe_in(&view, window, |this, view, event, window, cx| match event {
                TerminalViewEvent::Exited => this.terminal_exited(view, window, cx),
                TerminalViewEvent::TitleChanged => cx.notify(),
                TerminalViewEvent::OpenFile { path, line, column } => {
                    this.open_file(path, window, cx);
                    if let Some(line) = line {
                        this.go_to(*line, column.unwrap_or(1), window, cx);
                    }
                }
            });
        self.terminals.push(TerminalTab {
            view,
            _subscription: subscription,
        });
        self.active_terminal = self.terminals.len() - 1;
        self.layout.show_panel_view(PanelView::Terminal);
        self.focus_terminal(window, cx);
        self.session_changed(cx);
        cx.notify();
        true
    }

    pub(super) fn active_terminal_view(&self) -> Option<&Entity<TerminalView>> {
        self.terminals
            .get(self.active_terminal)
            .map(|terminal| &terminal.view)
    }

    pub(super) fn focus_terminal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(view) = self.active_terminal_view() {
            view.read(cx).focus_handle(cx).focus(window, cx);
        }
    }

    /// Shows the terminal, opening one if there is none, and gives it the
    /// keyboard.
    pub(super) fn show_terminal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.layout.show_panel_view(PanelView::Terminal);
        if self.terminals.is_empty() {
            self.new_terminal(window, cx);
            return;
        }
        self.focus_terminal(window, cx);
        self.session_changed(cx);
        cx.notify();
    }

    /// Ctrl+`: shows the terminal, or hides the panel when the terminal is
    /// already in front with the keyboard, as in VS Code.
    pub(super) fn toggle_terminal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let terminal_focused = self
            .active_terminal_view()
            .is_some_and(|view| view.read(cx).focus_handle(cx).contains_focused(window, cx));
        if self.layout.panel && self.layout.panel_view == PanelView::Terminal && terminal_focused {
            self.layout.panel = false;
            self.focus_after_panel(window, cx);
            self.session_changed(cx);
            cx.notify();
        } else {
            self.show_terminal(window, cx);
        }
    }

    /// Gives the keyboard back to the editors once the panel goes away.
    fn focus_after_panel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.panes.is_empty() {
            self.focus_handle.focus(window, cx);
        } else {
            self.focus_pane(self.active, window, cx);
        }
    }

    /// A shell ended: its terminal closes, and the panel with the last one.
    fn terminal_exited(
        &mut self,
        view: &Entity<TerminalView>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(index) = self
            .terminals
            .iter()
            .position(|terminal| &terminal.view == view)
        else {
            return;
        };
        let had_focus = view.read(cx).focus_handle(cx).contains_focused(window, cx);
        self.terminals.remove(index);
        if self.active_terminal >= index && self.active_terminal > 0 {
            self.active_terminal -= 1;
        }
        if self.terminals.is_empty() {
            if self.layout.panel_view == PanelView::Terminal {
                self.layout.panel = false;
                self.session_changed(cx);
            }
            if had_focus {
                self.focus_after_panel(window, cx);
            }
        } else if had_focus {
            self.focus_terminal(window, cx);
        }
        cx.notify();
    }

    /// What the Terminal view of the panel shows.
    pub(super) fn render_terminals(&self) -> Option<AnyElement> {
        let view = self.active_terminal_view()?;
        Some(view.clone().into_any_element())
    }

    /// The name of the terminal in front, for the panel's header.
    pub(super) fn active_terminal_name(&self, cx: &App) -> Option<String> {
        Some(self.active_terminal_view()?.read(cx).name())
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use gpui::TestAppContext;

    use super::*;

    fn script(cwd: PathBuf, script: &str, cx: &App) -> Spawn {
        let mut spawn = terminal_view::shell_spawn(cwd, cx);
        spawn.command = Some(("/bin/sh".into(), vec!["-c".into(), script.into()]));
        spawn
    }

    #[gpui::test]
    fn a_terminal_runs_in_the_panel_and_closes_when_its_shell_ends(cx: &mut TestAppContext) {
        cx.executor().allow_parking();
        cx.update(|cx| {
            gpui_component::init(cx);
            crate::workbench_window::init(cx);
        });
        let folder = tempfile::tempdir().unwrap();
        let base = std::fs::canonicalize(folder.path()).unwrap();
        let settings = Settings::load(base.join("settings.json"), None);
        let opening = Opening::new(Some(base.clone()), None);
        let (view, cx) =
            cx.add_window_view(|window, cx| Workbench::new(opening, settings, None, window, cx));

        view.update_in(cx, |view, window, cx| {
            let spawn = script(base.clone(), "pwd; read line; echo \"eco:$line\"", cx);
            assert!(view.add_terminal(spawn, window, cx));
        });
        let terminal = view.read_with(cx, |view, cx| {
            assert!(view.layout.panel);
            assert_eq!(view.layout.panel_view, PanelView::Terminal);
            view.active_terminal_view()
                .unwrap()
                .read(cx)
                .terminal()
                .clone()
        });
        let wait = |cx: &mut gpui::VisualTestContext, text: &str| {
            let deadline = Instant::now() + Duration::from_secs(10);
            loop {
                cx.run_until_parked();
                if terminal.snapshot().text().contains(text) {
                    return;
                }
                assert!(Instant::now() < deadline, "{}", terminal.snapshot().text());
                std::thread::sleep(Duration::from_millis(20));
            }
        };
        wait(cx, &base.display().to_string());

        // Typed keys go to the shell.
        cx.simulate_keystrokes("c i a o enter");
        wait(cx, "eco:ciao");

        // The shell ended: the terminal and the panel close.
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            cx.run_until_parked();
            if view.read_with(cx, |view, _| view.terminals.is_empty()) {
                break;
            }
            assert!(Instant::now() < deadline, "the terminal did not close");
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(!view.read_with(cx, |view, _| view.layout.panel));
    }

    #[gpui::test]
    fn keys_the_window_uses_elsewhere_reach_the_shell(cx: &mut TestAppContext) {
        cx.executor().allow_parking();
        cx.update(|cx| {
            gpui_component::init(cx);
            crate::workbench_window::init(cx);
        });
        let folder = tempfile::tempdir().unwrap();
        let base = std::fs::canonicalize(folder.path()).unwrap();
        let settings = Settings::load(base.join("settings.json"), None);
        let opening = Opening::new(Some(base.clone()), None);
        let (view, cx) =
            cx.add_window_view(|window, cx| Workbench::new(opening, settings, None, window, cx));
        view.update_in(cx, |view, window, cx| {
            // Prints the bytes of the next four keys in hexadecimal.
            let spawn = script(
                base.clone(),
                "stty raw -echo; echo pronto; dd bs=1 count=4 2>/dev/null | od -An -tx1; sleep 5",
                cx,
            );
            assert!(view.add_terminal(spawn, window, cx));
        });
        let terminal = view.read_with(cx, |view, cx| {
            view.active_terminal_view()
                .unwrap()
                .read(cx)
                .terminal()
                .clone()
        });
        let wait = |cx: &mut gpui::VisualTestContext, text: &str| {
            let deadline = Instant::now() + Duration::from_secs(10);
            loop {
                cx.run_until_parked();
                if terminal.snapshot().text().contains(text) {
                    return;
                }
                assert!(Instant::now() < deadline, "{}", terminal.snapshot().text());
                std::thread::sleep(Duration::from_millis(20));
            }
        };
        wait(cx, "pronto");
        // Tab would move the focus, Ctrl+R open Open Recent, Ctrl+G Go to
        // Line; in a terminal they are the shell's.
        cx.simulate_keystrokes("tab ctrl-r ctrl-g shift-tab");
        wait(cx, "09  12  07  1b");
        let focused = view.update_in(cx, |view, window, cx| {
            view.active_terminal_view()
                .unwrap()
                .read(cx)
                .focus_handle(cx)
                .is_focused(window)
        });
        assert!(focused, "the terminal keeps the keyboard");
    }
}
