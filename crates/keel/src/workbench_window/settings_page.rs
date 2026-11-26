//! How the Settings tab is drawn: Keel's pages on the left, each with its
//! icon, then the pages of the extensions; one search over every setting;
//! and the settings of a page in cards, one row each, with its name, what
//! it does, its control, and a mark when the user changed it.

use gpui::{
    Anchor, Animation, AnimationExt as _, AnyElement, Div, ElementId, ScrollHandle, SharedString,
    StatefulInteractiveElement as _, point,
};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::input::{Input, InputState, NumberInput};
use gpui_component::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_component::switch::Switch;
use gpui_component::{Icon, Sizable as _};
use gpui_kit_assets::IconName;

use super::look::settle;
use super::settings_view::{CATALOG, Field, FieldKind, Page, writer};
use super::shell::Colors;
use super::*;

/// What the Settings tab remembers while it is open: the page in view and
/// where its list was scrolled.
#[derive(Default)]
pub(super) struct SettingsUi {
    pub(super) page: usize,
    pub(super) scroll: ScrollHandle,
    /// The search field, made the first time the tab is drawn.
    search: std::cell::RefCell<Option<Entity<InputState>>>,
}

/// One page of the tab: Keel's own, or an extension's.
struct PageModel {
    icon: IconName,
    title: SharedString,
    description: SharedString,
    sections: Vec<Section>,
    /// An extension's page, listed under the extensions.
    extension: bool,
}

struct Section {
    title: Option<SharedString>,
    description: Option<SharedString>,
    rows: Vec<Row>,
}

/// A row of a card.
enum Row {
    Setting(Field),
    /// The two window styles, drawn small.
    StylePicker,
    /// The themes, drawn small.
    ThemePicker,
    /// Keel's version, with the button that looks for a new one.
    Version,
    /// The notices of the software inside Keel.
    Licenses,
}

impl Row {
    /// The words a search finds the row by.
    fn words(&self, italian: bool) -> Vec<String> {
        let pick = |it: &str, en: &str| if italian { it } else { en }.to_owned();
        match self {
            Row::Setting(field) => vec![
                field.title.clone(),
                field.description.clone(),
                field.key.clone(),
            ],
            Row::StylePicker => vec![
                pick("Stile della finestra", "Window Style"),
                "keel.interfaceStyle floating flat piatto pannelli sospesi card".to_owned(),
            ],
            Row::ThemePicker => vec![
                pick("Tema dei colori", "Color Theme"),
                "workbench.colorTheme theme tema colori colors".to_owned(),
            ],
            Row::Version => vec![
                pick("Versione aggiornamenti", "Version updates"),
                "version update versione aggiornamento".to_owned(),
            ],
            Row::Licenses => vec![
                pick("Licenze di terze parti", "Third-party licenses"),
                "license licence licenza open source".to_owned(),
            ],
        }
    }

    fn matches(&self, words: &[String], page_title: &str, italian: bool) -> bool {
        if words.is_empty() {
            return true;
        }
        let haystack = format!("{} {}", self.words(italian).join(" "), page_title).to_lowercase();
        words.iter().all(|word| haystack.contains(word.as_str()))
    }
}

/// A name in Italian and in English.
type Words = (&'static str, &'static str);

/// Where a setting of a long page goes: its group and what the group is.
fn section_of(key: &str) -> Option<(&'static str, &'static str)> {
    Some(match key {
        "editor.fontSize"
        | "editor.tabSize"
        | "editor.detectIndentation"
        | "editor.insertSpaces"
        | "editor.wordWrap"
        | "editor.cursorSmoothCaretAnimation"
        | "editor.cursorBlinking" => ("Testo e rientri", "Text and Indentation"),
        "editor.formatOnSave" | "editor.defaultFormatter" | "editor.codeActionsOnSave" => {
            ("Formattazione", "Formatting")
        }
        "editor.stickyScroll.enabled"
        | "editor.minimap.enabled"
        | "editor.rulers"
        | "editor.codeLens"
        | "editor.inlayHints.enabled"
        | "editor.glyphMargin"
        | "debug.inlineValues" => ("Accanto al codice", "Beside the Code"),
        "workbench.editor.enablePreview" | "diffEditor.renderSideBySide" => {
            ("Schede e differenze", "Tabs and Differences")
        }
        "files.autoSave"
        | "files.autoSaveDelay"
        | "files.trimTrailingWhitespace"
        | "files.insertFinalNewline" => ("Salvataggio", "Saving"),
        "files.hotExit" | "window.restoreWindows" => ("Finestre", "Windows"),
        super::agent_panel::KEEP_AWAKE_SETTING => ("Claude", "Claude"),
        "explorer.autoReveal"
        | "explorer.compactFolders"
        | "explorer.fileNesting.enabled"
        | "npm.enableScriptExplorer"
        | "files.exclude" => ("Albero dei file", "File Tree"),
        "git.autofetch" | "git.autofetchPeriod" => ("Dal remoto", "From the Remote"),
        "git.enableSmartCommit" | "git.postCommitCommand" | "git.branchProtection" => {
            ("Commit", "Commits")
        }
        _ => return None,
    })
}

