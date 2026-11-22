//! What a project can run: the tasks of `.vscode/tasks.json`, read as VS
//! Code reads them, and the ones Keel finds itself (npm scripts, cargo,
//! TypeScript). Here is only the reading; `tasks.rs` runs them.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde_json::Value;

use super::problem_matchers::{Matcher, matchers};
use super::terminal_view::shell_quote;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Group {
    Build,
    Test,
    Other,
}

/// How an argument is quoted on the command line.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Quoting {
    /// Quoted only when it has blanks, as VS Code does for plain strings.
    IfNeeded,
    Strong,
    Weak,
    Escape,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Part {
    text: String,
    quoting: Quoting,
}

#[derive(Clone, Debug)]
pub(super) struct TaskDef {
    pub(super) label: String,
    /// Where it comes from: `tasks.json`, `npm`, `cargo`, `tsc`.
    pub(super) source: &'static str,
    pub(super) detail: Option<String>,
    /// A program and its arguments, with no shell between.
    process: bool,
    command: Option<Part>,
    args: Vec<Part>,
    pub(super) cwd: Option<String>,
    pub(super) env: Vec<(String, String)>,
    pub(super) group: Group,
    /// The one its group's command runs without asking.
    pub(super) default: bool,
    pub(super) matchers: Vec<Matcher>,
    /// Matchers it names that could not be read.
    pub(super) unread: Vec<String>,
    pub(super) depends_on: Vec<String>,
    pub(super) in_sequence: bool,
    pub(super) background: bool,
    /// Left out of the list: run by other tasks.
    pub(super) hidden: bool,
    /// `presentation.reveal`: the terminal comes in front.
    pub(super) reveal: bool,
    pub(super) focus: bool,
}

/// A question a task asks before it runs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct TaskInput {
    pub(super) id: String,
    pub(super) description: String,
    pub(super) default: Option<String>,
    /// The values to choose from; empty for a text.
    pub(super) options: Vec<(String, String)>,
}

#[derive(Clone, Debug, Default)]
pub(super) struct TaskFile {
    pub(super) tasks: Vec<TaskDef>,
    pub(super) inputs: Vec<TaskInput>,
}

/// What the variables of a task stand for.
#[derive(Clone, Debug, Default)]
pub(super) struct Variables {
    pub(super) root: PathBuf,
    pub(super) file: Option<PathBuf>,
    /// From 1.
    pub(super) line: usize,
    pub(super) column: usize,
    pub(super) selected: String,
    pub(super) inputs: HashMap<String, String>,
    pub(super) config: HashMap<String, String>,
    pub(super) default_build: Option<String>,
}

fn text_of(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => Some(text.clone()),
        Value::Number(number) => Some(number.to_string()),
        Value::Bool(value) => Some(value.to_string()),
        _ => None,
    }
}

fn part_of(value: &Value) -> Option<Part> {
    if let Some(text) = text_of(value) {
        return Some(Part {
            text,
            quoting: Quoting::IfNeeded,
        });
    }
    // { "value": "a b", "quoting": "strong" }; a value can be a list.
    let text = match &value["value"] {
        Value::Array(parts) => parts
            .iter()
            .filter_map(text_of)
            .collect::<Vec<_>>()
            .join(" "),
        value => text_of(value)?,
    };
    Some(Part {
        text,
        quoting: match value["quoting"].as_str() {
            Some("strong") => Quoting::Strong,
            Some("weak") => Quoting::Weak,
            Some("escape") => Quoting::Escape,
            _ => Quoting::IfNeeded,
        },
    })
}

impl Part {
    fn expanded(&self, variables: &Variables) -> Result<Part, String> {
        Ok(Part {
            text: variables.expand(&self.text)?,
            quoting: self.quoting,
        })
    }

    fn on_the_line(&self) -> String {
        let text = &self.text;
        let quoted = |mark: char| text.len() >= 2 && text.starts_with(mark) && text.ends_with(mark);
        match self.quoting {
            Quoting::IfNeeded if text.is_empty() => "''".to_owned(),
            Quoting::IfNeeded if !text.contains(char::is_whitespace) => text.clone(),
            Quoting::IfNeeded if quoted('\'') || quoted('"') => text.clone(),
            Quoting::IfNeeded | Quoting::Strong => format!("'{}'", text.replace('\'', r"'\''")),
            Quoting::Weak => format!("\"{}\"", text.replace('\\', r"\\").replace('"', "\\\"")),
            Quoting::Escape => text
                .chars()
                .flat_map(|char| {
                    let escape = matches!(char, ' ' | '"' | '\'' | '\\').then_some('\\');
                    escape.into_iter().chain(std::iter::once(char))
                })
                .collect(),
        }
    }
}

