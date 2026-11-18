//! The Settings tab (Cmd+,): the settings Keel reads, by page, with a
//! switch, a menu, a number or a text field each, as VS Code's settings
//! editor. A change is written to the user's `settings.json` at once,
//! keeping its comments, and takes effect in the window.
//! "Open Settings (JSON)" opens the file itself.

use gpui::{AnyElement, AnyWindowHandle, SharedString, WeakEntity};
use gpui_component::Sizable as _;
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::setting::{
    NumberFieldOptions, SettingField, SettingGroup, SettingItem, SettingPage,
    Settings as SettingsPage,
};

use super::*;

/// The special tabs drawn by Keel rather than an editor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum PageKind {
    Settings,
    Keybindings,
}

impl PageKind {
    pub(super) fn uri(self) -> &'static str {
        match self {
            PageKind::Settings => "keel-settings:",
            PageKind::Keybindings => "keel-keybindings:",
        }
    }
}

/// What kind of value a setting takes.
#[derive(Clone, Copy)]
pub(super) enum Kind {
    Switch,
    Number {
        min: f64,
        max: f64,
    },
    /// The values and their names in Italian and English.
    Choice(&'static [(&'static str, &'static str, &'static str)]),
    Text,
    /// An object or a list: edited in `settings.json`.
    Json,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Page {
    Appearance,
    Editor,
    Files,
    Terminal,
    Languages,
    Extensions,
}

pub(super) struct Entry {
    pub(super) key: &'static str,
    pub(super) page: Page,
    pub(super) kind: Kind,
    /// The value when nobody sets it, as JSON.
    pub(super) default: &'static str,
    pub(super) title: (&'static str, &'static str),
    pub(super) description: (&'static str, &'static str),
}

const ON_OFF: &[(&str, &str, &str)] = &[("off", "Spento", "Off"), ("on", "Acceso", "On")];