/// The icon, the name and what each of Keel's pages holds.
fn page_words(
    page: Page,
) -> (
    IconName,
    (&'static str, &'static str),
    (&'static str, &'static str),
) {
    match page {
        Page::Appearance => (
            IconName::Palette,
            ("Interfaccia", "Interface"),
            (
                "Come appare Keel: lo stile della finestra, i colori, i movimenti e la lingua.",
                "How Keel looks: the window's style, its colors, its movements and its language.",
            ),
        ),
        Page::Editor => (
            IconName::SquareCode,
            ("Editor", "Editor"),
            (
                "Come si scrive: il testo, i rientri, la formattazione e cosa si vede accanto al codice.",
                "How you write: the text, indentation, formatting and what shows beside the code.",
            ),
        ),
        Page::Files => (
            IconName::FolderCog,
            ("File e finestre", "Files and Windows"),
            (
                "Quando si salva, cosa si riapre e come si mostra l'albero dei file.",
                "When files are saved, what opens again and how the file tree shows.",
            ),
        ),
        Page::Terminal => (
            IconName::SquareTerminal,
            ("Terminale", "Terminal"),
            (
                "Il terminale del pannello in basso: il suo testo e la sua tastiera.",
                "The terminal of the bottom panel: its text and its keyboard.",
            ),
        ),
        Page::Git => (
            IconName::GitBranch,
            ("Git", "Git"),
            (
                "I commit, il fetch e quello che Keel ti dice del repository.",
                "Commits, fetching and what Keel tells you about the repository.",
            ),
        ),
        Page::Languages => (
            IconName::Languages,
            ("Linguaggi", "Languages"),
            (
                "I language server, che danno errori, suggerimenti e definizioni.",
                "The language servers behind errors, completions and definitions.",
            ),
        ),
        Page::Extensions => (
            IconName::Blocks,
            ("Estensioni", "Extensions"),
            (
                "Le estensioni di Open VSX e i loro aggiornamenti.",
                "Extensions from Open VSX and their updates.",
            ),
        ),
        Page::Updates => (
            IconName::CircleArrowDown,
            ("Aggiornamenti", "Updates"),
            (
                "La versione di Keel, i suoi aggiornamenti e il software che porta con sé.",
                "Keel's version, its updates and the software it carries.",
            ),
        ),
    }
}

/// Keel's pages, in the order of the list.
pub(super) const KEEL_PAGES: [Page; 8] = [
    Page::Appearance,
    Page::Editor,
    Page::Files,
    Page::Terminal,
    Page::Git,
    Page::Languages,
    Page::Extensions,
    Page::Updates,
];

impl Workbench {
    /// A setting of Keel's catalog as a row; the preferred themes are
    /// chosen from the themes there are, not typed.
    fn catalog_row(&self, key: &str, italian: bool) -> Option<Row> {
        let entry = CATALOG.iter().find(|entry| entry.key == key)?;
        let mut field = Field::from_entry(entry, italian);
        if matches!(
            key,
            "workbench.preferredDarkColorTheme" | "workbench.preferredLightColorTheme"
        ) {
            field.kind = FieldKind::Choice(
                self.theme_names()
                    .into_iter()
                    .map(|name| (SharedString::from(name.clone()), SharedString::from(name)))
                    .collect(),
            );
        }
        Some(Row::Setting(field))
    }

