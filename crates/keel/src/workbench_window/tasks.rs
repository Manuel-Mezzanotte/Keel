//! VS Code's tasks: Cmd+Shift+B compiles, «Attività: Esegui» runs any of
//! the project's, from `.vscode/tasks.json` or found by Keel. They run in
//! the terminal, as if typed there; their problem matchers fill the
//! Problems view. Tasks that start by themselves when a folder opens are
//! left alone: a project's file never runs anything unless you ask.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::path::{Component, Path, PathBuf};
use std::rc::Rc;

use futures::FutureExt as _;
use futures::channel::oneshot;
use futures::future::{LocalBoxFuture, Shared, join_all};
use gpui::{AsyncWindowContext, Div, Subscription, WeakEntity};
use gpui_component::input::{Input, InputState};
use gpui_component::{Sizable as _, WindowExt as _};
use lsp::lsp_types::{Diagnostic, NumberOrString, Position, Range as LspRange};
use terminal::ShellCommand;

use super::problem_matchers::{AppliesTo, Found, Matcher};
use super::task_config::{
    self, Group, PlanError, TaskDef, TaskFile, TaskInput, Variables, with_dependencies,
};
use super::terminal_view::TerminalView;
use super::*;
use crate::quick_pick::{self, PickItem};

actions!(
    keel,
    [
        RunTask,
        RunBuildTask,
        RunTestTask,
        RerunLastTask,
        TerminateTask,
        ConfigureTasks
    ]
);

/// The tasks' problems, beside the language servers'.
pub(super) const TASKS: &str = "task";
/// How long a task that never ends is quiet before its output is read.
const BACKGROUND_PAUSE: Duration = Duration::from_millis(350);
/// Runs remembered, to find their terminals again.
const KEPT_RUNS: usize = 40;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum RunState {
    Running,
    /// A task that never ends, between one round and the next.
    Waiting,
    Ended(Option<i32>),
}

pub(super) struct TaskRun {
    id: u64,
    pub(super) label: String,
    pub(super) command: String,
    cwd: PathBuf,
    env: Vec<(String, String)>,
    terminal: WeakEntity<TerminalView>,
    pub(super) state: RunState,
    matchers: Vec<Matcher>,
    /// Where relative files may start from.
    bases: Vec<PathBuf>,
    done: Option<oneshot::Sender<bool>>,
    reading: Option<Task<()>>,
    /// The output read last, not to read it again.
    read: usize,
    _watch: Option<Subscription>,
}

#[derive(Default)]
pub(super) struct Tasks {
    pub(super) runs: Vec<TaskRun>,
    next_id: u64,
    last: Option<String>,
    /// Labels run lately, newest first.
    recent: Vec<String>,
}

/// A task with its variables filled in, ready to run.
struct Prepared {
    label: String,
    command: Option<String>,
    cwd: PathBuf,
    env: Vec<(String, String)>,
    matchers: Vec<Matcher>,
    bases: Vec<PathBuf>,
    depends_on: Vec<String>,
    in_sequence: bool,
    background: bool,
    reveal: bool,
    focus: bool,
}

type Outcome = Shared<LocalBoxFuture<'static, bool>>;

/// A path without its `.` and `..`.
fn cleaned(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for part in path.components() {
        match part {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            part => out.push(part),
        }
    }
    out
}

/// A command as one line to type: with a line break or another control
/// character in it, the shell would take it as several commands.
pub(super) fn one_line(command: &str) -> String {
    let command = command.trim();
    if !command.chars().any(char::is_control) {
        return command.to_owned();
    }
    let mut quoted = String::from("eval $'");
    for char in command.chars() {
        match char {
            '\n' => quoted.push_str("\\n"),
            '\t' => quoted.push_str("\\t"),
            '\\' => quoted.push_str("\\\\"),
            '\'' => quoted.push_str("\\'"),
            char if char.is_control() => quoted.push_str(&format!("\\u{:04x}", char as u32)),
            char => quoted.push(char),
        }
    }
    quoted.push('\'');
    quoted
}

/// The last round of a task that never ends, if it is over: from the last
/// line that begins one to the first after it that ends it.
fn last_round(output: &str, matchers: &[Matcher]) -> Option<String> {
    let background: Vec<_> = matchers
        .iter()
        .filter_map(|matcher| matcher.background.as_ref())
        .collect();
    let lines: Vec<&str> = output.lines().collect();
    let any = |line: &str, ends: bool| {
        background.iter().any(|background| {
            let edge = if ends {
                &background.ends
            } else {
                &background.begins
            };
            edge.as_ref().is_some_and(|edge| edge.is_match(line))
        })
    };
    let begin = lines.iter().rposition(|line| any(line, false));
    let begin = match begin {
        Some(begin) => begin,
        None if background
            .iter()
            .any(|background| background.active_on_start) =>
        {
            0
        }
        None if background
            .iter()
            .all(|background| background.begins.is_none()) =>
        {
            0
        }
        None => return None,
    };
    let end = lines[begin..].iter().position(|line| any(line, true))?;
    Some(lines[begin..=begin + end].join("\n"))
}