impl TaskDef {
    fn new(label: impl Into<String>, source: &'static str) -> Self {
        Self {
            label: label.into(),
            source,
            detail: None,
            process: false,
            command: None,
            args: Vec::new(),
            cwd: None,
            env: Vec::new(),
            group: Group::Other,
            default: false,
            matchers: Vec::new(),
            unread: Vec::new(),
            depends_on: Vec::new(),
            in_sequence: false,
            background: false,
            hidden: false,
            reveal: true,
            focus: false,
        }
    }

    /// A task Keel found, with its command line as it is.
    fn found(
        label: &str,
        source: &'static str,
        command: &str,
        matcher: &str,
        group: Group,
    ) -> Self {
        let mut task = Self::new(label, source);
        task.command = Some(Part {
            text: command.to_owned(),
            quoting: Quoting::IfNeeded,
        });
        task.detail = Some(command.to_owned());
        task.group = group;
        if !matcher.is_empty() {
            let names: Vec<Value> = matcher.split(' ').map(Value::from).collect();
            task.matchers = matchers(Some(&Value::Array(names))).0;
        }
        task
    }

    /// The line typed in the terminal, with the variables filled in;
    /// `None` for a task that only runs others.
    pub(super) fn command_line(&self, variables: &Variables) -> Result<Option<String>, String> {
        let Some(command) = &self.command else {
            return Ok(None);
        };
        let command = command.expanded(variables)?;
        let args = self
            .args
            .iter()
            .map(|arg| arg.expanded(variables))
            .collect::<Result<Vec<_>, _>>()?;
        if self.process {
            let words = std::iter::once(&command)
                .chain(&args)
                .map(|part| shell_quote(&part.text));
            return Ok(Some(words.collect::<Vec<_>>().join(" ")));
        }
        // Alone, the command is a whole line of shell, left as it is.
        if args.is_empty() {
            return Ok(Some(command.text));
        }
        let words = std::iter::once(&command)
            .chain(&args)
            .map(Part::on_the_line);
        Ok(Some(words.collect::<Vec<_>>().join(" ")))
    }

    /// The ids of the inputs it asks for.
    pub(super) fn inputs(&self) -> Vec<String> {
        let mut texts: Vec<&str> = Vec::new();
        texts.extend(self.command.iter().map(|part| part.text.as_str()));
        texts.extend(self.args.iter().map(|part| part.text.as_str()));
        texts.extend(self.cwd.as_deref());
        texts.extend(self.env.iter().map(|(_, value)| value.as_str()));
        let mut ids = Vec::new();
        for text in texts {
            for id in text.split("${input:").skip(1) {
                if let Some((id, _)) = id.split_once('}')
                    && !ids.iter().any(|known| known == id)
                {
                    ids.push(id.to_owned());
                }
            }
        }
        ids
    }
}

impl Variables {
    /// The text with its `${…}` filled in, or the one that is unknown.
    pub(super) fn expand(&self, text: &str) -> Result<String, String> {
        let mut out = String::with_capacity(text.len());
        let mut rest = text;
        while let Some(at) = rest.find("${") {
            out.push_str(&rest[..at]);
            let after = &rest[at + 2..];
            let Some(end) = after.find('}') else {
                out.push_str(&rest[at..]);
                return Ok(out);
            };
            let name = &after[..end];
            out.push_str(&self.value(name).ok_or_else(|| format!("${{{name}}}"))?);
            rest = &after[end + 1..];
        }
        out.push_str(rest);
        Ok(out)
    }

