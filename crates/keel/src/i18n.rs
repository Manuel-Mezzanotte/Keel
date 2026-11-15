//! Keel's interface texts in Italian and English.
//!
//! Every text lives in one table per language, so the compiler checks that
//! none is missing in either.

/// The language of the interface.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UiLanguage {
    Italian,
    English,
}

impl UiLanguage {
    /// The language from the `keel.language` setting: `it`, `en`, or `auto`
    /// to follow the Mac. Languages other than Italian fall back to English.
    pub fn from_setting(setting: Option<&str>, system_locale: Option<&str>) -> Self {
        match setting {
            Some("it") => UiLanguage::Italian,
            Some("en") => UiLanguage::English,
            _ if system_locale
                .is_some_and(|locale| locale.to_ascii_lowercase().starts_with("it")) =>
            {
                UiLanguage::Italian
            }
            _ => UiLanguage::English,
        }
    }

    /// The Mac's preferred language, such as `it-IT`.
    pub fn system_locale() -> Option<String> {
        sys_locale::get_locale()
    }

    /// The code extensions see as `vscode.env.language`.
    pub fn code(self) -> &'static str {
        match self {
            UiLanguage::Italian => "it",
            UiLanguage::English => "en",
        }
    }

    pub fn strings(self) -> &'static Strings {
        match self {
            UiLanguage::Italian => &ITALIAN,
            UiLanguage::English => &ENGLISH,
        }
    }
}

/// The texts of the interface.
pub struct Strings {
    pub save: &'static str,
    pub dont_save: &'static str,
    pub cancel: &'static str,
    pub ok: &'static str,
    pub untitled: &'static str,
    pub untitled_file_name: &'static str,
    pub changes_lost: &'static str,
    pub type_a_command: &'static str,
    pub choose_language: &'static str,
    pub command_save: &'static str,
    pub command_close_window: &'static str,
    pub command_language: &'static str,
    pub command_theme: &'static str,
    pub command_quit: &'static str,
    pub menu_file: &'static str,
    pub menu_edit: &'static str,
    pub menu_view: &'static str,
    pub menu_services: &'static str,
    pub menu_theme: &'static str,
    pub menu_language: &'static str,
    pub menu_close_window: &'static str,
    pub menu_undo: &'static str,
    pub menu_redo: &'static str,
    pub menu_cut: &'static str,
    pub menu_copy: &'static str,
    pub menu_paste: &'static str,
    pub menu_select_all: &'static str,
    pub menu_find: &'static str,
    pub menu_replace: &'static str,
    pub menu_command_palette: &'static str,
    pub choose_theme: &'static str,
    pub settings_unavailable: &'static str,
    pub search_commands: &'static str,
    pub toggle_sidebar: &'static str,
    pub toggle_panel: &'static str,
    pub toggle_agent: &'static str,
    pub view_explorer: &'static str,
    pub view_search: &'static str,
    pub view_source_control: &'static str,
    pub view_extensions: &'static str,
    pub open_editors: &'static str,
    pub installed_extensions: &'static str,
    pub no_extensions: &'static str,
    pub panel_problems: &'static str,
    pub panel_output: &'static str,
    pub panel_terminal: &'static str,
    pub panel_ports: &'static str,
    pub no_output: &'static str,
    pub agent_not_connected: &'static str,
    pub ask_claude: &'static str,
    pub plain_text: &'static str,
    pub close_editor: &'static str,
    pub welcome_open: &'static str,
    pub welcome_new_file: &'static str,
    pub menu_new_window: &'static str,
    pub menu_open_recent: &'static str,
    pub recent_projects: &'static str,
    pub open_recent_title: &'static str,
    pub no_recent_projects: &'static str,
    pub command_open: &'static str,
    pub command_new_file: &'static str,
    pub command_new_window: &'static str,
    pub command_open_recent: &'static str,
    pub no_folder: &'static str,
    pub explorer_new_file: &'static str,
    pub explorer_new_folder: &'static str,
    pub explorer_refresh: &'static str,
    pub explorer_collapse: &'static str,
    pub explorer_rename: &'static str,
    pub explorer_trash: &'static str,
    pub explorer_reveal: &'static str,
    pub explorer_copy_path: &'static str,
    pub explorer_copy_relative: &'static str,
    pub explorer_move: &'static str,
    pub trash_detail: &'static str,
    pub command_explorer_new_file: &'static str,
    pub command_explorer_new_folder: &'static str,
    pub command_explorer_refresh: &'static str,
    pub command_explorer_collapse: &'static str,
    pub tab_pin: &'static str,
    pub tab_unpin: &'static str,
    pub tab_close_others: &'static str,
    pub tab_close_right: &'static str,
    pub tab_close_all: &'static str,
    pub tab_split: &'static str,
    pub menu_close_editor: &'static str,
    pub menu_reopen_editor: &'static str,
    pub menu_split_editor: &'static str,
    pub command_close_editor: &'static str,
    pub command_close_others: &'static str,
    pub command_close_group: &'static str,
    pub command_close_all: &'static str,
    pub command_reopen_editor: &'static str,
    pub command_pin_editor: &'static str,
    pub command_keep_editor: &'static str,
    pub command_next_editor: &'static str,
    pub command_previous_editor: &'static str,
    pub command_split_editor: &'static str,
    pub command_focus_group_1: &'static str,
    pub command_focus_group_2: &'static str,
    pub command_focus_group_3: &'static str,
    pub search_files: &'static str,
    pub quick_open_placeholder: &'static str,
    pub menu_go: &'static str,
    pub menu_go_to_file: &'static str,
    pub menu_go_to_line: &'static str,
    pub command_quick_open: &'static str,
    pub command_go_to_line: &'static str,
    pub command_toggle_sidebar: &'static str,
    pub command_toggle_panel: &'static str,
    pub command_toggle_agent: &'static str,
    pub command_explorer: &'static str,
    pub command_search: &'static str,
    pub command_source_control: &'static str,
    pub command_extensions: &'static str,
    pub command_problems: &'static str,
    pub command_output: &'static str,
    pub command_terminal: &'static str,
    save_changes: &'static str,
    could_not_save: &'static str,
    could_not_use: &'static str,
    could_not_start: &'static str,
    could_not_open: &'static str,
    could_not_change: &'static str,
    go_to_line: &'static str,
    move_question: &'static str,
    trash_question: &'static str,
    coming_in_phase: &'static str,
    line_column: &'static str,
    spaces: &'static str,
    tab_size: &'static str,
}

