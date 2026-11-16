//! Keel's commands and key bindings, with VS Code's command names and a
//! `keybindings.json` in VS Code's format to change them.

use std::path::Path;

use gpui::{Action, App, KeyBinding};
use serde_json::Value;

use crate::workbench_window::explorer;
use crate::workbench_window::{
    CheckoutBranch, Commit, CommitAmend, CreateBranch, DiscardAll, GitFetch, GitPull, GitPush,
    GitRefresh, GitStash, GitStashPop, GitSync, StageAll, UnstageAll,
};
use crate::workbench_window::{
    CloseAllEditors, CloseEditor, CloseEditorsInGroup, CloseOtherEditors, FocusGroup1, FocusGroup2,
    FocusGroup3, KeepEditor, NextEditor, PreviousEditor, ReopenClosedEditor, SplitEditor,
    TogglePinEditor,
};
use crate::workbench_window::{
    CloseWindow, NewFile, NewWindow, Open, OpenRecent, Quit, Save, SelectLanguage, SelectTheme,
    ShowCommands, ShowExplorer, ShowExtensions, ShowSearch, ShowSourceControl, ToggleAgent,
    ToggleOutput, TogglePanel, ToggleProblems, ToggleSidebar, ToggleTerminal,
};
use crate::workbench_window::{
    CopyLinesDown, CopyLinesUp, DeleteLines, EDITOR_CONTEXT, InsertLineAfter, InsertLineBefore,
    MoveLinesDown, MoveLinesUp, ToggleBlockComment, ToggleLineComment, ToggleWordWrap, ZoomIn,
    ZoomOut, ZoomReset,
};
use crate::workbench_window::{
    FocusNextTerminal, FocusPreviousTerminal, KillTerminal, NewTerminal, OpenClaudeCode,
    OpenClaudeCodeInPanel, RenameTerminal, RunNpmScript, SplitTerminal, TERMINAL_CONTEXT,
    ToggleMaximizedPanel,
};
use crate::workbench_window::{FormatDocument, OrganizeImports, ShowGitGraph};
use crate::workbench_window::{
    GoToImplementation, GoToReferences, GoToSymbolInFile, GoToSymbolInWorkspace,
    GoToTypeDefinition, RenameSymbol, RevealDefinition,
};
use crate::workbench_window::{GoToLine, QuickOpen};
use crate::workbench_window::{NextChange, PreviousChange, RevertChange, StageChange};
use crate::workbench_window::{
    NextProblem, PreviousProblem, RestartLanguageServers, TriggerParameterHints,
};
use crate::workbench_window::{ShowMarkdownPreview, ShowMarkdownPreviewToSide};

/// A Keel command: its VS Code name, the action it runs and its default key.
pub struct KeelCommand {
    pub id: &'static str,
    pub action: fn() -> Box<dyn Action>,
    pub default_key: Option<&'static str>,
    /// Where the key works, such as only in an editor.
    pub context: Option<&'static str>,
}