/// Every setting the window offers, as VS Code names them.
pub(super) const CATALOG: &[Entry] = &[
    Entry {
        key: "keel.language",
        page: Page::Appearance,
        kind: Kind::Choice(&[
            ("auto", "Come macOS", "Like macOS"),
            ("it", "Italiano", "Italian"),
            ("en", "Inglese", "English"),
        ]),
        default: "\"auto\"",
        title: ("Lingua", "Language"),
        description: (
            "La lingua dei menu e dei testi di Keel.",
            "The language of Keel's menus and texts.",
        ),
    },
    Entry {
        key: "workbench.colorTheme",
        page: Page::Appearance,
        kind: Kind::Text,
        default: "\"Mezzanotte\"",
        title: ("Tema dei colori", "Color Theme"),
        description: (
            "Il nome del tema; «Preferenze: Tema colori» li elenca.",
            "The theme's name; \"Preferences: Color Theme\" lists them.",
        ),
    },
    Entry {
        key: "window.autoDetectColorScheme",
        page: Page::Appearance,
        kind: Kind::Switch,
        default: "false",
        title: (
            "Segui chiaro e scuro di macOS",
            "Follow macOS Light and Dark",
        ),
        description: (
            "Usa il tema chiaro o scuro preferito secondo l'aspetto di macOS.",
            "Use the preferred light or dark theme as macOS changes appearance.",
        ),
    },
    Entry {
        key: "workbench.preferredDarkColorTheme",
        page: Page::Appearance,
        kind: Kind::Text,
        default: "\"Mezzanotte\"",
        title: ("Tema scuro preferito", "Preferred Dark Theme"),
        description: (
            "Il tema quando macOS è scuro.",
            "The theme when macOS is dark.",
        ),
    },
    Entry {
        key: "workbench.preferredLightColorTheme",
        page: Page::Appearance,
        kind: Kind::Text,
        default: "\"Chiaro\"",
        title: ("Tema chiaro preferito", "Preferred Light Theme"),
        description: (
            "Il tema quando macOS è chiaro.",
            "The theme when macOS is light.",
        ),
    },
    Entry {
        key: "breadcrumbs.enabled",
        page: Page::Appearance,
        kind: Kind::Switch,
        default: "true",
        title: ("Percorso sopra l'editor", "Breadcrumbs"),
        description: (
            "Mostra il percorso del file sotto le schede.",
            "Show the file's path under the tabs.",
        ),
    },
    Entry {
        key: "editor.fontSize",
        page: Page::Editor,
        kind: Kind::Number { min: 6., max: 72. },
        default: "14",
        title: ("Dimensione del testo", "Font Size"),
        description: (
            "In punti. Cmd+= e Cmd+- la cambiano per la sessione.",
            "In points. Cmd+= and Cmd+- change it for the session.",
        ),
    },
    Entry {
        key: "editor.tabSize",
        page: Page::Editor,
        kind: Kind::Number { min: 1., max: 16. },
        default: "2",
        title: ("Larghezza del rientro", "Tab Size"),
        description: (
            "Quanti spazi vale un rientro.",
            "How many spaces an indentation is.",
        ),
    },
    Entry {
        key: "editor.insertSpaces",
        page: Page::Editor,
        kind: Kind::Switch,
        default: "true",
        title: ("Rientra con spazi", "Insert Spaces"),
        description: (
            "Spenta, Tab scrive un tab vero. I Makefile usano sempre i tab.",
            "Off, Tab types a real tab. Makefiles always use tabs.",
        ),
    },
    Entry {
        key: "editor.wordWrap",
        page: Page::Editor,
        kind: Kind::Choice(ON_OFF),
        default: "\"off\"",
        title: ("A capo automatico", "Word Wrap"),
        description: (
            "Le righe lunghe vanno a capo; Alt+Z lo cambia al volo.",
            "Long lines wrap; Alt+Z toggles it.",
        ),
    },
    Entry {
        key: "editor.formatOnSave",
        page: Page::Editor,
        kind: Kind::Switch,
        default: "false",
        title: ("Formatta al salvataggio", "Format on Save"),
        description: (
            "Cmd+S formatta il file prima di salvarlo.",
            "Cmd+S formats the file before saving it.",
        ),
    },
    Entry {
        key: "editor.defaultFormatter",
        page: Page::Editor,
        kind: Kind::Text,
        default: "\"\"",
        title: ("Chi formatta", "Default Formatter"),
        description: (
            "Vuoto: Prettier del progetto se c'è, altrimenti il server del linguaggio. «esbenp.prettier-vscode» vale Prettier.",
            "Empty: the project's Prettier if any, otherwise the language server. \"esbenp.prettier-vscode\" means Prettier.",
        ),
    },
    Entry {
        key: "editor.codeActionsOnSave",
        page: Page::Editor,
        kind: Kind::Json,
        default: "{}",
        title: ("Azioni al salvataggio", "Code Actions on Save"),
        description: (
            "Per esempio { \"source.fixAll.eslint\": \"explicit\" }.",
            "For example { \"source.fixAll.eslint\": \"explicit\" }.",
        ),
    },
    Entry {
        key: "editor.stickyScroll.enabled",
        page: Page::Editor,
        kind: Kind::Switch,
        default: "true",
        title: ("Righe ferme in alto (sticky scroll)", "Sticky Scroll"),
        description: (
            "Le prime righe dei blocchi in cui sei restano in cima. Vale per i file aperti dopo.",
            "The first lines of the blocks you are in stay at the top. Applies to files opened after.",
        ),
    },
    Entry {
        key: "editor.minimap.enabled",
        page: Page::Editor,
        kind: Kind::Switch,
        default: "true",
        title: ("Minimappa", "Minimap"),
        description: (
            "Il file in piccolo a destra. Vale per i file aperti dopo.",
            "The file in small on the right. Applies to files opened after.",
        ),
    },
    Entry {
        key: "workbench.editor.enablePreview",
        page: Page::Editor,
        kind: Kind::Switch,
        default: "true",
        title: ("Schede di anteprima", "Preview Tabs"),
        description: (
            "Un file aperto con un clic prende il posto del precedente finché non lo modifichi.",
            "A file opened with one click takes the previous one's place until you edit it.",
        ),
    },
    Entry {
        key: "diffEditor.renderSideBySide",
        page: Page::Editor,
        kind: Kind::Switch,
        default: "true",
        title: ("Differenze affiancate", "Side by Side Diff"),
        description: (
            "Spenta, le differenze sono in una colonna sola.",
            "Off, differences are in a single column.",
        ),
    },
    Entry {
        key: "files.autoSave",
        page: Page::Files,
        kind: Kind::Choice(&[
            ("off", "Spento", "Off"),
            ("afterDelay", "Dopo una pausa", "After Delay"),
            ("onFocusChange", "Cambiando editor", "On Focus Change"),
            ("onWindowChange", "Cambiando finestra", "On Window Change"),
        ]),
        default: "\"off\"",
        title: ("Salvataggio automatico", "Auto Save"),
        description: (
            "Quando salvare i file da soli.",
            "When to save files by themselves.",
        ),
    },
    Entry {
        key: "files.autoSaveDelay",
        page: Page::Files,
        kind: Kind::Number {
            min: 100.,
            max: 60_000.,
        },
        default: "1000",
        title: ("Pausa del salvataggio automatico", "Auto Save Delay"),
        description: (
            "In millisecondi, con «Dopo una pausa».",
            "In milliseconds, with \"After Delay\".",
        ),
    },
    Entry {
        key: "files.trimTrailingWhitespace",
        page: Page::Files,
        kind: Kind::Switch,
        default: "false",
        title: (
            "Togli gli spazi in fondo alle righe",
            "Trim Trailing Whitespace",
        ),
        description: ("Al salvataggio.", "On save."),
    },
    Entry {
        key: "files.insertFinalNewline",
        page: Page::Files,
        kind: Kind::Switch,
        default: "false",
        title: ("A capo alla fine del file", "Insert Final Newline"),
        description: ("Al salvataggio.", "On save."),
    },
    Entry {
        key: "files.hotExit",
        page: Page::Files,
        kind: Kind::Choice(&[
            ("onExit", "All'uscita", "On Exit"),
            ("off", "Spento", "Off"),
        ]),
        default: "\"onExit\"",
        title: ("Uscita senza domande", "Hot Exit"),
        description: (
            "Cmd+Q tiene le modifiche non salvate per la prossima volta.",
            "Cmd+Q keeps unsaved changes for next time.",
        ),
    },
    Entry {
        key: "window.restoreWindows",
        page: Page::Files,
        kind: Kind::Choice(&[("all", "Tutte", "All"), ("none", "Nessuna", "None")]),
        default: "\"all\"",
        title: ("Riapri le finestre", "Restore Windows"),
        description: (
            "Aprendo Keel senza percorso.",
            "When Keel opens without a path.",
        ),
    },
    Entry {
        key: "explorer.autoReveal",
        page: Page::Files,
        kind: Kind::Switch,
        default: "true",
        title: ("Mostra il file nell'albero", "Auto Reveal"),
        description: (
            "L'Esplora risorse segue il file aperto.",
            "The Explorer follows the open file.",
        ),
    },
    Entry {
        key: "npm.enableScriptExplorer",
        page: Page::Files,
        kind: Kind::Switch,
        default: "true",
        title: ("Script npm sotto i file", "NPM Scripts in the Explorer"),
        description: (
            "Mostra gli script di package.json sotto l'albero dei file.",
            "Show package.json's scripts under the file tree.",
        ),
    },
    Entry {
        key: "files.exclude",
        page: Page::Files,
        kind: Kind::Json,
        default: "{}",
        title: ("File nascosti", "Files Exclude"),
        description: (
            "Per esempio { \"**/dist\": true }.",
            "For example { \"**/dist\": true }.",
        ),
    },
    Entry {
        key: "terminal.integrated.fontSize",
        page: Page::Terminal,
        kind: Kind::Number { min: 6., max: 72. },
        default: "12",
        title: ("Dimensione del testo", "Font Size"),
        description: ("Del terminale, in punti.", "Of the terminal, in points."),
    },
    Entry {
        key: "terminal.integrated.fontFamily",
        page: Page::Terminal,
        kind: Kind::Text,
        default: "\"\"",
        title: ("Carattere", "Font Family"),
        description: ("Vuoto: quello dell'editor.", "Empty: the editor's."),
    },
    Entry {
        key: "terminal.integrated.macOptionIsMeta",
        page: Page::Terminal,
        kind: Kind::Switch,
        default: "false",
        title: ("Alt come Meta", "Option Is Meta"),
        description: (
            "Per le scorciatoie di zsh e di Emacs; spenta, Alt scrive le lettere accentate.",
            "For zsh and Emacs shortcuts; off, Option types accented letters.",
        ),
    },
    Entry {
        key: "keel.languageServers.autoInstall",
        page: Page::Languages,
        kind: Kind::Switch,
        default: "true",
        title: ("Installa i server da solo", "Install Servers Automatically"),
        description: (
            "Scarica il server di un linguaggio la prima volta che serve.",
            "Download a language's server the first time it is needed.",
        ),
    },
    Entry {
        key: "keel.languageServers.disabled",
        page: Page::Languages,
        kind: Kind::Json,
        default: "[]",
        title: ("Server spenti", "Disabled Servers"),
        description: (
            "Per esempio [\"eslint\", \"tailwindcss\"].",
            "For example [\"eslint\", \"tailwindcss\"].",
        ),
    },
    Entry {
        key: "extensions.autoCheckUpdates",
        page: Page::Extensions,
        kind: Kind::Switch,
        default: "true",
        title: ("Controlla gli aggiornamenti", "Check for Updates"),
        description: (
            "Cerca nuove versioni delle estensioni su Open VSX.",
            "Look for new versions of extensions on Open VSX.",
        ),
    },
];