/// Runs a task after those it depends on, each once; `false` when one
/// failed.
fn execute(
    label: String,
    plan: Rc<HashMap<String, Prepared>>,
    runs: Rc<RefCell<HashMap<String, Outcome>>>,
    this: WeakEntity<Workbench>,
    cx: AsyncWindowContext,
) -> Outcome {
    if let Some(running) = runs.borrow().get(&label) {
        return running.clone();
    }
    let known = runs.clone();
    let name = label.clone();
    let outcome = async move {
        let Some(task) = plan.get(&label) else {
            return false;
        };
        let start = |wanted: &String| {
            execute(
                wanted.clone(),
                plan.clone(),
                runs.clone(),
                this.clone(),
                cx.clone(),
            )
        };
        if task.in_sequence {
            for wanted in &task.depends_on {
                if !start(wanted).await {
                    return false;
                }
            }
        } else {
            let all = join_all(task.depends_on.iter().map(start)).await;
            if all.contains(&false) {
                return false;
            }
        }
        if task.command.is_none() {
            return true;
        }
        let mut cx = cx.clone();
        let started = this
            .update_in(&mut cx, |this, window, cx| {
                this.start_task(task, window, cx)
            })
            .ok()
            .flatten();
        match started {
            Some(done) => done.await.unwrap_or(false),
            None => false,
        }
    }
    .boxed_local()
    .shared();
    known.borrow_mut().insert(name, outcome.clone());
    outcome
}

impl Workbench {
    pub(super) fn task_actions(&self, root: Div, cx: &mut Context<Self>) -> Div {
        root.on_action(
            cx.listener(|this, _: &RunTask, window, cx| this.choose_task(None, window, cx)),
        )
        .on_action(cx.listener(|this, _: &RunBuildTask, window, cx| {
            this.run_group(Group::Build, window, cx)
        }))
        .on_action(
            cx.listener(|this, _: &RunTestTask, window, cx| {
                this.run_group(Group::Test, window, cx)
            }),
        )
        .on_action(cx.listener(
            |this, _: &RerunLastTask, window, cx| match this.tasks.last.clone() {
                Some(label) => this.run_task(&label, window, cx),
                None => this.choose_task(None, window, cx),
            },
        ))
        .on_action(
            cx.listener(|this, _: &TerminateTask, window, cx| this.terminate_task(window, cx)),
        )
        .on_action(
            cx.listener(|this, _: &ConfigureTasks, window, cx| this.configure_tasks(window, cx)),
        )
    }

    fn tasks_file(&self) -> Option<PathBuf> {
        Some(self.project.root.as_ref()?.join(".vscode/tasks.json"))
    }

    /// The project's tasks: the file's, then those found that it does not
    /// have.
    fn project_tasks(&mut self, cx: &mut Context<Self>) -> TaskFile {
        let Some(root) = self.project.root.clone() else {
            return TaskFile::default();
        };
        let mut file = TaskFile::default();
        // An open tasks.json is read as it is in its tab.
        let path = root.join(".vscode/tasks.json");
        let uri = Uri::file(&path).to_string();
        let text = match self.documents.get(&uri) {
            Some(document) => Some(document.synced_text.clone()),
            None => std::fs::read_to_string(&path).ok(),
        };
        if let Some(text) = text {
            match task_config::read(&text, &root) {
                Ok(read) => file = read,
                Err(error) => self.task_problem(
                    notices::fill(self.strings().tasks_bad_file, &[("error", &error)]),
                    cx,
                ),
            }
        }
        for task in task_config::found(&root) {
            if !file.tasks.iter().any(|known| known.label == task.label) {
                file.tasks.push(task);
            }
        }
        file
    }

    fn task_problem(&mut self, message: String, cx: &mut Context<Self>) {
        self.report(message);
        cx.notify();
    }

    /// Cmd+Shift+B and Run Test Task: the group's default task, or its
    /// only one, or a choice between them.
    fn run_group(&mut self, group: Group, window: &mut Window, cx: &mut Context<Self>) {
        let file = self.project_tasks(cx);
        let of_group: Vec<&TaskDef> = file
            .tasks
            .iter()
            .filter(|task| task.group == group && !task.hidden)
            .collect();
        let defaults: Vec<&&TaskDef> = of_group.iter().filter(|task| task.default).collect();
        let written: Vec<&&TaskDef> = of_group
            .iter()
            .filter(|task| task.source == "tasks.json")
            .collect();
        let chosen = match (defaults.as_slice(), written.as_slice(), of_group.as_slice()) {
            ([only], _, _) | ([], [only], _) => Some(only.label.clone()),
            ([], [], [only]) => Some(only.label.clone()),
            _ => None,
        };
        match chosen {
            Some(label) => self.run_task(&label, window, cx),
            None => self.choose_task(Some(group), window, cx),
        }
    }

    /// «Attività: Esegui»: a list, with those run lately first.
    fn choose_task(&mut self, group: Option<Group>, window: &mut Window, cx: &mut Context<Self>) {
        let file = self.project_tasks(cx);
        let mut tasks: Vec<TaskDef> = file
            .tasks
            .iter()
            .filter(|task| !task.hidden && group.is_none_or(|group| task.group == group))
            .cloned()
            .collect();
        // A group with nothing in it: every task, to choose one anyway.
        if tasks.is_empty() && group.is_some() {
            tasks = file.tasks.into_iter().filter(|task| !task.hidden).collect();
        }
        let strings = self.strings();
        if tasks.is_empty() {
            self.task_problem(strings.tasks_none.to_owned(), cx);
            return;
        }
        let recent = &self.tasks.recent;
        tasks.sort_by_key(|task| {
            recent
                .iter()
                .position(|label| *label == task.label)
                .unwrap_or(usize::MAX)
        });
        let items = tasks
            .iter()
            .map(|task| {
                let mut item = PickItem::new(task.label.clone());
                item.detail = Some(match &task.detail {
                    Some(detail) => format!("{} · {detail}", task.source),
                    None => task.source.to_owned(),
                });
                item
            })
            .collect();
        let placeholder = match group {
            Some(Group::Build) => strings.tasks_pick_build,
            Some(Group::Test) => strings.tasks_pick_test,
            _ => strings.tasks_pick,
        };
        let chosen = quick_pick::show(None, Some(placeholder.to_owned()), items, window, cx);
        cx.spawn_in(window, async move |this, cx| {
            let Some(index) = chosen.await.ok().flatten() else {
                return;
            };
            this.update_in(cx, |this, window, cx| {
                this.run_task(&tasks[index].label, window, cx)
            })
            .ok();
        })
        .detach();
    }