/// Keel's own commands. Their names follow VS Code, so a `keybindings.json`
/// copied from VS Code or Cursor works for them.
pub const COMMANDS: &[KeelCommand] = &[
    KeelCommand {
        id: "workbench.action.files.save",
        action: || Box::new(Save),
        default_key: Some("cmd+s"),
        context: None,
    },
    KeelCommand {
        id: "workbench.action.closeWindow",
        action: || Box::new(CloseWindow),
        default_key: Some("cmd+shift+w"),
        context: None,
    },
    KeelCommand {
        id: "workbench.action.quit",
        action: || Box::new(Quit),
        default_key: Some("cmd+q"),
        context: None,
    },
    KeelCommand {
        id: "workbench.action.showCommands",
        action: || Box::new(ShowCommands),
        default_key: Some("cmd+shift+p"),
        context: None,
    },
    KeelCommand {
        id: "workbench.action.selectTheme",
        action: || Box::new(SelectTheme),
        default_key: Some("cmd+k cmd+t"),
        context: None,
    },
    KeelCommand {
        id: "keel.selectLanguage",
        action: || Box::new(SelectLanguage),
        default_key: None,
        context: None,
    },
    KeelCommand {
        id: "workbench.action.toggleSidebarVisibility",
        action: || Box::new(ToggleSidebar),
        default_key: Some("cmd+b"),
        context: None,
    },
    KeelCommand {
        id: "workbench.action.togglePanel",
        action: || Box::new(TogglePanel),
        default_key: Some("cmd+j"),
        context: None,
    },
    KeelCommand {
        id: "workbench.action.toggleAuxiliaryBar",
        action: || Box::new(ToggleAgent),
        default_key: Some("cmd+alt+b"),
        context: None,
    },
    KeelCommand {
        id: "workbench.view.explorer",
        action: || Box::new(ShowExplorer),
        default_key: Some("cmd+shift+e"),
        context: None,
    },
    KeelCommand {
        id: "workbench.view.search",
        action: || Box::new(ShowSearch),
        default_key: Some("cmd+shift+f"),
        context: None,
    },
    KeelCommand {
        id: "workbench.view.scm",
        action: || Box::new(ShowSourceControl),
        default_key: Some("ctrl+shift+g"),
        context: None,
    },
    KeelCommand {
        id: "workbench.view.extensions",
        action: || Box::new(ShowExtensions),
        default_key: Some("cmd+shift+x"),
        context: None,
    },
    KeelCommand {
        id: "workbench.actions.view.problems",
        action: || Box::new(ToggleProblems),
        default_key: Some("cmd+shift+m"),
        context: None,
    },
    KeelCommand {
        id: "workbench.action.output.toggleOutput",
        action: || Box::new(ToggleOutput),
        default_key: Some("cmd+shift+u"),
        context: None,
    },
    KeelCommand {
        id: "workbench.action.terminal.toggleTerminal",
        action: || Box::new(ToggleTerminal),
        default_key: Some("ctrl+`"),
        context: None,
    },
    KeelCommand {
        id: "workbench.action.terminal.new",
        action: || Box::new(NewTerminal),
        default_key: Some("ctrl+shift+`"),
        context: None,
    },
    KeelCommand {
        id: "workbench.action.terminal.split",
        action: || Box::new(SplitTerminal),
        default_key: Some("cmd+\\"),
        context: Some(TERMINAL_CONTEXT),
    },
    KeelCommand {
        id: "workbench.action.terminal.kill",
        action: || Box::new(KillTerminal),
        default_key: None,
        context: None,
    },
    KeelCommand {
        id: "workbench.action.terminal.rename",
        action: || Box::new(RenameTerminal),
        default_key: None,
        context: None,
    },
    KeelCommand {
        id: "workbench.action.terminal.focusNext",
        action: || Box::new(FocusNextTerminal),
        default_key: Some("cmd+shift+]"),
        context: Some(TERMINAL_CONTEXT),
    },
    KeelCommand {
        id: "workbench.action.terminal.focusPrevious",
        action: || Box::new(FocusPreviousTerminal),
        default_key: Some("cmd+shift+["),
        context: Some(TERMINAL_CONTEXT),
    },
    KeelCommand {
        id: "workbench.action.toggleMaximizedPanel",
        action: || Box::new(ToggleMaximizedPanel),
        default_key: None,
        context: None,
    },
    KeelCommand {
        id: "keel.claudeCode.open",
        action: || Box::new(OpenClaudeCode),
        default_key: Some("cmd+escape"),
        context: None,
    },
    KeelCommand {
        id: "keel.claudeCode.openInPanel",
        action: || Box::new(OpenClaudeCodeInPanel),
        default_key: None,
        context: None,
    },
    KeelCommand {
        id: "git.commit",
        action: || Box::new(Commit),
        default_key: None,
        context: None,
    },
    KeelCommand {
        id: "git.commitStagedAmend",
        action: || Box::new(CommitAmend),
        default_key: None,
        context: None,
    },
    KeelCommand {
        id: "git.stageAll",
        action: || Box::new(StageAll),
        default_key: None,
        context: None,
    },
    KeelCommand {
        id: "git.unstageAll",
        action: || Box::new(UnstageAll),
        default_key: None,
        context: None,
    },
    KeelCommand {
        id: "git.cleanAll",
        action: || Box::new(DiscardAll),
        default_key: None,
        context: None,
    },
    KeelCommand {
        id: "editor.action.triggerSuggest",
        action: || Box::new(gpui_component::input::ShowCompletions),
        default_key: Some("ctrl+space"),
        context: Some(EDITOR_CONTEXT),
    },
    KeelCommand {
        id: "editor.action.triggerParameterHints",
        action: || Box::new(TriggerParameterHints),
        default_key: Some("cmd+shift+space"),
        context: Some(EDITOR_CONTEXT),
    },
    KeelCommand {
        id: "git-graph.view",
        action: || Box::new(ShowGitGraph),
        default_key: None,
        context: None,
    },
    KeelCommand {
        id: "markdown.showPreview",
        action: || Box::new(ShowMarkdownPreview),
        default_key: Some("cmd+shift+v"),
        context: Some(EDITOR_CONTEXT),
    },
    KeelCommand {
        id: "markdown.showPreviewToSide",
        action: || Box::new(ShowMarkdownPreviewToSide),
        default_key: Some("cmd+k v"),
        context: Some(EDITOR_CONTEXT),
    },
    KeelCommand {
        id: "editor.action.formatDocument",
        action: || Box::new(FormatDocument),
        default_key: Some("shift+alt+f"),
        context: Some(EDITOR_CONTEXT),
    },
    KeelCommand {
        id: "editor.action.organizeImports",
        action: || Box::new(OrganizeImports),
        default_key: Some("shift+alt+o"),
        context: Some(EDITOR_CONTEXT),
    },
    KeelCommand {
        id: "editor.action.revealDefinition",
        action: || Box::new(RevealDefinition),
        default_key: Some("f12"),
        context: Some(EDITOR_CONTEXT),
    },
    KeelCommand {
        id: "editor.action.goToImplementation",
        action: || Box::new(GoToImplementation),
        default_key: Some("cmd+f12"),
        context: Some(EDITOR_CONTEXT),
    },
    KeelCommand {
        id: "editor.action.goToTypeDefinition",
        action: || Box::new(GoToTypeDefinition),
        default_key: None,
        context: Some(EDITOR_CONTEXT),
    },
    KeelCommand {
        id: "editor.action.goToReferences",
        action: || Box::new(GoToReferences),
        default_key: Some("shift+f12"),
        context: Some(EDITOR_CONTEXT),
    },
    KeelCommand {
        id: "editor.action.rename",
        action: || Box::new(RenameSymbol),
        default_key: Some("f2"),
        context: Some(EDITOR_CONTEXT),
    },
    KeelCommand {
        id: "workbench.action.gotoSymbol",
        action: || Box::new(GoToSymbolInFile),
        default_key: Some("cmd+shift+o"),
        context: None,
    },
    KeelCommand {
        id: "workbench.action.showAllSymbols",
        action: || Box::new(GoToSymbolInWorkspace),
        default_key: Some("cmd+t"),
        context: None,
    },
    KeelCommand {
        id: "editor.action.marker.nextInFiles",
        action: || Box::new(NextProblem),
        default_key: Some("f8"),
        context: None,
    },
    KeelCommand {
        id: "editor.action.marker.prevInFiles",
        action: || Box::new(PreviousProblem),
        default_key: Some("shift+f8"),
        context: None,
    },
    KeelCommand {
        id: "keel.restartLanguageServers",
        action: || Box::new(RestartLanguageServers),
        default_key: None,
        context: None,
    },
    KeelCommand {
        id: "git.push",
        action: || Box::new(GitPush),
        default_key: None,
        context: None,
    },
    KeelCommand {
        id: "git.pull",
        action: || Box::new(GitPull),
        default_key: None,
        context: None,
    },
    KeelCommand {
        id: "git.fetch",
        action: || Box::new(GitFetch),
        default_key: None,
        context: None,
    },
    KeelCommand {
        id: "git.sync",
        action: || Box::new(GitSync),
        default_key: None,
        context: None,
    },
    KeelCommand {
        id: "git.stash",
        action: || Box::new(GitStash),
        default_key: None,
        context: None,
    },
    KeelCommand {
        id: "git.stashPop",
        action: || Box::new(GitStashPop),
        default_key: None,
        context: None,
    },
    KeelCommand {
        id: "git.refresh",
        action: || Box::new(GitRefresh),
        default_key: None,
        context: None,
    },
    KeelCommand {
        id: "git.checkout",
        action: || Box::new(CheckoutBranch),
        default_key: None,
        context: None,
    },
    KeelCommand {
        id: "git.branch",
        action: || Box::new(CreateBranch),
        default_key: None,
        context: None,
    },
    KeelCommand {
        id: "git.stageSelectedRanges",
        action: || Box::new(StageChange),
        default_key: None,
        context: None,
    },
    KeelCommand {
        id: "git.revertSelectedRanges",
        action: || Box::new(RevertChange),
        default_key: None,
        context: None,
    },
    KeelCommand {
        id: "workbench.action.editor.nextChange",
        action: || Box::new(NextChange),
        default_key: Some("alt+f5"),
        context: Some(EDITOR_CONTEXT),
    },
    KeelCommand {
        id: "workbench.action.editor.previousChange",
        action: || Box::new(PreviousChange),
        default_key: Some("shift+alt+f5"),
        context: Some(EDITOR_CONTEXT),
    },
    KeelCommand {
        id: "keel.npm.runScript",
        action: || Box::new(RunNpmScript),
        default_key: None,
        context: None,
    },
    KeelCommand {
        id: "workbench.action.files.openFileFolder",
        action: || Box::new(Open),
        default_key: Some("cmd+o"),
        context: None,
    },
    KeelCommand {
        id: "workbench.action.files.newUntitledFile",
        action: || Box::new(NewFile),
        default_key: Some("cmd+n"),
        context: None,
    },
    KeelCommand {
        id: "workbench.action.newWindow",
        action: || Box::new(NewWindow),
        default_key: Some("cmd+shift+n"),
        context: None,
    },
    KeelCommand {
        id: "workbench.action.openRecent",
        action: || Box::new(OpenRecent),
        default_key: Some("ctrl+r"),
        context: None,
    },
    KeelCommand {
        id: "workbench.action.closeActiveEditor",
        action: || Box::new(CloseEditor),
        default_key: Some("cmd+w"),
        context: None,
    },
    KeelCommand {
        id: "workbench.action.closeOtherEditors",
        action: || Box::new(CloseOtherEditors),
        default_key: Some("cmd+alt+t"),
        context: None,
    },
    KeelCommand {
        id: "workbench.action.closeEditorsInGroup",
        action: || Box::new(CloseEditorsInGroup),
        default_key: Some("cmd+k w"),
        context: None,
    },
    KeelCommand {
        id: "workbench.action.closeAllEditors",
        action: || Box::new(CloseAllEditors),
        default_key: Some("cmd+k cmd+w"),
        context: None,
    },
    KeelCommand {
        id: "workbench.action.reopenClosedEditor",
        action: || Box::new(ReopenClosedEditor),
        default_key: Some("cmd+shift+t"),
        context: None,
    },
    KeelCommand {
        id: "workbench.action.pinEditor",
        action: || Box::new(TogglePinEditor),
        default_key: Some("cmd+k shift+enter"),
        context: None,
    },
    KeelCommand {
        id: "workbench.action.keepEditor",
        action: || Box::new(KeepEditor),
        default_key: Some("cmd+k enter"),
        context: None,
    },
    KeelCommand {
        id: "workbench.action.nextEditor",
        action: || Box::new(NextEditor),
        default_key: Some("cmd+alt+right"),
        context: None,
    },
    KeelCommand {
        id: "workbench.action.previousEditor",
        action: || Box::new(PreviousEditor),
        default_key: Some("cmd+alt+left"),
        context: None,
    },
    KeelCommand {
        id: "workbench.action.splitEditor",
        action: || Box::new(SplitEditor),
        default_key: Some("cmd+\\"),
        context: None,
    },
    KeelCommand {
        id: "workbench.action.focusFirstEditorGroup",
        action: || Box::new(FocusGroup1),
        default_key: Some("cmd+1"),
        context: None,
    },
    KeelCommand {
        id: "workbench.action.focusSecondEditorGroup",
        action: || Box::new(FocusGroup2),
        default_key: Some("cmd+2"),
        context: None,
    },
    KeelCommand {
        id: "workbench.action.focusThirdEditorGroup",
        action: || Box::new(FocusGroup3),
        default_key: Some("cmd+3"),
        context: None,
    },
    KeelCommand {
        id: "workbench.action.quickOpen",
        action: || Box::new(QuickOpen),
        default_key: Some("cmd+p"),
        context: None,
    },
    KeelCommand {
        id: "workbench.action.gotoLine",
        action: || Box::new(GoToLine),
        default_key: Some("ctrl+g"),
        context: None,
    },
    KeelCommand {
        id: "editor.action.commentLine",
        action: || Box::new(ToggleLineComment),
        default_key: Some("cmd+/"),
        context: Some(EDITOR_CONTEXT),
    },
    KeelCommand {
        id: "editor.action.blockComment",
        action: || Box::new(ToggleBlockComment),
        default_key: Some("shift+alt+a"),
        context: Some(EDITOR_CONTEXT),
    },
    KeelCommand {
        id: "editor.action.moveLinesUpAction",
        action: || Box::new(MoveLinesUp),
        default_key: Some("alt+up"),
        context: Some(EDITOR_CONTEXT),
    },
    KeelCommand {
        id: "editor.action.moveLinesDownAction",
        action: || Box::new(MoveLinesDown),
        default_key: Some("alt+down"),
        context: Some(EDITOR_CONTEXT),
    },
    KeelCommand {
        id: "editor.action.copyLinesUpAction",
        action: || Box::new(CopyLinesUp),
        default_key: Some("shift+alt+up"),
        context: Some(EDITOR_CONTEXT),
    },
    KeelCommand {
        id: "editor.action.copyLinesDownAction",
        action: || Box::new(CopyLinesDown),
        default_key: Some("shift+alt+down"),
        context: Some(EDITOR_CONTEXT),
    },
    KeelCommand {
        id: "editor.action.deleteLines",
        action: || Box::new(DeleteLines),
        default_key: Some("cmd+shift+k"),
        context: Some(EDITOR_CONTEXT),
    },
    KeelCommand {
        id: "editor.action.insertLineAfter",
        action: || Box::new(InsertLineAfter),
        default_key: Some("cmd+enter"),
        context: Some(EDITOR_CONTEXT),
    },
    KeelCommand {
        id: "editor.action.insertLineBefore",
        action: || Box::new(InsertLineBefore),
        default_key: Some("cmd+shift+enter"),
        context: Some(EDITOR_CONTEXT),
    },
    KeelCommand {
        id: "editor.action.toggleWordWrap",
        action: || Box::new(ToggleWordWrap),
        default_key: Some("alt+z"),
        context: None,
    },
    KeelCommand {
        id: "workbench.action.zoomIn",
        action: || Box::new(ZoomIn),
        default_key: Some("cmd+="),
        context: None,
    },
    KeelCommand {
        id: "workbench.action.zoomOut",
        action: || Box::new(ZoomOut),
        default_key: Some("cmd+-"),
        context: None,
    },
    KeelCommand {
        id: "workbench.action.zoomReset",
        action: || Box::new(ZoomReset),
        default_key: None,
        context: None,
    },
    KeelCommand {
        id: "explorer.newFile",
        action: || Box::new(explorer::NewFileHere),
        default_key: None,
        context: None,
    },
    KeelCommand {
        id: "explorer.newFolder",
        action: || Box::new(explorer::NewFolderHere),
        default_key: None,
        context: None,
    },
    KeelCommand {
        id: "workbench.files.action.refreshFilesExplorer",
        action: || Box::new(explorer::Refresh),
        default_key: None,
        context: None,
    },
    KeelCommand {
        id: "workbench.files.action.collapseExplorerFolders",
        action: || Box::new(explorer::CollapseAll),
        default_key: None,
        context: None,
    },
];

