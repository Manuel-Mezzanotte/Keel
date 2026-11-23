//! Keel's menu bar on macOS, in the interface language.

use gpui::{App, Global, KeyBinding, Menu, MenuItem, OsAction, SystemMenuType, actions};
use gpui_component::input::{Copy, Cut, Paste, Redo, Replace, Search, SelectAll, Undo};

use crate::i18n::UiLanguage;
use crate::workbench_window::{
    CheckForUpdates, CloseWindow, NewFile, NewWindow, Open, OpenKeybindings, OpenProject,
    OpenSettings, Quit, Save, SelectLanguage, SelectTheme, ShowCommands, ShowExplorer,
    ShowExtensions, ShowSearch, ShowSourceControl, ToggleAgent, ToggleOutput, TogglePanel,
    ToggleProblems, ToggleSidebar, ToggleTerminal,
};
use crate::workbench_window::{
    CloseEditor, GoToLine, QuickOpen, ReopenClosedEditor, SplitEditor, SplitEditorDown,
};
use crate::workbench_window::{CloseFolder, RevertFile, SaveAll, SaveAs, ToggleAutoSave};
use crate::workbench_window::{
    ConfigureTasks, RerunLastTask, RunBuildTask, RunTask, TerminateTask,
};
use crate::workbench_window::{
    CopyLinesDown, CopyLinesUp, MoveLinesDown, MoveLinesUp, ToggleBlockComment, ToggleLineComment,
    ToggleWordWrap, ZoomIn, ZoomOut, ZoomReset,
};
use crate::workbench_window::{
    DebugConfigure, DebugPause, DebugRestart, DebugRun, DebugSelectAndStart, DebugStart,
    DebugStepInto, DebugStepOut, DebugStepOver, DebugStop, EditBreakpointCondition, EditLogpoint,
    RemoveAllBreakpoints, ToggleBreakpoint,
};
use crate::workbench_window::{
    GoBack, GoForward, GoToImplementation, GoToLastEdit, GoToReferences, GoToSymbolInFile,
    GoToSymbolInWorkspace, GoToTypeDefinition, NextChange, NextProblem, PreviousChange,
    PreviousProblem, RevealDefinition,
};
use crate::workbench_window::{
    KillTerminal, NewTerminal, OpenClaudeCode, OpenClaudeCodeInPanel, RenameTerminal,
    SplitTerminal, ToggleMaximizedPanel,
};

actions!(
    keel,
    [
        HideKeel,
        HideOtherApps,
        ShowAllApps,
        MinimizeWindow,
        ZoomWindow,
        BringAllToFront
    ]
);

/// The actions every Mac app has in its menu, with macOS's keys: hide
/// Keel, hide the other apps, show them all. They work with no window.
pub fn init(cx: &mut App) {
    cx.on_action(|_: &HideKeel, cx| cx.hide());
    cx.on_action(|_: &HideOtherApps, cx| cx.hide_other_apps());
    cx.on_action(|_: &ShowAllApps, cx| cx.unhide_other_apps());
    cx.on_action(|_: &MinimizeWindow, cx| {
        if let Some(window) = cx.active_window() {
            let _ = window.update(cx, |_, window, _| window.minimize_window());
        }
    });
    cx.on_action(|_: &ZoomWindow, cx| {
        if let Some(window) = cx.active_window() {
            let _ = window.update(cx, |_, window, _| window.zoom_window());
        }
    });
    cx.on_action(|_: &BringAllToFront, _| crate::mac::bring_all_to_front());
    cx.bind_keys([
        KeyBinding::new("cmd-h", HideKeel, None),
        KeyBinding::new("cmd-alt-h", HideOtherApps, None),
        KeyBinding::new("cmd-m", MinimizeWindow, None),
    ]);
}

/// Whether File → Auto Save has its check mark.
struct MenuAutoSave(bool);

impl Global for MenuAutoSave {}

/// Puts the check mark of File → Auto Save on or off.
pub fn set_auto_save(on: bool, cx: &mut App) {
    if cx.try_global::<MenuAutoSave>().map(|auto_save| auto_save.0) == Some(on) {
        return;
    }
    cx.set_global(MenuAutoSave(on));
    refresh(cx);
}

/// The language the menu bar was last built in.
struct MenuLanguage(UiLanguage);

impl Global for MenuLanguage {}

/// The language of the interface, as the menu bar was last built in;
/// English before that.
pub fn language(cx: &App) -> UiLanguage {
    cx.try_global::<MenuLanguage>()
        .map_or(UiLanguage::English, |language| language.0)
}

