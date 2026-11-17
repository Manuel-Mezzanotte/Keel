//! Fixing and formatting with the language servers: quick fixes and
//! refactorings (Cmd+.), organize imports, Format Document (Shift+Alt+F)
//! with the project's own Prettier when it has one, and what VS Code does
//! on save: `editor.codeActionsOnSave` and `editor.formatOnSave`.

use std::cell::RefCell;
use std::ops::Range;
use std::rc::Rc;
use std::time::Duration;

use futures::future::{Either, select};
use gpui::SharedString;
use gpui_component::input::CodeActionProvider;
use lsp::lsp_types::{
    self, CodeAction, CodeActionContext, CodeActionKind, CodeActionOrCommand, CodeActionParams,
    CodeActionTriggerKind, DocumentFormattingParams, ExecuteCommandParams, FormattingOptions,
    TextDocumentIdentifier,
    request::{CodeActionRequest, CodeActionResolveRequest, ExecuteCommand, Formatting},
};

use super::language_features::{LanguageFeatures, apply_text_edits};
use super::language_servers::RunningServer;
use super::problems::LineIndex;
use super::*;

actions!(keel, [FormatDocument, OrganizeImports]);

/// How long saving waits for fixes and formatting before it saves anyway.
const ON_SAVE_TIMEOUT: Duration = Duration::from_secs(3);

/// The languages Prettier formats.
const PRETTIER_LANGUAGES: &[&str] = &[
    "typescript",
    "typescriptreact",
    "javascript",
    "javascriptreact",
    "json",
    "jsonc",
    "css",
    "scss",
    "less",
    "html",
    "markdown",
    "yaml",
];

/// The actions offered last by the Cmd+. menu, with the server of each,
/// since gpui-component hands back only the action.
#[derive(Default)]
pub(super) struct OfferedActions {
    actions: Vec<(CodeAction, Rc<RunningServer>)>,
}

impl CodeActionProvider for LanguageFeatures {
    fn id(&self) -> SharedString {
        "keel-language-servers".into()
    }

    fn code_actions(
        &self,
        state: Entity<EditorState>,
        range: Range<usize>,
        _window: &mut Window,
        cx: &mut App,
    ) -> Task<anyhow::Result<Vec<CodeAction>>> {
        let text = state.read(cx).value().to_string();
        self.state.sync(&self.uri, &text);
        let lines = LineIndex::new(&text);
        let position = |offset: usize| {
            let position = workbench::position::position_of(&text, offset.min(text.len()));
            lsp_types::Position::new(position.line, position.character)
        };
        let lsp_range = lsp_types::Range::new(position(range.start), position(range.end));
        let _ = lines;
        let Ok(uri) = self.uri.parse::<lsp_types::Uri>() else {
            return Task::ready(Ok(Vec::new()));
        };
        let requests: Vec<_> = self
            .state
            .servers_for(&self.uri)
            .into_iter()
            .filter(|running| {
                running
                    .server
                    .capabilities()
                    .is_some_and(|capabilities| capabilities.code_action_provider.is_some())
            })
            .map(|running| {
                // The server's own problems that touch the range.
                let diagnostics = self
                    .state
                    .diagnostics_of(&self.uri, running.spec.id)
                    .into_iter()
                    .filter(|diagnostic| {
                        diagnostic.range.start <= lsp_range.end
                            && lsp_range.start <= diagnostic.range.end
                    })
                    .collect();
                let request = running
                    .server
                    .request::<CodeActionRequest>(CodeActionParams {
                        text_document: TextDocumentIdentifier::new(uri.clone()),
                        range: lsp_range,
                        context: CodeActionContext {
                            diagnostics,
                            only: None,
                            trigger_kind: Some(CodeActionTriggerKind::INVOKED),
                        },
                        work_done_progress_params: Default::default(),
                        partial_result_params: Default::default(),
                    });
                (running, request)
            })
            .collect();
        let offered = self.offered.clone();
        cx.spawn(async move |_| {
            let mut found = Vec::new();
            for (running, request) in requests {
                let Ok(Some(actions)) = request.await else {
                    continue;
                };
                for action in actions {
                    let action = match action {
                        CodeActionOrCommand::CodeAction(action) => action,
                        CodeActionOrCommand::Command(command) => CodeAction {
                            title: command.title.clone(),
                            command: Some(command),
                            ..Default::default()
                        },
                    };
                    // Disabled actions say why they cannot run: not offered.
                    if action.disabled.is_none() {
                        found.push((action, running.clone()));
                    }
                }
            }
            // Preferred fixes first, as in VS Code.
            found.sort_by_key(|(action, _)| !action.is_preferred.unwrap_or(false));
            let actions = found.iter().map(|(action, _)| action.clone()).collect();
            offered.borrow_mut().actions = found;
            Ok(actions)
        })
    }