impl Strings {
    /// The palette title of a Keel command, or `None` for commands the
    /// palette does not list, such as the palette itself.
    pub fn command_title(&self, id: &str) -> Option<&'static str> {
        Some(match id {
            "workbench.action.files.save" => self.command_save,
            "workbench.action.closeWindow" => self.command_close_window,
            "workbench.action.quit" => self.command_quit,
            "workbench.action.selectTheme" => self.command_theme,
            "keel.selectLanguage" => self.command_language,
            "workbench.action.toggleSidebarVisibility" => self.command_toggle_sidebar,
            "workbench.action.togglePanel" => self.command_toggle_panel,
            "workbench.action.toggleAuxiliaryBar" => self.command_toggle_agent,
            "workbench.view.explorer" => self.command_explorer,
            "workbench.view.search" => self.command_search,
            "workbench.view.scm" => self.command_source_control,
            "workbench.view.extensions" => self.command_extensions,
            "workbench.actions.view.problems" => self.command_problems,
            "workbench.action.output.toggleOutput" => self.command_output,
            "workbench.action.terminal.toggleTerminal" => self.command_terminal,
            "workbench.action.files.openFileFolder" => self.command_open,
            "workbench.action.files.newUntitledFile" => self.command_new_file,
            "workbench.action.newWindow" => self.command_new_window,
            "workbench.action.openRecent" => self.command_open_recent,
            "explorer.newFile" => self.command_explorer_new_file,
            "explorer.newFolder" => self.command_explorer_new_folder,
            "workbench.files.action.refreshFilesExplorer" => self.command_explorer_refresh,
            "workbench.files.action.collapseExplorerFolders" => self.command_explorer_collapse,
            "workbench.action.closeActiveEditor" => self.command_close_editor,
            "workbench.action.closeOtherEditors" => self.command_close_others,
            "workbench.action.closeEditorsInGroup" => self.command_close_group,
            "workbench.action.closeAllEditors" => self.command_close_all,
            "workbench.action.reopenClosedEditor" => self.command_reopen_editor,
            "workbench.action.pinEditor" => self.command_pin_editor,
            "workbench.action.keepEditor" => self.command_keep_editor,
            "workbench.action.nextEditor" => self.command_next_editor,
            "workbench.action.previousEditor" => self.command_previous_editor,
            "workbench.action.splitEditor" => self.command_split_editor,
            "workbench.action.focusFirstEditorGroup" => self.command_focus_group_1,
            "workbench.action.focusSecondEditorGroup" => self.command_focus_group_2,
            "workbench.action.focusThirdEditorGroup" => self.command_focus_group_3,
            "workbench.action.quickOpen" => self.command_quick_open,
            "workbench.action.gotoLine" => self.command_go_to_line,
            _ => return None,
        })
    }

    /// "Do you want to save the changes you made to {name}?"
    pub fn save_changes(&self, name: &str) -> String {
        self.save_changes.replace("{name}", name)
    }

    pub fn could_not_save(&self, name: &str) -> String {
        self.could_not_save.replace("{name}", name)
    }

    pub fn could_not_use(&self, path: &str) -> String {
        self.could_not_use.replace("{path}", path)
    }

    pub fn could_not_start(&self, extension: &str, error: &str) -> String {
        self.could_not_start
            .replace("{extension}", extension)
            .replace("{error}", error)
    }

    pub fn could_not_open(&self, path: &str, error: &str) -> String {
        self.could_not_open
            .replace("{path}", path)
            .replace("{error}", error)
    }

    pub fn could_not_change(&self, name: &str, error: &str) -> String {
        self.could_not_change
            .replace("{name}", name)
            .replace("{error}", error)
    }

    pub fn go_to_line_placeholder(&self, current: usize, total: usize) -> String {
        self.go_to_line
            .replace("{current}", &current.to_string())
            .replace("{total}", &total.to_string())
    }

    pub fn move_question(&self, name: &str, folder: &str) -> String {
        self.move_question
            .replace("{name}", name)
            .replace("{folder}", folder)
    }

    pub fn trash_question(&self, name: &str) -> String {
        self.trash_question.replace("{name}", name)
    }

    /// "Coming in phase 3", for the parts of the window not built yet.
    pub fn coming_in_phase(&self, phase: u8) -> String {
        self.coming_in_phase.replace("{phase}", &phase.to_string())
    }

    /// The cursor position in the status bar, counted from 1.
    pub fn line_column(&self, line: usize, column: usize) -> String {
        self.line_column
            .replace("{line}", &line.to_string())
            .replace("{column}", &column.to_string())
    }

    /// The indentation in the status bar.
    pub fn indentation(&self, size: usize, hard_tabs: bool) -> String {
        let text = if hard_tabs {
            self.tab_size
        } else {
            self.spaces
        };
        text.replace("{size}", &size.to_string())
    }
}