/// Builds the menu bar again, for example after the recent projects changed.
pub fn refresh(cx: &mut App) {
    if let Some(language) = cx.try_global::<MenuLanguage>().map(|language| language.0) {
        set(language, cx);
    }
}

/// Sets the menu bar. Called again when the interface language changes.
pub fn set(language: UiLanguage, cx: &mut App) {
    cx.set_global(MenuLanguage(language));
    let strings = language.strings();
    let auto_save = cx
        .try_global::<MenuAutoSave>()
        .is_none_or(|auto_save| auto_save.0);
    let recent: Vec<MenuItem> = crate::recent::projects(cx)
        .into_iter()
        .take(10)
        .map(|project| {
            MenuItem::action(
                crate::recent::short_path(&project),
                OpenProject { path: project },
            )
        })
        .collect();
    let menu = |name: &str, items: Vec<MenuItem>| Menu {
        name: name.to_owned().into(),
        items,
        disabled: false,
    };
    cx.set_menus(vec![
        // As every Mac app's: Settings first, then the system's items.
        menu(
            "Keel",
            vec![
                MenuItem::action(strings.menu_settings, OpenSettings),
                MenuItem::action(strings.menu_check_updates, CheckForUpdates),
                MenuItem::action(strings.menu_keybindings, OpenKeybindings),
                MenuItem::action(strings.menu_theme, SelectTheme),
                MenuItem::action(strings.menu_language, SelectLanguage),
                MenuItem::separator(),
                MenuItem::os_submenu(strings.menu_services, SystemMenuType::Services),
                MenuItem::separator(),
                MenuItem::action(strings.menu_hide, HideKeel),
                MenuItem::action(strings.menu_hide_others, HideOtherApps),
                MenuItem::action(strings.menu_show_all, ShowAllApps),
                MenuItem::separator(),
                MenuItem::action(strings.command_quit, Quit),
            ],
        ),
        menu(
            strings.menu_file,
            vec![
                MenuItem::action(strings.welcome_new_file, NewFile),
                MenuItem::action(strings.menu_new_window, NewWindow),
                MenuItem::separator(),
                MenuItem::action(strings.welcome_open, Open),
                MenuItem::submenu(Menu {
                    name: strings.menu_open_recent.into(),
                    disabled: recent.is_empty(),
                    items: recent,
                }),
                MenuItem::separator(),
                MenuItem::action(strings.save, Save),
                MenuItem::action(strings.menu_save_as, SaveAs),
                MenuItem::action(strings.menu_save_all, SaveAll),
                MenuItem::separator(),
                MenuItem::action(strings.menu_auto_save, ToggleAutoSave).checked(auto_save),
                MenuItem::separator(),
                MenuItem::action(strings.menu_revert, RevertFile),
                MenuItem::separator(),
                MenuItem::action(strings.menu_reopen_editor, ReopenClosedEditor),
                MenuItem::action(strings.menu_close_editor, CloseEditor),
                MenuItem::action(strings.menu_close_folder, CloseFolder),
                MenuItem::action(strings.menu_close_window, CloseWindow),
            ],
        ),
        menu(
            strings.menu_edit,
            vec![
                MenuItem::os_action(strings.menu_undo, Undo, OsAction::Undo),
                MenuItem::os_action(strings.menu_redo, Redo, OsAction::Redo),
                MenuItem::separator(),
                MenuItem::os_action(strings.menu_cut, Cut, OsAction::Cut),
                MenuItem::os_action(strings.menu_copy, Copy, OsAction::Copy),
                MenuItem::os_action(strings.menu_paste, Paste, OsAction::Paste),
                MenuItem::separator(),
                MenuItem::os_action(strings.menu_select_all, SelectAll, OsAction::SelectAll),
                MenuItem::separator(),
                MenuItem::action(strings.menu_find, Search),
                MenuItem::action(strings.menu_replace, Replace),
                MenuItem::separator(),
                MenuItem::action(strings.command_comment_line, ToggleLineComment),
                MenuItem::action(strings.command_block_comment, ToggleBlockComment),
            ],
        ),
        menu(
            strings.menu_selection,
            vec![
                MenuItem::os_action(strings.menu_select_all, SelectAll, OsAction::SelectAll),
                MenuItem::separator(),
                MenuItem::action(strings.command_copy_up, CopyLinesUp),
                MenuItem::action(strings.command_copy_down, CopyLinesDown),
                MenuItem::action(strings.command_move_up, MoveLinesUp),
                MenuItem::action(strings.command_move_down, MoveLinesDown),
            ],
        ),
        menu(
            strings.menu_view,
            vec![
                MenuItem::action(strings.menu_command_palette, ShowCommands),
                MenuItem::separator(),
                MenuItem::action(strings.menu_split_editor, SplitEditor),
                MenuItem::action(strings.menu_split_editor_down, SplitEditorDown),
                MenuItem::action(strings.menu_word_wrap, ToggleWordWrap),
                MenuItem::separator(),
                MenuItem::action(strings.menu_zoom_in, ZoomIn),
                MenuItem::action(strings.menu_zoom_out, ZoomOut),
                MenuItem::action(strings.menu_zoom_reset, ZoomReset),
                MenuItem::separator(),
                MenuItem::action(strings.view_explorer, ShowExplorer),
                MenuItem::action(strings.view_search, ShowSearch),
                MenuItem::action(strings.view_source_control, ShowSourceControl),
                MenuItem::action(strings.view_extensions, ShowExtensions),
                MenuItem::separator(),
                MenuItem::action(strings.panel_problems, ToggleProblems),
                MenuItem::action(strings.panel_output, ToggleOutput),
                MenuItem::action(strings.panel_terminal, ToggleTerminal),
                MenuItem::separator(),
                MenuItem::action(strings.toggle_sidebar, ToggleSidebar),
                MenuItem::action(strings.toggle_panel, TogglePanel),
                MenuItem::action(strings.toggle_agent, ToggleAgent),
            ],
        ),
        menu(
            strings.menu_go,
            vec![
                MenuItem::action(strings.menu_back, GoBack),
                MenuItem::action(strings.menu_forward, GoForward),
                MenuItem::action(strings.menu_last_edit, GoToLastEdit),
                MenuItem::separator(),
                MenuItem::action(strings.menu_go_to_file, QuickOpen),
                MenuItem::action(strings.menu_go_to_symbol, GoToSymbolInFile),
                MenuItem::action(strings.menu_go_to_workspace_symbol, GoToSymbolInWorkspace),
                MenuItem::action(strings.menu_go_to_line, GoToLine),
                MenuItem::separator(),
                MenuItem::action(strings.menu_go_to_definition, RevealDefinition),
                MenuItem::action(strings.menu_go_to_type_definition, GoToTypeDefinition),
                MenuItem::action(strings.menu_go_to_implementation, GoToImplementation),
                MenuItem::action(strings.menu_go_to_references, GoToReferences),
                MenuItem::separator(),
                MenuItem::action(strings.menu_next_problem, NextProblem),
                MenuItem::action(strings.menu_previous_problem, PreviousProblem),
                MenuItem::action(strings.menu_next_change, NextChange),
                MenuItem::action(strings.menu_previous_change, PreviousChange),
            ],
        ),
        menu(
            strings.menu_run,
            vec![
                MenuItem::action(strings.command_debug_start, DebugStart),
                MenuItem::action(strings.command_debug_run, DebugRun),
                MenuItem::action(strings.command_debug_stop, DebugStop),
                MenuItem::action(strings.command_debug_restart, DebugRestart),
                MenuItem::separator(),
                MenuItem::action(strings.command_debug_select, DebugSelectAndStart),
                MenuItem::action(strings.command_debug_configure, DebugConfigure),
                MenuItem::separator(),
                MenuItem::action(strings.command_debug_step_over, DebugStepOver),
                MenuItem::action(strings.command_debug_step_into, DebugStepInto),
                MenuItem::action(strings.command_debug_step_out, DebugStepOut),
                MenuItem::action(strings.command_debug_pause, DebugPause),
                MenuItem::separator(),
                MenuItem::action(strings.command_toggle_breakpoint, ToggleBreakpoint),
                MenuItem::action(
                    strings.command_conditional_breakpoint,
                    EditBreakpointCondition,
                ),
                MenuItem::action(strings.command_logpoint, EditLogpoint),
                MenuItem::action(strings.command_remove_breakpoints, RemoveAllBreakpoints),
            ],
        ),
        menu(
            strings.panel_terminal,
            vec![
                MenuItem::action(strings.terminal_new, NewTerminal),
                MenuItem::action(strings.terminal_split, SplitTerminal),
                MenuItem::action(strings.menu_rename_terminal, RenameTerminal),
                MenuItem::action(strings.terminal_kill, KillTerminal),
                MenuItem::separator(),
                MenuItem::action(strings.menu_run_task, RunTask),
                MenuItem::action(strings.menu_run_build_task, RunBuildTask),
                MenuItem::action(strings.menu_rerun_task, RerunLastTask),
                MenuItem::action(strings.menu_terminate_task, TerminateTask),
                MenuItem::action(strings.menu_configure_tasks, ConfigureTasks),
                MenuItem::separator(),
                MenuItem::action(strings.maximize_panel, ToggleMaximizedPanel),
                MenuItem::separator(),
                MenuItem::action(strings.command_claude_code, OpenClaudeCode),
                MenuItem::action(strings.command_claude_code_panel, OpenClaudeCodeInPanel),
            ],
        ),
        // macOS lists the open windows here.
        menu(
            strings.menu_window,
            vec![
                MenuItem::action(strings.menu_minimize, MinimizeWindow),
                MenuItem::action(strings.menu_zoom, ZoomWindow),
                MenuItem::separator(),
                MenuItem::action(strings.menu_bring_all_to_front, BringAllToFront),
            ],
        ),
        // macOS puts its field that searches every menu at the top.
        menu(
            strings.menu_help,
            vec![
                MenuItem::action(strings.menu_show_all_commands, ShowCommands),
                MenuItem::action(strings.menu_keybindings, OpenKeybindings),
                MenuItem::separator(),
                MenuItem::action(strings.menu_show_log, ToggleOutput),
            ],
        ),
    ]);
    crate::mac::mark_menus(strings.menu_window, strings.menu_help);
    // A right click on the Dock's icon: a new window and the recent
    // projects, as Xcode and VS Code offer.
    let mut dock = vec![MenuItem::action(strings.menu_new_window, NewWindow)];
    let recent: Vec<MenuItem> = crate::recent::projects(cx)
        .into_iter()
        .take(10)
        .map(|project| {
            MenuItem::action(
                crate::recent::short_path(&project),
                OpenProject { path: project },
            )
        })
        .collect();
    if !recent.is_empty() {
        dock.push(MenuItem::separator());
        dock.extend(recent);
    }
    cx.set_dock_menu(dock);
}