    /// Every page of the tab, Keel's first, then the extensions'.
    fn settings_pages(&self, italian: bool) -> Vec<PageModel> {
        let pick = |pair: (&'static str, &'static str)| -> SharedString {
            if italian { pair.0 } else { pair.1 }.into()
        };
        let section = |title: Option<(&'static str, &'static str)>, rows: Vec<Row>| Section {
            title: title.map(pick),
            description: None,
            rows,
        };
        let mut pages: Vec<PageModel> = KEEL_PAGES
            .into_iter()
            .map(|page| {
                let (icon, title, description) = page_words(page);
                let sections = match page {
                    Page::Appearance => {
                        let rows = |keys: &[&str]| -> Vec<Row> {
                            keys.iter()
                                .filter_map(|key| self.catalog_row(key, italian))
                                .collect()
                        };
                        let mut style = vec![Row::StylePicker];
                        // Corners and spacing are the floating panels' only.
                        if !self.look.flat() {
                            style.extend(rows(&[
                                super::look::RADIUS_SETTING,
                                super::look::GAP_SETTING,
                            ]));
                        }
                        let mut theme = vec![Row::ThemePicker];
                        theme.extend(rows(&[
                            "window.autoDetectColorScheme",
                            "workbench.preferredDarkColorTheme",
                            "workbench.preferredLightColorTheme",
                        ]));
                        vec![
                            Section {
                                title: Some(pick(("Stile della finestra", "Window Style"))),
                                description: Some(pick((
                                    "Lo stesso Keel, disegnato in due modi. Cambia al volo, in ogni finestra.",
                                    "The same Keel, drawn two ways. It changes at once, in every window.",
                                ))),
                                rows: style,
                            },
                            section(Some(("Tema", "Theme")), theme),
                            section(
                                Some(("Movimento", "Motion")),
                                rows(&[super::look::MOTION_SETTING]),
                            ),
                            section(
                                Some(("Generale", "General")),
                                rows(&[
                                    "keel.language",
                                    "breadcrumbs.enabled",
                                    "workbench.editor.wrapTabs",
                                ]),
                            ),
                        ]
                    }
                    Page::Updates => {
                        let mut rows = vec![Row::Version];
                        rows.extend(
                            CATALOG
                                .iter()
                                .filter(|entry| entry.page == Page::Updates)
                                .filter_map(|entry| self.catalog_row(entry.key, italian)),
                        );
                        rows.push(Row::Licenses);
                        vec![section(None, rows)]
                    }
                    _ => {
                        // The settings of the page, in the groups of
                        // `section_of`, in the order they first appear.
                        let mut sections: Vec<(Option<Words>, Vec<Row>)> = Vec::new();
                        for entry in CATALOG.iter().filter(|entry| entry.page == page) {
                            let Some(row) = self.catalog_row(entry.key, italian) else {
                                continue;
                            };
                            let group = section_of(entry.key);
                            match sections.iter_mut().find(|(title, _)| *title == group) {
                                Some((_, rows)) => rows.push(row),
                                None => sections.push((group, vec![row])),
                            }
                        }
                        sections
                            .into_iter()
                            .map(|(title, rows)| section(title, rows))
                            .collect()
                    }
                };
                PageModel {
                    icon,
                    title: pick(title),
                    description: pick(description),
                    sections,
                    extension: false,
                }
            })
            .collect();
        let strings = self.strings();
        for (id, name, sections) in self.extension_setting_pages() {
            pages.push(PageModel {
                icon: IconName::Puzzle,
                title: name.into(),
                description: strings.extension_settings_from(&id).into(),
                sections: sections
                    .into_iter()
                    .map(|section| Section {
                        title: section.title.map(SharedString::from),
                        description: None,
                        rows: section
                            .settings
                            .iter()
                            .map(|setting| {
                                Row::Setting(Field::from_extension(
                                    setting,
                                    self.settings.get(&setting.key),
                                ))
                            })
                            .collect(),
                    })
                    .collect(),
                extension: true,
            });
        }
        pages
    }

    /// Looks for `query` in the Settings tab, as typed in its field.
    #[cfg_attr(not(feature = "snapshots"), allow(dead_code))]
    pub(super) fn search_settings(
        &mut self,
        query: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(search) = self.settings_ui.search.borrow().clone() {
            search.update(cx, |input, cx| {
                input.set_value(query.to_owned(), window, cx)
            });
        }
    }

    /// Shows a page of the tab, from its top.
    pub(super) fn show_settings_page(&mut self, page: usize, cx: &mut Context<Self>) {
        self.settings_ui.page = page;
        self.settings_ui.scroll.set_offset(point(px(0.), px(0.)));
        cx.notify();
    }

    pub(super) fn render_settings_page(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let italian = self.language == UiLanguage::Italian;
        let colors = Colors::new(cx);
        let pages = self.settings_pages(italian);
        let placeholder = if italian {
            "Cerca un'impostazione"
        } else {
            "Search settings"
        };
        let made = self.settings_ui.search.borrow().clone();
        let search = match made {
            Some(search) => {
                if search.read(cx).presentation().placeholder() != placeholder {
                    search.update(cx, |input, cx| {
                        input.set_placeholder(placeholder, window, cx)
                    });
                }
                search
            }
            None => {
                let search = cx.new(|cx| InputState::new(window, cx).placeholder(placeholder));
                *self.settings_ui.search.borrow_mut() = Some(search.clone());
                search
            }
        };
        let query = search.read(cx).value().to_string();
        let words: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
        let searching = !words.is_empty();
        let selected = self.settings_ui.page.min(pages.len().saturating_sub(1));
        // How many rows of each page the search finds.
        let found: Vec<usize> = pages
            .iter()
            .map(|page| {
                page.sections
                    .iter()
                    .flat_map(|section| &section.rows)
                    .filter(|row| row.matches(&words, &page.title, italian))
                    .count()
            })
            .collect();

        let nav =
            self.render_settings_nav(&pages, &found, selected, searching, &search, colors, cx);
        let body = if searching {
            self.render_settings_results(&pages, &words, italian, colors, window, cx)
        } else {
            self.render_settings_body(&pages[selected], selected, italian, colors, window, cx)
        };
        div()
            .size_full()
            .flex()
            .bg(colors.editor)
            .child(nav)
            .child(
                div()
                    .id("settings-content")
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .overflow_y_scroll()
                    .track_scroll(&self.settings_ui.scroll)
                    .child(body),
            )
            .into_any_element()
    }

