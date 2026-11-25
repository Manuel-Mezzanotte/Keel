//! The Extensions view, as in VS Code and Cursor: the installed extensions
//! with their icon and description, turned on or off (everywhere or for
//! this project), uninstalled, installed from a VSIX, and a tab with each
//! one's details and README. Any change restarts the extension host in
//! place: the window stays open.

use std::collections::HashSet;

use extensions::marketplace::{self, Listing};
use extensions::{color_theme, cursor, icon_theme};
use gpui::{AnyElement, Entity, PathPromptOptions, StatefulInteractiveElement as _, img};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::input::{Input, InputEvent, InputState};
use gpui_component::menu::DropdownMenu as _;
use gpui_component::text::{TextView, TextViewState};
use gpui_component::{ActiveTheme as _, Disableable as _, Icon, Sizable as _};
use gpui_kit_assets::IconName;

use super::shell::{
    Colors, chevron_turn, empty_state, heading, note, section_header, tool_row, view_header,
};
use super::*;
use crate::extension_state::Scope;

actions!(
    keel,
    [
        InstallFromVsix,
        RestartExtensions,
        CheckExtensionUpdates,
        UpdateAllExtensions
    ]
);

/// How long typing pauses before Open VSX is asked.
const SEARCH_PAUSE: Duration = Duration::from_millis(300);
/// How many results a search shows.
const SEARCH_SIZE: usize = 30;

/// The Open VSX search at the top of the Extensions view, and the updates
/// found for the installed extensions.
pub(super) struct ExtensionStore {
    /// None where extensions are off, and in most tests.
    pub(super) client: Option<marketplace::Marketplace>,
    input: Entity<InputState>,
    /// The text last searched.
    query: String,
    results: Vec<Listing>,
    searching: bool,
    error: Option<String>,
    /// Bumped by every search, so a late answer is dropped.
    generation: u64,
    /// Lower-case ids being downloaded and installed.
    pub(super) installing: HashSet<String>,
    /// Newer versions, by lower-case id.
    pub(super) updates: HashMap<String, Listing>,
    pub(super) checked_updates: bool,
    /// Icons of the results, by lower-case id.
    icons: HashMap<String, PathBuf>,
    /// "Import from Cursor", while it is shown.
    pub(super) cursor: Option<super::import_extensions::CursorImport>,
    /// Cursor's extensions folder; none means `~/.cursor/extensions`.
    pub(super) cursor_dir: Option<PathBuf>,
    /// What keeps each installed extension from working fully in Keel,
    /// by lower-case id; worked out in the background.
    pub(super) limits: HashMap<String, Vec<cursor::Blocker>>,
    limits_task: Task<()>,
    /// What the project's `.vscode/extensions.json` recommends.
    pub(super) recommended: Vec<String>,
    /// The lists closed to their header.
    installed_collapsed: bool,
    recommended_collapsed: bool,
}

impl ExtensionStore {
    pub(super) fn new(
        strings: &'static Strings,
        window: &mut Window,
        cx: &mut Context<Workbench>,
    ) -> (Self, Subscription) {
        let placeholder = strings.extensions_search_placeholder;
        let input = cx.new(|cx| InputState::new(window, cx).placeholder(placeholder));
        let events = cx.subscribe_in(&input, window, |this, _, event: &InputEvent, window, cx| {
            if let InputEvent::Change = event {
                this.search_extensions(window, cx);
            }
        });
        let store = Self {
            client: None,
            input,
            query: String::new(),
            results: Vec::new(),
            searching: false,
            error: None,
            generation: 0,
            installing: HashSet::new(),
            updates: HashMap::new(),
            checked_updates: false,
            icons: HashMap::new(),
            cursor: None,
            cursor_dir: None,
            limits: HashMap::new(),
            limits_task: Task::ready(()),
            recommended: Vec::new(),
            installed_collapsed: false,
            recommended_collapsed: false,
        };
        (store, events)
    }
}

fn key(id: &str) -> String {
    id.to_lowercase()
}

/// "12.345" or "1,2 mln": how many times a result was downloaded.
fn downloads(count: u64) -> String {
    if count >= 1_000_000 {
        format!("{:.1} M", count as f64 / 1_000_000.0)
    } else if count >= 1_000 {
        format!("{:.1} k", count as f64 / 1_000.0)
    } else {
        count.to_string()
    }
}

/// What the menu of an installed extension does.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ExtensionCommand {
    Enable,
    Disable,
    DisableInProject,
    /// The extension's page in the Settings tab.
    Settings,
    /// Choose one of its color themes, or of its file icon themes.
    ColorTheme,
    IconTheme,
    Uninstall,
}

#[derive(Clone, Debug, PartialEq, gpui::Action)]
#[action(namespace = extensions, no_json)]
pub struct ExtensionMenu {
    pub id: String,
    pub command: ExtensionCommand,
}

/// Which themes of an extension to offer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ThemeKind {
    Color,
    Icon,
}

/// The tab with an extension's details.
pub(super) struct ExtensionDetailsTab {
    pub(super) id: String,
    pub(super) title: String,
    readme: Entity<TextViewState>,
}

pub(super) fn details_uri(id: &str) -> String {
    format!("keel-extension:{id}")
}

/// What an extension calls itself.
pub(super) fn display_name(extension: &InstalledExtension) -> String {
    extension
        .manifest
        .display_name
        .clone()
        .unwrap_or_else(|| extension.manifest.name.clone())
}

fn description(extension: &InstalledExtension) -> String {
    extension.manifest.raw()["description"]
        .as_str()
        .unwrap_or_default()
        .to_owned()
}

/// The extension's icon from its `package.json`, if it has one.
fn icon_path(extension: &InstalledExtension) -> Option<PathBuf> {
    let icon = extension.manifest.raw()["icon"].as_str()?;
    let path = extension.root.join(icon);
    path.is_file().then_some(path)
}

/// The README shipped in the package, whatever its case.
fn readme(extension: &InstalledExtension) -> String {
    std::fs::read_dir(&extension.root)
        .ok()
        .into_iter()
        .flatten()
        .flatten()
        .find(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .eq_ignore_ascii_case("README.md")
        })
        .and_then(|entry| std::fs::read_to_string(entry.path()).ok())
        .unwrap_or_default()
}

/// The size of the extensions' icons in the lists.
const ICON_SIZE: f32 = 36.;

/// The square shown for an extension without an icon.
fn placeholder_icon(colors: Colors) -> AnyElement {
    div()
        .size(px(ICON_SIZE))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(9.))
        .bg(colors.hover)
        .child(Icon::new(IconName::Blocks).text_color(colors.muted))
        .into_any_element()
}

/// The number next to a list's title.
fn count_badge(count: usize, colors: Colors) -> gpui::Div {
    div()
        .min_w(px(18.))
        .h(px(16.))
        .px_1()
        .flex()
        .items_center()
        .justify_center()
        .rounded_full()
        .bg(colors.hover)
        .text_color(colors.muted)
        .child(count.to_string())
}

/// A small label in a capsule of its own color, such as "Off".
fn pill(text: &str, color: Hsla) -> gpui::Div {
    div()
        .px(px(5.))
        .rounded(px(4.))
        .bg(color.opacity(0.14))
        .text_color(color)
        .truncate()
        .child(text.to_owned())
}

/// An extension's icon from a file, with the corners of the placeholder.
fn icon_image(path: PathBuf) -> AnyElement {
    img(path)
        .size(px(ICON_SIZE))
        .flex_none()
        .rounded(px(9.))
        .into_any_element()
}

