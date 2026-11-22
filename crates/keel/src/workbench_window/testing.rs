//! Tests from the margin, as VS Code's testing: a ▶ beside each test and
//! each group of tests, found in the file as it is written (Rust's
//! `#[test]`, Vitest's and Jest's `it`, `test` and `describe`, pytest's
//! `test_` functions and `Test` classes). A click runs it in the terminal
//! named «Test»; when the shell says how it ended, the ▶ becomes ✓ or ✗.
//! The Tests view of the bottom panel keeps the runs, to run one again,
//! see its output, or ask Claude why it failed.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::LazyLock;
use std::time::{Duration, Instant};

use gpui::{
    AnyElement, Bounds, Div, MouseButton, MouseDownEvent, Pixels, SharedString,
    StatefulInteractiveElement as _, TextRun, WeakEntity, canvas, point,
};
use gpui_component::RopeExt as _;
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::menu::ContextMenuExt as _;
use gpui_component::{Icon, Sizable as _};
use gpui_kit_assets::IconName;
use regex::Regex;
use terminal::ShellCommand;

use super::debug_config::{self, DebugConfig};
use super::notices::fill;
use super::shell::Colors;
use super::terminal_view::{TerminalView, shell_quote};
use super::*;

actions!(
    keel,
    [
        RunTestAtCursor,
        DebugTestAtCursor,
        RunTestHere,
        DebugTestHere,
        RunAllTests,
        RerunLastTests,
        RunTestsInFile
    ]
);

/// How many runs the Tests view keeps.
const KEPT_RUNS: usize = 40;
/// The width of the margin with the ▶.
const MARGIN: f32 = 20.;

/// A test or a group of tests, where it is in its file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct FoundTest {
    /// From 0.
    pub(super) line: usize,
    pub(super) name: String,
    /// What picks it out for its runner: Rust's full path, a regex of the
    /// name for Vitest and Jest, pytest's node id after the file.
    pub(super) filter: String,
    /// A module, a `describe` or a class: it runs all its tests.
    pub(super) group: bool,
}

/// How a run went.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Outcome {
    Running,
    Passed,
    Failed,
}

/// A run in the Tests view.
pub(super) struct TestRun {
    id: u64,
    label: String,
    command: String,
    outcome: Outcome,
    started: Instant,
    duration: Option<Duration>,
    terminal: WeakEntity<TerminalView>,
    /// The command in the terminal, once it ended.
    shell_command: Option<u64>,
    /// The tests it runs, by file and filter.
    keys: Vec<(PathBuf, String)>,
}

/// What a run needs to run again.
#[derive(Clone)]
struct RunPlan {
    label: String,
    command: String,
    keys: Vec<(PathBuf, String)>,
}

/// A document's tests, with the version of the text they were read from.
type FoundIn = (u64, Rc<Vec<FoundTest>>);

/// Where each glyph of the margin was painted, for the clicks.
type Placed = Rc<RefCell<Vec<(Bounds<Pixels>, FoundTest)>>>;

#[derive(Default)]
pub(super) struct Testing {
    /// The tests of each document, found again when its text changes.
    found: RefCell<HashMap<String, FoundIn>>,
    results: HashMap<(PathBuf, String), Outcome>,
    pub(super) runs: Vec<TestRun>,
    next_id: u64,
    last: Option<RunPlan>,
    /// The test the margin's menu was opened on.
    menu: Option<(PathBuf, FoundTest)>,
}