    fn value(&self, name: &str) -> Option<String> {
        let shown = |path: &Path| path.to_string_lossy().into_owned();
        if let Some(variable) = name.strip_prefix("env:") {
            return Some(std::env::var(variable).unwrap_or_default());
        }
        if let Some(id) = name.strip_prefix("input:") {
            return self.inputs.get(id).cloned();
        }
        if let Some(key) = name.strip_prefix("config:") {
            return self.config.get(key).cloned();
        }
        // `${workspaceFolder:app}` names a folder of a larger workspace.
        let base = name.split(':').next().unwrap_or(name);
        match base {
            "workspaceFolder" | "workspaceRoot" | "cwd" => return Some(shown(&self.root)),
            "workspaceFolderBasename" | "workspaceRootFolderName" => {
                return Some(shown(Path::new(self.root.file_name()?)));
            }
            "userHome" => return std::env::home_dir().map(|home| shown(&home)),
            "pathSeparator" | "/" => return Some("/".to_owned()),
            "lineNumber" => return Some(self.line.to_string()),
            "columnNumber" => return Some(self.column.to_string()),
            "selectedText" => return Some(self.selected.clone()),
            "defaultBuildTask" => return self.default_build.clone(),
            _ => {}
        }
        let file = self.file.as_deref()?;
        let relative = file.strip_prefix(&self.root).unwrap_or(file);
        let name_of = |path: Option<&std::ffi::OsStr>| path.map(|name| shown(Path::new(name)));
        match base {
            "file" => Some(shown(file)),
            "relativeFile" => Some(shown(relative)),
            "relativeFileDirname" => {
                let folder = relative.parent().map(shown).unwrap_or_default();
                Some(if folder.is_empty() {
                    ".".to_owned()
                } else {
                    folder
                })
            }
            "fileBasename" => name_of(file.file_name()),
            "fileBasenameNoExtension" => name_of(file.file_stem()),
            "fileExtname" => Some(
                file.extension()
                    .map(|extension| format!(".{}", extension.to_string_lossy()))
                    .unwrap_or_default(),
            ),
            "fileDirname" => file.parent().map(shown),
            "fileDirnameBasename" => name_of(file.parent()?.file_name()),
            "fileWorkspaceFolder" => Some(shown(&self.root)),
            _ => None,
        }
    }
}

/// Reads a `tasks.json`.
pub(super) fn read(text: &str, root: &Path) -> Result<TaskFile, String> {
    let file = settings::jsonc::parse(text).map_err(|error| error.to_string())?;
    let mut tasks = Vec::new();
    for value in file["tasks"].as_array().into_iter().flatten() {
        tasks.extend(task_of(value, &file, root));
    }
    let inputs = file["inputs"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|input| {
            let option = |option: &Value| match option {
                Value::String(value) => Some((value.clone(), value.clone())),
                option => {
                    let value = text_of(&option["value"])?;
                    let label = text_of(&option["label"]).unwrap_or_else(|| value.clone());
                    Some((label, value))
                }
            };
            Some(TaskInput {
                id: input["id"].as_str()?.to_owned(),
                description: input["description"].as_str().unwrap_or_default().to_owned(),
                default: text_of(&input["default"]),
                options: input["options"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(option)
                    .collect(),
            })
        })
        .collect();
    Ok(TaskFile { tasks, inputs })
}

/// What a task says for macOS over what it says for all.
fn on_mac<'a>(value: &'a Value, key: &str) -> &'a Value {
    let own = &value["osx"][key];
    if own.is_null() { &value[key] } else { own }
}

