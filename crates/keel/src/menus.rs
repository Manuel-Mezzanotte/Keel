//! Keel's menu bar on macOS, in the interface language.

use gpui::{App, Global, Menu, MenuItem, OsAction, SystemMenuType};
use gpui_component::input::{Copy, Cut, Paste, Redo, Replace, Search, SelectAll, Undo};

use crate::i18n::UiLanguage;
use crate::workbench_window::{CloseEditor, ReopenClosedEditor, SplitEditor};
use crate::workbench_window::{
    CloseWindow, NewFile, NewWindow, Open, OpenProject, Quit, Save, SelectLanguage, SelectTheme,
    ShowCommands, ShowExplorer, ShowExtensions, ShowSearch, ShowSourceControl, ToggleAgent,
    ToggleOutput, TogglePanel, ToggleProblems, ToggleSidebar, ToggleTerminal,
};

/// The language the menu bar was last built in.
struct MenuLanguage(UiLanguage);

impl Global for MenuLanguage {}

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
        menu(
            "Keel",
            vec![
                MenuItem::action(strings.menu_theme, SelectTheme),
                MenuItem::action(strings.menu_language, SelectLanguage),
                MenuItem::separator(),
                MenuItem::os_submenu(strings.menu_services, SystemMenuType::Services),
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
                MenuItem::separator(),
                MenuItem::action(strings.menu_reopen_editor, ReopenClosedEditor),
                MenuItem::action(strings.menu_close_editor, CloseEditor),
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
            ],
        ),
        menu(
            strings.menu_view,
            vec![
                MenuItem::action(strings.menu_command_palette, ShowCommands),
                MenuItem::separator(),
                MenuItem::action(strings.menu_split_editor, SplitEditor),
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
    ]);
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

    #[gpui::test]
    fn the_menus_follow_the_interface_language(cx: &mut TestAppContext) {
        cx.update(|cx| set(UiLanguage::Italian, cx));
        assert_eq!(menu_names(cx), ["Keel", "File", "Modifica", "Vista"]);

        cx.update(|cx| set(UiLanguage::English, cx));
        assert_eq!(menu_names(cx), ["Keel", "File", "Edit", "View"]);
    }
}