static RUST_TEST: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^\s*#\[\s*(?:[\w:]+::)?(?:test|rstest|test_case)\b").expect("valid")
});
static RUST_FN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\s*(?:pub\s+)?(?:async\s+)?fn\s+(\w+)").expect("valid"));
static RUST_MOD: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(\s*)(?:pub(?:\([^)]*\))?\s+)?mod\s+(\w+)\s*\{").expect("valid")
});
static JS_TEST: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"^\s*(it|test|describe)(?:\.(?:only|skip|concurrent|todo|each\([^)]*\)))?\s*\(\s*(['"`])(.+?)['"`]"#,
    )
    .expect("valid")
});
static PY_DEF: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(\s*)(?:async\s+)?def\s+(test\w*)\s*\(").expect("valid"));
static PY_CLASS: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(\s*)class\s+(Test\w*)").expect("valid"));

fn indent(line: &str) -> usize {
    line.len() - line.trim_start().len()
}

/// The module path of a Rust file inside its crate: `src/a/b.rs` is
/// `a::b`, `src/a/mod.rs` is `a`, `src/lib.rs` and `src/main.rs` nothing.
fn rust_module(path: &Path) -> Vec<String> {
    let parts: Vec<String> = path
        .components()
        .map(|part| part.as_os_str().to_string_lossy().into_owned())
        .collect();
    let Some(src) = parts.iter().rposition(|part| part == "src") else {
        return Vec::new();
    };
    let mut module: Vec<String> = parts[src + 1..]
        .iter()
        .map(|part| part.trim_end_matches(".rs").to_owned())
        .collect();
    if module
        .last()
        .is_some_and(|last| matches!(last.as_str(), "lib" | "main" | "mod"))
    {
        module.pop();
    }
    module
}

/// The tests in a file, by its language.
pub(super) fn find_tests(path: &Path, text: &str) -> Vec<FoundTest> {
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let extension = path
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or_default();
    match extension {
        "rs" => rust_tests(path, text),
        "ts" | "tsx" | "js" | "jsx" | "mjs" | "cjs" | "mts" | "cts"
            if name.contains(".test.") || name.contains(".spec.") =>
        {
            js_tests(text)
        }
        "py" if name.starts_with("test_") || name.ends_with("_test.py") => python_tests(text),
        _ => Vec::new(),
    }
}

fn rust_tests(path: &Path, text: &str) -> Vec<FoundTest> {
    let lines: Vec<&str> = text.lines().collect();
    let module = rust_module(path);
    let mut tests = Vec::new();
    let mut groups: Vec<(usize, Vec<String>)> = Vec::new();
    for (at, line) in lines.iter().enumerate() {
        if !RUST_TEST.is_match(line) {
            continue;
        }
        // The function after the attributes.
        let Some((fn_line, name)) = (at + 1..(at + 6).min(lines.len())).find_map(|next| {
            RUST_FN
                .captures(lines[next])
                .map(|found| (next, found[1].to_owned()))
        }) else {
            continue;
        };
        // The modules it is in: going up, each `mod x {` less indented.
        let mut within = Vec::new();
        let mut depth = indent(lines[fn_line]);
        for above in (0..fn_line).rev() {
            if let Some(found) = RUST_MOD.captures(lines[above]) {
                let mod_indent = found[1].len();
                if mod_indent < depth {
                    within.push((above, found[2].to_owned()));
                    depth = mod_indent;
                }
            }
            if depth == 0 {
                break;
            }
        }
        within.reverse();
        let mut path_parts = module.clone();
        for (mod_line, mod_name) in &within {
            path_parts.push(mod_name.clone());
            if !groups.iter().any(|(line, _)| line == mod_line) {
                groups.push((*mod_line, path_parts.clone()));
            }
        }
        path_parts.push(name.clone());
        tests.push(FoundTest {
            line: fn_line,
            name,
            filter: path_parts.join("::"),
            group: false,
        });
    }
    for (line, parts) in groups {
        tests.push(FoundTest {
            line,
            name: parts.last().cloned().unwrap_or_default(),
            filter: format!("{}::", parts.join("::")),
            group: true,
        });
    }
    tests.sort_by_key(|test| test.line);
    tests
}

fn js_tests(text: &str) -> Vec<FoundTest> {
    text.lines()
        .enumerate()
        .filter_map(|(line, content)| {
            let found = JS_TEST.captures(content)?;
            let name = found[3].to_owned();
            Some(FoundTest {
                line,
                filter: regex::escape(&name),
                name,
                group: &found[1] == "describe",
            })
        })
        .collect()
}

fn python_tests(text: &str) -> Vec<FoundTest> {
    let lines: Vec<&str> = text.lines().collect();
    let mut tests = Vec::new();
    for (at, line) in lines.iter().enumerate() {
        if let Some(found) = PY_CLASS.captures(line) {
            tests.push(FoundTest {
                line: at,
                name: found[2].to_owned(),
                filter: found[2].to_owned(),
                group: true,
            });
        } else if let Some(found) = PY_DEF.captures(line) {
            let depth = found[1].len();
            let name = found[2].to_owned();
            // A method of the class above it.
            let class = (depth > 0)
                .then(|| {
                    (0..at).rev().find_map(|above| {
                        PY_CLASS
                            .captures(lines[above])
                            .filter(|class| class[1].len() < depth)
                            .map(|class| class[2].to_owned())
                    })
                })
                .flatten();
            tests.push(FoundTest {
                line: at,
                filter: match class {
                    Some(class) => format!("{class}::{name}"),
                    None => name.clone(),
                },
                name,
                group: false,
            });
        }
    }
    tests
}

/// The folder above `path` that holds `file`, the nearest.
fn nearest(path: &Path, file: &str) -> Option<PathBuf> {
    path.ancestors()
        .skip(1)
        .find(|folder| folder.join(file).is_file())
        .map(Path::to_path_buf)
}

/// The package of a Rust crate, from its `Cargo.toml`.
fn cargo_package(crate_dir: &Path) -> Option<String> {
    let manifest = std::fs::read_to_string(crate_dir.join("Cargo.toml")).ok()?;
    let mut in_package = false;
    for line in manifest.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_package = line == "[package]";
        } else if in_package
            && let Some(value) = line.strip_prefix("name")
            && let Some(value) = value.trim_start().strip_prefix('=')
        {
            return Some(value.trim().trim_matches('"').to_owned());
        }
    }
    None
}

/// How a JavaScript project runs a program of its own: `pnpm exec`, `yarn`,
/// `bunx` or `npx`, by its lock file.
fn js_exec(folder: &Path, root: &Path) -> &'static str {
    let has = |file: &str| folder.join(file).is_file() || root.join(file).is_file();
    if has("pnpm-lock.yaml") {
        "pnpm exec"
    } else if has("yarn.lock") {
        "yarn"
    } else if has("bun.lockb") || has("bun.lock") {
        "bunx"
    } else {
        "npx"
    }
}