impl Workbench {
    /// Opens a special tab, or shows it if it is open.
    pub(super) fn open_page(
        &mut self,
        kind: PageKind,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let uri = kind.uri().to_owned();
        if !self.documents.contains_key(&uri) {
            self.documents.insert(
                uri.clone(),
                OpenDocument {
                    uri: uri.clone(),
                    backing: Backing::Page(kind),
                    language_id: "keel-page".to_owned(),
                    version: 1,
                    synced_text: String::new(),
                    modified: false,
                },
            );
        }
        let column = self.active.min(self.panes.len().saturating_sub(1));
        self.show_tab(&uri, column, OpenAs::Normal, true, window, cx);
    }

    /// Opens the user's `settings.json`, creating it empty if needed.
    pub(super) fn open_settings_json(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(data) = self.data_dir.clone() else {
            return;
        };
        let file = crate::paths::user_settings_file(&data);
        if !file.exists() {
            let _ = std::fs::create_dir_all(&data);
            let _ = std::fs::write(&file, "{\n}\n");
        }
        self.open_file(&file, window, cx);
    }

    /// Saves a setting in the user's file and applies it to the window.
    pub(super) fn change_setting(
        &mut self,
        key: &str,
        value: Option<Value>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Err(error) = self.settings.update_user(key, value) {
            self.report(format!("{error:#}"));
            cx.notify();
            return;
        }
        self.apply_setting(key, window, cx);
        self.language_settings_changed();
        cx.notify();
    }