/// The editor's own key that VS Code uses for something else: gpui-component
/// opens Replace with Cmd+Shift+F, which in VS Code searches all files.
/// Replace moves to Cmd+Alt+F, as in VS Code.
const EDITOR_CONTEXT_INPUT: &str = "Input";

/// An entry of the user's `keybindings.json`.
#[derive(Clone, Debug, PartialEq)]
pub struct UserBinding {
    /// The command, without the leading `-` of a removal.
    pub command: String,
    /// `true` for `"-command"` entries, which remove a binding.
    pub removes: bool,
    /// The keystrokes in GPUI's notation, such as `cmd-shift-p`.
    pub keystrokes: String,
    pub when: Option<String>,
    pub args: Option<Value>,
}

/// Reads `keybindings.json`. A missing file means no changes; entries Keel
/// cannot read are skipped and reported.
pub fn read_user_bindings(file: &Path) -> (Vec<UserBinding>, Vec<String>) {
    let mut warnings = Vec::new();
    let text = match std::fs::read_to_string(file) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return (Vec::new(), warnings);
        }
        Err(error) => {
            warnings.push(format!("{}: {error}", file.display()));
            return (Vec::new(), warnings);
        }
    };
    let entries = match settings::parse_jsonc(&text) {
        Ok(Value::Array(entries)) => entries,
        Ok(_) => {
            warnings.push(format!(
                "{}: key bindings must be a JSON list",
                file.display()
            ));
            return (Vec::new(), warnings);
        }
        Err(error) => {
            warnings.push(format!("{}: {error}", file.display()));
            return (Vec::new(), warnings);
        }
    };
    let bindings = entries
        .iter()
        .filter_map(|entry| {
            let command = entry["command"].as_str()?;
            let key = entry["key"].as_str().unwrap_or_default();
            let Some(keystrokes) = workbench::keybinding::to_gpui(key, entry["mac"].as_str())
            else {
                warnings.push(format!("{}: cannot read the key \"{key}\"", file.display()));
                return None;
            };
            let (command, removes) = match command.strip_prefix('-') {
                Some(command) => (command, true),
                None => (command, false),
            };
            Some(UserBinding {
                command: command.to_owned(),
                removes,
                keystrokes,
                when: entry["when"].as_str().map(str::to_owned),
                args: entry.get("args").cloned(),
            })
        })
        .collect();
    (bindings, warnings)
}