    /// Runs a task and those it depends on, after asking what they ask.
    pub(super) fn run_task(&mut self, label: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.task_outcome(label, window, cx).detach();
    }

    /// Runs a task; `true` when it and those it depends on went well.
    pub(super) fn task_outcome(
        &mut self,
        label: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Task<bool> {
        let file = self.project_tasks(cx);
        // The group's default, by its variable.
        let default = file
            .tasks
            .iter()
            .find(|task| task.group == Group::Build && task.default)
            .map(|task| task.label.clone());
        let label = match (label, &default) {
            ("${defaultBuildTask}", Some(default)) => default.as_str(),
            (label, _) => label,
        };
        let strings = self.strings();
        let plan = match with_dependencies(label, &file.tasks) {
            Ok(plan) => plan,
            Err(PlanError::Cycle(label)) => {
                let text = notices::fill(strings.tasks_cycle, &[("task", &label)]);
                self.task_problem(text, cx);
                return Task::ready(false);
            }
            Err(PlanError::Missing { wanted, by }) => {
                let text = if by.is_empty() {
                    notices::fill(strings.tasks_missing, &[("task", &wanted)])
                } else {
                    notices::fill(
                        strings.tasks_missing_dependency,
                        &[("task", &by), ("wanted", &wanted)],
                    )
                };
                self.task_problem(text, cx);
                return Task::ready(false);
            }
        };
        self.tasks.last = Some(label.to_owned());
        self.tasks.recent.retain(|known| known != label);
        self.tasks.recent.insert(0, label.to_owned());
        self.tasks.recent.truncate(20);
        let mut asked: Vec<String> = Vec::new();
        for id in plan.iter().flat_map(TaskDef::inputs) {
            if !asked.contains(&id) {
                asked.push(id);
            }
        }
        let label = label.to_owned();
        cx.spawn_in(window, async move |this, cx| {
            let mut inputs = HashMap::new();
            for id in asked {
                let known = file.inputs.iter().find(|input| input.id == id).cloned();
                // An input it does not describe is asked as a text.
                let input = known.unwrap_or(TaskInput {
                    description: id.clone(),
                    id: id.clone(),
                    default: None,
                    options: Vec::new(),
                });
                let Ok(answer) = this.update_in(cx, |this, window, cx| {
                    this.ask_task_input(&input, window, cx)
                }) else {
                    return false;
                };
                // Esc: nothing runs.
                let Some(answer) = answer.await.ok().flatten() else {
                    return false;
                };
                inputs.insert(id, answer);
            }
            let Ok(Some(prepared)) = this.update_in(cx, |this, window, cx| {
                this.prepare_tasks(plan, &file, inputs, window, cx)
            }) else {
                return false;
            };
            let runs = Rc::default();
            execute(label, Rc::new(prepared), runs, this.clone(), cx.clone()).await
        })
    }

    /// Asks what an input wants: one of its values, or a text.
    pub(super) fn ask_task_input(
        &mut self,
        input: &TaskInput,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> oneshot::Receiver<Option<String>> {
        let (sender, receiver) = oneshot::channel();
        let title = Some(input.description.clone()).filter(|title| !title.is_empty());
        if !input.options.is_empty() {
            let mut options = input.options.clone();
            // The default first, where Enter takes it.
            if let Some(default) = &input.default {
                options.sort_by_key(|(_, value)| value != default);
            }
            let items = options
                .iter()
                .map(|(label, value)| {
                    let mut item = PickItem::new(label.clone());
                    item.detail = (label != value).then(|| value.clone());
                    item
                })
                .collect();
            let chosen = quick_pick::show(None, title, items, window, cx);
            cx.spawn(async move |_, _| {
                let index = chosen.await.ok().flatten();
                let value = index.and_then(|index| Some(options.get(index)?.1.clone()));
                let _ = sender.send(value);
            })
            .detach();
            return receiver;
        }
        let placeholder = title.unwrap_or_else(|| input.id.clone());
        let default = input.default.clone().unwrap_or_default();
        let field = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(placeholder.clone())
                .default_value(default)
        });
        // Dropped with the dialog when it closes without an answer.
        let sender = Rc::new(RefCell::new(Some(sender)));
        let shown = field.clone();
        window.open_dialog(cx, move |dialog, _, _| {
            let sender = sender.clone();
            let read = shown.clone();
            dialog
                .w(px(420.))
                .title(placeholder.clone())
                .on_ok(move |_, _, cx| {
                    if let Some(sender) = sender.borrow_mut().take() {
                        let _ = sender.send(Some(read.read(cx).value().to_string()));
                    }
                    true
                })
                .child(Input::new(&shown).small())
        });
        field.update(cx, |field, cx| field.focus(window, cx));
        receiver
    }