    fn perform_code_action(
        &self,
        _state: Entity<EditorState>,
        action: CodeAction,
        _push_to_history: bool,
        window: &mut Window,
        cx: &mut App,
    ) -> Task<anyhow::Result<()>> {
        let running = self
            .offered
            .borrow()
            .actions
            .iter()
            .find(|(offered, _)| *offered == action)
            .map(|(_, running)| running.clone());
        let Some(running) = running else {
            return Task::ready(Ok(()));
        };
        let workbench = self.workbench.clone();
        let handle = window.window_handle();
        cx.spawn(async move |cx| {
            let action = resolve(&running, action).await;
            handle
                .update(cx, |_, window, cx| {
                    workbench
                        .update(cx, |this, cx| {
                            this.run_code_action(&running, &action, window, cx)
                        })
                        .ok();
                })
                .ok();
            Ok(())
        })
    }
}

/// Asks the server for an action's edit when it left it out.
async fn resolve(running: &RunningServer, action: CodeAction) -> CodeAction {
    let resolves = running
        .server
        .capabilities()
        .and_then(
            |capabilities| match capabilities.code_action_provider.as_ref()? {
                lsp_types::CodeActionProviderCapability::Options(options) => {
                    options.resolve_provider
                }
                lsp_types::CodeActionProviderCapability::Simple(_) => None,
            },
        )
        .unwrap_or(false);
    if action.edit.is_some() || !resolves {
        return action;
    }
    running
        .server
        .request::<CodeActionResolveRequest>(action.clone())
        .await
        .unwrap_or(action)
}

impl Workbench {
    /// Applies an action's edit, then runs its command on the server.
    fn run_code_action(
        &mut self,
        running: &RunningServer,
        action: &CodeAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(edit) = &action.edit {
            self.apply_workspace_edit(edit, window, cx);
        }
        if let Some(command) = &action.command {
            let arguments = command.arguments.clone().unwrap_or_default();
            if self.client_command(&command.command, &arguments, window, cx) {
                return;
            }
            let request = running
                .server
                .request::<ExecuteCommand>(ExecuteCommandParams {
                    command: command.command.clone(),
                    arguments: command.arguments.clone().unwrap_or_default(),
                    work_done_progress_params: Default::default(),
                });
            cx.background_spawn(async move {
                let _ = request.await;
            })
            .detach();
        }
    }

    /// The setting `key` for `language_id`: from `"[language]": { ... }`
    /// first, as VS Code reads it, then the general one.
    pub(super) fn language_setting(&self, language_id: &str, key: &str) -> Option<Value> {
        self.settings
            .get(&format!("[{language_id}]"))
            .and_then(|overrides| overrides.get(key))
            .or_else(|| self.settings.get(key))
            .cloned()
    }

    /// The kinds `editor.codeActionsOnSave` asks for, such as
    /// `source.fixAll.eslint`. "never" and `false` turn one off.
    fn code_actions_on_save(&self, language_id: &str) -> Vec<String> {
        match self.language_setting(language_id, "editor.codeActionsOnSave") {
            Some(Value::Object(kinds)) => kinds
                .into_iter()
                .filter(|(_, when)| match when {
                    Value::Bool(on) => *on,
                    Value::String(when) => when != "never",
                    _ => false,
                })
                .map(|(kind, _)| kind)
                .collect(),
            Some(Value::Array(kinds)) => kinds
                .into_iter()
                .filter_map(|kind| kind.as_str().map(str::to_owned))
                .collect(),
            _ => Vec::new(),
        }
    }