/// The command that runs `test` of `path`, from the project's top.
pub(super) fn test_command(root: &Path, path: &Path, test: Option<&FoundTest>) -> Option<String> {
    let extension = path.extension().and_then(|extension| extension.to_str())?;
    match extension {
        "rs" => {
            let crate_dir = nearest(path, "Cargo.toml")?;
            let package = cargo_package(&crate_dir)?;
            let relative = path.strip_prefix(&crate_dir).ok()?;
            // An integration test is a crate of its own, `--test name`.
            let integration = relative
                .components()
                .next()
                .is_some_and(|first| first.as_os_str() == "tests");
            let mut command = format!("cargo test -p {package}");
            let filter = match test {
                Some(test) if integration => test.filter.rsplit("::").next().map(str::to_owned),
                Some(test) => Some(test.filter.clone()),
                None if integration => None,
                None => {
                    let module = rust_module(path);
                    (!module.is_empty()).then(|| format!("{}::", module.join("::")))
                }
            };
            if integration {
                let stem = path.file_stem()?.to_string_lossy().into_owned();
                command.push_str(&format!(" --test {stem}"));
            }
            match (filter, test) {
                (Some(filter), Some(test)) if !test.group && !integration => {
                    command.push_str(&format!(" -- {} --exact", shell_quote(&filter)))
                }
                (Some(filter), _) => command.push_str(&format!(" -- {}", shell_quote(&filter))),
                (None, _) => {}
            }
            Some(command)
        }
        "py" => {
            let relative = path.strip_prefix(root).unwrap_or(path).to_string_lossy();
            let python = if root.join(".venv/bin/python").is_file() {
                ".venv/bin/python"
            } else {
                "python3"
            };
            let node = match test {
                Some(test) => format!("{relative}::{}", test.filter),
                None => relative.into_owned(),
            };
            Some(format!("{python} -m pytest {}", shell_quote(&node)))
        }
        _ => {
            let package_dir = nearest(path, "package.json")?;
            let manifest: serde_json::Value = serde_json::from_str(
                &std::fs::read_to_string(package_dir.join("package.json")).ok()?,
            )
            .ok()?;
            let has = |name: &str| {
                ["dependencies", "devDependencies"]
                    .iter()
                    .any(|section| manifest[section].get(name).is_some())
            };
            let exec = js_exec(&package_dir, root);
            let file = path
                .strip_prefix(&package_dir)
                .unwrap_or(path)
                .to_string_lossy();
            let runner = if has("vitest") {
                format!("{exec} vitest run {}", shell_quote(&file))
            } else if has("jest") {
                format!("{exec} jest {}", shell_quote(&file))
            } else {
                return None;
            };
            let named = match test {
                Some(test) => format!("{runner} -t {}", shell_quote(&test.filter)),
                None => runner,
            };
            Some(if package_dir == root {
                named
            } else {
                let folder = package_dir.strip_prefix(root).unwrap_or(&package_dir);
                format!("cd {} && {named}", shell_quote(&folder.to_string_lossy()))
            })
        }
    }
}

/// The debugging of a test, of a group or of a file's: what its command
/// runs, with a debugger.
fn test_debug_config(root: &Path, path: &Path, test: Option<&FoundTest>) -> Option<DebugConfig> {
    let name = test.map_or_else(
        || {
            path.file_name()
                .map(|name| name.to_string_lossy().into_owned())
        },
        |test| Some(test.name.clone()),
    )?;
    let command = test_command(root, path, test)?;
    match path.extension()?.to_str()? {
        "rs" => {
            let words = debug_config::words(&json!(command));
            let split = words
                .iter()
                .position(|word| word == "--")
                .unwrap_or(words.len());
            // `cargo test --no-run …` makes the program; its own arguments
            // go to it.
            let mut build = vec!["test".to_owned(), "--no-run".to_owned()];
            build.extend(words[2..split].iter().cloned());
            let mut program: Vec<String> = words.iter().skip(split + 1).cloned().collect();
            program.push("--nocapture".to_owned());
            program.push("--test-threads=1".to_owned());
            Some(debug_config::rust_test(&name, build, program, path))
        }
        "py" => {
            let relative = path.strip_prefix(root).unwrap_or(path).to_string_lossy();
            let node = match test {
                Some(test) => format!("{relative}::{}", test.filter),
                None => relative.into_owned(),
            };
            Some(debug_config::test_of(
                &name,
                "debugpy",
                json!({ "module": "pytest", "args": [node, "-s"], "console": "internalConsole", "justMyCode": true }),
            ))
        }
        _ => {
            let folder = nearest(path, "package.json")?;
            let file = path.strip_prefix(&folder).unwrap_or(path);
            let mut args = vec![json!(file)];
            if let Some(test) = test {
                args.extend([json!("-t"), json!(test.filter)]);
            }
            let vitest = command.contains(" vitest ");
            let program = folder.join(if vitest {
                "node_modules/vitest/vitest.mjs"
            } else {
                "node_modules/jest/bin/jest.js"
            });
            if vitest {
                args.insert(0, json!("run"));
                args.push(json!("--no-file-parallelism"));
            } else {
                args.push(json!("--runInBand"));
            }
            Some(debug_config::test_of(
                &name,
                "pwa-node",
                json!({ "program": program, "args": args, "cwd": folder, "skipFiles": ["<node_internals>/**"] }),
            ))
        }
    }
}