/// The keystrokes of every Keel command: the defaults, minus the ones the
/// user removed, plus the ones the user added.
pub fn keel_bindings(user: &[UserBinding]) -> Vec<(&'static KeelCommand, String)> {
    let mut bindings = Vec::new();
    for command in COMMANDS {
        if let Some(key) = command.default_key {
            let keystrokes = workbench::keybinding::to_gpui(key, None).unwrap_or_default();
            let removed = user.iter().any(|binding| {
                binding.removes && binding.command == command.id && binding.keystrokes == keystrokes
            });
            if !removed {
                bindings.push((command, keystrokes));
            }
        }
    }
    for binding in user.iter().filter(|binding| !binding.removes) {
        if let Some(command) = COMMANDS
            .iter()
            .find(|command| command.id == binding.command)
        {
            bindings.push((command, binding.keystrokes.clone()));
        }
    }
    bindings
}

/// Commands whose keys still work in a terminal, as in VS Code's
/// `terminal.integrated.commandsToSkipShell`.
const SKIP_SHELL: &[&str] = &[
    "workbench.action.terminal.toggleTerminal",
    "workbench.action.terminal.new",
    "workbench.view.scm",
];

/// Keys of Keel's commands that a terminal keeps for the shell: those
/// without Cmd, such as Ctrl+R for the history and Ctrl+G, and the chords
/// that start with Cmd+K, so Cmd+K clears the terminal at once.
pub fn window_keys_for_shell(user: &[UserBinding]) -> Vec<String> {
    let mut keys: Vec<String> = keel_bindings(user)
        .into_iter()
        .filter(|(command, keystrokes)| {
            command.context.is_none()
                && !SKIP_SHELL.contains(&command.id)
                && (!keystrokes.contains("cmd-") || keystrokes.starts_with("cmd-k "))
        })
        .map(|(_, keystrokes)| keystrokes)
        .collect();
    keys.sort();
    keys.dedup();
    keys
}