    /// What a task's variables stand for now.
    pub(super) fn task_variables(
        &self,
        file: &TaskFile,
        inputs: HashMap<String, String>,
        cx: &App,
    ) -> Variables {
        let pane = self.panes.get(self.active);
        let document = pane.and_then(|pane| self.documents.get(&pane.uri));
        let mut variables = Variables {
            root: self.terminal_folder(),
            file: document
                .and_then(|document| document.disk_path())
                .map(Path::to_path_buf),
            line: 1,
            column: 1,
            selected: String::new(),
            inputs,
            config: HashMap::new(),
            default_build: file
                .tasks
                .iter()
                .find(|task| task.group == Group::Build && task.default)
                .map(|task| task.label.clone()),
        };
        if let Some(pane) = pane {
            let editor = pane.editor.read(cx);
            let cursor = editor.cursor_position();
            variables.line = cursor.line as usize + 1;
            variables.column = cursor.character as usize + 1;
            let text = editor.value().to_string();
            variables.selected = text
                .get(editor.selected_range())
                .unwrap_or_default()
                .to_owned();
        }
        for (key, value) in self.settings.merged() {
            let value = match value {
                Value::String(text) => text,
                Value::Number(number) => number.to_string(),
                Value::Bool(value) => value.to_string(),
                _ => continue,
            };
            variables.config.insert(key, value);
        }
        variables
    }

    /// Fills in the tasks of a plan; `None` after saying what is wrong.
    fn prepare_tasks(
        &mut self,
        plan: Vec<TaskDef>,
        file: &TaskFile,
        inputs: HashMap<String, String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<HashMap<String, Prepared>> {
        // As VS Code's `task.saveBeforeRun`: the task sees what you wrote.
        if self.settings.string("task.saveBeforeRun") != Some("never") {
            self.save_all_quietly(window, cx);
        }
        let variables = self.task_variables(file, inputs, cx);
        let strings = self.strings();
        let root = variables.root.clone();
        let mut prepared = HashMap::new();
        for task in plan {
            let filled = (|| {
                let cwd = match &task.cwd {
                    Some(cwd) => cleaned(&root.join(variables.expand(cwd)?)),
                    None => root.clone(),
                };
                let env = task
                    .env
                    .iter()
                    .map(|(name, value)| Ok((name.clone(), variables.expand(value)?)))
                    .collect::<Result<Vec<_>, String>>()?;
                let mut bases = Vec::new();
                for base in task
                    .matchers
                    .iter()
                    .filter_map(|matcher| matcher.base.as_ref())
                {
                    bases.push(cleaned(&root.join(variables.expand(base)?)));
                }
                bases.push(cwd.clone());
                bases.push(root.clone());
                Ok::<_, String>((task.command_line(&variables)?, cwd, env, bases))
            })();
            let (command, cwd, env, bases) = match filled {
                Ok(filled) => filled,
                Err(variable) => {
                    let text = notices::fill(
                        strings.tasks_unknown_variable,
                        &[("task", &task.label), ("variable", &variable)],
                    );
                    self.task_problem(text, cx);
                    return None;
                }
            };
            if let Some(unread) = task.unread.first() {
                let text = notices::fill(
                    strings.tasks_bad_matcher,
                    &[("task", &task.label), ("matcher", unread)],
                );
                self.task_problem(text, cx);
            }
            prepared.insert(
                task.label.clone(),
                Prepared {
                    label: task.label,
                    command: command.map(|command| one_line(&command)),
                    cwd,
                    env,
                    matchers: task.matchers,
                    bases,
                    depends_on: task.depends_on,
                    in_sequence: task.in_sequence,
                    background: task.background,
                    reveal: task.reveal,
                    focus: task.focus,
                },
            );
        }
        Some(prepared)
    }

    /// Types a task's command in its terminal; the receiver hears how it
    /// ended, or that a task that never ends finished its first round.
    fn start_task(
        &mut self,
        task: &Prepared,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<oneshot::Receiver<bool>> {
        let command = task.command.clone()?;
        let (sender, receiver) = oneshot::channel();
        // Already running: it comes in front.
        let running = self.tasks.runs.iter().position(|run| {
            run.label == task.label
                && !matches!(run.state, RunState::Ended(_))
                && run.terminal.upgrade().is_some()
        });
        if let Some(running) = running {
            if let Some(view) = self.tasks.runs[running].terminal.upgrade()
                && let Some(index) = self.terminal_index(&view)
            {
                self.active_terminal = index;
                self.layout.show_panel_view(PanelView::Terminal);
            }
            let waits = self.tasks.runs[running].state == RunState::Waiting;
            self.status_message = Some(notices::fill(
                self.strings().tasks_already,
                &[("task", &task.label)],
            ));
            cx.notify();
            if waits {
                let _ = sender.send(true);
                return Some(receiver);
            }
            return None;
        }
        let view = self.task_terminal(task, window, cx)?;
        self.set_task_problems(&task.label, Vec::new(), cx);
        self.tasks.next_id += 1;
        let id = self.tasks.next_id;
        // Without the shell's marks nothing says when it ends.
        let reports = view.read(cx).terminal().reports_commands();
        let watch = (task.background && reports).then(|| {
            cx.observe(&view, move |this, _, cx| {
                this.background_task_printed(id, cx)
            })
        });
        let mut done = Some(sender);
        if !reports && let Some(done) = done.take() {
            let _ = done.send(true);
        }
        self.tasks.runs.push(TaskRun {
            id,
            label: task.label.clone(),
            command: command.clone(),
            cwd: task.cwd.clone(),
            env: task.env.clone(),
            terminal: view.downgrade(),
            state: if reports {
                RunState::Running
            } else {
                RunState::Ended(None)
            },
            matchers: task.matchers.clone(),
            bases: task.bases.clone(),
            done,
            reading: None,
            read: 0,
            _watch: watch,
        });
        if self.tasks.runs.len() > KEPT_RUNS {
            self.tasks.runs.remove(0);
        }
        view.update(cx, |view, cx| view.run_at_prompt(&command, cx));
        cx.notify();
        Some(receiver)
    }

    /// The task's terminal: its last one when free and started the same
    /// way, or a new one with the task's name.
    fn task_terminal(
        &mut self,
        task: &Prepared,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Entity<TerminalView>> {
        let before = (self.layout.panel, self.layout.panel_view);
        let used = self
            .tasks
            .runs
            .iter()
            .rev()
            .filter(|run| run.label == task.label && run.cwd == task.cwd && run.env == task.env)
            .filter_map(|run| run.terminal.upgrade())
            .find(|view| view.read(cx).runs_shell());
        let free = used.and_then(|view| Some((self.terminal_index(&view)?, view)));
        let view = match free {
            Some((index, view)) => {
                self.active_terminal = index;
                self.layout.show_panel_view(PanelView::Terminal);
                view
            }
            None => {
                let mut spawn = terminal_view::shell_spawn(task.cwd.clone(), cx);
                spawn.env.extend(task.env.iter().cloned());
                let view = self.add_terminal(spawn, None, task.focus, window, cx)?;
                self.name_terminal(&view, task.label.clone());
                view
            }
        };
        if !task.reveal {
            (self.layout.panel, self.layout.panel_view) = before;
        }
        Some(view)
    }

    /// A command ended in a terminal: if it was a task, how it went and
    /// the problems in its output.
    pub(super) fn task_command_finished(
        &mut self,
        view: &Entity<TerminalView>,
        command: &ShellCommand,
        cx: &mut Context<Self>,
    ) {
        let Some(text) = command.text.as_deref() else {
            return;
        };
        let Some(index) = self.tasks.runs.iter().rposition(|run| {
            !matches!(run.state, RunState::Ended(_))
                && run.command == text
                && run.terminal.upgrade().as_ref() == Some(view)
        }) else {
            return;
        };
        let output = view.read(cx).output_of(command.id).unwrap_or_default();
        let run = &mut self.tasks.runs[index];
        run.state = RunState::Ended(command.exit);
        run._watch = None;
        run.reading = None;
        let done = run.done.take();
        let (label, id) = (run.label.clone(), run.id);
        let count = self.read_task_problems(id, &output, cx);
        let passed = command.exit == Some(0);
        let strings = self.strings();
        let exit = command.exit.map_or("?".to_owned(), |exit| exit.to_string());
        let message = match (passed, count) {
            (true, 0) => notices::fill(strings.tasks_done, &[("task", &label)]),
            (true, count) => notices::fill(
                strings.tasks_done_problems,
                &[("task", &label), ("count", &count.to_string())],
            ),
            (false, 0) => notices::fill(strings.tasks_failed, &[("task", &label), ("exit", &exit)]),
            (false, count) => notices::fill(
                strings.tasks_failed_problems,
                &[
                    ("task", &label),
                    ("exit", &exit),
                    ("count", &count.to_string()),
                ],
            ),
        };
        self.status_message = Some(message);
        if let Some(done) = done {
            let _ = done.send(passed);
        }
        cx.notify();
    }

    /// A terminal closed: the task in it is over.
    pub(super) fn task_terminal_closed(&mut self, view: &Entity<TerminalView>) {
        for run in &mut self.tasks.runs {
            if run.terminal.upgrade().as_ref() == Some(view)
                && !matches!(run.state, RunState::Ended(_))
            {
                run.state = RunState::Ended(None);
                run.done = None;
                run._watch = None;
                run.reading = None;
            }
        }
    }

    /// A task that never ends wrote something: read once it is quiet.
    fn background_task_printed(&mut self, id: u64, cx: &mut Context<Self>) {
        let Some(run) = self.tasks.runs.iter_mut().find(|run| run.id == id) else {
            return;
        };
        if run.reading.is_some() || matches!(run.state, RunState::Ended(_)) {
            return;
        }
        run.reading = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(BACKGROUND_PAUSE).await;
            this.update(cx, |this, cx| this.read_background_task(id, cx))
                .ok();
        }));
    }

