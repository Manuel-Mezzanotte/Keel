//! The terminals of the bottom panel: opening them in the project, tabs
//! and terminals side by side, renaming and closing them, and the panel's
//! height.

use gpui::{
    AnyElement, ClickEvent, Div, DragMoveEvent, Entity, Render, StatefulInteractiveElement as _,
    Subscription,
};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::input::{Input, InputState};
use gpui_component::menu::ContextMenuExt as _;
use gpui_component::{Icon, Sizable as _};
use gpui_kit_assets::IconName;
use terminal::Spawn;

use super::claude_code::{OpenClaudeCode, OpenClaudeCodeInPanel};
use super::shell::Colors;
use super::terminal_view::{self, TerminalLook, TerminalView, TerminalViewEvent};
use super::*;

actions!(
    keel,
    [
        NewTerminal,
        SplitTerminal,
        KillTerminal,
        RenameTerminal,
        FocusNextTerminal,
        FocusPreviousTerminal,
        ToggleMaximizedPanel
    ]
);

/// A command on one terminal tab, from its menu.
#[derive(Clone, Debug, PartialEq, gpui::Action)]
#[action(namespace = keel, no_json)]
pub(crate) struct TerminalTabCommand {
    pub index: usize,
    pub command: TerminalCommand,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TerminalCommand {
    Split,
    Rename,
    Kill,
}

/// A terminal of the panel. Terminals with the same group are shown side
/// by side under one tab.
pub(super) struct TerminalTab {
    pub(super) view: Entity<TerminalView>,
    /// The name the user gave it, if any.
    pub(super) name: Option<String>,
    pub(super) group: usize,
    _subscription: Subscription,
}

/// The font size of terminals when the settings do not say, as in VS Code
/// on the Mac.
const TERMINAL_FONT_SIZE: f32 = 12.;

/// The bottom panel's height when the user has not changed it.
pub(super) const PANEL_HEIGHT: f32 = 260.;
const PANEL_MIN_HEIGHT: f32 = 90.;

/// The handle on the panel's top edge, dragged to change its height.
#[derive(Clone)]
pub(super) struct PanelResize;

impl Render for PanelResize {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        gpui::Empty
    }
}

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
            strings: self.strings(),
        }
    }

    /// Where new terminals start: the project, or the home folder.
    pub(super) fn terminal_folder(&self) -> PathBuf {
        self.project
            .root
            .clone()
            .or_else(std::env::home_dir)
            .unwrap_or_else(|| PathBuf::from("/"))
    }

    /// Opens a terminal in the project, under a new tab, and puts it in
    /// front.
    pub(super) fn new_terminal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let spawn = terminal_view::shell_spawn(self.terminal_folder(), cx);
        self.add_terminal(spawn, None, true, window, cx);
    }

    /// Starts a terminal as described: beside the terminals of `group`, or
    /// under a new tab. Returns its view, or `None` with a message when it
    /// cannot start.
    pub(super) fn add_terminal(
        &mut self,
        spawn: Spawn,
        group: Option<usize>,
        focus: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Entity<TerminalView>> {
        let view = self.start_terminal_view(spawn, window, cx)?;
        let subscription = cx.subscribe_in(&view, window, Self::terminal_event);
        let group = group.unwrap_or_else(|| {
            self.next_terminal_group += 1;
            self.next_terminal_group
        });
        // Terminals of a group stay next to each other.
        let index = self
            .terminals
            .iter()
            .rposition(|terminal| terminal.group == group)
            .map_or(self.terminals.len(), |last| last + 1);
        self.terminals.insert(
            index,
            TerminalTab {
                view: view.clone(),
                name: None,
                group,
                _subscription: subscription,
            },
        );
        self.active_terminal = index;
        self.layout.show_panel_view(PanelView::Terminal);
        self.watch_ports(cx);
        if focus {
            self.focus_terminal(window, cx);
        }
        self.session_changed(cx);
        cx.notify();
        Some(view)
    }

    /// Starts the shell and makes its view, or reports why it could not.
    pub(super) fn start_terminal_view(
        &mut self,
        spawn: Spawn,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Entity<TerminalView>> {
        let mut spawn = spawn;
        // Claude Code started here connects to this window.
        spawn.env.extend(self.claude_ide_env());
        let started = match terminal_view::start(spawn) {
            Ok(started) => started,
            Err(error) => {
                let message = self.strings().terminal_failed(&error.to_string());
                self.report(message);
                cx.notify();
                return None;
            }
        };
        let look = self.terminal_look(cx);
        Some(cx.new(|cx| TerminalView::new(started, look, window, cx)))
    }

    pub(super) fn terminal_event(
        &mut self,
        view: &Entity<TerminalView>,
        event: &TerminalViewEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            TerminalViewEvent::Exited => {
                if let Some(index) = self.terminal_index(view) {
                    self.remove_terminal(index, window, cx);
                } else {
                    self.agent_terminal_exited(view, window, cx);
                }
            }
            TerminalViewEvent::TitleChanged => cx.notify(),
            TerminalViewEvent::Focused => {
                if let Some(index) = self.terminal_index(view) {
                    self.active_terminal = index;
                    cx.notify();
                }
            }
            TerminalViewEvent::OpenFile { path, line, column } => {
                self.open_file(path, window, cx);
                if let Some(line) = line {
                    self.go_to(*line, column.unwrap_or(1), window, cx);
                }
            }
            TerminalViewEvent::CommandFinished(command) => {
                self.test_command_finished(view, command, cx);
                self.task_command_finished(view, command, cx);
                self.terminal_command_finished(view, command, window, cx)
            }
            TerminalViewEvent::AskClaude(prompt) => self.ask_claude(prompt.clone(), window, cx),
            TerminalViewEvent::Status(message) => {
                self.status_message = Some(message.clone());
                cx.notify();
            }
            TerminalViewEvent::Notify(text) => {
                self.terminal_wants_you(view, Some(text), window, cx)
            }
            TerminalViewEvent::Bell => self.terminal_wants_you(view, None, window, cx),
        }
    }

    /// Gives a terminal the name its tab shows.
    pub(super) fn name_terminal(&mut self, view: &Entity<TerminalView>, name: String) {
        if let Some(index) = self.terminal_index(view) {
            self.terminals[index].name = Some(name);
        }
    }

    pub(super) fn terminal_index(&self, view: &Entity<TerminalView>) -> Option<usize> {
        self.terminals
            .iter()
            .position(|terminal| &terminal.view == view)
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
            self.panel_maximized = false;
            self.focus_after_panel(window, cx);
            self.session_changed(cx);
            cx.notify();
        } else {
            self.show_terminal(window, cx);
        }
    }

    /// Gives the keyboard back to the editors once a terminal goes away.
    pub(super) fn focus_after_panel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.panes.is_empty() {
            self.focus_handle.focus(window, cx);
        } else {
            self.focus_pane(self.active, window, cx);
        }
    }

    /// Closes a terminal, ending its shell. With the last one the panel
    /// closes too.
    fn remove_terminal(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        if index >= self.terminals.len() {
            return;
        }
        let had_focus = self.terminals[index]
            .view
            .read(cx)
            .focus_handle(cx)
            .contains_focused(window, cx);
        let group = self.terminals[index].group;
        let removed = self.terminals.remove(index);
        self.task_terminal_closed(&removed.view);
        // The neighbour in the same group comes forward, then the tab before.
        let same_group = self
            .terminals
            .iter()
            .position(|terminal| terminal.group == group);
        self.active_terminal = same_group.unwrap_or(index.saturating_sub(1));
        self.active_terminal = self
            .active_terminal
            .min(self.terminals.len().saturating_sub(1));
        if self.terminals.is_empty() {
            if self.layout.panel_view == PanelView::Terminal {
                self.layout.panel = false;
                self.panel_maximized = false;
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

    /// Opens a terminal beside one, in the folder that one is in, as VS
    /// Code's split does.
    fn split_terminal(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(terminal) = self.terminals.get(index) else {
            self.new_terminal(window, cx);
            return;
        };
        let group = terminal.group;
        let folder = terminal.view.read(cx).current_dir();
        let spawn = terminal_view::shell_spawn(folder, cx);
        self.add_terminal(spawn, Some(group), true, window, cx);
    }

    fn rename_terminal(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(terminal) = self.terminals.get(index) else {
            return;
        };
        let current = terminal
            .name
            .clone()
            .unwrap_or_else(|| terminal.view.read(cx).name());
        let placeholder = self.strings().terminal_name;
        let input = cx.new(|cx| {
            let mut state = InputState::new(window, cx).placeholder(placeholder);
            state.set_value(current, window, cx);
            state
        });
        let view = terminal.view.clone();
        let this = cx.weak_entity();
        let field = input.clone();
        // The dialog takes Enter as its OK, which reads the field.
        window.open_dialog(cx, move |dialog, _, _| {
            let this = this.clone();
            let view = view.clone();
            let shown = field.clone();
            let field = field.clone();
            dialog
                .w(px(360.))
                .on_ok(move |_, window, cx| {
                    let typed = field.read(cx).value().trim().to_owned();
                    this.update(cx, |this, cx| {
                        if let Some(index) = this.terminal_index(&view) {
                            this.terminals[index].name = (!typed.is_empty()).then_some(typed);
                            this.focus_terminal(window, cx);
                            cx.notify();
                        }
                    })
                    .ok();
                    true
                })
                .child(Input::new(&shown).small())
        });
        input.update(cx, |input, cx| {
            input.focus(window, cx);
            input.select_all(window, cx);
        });
    }

    /// The tab after (or before) the one in front, round the end.
    fn focus_other_group(&mut self, forward: bool, window: &mut Window, cx: &mut Context<Self>) {
        let groups = self.terminal_groups();
        if groups.len() < 2 {
            return;
        }
        let current = self
            .terminals
            .get(self.active_terminal)
            .and_then(|terminal| {
                groups
                    .iter()
                    .position(|(group, _)| *group == terminal.group)
            })
            .unwrap_or(0);
        let next = if forward {
            (current + 1) % groups.len()
        } else {
            (current + groups.len() - 1) % groups.len()
        };
        self.active_terminal = groups[next].1[0];
        self.focus_terminal(window, cx);
        cx.notify();
    }

    /// The tabs: each group with the indices of its terminals, in order.
    fn terminal_groups(&self) -> Vec<(usize, Vec<usize>)> {
        let mut groups: Vec<(usize, Vec<usize>)> = Vec::new();
        for (index, terminal) in self.terminals.iter().enumerate() {
            match groups.last_mut() {
                Some((group, members)) if *group == terminal.group => members.push(index),
                _ => groups.push((terminal.group, vec![index])),
            }
        }
        groups
    }

    fn terminal_name(&self, index: usize, cx: &App) -> String {
        let terminal = &self.terminals[index];
        terminal
            .name
            .clone()
            .unwrap_or_else(|| terminal.view.read(cx).name())
    }

    fn run_terminal_tab_command(
        &mut self,
        command: &TerminalTabCommand,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match command.command {
            TerminalCommand::Split => self.split_terminal(command.index, window, cx),
            TerminalCommand::Rename => self.rename_terminal(command.index, window, cx),
            TerminalCommand::Kill => self.remove_terminal(command.index, window, cx),
        }
    }

    pub(super) fn toggle_maximized_panel(&mut self, cx: &mut Context<Self>) {
        self.layout.panel = true;
        self.panel_maximized = !self.panel_maximized;
        cx.notify();
    }

    /// The terminal commands of the window.
    pub(super) fn terminal_actions(&self, root: Div, cx: &mut Context<Self>) -> Div {
        root.on_action(
            cx.listener(|this, _: &NewTerminal, window, cx| this.new_terminal(window, cx)),
        )
        .on_action(cx.listener(|this, _: &SplitTerminal, window, cx| {
            this.split_terminal(this.active_terminal, window, cx)
        }))
        .on_action(cx.listener(|this, _: &KillTerminal, window, cx| {
            this.remove_terminal(this.active_terminal, window, cx)
        }))
        .on_action(cx.listener(|this, _: &RenameTerminal, window, cx| {
            this.rename_terminal(this.active_terminal, window, cx)
        }))
        .on_action(cx.listener(|this, _: &FocusNextTerminal, window, cx| {
            this.focus_other_group(true, window, cx)
        }))
        .on_action(cx.listener(|this, _: &FocusPreviousTerminal, window, cx| {
            this.focus_other_group(false, window, cx)
        }))
        .on_action(
            cx.listener(|this, _: &ToggleMaximizedPanel, _, cx| this.toggle_maximized_panel(cx)),
        )
        .on_action(
            cx.listener(|this, command: &TerminalTabCommand, window, cx| {
                this.run_terminal_tab_command(command, window, cx)
            }),
        )
        .on_action(
            cx.listener(|this, _: &OpenClaudeCode, window, cx| this.open_claude_code(window, cx)),
        )
        .on_action(cx.listener(|this, _: &OpenClaudeCodeInPanel, window, cx| {
            this.open_claude_code_in_panel(window, cx)
        }))
    }

    // -----------------------------------------------------------------
    // Drawing

    /// The tabs and buttons of the Terminal view, for the panel's header.
    pub(super) fn render_terminal_header(&self, colors: Colors, cx: &mut Context<Self>) -> Div {
        let strings = self.strings();
        let active_group = self
            .terminals
            .get(self.active_terminal)
            .map(|terminal| terminal.group);
        let focus = self.focus_handle.clone();
        let (danger, warning) = (cx.theme().danger, cx.theme().warning);
        let tabs =
            self.terminal_groups()
                .into_iter()
                .map(|(group, members)| {
                    let first = members[0];
                    let is_active = Some(group) == active_group;
                    let label = members
                        .iter()
                        .map(|&index| self.terminal_name(index, cx))
                        .collect::<Vec<_>>()
                        .join(" | ");
                    let statuses: Vec<_> = members
                        .iter()
                        .map(|&index| self.terminals[index].view.read(cx).status())
                        .collect();
                    let failed = statuses.iter().any(|status| status.failed);
                    let progress = statuses.iter().find_map(|status| status.progress);
                    let entries = [
                        (strings.terminal_split, TerminalCommand::Split),
                        (strings.terminal_rename, TerminalCommand::Rename),
                        (strings.terminal_kill, TerminalCommand::Kill),
                    ];
                    let focus = focus.clone();
                    div()
                        .id(("terminal-tab", group))
                        .relative()
                        .overflow_hidden()
                        .h(px(22.))
                        .pl_2()
                        .pr_1()
                        .flex()
                        .items_center()
                        .gap_1()
                        .rounded(px(6.))
                        .text_xs()
                        .cursor_pointer()
                        .text_color(if is_active { colors.text } else { colors.muted })
                        .when(is_active, |tab| tab.bg(colors.selected))
                        .when(!is_active, |tab| tab.hover(|tab| tab.bg(colors.hover)))
                        .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                            this.active_terminal = first;
                            this.focus_terminal(window, cx);
                            cx.notify();
                        }))
                        .context_menu(move |menu, _, _| {
                            let mut menu = menu.action_context(focus.clone());
                            for (label, command) in entries {
                                menu = menu.menu(
                                    label,
                                    Box::new(TerminalTabCommand {
                                        index: first,
                                        command,
                                    }),
                                );
                            }
                            menu
                        })
                        .child(
                            div()
                                .relative()
                                .child(Icon::new(IconName::SquareTerminal).xsmall())
                                // The last command failed: a red dot, as on
                                // VS Code's tabs.
                                .when(failed, |icon| {
                                    icon.child(
                                        div()
                                            .absolute()
                                            .top(px(-2.))
                                            .right(px(-2.))
                                            .size(px(6.))
                                            .rounded_full()
                                            .bg(danger),
                                    )
                                }),
                        )
                        .child(div().max_w(px(220.)).truncate().child(label))
                        .children(progress.map(|progress| {
                            render_progress(progress, colors.accent, danger, warning)
                        }))
                        .child(
                            Button::new(("kill-terminal", group))
                                .ghost()
                                .small()
                                .icon(Icon::new(IconName::X).xsmall())
                                .tooltip(strings.terminal_kill)
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    // Closing a tab ends every terminal in it.
                                    while let Some(index) = this
                                        .terminals
                                        .iter()
                                        .position(|terminal| terminal.group == group)
                                    {
                                        this.remove_terminal(index, window, cx);
                                    }
                                })),
                        )
                })
                .collect::<Vec<_>>();
        div()
            .ml_auto()
            .flex()
            .items_center()
            .gap_1()
            .children(tabs)
            .child(
                Button::new("new-terminal")
                    .ghost()
                    .small()
                    .icon(Icon::new(IconName::Plus))
                    .tooltip(strings.terminal_new)
                    .on_click(cx.listener(|this, _, window, cx| this.new_terminal(window, cx))),
            )
            .child(
                Button::new("split-terminal")
                    .ghost()
                    .small()
                    .icon(Icon::new(IconName::Columns2))
                    .tooltip(strings.terminal_split)
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.split_terminal(this.active_terminal, window, cx)
                    })),
            )
    }

    /// The terminals of the tab in front, side by side.
    pub(super) fn render_terminals(&self, colors: Colors) -> Option<AnyElement> {
        let group = self.terminals.get(self.active_terminal)?.group;
        let views = self
            .terminals
            .iter()
            .filter(|terminal| terminal.group == group)
            .enumerate()
            .map(|(position, terminal)| {
                div()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .when(position > 0, |pane| {
                        pane.border_l_1().border_color(colors.border)
                    })
                    .child(terminal.view.clone())
            });
        Some(
            div()
                .size_full()
                .flex()
                .flex_row()
                .children(views)
                .into_any_element(),
        )
    }

    /// The thin strip on the panel's top edge that changes its height.
    pub(super) fn render_panel_resize(&self, colors: Colors) -> impl IntoElement {
        div()
            .id("panel-resize")
            .absolute()
            .top_0()
            .left_0()
            .right_0()
            .h(px(5.))
            .cursor_row_resize()
            .hover(move |handle| handle.bg(colors.accent.opacity(0.4)))
            .on_drag(PanelResize, |resize, _, _, cx| cx.new(|_| resize.clone()))
    }

    /// Follows the drag of the panel's edge within the editors' area.
    pub(super) fn resize_panel(
        &mut self,
        event: &DragMoveEvent<PanelResize>,
        cx: &mut Context<Self>,
    ) {
        let bounds = event.bounds;
        let height = f32::from(bounds.bottom() - event.event.position.y);
        let room = f32::from(bounds.size.height) - 80.;
        self.panel_height = height.clamp(PANEL_MIN_HEIGHT, room.max(PANEL_MIN_HEIGHT));
        self.panel_maximized = false;
        self.session_changed(cx);
        cx.notify();
    }
}