/// Binds Keel's commands. Keys follow the physical layout of non-US
/// keyboards, such as the Italian one, the way macOS apps do.
pub fn bind(user: &[UserBinding], cx: &mut App) -> Vec<String> {
    let mut warnings = Vec::new();
    let mut key_bindings = Vec::new();
    for (command, keystrokes) in keel_bindings(user) {
        let context = command
            .context
            .and_then(|context| gpui::KeyBindingContextPredicate::parse(context).ok())
            .map(std::rc::Rc::new);
        match KeyBinding::load(&keystrokes, (command.action)(), context, true, None, cx) {
            Ok(binding) => key_bindings.push(binding),
            Err(error) => warnings.push(format!("{}: {error}", command.id)),
        }
    }
    key_bindings.extend(explorer::tree_bindings());
    key_bindings.extend(crate::workbench_window::source_control_bindings());
    key_bindings.extend(crate::workbench_window::pdf_bindings());
    key_bindings.extend(crate::workbench_window::terminal_bindings(
        &window_keys_for_shell(user),
        cx,
    ));
    // Cmd+Enter inserts a line below, as in VS Code, rather than the
    // editor's own secondary Enter.
    let enter = gpui_component::input::Enter {
        secondary: true,
        shift: false,
    };
    key_bindings.push(KeyBinding::new(
        "cmd-enter",
        gpui::Unbind(enter.name().into()),
        Some(EDITOR_CONTEXT_INPUT),
    ));
    let replace = gpui_component::input::Replace;
    key_bindings.push(KeyBinding::new(
        "cmd-shift-f",
        gpui::Unbind(replace.name().into()),
        Some(EDITOR_CONTEXT_INPUT),
    ));
    key_bindings.push(KeyBinding::new(
        "cmd-alt-f",
        replace,
        Some(EDITOR_CONTEXT_INPUT),
    ));
    cx.bind_keys(key_bindings);
    warnings
}