    /// Cmd+S: fixes and formats as the settings say, then saves.
    pub(super) fn save_with_participants(
        &mut self,
        uri: &str,
        after: Option<AfterConfirm>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(document) = self.documents.get(uri) else {
            return;
        };
        let language_id = document.language_id.clone();
        let kinds = self.code_actions_on_save(&language_id);
        let format = self
            .language_setting(&language_id, "editor.formatOnSave")
            .and_then(|value| value.as_bool())
            .unwrap_or(false);
        let Some(index) = self.pane_index(uri).filter(|_| !kinds.is_empty() || format) else {
            self.save_file(uri, after, window, cx);
            return;
        };
        let editor = self.panes[index].editor.clone();
        let uri = uri.to_owned();
        cx.spawn_in(window, async move |this, cx| {
            let timeout = cx.background_executor().timer(ON_SAVE_TIMEOUT);
            let timed_out = {
                let work = async {
                    for kind in kinds {
                        let Ok(task) = this.update_in(cx, |this, window, cx| {
                            this.apply_code_actions_of_kind(&uri, &editor, &kind, window, cx)
                        }) else {
                            return;
                        };
                        task.await;
                    }
                    if format {
                        let Ok(task) = this.update_in(cx, |this, window, cx| {
                            this.format_document(&uri, &editor, window, cx)
                        }) else {
                            return;
                        };
                        task.await;
                    }
                };
                futures::pin_mut!(work);
                matches!(select(work, timeout).await, Either::Right(_))
            };
            if timed_out {
                this.update(cx, |this, _| {
                    this.log(this.strings().format_timed_out.to_owned())
                })
                .ok();
            }
            this.update_in(cx, |this, window, cx| {
                // The fixes reached the document through the editor's changes.
                this.sync_document_from(&uri, None, window, cx);
                this.save_file(&uri, after, window, cx);
            })
            .ok();
        })
        .detach();
    }

