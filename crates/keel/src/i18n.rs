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
    save_changes: &'static str,
    could_not_save: &'static str,
    could_not_use: &'static str,
    could_not_start: &'static str,
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
    save_changes: "Vuoi salvare le modifiche a {name}?",
    could_not_save: "Keel non è riuscito a salvare {name}",
    could_not_use: "Keel non può usare {path}",
    could_not_start: "{extension} non è partita: {error}",
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
    save_changes: "Do you want to save the changes you made to {name}?",
    could_not_save: "Keel could not save {name}",
    could_not_use: "Keel could not use {path}",
    could_not_start: "{extension} could not start: {error}",
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
    }
}