/// The extensions a project asks for in `.vscode/extensions.json`, as VS
/// Code reads them: `recommendations` less `unwantedRecommendations`, by
/// lower-case id.
pub(super) fn recommendations(root: &Path) -> Vec<String> {
    let Ok(text) = std::fs::read_to_string(root.join(".vscode/extensions.json")) else {
        return Vec::new();
    };
    let Ok(file) = settings::jsonc::parse(&text) else {
        return Vec::new();
    };
    let ids = |key: &str| -> Vec<String> {
        file[key]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .map(key_of)
            .filter(|id| id.contains('.'))
            .collect()
    };
    let unwanted = ids("unwantedRecommendations");
    let mut wanted = Vec::new();
    for id in ids("recommendations") {
        if !unwanted.contains(&id) && !wanted.contains(&id) {
            wanted.push(id);
        }
    }
    wanted
}

fn key_of(id: &str) -> String {
    id.trim().to_lowercase()
}

/// A line about what an extension adds: commands, themes, snippets...
fn contributions(extension: &InstalledExtension, strings: &Strings) -> String {
    let contributes = &extension.manifest.raw()["contributes"];
    let count = |key: &str| contributes[key].as_array().map_or(0, Vec::len);
    let mut parts = Vec::new();
    for (key, label) in [
        ("commands", strings.extension_commands),
        ("keybindings", strings.extension_keybindings),
        ("themes", strings.extension_themes),
        ("iconThemes", strings.extension_icon_themes),
        ("snippets", strings.extension_snippets),
        ("languages", strings.extension_languages),
    ] {
        let found = count(key);
        if found > 0 {
            parts.push(format!("{found} {label}"));
        }
    }
    parts.join(" · ")
}

impl Workbench {
    /// Works out, in the background, what each installed extension needs
    /// that Keel lacks: their main scripts can be several megabytes.
    pub(super) fn check_extension_limits(&mut self, cx: &mut Context<Self>) {
        let extensions = self.installed.clone();
        let found = cx.background_spawn(async move {
            extensions
                .iter()
                .map(|extension| (extension.id().to_lowercase(), cursor::blockers(extension)))
                .collect::<HashMap<_, _>>()
        });
        self.store.limits_task = cx.spawn(async move |this, cx| {
            let limits = found.await;
            this.update(cx, |this, cx| {
                this.store.limits = limits;
                cx.notify();
            })
            .ok();
        });
    }

    /// How well an extension works in Keel: `None` when fully, otherwise
    /// whether it works at all and why not.
    fn extension_limits(&self, id: &str) -> Option<(bool, String)> {
        let blockers = self.store.limits.get(&id.to_lowercase())?;
        if blockers.is_empty() {
            return None;
        }
        let strings = self.strings();
        let blocking: Vec<cursor::Blocker> = blockers
            .iter()
            .copied()
            .filter(|blocker| blocker.blocks())
            .collect();
        Some(if blocking.is_empty() {
            (
                true,
                format!(
                    "{}. {}.",
                    strings.extension_partly_working,
                    strings.cursor_partial(&super::import_extensions::reasons(blockers, strings))
                ),
            )
        } else {
            (
                false,
                strings.extension_not_working_detail(&super::import_extensions::reasons(
                    &blocking, strings,
                )),
            )
        })
    }

    /// Whether `id` runs in this window.
    pub(super) fn extension_enabled(&self, id: &str) -> bool {
        self.extension_state
            .is_enabled(id, self.project.root.as_deref())
    }

    /// Starts the extension host again, with the extensions installed and
    /// turned on now. Open files and editors are told to it again.
    pub(super) fn restart_extensions(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(data) = self.extension_data.clone() else {
            return;
        };
        // Dropping the host stops it, even with an extension stuck.
        self.extensions = None;
        self.clear_extension_diagnostics(None, cx);
        for pane in &mut self.panes {
            for collection in pane.decorations.values() {
                collection.set(Vec::new(), cx);
            }
            pane.decorations.clear();
        }
        match crate::windows::extension_setup(&data) {
            Some(setup) => self.start_extensions(setup, window, cx),
            None => {
                self.installed = crate::paths::installed_extensions(&data);
                self.check_extension_limits(cx);
                self.report(self.strings().extensions_failed.to_owned());
            }
        }
        for index in 0..self.panes.len() {
            self.install_completions(index, cx);
        }
        self.schedule_gutter_refresh(window, cx);
        cx.notify();
    }