/// Every test of the project: `cargo test`, the package's test script, or
/// pytest.
fn all_tests_command(root: &Path) -> Option<String> {
    if root.join("Cargo.toml").is_file() {
        return Some("cargo test".to_owned());
    }
    if let Ok(text) = std::fs::read_to_string(root.join("package.json"))
        && let Ok(manifest) = serde_json::from_str::<serde_json::Value>(&text)
        && manifest["scripts"].get("test").is_some()
    {
        let runner = match js_exec(root, root) {
            "pnpm exec" => "pnpm",
            "yarn" => "yarn",
            "bunx" => "bun run",
            _ => "npm",
        };
        return Some(format!("{runner} test"));
    }
    let python = ["pytest.ini", "pyproject.toml", "setup.cfg", "tox.ini"]
        .iter()
        .any(|file| root.join(file).is_file());
    python.then(|| "python3 -m pytest".to_owned())
}

/// How each test went, from cargo's lines `test a::b ... ok`.
fn each_result(output: &str) -> HashMap<String, Outcome> {
    let mut each = HashMap::new();
    for line in output.lines() {
        let Some(rest) = line.trim().strip_prefix("test ") else {
            continue;
        };
        if let Some((name, result)) = rest.split_once(" ... ") {
            let result = match result.trim() {
                "ok" => Outcome::Passed,
                "FAILED" => Outcome::Failed,
                _ => continue,
            };
            each.insert(name.trim().to_owned(), result);
        }
    }
    each
}

impl Workbench {
    pub(super) fn testing_actions(&self, root: Div, cx: &mut Context<Self>) -> Div {
        root.on_action(
            cx.listener(|this, _: &RunTestAtCursor, window, cx| {
                this.run_test_at_cursor(window, cx)
            }),
        )
        .on_action(cx.listener(|this, _: &RunAllTests, window, cx| this.run_all_tests(window, cx)))
        .on_action(
            cx.listener(|this, _: &RerunLastTests, window, cx| this.rerun_last_tests(window, cx)),
        )
        .on_action(
            cx.listener(|this, _: &RunTestsInFile, window, cx| this.run_file_tests(window, cx)),
        )
        .on_action(cx.listener(|this, _: &DebugTestAtCursor, window, cx| {
            if let Some((path, test)) = this.test_at_cursor(cx) {
                this.debug_found_test(&path, test.as_ref(), window, cx);
            }
        }))
        // The margin's menu, at the test it was opened on.
        .on_action(cx.listener(|this, _: &RunTestHere, window, cx| {
            if let Some((path, test)) = this.testing.menu.take() {
                this.run_found_test(&path, Some(&test), window, cx);
            }
        }))
        .on_action(cx.listener(|this, _: &DebugTestHere, window, cx| {
            if let Some((path, test)) = this.testing.menu.take() {
                this.debug_found_test(&path, Some(&test), window, cx);
            }
        }))
    }

    /// The tests of a document, found again only when its text changed.
    fn tests_of(&self, uri: &str) -> Option<(PathBuf, Rc<Vec<FoundTest>>)> {
        let document = self.documents.get(uri)?;
        let path = document.disk_path()?.to_path_buf();
        let mut found = self.testing.found.borrow_mut();
        if let Some((version, tests)) = found.get(uri)
            && *version == document.version
        {
            return Some((path, tests.clone()));
        }
        let tests = Rc::new(find_tests(&path, &document.synced_text));
        found.insert(uri.to_owned(), (document.version, tests.clone()));
        Some((path, tests))
    }

