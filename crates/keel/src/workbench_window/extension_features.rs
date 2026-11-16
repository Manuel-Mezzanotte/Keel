//! What extensions add to the editor through the `vscode` bridge besides
//! completions: their diagnostics (`createDiagnosticCollection`), shown with
//! the language servers' in the Problems view and as underlines, and their
//! quick fixes (`registerCodeActionsProvider`) in the Cmd+. menu.

use std::cell::RefCell;
use std::collections::BTreeSet;
use std::ops::Range;
use std::sync::{Mutex, OnceLock};

use gpui::SharedString;
use gpui_component::input::CodeActionProvider;
use lsp::lsp_types::{self, CodeAction, CodeActionKind, Diagnostic};

use super::language_servers::LanguageState;
use super::*;

/// Collection names live as long as Keel, like server ids: the diagnostics
/// map is keyed by `&'static str`. Each name is kept once.
fn owner_name(name: &str) -> &'static str {
    static NAMES: OnceLock<Mutex<BTreeSet<&'static str>>> = OnceLock::new();
    let mut names = NAMES.get_or_init(Default::default).lock().unwrap();
    if let Some(known) = names.get(name) {
        return known;
    }
    let leaked: &'static str = Box::leak(name.to_owned().into_boxed_str());
    names.insert(leaked);
    leaked
}

/// Whether `owner` is a collection of an extension rather than a server.
fn is_extension_owner(owner: &str) -> bool {
    EXTENSION_OWNERS.with(|owners| owners.borrow().contains(owner))
}

thread_local! {
    /// The collection names extensions used in this process.
    static EXTENSION_OWNERS: RefCell<BTreeSet<&'static str>> = const { RefCell::new(BTreeSet::new()) };
}

impl Workbench {
    /// `diagnostics/set`: an extension's problems for one file.
    pub(super) fn set_extension_diagnostics(&mut self, params: &Value, cx: &mut Context<Self>) {
        let Some(uri) = params["uri"].as_str().map(str::to_owned) else {
            return;
        };
        let owner = owner_name(params["owner"].as_str().unwrap_or("extension"));
        EXTENSION_OWNERS.with(|owners| owners.borrow_mut().insert(owner));
        let diagnostics: Vec<Diagnostic> = params["diagnostics"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|diagnostic| serde_json::from_value(diagnostic.clone()).ok())
            .collect();
        let by_owner = self.languages.diagnostics.entry(uri.clone()).or_default();
        if diagnostics.is_empty() {
            by_owner.remove(owner);
        } else {
            by_owner.insert(owner, diagnostics);
        }
        if by_owner.is_empty() {
            self.languages.diagnostics.remove(&uri);
        }
        self.diagnostics_changed(&uri, cx);
    }

    /// `diagnostics/clear`, or every collection when `owner` is none (the
    /// extension host stopped).
    pub(super) fn clear_extension_diagnostics(
        &mut self,
        owner: Option<&str>,
        cx: &mut Context<Self>,
    ) {
        let mut changed = Vec::new();
        for (uri, by_owner) in self.languages.diagnostics.iter_mut() {
            let before = by_owner.len();
            by_owner.retain(|name, _| match owner {
                Some(owner) => *name != owner,
                None => !is_extension_owner(name),
            });
            if by_owner.len() != before {
                changed.push(uri.clone());
            }
        }
        self.languages
            .diagnostics
            .retain(|_, by_owner| !by_owner.is_empty());
        for uri in changed {
            self.diagnostics_changed(&uri, cx);
        }
    }

    /// The extensions' quick fixes for pane `index`, if a provider matches.
    pub(super) fn extension_code_actions(&self, index: usize) -> Option<Rc<ExtensionCodeActions>> {
        let extensions = self.extensions.as_ref()?;
        let pane = self.panes.get(index)?;
        let document = self.documents.get(&pane.uri)?;
        let scheme = Uri::parse(&document.uri)
            .map(|uri| uri.scheme)
            .unwrap_or_default();
        extensions
            .code_action_providers
            .iter()
            .any(|(_, selector)| selector_matches(selector, &scheme, &document.language_id))
            .then(|| {
                Rc::new(ExtensionCodeActions {
                    host: extensions.host.clone(),
                    state: self.languages.state.clone(),
                    uri: pane.uri.clone(),
                })
            })
    }
}