    /// Runs every action of `kind` (such as `source.organizeImports`) the
    /// servers offer for the whole file. The task ends when they are applied.
    fn apply_code_actions_of_kind(
        &mut self,
        uri: &str,
        editor: &Entity<EditorState>,
        kind: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Task<()> {
        let text = editor.read(cx).value().to_string();
        self.languages.sync(uri, &text);
        let end = workbench::position::position_of(&text, text.len());
        let range = lsp_types::Range::new(
            lsp_types::Position::new(0, 0),
            lsp_types::Position::new(end.line, end.character),
        );
        let Ok(parsed) = uri.parse::<lsp_types::Uri>() else {
            return Task::ready(());
        };
        let requests: Vec<_> = self
            .languages
            .servers_for(uri)
            .into_iter()
            .filter(|running| {
                running
                    .server
                    .capabilities()
                    .is_some_and(|capabilities| capabilities.code_action_provider.is_some())
            })
            .map(|running| {
                let diagnostics = self.languages.state.diagnostics_of(uri, running.spec.id);
                let request = running
                    .server
                    .request::<CodeActionRequest>(CodeActionParams {
                        text_document: TextDocumentIdentifier::new(parsed.clone()),
                        range,
                        context: CodeActionContext {
                            diagnostics,
                            only: Some(vec![CodeActionKind::from(kind.to_owned())]),
                            trigger_kind: Some(CodeActionTriggerKind::AUTOMATIC),
                        },
                        work_done_progress_params: Default::default(),
                        partial_result_params: Default::default(),
                    });
                (running, request)
            })
            .collect();
        let kind = kind.to_owned();
        cx.spawn_in(window, async move |this, cx| {
            for (running, request) in requests {
                let Ok(Some(actions)) = request.await else {
                    continue;
                };
                for action in actions {
                    let CodeActionOrCommand::CodeAction(action) = action else {
                        continue;
                    };
                    // `source.fixAll` also matches `source.fixAll.eslint`.
                    let matches = action.kind.as_ref().is_some_and(|found| {
                        found.as_str() == kind || found.as_str().starts_with(&format!("{kind}."))
                    });
                    if !matches || action.disabled.is_some() {
                        continue;
                    }
                    let action = resolve(&running, action).await;
                    this.update_in(cx, |this, window, cx| {
                        this.run_code_action(&running, &action, window, cx)
                    })
                    .ok();
                }
            }
        })
    }

    /// Formats a whole file: with the project's Prettier when it is the
    /// formatter, otherwise with the language server.
    pub(super) fn format_document(
        &mut self,
        uri: &str,
        editor: &Entity<EditorState>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Task<()> {
        let Some(document) = self.documents.get(uri) else {
            return Task::ready(());
        };
        let language_id = document.language_id.clone();
        let path = document.disk_path().map(Path::to_path_buf);
        let text = editor.read(cx).value().to_string();
        let formatter = self
            .language_setting(&language_id, "editor.defaultFormatter")
            .and_then(|value| value.as_str().map(str::to_owned));
        let prettier = self.project_prettier().filter(|_| {
            PRETTIER_LANGUAGES.contains(&language_id.as_str())
                && formatter
                    .as_deref()
                    .is_none_or(|formatter| formatter == "esbenp.prettier-vscode")
        });
        let editor = editor.downgrade();
        if let (Some(prettier), Some(path)) = (prettier, path) {
            let root = self.project.root.clone().unwrap_or_default();
            let run =
                cx.background_spawn(async move { run_prettier(&prettier, &root, &path, &text) });
            return cx.spawn_in(window, async move |this, cx| match run.await {
                Ok(formatted) => {
                    if let Some(editor) = editor.upgrade() {
                        cx.update(|window, cx| tabs::replace_text(&editor, &formatted, window, cx))
                            .ok();
                    }
                }
                Err(error) => {
                    this.update(cx, |this, cx| {
                        this.report(format!("Prettier: {error}"));
                        cx.notify();
                    })
                    .ok();
                }
            });
        }
        self.languages.sync(uri, &text);
        let running = self.languages.servers_for(uri).into_iter().find(|running| {
            running
                .server
                .capabilities()
                .is_some_and(|capabilities| capabilities.document_formatting_provider.is_some())
        });
        let (Some(running), Ok(parsed)) = (running, uri.parse::<lsp_types::Uri>()) else {
            return Task::ready(());
        };
        let tab_size = self.tab_size();
        let request = running
            .server
            .request::<Formatting>(DocumentFormattingParams {
                text_document: TextDocumentIdentifier::new(parsed),
                options: FormattingOptions {
                    tab_size: tab_size.tab_size as u32,
                    insert_spaces: !tab_size.hard_tabs,
                    trim_trailing_whitespace: self
                        .language_setting(&language_id, "files.trimTrailingWhitespace")
                        .and_then(|value| value.as_bool()),
                    insert_final_newline: self
                        .language_setting(&language_id, "files.insertFinalNewline")
                        .and_then(|value| value.as_bool()),
                    ..Default::default()
                },
                work_done_progress_params: Default::default(),
            });
        cx.spawn_in(window, async move |_, cx| {
            let Ok(Some(edits)) = request.await else {
                return;
            };
            if let Some(editor) = editor.upgrade() {
                cx.update(|window, cx| apply_text_edits(&editor, &edits, window, cx))
                    .ok();
            }
        })
    }

    /// The project's own Prettier, from its `node_modules`.
    fn project_prettier(&self) -> Option<PathBuf> {
        let root = self.project.root.as_ref()?;
        let prettier = root.join("node_modules/.bin/prettier");
        prettier.exists().then_some(prettier)
    }

    fn format_active(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(pane) = self.panes.get(self.active) else {
            return;
        };
        let (uri, editor) = (pane.uri.clone(), pane.editor.clone());
        self.format_document(&uri, &editor, window, cx).detach();
    }

    fn organize_imports(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(pane) = self.panes.get(self.active) else {
            return;
        };
        let (uri, editor) = (pane.uri.clone(), pane.editor.clone());
        self.apply_code_actions_of_kind(&uri, &editor, "source.organizeImports", window, cx)
            .detach();
    }

    pub(super) fn code_action_actions(&self, root: gpui::Div, cx: &mut Context<Self>) -> gpui::Div {
        root.on_action(
            cx.listener(|this, _: &FormatDocument, window, cx| this.format_active(window, cx)),
        )
        .on_action(
            cx.listener(|this, _: &OrganizeImports, window, cx| this.organize_imports(window, cx)),
        )
    }
}

/// Runs Prettier on `text` as if it were the file at `path`, so the
/// project's configuration and ignore file apply. Returns the formatted
/// text; a file Prettier ignores comes back unchanged.
fn run_prettier(prettier: &Path, root: &Path, path: &Path, text: &str) -> anyhow::Result<String> {
    use std::io::Write as _;
    // Tests keep to Keel's own environment instead of the user's shell.
    #[cfg(not(test))]
    let env = lsp::servers::login_environment().clone();
    #[cfg(test)]
    let env: std::collections::HashMap<String, String> = std::env::vars().collect();
    let mut child = std::process::Command::new(prettier)
        .arg("--stdin-filepath")
        .arg(path)
        .current_dir(root)
        .envs(&env)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()?;
    child
        .stdin
        .take()
        .ok_or_else(|| anyhow::anyhow!("no stdin"))?
        .write_all(text.as_bytes())?;
    let output = child.wait_with_output()?;
    if !output.status.success() {
        let error = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("{}", error.lines().next().unwrap_or("failed"));
    }
    let formatted = String::from_utf8(output.stdout)?;
    // An ignored file prints nothing.
    if formatted.is_empty() && !text.is_empty() {
        return Ok(text.to_owned());
    }
    Ok(formatted)
}

/// Keeps the actions offered by one editor's Cmd+. menu.
pub(super) type Offered = Rc<RefCell<OfferedActions>>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prettier_formats_through_its_command_line() {
        // A stand-in Prettier that upper-cases what it reads.
        let folder = tempfile::tempdir().unwrap();
        let prettier = folder.path().join("prettier");
        std::fs::write(&prettier, "#!/bin/sh\ntr a-z A-Z\n").unwrap();
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(&prettier, std::fs::Permissions::from_mode(0o755)).unwrap();
        let path = folder.path().join("app.ts");
        let formatted = run_prettier(&prettier, folder.path(), &path, "const a = 1;\n").unwrap();
        assert_eq!(formatted, "CONST A = 1;\n");

        std::fs::write(
            &prettier,
            "#!/bin/sh\necho 'SyntaxError: nope' >&2\nexit 2\n",
        )
        .unwrap();
        let error = run_prettier(&prettier, folder.path(), &path, "x").unwrap_err();
        assert_eq!(error.to_string(), "SyntaxError: nope");
    }
}

#[cfg(test)]
mod window_tests {
    use std::time::Instant;

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
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    /// A project with `app.ts`, open, and `settings` as the user's.
    fn window<'a>(
        cx: &'a mut TestAppContext,
        settings: &str,
    ) -> (
        tempfile::TempDir,
        PathBuf,
        Entity<Workbench>,
        &'a mut VisualTestContext,
    ) {
        cx.executor().allow_parking();
        cx.update(|cx| {
            gpui_component::init(cx);
            language::register_grammars();
            crate::workbench_window::init(cx);
        });
        let folder = tempfile::tempdir().unwrap();
        let base = std::fs::canonicalize(folder.path()).unwrap();
        std::fs::write(base.join("app.ts"), "const a = 1; // TODO\n").unwrap();
        std::fs::write(base.join("settings.json"), settings).unwrap();
        let settings = Settings::load(base.join("settings.json"), None);
        let opening = Opening::new(Some(base.clone()), None);
        let (view, cx) = cx.add_window_view(|window, cx| {
            let mut workbench = Workbench::new(opening, settings, None, window, cx);
            workbench.languages.fake = true;
            workbench
        });
        let path = base.join("app.ts");
        view.update_in(cx, |view, window, cx| view.open_file(&path, window, cx));
        wait_until(cx, "the warning", |cx| {
            view.read_with(cx, |view, _| view.problems.warnings == 1)
        });
        (folder, base, view, cx)
    }