    /// Applies a setting that changed to what the window keeps from it.
    pub(super) fn apply_setting(&mut self, key: &str, window: &mut Window, cx: &mut Context<Self>) {
        match key {
            "editor.fontSize" => self.zoom(0., cx),
            "editor.wordWrap" => {
                let wrap = self.settings.string("editor.wordWrap") == Some("on");
                self.set_word_wrap(wrap, window, cx);
            }
            "keel.language" => {
                let language = UiLanguage::from_setting(
                    self.settings.string("keel.language"),
                    UiLanguage::system_locale().as_deref(),
                );
                self.set_language(language, window, cx);
            }
            "workbench.colorTheme"
            | "window.autoDetectColorScheme"
            | "workbench.preferredDarkColorTheme"
            | "workbench.preferredLightColorTheme" => {
                let choice = crate::theme::ThemeChoice::from_settings(&self.settings);
                crate::theme::apply(&choice, Some(window), cx);
            }
            "editor.tabSize" | "editor.insertSpaces" => self.apply_tab_size(cx),
            _ => {}
        }
    }

    /// Gives every open editor the indentation of its language again.
    fn apply_tab_size(&mut self, cx: &mut Context<Self>) {
        let editors: Vec<(Entity<EditorState>, String)> = self
            .panes
            .iter()
            .map(|pane| (pane.editor.clone(), pane.uri.clone()))
            .chain(
                self.groups
                    .iter()
                    .flat_map(|group| &group.tabs)
                    .filter_map(|tab| {
                        tab.parked
                            .as_ref()
                            .map(|pane| (pane.editor.clone(), pane.uri.clone()))
                    }),
            )
            .collect();
        for (editor, uri) in editors {
            let language = self
                .documents
                .get(&uri)
                .map(|document| document.language_id.clone())
                .unwrap_or_default();
            let tab_size = self.tab_size_for(&language);
            editor.update(cx, |editor, cx| editor.set_tab_size(tab_size, cx));
        }
    }