pub const ITALIAN: Strings = Strings {
    save: "Salva",
    dont_save: "Non salvare",
    cancel: "Annulla",
    ok: "OK",
    untitled: "Senza titolo",
    untitled_file_name: "Senza titolo.txt",
    changes_lost: "Se non le salvi, le modifiche andranno perse.",
    type_a_command: "Scrivi un comando",
    choose_language: "Lingua dell'interfaccia",
    command_save: "File: Salva",
    command_close_window: "Vista: Chiudi finestra",
    command_language: "Preferenze: Lingua",
    command_theme: "Preferenze: Tema colori",
    command_quit: "Esci da Keel",
    menu_file: "File",
    menu_edit: "Modifica",
    menu_view: "Vista",
    menu_services: "Servizi",
    menu_theme: "Tema colori…",
    menu_language: "Lingua…",
    menu_close_window: "Chiudi finestra",
    menu_undo: "Annulla",
    menu_redo: "Ripeti",
    menu_cut: "Taglia",
    menu_copy: "Copia",
    menu_paste: "Incolla",
    menu_select_all: "Seleziona tutto",
    menu_find: "Trova…",
    menu_replace: "Sostituisci…",
    menu_command_palette: "Palette dei comandi…",
    choose_theme: "Tema colori",
    settings_unavailable: "Le impostazioni non sono ancora disponibili",
    search_commands: "Cerca comandi",
    toggle_sidebar: "Mostra o nascondi la barra laterale",
    toggle_panel: "Mostra o nascondi il pannello",
    toggle_agent: "Mostra o nascondi Claude Code",
    view_explorer: "Esplora risorse",
    view_search: "Cerca",
    view_source_control: "Controllo del codice sorgente",
    view_extensions: "Estensioni",
    open_editors: "Editor aperti",
    installed_extensions: "Installate",
    no_extensions: "Nessuna estensione installata",
    panel_problems: "Problemi",
    panel_output: "Output",
    panel_terminal: "Terminale",
    panel_ports: "Porte",
    no_output: "Nessun messaggio",
    agent_not_connected: "Non collegato",
    ask_claude: "Chiedi a Claude…",
    plain_text: "Testo normale",
    close_editor: "Chiudi",
    welcome_open: "Apri…",
    welcome_new_file: "Nuovo file",
    menu_new_window: "Nuova finestra",
    menu_open_recent: "Apri recente",
    recent_projects: "Recenti",
    open_recent_title: "Apri un progetto recente",
    no_recent_projects: "Nessun progetto recente",
    command_open: "File: Apri…",
    command_new_file: "File: Nuovo file",
    command_new_window: "File: Nuova finestra",
    command_open_recent: "File: Apri recente…",
    no_folder: "Apri una cartella per vederne i file.",
    explorer_new_file: "Nuovo file…",
    explorer_new_folder: "Nuova cartella…",
    explorer_refresh: "Aggiorna",
    explorer_collapse: "Comprimi tutto",
    explorer_rename: "Rinomina…",
    explorer_trash: "Sposta nel Cestino",
    explorer_reveal: "Mostra nel Finder",
    explorer_copy_path: "Copia percorso",
    explorer_copy_relative: "Copia percorso relativo",
    explorer_move: "Sposta",
    trash_detail: "Puoi recuperarlo dal Cestino.",
    command_explorer_new_file: "Esplora risorse: Nuovo file",
    command_explorer_new_folder: "Esplora risorse: Nuova cartella",
    command_explorer_refresh: "Esplora risorse: Aggiorna",
    command_explorer_collapse: "Esplora risorse: Comprimi tutto",
    tab_pin: "Fissa",
    tab_unpin: "Sblocca",
    tab_close_others: "Chiudi gli altri",
    tab_close_right: "Chiudi quelli a destra",
    tab_close_all: "Chiudi tutti",
    tab_split: "Dividi a destra",
    menu_close_editor: "Chiudi editor",
    menu_reopen_editor: "Riapri editor chiuso",
    menu_split_editor: "Dividi editor",
    command_close_editor: "Vista: Chiudi editor",
    command_close_others: "Vista: Chiudi gli altri editor del gruppo",
    command_close_group: "Vista: Chiudi tutti gli editor del gruppo",
    command_close_all: "Vista: Chiudi tutti gli editor",
    command_reopen_editor: "Vista: Riapri l'editor chiuso",
    command_pin_editor: "Vista: Fissa o sblocca l'editor",
    command_keep_editor: "Vista: Mantieni l'editor aperto",
    command_next_editor: "Vista: Editor successivo",
    command_previous_editor: "Vista: Editor precedente",
    command_split_editor: "Vista: Dividi l'editor",
    command_focus_group_1: "Vista: Vai al primo gruppo",
    command_focus_group_2: "Vista: Vai al secondo gruppo",
    command_focus_group_3: "Vista: Vai al terzo gruppo",
    search_files: "Cerca file",
    quick_open_placeholder: "Cerca un file per nome; aggiungi :riga per andarci",
    menu_go: "Vai",
    menu_go_to_file: "Vai al file…",
    menu_go_to_line: "Vai alla riga…",
    command_quick_open: "Vai al file…",
    command_go_to_line: "Vai alla riga o colonna…",
    command_toggle_sidebar: "Vista: Mostra o nascondi la barra laterale",
    command_toggle_panel: "Vista: Mostra o nascondi il pannello",
    command_toggle_agent: "Vista: Mostra o nascondi Claude Code",
    command_explorer: "Vista: Esplora risorse",
    command_search: "Vista: Cerca",
    command_source_control: "Vista: Controllo del codice sorgente",
    command_extensions: "Vista: Estensioni",
    command_problems: "Vista: Problemi",
    command_output: "Vista: Output",
    command_terminal: "Vista: Terminale",
    save_changes: "Vuoi salvare le modifiche a {name}?",
    could_not_save: "Keel non è riuscito a salvare {name}",
    could_not_use: "Keel non può usare {path}",
    could_not_start: "{extension} non è partita: {error}",
    could_not_open: "Keel non riesce ad aprire {path}: {error}",
    could_not_change: "Operazione non riuscita su {name}: {error}",
    go_to_line: "Riga attuale {current} di {total}. Scrivi la riga, anche con :colonna",
    move_question: "Vuoi spostare «{name}» in «{folder}»?",
    trash_question: "Vuoi spostare «{name}» nel Cestino?",
    coming_in_phase: "Arriva con la fase {phase}",
    line_column: "Riga {line}, col {column}",
    spaces: "Spazi: {size}",
    tab_size: "Tabulazione: {size}",
};

