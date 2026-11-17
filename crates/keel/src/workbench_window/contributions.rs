//! What extensions add to Keel's look and languages without running code:
//! color themes (in the theme picker), file icon themes (in the Explorer)
//! and languages (file associations and comments). They follow the running
//! extensions, so turning one off takes its themes and languages away.

use std::path::Path;

use extensions::color_theme;
use extensions::icon_theme::{self, Entry, IconTheme};
use extensions::languages::{self, LanguageRegistry};
use gpui_component::ThemeRegistry;

use super::*;

actions!(keel, [SelectIconTheme]);

/// The colors of `workbench.colorCustomizations` Keel uses, as Peacock sets
/// them: the title bar and the status bar.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(super) struct CustomColors {
    pub(super) title_background: Option<gpui::Hsla>,
    pub(super) title_foreground: Option<gpui::Hsla>,
    pub(super) status_background: Option<gpui::Hsla>,
    pub(super) status_foreground: Option<gpui::Hsla>,
}

/// `#RGB`, `#RGBA`, `#RRGGBB` or `#RRGGBBAA`.
fn parse_color(value: &Value) -> Option<gpui::Hsla> {
    gpui::Rgba::try_from(value.as_str()?).ok().map(Into::into)
}

impl Workbench {
    /// Registers the themes and languages of the running extensions and
    /// loads the icon theme of `workbench.iconTheme`.
    pub(super) fn apply_contributions(
        &mut self,
        running: &[InstalledExtension],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let mut registered = false;
        for extension in running {
            let themes: Vec<Value> = color_theme::color_themes(extension)
                .iter()
                .filter_map(|theme| match color_theme::convert(theme) {
                    Ok(theme) => Some(theme),
                    Err(error) => {
                        self.log(format!("{}: {error:#}", extension.id()));
                        None
                    }
                })
                .collect();
            if themes.is_empty() {
                continue;
            }
            let set = json!({
                "name": extension.id(),
                "author": extension.manifest.publisher,
                "themes": themes,
            });
            match ThemeRegistry::global_mut(cx).load_themes_from_str(&set.to_string()) {
                Ok(()) => registered = true,
                Err(error) => self.log(format!("{}: {error:#}", extension.id())),
            }
        }
        // A theme saved in the settings may only exist now.
        let choice = crate::theme::ThemeChoice::from_settings(&self.settings);
        let wanted = if choice.follow_system {
            vec![choice.dark.clone(), choice.light.clone()]
        } else {
            vec![choice.theme.clone()]
        };
        if registered
            && wanted
                .iter()
                .any(|name| !crate::theme::NAMES.contains(&name.as_str()))
        {
            crate::theme::apply(&choice, Some(window), cx);
        }

        self.extension_languages = Rc::new(LanguageRegistry::new(
            running.iter().flat_map(languages::language_contributions),
        ));
        // Files opened as plain text may belong to a language just added.
        let relabeled: Vec<(String, String)> = self
            .documents
            .values()
            .filter_map(|document| match &document.backing {
                Backing::File(file) if document.language_id == "plaintext" => {
                    let language = self.language_id_for(file.path());
                    (language != "plaintext").then(|| (document.uri.clone(), language))
                }
                _ => None,
            })
            .collect();
        for (uri, language) in relabeled {
            if let Some(document) = self.documents.get_mut(&uri) {
                document.language_id = language;
            }
        }

        let wanted = self
            .settings
            .string("workbench.iconTheme")
            .map(str::to_owned);
        self.load_icon_theme(running, wanted);
    }

    /// `workbench.colorCustomizations`, with the overrides for the current
    /// theme (`"[Dracula Theme]": {...}`) on top.
    pub(super) fn custom_colors(&self, cx: &App) -> CustomColors {
        let Some(all) = self
            .settings
            .get("workbench.colorCustomizations")
            .and_then(Value::as_object)
        else {
            return CustomColors::default();
        };
        let theme = gpui_component::Theme::global(cx);
        let current = if theme.mode.is_dark() {
            &theme.dark_theme.name
        } else {
            &theme.light_theme.name
        };
        let scoped = all.get(&format!("[{current}]")).and_then(Value::as_object);
        let color = |key: &str| {
            scoped
                .and_then(|scoped| scoped.get(key))
                .or_else(|| all.get(key))
                .and_then(parse_color)
        };
        CustomColors {
            title_background: color("titleBar.activeBackground"),
            title_foreground: color("titleBar.activeForeground"),
            status_background: color("statusBar.background"),
            status_foreground: color("statusBar.foreground"),
        }
    }