    fn text(view: &Entity<Workbench>, cx: &mut VisualTestContext) -> String {
        view.read_with(cx, |view, cx| {
            view.active_editor().unwrap().read(cx).value().to_string()
        })
    }

    #[gpui::test]
    fn quick_fixes_resolve_their_edit_and_commands_run(cx: &mut TestAppContext) {
        let (_folder, _base, view, cx) = window(cx, "{}");
        let found: Rc<RefCell<Option<Vec<CodeAction>>>> = Default::default();
        view.update_in(cx, |view, window, cx| {
            let editor = view.active_editor().unwrap();
            let provider = editor.read(cx).lsp().code_action_providers[0].clone();
            // The cursor on "TODO".
            let task = provider.code_actions(editor, 17..17, window, cx);
            let found = found.clone();
            cx.spawn(async move |_, _| *found.borrow_mut() = task.await.ok())
                .detach();
        });
        wait_until(cx, "the actions", |_| found.borrow().is_some());
        let actions = found.take().unwrap();
        let titles: Vec<&str> = actions.iter().map(|action| action.title.as_str()).collect();
        // The preferred fix comes first.
        assert_eq!(titles, ["Replace with DONE", "Organize", "Run a command"]);
        for action in [actions[0].clone(), actions[2].clone()] {
            view.update_in(cx, |view, window, cx| {
                let editor = view.active_editor().unwrap();
                let provider = editor.read(cx).lsp().code_action_providers[0].clone();
                provider
                    .perform_code_action(editor, action, true, window, cx)
                    .detach();
            });
        }
        wait_until(cx, "the fix", |cx| {
            text(&view, cx) == "const a = 1; // DONE\n"
        });
        wait_until(cx, "the command", |cx| {
            view.read_with(cx, |view, _| {
                view.output
                    .iter()
                    .any(|(_, line)| line.contains("executed fake.command [1]"))
            })
        });
    }