    pub(super) fn render_settings_page(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let italian = self.language == UiLanguage::Italian;
        let pick = |pair: (&'static str, &'static str)| if italian { pair.0 } else { pair.1 };
        let workbench = cx.entity().downgrade();
        let handle = window.window_handle();
        let pages = [
            (Page::Appearance, ("Aspetto", "Appearance")),
            (Page::Editor, ("Editor", "Editor")),
            (Page::Files, ("File e finestre", "Files and Windows")),
            (Page::Terminal, ("Terminale", "Terminal")),
            (Page::Languages, ("Linguaggi", "Languages")),
            (Page::Extensions, ("Estensioni", "Extensions")),
        ];
        let mut settings = SettingsPage::new("keel-settings").sidebar_width(px(200.));
        for (page, title) in pages {
            let items: Vec<SettingItem> = CATALOG
                .iter()
                .filter(|entry| entry.page == page)
                .map(|entry| {
                    let mut description = pick(entry.description).to_owned();
                    if self.settings.source_of(entry.key) == Some(settings::Source::Project)
                        || self.settings.source_of(entry.key)
                            == Some(settings::Source::VsCodeProject)
                    {
                        description.push_str(if italian {
                            " Il progetto la cambia nelle sue impostazioni."
                        } else {
                            " The project changes it in its settings."
                        });
                    }
                    item(entry, italian, &workbench, handle)
                        .keywords([entry.key])
                        .description(description)
                })
                .collect();
            settings = settings.page(
                SettingPage::new(pick(title))
                    .resettable(true)
                    .group(SettingGroup::new().items(items)),
            );
        }
        let strings = self.strings();
        div()
            .size_full()
            .flex()
            .flex_col()
            .child(
                div().flex().justify_end().px_3().pt_2().child(
                    Button::new("open-settings-json")
                        .ghost()
                        .small()
                        .label(strings.open_settings_json)
                        .on_click(
                            cx.listener(|this, _, window, cx| this.open_settings_json(window, cx)),
                        ),
                ),
            )
            .child(div().flex_1().min_h_0().child(settings))
            .into_any_element()
    }
}