    /// The margin of ▶ beside an editor's tests, aligned to their lines.
    pub(super) fn render_test_margin(
        &self,
        index: usize,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let pane = self.panes.get(index)?;
        let (path, tests) = self.tests_of(&pane.uri)?;
        if tests.is_empty() {
            return None;
        }
        let editor = pane.editor.read(cx);
        let rows = editor.visible_row_range().unwrap_or(0..0);
        let theme = cx.theme();
        let (success, danger, accent) = (theme.success, theme.danger, theme.primary);
        let mut marks: Vec<(Pixels, Pixels, &'static str, gpui::Hsla, FoundTest)> = Vec::new();
        for test in tests.iter().filter(|test| rows.contains(&test.line)) {
            let offset = editor.text().line_start_offset(test.line);
            let Some(bounds) = editor.range_to_bounds(&(offset..offset)) else {
                continue;
            };
            let (glyph, color) = match self
                .testing
                .results
                .get(&(path.clone(), test.filter.clone()))
            {
                Some(Outcome::Running) => ("◌", accent),
                Some(Outcome::Passed) => ("✓", success),
                Some(Outcome::Failed) => ("✗", danger),
                None if test.group => ("»", success.opacity(0.75)),
                None => ("▶", success.opacity(0.9)),
            };
            marks.push((
                bounds.origin.y,
                bounds.size.height,
                glyph,
                color,
                test.clone(),
            ));
        }
        let placed: Placed = Rc::default();
        let painted = placed.clone();
        let margin = canvas(
            |_, _, _| {},
            move |bounds, _, window, cx| {
                let font = window.text_style().font();
                let size = px(12.);
                let mut spots = Vec::new();
                for (y, height, glyph, color, test) in &marks {
                    let run = TextRun {
                        len: glyph.len(),
                        font: font.clone(),
                        color: *color,
                        background_color: None,
                        underline: None,
                        strikethrough: None,
                    };
                    let shaped = window.text_system().shape_line(
                        SharedString::from(*glyph),
                        size,
                        &[run],
                        None,
                    );
                    let origin = point(
                        bounds.origin.x + (bounds.size.width - shaped.width) / 2.,
                        *y + (*height - size * 1.3) / 2.,
                    );
                    let _ =
                        shaped.paint(origin, size * 1.3, gpui::TextAlign::Left, None, window, cx);
                    spots.push((
                        Bounds::new(
                            point(bounds.origin.x, *y),
                            gpui::size(bounds.size.width, *height),
                        ),
                        test.clone(),
                    ));
                }
                *painted.borrow_mut() = spots;
            },
        )
        .size_full();
        let (menu_placed, menu_path) = (placed.clone(), path.clone());
        let strings = self.strings();
        Some(
            div()
                .id(("test-margin", index))
                .w(px(MARGIN))
                .h_full()
                .flex_none()
                .cursor_pointer()
                .on_mouse_down(
                    MouseButton::Right,
                    cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                        this.testing.menu = menu_placed
                            .borrow()
                            .iter()
                            .find(|(bounds, _)| bounds.contains(&event.position))
                            .map(|(_, test)| (menu_path.clone(), test.clone()));
                        cx.notify();
                    }),
                )
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                        let test = placed
                            .borrow()
                            .iter()
                            .find(|(bounds, _)| bounds.contains(&event.position))
                            .map(|(_, test)| test.clone());
                        if let Some(test) = test {
                            this.run_found_test(&path, Some(&test), window, cx);
                        }
                    }),
                )
                .context_menu(move |menu, _, _| {
                    menu.menu(strings.tests_run_here, Box::new(RunTestHere))
                        .menu(strings.tests_debug_here, Box::new(DebugTestHere))
                })
                .child(margin)
                .into_any_element(),
        )
    }

    /// Debugs a test, a group, or with `None` every test of the file.
    fn debug_found_test(
        &mut self,
        path: &Path,
        test: Option<&FoundTest>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(root) = self.project.root.clone() else {
            return;
        };
        match test_debug_config(&root, path, test) {
            Some(config) => self.start_debugging(config, false, window, cx),
            None => {
                self.status_message = Some(self.strings().tests_none_found.to_owned());
                cx.notify();
            }
        }
    }

    /// Runs a test, a group, or with `None` every test of the file.
    fn run_found_test(
        &mut self,
        path: &Path,
        test: Option<&FoundTest>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let strings = self.strings();
        let Some(root) = self.project.root.clone() else {
            return;
        };
        let Some(command) = test_command(&root, path, test) else {
            self.status_message = Some(strings.tests_none_found.to_owned());
            cx.notify();
            return;
        };
        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        let label = match test {
            Some(test) => test.name.clone(),
            None => fill(strings.tests_file, &[("name", &name)]),
        };
        // Every test of the file, or of the group, waits for the run too.
        let tests = self
            .panes
            .iter()
            .find_map(|pane| {
                self.tests_of(&pane.uri)
                    .filter(|(file, _)| file == path)
                    .map(|(_, tests)| tests)
            })
            .unwrap_or_default();
        let keys = match test {
            Some(test) if test.group => tests
                .iter()
                .filter(|other| other.line >= test.line && other.filter.starts_with(&test.filter))
                .map(|other| (path.to_path_buf(), other.filter.clone()))
                .chain([(path.to_path_buf(), test.filter.clone())])
                .collect(),
            Some(test) => vec![(path.to_path_buf(), test.filter.clone())],
            None => tests
                .iter()
                .map(|test| (path.to_path_buf(), test.filter.clone()))
                .collect(),
        };
        self.start_test_run(
            RunPlan {
                label,
                command,
                keys,
            },
            window,
            cx,
        );
    }

    /// Cmd+; C: the test the cursor is in, or the file's.
    /// The test at the cursor, or the one before it.
    fn test_at_cursor(&self, cx: &App) -> Option<(PathBuf, Option<FoundTest>)> {
        let pane = self.panes.get(self.active)?;
        let line = pane.editor.read(cx).cursor_position().line as usize;
        let (path, tests) = self.tests_of(&pane.uri)?;
        let test = tests
            .iter()
            .rfind(|test| test.line <= line && !test.group)
            .or_else(|| tests.iter().rfind(|test| test.line <= line))
            .cloned();
        Some((path, test))
    }

    fn run_test_at_cursor(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some((path, test)) = self.test_at_cursor(cx) {
            self.run_found_test(&path, test.as_ref(), window, cx);
        }
    }

    fn run_file_tests(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(path) = self
            .panes
            .get(self.active)
            .and_then(|pane| self.documents.get(&pane.uri))
            .and_then(|document| document.disk_path().map(Path::to_path_buf))
        else {
            return;
        };
        self.run_found_test(&path, None, window, cx);
    }

    /// Cmd+; A: every test of the project.
    fn run_all_tests(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let strings = self.strings();
        let Some(command) = self.project.root.as_deref().and_then(all_tests_command) else {
            self.status_message = Some(strings.tests_none_found.to_owned());
            cx.notify();
            return;
        };
        self.start_test_run(
            RunPlan {
                label: strings.tests_all.to_owned(),
                command,
                keys: Vec::new(),
            },
            window,
            cx,
        );
    }

    /// Cmd+; L: the last run again.
    fn rerun_last_tests(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(plan) = self.testing.last.clone() {
            self.start_test_run(plan, window, cx);
        }
    }

    /// The terminal the tests run in: «Test», when its shell is free.
    fn test_terminal(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Entity<TerminalView>> {
        let free = self.terminals.iter().position(|terminal| {
            terminal.name.as_deref() == Some("Test") && terminal.view.read(cx).runs_shell()
        });
        if let Some(index) = free {
            self.active_terminal = index;
            self.layout.show_panel_view(PanelView::Terminal);
            return Some(self.terminals[index].view.clone());
        }
        let spawn = terminal_view::shell_spawn(self.terminal_folder(), cx);
        let view = self.add_terminal(spawn, None, false, window, cx)?;
        self.name_terminal(&view, "Test".to_owned());
        Some(view)
    }

    fn start_test_run(&mut self, plan: RunPlan, window: &mut Window, cx: &mut Context<Self>) {
        let Some(view) = self.test_terminal(window, cx) else {
            return;
        };
        view.update(cx, |view, cx| view.run_at_prompt(&plan.command, cx));
        for key in &plan.keys {
            self.testing.results.insert(key.clone(), Outcome::Running);
        }
        self.testing.next_id += 1;
        self.testing.runs.push(TestRun {
            id: self.testing.next_id,
            label: plan.label.clone(),
            command: plan.command.clone(),
            outcome: Outcome::Running,
            started: Instant::now(),
            duration: None,
            terminal: view.downgrade(),
            shell_command: None,
            keys: plan.keys.clone(),
        });
        if self.testing.runs.len() > KEPT_RUNS {
            self.testing.runs.remove(0);
        }
        self.testing.last = Some(plan);
        cx.notify();
    }

    /// A command ended in a terminal: if it was a test run, how it went.
    pub(super) fn test_command_finished(
        &mut self,
        view: &Entity<TerminalView>,
        command: &ShellCommand,
        cx: &mut Context<Self>,
    ) {
        let Some(text) = command.text.as_deref() else {
            return;
        };
        let Some(run) = self.testing.runs.iter_mut().rev().find(|run| {
            run.outcome == Outcome::Running
                && run.command == text
                && run.terminal.upgrade().as_ref() == Some(view)
        }) else {
            return;
        };
        let outcome = if command.exit == Some(0) {
            Outcome::Passed
        } else {
            Outcome::Failed
        };
        run.outcome = outcome;
        run.duration = Some(run.started.elapsed());
        run.shell_command = Some(command.id);
        let keys = run.keys.clone();
        // Rust says how each test went: `test a::b ... ok`.
        let output = view.read(cx).output_of(command.id).unwrap_or_default();
        let each = each_result(&output);
        for key in keys {
            let result = each.get(&key.1).copied().unwrap_or(outcome);
            self.testing.results.insert(key, result);
        }
        cx.notify();
    }

    /// The Tests view of the bottom panel: the runs, the latest first.
    pub(super) fn render_tests(&self, colors: Colors, cx: &mut Context<Self>) -> AnyElement {
        let strings = self.strings();
        if self.testing.runs.is_empty() {
            return div()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .text_sm()
                .text_color(colors.muted)
                .child(strings.tests_empty)
                .into_any_element();
        }
        let theme = cx.theme();
        let rows = self.testing.runs.iter().rev().map(|run| {
            let id = run.id;
            let (icon, color, state) = match run.outcome {
                Outcome::Running => (IconName::LoaderCircle, theme.primary, strings.tests_running),
                Outcome::Passed => (IconName::CircleCheck, theme.success, strings.tests_passed),
                Outcome::Failed => (IconName::CircleX, theme.danger, strings.tests_failed),
            };
            let mut detail = vec![state.to_owned()];
            if let Some(duration) = run.duration {
                detail.push(super::terminal_view::duration_text(duration));
            }
            let failed = run.outcome == Outcome::Failed;
            div()
                .id(("test-run", id as usize))
                .group("test-run")
                .h(px(28.))
                .px_2()
                .flex()
                .items_center()
                .gap_2()
                .rounded(px(6.))
                .text_sm()
                .hover(|row| row.bg(colors.hover))
                .child(Icon::new(icon).xsmall().text_color(color))
                .child(
                    div()
                        .flex_none()
                        .text_color(colors.text)
                        .child(run.label.clone()),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_xs()
                        .font_family(cx.theme().mono_font_family.clone())
                        .text_color(colors.faint)
                        .child(run.command.clone()),
                )
                .child(
                    div()
                        .flex_none()
                        .text_xs()
                        .text_color(colors.muted)
                        .child(detail.join(" · ")),
                )
                .child(
                    div()
                        .flex()
                        .gap_0p5()
                        .invisible()
                        .group_hover("test-run", |buttons| buttons.visible())
                        .when(failed, |buttons| {
                            buttons.child(
                                Button::new(("test-claude", id as usize))
                                    .ghost()
                                    .xsmall()
                                    .icon(Icon::new(IconName::Sparkles))
                                    .tooltip(strings.terminal_ask_claude)
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        this.ask_claude_about_run(id, window, cx)
                                    })),
                            )
                        })
                        .child(
                            Button::new(("test-output", id as usize))
                                .ghost()
                                .xsmall()
                                .icon(Icon::new(IconName::SquareTerminal))
                                .tooltip(strings.tests_show_output)
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.show_run_output(id, window, cx)
                                })),
                        )
                        .child(
                            Button::new(("test-again", id as usize))
                                .ghost()
                                .xsmall()
                                .icon(Icon::new(IconName::RotateCcw))
                                .tooltip(strings.tests_run_again)
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.run_again(id, window, cx)
                                })),
                        ),
                )
        });
        div()
            .id("test-runs")
            .size_full()
            .overflow_y_scroll()
            .p_1()
            .flex()
            .flex_col()
            .children(rows)
            .into_any_element()
    }

    fn run_again(&mut self, id: u64, window: &mut Window, cx: &mut Context<Self>) {
        let Some(run) = self.testing.runs.iter().find(|run| run.id == id) else {
            return;
        };
        let plan = RunPlan {
            label: run.label.clone(),
            command: run.command.clone(),
            keys: run.keys.clone(),
        };
        self.start_test_run(plan, window, cx);
    }

    fn show_run_output(&mut self, id: u64, window: &mut Window, cx: &mut Context<Self>) {
        let Some(run) = self.testing.runs.iter().find(|run| run.id == id) else {
            return;
        };
        let Some(view) = run.terminal.upgrade() else {
            return;
        };
        if let Some(command) = run.shell_command {
            view.update(cx, |view, cx| view.show_command(command, cx));
        }
        self.reveal_terminal(&view, window, cx);
    }

    fn ask_claude_about_run(&mut self, id: u64, window: &mut Window, cx: &mut Context<Self>) {
        let Some(run) = self.testing.runs.iter().find(|run| run.id == id) else {
            return;
        };
        let (Some(view), Some(command_id)) = (run.terminal.upgrade(), run.shell_command) else {
            return;
        };
        let prompt = {
            let view = view.read(cx);
            view.commands()
                .into_iter()
                .find(|command| command.id == command_id)
                .map(|command| view.claude_prompt_for(&command))
        };
        if let Some(prompt) = prompt {
            self.ask_claude(prompt, window, cx);
        }
    }
}