    /// The list of pages, with the search on top and the file at the bottom.
    #[allow(clippy::too_many_arguments)]
    fn render_settings_nav(
        &self,
        pages: &[PageModel],
        found: &[usize],
        selected: usize,
        searching: bool,
        search: &Entity<InputState>,
        colors: Colors,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let italian = self.language == UiLanguage::Italian;
        let strings = self.strings();
        let changed = |page: &PageModel| {
            page.sections
                .iter()
                .flat_map(|section| &section.rows)
                .filter(|row| match row {
                    Row::Setting(field) => {
                        self.settings.source_of(&field.key) == Some(settings::Source::User)
                    }
                    _ => false,
                })
                .count()
        };
        let item = |index: usize, page: &PageModel, cx: &mut Context<Self>| {
            let active = !searching && index == selected;
            let count = if searching {
                found[index]
            } else {
                changed(page)
            };
            let dim = searching && found[index] == 0;
            let search = search.clone();
            div()
                .id(("settings-nav", index))
                .h(px(30.))
                .px(px(9.))
                .flex()
                .items_center()
                .gap(px(9.))
                .rounded(px(7.))
                .cursor_pointer()
                .text_size(px(13.))
                .text_color(if active { colors.text } else { colors.muted })
                .when(active, |item| item.bg(colors.selected))
                .when(!active, |item| {
                    item.hover(|item| item.bg(colors.hover).text_color(colors.text))
                })
                .when(dim, |item| item.opacity(0.45))
                .on_click(cx.listener(move |this, _, window, cx| {
                    search.update(cx, |input, cx| input.set_value("", window, cx));
                    this.show_settings_page(index, cx);
                }))
                .child(
                    Icon::new(page.icon)
                        .size(px(15.))
                        .flex_none()
                        .text_color(if active { colors.accent } else { colors.muted }),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .child(page.title.clone()),
                )
                .when(count > 0, |item| {
                    item.child(
                        div()
                            .flex_none()
                            .min_w(px(18.))
                            .h(px(18.))
                            .px(px(5.))
                            .rounded_full()
                            .flex()
                            .items_center()
                            .justify_center()
                            .text_size(px(11.))
                            .font_weight(FontWeight::MEDIUM)
                            .when(searching, |pill| {
                                pill.bg(colors.accent.opacity(0.14))
                                    .text_color(colors.accent)
                            })
                            .when(!searching, |pill| {
                                pill.bg(colors.text.opacity(0.07)).text_color(colors.muted)
                            })
                            .child(count.to_string()),
                    )
                })
        };
        let mut list = div()
            .id("settings-nav-list")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .px(px(8.))
            .pb_2()
            .flex()
            .flex_col()
            .gap(px(2.));
        let mut heading_done = false;
        for (index, page) in pages.iter().enumerate() {
            if page.extension && !heading_done {
                heading_done = true;
                list = list.child(
                    div()
                        .pt(px(14.))
                        .pb(px(4.))
                        .px(px(9.))
                        .text_size(px(11.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(colors.faint)
                        .child(if italian {
                            "DELLE ESTENSIONI"
                        } else {
                            "FROM EXTENSIONS"
                        }),
                );
            }
            list = list.child(item(index, page, cx));
        }
        div()
            .w(px(212.))
            .flex_none()
            .h_full()
            .flex()
            .flex_col()
            .border_r_1()
            .border_color(colors.border.opacity(0.7))
            .child(
                div()
                    .px(px(17.))
                    .pt(px(18.))
                    .pb(px(10.))
                    .text_size(px(15.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(colors.text)
                    .child(if italian { "Impostazioni" } else { "Settings" }),
            )
            .child(
                div().px(px(10.)).pb(px(10.)).child(
                    Input::new(search)
                        .small()
                        .cleanable(true)
                        .prefix(Icon::new(IconName::Search).small().text_color(colors.muted)),
                ),
            )
            .child(list)
            .child(
                div()
                    .p(px(8.))
                    .border_t_1()
                    .border_color(colors.border.opacity(0.7))
                    .child(
                        Button::new("open-settings-json")
                            .ghost()
                            .small()
                            .w_full()
                            .icon(IconName::FileBraces)
                            .label(strings.open_settings_json)
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.open_settings_json(window, cx)
                            })),
                    ),
            )
            .into_any_element()
    }

    /// A page: its title and what it holds, then its groups in cards.
    fn render_settings_body(
        &self,
        page: &PageModel,
        index: usize,
        italian: bool,
        colors: Colors,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let mut body = settings_column().child(page_header(page, colors));
        for section in &page.sections {
            body = body.child(self.render_section(section, None, &[], italian, colors, window, cx));
        }
        let duration = self.look.duration(200);
        if duration.is_zero() {
            return body.into_any_element();
        }
        body.with_animation(
            ElementId::Name(SharedString::from(format!("settings-page-{index}"))),
            Animation::new(duration).with_easing(settle),
            |body, delta| body.opacity(delta).top(px(6. * (1. - delta))),
        )
        .into_any_element()
    }

    /// What a search finds, page by page.
    fn render_settings_results(
        &self,
        pages: &[PageModel],
        words: &[String],
        italian: bool,
        colors: Colors,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let mut body = settings_column();
        let mut any = false;
        for page in pages {
            let sections: Vec<&Section> = page
                .sections
                .iter()
                .filter(|section| {
                    section
                        .rows
                        .iter()
                        .any(|row| row.matches(words, &page.title, italian))
                })
                .collect();
            if sections.is_empty() {
                continue;
            }
            any = true;
            body = body.child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .pt(px(4.))
                    .child(Icon::new(page.icon).size(px(16.)).text_color(colors.accent))
                    .child(
                        div()
                            .text_size(px(15.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(colors.text)
                            .child(page.title.clone()),
                    ),
            );
            for section in sections {
                body = body.child(self.render_section(
                    section,
                    Some(&page.title),
                    words,
                    italian,
                    colors,
                    window,
                    cx,
                ));
            }
        }
        if !any {
            return div()
                .size_full()
                .pt(px(96.))
                .flex()
                .flex_col()
                .items_center()
                .gap(px(10.))
                .child(
                    Icon::new(IconName::TextSearch)
                        .size(px(28.))
                        .text_color(colors.faint),
                )
                .child(
                    div()
                        .text_size(px(15.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(colors.text)
                        .child(if italian {
                            "Nessuna impostazione trovata"
                        } else {
                            "No setting found"
                        }),
                )
                .child(
                    div()
                        .max_w(px(340.))
                        .px_4()
                        .text_center()
                        .text_size(px(13.))
                        .line_height(px(19.))
                        .text_color(colors.muted)
                        .child(if italian {
                            "Prova con un'altra parola, o col nome nel file, come editor.fontSize."
                        } else {
                            "Try another word, or the name in the file, such as editor.fontSize."
                        }),
                )
                .into_any_element();
        }
        body.into_any_element()
    }

    /// A group: its title, and its rows in one card.
    #[allow(clippy::too_many_arguments)]
    fn render_section(
        &self,
        section: &Section,
        page_title: Option<&SharedString>,
        words: &[String],
        italian: bool,
        colors: Colors,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let title = page_title.map_or("", |title| title.as_ref());
        let rows: Vec<AnyElement> = section
            .rows
            .iter()
            .filter(|row| row.matches(words, title, italian))
            .enumerate()
            .map(|(index, row)| {
                let element = self.render_row(row, italian, colors, window, cx);
                div()
                    .when(index > 0, |row| {
                        row.border_t_1().border_color(colors.border.opacity(0.55))
                    })
                    .child(element)
                    .into_any_element()
            })
            .collect();
        div()
            .flex()
            .flex_col()
            .gap(px(8.))
            .when(
                section.title.is_some() || section.description.is_some(),
                |group| {
                    group.child(
                        div()
                            .px(px(2.))
                            .flex()
                            .flex_col()
                            .gap(px(2.))
                            .when_some(section.title.clone(), |head, title| {
                                head.child(
                                    div()
                                        .text_size(px(13.))
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_color(colors.text)
                                        .child(title),
                                )
                            })
                            .when_some(section.description.clone(), |head, description| {
                                head.child(
                                    div()
                                        .text_size(px(12.5))
                                        .text_color(colors.muted)
                                        .child(description),
                                )
                            }),
                    )
                },
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .rounded(px(10.))
                    .border_1()
                    .border_color(colors.border.opacity(0.8))
                    .bg(card(colors))
                    .overflow_hidden()
                    .children(rows),
            )
            .into_any_element()
    }

    fn render_row(
        &self,
        row: &Row,
        italian: bool,
        colors: Colors,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let workbench = cx.entity().downgrade();
        let handle = window.window_handle();
        let strings = self.strings();
        match row {
            Row::Setting(field) => self.render_setting(field, italian, colors, window, cx),
            Row::StylePicker => div()
                .p(px(14.))
                .child(super::interface_page::style_picker(
                    self, &workbench, handle, italian, colors,
                ))
                .into_any_element(),
            Row::ThemePicker => div()
                .p(px(14.))
                .child(super::interface_page::theme_picker(
                    self, &workbench, handle, colors, cx,
                ))
                .into_any_element(),
            Row::Version => {
                let checking = self.checking_updates;
                let control = Button::new("check-for-updates")
                    .outline()
                    .small()
                    .loading(checking)
                    .label(if checking {
                        strings.update_checking
                    } else {
                        strings.update_check
                    })
                    .on_click(
                        cx.listener(|this, _, window, cx| this.check_for_updates(true, window, cx)),
                    );
                row_layout(
                    "settings-version",
                    strings.update_version.into(),
                    strings
                        .update_version_detail(crate::version::VERSION, crate::version::BUILD)
                        .into(),
                    false,
                    control.into_any_element(),
                    None,
                    colors,
                )
            }
            Row::Licenses => {
                let control = Button::new("third-party-licenses")
                    .outline()
                    .small()
                    .label(strings.update_licenses_show)
                    .on_click(cx.listener(|this, _, window, cx| this.open_licenses(window, cx)));
                row_layout(
                    "settings-licenses",
                    strings.update_licenses.into(),
                    strings.update_licenses_detail.into(),
                    false,
                    control.into_any_element(),
                    None,
                    colors,
                )
            }
        }
    }

    /// One setting: its name and what it does, its control, and a way back
    /// to the default when the user changed it.
    fn render_setting(
        &self,
        field: &Field,
        italian: bool,
        colors: Colors,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let source = self.settings.source_of(&field.key);
        let changed = source == Some(settings::Source::User);
        let mut description = field.description.clone();
        if matches!(
            source,
            Some(settings::Source::Project | settings::Source::VsCodeProject)
        ) {
            description.push_str(if italian {
                " Il progetto la cambia nelle sue impostazioni."
            } else {
                " The project changes it in its settings."
            });
        }
        let control = self.setting_control(field, italian, window, cx);
        let reset = changed.then(|| {
            let key = field.key.clone();
            Button::new("reset")
                .ghost()
                .xsmall()
                .icon(IconName::RotateCcw)
                .tooltip(if italian {
                    "Torna al valore di Keel"
                } else {
                    "Back to Keel's value"
                })
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.change_setting(&key, None, window, cx)
                }))
                .into_any_element()
        });
        row_layout(
            SharedString::from(format!("setting-{}", field.key)),
            field.title.clone().into(),
            description.into(),
            changed,
            control,
            reset,
            colors,
        )
    }

    /// The control of a setting, reading it from the window's settings
    /// and writing it to the user's file.
    fn setting_control(
        &self,
        field: &Field,
        italian: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let workbench = cx.entity().downgrade();
        let handle = window.window_handle();
        let key: SharedString = field.key.clone().into();
        let value = self
            .settings
            .get(&field.key)
            .cloned()
            .unwrap_or_else(|| field.default.clone());
        let write = writer(&workbench, handle, key.clone());
        let as_text = |value: &Value| match value {
            Value::String(text) => text.clone(),
            Value::Null => String::new(),
            other => other.to_string(),
        };
        match &field.kind {
            FieldKind::Switch => Switch::new("switch")
                .accessibility_label(field.title.clone())
                .checked(value.as_bool().unwrap_or(false))
                .on_click(move |on: &bool, _, cx| write(Value::Bool(*on), cx))
                .into_any_element(),
            FieldKind::Choice(options) => {
                let now = as_text(&value);
                let label = options
                    .iter()
                    .find(|(value, _)| value.as_ref() == now)
                    .map(|(_, label)| label.clone())
                    .unwrap_or_else(|| now.clone().into());
                let options = options.clone();
                let scrollable = options.len() > 12;
                Button::new("choice")
                    .outline()
                    .small()
                    .min_w(px(148.))
                    .accessibility_label(format!("{}: {label}", field.title))
                    .label(label)
                    .dropdown_caret(true)
                    .dropdown_menu_with_anchor(Anchor::TopRight, move |menu, _, _| {
                        let menu = options.iter().fold(menu, |menu, (value, label)| {
                            let write = write.clone();
                            let value = value.clone();
                            menu.item(
                                PopupMenuItem::new(label.clone())
                                    .checked(value.as_ref() == now)
                                    .on_click(move |_, _, cx| {
                                        write(Value::String(value.to_string()), cx)
                                    }),
                            )
                        });
                        menu.scrollable(scrollable)
                    })
                    .into_any_element()
            }
            FieldKind::Number { min, max, step } => {
                let (min, max, step) = (*min, *max, *step);
                let now = value.as_f64().unwrap_or(0.);
                let state = window.use_keyed_state(
                    SharedString::from(format!("settings-number-{key}")),
                    cx,
                    |window, cx| {
                        let input = cx.new(|cx| {
                            InputState::new(window, cx)
                                .default_value(now.to_string())
                                .step(step)
                                .min(min)
                                .max(max)
                        });
                        let subscription = cx.subscribe(&input, {
                            let write = write.clone();
                            move |state: &mut NumberState, input, event: &InputEvent, cx| {
                                if !matches!(event, InputEvent::Change) {
                                    return;
                                }
                                let text = input.read(cx).value().to_string();
                                // Halfway values ("-", "1.") wait for the next key.
                                let Ok(parsed) = text.parse::<f64>() else {
                                    return;
                                };
                                let clamped = parsed.clamp(min, max);
                                if clamped == state.value {
                                    return;
                                }
                                state.value = clamped;
                                // Whole numbers stay whole in the file.
                                write(
                                    if clamped.fract() == 0. {
                                        json!(clamped as i64)
                                    } else {
                                        json!(clamped)
                                    },
                                    cx,
                                );
                            }
                        });
                        NumberState {
                            input,
                            value: now,
                            _subscription: subscription,
                        }
                    },
                );
                // A value changed elsewhere, in the file or by a reset.
                let input = state.update(cx, |state, cx| {
                    if state.value != now {
                        state.value = now;
                        state
                            .input
                            .update(cx, |input, cx| input.set_value(now.to_string(), window, cx));
                    }
                    state.input.clone()
                });
                NumberInput::new(&input)
                    .small()
                    .w(px(124.))
                    .into_any_element()
            }
            FieldKind::Text => {
                let now = as_text(&value);
                let state = window.use_keyed_state(
                    SharedString::from(format!("settings-text-{key}")),
                    cx,
                    |window, cx| {
                        let input =
                            cx.new(|cx| InputState::new(window, cx).default_value(now.clone()));
                        let subscription = cx.subscribe(&input, {
                            let write = write.clone();
                            move |state: &mut TextState, input, event: &InputEvent, cx| {
                                if !matches!(event, InputEvent::Change) {
                                    return;
                                }
                                let text = input.read(cx).value().to_string();
                                if text == state.value {
                                    return;
                                }
                                state.value = text.clone();
                                write(Value::String(text), cx);
                            }
                        });
                        TextState {
                            input,
                            value: now.clone(),
                            _subscription: subscription,
                        }
                    },
                );
                let input = state.update(cx, |state, cx| {
                    if state.value != now {
                        state.value = now.clone();
                        state
                            .input
                            .update(cx, |input, cx| input.set_value(now.clone(), window, cx));
                    }
                    state.input.clone()
                });
                Input::new(&input).small().w(px(220.)).into_any_element()
            }
            FieldKind::Json => Button::new("json")
                .outline()
                .small()
                .icon(IconName::FileBraces)
                .label(if italian {
                    "Modifica in settings.json"
                } else {
                    "Edit in settings.json"
                })
                .on_click(cx.listener(|this, _, window, cx| this.open_settings_json(window, cx)))
                .into_any_element(),
        }
    }
}

/// A number field's input, and the value last written.
struct NumberState {
    input: Entity<InputState>,
    value: f64,
    _subscription: Subscription,
}

/// A text field's input, and the text last written.
struct TextState {
    input: Entity<InputState>,
    value: String,
    _subscription: Subscription,
}

/// The column the pages are drawn in, as wide as reads well.
fn settings_column() -> Div {
    div()
        .relative()
        .w_full()
        .max_w(px(780.))
        .px(px(32.))
        .pt(px(26.))
        .pb(px(48.))
        .flex()
        .flex_col()
        .gap(px(26.))
}

/// The page's name, large, and what it holds.
fn page_header(page: &PageModel, colors: Colors) -> Div {
    div()
        .flex()
        .flex_col()
        .gap(px(6.))
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(10.))
                .child(Icon::new(page.icon).size(px(20.)).text_color(colors.accent))
                .child(
                    div()
                        .text_size(px(22.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(colors.text)
                        .child(page.title.clone()),
                ),
        )
        .child(
            div()
                .text_size(px(13.))
                .line_height(px(19.))
                .text_color(colors.muted)
                .child(page.description.clone()),
        )
}

/// The cards' background: a shade off the editor's.
fn card(colors: Colors) -> Hsla {
    if colors.dark {
        gpui::white().opacity(0.028)
    } else {
        gpui::black().opacity(0.018)
    }
}

/// A row: name and description on the left, the control on the right, a
/// bar on the left edge when the user changed it, as VS Code marks them.
fn row_layout(
    id: impl Into<SharedString>,
    title: SharedString,
    description: SharedString,
    changed: bool,
    control: AnyElement,
    reset: Option<AnyElement>,
    colors: Colors,
) -> AnyElement {
    div()
        .id(ElementId::Name(id.into()))
        .relative()
        .px(px(16.))
        .py(px(12.))
        .flex()
        .items_center()
        .gap(px(16.))
        .when(changed, |row| {
            row.child(
                div()
                    .absolute()
                    .left_0()
                    .top(px(10.))
                    .bottom(px(10.))
                    .w(px(3.))
                    .rounded_r(px(2.))
                    .bg(colors.accent),
            )
        })
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .gap(px(3.))
                .child(
                    div()
                        .text_size(px(13.5))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(colors.text)
                        .child(title),
                )
                .when(!description.is_empty(), |text| {
                    text.child(
                        div()
                            .text_size(px(12.5))
                            .line_height(px(18.))
                            .text_color(colors.muted)
                            .child(description),
                    )
                }),
        )
        .child(
            div()
                .flex_none()
                .flex()
                .items_center()
                .gap(px(6.))
                .children(reset)
                .child(control),
        )
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use gpui::{TestAppContext, VisualTestContext};