#[cfg(test)]
mod tests {
    use gpui::TestAppContext;

    use super::*;

    fn menu_names(cx: &mut TestAppContext) -> Vec<String> {
        cx.update(|cx| {
            cx.get_menus()
                .unwrap_or_default()
                .iter()
                .map(|menu| menu.name.to_string())
                .collect()
        })
    }

    /// Keel's menu is laid out as every Mac app's: Settings on top, the
    /// system's items below, Quit last; ⌘H and ⌥⌘H hide, as everywhere.
    #[gpui::test]
    fn keels_menu_starts_with_settings_as_mac_apps_do(cx: &mut TestAppContext) {
        cx.update(|cx| {
            init(cx);
            set(UiLanguage::Italian, cx);
        });
        let items: Vec<String> = cx.update(|cx| {
            cx.get_menus().unwrap()[0]
                .items
                .iter()
                .map(|item| match item {
                    gpui::OwnedMenuItem::Action { name, .. } => name.clone(),
                    gpui::OwnedMenuItem::Separator => "—".to_owned(),
                    gpui::OwnedMenuItem::Submenu(menu) => menu.name.to_string(),
                    gpui::OwnedMenuItem::SystemMenu(menu) => menu.name.to_string(),
                })
                .collect()
        });
        assert_eq!(
            items,
            [
                "Impostazioni…",
                "Cerca aggiornamenti…",
                "Scorciatoie da tastiera",
                "Tema colori…",
                "Lingua…",
                "—",
                "Servizi",
                "—",
                "Nascondi Keel",
                "Nascondi altre",
                "Mostra tutte",
                "—",
                "Esci da Keel"
            ]
        );
        let keys = |action: &dyn gpui::Action, cx: &mut TestAppContext| -> Vec<String> {
            cx.update(|cx| {
                cx.key_bindings()
                    .borrow()
                    .bindings_for_action(action)
                    .map(|binding| {
                        binding
                            .keystrokes()
                            .iter()
                            .map(|keystroke| keystroke.inner().unparse(gpui::OperatingSystem::Mac))
                            .collect::<Vec<_>>()
                            .join(" ")
                    })
                    .collect()
            })
        };
        assert_eq!(keys(&HideKeel, cx), ["cmd-h"]);
        assert_eq!(keys(&HideOtherApps, cx), ["alt-cmd-h"]);
    }

    #[gpui::test]
    fn the_menus_follow_the_interface_language(cx: &mut TestAppContext) {
        cx.update(|cx| set(UiLanguage::Italian, cx));
        assert_eq!(
            menu_names(cx),
            [
                "Keel",
                "File",
                "Modifica",
                "Selezione",
                "Vista",
                "Vai",
                "Esegui",
                "Terminale",
                "Finestra",
                "Aiuto"
            ]
        );

        cx.update(|cx| set(UiLanguage::English, cx));
        assert_eq!(
            menu_names(cx),
            [
                "Keel",
                "File",
                "Edit",
                "Selection",
                "View",
                "Go",
                "Run",
                "Terminal",
                "Window",
                "Help"
            ]
        );
    }
}
