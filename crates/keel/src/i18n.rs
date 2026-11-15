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