#[cfg(test)]
mod tests {
    use gpui::{TestAppContext, VisualTestContext};

    use super::*;

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

    fn last_outcome(view: &Entity<Workbench>, cx: &mut VisualTestContext) -> Option<Outcome> {
        view.read_with(cx, |view, _| {
            view.testing.runs.last().map(|run| run.outcome)
        })
    }

    #[gpui::test]
    fn a_run_waits_for_the_prompt_and_reads_how_it_went(cx: &mut TestAppContext) {
        cx.executor().allow_parking();
        cx.update(|cx| {
            gpui_component::init(cx);
            crate::workbench_window::init(cx);
        });
        let folder = tempfile::tempdir().unwrap();
        let base = std::fs::canonicalize(folder.path()).unwrap();
        let home = base.join("home");
        std::fs::create_dir(&home).unwrap();
        std::fs::write(home.join(".zshrc"), "PS1='$ '\n").unwrap();
        let settings = Settings::load(base.join("settings.json"), None);
        let opening = Opening::new(Some(base.clone()), None);
        let (view, cx) =
            cx.add_window_view(|window, cx| Workbench::new(opening, settings, None, window, cx));
        let key = (base.join("src/lib.rs"), "tests::adds".to_owned());
        let plan = |command: &str| RunPlan {
            label: "adds".to_owned(),
            command: command.to_owned(),
            keys: vec![key.clone()],
        };

        view.update_in(cx, |view, window, cx| {
            let mut spawn = terminal_view::shell_spawn(base.clone(), cx);
            spawn.command = Some(("/bin/zsh".into(), vec!["-i".into()]));
            spawn
                .env
                .insert("HOME".into(), home.to_string_lossy().into_owned());
            spawn.integrate_zsh(&home).unwrap();
            let terminal = view.add_terminal(spawn, None, false, window, cx).unwrap();
            view.name_terminal(&terminal, "Test".to_owned());
            // Right away, while zsh is still starting.
            view.start_test_run(plan("echo uno"), window, cx);
        });
        wait_until(cx, "the run to pass", |cx| {
            last_outcome(&view, cx) == Some(Outcome::Passed)
        });
        let screen = view.read_with(cx, |view, cx| {
            assert_eq!(view.terminals.len(), 1);
            assert_eq!(view.testing.results.get(&key), Some(&Outcome::Passed));
            view.terminals[0].view.read(cx).terminal().snapshot().text()
        });
        // Typed at the prompt, not echoed above it as well.
        assert_eq!(screen.matches("echo uno").count(), 1, "{screen}");

        view.update_in(cx, |view, window, cx| {
            view.start_test_run(plan("false"), window, cx)
        });
        wait_until(cx, "the run to fail", |cx| {
            last_outcome(&view, cx) == Some(Outcome::Failed)
        });
        view.read_with(cx, |view, _| {
            // The same terminal, free again, ran it.
            assert_eq!(view.terminals.len(), 1);
            assert_eq!(view.testing.runs.len(), 2);
            assert_eq!(view.testing.results.get(&key), Some(&Outcome::Failed));
            assert!(view.testing.runs[1].duration.is_some());
        });
    }