#[cfg(test)]
mod tests {
    use super::*;

    fn user(text: &str) -> Vec<UserBinding> {
        let folder = tempfile::tempdir().unwrap();
        let file = folder.path().join("keybindings.json");
        std::fs::write(&file, text).unwrap();
        let (bindings, warnings) = read_user_bindings(&file);
        assert!(warnings.is_empty(), "{warnings:?}");
        bindings
    }

    #[test]
    fn terminals_keep_the_shells_keys() {
        let keys = window_keys_for_shell(&[]);
        for key in ["ctrl-r", "ctrl-g", "alt-z", "cmd-k cmd-t"] {
            assert!(keys.contains(&key.to_owned()), "{key} in {keys:?}");
        }
        for key in ["ctrl-`", "cmd-p", "cmd-s", "ctrl-shift-g"] {
            assert!(!keys.contains(&key.to_owned()), "{key} in {keys:?}");
        }
    }

    #[test]
    fn reads_the_vs_code_format() {
        let bindings = user(
            r#"[
                // Cursor-style palette
                { "key": "cmd+shift+a", "command": "workbench.action.showCommands" },
                { "key": "cmd+s", "command": "-workbench.action.files.save" },
                { "key": "ctrl+g", "mac": "cmd+g", "command": "vsauto.start", "when": "editorTextFocus", "args": { "x": 1 } },
            ]"#,
        );
        assert_eq!(bindings.len(), 3);
        assert_eq!(bindings[0].keystrokes, "shift-cmd-a");
        assert!(bindings[1].removes);
        assert_eq!(bindings[1].command, "workbench.action.files.save");
        assert_eq!(bindings[2].keystrokes, "cmd-g");
        assert_eq!(bindings[2].when.as_deref(), Some("editorTextFocus"));
        assert_eq!(bindings[2].args, Some(serde_json::json!({ "x": 1 })));
    }

    #[test]
    fn defaults_can_be_removed_and_added() {
        let bindings = user(
            r#"[
                { "key": "cmd+s", "command": "-workbench.action.files.save" },
                { "key": "cmd+alt+s", "command": "workbench.action.files.save" },
            ]"#,
        );
        let keel = keel_bindings(&bindings);
        let save: Vec<&str> = keel
            .iter()
            .filter(|(command, _)| command.id == "workbench.action.files.save")
            .map(|(_, keystrokes)| keystrokes.as_str())
            .collect();
        assert_eq!(save, ["alt-cmd-s"]);
        assert!(keel.iter().any(
            |(command, keys)| command.id == "workbench.action.showCommands"
                && keys == "shift-cmd-p"
        ));
        assert!(keel.iter().any(
            |(command, keys)| command.id == "workbench.action.selectTheme" && keys == "cmd-k cmd-t"
        ));
    }

    #[test]
    fn no_file_means_the_defaults() {
        let (bindings, warnings) =
            read_user_bindings(Path::new("/nonexistent/keel/keybindings.json"));
        assert!(bindings.is_empty() && warnings.is_empty());
        assert_eq!(keel_bindings(&[]).len(), 67);
    }

    #[test]
    fn unreadable_keys_are_reported() {
        let folder = tempfile::tempdir().unwrap();
        let file = folder.path().join("keybindings.json");
        std::fs::write(&file, r#"[{ "key": "hyper+x", "command": "a" }]"#).unwrap();
        let (bindings, warnings) = read_user_bindings(&file);
        assert!(bindings.is_empty());
        assert_eq!(warnings.len(), 1);
    }
}