fn task_of(value: &Value, file: &Value, root: &Path) -> Option<TaskDef> {
    let kind = value["type"].as_str().unwrap_or("shell");
    let label = value["label"].as_str().or(value["taskName"].as_str());
    let mut task = match kind {
        "npm" => {
            let script = value["script"].as_str()?;
            let folder = root.join(value["path"].as_str().unwrap_or_default());
            let scripts = super::npm_scripts::NpmScripts::read(&folder);
            let command = scripts.map_or_else(
                || format!("npm run {}", shell_quote(script)),
                |scripts| scripts.command(script),
            );
            let mut task = TaskDef::found(
                &format!("npm: {script}"),
                "tasks.json",
                &command,
                "",
                Group::Other,
            );
            if folder != root {
                task.cwd = Some(folder.to_string_lossy().into_owned());
            }
            task
        }
        "cargo" => {
            let command = value["command"].as_str()?;
            let mut line = format!("cargo {command}");
            for arg in value["args"].as_array().into_iter().flatten() {
                line.push(' ');
                line.push_str(&part_of(arg)?.on_the_line());
            }
            TaskDef::found(
                &format!("cargo {command}"),
                "tasks.json",
                &line,
                "$rustc $rust-panic",
                Group::Other,
            )
        }
        "typescript" => {
            let config = value["tsconfig"].as_str().unwrap_or("tsconfig.json");
            let watch = value["option"].as_str() == Some("watch");
            typescript_task(root, config, watch, "tasks.json")
        }
        "shell" | "process" => {
            let mut task = TaskDef::new(label?, "tasks.json");
            task.process = kind == "process";
            task.command = part_of(on_mac(value, "command"));
            task.args = on_mac(value, "args")
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(part_of)
                .collect();
            task
        }
        // A kind of task an extension runs, which Keel does not know.
        _ => return None,
    };
    if let Some(label) = label {
        task.label = label.to_owned();
    }
    for options in [
        &file["options"],
        &file["osx"]["options"],
        &value["options"],
        &value["osx"]["options"],
    ] {
        if let Some(cwd) = options["cwd"].as_str() {
            task.cwd = Some(cwd.to_owned());
        }
        for (name, value) in options["env"].as_object().into_iter().flatten() {
            task.env.retain(|(known, _)| known != name);
            task.env
                .push((name.clone(), text_of(value).unwrap_or_default()));
        }
    }
    if let Some(detail) = value["detail"].as_str() {
        task.detail = Some(detail.to_owned());
    }
    let group = &value["group"];
    let kind = group.as_str().or(group["kind"].as_str());
    match kind {
        Some("build") => task.group = Group::Build,
        Some("test") => task.group = Group::Test,
        _ => {}
    }
    // A glob names the files it is the default for; `true` is all.
    task.default = matches!(&group["isDefault"], Value::Bool(true) | Value::String(_));
    if !value["problemMatcher"].is_null() {
        (task.matchers, task.unread) = matchers(Some(&value["problemMatcher"]));
    }
    task.depends_on = match &value["dependsOn"] {
        Value::String(label) => vec![label.clone()],
        Value::Array(labels) => labels
            .iter()
            .filter_map(|label| label.as_str().map(str::to_owned))
            .collect(),
        _ => Vec::new(),
    };
    task.in_sequence = value["dependsOrder"].as_str() == Some("sequence");
    task.background = value["isBackground"].as_bool().unwrap_or(false);
    task.hidden = value["hide"].as_bool().unwrap_or(false);
    let presentation = &value["presentation"];
    task.reveal = presentation["reveal"].as_str().unwrap_or("always") == "always";
    task.focus = presentation["focus"].as_bool().unwrap_or(false);
    Some(task)
}

fn typescript_task(root: &Path, config: &str, watch: bool, source: &'static str) -> TaskDef {
    let runner = match super::npm_scripts::package_manager_of(root) {
        "pnpm" => "pnpm exec",
        "yarn" => "yarn",
        "bun" => "bunx",
        _ => "npx",
    };
    let config_arg = shell_quote(config);
    let (label, command, matcher) = if watch {
        (
            format!("tsc: watch - {config}"),
            format!("{runner} tsc -p {config_arg} --watch --noEmit"),
            "$tsc-watch",
        )
    } else {
        (
            format!("tsc: build - {config}"),
            format!("{runner} tsc -p {config_arg}"),
            "$tsc",
        )
    };
    let mut task = TaskDef::found(&label, source, &command, matcher, Group::Build);
    task.background = watch;
    task
}