    /// Asks for a `.vsix` and installs it.
    fn install_from_vsix(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let chosen = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some(self.strings().extensions_install_vsix.into()),
        });
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(paths))) = chosen.await else {
                return;
            };
            let Some(path) = paths.into_iter().next() else {
                return;
            };
            this.update_in(cx, |this, window, cx| this.install_vsix(&path, window, cx))
                .ok();
        })
        .detach();
    }

    /// Installs a package, puts away older versions of it, and restarts
    /// the extensions.
    pub(super) fn install_vsix(
        &mut self,
        path: &Path,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(data) = self.extension_data.clone() else {
            return;
        };
        let known: Vec<String> = self
            .installed
            .iter()
            .map(|extension| extension.id().to_lowercase())
            .collect();
        match crate::paths::install_extension(&data, path) {
            Ok(installed) => {
                let id = installed.id();
                // New, rather than a new version, and not one of many
                // coming from Cursor: its themes are offered at once.
                let offer = !known.contains(&id.to_lowercase()) && self.store.cursor.is_none();
                for old in std::fs::read_dir(crate::paths::extensions_dir(&data))
                    .into_iter()
                    .flatten()
                    .flatten()
                {
                    let folder = old.path();
                    let older = folder != installed.root
                        && InstalledExtension::load(&folder)
                            .is_ok_and(|other| other.id().eq_ignore_ascii_case(&id));
                    if older {
                        let _ = files::trash(&folder);
                    }
                }
                self.status_message = Some(
                    self.strings()
                        .extension_installed(&display_name(&installed)),
                );
                self.restart_extensions(window, cx);
                if offer {
                    self.offer_new_themes(&id, window, cx);
                }
            }
            Err(error) => self.report(format!("{error:#}")),
        }
        cx.notify();
    }

    pub(super) fn run_extension_menu(
        &mut self,
        action: &ExtensionMenu,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match action.command {
            ExtensionCommand::Settings => {
                self.open_extension_settings(&action.id, window, cx);
                return;
            }
            ExtensionCommand::ColorTheme => {
                self.offer_extension_themes(&action.id, ThemeKind::Color, false, window, cx);
                return;
            }
            ExtensionCommand::IconTheme => {
                self.offer_extension_themes(&action.id, ThemeKind::Icon, false, window, cx);
                return;
            }
            _ => {}
        }
        let Some(data) = self.extension_data.clone() else {
            return;
        };
        let Some(extension) = self
            .installed
            .iter()
            .find(|extension| extension.id().eq_ignore_ascii_case(&action.id))
            .cloned()
        else {
            return;
        };
        let project = self.project.root.clone();
        match action.command {
            ExtensionCommand::Settings
            | ExtensionCommand::ColorTheme
            | ExtensionCommand::IconTheme => {}
            ExtensionCommand::Enable
            | ExtensionCommand::Disable
            | ExtensionCommand::DisableInProject => {
                let (enabled, scope) = match action.command {
                    ExtensionCommand::Enable => {
                        (true, project.map_or(Scope::Everywhere, Scope::Project))
                    }
                    ExtensionCommand::Disable => (false, Scope::Everywhere),
                    _ => match project {
                        Some(project) => (false, Scope::Project(project)),
                        None => return,
                    },
                };
                self.extension_state.set(&action.id, enabled, &scope);
                if let Err(error) = self.extension_state.save(&data) {
                    self.report(error.to_string());
                }
                self.restart_extensions(window, cx);
            }
            ExtensionCommand::Uninstall => {
                let strings = self.strings();
                let answer = window.prompt(
                    PromptLevel::Warning,
                    &strings.extension_uninstall_question(&display_name(&extension)),
                    Some(strings.trash_detail),
                    &[strings.extension_uninstall, strings.cancel],
                    cx,
                );
                cx.spawn_in(window, async move |this, cx| {
                    if answer.await != Ok(0) {
                        return;
                    }
                    this.update_in(cx, |this, window, cx| {
                        // To the Trash, so it can come back.
                        if let Err(error) = files::trash(&extension.root) {
                            this.report(error.to_string());
                            return;
                        }
                        this.extension_state.forget(&extension.id());
                        let _ = this.extension_state.save(&data);
                        let uri = details_uri(&extension.id());
                        this.close_document_tabs(&uri, window, cx);
                        this.restart_extensions(window, cx);
                    })
                    .ok();
                })
                .detach();
            }
        }
    }

    /// Offers the themes of an extension just installed, as VS Code does:
    /// its color themes, then its file icon themes.
    fn offer_new_themes(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(extension) = self
            .installed
            .iter()
            .find(|extension| extension.id().eq_ignore_ascii_case(id))
        else {
            return;
        };
        let colors = !color_theme::color_themes(extension).is_empty();
        let icons = !icon_theme::icon_themes(extension).is_empty();
        if colors {
            self.offer_extension_themes(id, ThemeKind::Color, icons, window, cx);
        } else if icons {
            self.offer_extension_themes(id, ThemeKind::Icon, false, window, cx);
        }
    }

    /// A list of an extension's color or icon themes: the one chosen is
    /// used at once; "Keep as Is", or Escape, changes nothing. With
    /// `then_icons`, its icon themes are offered next.
    pub(super) fn offer_extension_themes(
        &mut self,
        id: &str,
        kind: ThemeKind,
        then_icons: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(extension) = self
            .installed
            .iter()
            .find(|extension| extension.id().eq_ignore_ascii_case(id))
        else {
            return;
        };
        let strings = self.strings();
        // What is saved, and what the list shows.
        let (title, themes): (&str, Vec<(String, String)>) = match kind {
            ThemeKind::Color => (
                strings.choose_theme,
                color_theme::color_themes(extension)
                    .into_iter()
                    .map(|theme| (theme.label.clone(), theme.label))
                    .collect(),
            ),
            ThemeKind::Icon => (
                strings.choose_icon_theme,
                icon_theme::icon_themes(extension)
                    .into_iter()
                    .map(|theme| (theme.id, theme.label))
                    .collect(),
            ),
        };
        if themes.is_empty() {
            return;
        }
        let mut items: Vec<PickItem> = themes
            .iter()
            .map(|(_, label)| PickItem::new(label.clone()))
            .collect();
        items.push(PickItem::new(strings.keep_as_is));
        let chosen = quick_pick::show(Some(title.to_owned()), None, items, window, cx);
        let id = id.to_owned();
        cx.spawn_in(window, async move |this, cx| {
            let index = chosen.await.ok().flatten();
            this.update_in(cx, |this, window, cx| {
                if let Some((value, _)) = index.and_then(|index| themes.get(index)) {
                    match kind {
                        ThemeKind::Color => this.set_theme(value, window, cx),
                        ThemeKind::Icon => this.set_icon_theme(Some(value.clone()), cx),
                    }
                }
                if then_icons {
                    this.offer_extension_themes(&id, ThemeKind::Icon, false, window, cx);
                }
            })
            .ok();
        })
        .detach();
    }

    /// Opens the tab with an extension's details and README.
    pub(super) fn open_extension_details(
        &mut self,
        id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let uri = details_uri(id);
        if !self.documents.contains_key(&uri) {
            let Some(extension) = self.installed.iter().find(|extension| extension.id() == id)
            else {
                return;
            };
            let text = readme(extension);
            let readme = cx.new(|cx| {
                TextViewState::markdown(&text, cx)
                    .scrollable(true)
                    .selectable(true)
            });
            self.documents.insert(
                uri.clone(),
                OpenDocument {
                    uri: uri.clone(),
                    backing: Backing::ExtensionDetails(Box::new(ExtensionDetailsTab {
                        id: id.to_owned(),
                        title: self.strings().extension_tab_title(&display_name(extension)),
                        readme,
                    })),
                    language_id: "extension".to_owned(),
                    version: 1,
                    synced_text: String::new(),
                    modified: false,
                },
            );
        }
        let column = self.active.min(self.panes.len().saturating_sub(1));
        self.show_tab(&uri, column, OpenAs::Preview, true, window, cx);
    }

    // -----------------------------------------------------------------
    // Open VSX

    /// Searches Open VSX for the text in the search box, after a pause in
    /// typing. An empty box shows the installed extensions again.
    fn search_extensions(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let query = self.store.input.read(cx).value().trim().to_owned();
        self.store.generation += 1;
        let generation = self.store.generation;
        self.store.query = query.clone();
        self.store.error = None;
        let Some(client) = self.store.client.clone().filter(|_| !query.is_empty()) else {
            self.store.results.clear();
            self.store.searching = false;
            cx.notify();
            return;
        };
        self.store.searching = true;
        cx.notify();
        let icons = self
            .extension_data
            .as_deref()
            .map(|data| crate::paths::open_vsx_cache_dir(data).join("icons"));
        cx.spawn_in(window, async move |this, cx| {
            cx.background_executor().timer(SEARCH_PAUSE).await;
            let current = |this: &Workbench| this.store.generation == generation;
            if !this.read_with(cx, |this, _| current(this)).unwrap_or(false) {
                return;
            }
            let search = client.clone();
            let found = cx
                .background_spawn(async move { search.search(&query, SEARCH_SIZE) })
                .await;
            let listings = this
                .update(cx, |this, cx| {
                    if !current(this) {
                        return Vec::new();
                    }
                    this.store.searching = false;
                    match found {
                        Ok(found) => this.store.results = found,
                        Err(error) => {
                            this.store.results.clear();
                            this.store.error = Some(format!("{error:#}"));
                        }
                    }
                    cx.notify();
                    this.store
                        .results
                        .iter()
                        .filter(|listing| !this.store.icons.contains_key(&key(&listing.id())))
                        .cloned()
                        .collect()
                })
                .unwrap_or_default();
            let Some(icons) = icons else { return };
            for listing in listings {
                let (client, folder) = (client.clone(), icons.clone());
                let icon = cx
                    .background_spawn(async move {
                        client
                            .icon(&listing, &folder)
                            .map(|path| (listing.id(), path))
                    })
                    .await;
                if let Ok((id, path)) = icon {
                    let shown = this.update(cx, |this, cx| {
                        this.store.icons.insert(key(&id), path);
                        cx.notify();
                    });
                    if shown.is_err() {
                        return;
                    }
                }
            }
        })
        .detach();
    }

    /// Downloads the latest version of `id` for this Mac and installs it,
    /// in place of the version already there.
    pub(super) fn install_from_open_vsx(
        &mut self,
        id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (Some(client), Some(data)) = (self.store.client.clone(), self.extension_data.clone())
        else {
            return;
        };
        if !self.store.installing.insert(key(id)) {
            return;
        }
        cx.notify();
        let folder = crate::paths::open_vsx_cache_dir(&data).join("downloads");
        let wanted = id.to_owned();
        let download = cx.background_spawn(async move {
            let listing = client.latest(&wanted)?;
            client.download(&listing, &folder)
        });
        let id = id.to_owned();
        cx.spawn_in(window, async move |this, cx| {
            let package = download.await;
            this.update_in(cx, |this, window, cx| {
                this.store.installing.remove(&key(&id));
                match package {
                    Ok(path) => {
                        this.store.updates.remove(&key(&id));
                        this.install_vsix(&path, window, cx);
                        // A single file of Keel's own cache.
                        let _ = std::fs::remove_file(&path);
                    }
                    Err(error) => this.report(format!("{id}: {error:#}")),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Asks Open VSX for newer versions of the installed extensions.
    /// `told` says the result in the status bar, for the command.
    pub(super) fn check_extension_updates(
        &mut self,
        told: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(client) = self.store.client.clone() else {
            return;
        };
        self.store.checked_updates = true;
        let installed: Vec<(String, String)> = self
            .installed
            .iter()
            .map(|extension| (extension.id(), extension.manifest.version.clone()))
            .collect();
        let check = cx.background_spawn(async move {
            installed
                .into_iter()
                .filter_map(|(id, version)| {
                    // Extensions not on Open VSX, such as local ones, are skipped.
                    let latest = client.latest(&id).ok()?;
                    marketplace::is_newer(&latest.version, &version).then(|| (key(&id), latest))
                })
                .collect::<HashMap<_, _>>()
        });
        cx.spawn_in(window, async move |this, cx| {
            let updates = check.await;
            this.update(cx, |this, cx| {
                if told {
                    let strings = this.strings();
                    this.status_message = Some(if updates.is_empty() {
                        strings.extensions_up_to_date.to_owned()
                    } else {
                        strings.extension_updates_found(updates.len())
                    });
                }
                this.store.updates = updates;
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn update_all_extensions(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let ids: Vec<String> = self.store.updates.values().map(Listing::id).collect();
        for id in ids {
            self.install_from_open_vsx(&id, window, cx);
        }
    }

    /// The installed version of `id`, if any.
    pub(super) fn installed_version(&self, id: &str) -> Option<&str> {
        self.installed
            .iter()
            .find(|extension| extension.id().eq_ignore_ascii_case(id))
            .map(|extension| extension.manifest.version.as_str())
    }

    /// Closes every tab showing `uri`.
    fn close_document_tabs(&mut self, uri: &str, window: &mut Window, cx: &mut Context<Self>) {
        while let Some((group, tab)) = self.groups.iter().enumerate().find_map(|(group, tabs)| {
            tabs.tabs
                .iter()
                .position(|tab| tab.uri == uri)
                .map(|tab| (group, tab))
        }) {
            self.close_tab_now(group, tab, window, cx);
        }
    }

    /// The search box, and while it holds text the results from Open VSX
    /// in place of the installed extensions.
    fn render_extension_search(&self, colors: Colors) -> Option<AnyElement> {
        self.store.client.as_ref()?;
        Some(
            div()
                .px_1()
                .pb_1()
                .child(
                    Input::new(&self.store.input)
                        .prefix(Icon::new(IconName::Search).small().text_color(colors.faint))
                        .cleanable(true),
                )
                .into_any_element(),
        )
    }

    fn render_open_vsx_results(&self, colors: Colors, cx: &mut Context<Self>) -> AnyElement {
        let strings = self.strings();
        let mut view = div()
            .flex()
            .flex_col()
            .gap_1()
            .child(heading("Open VSX", colors));
        if let Some(error) = &self.store.error {
            return view.child(note(error.clone(), colors)).into_any_element();
        }
        if self.store.searching && self.store.results.is_empty() {
            return view
                .child(note(strings.extensions_searching.to_owned(), colors))
                .into_any_element();
        }
        if self.store.results.is_empty() {
            return view
                .child(note(strings.extensions_none_found.to_owned(), colors))
                .into_any_element();
        }
        view = view.children(
            self.store
                .results
                .iter()
                .enumerate()
                .map(|(index, listing)| self.render_listing(index, listing, colors, cx)),
        );
        view.into_any_element()
    }

    fn render_listing(
        &self,
        index: usize,
        listing: &Listing,
        colors: Colors,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let strings = self.strings();
        let id = listing.id();
        let icon: AnyElement = match self.store.icons.get(&key(&id)) {
            Some(path) => icon_image(path.clone()),
            None => placeholder_icon(colors),
        };
        let installed = self.installed_version(&id);
        let installing = self.store.installing.contains(&key(&id));
        let button = Button::new(("open-vsx-install", index)).small();
        let button = if installing {
            button
                .ghost()
                .label(strings.extension_installing)
                .disabled(true)
        } else {
            match installed {
                Some(version) if !marketplace::is_newer(&listing.version, version) => button
                    .ghost()
                    .label(strings.extension_installed_badge)
                    .disabled(true),
                Some(_) => button.primary().label(strings.extension_update),
                None => button.primary().label(strings.extension_install),
            }
            .on_click(
                cx.listener(move |this, _, window, cx| this.install_from_open_vsx(&id, window, cx)),
            )
        };
        div()
            .id(("open-vsx", index))
            .px_2()
            .py(px(7.))
            .flex()
            .items_center()
            .gap_2p5()
            .rounded(px(8.))
            .hover(|row| row.bg(colors.hover))
            .child(icon)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .flex()
                            .items_baseline()
                            .gap_1p5()
                            .child(
                                div()
                                    .min_w_0()
                                    .truncate()
                                    .text_sm()
                                    .text_color(colors.text)
                                    .child(listing.title().to_owned()),
                            )
                            .child(
                                div()
                                    .flex_none()
                                    .text_xs()
                                    .text_color(colors.faint)
                                    .child(listing.version.clone()),
                            ),
                    )
                    .child(
                        div()
                            .truncate()
                            .text_xs()
                            .text_color(colors.muted)
                            .child(listing.description.clone().unwrap_or_default()),
                    )
                    .child(
                        div()
                            .flex()
                            .gap_1p5()
                            .text_xs()
                            .text_color(colors.faint)
                            .child(listing.namespace.clone())
                            .child(format!("↓ {}", downloads(listing.download_count))),
                    ),
            )
            .child(button)
            .into_any_element()
    }

    pub(super) fn extensions_view_actions(
        &self,
        root: gpui::Div,
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        root.on_action(
            cx.listener(|this, _: &InstallFromVsix, window, cx| this.install_from_vsix(window, cx)),
        )
        .on_action(cx.listener(|this, _: &RestartExtensions, window, cx| {
            this.restart_extensions_by_hand(window, cx)
        }))
        .on_action(cx.listener(|this, action: &ExtensionMenu, window, cx| {
            this.run_extension_menu(action, window, cx)
        }))
        .on_action(cx.listener(|this, _: &CheckExtensionUpdates, window, cx| {
            this.check_extension_updates(true, window, cx)
        }))
        .on_action(cx.listener(|this, _: &UpdateAllExtensions, window, cx| {
            this.update_all_extensions(window, cx)
        }))
    }

    // -----------------------------------------------------------------
    // Drawing

    pub(super) fn extension_menu_items(&self, id: &str) -> Vec<(&'static str, ExtensionMenu)> {
        let strings = self.strings();
        let item = |command: ExtensionCommand| ExtensionMenu {
            id: id.to_owned(),
            command,
        };
        let mut items = Vec::new();
        // A theme extension is there for its themes: they come first.
        if let Some(extension) = self
            .extension_enabled(id)
            .then(|| {
                self.installed
                    .iter()
                    .find(|extension| extension.id().eq_ignore_ascii_case(id))
            })
            .flatten()
        {
            if !color_theme::color_themes(extension).is_empty() {
                items.push((
                    strings.extension_set_color_theme,
                    item(ExtensionCommand::ColorTheme),
                ));
            }
            if !icon_theme::icon_themes(extension).is_empty() {
                items.push((
                    strings.extension_set_icon_theme,
                    item(ExtensionCommand::IconTheme),
                ));
            }
        }
        if self.extension_enabled(id) {
            items.push((strings.extension_disable, item(ExtensionCommand::Disable)));
            if self.project.root.is_some() {
                items.push((
                    strings.extension_disable_project,
                    item(ExtensionCommand::DisableInProject),
                ));
            }
        } else {
            items.push((strings.extension_enable, item(ExtensionCommand::Enable)));
        }
        if self.extension_has_settings(id) {
            items.push((strings.extension_settings, item(ExtensionCommand::Settings)));
        }
        items.push((
            strings.extension_uninstall,
            item(ExtensionCommand::Uninstall),
        ));
        items
    }

    pub(super) fn render_extensions(
        &self,
        colors: Colors,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let strings = self.strings();
        let focus = self.focus_handle.clone();
        let has_updates = !self.store.updates.is_empty();
        let header = view_header(strings.view_extensions, colors)
            .child(
                Button::new("extensions-check")
                    .ghost()
                    .small()
                    .icon(Icon::new(IconName::RefreshCw))
                    .tooltip(strings.extensions_check_updates)
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.check_extension_updates(true, window, cx)
                    })),
            )
            .child(
                Button::new("extensions-more")
                    .ghost()
                    .small()
                    .icon(Icon::new(IconName::Ellipsis))
                    .dropdown_menu(move |menu, _, _| {
                        menu.action_context(focus.clone())
                            .menu(
                                strings.extensions_check_updates,
                                Box::new(CheckExtensionUpdates),
                            )
                            .when(has_updates, |menu| {
                                menu.menu(
                                    strings.extensions_update_all,
                                    Box::new(UpdateAllExtensions),
                                )
                            })
                            .separator()
                            .menu(
                                strings.extensions_import_vscode,
                                Box::new(super::import_extensions::ImportFromVsCode),
                            )
                            .menu(
                                strings.extensions_import_cursor,
                                Box::new(super::import_extensions::ImportFromCursor),
                            )
                            .menu(strings.extensions_install_vsix, Box::new(InstallFromVsix))
                            .menu(strings.extensions_restart, Box::new(RestartExtensions))
                    }),
            );
        let mut view = div().flex().flex_col().gap_1().pb_3().child(header);
        view = view.children(self.render_extension_search(colors));
        if let Some(report) = self
            .store
            .cursor
            .as_ref()
            .filter(|_| self.store.query.is_empty())
        {
            return view
                .child(self.render_cursor_import(report, colors, cx))
                .into_any_element();
        }
        if !self.store.query.is_empty() && self.store.client.is_some() {
            return view
                .child(self.render_open_vsx_results(colors, cx))
                .into_any_element();
        }
        // Updates found: one click takes them all.
        if has_updates {
            view = view.child(
                div()
                    .mx_1()
                    .my_1()
                    .px_2p5()
                    .py_2()
                    .flex()
                    .items_center()
                    .gap_2()
                    .rounded(px(9.))
                    .bg(colors.accent.opacity(0.1))
                    .border_1()
                    .border_color(colors.accent.opacity(0.25))
                    .child(
                        Icon::new(IconName::Download)
                            .small()
                            .text_color(colors.accent),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_sm()
                            .text_color(colors.text)
                            .child(format!(
                                "{} · {}",
                                strings.extensions_updates_available,
                                self.store.updates.len()
                            )),
                    )
                    .child(
                        Button::new("extensions-update-all")
                            .primary()
                            .small()
                            .label(strings.extension_update)
                            .tooltip(strings.extensions_update_all)
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.update_all_extensions(window, cx)
                            })),
                    ),
            );
        }

        let installed_open = !self.store.installed_collapsed;
        let turn = chevron_turn(
            "extensions-installed-turn",
            installed_open,
            self.look,
            window,
            cx,
        );
        view = view.child(
            section_header(
                "extensions-installed",
                strings.installed_extensions,
                turn,
                colors,
            )
            .on_click(cx.listener(|this, _, _, cx| {
                this.store.installed_collapsed = !this.store.installed_collapsed;
                cx.notify();
            }))
            .child(count_badge(self.installed.len(), colors)),
        );
        if installed_open {
            if self.installed.is_empty() {
                view = view.child(empty_state(
                    IconName::Blocks,
                    strings.extensions_empty_title,
                    strings.extensions_empty_hint,
                    colors,
                ));
            } else {
                let mut installed: Vec<&InstalledExtension> = self.installed.iter().collect();
                installed.sort_by_key(|extension| display_name(extension).to_lowercase());
                view =
                    view.children(installed.into_iter().enumerate().map(|(index, extension)| {
                        self.render_extension_row(index, extension, colors, cx)
                    }));
            }
        }

        // What the project recommends and is not installed yet.
        let recommended: Vec<String> = self
            .store
            .recommended
            .iter()
            .filter(|id| self.installed_version(id).is_none())
            .cloned()
            .collect();
        if !recommended.is_empty() {
            let open = !self.store.recommended_collapsed;
            let turn = chevron_turn("extensions-recommended-turn", open, self.look, window, cx);
            view = view.child(
                div().pt_2().child(
                    section_header(
                        "extensions-recommended",
                        strings.extensions_recommended,
                        turn,
                        colors,
                    )
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.store.recommended_collapsed = !this.store.recommended_collapsed;
                        cx.notify();
                    }))
                    .child(count_badge(recommended.len(), colors)),
                ),
            );
            if open {
                view = view.children(
                    recommended
                        .into_iter()
                        .enumerate()
                        .map(|(index, id)| self.render_recommended_row(index, id, colors, cx)),
                );
            }
        }

        // The tools, in sight rather than behind "…".
        // The settings row goes without its keys: its label needs the room.
        let tool = |id: &'static str,
                    icon: IconName,
                    label: &'static str,
                    action: Box<dyn gpui::Action>| {
            let keys = (id != "extensions-tool-settings")
                .then(|| super::shell::keys_for(action.as_ref(), window))
                .flatten();
            tool_row(id, icon, label, keys, colors)
                .on_click(move |_, window, cx| window.dispatch_action(action.boxed_clone(), cx))
        };
        view = view
            .child(
                div()
                    .pt_3()
                    .child(heading(strings.extensions_tools, colors)),
            )
            .child(div().px_1().flex().flex_col().children([
                tool(
                    "extensions-tool-vsix",
                    IconName::PackagePlus,
                    strings.extensions_tool_vsix,
                    Box::new(InstallFromVsix),
                ),
                tool(
                    "extensions-tool-vscode",
                    IconName::Import,
                    strings.extensions_import_vscode,
                    Box::new(super::import_extensions::ImportFromVsCode),
                ),
                tool(
                    "extensions-tool-cursor",
                    IconName::Import,
                    strings.extensions_import_cursor,
                    Box::new(super::import_extensions::ImportFromCursor),
                ),
                tool(
                    "extensions-tool-settings",
                    IconName::Settings,
                    strings.extensions_tool_settings,
                    Box::new(OpenSettings),
                ),
                tool(
                    "extensions-tool-restart",
                    IconName::RotateCcw,
                    strings.extensions_restart,
                    Box::new(RestartExtensions),
                ),
            ]))
            .child(
                div()
                    .px_2()
                    .pt_3()
                    .text_xs()
                    .text_color(colors.faint)
                    .child(strings.extensions_footnote),
            );
        view.into_any_element()
    }

    /// An extension the project recommends: its id, and Install.
    fn render_recommended_row(
        &self,
        index: usize,
        id: String,
        colors: Colors,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let strings = self.strings();
        let installing = self.store.installing.contains(&key(&id));
        let (publisher, name) = id.split_once('.').unwrap_or(("", id.as_str()));
        let (publisher, name) = (publisher.to_owned(), name.to_owned());
        let install = self.store.client.as_ref().map(|_| {
            let id = id.clone();
            Button::new(("extension-recommended-install", index))
                .primary()
                .small()
                .label(if installing {
                    strings.extension_installing
                } else {
                    strings.extension_install
                })
                .disabled(installing)
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.install_from_open_vsx(&id, window, cx)
                }))
        });
        div()
            .id(("extension-recommended", index))
            .px_2()
            .py_1p5()
            .flex()
            .items_center()
            .gap_2p5()
            .rounded(px(8.))
            .hover(|row| row.bg(colors.hover))
            .child(placeholder_icon(colors))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .truncate()
                            .text_sm()
                            .font_weight(gpui::FontWeight::MEDIUM)
                            .text_color(colors.text)
                            .child(name),
                    )
                    .child(
                        div()
                            .truncate()
                            .text_xs()
                            .text_color(colors.faint)
                            .child(publisher),
                    ),
            )
            .children(install)
            .into_any_element()
    }

    fn render_extension_row(
        &self,
        index: usize,
        extension: &InstalledExtension,
        colors: Colors,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let id = extension.id();
        let enabled = self.extension_enabled(&id);
        let strings = self.strings();
        let icon: AnyElement = match icon_path(extension) {
            Some(path) => icon_image(path),
            None => placeholder_icon(colors),
        };
        let items = self.extension_menu_items(&id);
        let update = self
            .store
            .updates
            .get(&key(&id))
            .map(|listing| listing.version.clone());
        let installing = self.store.installing.contains(&key(&id));
        let focus = self.focus_handle.clone();
        let opened = id.clone();
        let limits = self.extension_limits(&id);
        div()
            .id(("extension", index))
            .px_2()
            .py(px(7.))
            .flex()
            .items_center()
            .gap_2p5()
            .rounded(px(8.))
            .cursor_pointer()
            .hover(|row| row.bg(colors.hover))
            .active(|row| row.bg(colors.selected))
            .on_click(cx.listener(move |this, _, window, cx| {
                this.open_extension_details(&opened, window, cx)
            }))
            .child(div().when(!enabled, |icon| icon.opacity(0.5)).child(icon))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap(px(1.))
                    // The name keeps the whole width: the version goes under it.
                    .child(
                        div()
                            .truncate()
                            .text_sm()
                            .font_weight(gpui::FontWeight::MEDIUM)
                            .text_color(if enabled { colors.text } else { colors.muted })
                            .child(display_name(extension)),
                    )
                    .child(
                        div()
                            .truncate()
                            .text_xs()
                            .text_color(colors.muted)
                            .child(description(extension)),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_1()
                            .min_w_0()
                            .text_xs()
                            .text_color(colors.faint)
                            .child(
                                div()
                                    .min_w_0()
                                    .truncate()
                                    .child(extension.manifest.publisher.clone()),
                            )
                            .child(
                                div()
                                    .flex_none()
                                    .child(format!("· {}", extension.manifest.version)),
                            )
                            .when(!enabled, |line| {
                                line.child(
                                    pill(strings.extension_disabled_badge, colors.muted)
                                        .flex_none(),
                                )
                            })
                            .when_some(limits, |line, (works, why)| {
                                line.child(
                                    pill(
                                        if works {
                                            strings.extension_partly_working
                                        } else {
                                            strings.extension_not_working
                                        },
                                        if works {
                                            colors.muted
                                        } else {
                                            cx.theme().warning
                                        },
                                    )
                                    .id(("extension-limits", index))
                                    .min_w_0()
                                    .tooltip(
                                        move |window, cx| {
                                            gpui_component::tooltip::Tooltip::new(why.clone())
                                                .build(window, cx)
                                        },
                                    ),
                                )
                            }),
                    ),
            )
            .when_some(update.filter(|_| !installing), |row, version| {
                let id = id.clone();
                row.child(
                    Button::new(("extension-update", index))
                        .primary()
                        .small()
                        .label(strings.extension_update)
                        .tooltip(version)
                        .on_click(cx.listener(move |this, _, window, cx| {
                            cx.stop_propagation();
                            this.install_from_open_vsx(&id, window, cx)
                        })),
                )
            })
            .when(installing, |row| {
                row.child(
                    div()
                        .text_xs()
                        .text_color(colors.faint)
                        .child(strings.extension_installing),
                )
            })
            .child(
                Button::new(("extension-menu", index))
                    .ghost()
                    .small()
                    .icon(Icon::new(IconName::Settings))
                    .dropdown_menu(move |menu, _, _| {
                        let mut menu = menu.action_context(focus.clone());
                        for (label, action) in &items {
                            menu = menu.menu(*label, Box::new(action.clone()));
                        }
                        menu
                    }),
            )
            .into_any_element()
    }

    pub(super) fn render_extension_details(
        &self,
        tab: &ExtensionDetailsTab,
        colors: Colors,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(extension) = self
            .installed
            .iter()
            .find(|extension| extension.id() == tab.id)
        else {
            return note(self.strings().extension_missing.to_owned(), colors).into_any_element();
        };
        let strings = self.strings();
        let enabled = self.extension_enabled(&tab.id);
        let icon: AnyElement = match icon_path(extension) {
            Some(path) => img(path).size(px(72.)).flex_none().into_any_element(),
            None => div()
                .size(px(72.))
                .flex_none()
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(12.))
                .bg(colors.hover)
                .child(
                    Icon::new(IconName::Blocks)
                        .size(px(32.))
                        .text_color(colors.muted),
                )
                .into_any_element(),
        };
        let buttons = self
            .extension_menu_items(&tab.id)
            .into_iter()
            .enumerate()
            .map(|(index, (label, action))| {
                Button::new(("extension-detail-action", index))
                    .small()
                    .when(index == 0, |button| button.primary())
                    .when(index > 0, |button| button.ghost())
                    .label(label)
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.run_extension_menu(&action, window, cx)
                    }))
            });
        let theme = cx.theme();
        div()
            .size_full()
            .flex()
            .flex_col()
            .child(
                div()
                    .flex_none()
                    .p_6()
                    .flex()
                    .gap_4()
                    .border_b_1()
                    .border_color(colors.border)
                    .child(icon)
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .child(
                                div()
                                    .flex()
                                    .items_baseline()
                                    .gap_2()
                                    .child(
                                        div()
                                            .text_xl()
                                            .font_weight(gpui::FontWeight::SEMIBOLD)
                                            .text_color(colors.text)
                                            .child(display_name(extension)),
                                    )
                                    .child(
                                        div()
                                            .text_sm()
                                            .text_color(colors.muted)
                                            .child(extension.manifest.version.clone()),
                                    ),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(colors.faint)
                                    .font_family(theme.mono_font_family.clone())
                                    .child(tab.id.clone()),
                            )
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(colors.muted)
                                    .child(description(extension)),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(colors.faint)
                                    .child(contributions(extension, strings)),
                            )
                            .when(!enabled, |info| {
                                info.child(
                                    div()
                                        .text_xs()
                                        .text_color(theme.warning)
                                        .child(strings.extension_disabled_badge),
                                )
                            })
                            .when_some(self.extension_limits(&tab.id), |info, (works, why)| {
                                info.child(
                                    div()
                                        .text_xs()
                                        .text_color(if works {
                                            colors.muted
                                        } else {
                                            theme.warning
                                        })
                                        .child(why),
                                )
                            })
                            .child(div().pt_2().flex().gap_2().children(buttons)),
                    ),
            )
            .child(
                div().flex_1().min_h_0().flex().justify_center().child(
                    div().h_full().w_full().max_w(px(880.)).px_8().child(
                        TextView::new(&tab.readme)
                            .scrollable(true)
                            .selectable(true)
                            .size_full(),
                    ),
                ),
            )
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use gpui::{TestAppContext, VisualTestContext};

    use super::super::tests::test_settings;
    use super::*;
    use crate::extension_state::ExtensionState;

    const ID: &str = "manuel-local.vsauto-ghost";

    /// The project's recommendations, as VS Code reads them: comments
    /// allowed, unwanted ones left out, each id once and in lower case.
    #[test]
    fn the_project_recommends_extensions() {
        let folder = tempfile::tempdir().unwrap();
        assert!(recommendations(folder.path()).is_empty());
        std::fs::create_dir_all(folder.path().join(".vscode")).unwrap();
        std::fs::write(
            folder.path().join(".vscode/extensions.json"),
            r#"{
                // Formatting and lint.
                "recommendations": [
                    "esbenp.prettier-vscode",
                    "dbaeumer.vscode-eslint",
                    "ESBENP.prettier-vscode",
                    "not-an-id",
                    "ms-python.python",
                ],
                "unwantedRecommendations": ["ms-python.python"]
            }"#,
        )
        .unwrap();
        assert_eq!(
            recommendations(folder.path()),
            ["esbenp.prettier-vscode", "dbaeumer.vscode-eslint"]
        );
    }

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

    /// The Extensions view says which extensions work only in part in
    /// Keel, or not at all, and why; the same check as "Import from Cursor".
    #[gpui::test]
    fn extensions_that_work_in_part_are_labelled(cx: &mut TestAppContext) {
        cx.executor().allow_parking();
        cx.update(|cx| {
            gpui_component::init(cx);
            language::register_grammars();
            init(cx);
        });
        let data = tempfile::tempdir().unwrap();
        for (name, contributes, main) in [
            ("fine", r#"{ "themes": [] }"#, ""),
            (
                "lint",
                r#"{ "taskDefinitions": [{ "type": "lint" }] }"#,
                "vscode.window.createWebviewPanel('lint');",
            ),
            ("debug", r#"{ "debuggers": [{ "type": "node" }] }"#, ""),
        ] {
            let root =
                crate::paths::extensions_dir(data.path()).join(format!("tests.{name}-1.0.0"));
            std::fs::create_dir_all(&root).unwrap();
            std::fs::write(
                root.join("package.json"),
                format!(
                    r#"{{ "name": "{name}", "publisher": "tests", "version": "1.0.0",
                         "main": "./extension.js", "contributes": {contributes} }}"#
                ),
            )
            .unwrap();
            std::fs::write(root.join("extension.js"), main).unwrap();
        }
        let folder = tempfile::tempdir().unwrap();
        let settings_file = folder.path().join("settings.json");
        std::fs::write(&settings_file, r#"{ "keel.language": "en" }"#).unwrap();
        let path = folder.path().join("notes.md");
        std::fs::write(&path, "# Notes\n").unwrap();
        let mut setup = crate::windows::extension_setup(data.path()).unwrap();
        setup.marketplace = None;
        let document = Document::open(&path).unwrap();
        let (view, cx) = cx.add_window_view(|window, cx| {
            let settings = Settings::load(&settings_file, None);
            Workbench::new(Some(document), settings, Some(setup), window, cx)
        });
        wait_until(cx, "the check of the extensions", |cx| {
            view.read_with(cx, |view, _| view.store.limits.len() == 3)
        });
        view.read_with(cx, |view, _| {
            assert_eq!(view.extension_limits("tests.fine"), None);
            assert_eq!(
                view.extension_limits("tests.lint"),
                Some((
                    true,
                    "Works in part. Without: web pages inside the window, its tasks.".to_owned()
                ))
            );
            assert_eq!(
                view.extension_limits("tests.debug"),
                Some((
                    false,
                    "Does not work in Keel: needs the debugger.".to_owned()
                ))
            );
        });
        // Drawn in the view and in the details tab.
        view.update_in(cx, |view, window, cx| {
            view.layout
                .select_side_view(crate::layout::SideView::Extensions);
            view.open_extension_details("tests.debug", window, cx);
        });
        cx.update(|window, _| window.refresh());
        cx.run_until_parked();
    }

    /// A package with a color theme and a file icon theme.
    fn theme_vsix(folder: &Path) -> PathBuf {
        use std::io::Write as _;
        let path = folder.join("looks.vsix");
        let mut zip = zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
        for (name, text) in [
            (
                "extension/package.json",
                r#"{ "name": "looks", "displayName": "Looks", "publisher": "someone",
                     "version": "1.0.0", "contributes": {
                       "themes": [{ "label": "Ghost Night", "uiTheme": "vs-dark", "path": "./night.json" }],
                       "iconThemes": [{ "id": "ghost-icons", "label": "Ghost Icons", "path": "./icons.json" }]
                     } }"#,
            ),
            (
                "extension/night.json",
                r##"{ "colors": { "editor.background": "#101820", "editor.foreground": "#E0E0E0" } }"##,
            ),
            (
                "extension/icons.json",
                r#"{ "iconDefinitions": { "file": { "iconPath": "./file.svg" } }, "file": "file" }"#,
            ),
            (
                "extension/file.svg",
                r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16"/>"#,
            ),
        ] {
            zip.start_file(name, zip::write::SimpleFileOptions::default())
                .unwrap();
            zip.write_all(text.as_bytes()).unwrap();
        }
        zip.finish().unwrap();
        path
    }

    /// As in VS Code: once installed, a theme extension offers its color
    /// themes and then its icon themes; its menu offers them again.
    #[gpui::test]
    fn a_theme_extension_offers_its_themes_once_installed(cx: &mut TestAppContext) {
        cx.executor().allow_parking();
        cx.update(|cx| {
            gpui_component::init(cx);
            language::register_grammars();
            crate::theme::init(cx);
            init(cx);
        });
        let data = tempfile::tempdir().unwrap();
        let folder = tempfile::tempdir().unwrap();
        let package = theme_vsix(folder.path());
        let settings_file = folder.path().join("settings.json");
        std::fs::write(&settings_file, r#"{ "keel.language": "en" }"#).unwrap();
        let path = folder.path().join("notes.md");
        std::fs::write(&path, "# Notes\n").unwrap();
        let mut setup = crate::windows::extension_setup(data.path()).unwrap();
        setup.marketplace = None;
        let document = Document::open(&path).unwrap();
        let mut workbench = None;
        let (_root, cx) = cx.add_window_view(|window, cx| {
            let settings = Settings::load(&settings_file, None);
            let view =
                cx.new(|cx| Workbench::new(Some(document), settings, Some(setup), window, cx));
            workbench = Some(view.clone());
            gpui_component::Root::new(view, window, cx)
        });
        let view = workbench.unwrap();
        let open =
            |cx: &mut VisualTestContext| cx.update(|window, cx| window.has_active_dialog(cx));
        let setting = |view: &Entity<Workbench>, cx: &mut VisualTestContext, key: &str| {
            view.read_with(cx, |view, _| view.settings.string(key).map(str::to_owned))
        };

        view.update_in(cx, |view, window, cx| {
            view.install_vsix(&package, window, cx)
        });
        cx.run_until_parked();
        assert!(open(cx), "its color themes are offered");
        cx.simulate_input("Ghost Night");
        cx.simulate_keystrokes("enter");
        cx.run_until_parked();
        assert_eq!(
            setting(&view, cx, "workbench.colorTheme").as_deref(),
            Some("Ghost Night")
        );
        assert!(open(cx), "then its icon themes");
        cx.simulate_input("Keep");
        cx.simulate_keystrokes("enter");
        cx.run_until_parked();
        assert!(!open(cx));
        assert_eq!(setting(&view, cx, "workbench.iconTheme"), None);

        // Its menu has them first, and sets them.
        let menu: Vec<&str> = view.read_with(cx, |view, _| {
            view.extension_menu_items("someone.looks")
                .into_iter()
                .map(|(label, _)| label)
                .collect()
        });
        assert_eq!(
            menu,
            [
                "Set Color Theme",
                "Set File Icon Theme",
                "Disable",
                "Disable (Workspace)",
                "Uninstall"
            ]
        );
        view.update_in(cx, |view, window, cx| {
            view.run_extension_menu(
                &ExtensionMenu {
                    id: "someone.looks".to_owned(),
                    command: ExtensionCommand::IconTheme,
                },
                window,
                cx,
            )
        });
        cx.run_until_parked();
        cx.simulate_input("Ghost Icons");
        cx.simulate_keystrokes("enter");
        cx.run_until_parked();
        assert_eq!(
            setting(&view, cx, "workbench.iconTheme").as_deref(),
            Some("ghost-icons")
        );
        assert!(view.read_with(cx, |view, _| view.icon_theme.is_some()));
    }

    fn running_commands(view: &Entity<Workbench>, cx: &mut VisualTestContext) -> usize {
        view.read_with(cx, |view, _| {
            view.extensions
                .as_ref()
                .map_or(0, |extensions| extensions.titles.len())
        })
    }

    #[gpui::test]
    fn extensions_install_turn_off_and_on_without_reopening(cx: &mut TestAppContext) {
        cx.executor().allow_parking();
        cx.update(|cx| {
            gpui_component::init(cx);
            language::register_grammars();
            init(cx);
        });
        let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures");
        let data = tempfile::tempdir().unwrap();
        let project = tempfile::tempdir().unwrap();
        let path = project.path().join("notes.md");
        std::fs::write(&path, "# Notes\n").unwrap();
        let mut setup = crate::windows::extension_setup(data.path()).unwrap();
        setup.marketplace = None;
        let document = Document::open(&path).unwrap();
        let mut workbench = None;
        let (_root, cx) = cx.add_window_view(|window, cx| {
            let view = cx
                .new(|cx| Workbench::new(Some(document), test_settings(), Some(setup), window, cx));
            workbench = Some(view.clone());
            gpui_component::Root::new(view, window, cx)
        });
        let view = workbench.unwrap();
        assert_eq!(running_commands(&view, cx), 0);

        // Install from a package: listed, running, with its commands.
        view.update_in(cx, |view, window, cx| {
            view.install_vsix(
                &fixtures.join("extensions/vsauto-ghost-local-0.6.1-editor-layout.vsix"),
                window,
                cx,
            )
        });
        cx.run_until_parked();
        view.read_with(cx, |view, _| {
            assert_eq!(view.installed.len(), 1);
            assert!(view.installed[0].id().eq_ignore_ascii_case(ID));
            assert!(view.extension_enabled(ID));
            assert!(view.extensions.as_ref().unwrap().host.is_running());
        });
        assert!(running_commands(&view, cx) > 0);

        // Off: still listed, no longer running, and the choice is saved.
        view.update_in(cx, |view, window, cx| {
            let action = ExtensionMenu {
                id: ID.to_owned(),
                command: ExtensionCommand::Disable,
            };
            view.run_extension_menu(&action, window, cx)
        });
        cx.run_until_parked();
        view.read_with(cx, |view, _| {
            assert_eq!(view.installed.len(), 1);
            assert!(!view.extension_enabled(ID));
        });
        assert_eq!(running_commands(&view, cx), 0);
        assert!(!ExtensionState::load(data.path()).is_enabled(ID, None));

        // On again: back without reopening the window.
        view.update_in(cx, |view, window, cx| {
            let action = ExtensionMenu {
                id: ID.to_owned(),
                command: ExtensionCommand::Enable,
            };
            view.run_extension_menu(&action, window, cx)
        });
        wait_until(cx, "the commands back", |cx| {
            running_commands(&view, cx) > 0
        });

        // The details tab shows the README.
        view.update_in(cx, |view, window, cx| {
            view.open_extension_details(&view.installed[0].id(), window, cx)
        });
        view.read_with(cx, |view, _| {
            let uri = details_uri(&view.installed[0].id());
            assert!(matches!(
                view.documents.get(&uri).map(|document| &document.backing),
                Some(Backing::ExtensionDetails(_))
            ));
        });

        // Uninstalling asks first; cancelling keeps everything.
        view.update_in(cx, |view, window, cx| {
            let action = ExtensionMenu {
                id: ID.to_owned(),
                command: ExtensionCommand::Uninstall,
            };
            view.run_extension_menu(&action, window, cx)
        });
        assert!(cx.has_pending_prompt());
        cx.simulate_prompt_answer("Annulla");
        cx.run_until_parked();
        view.read_with(cx, |view, _| {
            assert_eq!(view.installed.len(), 1);
            assert!(view.installed[0].root.is_dir());
        });
    }

    /// An Open VSX made of files, with VSAuto Ghost at `version`.
    fn fake_open_vsx(root: &Path, version: &str) -> marketplace::Marketplace {
        let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures");
        let package = fixtures.join("extensions/vsauto-ghost-local-0.6.1-editor-layout.vsix");
        let base = format!("file://{}", root.display());
        std::fs::write(root.join("icon.png"), b"PNG").unwrap();
        let listing = json!({
            "namespace": "manuel-local", "name": "vsauto-ghost", "version": version,
            "displayName": "VSAuto Ghost Local", "description": "Ghost typing", "downloadCount": 1234,
            "files": {
                "download": format!("file://{}", package.display()),
                "icon": format!("{base}/icon.png"),
            }
        });
        std::fs::create_dir_all(root.join("api/-")).unwrap();
        std::fs::create_dir_all(root.join("api/manuel-local")).unwrap();
        std::fs::write(
            root.join("api/-/search"),
            json!({ "extensions": [listing] }).to_string(),
        )
        .unwrap();
        std::fs::write(
            root.join("api/manuel-local/vsauto-ghost"),
            listing.to_string(),
        )
        .unwrap();
        marketplace::Marketplace::with_base(base)
    }

    #[gpui::test]
    fn extensions_are_found_installed_and_updated_from_open_vsx(cx: &mut TestAppContext) {
        cx.executor().allow_parking();
        cx.update(|cx| {
            gpui_component::init(cx);
            language::register_grammars();
            init(cx);
        });
        let data = tempfile::tempdir().unwrap();
        let registry = tempfile::tempdir().unwrap();
        let mut setup = crate::windows::extension_setup(data.path()).unwrap();
        setup.marketplace = Some(fake_open_vsx(registry.path(), "0.6.1"));
        let mut workbench = None;
        let (_root, cx) = cx.add_window_view(|window, cx| {
            let view = cx.new(|cx| Workbench::new(None, test_settings(), Some(setup), window, cx));
            workbench = Some(view.clone());
            gpui_component::Root::new(view, window, cx)
        });
        let view = workbench.unwrap();

        // Searching shows the results, with their icons.
        view.update_in(cx, |view, window, cx| {
            view.store
                .input
                .update(cx, |input, cx| input.set_value("ghost", window, cx));
            view.search_extensions(window, cx);
        });
        wait_until(cx, "the results", |cx| {
            view.read_with(cx, |view, _| {
                view.store.results.len() == 1 && !view.store.icons.is_empty()
            })
        });
        view.read_with(cx, |view, _| {
            assert_eq!(view.store.results[0].id(), ID);
            assert!(view.installed.is_empty());
        });

        // Install: downloaded, installed, running, and the download is gone.
        view.update_in(cx, |view, window, cx| {
            view.install_from_open_vsx(ID, window, cx)
        });
        wait_until(cx, "the installed extension", |cx| {
            view.read_with(cx, |view, _| {
                view.installed.len() == 1 && view.store.installing.is_empty()
            })
        });
        assert!(running_commands(&view, cx) > 0);
        let downloads = crate::paths::open_vsx_cache_dir(data.path()).join("downloads");
        assert_eq!(std::fs::read_dir(&downloads).unwrap().count(), 0);

        // A newer version on the registry shows up as an update.
        fake_open_vsx(registry.path(), "0.7.0");
        view.update_in(cx, |view, window, cx| {
            view.check_extension_updates(true, window, cx)
        });
        wait_until(cx, "the update", |cx| {
            view.read_with(cx, |view, _| view.store.updates.contains_key(ID))
        });
        view.read_with(cx, |view, _| {
            assert_eq!(
                view.status_message.as_deref(),
                Some(view.strings().extension_updates_found(1).as_str())
            );
        });

        // An empty box shows the installed extensions again.
        view.update_in(cx, |view, window, cx| {
            view.store
                .input
                .update(cx, |input, cx| input.set_value("", window, cx));
            view.search_extensions(window, cx);
        });
        view.read_with(cx, |view, _| {
            assert!(view.store.query.is_empty() && view.store.results.is_empty())
        });
    }
}