    fn read_background_task(&mut self, id: u64, cx: &mut Context<Self>) {
        let Some(index) = self.tasks.runs.iter().position(|run| run.id == id) else {
            return;
        };
        let run = &mut self.tasks.runs[index];
        run.reading = None;
        let Some(view) = run.terminal.upgrade() else {
            return;
        };
        let terminal = view.read(cx).terminal().clone();
        let command = terminal
            .commands()
            .into_iter()
            .rev()
            .find(|command| !command.finished && command.text.as_deref() == Some(&run.command));
        let Some(output) = command.and_then(|command| terminal.output_of(command.id)) else {
            return;
        };
        if output.len() == run.read {
            return;
        }
        run.read = output.len();
        match last_round(&output, &run.matchers) {
            Some(round) => {
                run.state = RunState::Waiting;
                let done = run.done.take();
                self.read_task_problems(id, &round, cx);
                if let Some(done) = done {
                    let _ = done.send(true);
                }
            }
            None => run.state = RunState::Running,
        }
        cx.notify();
    }

    /// Puts the problems of a run's output in the Problems view; returns
    /// how many there are.
    fn read_task_problems(&mut self, id: u64, output: &str, cx: &mut Context<Self>) -> usize {
        let Some(run) = self.tasks.runs.iter().find(|run| run.id == id) else {
            return 0;
        };
        let label = run.label.clone();
        let mut problems: Vec<(String, Diagnostic)> = Vec::new();
        for matcher in &run.matchers {
            for found in matcher.find(output) {
                let Some(uri) = self.problem_uri(&found, &run.bases) else {
                    continue;
                };
                let open = self.documents.contains_key(&uri);
                let served = !self.languages.servers_for(&uri).is_empty();
                let wanted = match matcher.applies_to {
                    AppliesTo::All => true,
                    AppliesTo::Open => open,
                    // Its server says the same, as it is typed.
                    AppliesTo::Closed => !(open && served),
                };
                if wanted {
                    problems.push((uri, diagnostic(&found, matcher, &label)));
                }
            }
        }
        let count = problems.len();
        self.set_task_problems(&label, problems, cx);
        count
    }