/// The tasks a project has without writing them.
pub(super) fn found(root: &Path) -> Vec<TaskDef> {
    let mut tasks = Vec::new();
    if root.join("Cargo.toml").is_file() {
        for (command, group) in [
            ("build", Group::Build),
            ("check", Group::Build),
            ("clippy", Group::Build),
            ("test", Group::Test),
            ("run", Group::Other),
        ] {
            tasks.push(TaskDef::found(
                &format!("cargo {command}"),
                "cargo",
                &format!("cargo {command}"),
                "$rustc $rust-panic",
                group,
            ));
        }
    }
    if let Some(scripts) = super::npm_scripts::NpmScripts::read(root) {
        for (name, body) in &scripts.scripts {
            let group = match name.as_str() {
                "build" | "compile" | "watch" => Group::Build,
                "test" => Group::Test,
                _ => Group::Other,
            };
            let mut task = TaskDef::found(
                &format!("npm: {name}"),
                "npm",
                &scripts.command(name),
                "",
                group,
            );
            task.detail = Some(body.clone());
            tasks.push(task);
        }
    }
    if root.join("tsconfig.json").is_file() {
        tasks.push(typescript_task(root, "tsconfig.json", false, "tsc"));
        tasks.push(typescript_task(root, "tsconfig.json", true, "tsc"));
    }
    tasks
}