    #[test]
    fn rust_tests_with_their_paths() {
        let text = "fn a() {}\n\n#[cfg(test)]\nmod tests {\n    use super::*;\n\n    #[test]\n    fn adds() {}\n\n    #[gpui::test]\n    async fn draws(cx: &mut X) {}\n}\n";
        let tests = find_tests(Path::new("/p/crates/editor/src/text/edits.rs"), text);
        let found: Vec<(usize, &str, bool)> = tests
            .iter()
            .map(|test| (test.line, test.filter.as_str(), test.group))
            .collect();
        assert_eq!(
            found,
            vec![
                (3, "text::edits::tests::", true),
                (7, "text::edits::tests::adds", false),
                (10, "text::edits::tests::draws", false),
            ]
        );
    }

    #[test]
    fn js_and_python_tests() {
        let js = "describe('cart', () => {\n  it(\"adds (one)\", () => {})\n  test.only(`total`, async () => {})\n})\n";
        let tests = find_tests(Path::new("/p/src/cart.test.ts"), js);
        let names: Vec<(&str, bool)> = tests
            .iter()
            .map(|test| (test.name.as_str(), test.group))
            .collect();
        assert_eq!(
            names,
            vec![("cart", true), ("adds (one)", false), ("total", false)]
        );
        assert_eq!(tests[1].filter, r"adds \(one\)");
        assert!(find_tests(Path::new("/p/src/cart.ts"), js).is_empty());

        let py = "class TestCart:\n    def test_adds(self):\n        pass\n\ndef test_total():\n    pass\n";
        let tests = find_tests(Path::new("/p/tests/test_cart.py"), py);
        let filters: Vec<&str> = tests.iter().map(|test| test.filter.as_str()).collect();
        assert_eq!(
            filters,
            vec!["TestCart", "TestCart::test_adds", "test_total"]
        );
    }