    /// The errors of a build Keel made itself, for the debugger.
    pub(super) fn build_problems(&mut self, output: &str, cx: &mut Context<Self>) {
        const BUILD: &str = "cargo (debug)";
        let (matchers, _) = super::problem_matchers::matchers(Some(&json!("$rustc")));
        let bases = vec![self.terminal_folder()];
        let mut problems = Vec::new();
        for matcher in &matchers {
            for found in matcher.find(output) {
                if let Some(uri) = self.problem_uri(&found, &bases) {
                    problems.push((uri, diagnostic(&found, matcher, BUILD)));
                }
            }
        }
        self.set_task_problems(BUILD, problems, cx);
    }

    /// The file a problem names, from the folders it may start from.
    fn problem_uri(&self, found: &Found, bases: &[PathBuf]) -> Option<String> {
        let file = found.file.strip_prefix("file://").unwrap_or(&found.file);
        let path = Path::new(file);
        let path = if path.is_absolute() {
            cleaned(path)
        } else {
            bases
                .iter()
                .map(|base| cleaned(&base.join(path)))
                .find(|path| path.is_file())?
        };
        path.is_file().then(|| Uri::file(&path).to_string())
    }

    /// A task's problems take the place of those it had.
    fn set_task_problems(
        &mut self,
        label: &str,
        problems: Vec<(String, Diagnostic)>,
        cx: &mut Context<Self>,
    ) {
        let of_task = |diagnostic: &Diagnostic| {
            diagnostic
                .data
                .as_ref()
                .is_some_and(|data| data["task"] == label)
        };
        let mut touched: HashSet<String> = HashSet::new();
        for (uri, by_server) in &mut self.languages.diagnostics {
            let Some(list) = by_server.get_mut(TASKS) else {
                continue;
            };
            let before = list.len();
            list.retain(|diagnostic| !of_task(diagnostic));
            if list.len() != before {
                touched.insert(uri.clone());
            }
            if list.is_empty() {
                by_server.remove(TASKS);
            }
        }
        for (uri, diagnostic) in problems {
            self.languages
                .diagnostics
                .entry(uri.clone())
                .or_default()
                .entry(TASKS)
                .or_default()
                .push(diagnostic);
            touched.insert(uri);
        }
        self.languages
            .diagnostics
            .retain(|_, by_server| !by_server.is_empty());
        for uri in touched {
            self.diagnostics_changed(&uri, cx);
        }
    }

    /// The tasks running, for the status bar.
    pub(super) fn tasks_status(&self) -> Option<String> {
        let running: Vec<&str> = self
            .tasks
            .runs
            .iter()
            .filter(|run| run.state == RunState::Running && run.terminal.upgrade().is_some())
            .map(|run| run.label.as_str())
            .collect();
        (!running.is_empty()).then(|| {
            notices::fill(
                self.strings().tasks_running,
                &[("tasks", &running.join(", "))],
            )
        })
    }

    /// «Attività: Ferma»: Ctrl+C to the task running, or to the one chosen.
    fn terminate_task(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let running: Vec<(String, Entity<TerminalView>)> = self
            .tasks
            .runs
            .iter()
            .filter(|run| !matches!(run.state, RunState::Ended(_)))
            .filter_map(|run| Some((run.label.clone(), run.terminal.upgrade()?)))
            .collect();
        let stop = |view: &Entity<TerminalView>, cx: &mut Context<Self>| {
            view.read(cx).terminal().write(vec![0x03]);
        };
        match running.as_slice() {
            [] => {
                self.status_message = Some(self.strings().tasks_none_running.to_owned());
                cx.notify();
            }
            [(_, view)] => stop(view, cx),
            _ => {
                let items = running
                    .iter()
                    .map(|(label, _)| PickItem::new(label.clone()))
                    .collect();
                let chosen = quick_pick::show(None, None, items, window, cx);
                cx.spawn_in(window, async move |this, cx| {
                    let Some(index) = chosen.await.ok().flatten() else {
                        return;
                    };
                    this.update(cx, |_, cx| {
                        running[index].1.read(cx).terminal().write(vec![0x03]);
                    })
                    .ok();
                })
                .detach();
            }
        }
    }

    /// Opens `.vscode/tasks.json`, written from what the project has when
    /// there is none.
    fn configure_tasks(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (Some(path), Some(root)) = (self.tasks_file(), self.project.root.clone()) else {
            return;
        };
        if !path.exists() {
            let text = template(&task_config::found(&root));
            let written = path
                .parent()
                .map_or(Ok(()), std::fs::create_dir_all)
                .and_then(|()| std::fs::write(&path, text));
            if let Err(error) = written {
                return self.task_problem(format!("{}: {error}", path.display()), cx);
            }
        }
        self.open_file(&path, window, cx);
    }
}