    /// The VS Code language of a file: Keel's own first, then the
    /// languages of the running extensions.
    pub(super) fn language_id_for(&self, path: &Path) -> String {
        let builtin = language::vscode_id_for_path(path);
        // Spring Boot's configuration, as its VS Code extension names it,
        // in Spring Boot projects only: elsewhere it stays plain YAML.
        if let Some(spring) = spring_boot_language(path, builtin)
            && self.project.root.as_deref().is_some_and(|root| {
                lsp::servers::spec("spring-boot").is_some_and(|spec| spec.wanted_in(root))
            })
        {
            return spring.to_owned();
        }
        if builtin != "plaintext" {
            return builtin.to_owned();
        }
        path.file_name()
            .and_then(|name| name.to_str())
            .and_then(|name| self.extension_languages.language_for(name))
            .unwrap_or(builtin)
            .to_owned()
    }

    /// How `language` writes comments: Keel's own, or an extension's.
    pub(super) fn comment_syntax_for<'a>(
        languages: &'a LanguageRegistry,
        language: &str,
    ) -> editor::line_edits::CommentSyntax<'a> {
        let builtin = editor::line_edits::comment_syntax(language);
        if builtin.line.is_some() || builtin.block.is_some() {
            return builtin;
        }
        match languages.comments(language) {
            Some((line, block)) => editor::line_edits::CommentSyntax { line, block },
            None => builtin,
        }
    }

    /// The icon themes of the running extensions.
    fn icon_theme_contributions(&self) -> Vec<icon_theme::IconThemeContribution> {
        self.installed
            .iter()
            .filter(|extension| self.extension_enabled(&extension.id()))
            .flat_map(icon_theme::icon_themes)
            .collect()
    }

    /// Loads the icon theme `wanted`, if a running extension has it;
    /// otherwise the Explorer keeps Keel's icons.
    fn load_icon_theme(&mut self, running: &[InstalledExtension], wanted: Option<String>) {
        self.icon_theme = wanted.and_then(|wanted| {
            let contribution = running
                .iter()
                .flat_map(icon_theme::icon_themes)
                .find(|theme| theme.id == wanted)?;
            match IconTheme::load(&contribution) {
                Ok(theme) => Some(Rc::new(theme)),
                Err(error) => {
                    self.log(format!("{wanted}: {error:#}"));
                    None
                }
            }
        });
    }

    /// The picture the icon theme has for a row of the Explorer.
    pub(super) fn themed_icon(
        &self,
        path: &Path,
        is_dir: bool,
        expanded: bool,
        cx: &App,
    ) -> Option<std::path::PathBuf> {
        let theme = self.icon_theme.as_ref()?;
        let name = path.file_name()?.to_str()?;
        let light = !cx.theme().is_dark();
        let entry = if is_dir {
            Entry::Folder {
                name,
                expanded,
                root: false,
            }
        } else {
            let language = self.language_id_for(path);
            return theme
                .icon(
                    Entry::File {
                        name,
                        language: &language,
                    },
                    light,
                )
                .map(Path::to_path_buf);
        };
        theme.icon(entry, light).map(Path::to_path_buf)
    }

    /// The theme picker: Keel's themes, then those of the extensions.
    pub(super) fn theme_names(&self) -> Vec<String> {
        let mut names: Vec<String> = crate::theme::NAMES
            .iter()
            .map(|name| (*name).to_owned())
            .collect();
        for extension in self
            .installed
            .iter()
            .filter(|extension| self.extension_enabled(&extension.id()))
        {
            for theme in color_theme::color_themes(extension) {
                if !names.contains(&theme.label) {
                    names.push(theme.label);
                }
            }
        }
        names
    }

    /// "Preferences: File Icon Theme": Keel's icons or an extension's.
    fn choose_icon_theme(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let strings = self.strings();
        let themes = self.icon_theme_contributions();
        let mut items = vec![PickItem::new(strings.icon_theme_none)];
        items.extend(
            themes
                .iter()
                .map(|theme| PickItem::new(theme.label.clone())),
        );
        let chosen = quick_pick::show(
            Some(strings.choose_icon_theme.to_owned()),
            None,
            items,
            window,
            cx,
        );
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Some(index)) = chosen.await else {
                return;
            };
            let id = index
                .checked_sub(1)
                .and_then(|index| themes.get(index))
                .map(|theme| theme.id.clone());
            this.update(cx, |this, cx| this.set_icon_theme(id, cx)).ok();
        })
        .detach();
    }

    /// Saves and shows an icon theme; none means Keel's icons.
    pub(super) fn set_icon_theme(&mut self, id: Option<String>, cx: &mut Context<Self>) {
        if let Err(error) = self
            .settings
            .update_user("workbench.iconTheme", id.clone().map(Value::String))
        {
            self.report(error.to_string());
        }
        let running: Vec<InstalledExtension> = self
            .installed
            .iter()
            .filter(|extension| self.extension_enabled(&extension.id()))
            .cloned()
            .collect();
        // Shown even when the settings file cannot be written.
        self.load_icon_theme(&running, id);
        cx.notify();
    }

    pub(super) fn contributions_actions(
        &self,
        root: gpui::Div,
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        root.on_action(
            cx.listener(|this, _: &SelectIconTheme, window, cx| this.choose_icon_theme(window, cx)),
        )
    }
}