/// The item of one setting, reading and writing it through the window.
fn item(
    entry: &'static Entry,
    italian: bool,
    workbench: &WeakEntity<Workbench>,
    handle: AnyWindowHandle,
) -> SettingItem {
    let title = if italian {
        entry.title.0
    } else {
        entry.title.1
    };
    let default: Value = serde_json::from_str(entry.default).unwrap_or(Value::Null);
    let key = entry.key;
    let read = {
        let workbench = workbench.clone();
        let default = default.clone();
        std::rc::Rc::new(move |cx: &App| -> Value {
            workbench
                .upgrade()
                .and_then(|workbench| workbench.read(cx).settings.get(key).cloned())
                .unwrap_or_else(|| default.clone())
        })
    };
    let write = {
        let workbench = workbench.clone();
        std::rc::Rc::new(move |value: Value, cx: &mut App| {
            let workbench = workbench.clone();
            let _ = handle.update(cx, |_, window, cx| {
                workbench
                    .update(cx, |workbench, cx| {
                        workbench.change_setting(key, Some(value), window, cx)
                    })
                    .ok();
            });
        })
    };
    let as_text = |value: &Value| match value {
        Value::String(text) => text.clone(),
        Value::Null => String::new(),
        other => other.to_string(),
    };
    match entry.kind {
        Kind::Switch => SettingItem::new(
            title,
            SettingField::switch(
                {
                    let read = read.clone();
                    move |cx| read(cx).as_bool().unwrap_or(false)
                },
                move |on, cx| write(Value::Bool(on), cx),
            )
            .default_value(default.as_bool().unwrap_or(false)),
        ),
        Kind::Number { min, max } => SettingItem::new(
            title,
            SettingField::number_input(
                NumberFieldOptions { min, max, step: 1. },
                {
                    let read = read.clone();
                    move |cx| read(cx).as_f64().unwrap_or(0.)
                },
                move |value, cx| {
                    // Whole numbers stay whole in the file.
                    let value = if value.fract() == 0. {
                        serde_json::json!(value as i64)
                    } else {
                        serde_json::json!(value)
                    };
                    write(value, cx)
                },
            )
            .default_value(default.as_f64().unwrap_or(0.)),
        ),
        Kind::Choice(options) => {
            let options: Vec<(SharedString, SharedString)> = options
                .iter()
                .map(|(value, it, en)| ((*value).into(), (if italian { *it } else { *en }).into()))
                .collect();
            SettingItem::new(
                title,
                SettingField::dropdown(
                    options,
                    {
                        let read = read.clone();
                        move |cx| as_text(&read(cx)).into()
                    },
                    move |value: SharedString, cx| write(Value::String(value.to_string()), cx),
                )
                .default_value(SharedString::from(as_text(&default))),
            )
        }
        Kind::Text => SettingItem::new(
            title,
            SettingField::input(
                {
                    let read = read.clone();
                    move |cx| as_text(&read(cx)).into()
                },
                move |value: SharedString, cx| write(Value::String(value.to_string()), cx),
            )
            .default_value(SharedString::from(as_text(&default))),
        ),
        Kind::Json => {
            let workbench = workbench.clone();
            SettingItem::new(
                title,
                SettingField::<SharedString>::render(move |_, _, _| {
                    let workbench = workbench.clone();
                    let label = if italian {
                        "Modifica in settings.json"
                    } else {
                        "Edit in settings.json"
                    };
                    Button::new(SharedString::from(format!("setting-json-{key}")))
                        .outline()
                        .small()
                        .label(label)
                        .on_click(move |_, window, cx| {
                            workbench
                                .update(cx, |workbench, cx| {
                                    workbench.open_settings_json(window, cx)
                                })
                                .ok();
                        })
                }),
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use gpui::{TestAppContext, VisualTestContext};

    use super::*;

    fn window(
        cx: &mut TestAppContext,
    ) -> (
        tempfile::TempDir,
        PathBuf,
        Entity<Workbench>,
        &mut VisualTestContext,
    ) {
        cx.update(|cx| {
            gpui_component::init(cx);
            language::register_grammars();
            crate::workbench_window::init(cx);
        });
        let folder = tempfile::tempdir().unwrap();
        let data = std::fs::canonicalize(folder.path()).unwrap();
        let root = data.join("progetto");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("app.ts"), "uno();\n").unwrap();
        let settings_file = crate::paths::user_settings_file(&data);
        std::fs::write(
            &settings_file,
            "{\n    // mio\n    \"keel.language\": \"it\"\n}\n",
        )
        .unwrap();
        let settings = Settings::load(&settings_file, Some(&root));
        let opening = Opening::new(Some(root.clone()), None).with_data_dir(data.clone());
        let (view, cx) =
            cx.add_window_view(|window, cx| Workbench::new(opening, settings, None, window, cx));
        (folder, data, view, cx)
    }

    #[gpui::test]
    fn a_setting_changed_in_the_page_is_saved_and_applied(cx: &mut TestAppContext) {
        let (_folder, data, view, cx) = window(cx);
        cx.simulate_keystrokes("cmd-,");
        let open = view.read_with(cx, |view, _| {
            view.documents.contains_key(PageKind::Settings.uri())
        });
        assert!(open, "Cmd+, opens the Settings tab");
        cx.run_until_parked();
        view.update_in(cx, |view, window, cx| {
            view.change_setting("editor.fontSize", Some(json!(18)), window, cx)
        });
        assert_eq!(view.read_with(cx, |view, _| view.font_size), 18.);
        let text = std::fs::read_to_string(crate::paths::user_settings_file(&data)).unwrap();
        assert_eq!(
            text,
            "{\n    // mio\n    \"keel.language\": \"it\",\n    \"editor.fontSize\": 18\n}\n"
        );
    }

    #[gpui::test]
    fn a_new_shortcut_works_at_once_and_the_old_one_stops(cx: &mut TestAppContext) {
        let (_folder, data, view, cx) = window(cx);
        let file = view.read_with(cx, |view, _| {
            view.project.root.clone().unwrap().join("app.ts")
        });
        view.update_in(cx, |view, window, cx| view.open_file(&file, window, cx));
        let text = |view: &Entity<Workbench>, cx: &mut VisualTestContext| {
            view.read_with(cx, |view, cx| {
                view.active_editor().unwrap().read(cx).value().to_string()
            })
        };
        view.update_in(cx, |view, window, cx| {
            view.set_keybinding(
                "editor.action.commentLine",
                Some("cmd-alt-/".to_owned()),
                window,
                cx,
            )
        });
        let written = std::fs::read_to_string(crate::paths::user_keybindings_file(&data)).unwrap();
        assert!(
            written.contains("\"cmd+alt+/\"") && written.contains("-editor.action.commentLine")
        );
        view.update_in(cx, |view, window, cx| {
            view.active_editor()
                .unwrap()
                .update(cx, |editor, cx| editor.focus(window, cx))
        });
        cx.simulate_keystrokes("cmd-/");
        assert_eq!(text(&view, cx), "uno();\n", "the old key stopped");
        cx.simulate_keystrokes("cmd-alt-/");
        assert_eq!(text(&view, cx), "// uno();\n");
        // Back to the default.
        view.update_in(cx, |view, window, cx| {
            view.set_keybinding("editor.action.commentLine", None, window, cx)
        });
        cx.simulate_keystrokes("cmd-/");
        assert_eq!(text(&view, cx), "uno();\n");
    }
}