/// A first `tasks.json`: the project's own build when Keel found one.
fn template(found: &[TaskDef]) -> String {
    let build = found.iter().find(|task| task.group == Group::Build);
    let (label, command, matcher) = match build {
        Some(task) => (
            task.label.as_str(),
            task.detail
                .as_deref()
                .filter(|_| task.source == "cargo")
                .unwrap_or(match task.source {
                    "tsc" => "npx tsc -p tsconfig.json",
                    _ => "npm run build",
                }),
            match task.source {
                "cargo" => "[\"$rustc\", \"$rust-panic\"]",
                "tsc" => "\"$tsc\"",
                _ => "[]",
            },
        ),
        None => ("echo", "echo ciao", "[]"),
    };
    format!(
        "{{\n    // https://code.visualstudio.com/docs/debugtest/tasks\n    \"version\": \"2.0.0\",\n    \"tasks\": [\n        {{\n            \"label\": \"{label}\",\n            \"type\": \"shell\",\n            \"command\": \"{command}\",\n            \"group\": {{ \"kind\": \"build\", \"isDefault\": true }},\n            \"problemMatcher\": {matcher}\n        }}\n    ]\n}}\n"
    )
}

fn diagnostic(found: &Found, matcher: &Matcher, label: &str) -> Diagnostic {
    let start = Position::new(found.line, found.column.unwrap_or(0));
    let end = match (found.end, found.column) {
        (Some((line, column)), _) => Position::new(line, column),
        (None, Some(column)) => Position::new(found.line, column + 1),
        // No column: the whole line.
        (None, None) => Position::new(found.line, 100_000),
    };
    Diagnostic {
        range: LspRange::new(start, end),
        severity: Some(found.severity),
        code: found.code.clone().map(NumberOrString::String),
        source: Some(matcher.source.clone().unwrap_or_else(|| label.to_owned())),
        message: found.message.clone(),
        data: Some(json!({ "task": label })),
        ..Diagnostic::default()
    }
}