pub const ENGLISH: Strings = Strings {
    save: "Save",
    dont_save: "Don't Save",
    cancel: "Cancel",
    ok: "OK",
    untitled: "Untitled",
    untitled_file_name: "Untitled.txt",
    changes_lost: "Your changes will be lost if you don't save them.",
    type_a_command: "Type a command",
    choose_language: "Interface language",
    command_save: "File: Save",
    command_close_window: "View: Close Window",
    command_language: "Preferences: Language",
    command_theme: "Preferences: Color Theme",
    command_quit: "Quit Keel",
    menu_file: "File",
    menu_edit: "Edit",
    menu_view: "View",
    menu_services: "Services",
    menu_theme: "Color Theme…",
    menu_language: "Language…",
    menu_close_window: "Close Window",
    menu_undo: "Undo",
    menu_redo: "Redo",
    menu_cut: "Cut",
    menu_copy: "Copy",
    menu_paste: "Paste",
    menu_select_all: "Select All",
    menu_find: "Find…",
    menu_replace: "Replace…",
    menu_command_palette: "Command Palette…",
    choose_theme: "Color theme",
    settings_unavailable: "Settings are not available yet",
    search_commands: "Search commands",
    toggle_sidebar: "Show or hide the sidebar",
    toggle_panel: "Show or hide the panel",
    toggle_agent: "Show or hide Claude Code",
    view_explorer: "Explorer",
    view_search: "Search",
    view_source_control: "Source Control",
    view_extensions: "Extensions",
    open_editors: "Open Editors",
    installed_extensions: "Installed",
    no_extensions: "No extensions installed",
    panel_problems: "Problems",
    panel_output: "Output",
    panel_terminal: "Terminal",
    panel_ports: "Ports",
    no_output: "No messages",
    agent_not_connected: "Not connected",
    ask_claude: "Ask Claude…",
    plain_text: "Plain Text",
    close_editor: "Close",
    welcome_open: "Open…",
    welcome_new_file: "New File",
    menu_new_window: "New Window",
    menu_open_recent: "Open Recent",
    recent_projects: "Recent",
    open_recent_title: "Open a recent project",
    no_recent_projects: "No recent projects",
    command_open: "File: Open…",
    command_new_file: "File: New File",
    command_new_window: "File: New Window",
    command_open_recent: "File: Open Recent…",
    no_folder: "Open a folder to see its files.",
    explorer_new_file: "New File…",
    explorer_new_folder: "New Folder…",
    explorer_refresh: "Refresh",
    explorer_collapse: "Collapse All",
    explorer_rename: "Rename…",
    explorer_trash: "Move to Trash",
    explorer_reveal: "Reveal in Finder",
    explorer_copy_path: "Copy Path",
    explorer_copy_relative: "Copy Relative Path",
    explorer_move: "Move",
    trash_detail: "You can restore it from the Trash.",
    command_explorer_new_file: "Explorer: New File",
    command_explorer_new_folder: "Explorer: New Folder",
    command_explorer_refresh: "Explorer: Refresh",
    command_explorer_collapse: "Explorer: Collapse All",
    tab_pin: "Pin",
    tab_unpin: "Unpin",
    tab_close_others: "Close Others",
    tab_close_right: "Close to the Right",
    tab_close_all: "Close All",
    tab_split: "Split Right",
    menu_close_editor: "Close Editor",
    menu_reopen_editor: "Reopen Closed Editor",
    menu_split_editor: "Split Editor",
    command_close_editor: "View: Close Editor",
    command_close_others: "View: Close Other Editors in Group",
    command_close_group: "View: Close All Editors in Group",
    command_close_all: "View: Close All Editors",
    command_reopen_editor: "View: Reopen Closed Editor",
    command_pin_editor: "View: Pin or Unpin Editor",
    command_keep_editor: "View: Keep Editor",
    command_next_editor: "View: Open Next Editor",
    command_previous_editor: "View: Open Previous Editor",
    command_split_editor: "View: Split Editor",
    command_focus_group_1: "View: Focus First Editor Group",
    command_focus_group_2: "View: Focus Second Editor Group",
    command_focus_group_3: "View: Focus Third Editor Group",
    search_files: "Search files",
    quick_open_placeholder: "Search files by name; add :line to go there",
    menu_go: "Go",
    menu_go_to_file: "Go to File…",
    menu_go_to_line: "Go to Line…",
    command_quick_open: "Go to File…",
    command_go_to_line: "Go to Line/Column…",
    command_toggle_sidebar: "View: Toggle Sidebar",
    command_toggle_panel: "View: Toggle Panel",
    command_toggle_agent: "View: Toggle Claude Code",
    command_explorer: "View: Show Explorer",
    command_search: "View: Show Search",
    command_source_control: "View: Show Source Control",
    command_extensions: "View: Show Extensions",
    command_problems: "View: Toggle Problems",
    command_output: "View: Toggle Output",
    command_terminal: "View: Toggle Terminal",
    save_changes: "Do you want to save the changes you made to {name}?",
    could_not_save: "Keel could not save {name}",
    could_not_use: "Keel could not use {path}",
    could_not_start: "{extension} could not start: {error}",
    could_not_open: "Keel could not open {path}: {error}",
    could_not_change: "Could not change {name}: {error}",
    go_to_line: "Current line {current} of {total}. Type a line, with :column if you like",
    move_question: "Do you want to move \"{name}\" into \"{folder}\"?",
    trash_question: "Do you want to move \"{name}\" to the Trash?",
    coming_in_phase: "Coming in phase {phase}",
    line_column: "Ln {line}, Col {column}",
    spaces: "Spaces: {size}",
    tab_size: "Tab Size: {size}",
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_setting_wins_over_the_mac() {
        assert_eq!(
            UiLanguage::from_setting(Some("en"), Some("it-IT")),
            UiLanguage::English
        );
        assert_eq!(
            UiLanguage::from_setting(Some("it"), Some("en-US")),
            UiLanguage::Italian
        );
    }

    #[test]
    fn auto_follows_the_mac_and_falls_back_to_english() {
        assert_eq!(
            UiLanguage::from_setting(Some("auto"), Some("it-IT")),
            UiLanguage::Italian
        );
        assert_eq!(
            UiLanguage::from_setting(None, Some("IT")),
            UiLanguage::Italian
        );
        assert_eq!(
            UiLanguage::from_setting(None, Some("de-DE")),
            UiLanguage::English
        );
        assert_eq!(UiLanguage::from_setting(None, None), UiLanguage::English);
    }

    #[test]
    fn texts_fill_in_their_values() {
        assert_eq!(
            ITALIAN.save_changes("app.ts"),
            "Vuoi salvare le modifiche a app.ts?"
        );
        assert_eq!(
            ENGLISH.could_not_save("app.ts"),
            "Keel could not save app.ts"
        );
        assert_eq!(
            ITALIAN.could_not_start("manuel-local.vsauto-ghost", "boom"),
            "manuel-local.vsauto-ghost non è partita: boom"
        );
        assert_eq!(ITALIAN.coming_in_phase(3), "Arriva con la fase 3");
        assert_eq!(ENGLISH.line_column(33, 28), "Ln 33, Col 28");
        assert_eq!(ITALIAN.indentation(2, false), "Spazi: 2");
        assert_eq!(ENGLISH.indentation(4, true), "Tab Size: 4");
    }
}