    #[gpui::test]
    fn saving_fixes_then_formats_then_writes(cx: &mut TestAppContext) {
        let (_folder, base, view, cx) = window(
            cx,
            r#"{
                "[typescript]": { "editor.formatOnSave": true },
                "editor.codeActionsOnSave": { "source.organizeImports": "explicit", "source.fixAll": "never" }
            }"#,
        );
        view.update_in(cx, |view, window, cx| {
            let uri = view.panes[view.active].uri.clone();
            view.save_document(&uri, None, window, cx);
        });
        let expected = "// formatted\n// organized\nconst a = 1; // TODO\n";
        wait_until(cx, "the saved file", |_| {
            std::fs::read_to_string(base.join("app.ts")).unwrap() == expected
        });
        assert_eq!(text(&view, cx), expected);
    }

    #[gpui::test]
    fn the_projects_prettier_formats(cx: &mut TestAppContext) {
        let (_folder, base, view, cx) = window(cx, "{}");
        let bin = base.join("node_modules/.bin");
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::write(bin.join("prettier"), "#!/bin/sh\ntr a-z A-Z\n").unwrap();
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(bin.join("prettier"), std::fs::Permissions::from_mode(0o755))
            .unwrap();
        view.update_in(cx, |view, window, cx| view.format_active(window, cx));
        wait_until(cx, "Prettier's text", |cx| {
            text(&view, cx) == "CONST A = 1; // TODO\n"
        });
    }
}