/// A program's progress (OSC 9;4) along the bottom of its tab, as
/// Windows Terminal and Ghostty show it.
fn render_progress(
    progress: terminal::Progress,
    accent: gpui::Hsla,
    danger: gpui::Hsla,
    warning: gpui::Hsla,
) -> AnyElement {
    use gpui::{AnimationExt as _, relative};
    use terminal::Progress;
    let bar = div()
        .absolute()
        .left_0()
        .bottom_0()
        .h(px(2.))
        .rounded_full();
    match progress {
        Progress::Normal(percent) => bar
            .w(relative(f32::from(percent) / 100.))
            .bg(accent)
            .into_any_element(),
        Progress::Error(percent) => bar
            .w(relative(f32::from(percent.max(8)) / 100.))
            .bg(danger)
            .into_any_element(),
        Progress::Paused(percent) => bar
            .w(relative(f32::from(percent.max(8)) / 100.))
            .bg(warning)
            .into_any_element(),
        Progress::Indeterminate => bar
            .w(relative(0.3))
            .bg(accent)
            .with_animation(
                "terminal-progress",
                gpui::Animation::new(std::time::Duration::from_millis(1400)).repeat(),
                |bar, delta| bar.left(relative(delta * 1.3 - 0.3)),
            )
            .into_any_element(),
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use gpui::{TestAppContext, VisualTestContext};

    use super::*;

    fn script(cwd: PathBuf, script: &str, cx: &App) -> Spawn {
        let mut spawn = terminal_view::shell_spawn(cwd, cx);
        spawn.command = Some(("/bin/sh".into(), vec!["-c".into(), script.into()]));
        spawn
    }

    fn window(
        cx: &mut TestAppContext,
    ) -> (
        tempfile::TempDir,
        PathBuf,
        Entity<Workbench>,
        &mut VisualTestContext,
    ) {
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
        (folder, base, view, cx)
    }

    fn wait_until(
        cx: &mut VisualTestContext,
        what: &str,
        check: impl Fn(&mut VisualTestContext) -> bool,
    ) {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            cx.run_until_parked();
            if check(cx) {
                return;
            }
            assert!(Instant::now() < deadline, "gave up waiting for {what}");
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    fn screen(view: &Entity<Workbench>, index: usize, cx: &mut VisualTestContext) -> String {
        view.read_with(cx, |view, cx| {
            view.terminals[index]
                .view
                .read(cx)
                .terminal()
                .snapshot()
                .text()
        })
    }

    #[gpui::test]
    fn a_terminal_runs_in_the_panel_and_closes_when_its_shell_ends(cx: &mut TestAppContext) {
        let (_folder, base, view, cx) = window(cx);
        view.update_in(cx, |view, window, cx| {
            let spawn = script(
                base.clone(),
                "pwd; read line; echo \"eco:$line\"; sleep 1",
                cx,
            );
            assert!(view.add_terminal(spawn, None, true, window, cx).is_some());
        });
        view.read_with(cx, |view, _| {
            assert!(view.layout.panel);
            assert_eq!(view.layout.panel_view, PanelView::Terminal);
        });
        let folder = base.display().to_string();
        wait_until(cx, "the folder", |cx| {
            screen(&view, 0, cx).contains(&folder)
        });

        // Typed keys go to the shell.
        cx.simulate_keystrokes("c i a o enter");
        wait_until(cx, "the echo", |cx| {
            screen(&view, 0, cx).contains("eco:ciao")
        });

        // The shell ended: the terminal and the panel close.
        wait_until(cx, "the terminal to close", |cx| {
            view.read_with(cx, |view, _| view.terminals.is_empty())
        });
        assert!(!view.read_with(cx, |view, _| view.layout.panel));
    }

    #[gpui::test]
    fn keys_the_window_uses_elsewhere_reach_the_shell(cx: &mut TestAppContext) {
        let (_folder, base, view, cx) = window(cx);
        view.update_in(cx, |view, window, cx| {
            // Prints the bytes of the next four keys in hexadecimal.
            let spawn = script(
                base.clone(),
                "stty raw -echo; echo pronto; dd bs=1 count=4 2>/dev/null | od -An -tx1; sleep 5",
                cx,
            );
            assert!(view.add_terminal(spawn, None, true, window, cx).is_some());
        });
        wait_until(cx, "the prompt", |cx| {
            screen(&view, 0, cx).contains("pronto")
        });
        // Tab would move the focus, Ctrl+R open Open Recent, Ctrl+G Go to
        // Line; in a terminal they are the shell's.
        cx.simulate_keystrokes("tab ctrl-r ctrl-g shift-tab");
        wait_until(cx, "the bytes", |cx| {
            screen(&view, 0, cx).contains("09  12  07  1b")
        });
        let focused = view.update_in(cx, |view, window, cx| {
            view.active_terminal_view()
                .unwrap()
                .read(cx)
                .focus_handle(cx)
                .is_focused(window)
        });
        assert!(focused, "the terminal keeps the keyboard");
    }

    #[gpui::test]
    fn tabs_split_rename_and_close(cx: &mut TestAppContext) {
        let (_folder, base, view, cx) = window(cx);
        let sleeper =
            |view: &mut Workbench, group, window: &mut Window, cx: &mut Context<Workbench>| {
                let spawn = script(base.clone(), "sleep 30", cx);
                view.add_terminal(spawn, group, true, window, cx).unwrap();
            };
        view.update_in(cx, |view, window, cx| {
            sleeper(view, None, window, cx);
            sleeper(view, None, window, cx);
            let first_group = view.terminals[0].group;
            // A split goes beside the first terminal, under its tab.
            sleeper(view, Some(first_group), window, cx);
        });
        view.read_with(cx, |view, _| {
            let groups = view.terminal_groups();
            assert_eq!(groups.len(), 2);
            assert_eq!(groups[0].1, [0, 1]);
            assert_eq!(view.active_terminal, 1);
        });

        // The next tab, round the end.
        view.update_in(cx, |view, window, cx| {
            view.focus_other_group(true, window, cx)
        });
        assert_eq!(view.read_with(cx, |view, _| view.active_terminal), 2);

        view.update_in(cx, |view, _, cx| {
            view.terminals[2].name = Some("server".into());
            assert_eq!(view.terminal_name(2, cx), "server");
        });

        // Closing the split leaves its neighbour in front.
        view.update_in(cx, |view, window, cx| view.remove_terminal(1, window, cx));
        view.read_with(cx, |view, _| {
            assert_eq!(view.terminals.len(), 2);
            assert_eq!(view.active_terminal, 0);
        });
        view.update_in(cx, |view, window, cx| {
            view.remove_terminal(0, window, cx);
            view.remove_terminal(0, window, cx);
        });
        assert!(!view.read_with(cx, |view, _| view.layout.panel));
    }
}