/// Whether a task's problem is already said by a language server: on the
/// same line, with the same words.
pub(super) fn said_already(problem: &Diagnostic, others: &[&Diagnostic]) -> bool {
    let first = |message: &str| {
        message
            .lines()
            .next()
            .unwrap_or_default()
            .trim()
            .to_lowercase()
    };
    let mine = first(&problem.message);
    others.iter().any(|other| {
        let other_first = first(&other.message);
        other.range.start.line == problem.range.start.line
            && !mine.is_empty()
            && (other_first.contains(&mine) || mine.contains(&other_first))
            && !other_first.is_empty()
    })
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::time::Instant;

    use gpui::{TestAppContext, VisualTestContext};
    use lsp::lsp_types::DiagnosticSeverity;

    use super::*;

    const TASKS_JSON: &str = r#"{
        // Le attività della prova
        "version": "2.0.0",
        "tasks": [
            { "label": "prepara", "type": "shell", "command": "echo preparato > pronto.txt" },
            {
                "label": "compila",
                "type": "shell",
                "command": "cat pronto.txt; printf 'src/a.c:2:5: error: manca qualcosa\\n'; false",
                "dependsOn": "prepara",
                "problemMatcher": "$gcc",
                "group": { "kind": "build", "isDefault": true },
            },
            { "label": "dopo", "command": "echo mai > mai.txt", "dependsOn": "compila" },
            {
                "label": "dove",
                "type": "shell",
                "command": "pwd > ${workspaceFolder}/dove.txt && echo $COLORE >> ${workspaceFolder}/dove.txt",
                "options": { "cwd": "${workspaceFolder}/src", "env": { "COLORE": "verde scuro" } },
            },
            {
                "label": "guarda",
                "type": "shell",
                "command": "echo parto; printf 'src/a.c:1:1: error: uno\\n'; echo fatto; sleep 60",
                "isBackground": true,
                "problemMatcher": {
                    "base": "$gcc",
                    "background": { "beginsPattern": "^parto", "endsPattern": "^fatto" },
                },
            },
            { "label": "usa", "command": "echo usato > usato.txt", "dependsOn": ["guarda"] },
        ],
    }"#;

    fn project(
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
        let base = fs::canonicalize(folder.path()).unwrap();
        let home = base.join("home");
        fs::create_dir(&home).unwrap();
        fs::write(home.join(".zshrc"), "PS1='$ '\n").unwrap();
        terminal_view::TEST_HOME.with(|test| *test.borrow_mut() = Some(home));
        let root = base.join("app");
        fs::create_dir_all(root.join(".vscode")).unwrap();
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(root.join("src/a.c"), "int a;\nint b\n").unwrap();
        fs::write(root.join(".vscode/tasks.json"), TASKS_JSON).unwrap();
        let settings = Settings::load(base.join("settings.json"), None);
        let opening = Opening::new(Some(root.clone()), None);
        let (view, cx) =
            cx.add_window_view(|window, cx| Workbench::new(opening, settings, None, window, cx));
        (folder, root, view, cx)
    }

    fn wait_until(
        cx: &mut VisualTestContext,
        what: &str,
        check: impl Fn(&mut VisualTestContext) -> bool,
    ) {
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            cx.executor().advance_clock(Duration::from_millis(100));
            cx.run_until_parked();
            if check(cx) {
                return;
            }
            assert!(Instant::now() < deadline, "gave up waiting for {what}");
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    fn state(
        view: &Entity<Workbench>,
        label: &str,
        cx: &mut VisualTestContext,
    ) -> Option<RunState> {
        view.read_with(cx, |view, _| {
            let run = view.tasks.runs.iter().rev().find(|run| run.label == label);
            run.map(|run| run.state)
        })
    }

    fn problems(
        view: &Entity<Workbench>,
        cx: &mut VisualTestContext,
    ) -> Vec<(String, u32, String)> {
        view.read_with(cx, |view, _| {
            view.problems
                .files
                .iter()
                .flat_map(|(uri, problems)| {
                    problems.iter().map(|problem| {
                        assert_eq!(problem.diagnostic.severity, Some(DiagnosticSeverity::ERROR));
                        (
                            uri.rsplit('/').next().unwrap().to_owned(),
                            problem.diagnostic.range.start.line,
                            problem.diagnostic.message.clone(),
                        )
                    })
                })
                .collect()
        })
    }

    #[gpui::test]
    fn a_build_runs_after_what_it_needs_and_fills_the_problems(cx: &mut TestAppContext) {
        let (_folder, root, view, cx) = project(cx);
        cx.simulate_keystrokes("cmd-shift-b");
        wait_until(cx, "the build", |cx| {
            state(&view, "compila", cx) == Some(RunState::Ended(Some(1)))
        });
        assert_eq!(state(&view, "prepara", cx), Some(RunState::Ended(Some(0))));
        assert_eq!(
            fs::read_to_string(root.join("pronto.txt")).unwrap(),
            "preparato\n"
        );
        assert_eq!(
            problems(&view, cx),
            [("a.c".to_owned(), 1, "manca qualcosa".to_owned())]
        );
        let (names, errors, status) = view.read_with(cx, |view, _| {
            let names: Vec<_> = view
                .terminals
                .iter()
                .filter_map(|tab| tab.name.clone())
                .collect();
            (
                names,
                view.problems.errors,
                view.status_message.clone().unwrap(),
            )
        });
        assert_eq!(names, ["prepara", "compila"]);
        assert_eq!(errors, 1);
        assert!(
            status.contains("compila") && status.contains('1'),
            "{status}"
        );

        // What depends on a task that fails does not run.
        view.update_in(cx, |view, window, cx| view.run_task("dopo", window, cx));
        wait_until(cx, "the build again", |cx| {
            view.read_with(cx, |view, _| {
                view.tasks
                    .runs
                    .iter()
                    .filter(|run| run.label == "compila")
                    .count()
                    == 2
            }) && state(&view, "compila", cx) == Some(RunState::Ended(Some(1)))
        });
        assert_eq!(state(&view, "dopo", cx), None);
        assert!(!root.join("mai.txt").exists());
        // Its terminals were free: used again.
        assert_eq!(view.read_with(cx, |view, _| view.terminals.len()), 2);

        // Put right: the problem goes with the next run.
        let right = TASKS_JSON.replace(
            "printf 'src/a.c:2:5: error: manca qualcosa\\\\n'; false",
            "true",
        );
        assert_ne!(right, TASKS_JSON);
        fs::write(root.join(".vscode/tasks.json"), right).unwrap();
        view.update_in(cx, |view, window, cx| view.run_task("compila", window, cx));
        wait_until(cx, "a good build", |cx| {
            state(&view, "compila", cx) == Some(RunState::Ended(Some(0)))
        });
        assert_eq!(problems(&view, cx), []);
    }

    #[gpui::test]
    fn a_task_has_its_folder_and_its_environment(cx: &mut TestAppContext) {
        let (_folder, root, view, cx) = project(cx);
        view.update_in(cx, |view, window, cx| view.run_task("dove", window, cx));
        wait_until(cx, "the task", |cx| {
            state(&view, "dove", cx) == Some(RunState::Ended(Some(0)))
        });
        assert_eq!(
            fs::read_to_string(root.join("dove.txt")).unwrap(),
            format!("{}\nverde scuro\n", root.join("src").display())
        );
        // A name that is not there says so, and runs nothing.
        view.update_in(cx, |view, window, cx| view.run_task("chissà", window, cx));
        let status = view.read_with(cx, |view, _| view.status_message.clone().unwrap());
        assert!(status.contains("chissà"), "{status}");
    }

    #[gpui::test]
    fn a_task_that_never_ends_says_when_it_is_ready(cx: &mut TestAppContext) {
        let (_folder, root, view, cx) = project(cx);
        view.update_in(cx, |view, window, cx| view.run_task("usa", window, cx));
        wait_until(cx, "the task after the watch", |cx| {
            state(&view, "usa", cx) == Some(RunState::Ended(Some(0)))
        });
        assert!(root.join("usato.txt").exists());
        assert_eq!(state(&view, "guarda", cx), Some(RunState::Waiting));
        assert_eq!(
            problems(&view, cx),
            [("a.c".to_owned(), 0, "uno".to_owned())]
        );
        // Stopped with Ctrl+C.
        view.update_in(cx, |view, window, cx| view.terminate_task(window, cx));
        wait_until(cx, "the watch to stop", |cx| {
            matches!(state(&view, "guarda", cx), Some(RunState::Ended(Some(_))))
        });
    }

    #[test]
    fn a_command_is_one_line_for_the_shell() {
        assert_eq!(one_line("  make all  "), "make all");
        assert_eq!(one_line("echo 'a\nb'"), "eval $'echo \\'a\\nb\\''");
        assert_eq!(one_line("echo \\x\nrm y"), "eval $'echo \\\\x\\nrm y'");
    }

    #[test]
    fn the_last_round_of_a_watch() {
        let (matchers, _) = super::super::problem_matchers::matchers(Some(&json!("$tsc-watch")));
        let first = "[10:00:00] Starting compilation in watch mode...\n\nsrc/a.ts(1,1): error TS1: x\n\n[10:00:01] Found 1 error. Watching for file changes.\n";
        assert!(last_round(first, &matchers).unwrap().contains("TS1"));
        let again = format!(
            "{first}\n[10:01:00] File change detected. Starting incremental compilation...\n"
        );
        assert_eq!(last_round(&again, &matchers), None);
        let done = format!("{again}\n[10:01:01] Found 0 errors. Watching for file changes.\n");
        assert!(!last_round(&done, &matchers).unwrap().contains("TS1"));
    }
}