/// The Spring Boot language of `application.properties`, `bootstrap.yml`
/// and their profiles (`application-dev.yaml`...).
fn spring_boot_language(path: &Path, builtin: &str) -> Option<&'static str> {
    let name = path.file_name()?.to_str()?;
    if !(name.starts_with("application") || name.starts_with("bootstrap")) {
        return None;
    }
    match builtin {
        "properties" => Some("spring-boot-properties"),
        "yaml" => Some("spring-boot-properties-yaml"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn spring_boot_configuration_files() {
        let cases = [
            (
                "application.properties",
                "properties",
                Some("spring-boot-properties"),
            ),
            (
                "application-dev.yml",
                "yaml",
                Some("spring-boot-properties-yaml"),
            ),
            (
                "bootstrap.yaml",
                "yaml",
                Some("spring-boot-properties-yaml"),
            ),
            ("compose.yaml", "yaml", None),
            ("messages.properties", "properties", None),
            ("application.json", "json", None),
        ];
        for (name, builtin, expected) in cases {
            assert_eq!(
                super::spring_boot_language(Path::new(name), builtin),
                expected,
                "{name}"
            );
        }
    }

    use gpui::TestAppContext;

    use super::*;

    /// An extension with a color theme, an icon theme and a language.
    fn write_extension(data: &Path) {
        let root = crate::paths::extensions_dir(data).join("someone.looks-1.0.0");
        std::fs::create_dir_all(root.join("icons")).unwrap();
        std::fs::write(
            root.join("package.json"),
            r#"{ "name": "looks", "publisher": "someone", "version": "1.0.0",
                 "contributes": {
                    "themes": [{ "label": "Ghost Night", "uiTheme": "vs-dark", "path": "./night.json" }],
                    "iconThemes": [{ "id": "ghost-icons", "label": "Ghost Icons", "path": "./icons.json" }],
                    "languages": [{ "id": "ghostlang", "extensions": [".ghost"], "configuration": "./ghost.json" }]
                 } }"#,
        )
        .unwrap();
        std::fs::write(
            root.join("night.json"),
            r##"{ "colors": { "editor.background": "#101820", "editor.foreground": "#E0E0E0" },
                  "tokenColors": [{ "scope": "keyword", "settings": { "foreground": "#FF0088" } }] }"##,
        )
        .unwrap();
        std::fs::write(
            root.join("icons.json"),
            r#"{ "iconDefinitions": {
                    "file": { "iconPath": "./icons/file.svg" },
                    "ghost": { "iconPath": "./icons/ghost.svg" },
                    "folder": { "iconPath": "./icons/folder.svg" } },
                 "file": "file", "folder": "folder",
                 "languageIds": { "ghostlang": "ghost" } }"#,
        )
        .unwrap();
        std::fs::write(
            root.join("ghost.json"),
            r#"{ "comments": { "lineComment": ";;" } }"#,
        )
        .unwrap();
    }

    #[gpui::test]
    fn extensions_bring_themes_icons_and_languages(cx: &mut TestAppContext) {
        cx.executor().allow_parking();
        cx.update(|cx| {
            gpui_component::init(cx);
            language::register_grammars();
            crate::theme::init(cx);
            init(cx);
        });
        let data = tempfile::tempdir().unwrap();
        write_extension(data.path());
        let project = tempfile::tempdir().unwrap();
        let path = project.path().join("spooky.ghost");
        std::fs::write(&path, "boo\n").unwrap();
        let config = tempfile::tempdir().unwrap();
        let settings = Settings::load(config.path().join("settings.json"), None);
        let mut setup = crate::windows::extension_setup(data.path()).unwrap();
        setup.marketplace = None;
        let document = Document::open(&path).unwrap();
        let (view, cx) = cx.add_window_view(|window, cx| {
            Workbench::new(Some(document), settings, Some(setup), window, cx)
        });

        // The language: the file opened as plain text is now "ghostlang".
        view.read_with(cx, |view, _| {
            let document = view
                .documents
                .values()
                .find(|document| document.uri.ends_with("spooky.ghost"))
                .unwrap();
            assert_eq!(document.language_id, "ghostlang");
            let syntax = Workbench::comment_syntax_for(&view.extension_languages, "ghostlang");
            assert_eq!(syntax.line, Some(";;"));
            let edit = editor::line_edits::toggle_line_comment("boo", 0..0, syntax).unwrap();
            assert_eq!(edit.text, ";; boo");
            assert_eq!(view.language_id_for(Path::new("a.rs")), "rust");
        });

        // The color theme: in the picker, and shown when chosen.
        view.update_in(cx, |view, window, cx| {
            assert!(view.theme_names().contains(&"Ghost Night".to_owned()));
            view.set_theme("Ghost Night", window, cx);
        });
        cx.update(|_, cx| {
            assert!(cx.theme().is_dark());
            assert_eq!(cx.theme().background, gpui::Hsla::from(gpui::rgb(0x101820)));
            assert!(crate::theme::is_dark("Ghost Night", cx));
        });

        // The icon theme: Keel's icons until chosen, then the extension's.
        view.update(cx, |view, cx| {
            assert!(view.themed_icon(&path, false, false, cx).is_none());
            view.set_icon_theme(Some("ghost-icons".into()), cx);
            let icon = view.themed_icon(&path, false, false, cx).unwrap();
            assert!(icon.ends_with("icons/ghost.svg"), "{}", icon.display());
            let folder = view.themed_icon(project.path(), true, false, cx).unwrap();
            assert!(folder.ends_with("icons/folder.svg"));
            view.set_icon_theme(None, cx);
            assert!(view.themed_icon(&path, false, false, cx).is_none());
        });
        let saved = std::fs::read_to_string(config.path().join("settings.json")).unwrap();
        assert!(saved.contains("Ghost Night"), "{saved}");
    }

    #[gpui::test]
    fn peacock_colors_go_to_the_project_and_the_bars(cx: &mut TestAppContext) {
        cx.executor().allow_parking();
        cx.update(|cx| {
            gpui_component::init(cx);
            language::register_grammars();
            crate::theme::init(cx);
            init(cx);
        });
        let data = tempfile::tempdir().unwrap();
        let root = crate::paths::extensions_dir(data.path()).join("keel-tests.peacock-1.0.0");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(
            root.join("package.json"),
            r#"{ "name": "peacock", "publisher": "keel-tests", "version": "1.0.0",
                 "main": "./extension.js", "activationEvents": ["*"] }"#,
        )
        .unwrap();
        // As Peacock does: no target, so the project's settings.
        std::fs::write(
            root.join("extension.js"),
            r##"const vscode = require("vscode");
                exports.activate = async () => {
                  await vscode.workspace.getConfiguration().update("workbench.colorCustomizations", {
                    "titleBar.activeBackground": "#42b883",
                    "titleBar.activeForeground": "#15202b",
                    "statusBar.background": "#42b883",
                  });
                  await vscode.workspace.getConfiguration("peacock").update("color", "#42b883");
                };"##,
        )
        .unwrap();
        let project = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(project.path().join(".git")).unwrap();
        let path = project.path().join("notes.md");
        std::fs::write(&path, "# Notes\n").unwrap();
        let config = tempfile::tempdir().unwrap();
        let settings = Settings::load(config.path().join("settings.json"), Some(project.path()));
        let mut setup = crate::windows::extension_setup(data.path()).unwrap();
        setup.marketplace = None;
        let document = Document::open(&path).unwrap();
        let (view, cx) = cx.add_window_view(|window, cx| {
            Workbench::new(Some(document), settings, Some(setup), window, cx)
        });
        let saved = project.path().join(".vscode/settings.json");
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while !std::fs::read_to_string(&saved).is_ok_and(|text| text.contains("peacock.color")) {
            assert!(
                std::time::Instant::now() < deadline,
                "the project's settings were not written"
            );
            cx.run_until_parked();
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        assert!(
            !config.path().join("settings.json").exists(),
            "the user's settings stay as they were"
        );
        view.read_with(cx, |view, cx| {
            let colors = view.custom_colors(cx);
            assert_eq!(colors.title_background, parse_color(&json!("#42b883")));
            assert_eq!(colors.title_foreground, parse_color(&json!("#15202b")));
            assert_eq!(colors.status_background, parse_color(&json!("#42b883")));
            assert_eq!(colors.status_foreground, None);
        });
    }
}