    use super::*;

    fn window(
        cx: &mut TestAppContext,
    ) -> (tempfile::TempDir, Entity<Workbench>, &mut VisualTestContext) {
        cx.update(|cx| {
            gpui_component::init(cx);
            language::register_grammars();
            crate::workbench_window::init(cx);
        });
        let folder = tempfile::tempdir().unwrap();
        let data = std::fs::canonicalize(folder.path()).unwrap();
        let settings = Settings::load(crate::paths::user_settings_file(&data), None);
        let opening = Opening::new(None, None).with_data_dir(data);
        let (view, cx) =
            cx.add_window_view(|window, cx| Workbench::new(opening, settings, None, window, cx));
        (folder, view, cx)
    }

    fn keys(pages: &[PageModel]) -> Vec<String> {
        pages
            .iter()
            .flat_map(|page| &page.sections)
            .flat_map(|section| &section.rows)
            .filter_map(|row| match row {
                Row::Setting(field) => Some(field.key.clone()),
                _ => None,
            })
            .collect()
    }

    #[gpui::test]
    fn every_setting_of_the_catalog_has_its_place(cx: &mut TestAppContext) {
        let (_folder, view, cx) = window(cx);
        let shown = view.read_with(cx, |view, _| keys(&view.settings_pages(true)));
        for entry in CATALOG {
            // The style and the theme are chosen from their pictures.
            if matches!(
                entry.key,
                "workbench.colorTheme" | super::super::look::STYLE_SETTING
            ) {
                continue;
            }
            assert!(
                shown.iter().any(|key| key == entry.key),
                "{} is missing",
                entry.key
            );
        }
        // The preferred themes are chosen from a list, not typed.
        view.read_with(cx, |view, _| {
            let pages = view.settings_pages(true);
            let dark = pages
                .iter()
                .flat_map(|page| &page.sections)
                .flat_map(|section| &section.rows)
                .find_map(|row| match row {
                    Row::Setting(field) if field.key == "workbench.preferredDarkColorTheme" => {
                        Some(field)
                    }
                    _ => None,
                })
                .unwrap();
            assert!(matches!(&dark.kind, FieldKind::Choice(themes) if !themes.is_empty()));
        });
    }

    #[gpui::test]
    fn a_search_finds_settings_by_name_words_or_key(cx: &mut TestAppContext) {
        let (_folder, view, cx) = window(cx);
        view.read_with(cx, |view, _| {
            let pages = view.settings_pages(true);
            let found = |query: &str| -> Vec<String> {
                let words: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
                pages
                    .iter()
                    .flat_map(|page| {
                        page.sections
                            .iter()
                            .flat_map(|section| &section.rows)
                            .filter(|row| row.matches(&words, &page.title, true))
                            .filter_map(|row| match row {
                                Row::Setting(field) => Some(field.key.clone()),
                                _ => None,
                            })
                            .collect::<Vec<_>>()
                    })
                    .collect()
            };
            assert!(found("minimappa").contains(&"editor.minimap.enabled".to_owned()));
            assert!(found("editor.fontSize").contains(&"editor.fontSize".to_owned()));
            // Every word counts: the terminal's text size, not the editor's.
            assert_eq!(
                found("terminale dimensione"),
                ["terminal.integrated.fontSize"]
            );
            assert!(found("zzzz").is_empty());
        });
    }
}