/// Asks the extensions' code action providers, as `LanguageFeatures` asks
/// the servers.
pub(super) struct ExtensionCodeActions {
    host: Rc<ExtensionHost>,
    state: Rc<LanguageState>,
    uri: String,
}

impl CodeActionProvider for ExtensionCodeActions {
    fn id(&self) -> SharedString {
        "keel-extensions".into()
    }

    fn code_actions(
        &self,
        state: Entity<EditorState>,
        range: Range<usize>,
        _window: &mut Window,
        cx: &mut App,
    ) -> Task<anyhow::Result<Vec<CodeAction>>> {
        let text = state.read(cx).value().to_string();
        let position = |offset: usize| {
            let position = workbench::position::position_of(&text, offset.min(text.len()));
            lsp_types::Position::new(position.line, position.character)
        };
        let range = lsp_types::Range::new(position(range.start), position(range.end));
        // Every problem of the file that touches the range, as VS Code
        // gives the providers.
        let diagnostics: Vec<Diagnostic> = self
            .state
            .all_diagnostics(&self.uri)
            .into_iter()
            .filter(|diagnostic| {
                diagnostic.range.start <= range.end && range.start <= diagnostic.range.end
            })
            .collect();
        let call = self.host.call(
            "codeActions/provide",
            json!({ "uri": self.uri, "range": range, "diagnostics": diagnostics }),
        );
        cx.spawn(async move |_| {
            let answer = call.await.unwrap_or(Value::Null);
            let mut actions: Vec<CodeAction> = answer
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|item| {
                    // The extension keeps the action; Keel keeps its handle.
                    let handle = item["handle"].as_u64()?;
                    Some(CodeAction {
                        title: item["title"].as_str().unwrap_or_default().to_owned(),
                        kind: item["kind"]
                            .as_str()
                            .map(|kind| CodeActionKind::from(kind.to_owned())),
                        is_preferred: item["isPreferred"].as_bool(),
                        data: Some(json!({ "keelExtensionAction": handle })),
                        ..Default::default()
                    })
                })
                .collect();
            actions.sort_by_key(|action| !action.is_preferred.unwrap_or(false));
            Ok(actions)
        })
    }

    fn perform_code_action(
        &self,
        _state: Entity<EditorState>,
        action: CodeAction,
        _push_to_history: bool,
        _window: &mut Window,
        cx: &mut App,
    ) -> Task<anyhow::Result<()>> {
        let handle = action
            .data
            .as_ref()
            .and_then(|data| data["keelExtensionAction"].as_u64());
        let Some(handle) = handle else {
            return Task::ready(Ok(()));
        };
        let call = self
            .host
            .call("codeActions/run", json!({ "handle": handle }));
        cx.spawn(async move |_| {
            call.await?;
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use gpui::{TestAppContext, VisualTestContext};

    use super::super::tests::test_settings;
    use super::*;

    fn wait_until(
        cx: &mut VisualTestContext,
        what: &str,
        check: impl Fn(&mut VisualTestContext) -> bool,
    ) {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            cx.executor().advance_clock(Duration::from_millis(100));
            cx.run_until_parked();
            if check(cx) {
                return;
            }
            assert!(Instant::now() < deadline, "timed out waiting for {what}");
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    /// Like markdownlint: trailing spaces are a warning, with a fix.
    const LINTER: &str = r#"
const vscode = require("vscode");
exports.activate = (context) => {
  const collection = vscode.languages.createDiagnosticCollection("lint");
  const lint = (document) => {
    if (document.languageId !== "markdown") return;
    const found = [];
    document.getText().split("\n").forEach((line, number) => {
      const trimmed = line.replace(/ +$/, "");
      if (trimmed.length !== line.length) {
        const range = new vscode.Range(number, trimmed.length, number, line.length);
        const diagnostic = new vscode.Diagnostic(range, "Trailing spaces", vscode.DiagnosticSeverity.Warning);
        diagnostic.source = "lint";
        found.push(diagnostic);
      }
    });
    collection.set(document.uri, found);
  };
  context.subscriptions.push(
    collection,
    vscode.workspace.onDidOpenTextDocument(lint),
    vscode.workspace.onDidChangeTextDocument((event) => lint(event.document)),
    vscode.languages.registerCodeActionsProvider({ language: "markdown" }, {
      provideCodeActions(document, range, context) {
        return context.diagnostics
          .filter((diagnostic) => diagnostic.source === "lint")
          .map((diagnostic) => {
            const action = new vscode.CodeAction("Remove trailing spaces", vscode.CodeActionKind.QuickFix);
            action.edit = new vscode.WorkspaceEdit();
            action.edit.delete(document.uri, diagnostic.range);
            action.isPreferred = true;
            return action;
          });
      },
    }),
  );
  vscode.workspace.textDocuments.forEach(lint);
};
"#;

    #[gpui::test]
    fn extension_problems_are_shown_and_fixed(cx: &mut TestAppContext) {
        cx.executor().allow_parking();
        cx.update(|cx| {
            gpui_component::init(cx);
            language::register_grammars();
            init(cx);
        });
        let data = tempfile::tempdir().unwrap();
        let root = crate::paths::extensions_dir(data.path()).join("keel-tests.lint-1.0.0");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(
            root.join("package.json"),
            r#"{ "name": "lint", "publisher": "keel-tests", "version": "1.0.0",
                 "main": "./extension.js", "activationEvents": ["onLanguage:markdown"] }"#,
        )
        .unwrap();
        std::fs::write(root.join("extension.js"), LINTER).unwrap();
        let project = tempfile::tempdir().unwrap();
        let path = project.path().join("notes.md");
        std::fs::write(&path, "# Notes   \nfine\n").unwrap();
        let mut setup = crate::windows::extension_setup(data.path()).unwrap();
        setup.marketplace = None;
        let document = Document::open(&path).unwrap();
        let (view, cx) = cx.add_window_view(|window, cx| {
            Workbench::new(Some(document), test_settings(), Some(setup), window, cx)
        });

        // The warning reaches the Problems view, under the extension's name.
        wait_until(cx, "the problem", |cx| {
            view.read_with(cx, |view, _| view.problems.warnings == 1)
        });
        view.read_with(cx, |view, _| {
            let (_, problems) = &view.problems.files[0];
            assert_eq!(problems[0].server, "lint");
            assert_eq!(problems[0].diagnostic.message, "Trailing spaces");
        });

        // Cmd+. offers the extension's fix, and it removes the spaces.
        let found: Rc<RefCell<Option<Vec<CodeAction>>>> = Default::default();
        view.update_in(cx, |view, window, cx| {
            let editor = view.active_editor().unwrap();
            let providers = editor.read(cx).lsp().code_action_providers.clone();
            assert_eq!(providers.len(), 2, "the servers' and the extensions'");
            let task = providers[1].code_actions(editor, 8..8, window, cx);
            let found = found.clone();
            cx.spawn(async move |_, _| *found.borrow_mut() = task.await.ok())
                .detach();
        });
        wait_until(cx, "the fix", |_| found.borrow().is_some());
        let actions = found.take().unwrap();
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].title, "Remove trailing spaces");
        view.update_in(cx, |view, window, cx| {
            let editor = view.active_editor().unwrap();
            let provider = editor.read(cx).lsp().code_action_providers[1].clone();
            provider
                .perform_code_action(editor, actions[0].clone(), true, window, cx)
                .detach();
        });
        wait_until(cx, "the fixed text", |cx| {
            view.read_with(cx, |view, cx| {
                view.active_editor().unwrap().read(cx).value() == "# Notes\nfine\n"
            })
        });
        // Linting again after the change clears the problem.
        wait_until(cx, "no problems", |cx| {
            view.read_with(cx, |view, _| view.problems.warnings == 0)
        });

        // A stopped host takes its problems away.
        view.update_in(cx, |view, window, cx| {
            let uri = view.panes[0].uri.clone();
            view.set_extension_diagnostics(
                &json!({ "owner": "lint", "uri": uri, "diagnostics": [{
                    "range": { "start": { "line": 0, "character": 0 }, "end": { "line": 0, "character": 1 } },
                    "message": "again", "severity": 2 }] }),
                cx,
            );
            assert_eq!(view.problems.warnings, 1);
            view.clear_extension_diagnostics(None, cx);
            assert_eq!(view.problems.warnings, 0);
            let _ = window;
        });
    }
}