    #[test]
    fn cargo_says_how_each_test_went() {
        let output = "running 3 tests\ntest a::tests::adds ... ok\ntest a::tests::draws ... FAILED\ntest a::tests::slow ... ignored\n\ntest result: FAILED.";
        let each = each_result(output);
        assert_eq!(each.get("a::tests::adds"), Some(&Outcome::Passed));
        assert_eq!(each.get("a::tests::draws"), Some(&Outcome::Failed));
        assert_eq!(each.get("a::tests::slow"), None);
    }

    #[test]
    fn commands_for_each_runner() {
        let folder = tempfile::tempdir().unwrap();
        let root = folder.path();
        std::fs::create_dir_all(root.join("crates/editor/src")).unwrap();
        std::fs::write(
            root.join("crates/editor/Cargo.toml"),
            "[package]\nname = \"editor\"\nversion = \"0.1.0\"\n\n[dependencies]\nname = 1\n",
        )
        .unwrap();
        let file = root.join("crates/editor/src/text_edits.rs");
        let test = FoundTest {
            line: 3,
            name: "adds".into(),
            filter: "text_edits::tests::adds".into(),
            group: false,
        };
        assert_eq!(
            test_command(root, &file, Some(&test)).as_deref(),
            Some("cargo test -p editor -- text_edits::tests::adds --exact")
        );
        assert_eq!(
            test_command(root, &file, None).as_deref(),
            Some("cargo test -p editor -- text_edits::")
        );

        std::fs::create_dir_all(root.join("web/src")).unwrap();
        std::fs::write(
            root.join("web/package.json"),
            r#"{ "devDependencies": { "vitest": "^3" } }"#,
        )
        .unwrap();
        std::fs::write(root.join("web/pnpm-lock.yaml"), "").unwrap();
        let test = FoundTest {
            line: 1,
            name: "adds one".into(),
            filter: "adds one".into(),
            group: false,
        };
        assert_eq!(
            test_command(root, &root.join("web/src/cart.test.ts"), Some(&test)).as_deref(),
            Some("cd web && pnpm exec vitest run src/cart.test.ts -t 'adds one'")
        );

        let test = FoundTest {
            line: 1,
            name: "test_adds".into(),
            filter: "TestCart::test_adds".into(),
            group: false,
        };
        assert_eq!(
            test_command(root, &root.join("tests/test_cart.py"), Some(&test)).as_deref(),
            Some("python3 -m pytest tests/test_cart.py::TestCart::test_adds")
        );
    }
}