/// The order tasks run in: those `label` depends on, then itself. The
/// error is the label missing, or the one that comes round again.
pub(super) fn with_dependencies(label: &str, tasks: &[TaskDef]) -> Result<Vec<TaskDef>, PlanError> {
    fn visit(
        label: &str,
        tasks: &[TaskDef],
        path: &mut Vec<String>,
        plan: &mut Vec<TaskDef>,
    ) -> Result<(), PlanError> {
        if path.iter().any(|known| known == label) {
            return Err(PlanError::Cycle(label.to_owned()));
        }
        if plan.iter().any(|known| known.label == label) {
            return Ok(());
        }
        let task = tasks
            .iter()
            .find(|task| task.label == label)
            .ok_or_else(|| PlanError::Missing {
                wanted: label.to_owned(),
                by: path.last().cloned().unwrap_or_default(),
            })?;
        path.push(label.to_owned());
        for wanted in &task.depends_on {
            visit(wanted, tasks, path, plan)?;
        }
        path.pop();
        plan.push(task.clone());
        Ok(())
    }
    let mut plan = Vec::new();
    visit(label, tasks, &mut Vec::new(), &mut plan)?;
    Ok(plan)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum PlanError {
    Cycle(String),
    Missing { wanted: String, by: String },
}

#[cfg(test)]
mod tests {
    use super::*;

    fn variables() -> Variables {
        Variables {
            root: "/p/app".into(),
            file: Some("/p/app/src/ui/menu.test.ts".into()),
            line: 12,
            column: 3,
            selected: "ciao".into(),
            inputs: HashMap::from([("porta".to_owned(), "80 81".to_owned())]),
            config: HashMap::from([("editor.tabSize".to_owned(), "2".to_owned())]),
            default_build: None,
        }
    }

    #[test]
    fn variables_stand_for_what_is_open() {
        let expand = |text: &str| variables().expand(text);
        assert_eq!(expand("${workspaceFolder}/x").unwrap(), "/p/app/x");
        assert_eq!(expand("${relativeFile}").unwrap(), "src/ui/menu.test.ts");
        assert_eq!(expand("${relativeFileDirname}").unwrap(), "src/ui");
        assert_eq!(
            expand("${fileBasenameNoExtension}${fileExtname}").unwrap(),
            "menu.test.ts"
        );
        assert_eq!(
            expand("${fileDirnameBasename}:${lineNumber}").unwrap(),
            "ui:12"
        );
        assert_eq!(
            expand("${input:porta} ${config:editor.tabSize}").unwrap(),
            "80 81 2"
        );
        assert_eq!(expand("${workspaceFolderBasename}").unwrap(), "app");
        assert_eq!(expand("costa $5 {x}").unwrap(), "costa $5 {x}");
        assert_eq!(
            expand("${command:ext.pick}"),
            Err("${command:ext.pick}".to_owned())
        );
        assert_eq!(expand("${mai}"), Err("${mai}".to_owned()));
        // With no file open, its variables are unknown.
        let none = Variables {
            file: None,
            ..variables()
        };
        assert!(none.expand("${file}").is_err());
    }

    #[test]
    fn a_tasks_file_is_read_as_vs_code_reads_it() {
        let file = read(
            r#"{
                // commenti e virgole finali
                "version": "2.0.0",
                "options": { "env": { "A": "1", "B": "due" } },
                "tasks": [
                    {
                        "label": "compila",
                        "type": "shell",
                        "command": "make ${input:porta} && echo fatto",
                        "osx": { "command": "gmake all && echo fatto" },
                        "group": { "kind": "build", "isDefault": true },
                        "problemMatcher": ["$gcc", "$boh"],
                        "options": { "cwd": "${workspaceFolder}/c", "env": { "A": "2" } },
                    },
                    {
                        "label": "cerca",
                        "type": "shell",
                        "command": "grep",
                        "args": ["-r", "due parole", { "value": "it's $HOME", "quoting": "strong" }, { "value": "a\"b", "quoting": "weak" }, { "value": "c d", "quoting": "escape" }, ""],
                        "presentation": { "reveal": "silent" },
                    },
                    { "label": "diretto", "type": "process", "command": "/bin/echo", "args": ["a b", "$x"], "isBackground": true },
                    { "type": "cargo", "command": "build", "args": ["--release"] },
                    { "label": "tutto", "dependsOn": ["compila", "cerca"], "dependsOrder": "sequence" },
                    { "label": "sconosciuto", "type": "gulp", "task": "x" },
                ],
                "inputs": [
                    { "id": "porta", "type": "pickString", "description": "Quale?", "options": ["80", { "label": "sicura", "value": "443" }], "default": "80" },
                ],
            }"#,
            Path::new("/p/app"),
        )
        .unwrap();
        let labels: Vec<&str> = file.tasks.iter().map(|task| task.label.as_str()).collect();
        assert_eq!(
            labels,
            ["compila", "cerca", "diretto", "cargo build", "tutto"]
        );
        let line = |index: usize| file.tasks[index].command_line(&variables()).unwrap();
        // macOS's own command, with shell of its own.
        assert_eq!(line(0).as_deref(), Some("gmake all && echo fatto"));
        assert_eq!(
            line(1).as_deref(),
            Some(r#"grep -r 'due parole' 'it'\''s $HOME' "a\"b" c\ d ''"#)
        );
        assert_eq!(line(2).as_deref(), Some("/bin/echo 'a b' '$x'"));
        assert_eq!(line(3).as_deref(), Some("cargo build --release"));
        assert_eq!(line(4), None);
        let compila = &file.tasks[0];
        assert_eq!((compila.group, compila.default), (Group::Build, true));
        assert_eq!(compila.cwd.as_deref(), Some("${workspaceFolder}/c"));
        assert!(compila.env.contains(&("A".to_owned(), "2".to_owned())));
        assert!(compila.env.contains(&("B".to_owned(), "due".to_owned())));
        assert_eq!(
            (compila.matchers.len(), compila.unread.clone()),
            (1, vec!["$boh".to_owned()])
        );
        assert!(!file.tasks[1].reveal);
        assert!(file.tasks[2].background);
        assert_eq!(file.tasks[3].matchers.len(), 2);
        assert_eq!(
            file.inputs[0].options[1],
            ("sicura".to_owned(), "443".to_owned())
        );
        assert!(read("{ rotto", Path::new("/p")).is_err());
    }

    #[test]
    fn dependencies_come_first_and_never_round_again() {
        let file = |text: &str| read(text, Path::new("/p")).unwrap().tasks;
        let tasks = file(
            r#"{ "tasks": [
                { "label": "a", "command": "a", "dependsOn": ["b", "c"] },
                { "label": "b", "command": "b", "dependsOn": "c" },
                { "label": "c", "command": "c ${input:x} ${input:y} ${input:x}" },
            ] }"#,
        );
        let plan = with_dependencies("a", &tasks).unwrap();
        let labels: Vec<&str> = plan.iter().map(|task| task.label.as_str()).collect();
        assert_eq!(labels, ["c", "b", "a"]);
        assert_eq!(plan[0].inputs(), ["x", "y"]);
        let round = file(
            r#"{ "tasks": [{ "label": "a", "command": "a", "dependsOn": "b" }, { "label": "b", "command": "b", "dependsOn": "a" }] }"#,
        );
        assert_eq!(
            with_dependencies("a", &round).err(),
            Some(PlanError::Cycle("a".into()))
        );
        let missing = file(r#"{ "tasks": [{ "label": "a", "command": "a", "dependsOn": "z" }] }"#);
        assert_eq!(
            with_dependencies("a", &missing).err(),
            Some(PlanError::Missing {
                wanted: "z".into(),
                by: "a".into()
            })
        );
    }
}
