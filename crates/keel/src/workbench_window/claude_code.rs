//! Claude Code in a terminal: on the right, where its own panel comes in
//! phase 10, or as a tab of the bottom panel. Cmd+Esc opens it, as the
//! Claude Code extension does in VS Code.

use gpui::{
    AnyElement, DragMoveEvent, Entity, Render, StatefulInteractiveElement as _, Subscription,
};

use super::shell::Colors;
use super::terminal_view::{self, TerminalView};
use super::*;

actions!(keel, [OpenClaudeCode, OpenClaudeCodeInPanel]);

/// The command a Claude Code terminal runs.
const CLAUDE: &str = "claude";

/// Claude Code's panel on the right, before it holds a terminal.
pub(super) const AGENT_WIDTH: f32 = 320.;
/// Wide enough for Claude Code's screen, about 80 columns.
const AGENT_TERMINAL_WIDTH: f32 = 600.;
const AGENT_MIN_WIDTH: f32 = 240.;

/// The handle on the left edge of Claude Code's panel.
#[derive(Clone)]
pub(super) struct AgentResize;

impl Render for AgentResize {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        gpui::Empty
    }
}

/// The terminal running Claude Code on the right.
pub(super) struct AgentTerminal {
    pub(super) view: Entity<TerminalView>,
    _subscription: Subscription,
}

impl Workbench {
    /// Cmd+Esc: opens Claude Code in a terminal on the right, or gives it
    /// the keyboard when it is open.
    pub(super) fn open_claude_code(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.layout.agent = true;
        if self.agent_terminal.is_none() {
            let spawn = terminal_view::shell_spawn(self.terminal_folder(), cx);
            if !self.start_agent_terminal(spawn, window, cx) {
                return;
            }
        }
        if let Some(agent) = &self.agent_terminal {
            agent.view.read(cx).focus_handle(cx).focus(window, cx);
        }
        self.session_changed(cx);
        cx.notify();
    }

    /// Starts the terminal on the right and runs Claude Code in it, with
    /// the panel wide enough for its screen.
    fn start_agent_terminal(
        &mut self,
        spawn: terminal::Spawn,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(view) = self.start_terminal_view(spawn, window, cx) else {
            return false;
        };
        view.read(cx).run(CLAUDE);
        let subscription = cx.subscribe_in(&view, window, Self::terminal_event);
        self.agent_terminal = Some(AgentTerminal {
            view,
            _subscription: subscription,
        });
        self.agent_width = self.agent_width.max(AGENT_TERMINAL_WIDTH);
        true
    }

    /// Claude Code in a new tab of the bottom panel.
    pub(super) fn open_claude_code_in_panel(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let spawn = terminal_view::shell_spawn(self.terminal_folder(), cx);
        if let Some(view) = self.add_terminal(spawn, None, true, window, cx) {
            view.read(cx).run(CLAUDE);
        }
    }

    /// The shell of the terminal on the right ended: the panel shows how to
    /// open it again.
    pub(super) fn agent_terminal_exited(
        &mut self,
        view: &Entity<TerminalView>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(agent) = &self.agent_terminal else {
            return;
        };
        if &agent.view != view {
            return;
        }
        let had_focus = view.read(cx).focus_handle(cx).contains_focused(window, cx);
        self.agent_terminal = None;
        if had_focus {
            self.focus_after_panel(window, cx);
        }
        cx.notify();
    }

    /// Ends Claude Code's terminal on the right.
    pub(super) fn close_agent_terminal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(agent) = &self.agent_terminal {
            let view = agent.view.clone();
            self.agent_terminal_exited(&view, window, cx);
        }
    }

    pub(super) fn render_agent_terminal(&self) -> Option<AnyElement> {
        let agent = self.agent_terminal.as_ref()?;
        Some(agent.view.clone().into_any_element())
    }

    /// The strip on the left edge of Claude Code's panel that changes its
    /// width.
    pub(super) fn render_agent_resize(&self, colors: Colors) -> impl IntoElement {
        div()
            .id("agent-resize")
            .absolute()
            .left(px(-5.))
            .top_0()
            .bottom_0()
            .w(px(6.))
            .cursor_col_resize()
            .hover(move |handle| handle.bg(colors.accent.opacity(0.4)))
            .on_drag(AgentResize, |resize, _, _, cx| cx.new(|_| resize.clone()))
    }

    /// Follows the drag of the panel's edge; `bounds` is the row of panels.
    pub(super) fn resize_agent(
        &mut self,
        event: &DragMoveEvent<AgentResize>,
        cx: &mut Context<Self>,
    ) {
        let bounds = event.bounds;
        let width = f32::from(bounds.right() - event.event.position.x) - super::shell::GAP;
        let room = f32::from(bounds.size.width) * 0.7;
        self.agent_width = width.clamp(AGENT_MIN_WIDTH, room.max(AGENT_MIN_WIDTH));
        self.session_changed(cx);
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use gpui::TestAppContext;

    use super::*;

    #[gpui::test]
    fn claude_code_opens_on_the_right_and_its_panel_widens(cx: &mut TestAppContext) {
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
            // A shell that shows what it was asked to run, instead of the
            // real Claude Code.
            let mut spawn = terminal_view::shell_spawn(base.clone(), cx);
            spawn.command = Some((
                "/bin/sh".into(),
                vec![
                    "-c".into(),
                    "read line; echo \"ran:$line\"; sleep 30".into(),
                ],
            ));
            assert!(view.start_agent_terminal(spawn, window, cx));
            view.layout.agent = false;
            // Cmd+Esc shows the panel, keeps the terminal and gives it the keyboard.
            view.open_claude_code(window, cx);
            assert!(view.layout.agent);
            assert!(view.agent_width >= AGENT_TERMINAL_WIDTH);
            let focused = view
                .agent_terminal
                .as_ref()
                .unwrap()
                .view
                .read(cx)
                .focus_handle(cx)
                .is_focused(window);
            assert!(focused);
        });
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            cx.run_until_parked();
            let text = view.read_with(cx, |view, cx| {
                let agent = view.agent_terminal.as_ref().unwrap();
                agent.view.read(cx).terminal().snapshot().text()
            });
            if text.contains("ran:claude") {
                break;
            }
            assert!(Instant::now() < deadline, "{text}");
            std::thread::sleep(Duration::from_millis(20));
        }
        view.update_in(cx, |view, window, cx| view.close_agent_terminal(window, cx));
        assert!(view.read_with(cx, |view, _| view.agent_terminal.is_none()));
    }
}
