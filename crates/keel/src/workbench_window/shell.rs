//! The layout of sketch D: rounded panels floating on the window's
//! background, a unified toolbar beside the traffic lights, the sidebar, the
//! editors with the bottom panel, Claude Code on the right, the status bar.

use gpui::{
    AnyElement, Bounds, Div, FontWeight, Pixels, Render, StatefulInteractiveElement as _,
    TitlebarOptions, WindowBounds, WindowOptions, point, size, svg,
};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::menu::ContextMenuExt as _;
use gpui_component::{Icon, Selectable as _, Sizable as _, TitleBar};
use gpui_kit_assets::IconName;

use super::*;

const TOOLBAR_HEIGHT: f32 = 52.;
/// The space between the panels and around them.
pub(super) const GAP: f32 = 8.;
const RADIUS: f32 = 12.;
const SIDEBAR_WIDTH: f32 = 240.;
const STATUS_HEIGHT: f32 = 24.;
/// Where the traffic lights sit, centred in the toolbar.
const TRAFFIC_LIGHTS_X: f32 = 18.;
const TRAFFIC_LIGHTS_Y: f32 = (TOOLBAR_HEIGHT - 14.) / 2.;
/// Room for the traffic lights before the toolbar's content.
const TRAFFIC_LIGHTS_WIDTH: f32 = 88.;

/// The window: the toolbar is drawn by Keel, with macOS's traffic lights.
pub(crate) fn window_options(bounds: Bounds<Pixels>) -> WindowOptions {
    WindowOptions {
        titlebar: Some(TitlebarOptions {
            title: Some("Keel".into()),
            appears_transparent: true,
            traffic_light_position: Some(point(px(TRAFFIC_LIGHTS_X), px(TRAFFIC_LIGHTS_Y))),
        }),
        // Keel's toolbar moves the window itself, as gpui-component expects.
        app_owns_titlebar_drag: true,
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        window_min_size: Some(size(px(720.), px(480.))),
        ..Default::default()
    }
}

/// The theme's colors under the names of the sketch.
#[derive(Clone, Copy)]
pub(super) struct Colors {
    /// The window's background, around the panels.
    pub(super) desk: Hsla,
    /// The sidebar and Claude Code.
    pub(super) panel: Hsla,
    /// The editors.
    pub(super) editor: Hsla,
    pub(super) border: Hsla,
    pub(super) text: Hsla,
    pub(super) muted: Hsla,
    pub(super) faint: Hsla,
    pub(super) accent: Hsla,
    pub(super) selected: Hsla,
    pub(super) hover: Hsla,
}

impl Colors {
    pub(super) fn new(cx: &App) -> Self {
        let theme = cx.theme();
        Self {
            desk: theme.title_bar,
            panel: theme.sidebar,
            editor: theme.background,
            border: theme.border,
            text: theme.foreground,
            muted: theme.muted_foreground,
            faint: theme.muted_foreground.opacity(0.7),
            accent: theme.primary,
            selected: theme.list_active,
            hover: theme.secondary_hover,
        }
    }
}

/// A rounded panel floating on the window's background.
fn floating(background: Hsla, border: Hsla) -> Div {
    div()
        .h_full()
        .flex()
        .flex_col()
        .overflow_hidden()
        .rounded(px(RADIUS))
        .border_1()
        .border_color(border)
        .bg(background)
}

pub(super) fn heading(text: &str, colors: Colors) -> Div {
    div()
        .px_2()
        .pt_1()
        .pb_1()
        .text_xs()
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(colors.faint)
        .child(text.to_uppercase())
}

/// A short note for a part of the window that comes in a later phase.
pub(super) fn note(text: String, colors: Colors) -> Div {
    div()
        .px_2()
        .py_1()
        .text_sm()
        .text_color(colors.muted)
        .child(text)
}

fn side_view_icon(view: SideView) -> IconName {
    match view {
        SideView::Explorer => IconName::Files,
        SideView::Search => IconName::Search,
        SideView::SourceControl => IconName::GitBranch,
        SideView::Extensions => IconName::Blocks,
    }
}

fn side_view_title(view: SideView, strings: &Strings) -> &'static str {
    match view {
        SideView::Explorer => strings.view_explorer,
        SideView::Search => strings.view_search,
        SideView::SourceControl => strings.view_source_control,
        SideView::Extensions => strings.view_extensions,
    }
}

fn panel_view_title(view: PanelView, strings: &Strings) -> &'static str {
    match view {
        PanelView::Problems => strings.panel_problems,
        PanelView::Output => strings.panel_output,
        PanelView::Terminal => strings.panel_terminal,
        PanelView::Ports => strings.panel_ports,
    }
}

/// The name of a language in the status bar, from its VS Code identifier.
pub(super) fn language_name(id: &str, strings: &Strings) -> String {
    match id {
        "typescript" => "TypeScript",
        "typescriptreact" => "TypeScript React",
        "javascript" => "JavaScript",
        "javascriptreact" => "JavaScript React",
        "json" => "JSON",
        "jsonc" => "JSON with Comments",
        "markdown" => "Markdown",
        "rust" => "Rust",
        "python" => "Python",
        "java" => "Java",
        "c" => "C",
        "cpp" => "C++",
        "csharp" => "C#",
        "html" => "HTML",
        "css" => "CSS",
        "scss" => "SCSS",
        "less" => "Less",
        "yaml" => "YAML",
        "toml" => "TOML",
        "prisma" => "Prisma",
        "shellscript" => "Shell Script",
        "dotenv" => "Environment Variables",
        "plaintext" => strings.plain_text,
        other => other,
    }
    .to_owned()
}

impl Workbench {
    // -----------------------------------------------------------------
    // Layout commands

    pub(super) fn toggle_sidebar(&mut self, cx: &mut Context<Self>) {
        self.layout.sidebar = !self.layout.sidebar;
        self.session_changed(cx);
        cx.notify();
    }

    pub(super) fn toggle_panel(&mut self, cx: &mut Context<Self>) {
        self.layout.panel = !self.layout.panel;
        self.session_changed(cx);
        cx.notify();
    }

    pub(super) fn toggle_agent(&mut self, cx: &mut Context<Self>) {
        self.layout.agent = !self.layout.agent;
        self.session_changed(cx);
        cx.notify();
    }

    pub(super) fn show_side_view(&mut self, view: SideView, cx: &mut Context<Self>) {
        self.layout.show_side_view(view);
        self.session_changed(cx);
        cx.notify();
    }

    pub(super) fn toggle_panel_view(&mut self, view: PanelView, cx: &mut Context<Self>) {
        self.layout.toggle_panel_view(view);
        self.session_changed(cx);
        cx.notify();
    }

    /// Shows a document from the Open Editors list: in its pane if it has
    /// one, otherwise in the active pane.
    fn show_document(&mut self, uri: &str, window: &mut Window, cx: &mut Context<Self>) {
        if !self.documents.contains_key(uri) {
            return;
        }
        let column = self.active.min(self.panes.len().saturating_sub(1));
        self.open_in_pane(uri, column, true, window, cx);
        self.publish_editors(window, cx);
        self.refresh_title(window);
        cx.notify();
    }

    // -----------------------------------------------------------------
    // Drawing

    fn render_toolbar(&self, colors: Colors, window: &Window, cx: &mut Context<Self>) -> TitleBar {
        let strings = self.strings();
        let project = div()
            .flex_none()
            .flex()
            .items_center()
            .gap_2()
            .child(
                svg()
                    .path(crate::assets::LOGO)
                    .size(px(16.))
                    .text_color(colors.muted),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(colors.text)
                            .child(self.project.name.clone()),
                    )
                    .when_some(self.project.branch.clone(), |project, branch| {
                        project.child(div().text_xs().text_color(colors.muted).child(branch))
                    }),
            );

        let search = div()
            .id("toolbar-search")
            .w_full()
            .max_w(px(420.))
            .h(px(28.))
            .px_2p5()
            .flex()
            .items_center()
            .gap_2()
            .rounded(px(8.))
            .border_1()
            .border_color(colors.border)
            .bg(colors.editor)
            .text_sm()
            .text_color(colors.faint)
            .cursor_pointer()
            .hover(|search| search.text_color(colors.muted))
            .on_click(cx.listener(|this, _, window, cx| this.quick_open(window, cx)))
            .child(Icon::new(IconName::Search).small())
            .child(if self.project.root.is_some() {
                strings.search_files
            } else {
                strings.search_commands
            })
            .child(
                div()
                    .ml_auto()
                    .text_xs()
                    .child(if self.project.root.is_some() {
                        "⌘P"
                    } else {
                        "⇧⌘P"
                    }),
            );

        let toggles = div()
            .flex_none()
            .flex()
            .items_center()
            .gap_1()
            .child(
                Button::new("toggle-sidebar")
                    .ghost()
                    .small()
                    .icon(Icon::new(IconName::PanelLeft))
                    .selected(self.layout.sidebar)
                    .tooltip(strings.toggle_sidebar)
                    .on_click(cx.listener(|this, _, _, cx| this.toggle_sidebar(cx))),
            )
            .child(
                Button::new("toggle-panel")
                    .ghost()
                    .small()
                    .icon(Icon::new(IconName::PanelBottom))
                    .selected(self.layout.panel)
                    .tooltip(strings.toggle_panel)
                    .on_click(cx.listener(|this, _, _, cx| this.toggle_panel(cx))),
            )
            .child(
                Button::new("toggle-agent")
                    .ghost()
                    .small()
                    .icon(Icon::new(IconName::PanelRight))
                    .selected(self.layout.agent)
                    .tooltip(strings.toggle_agent)
                    .on_click(cx.listener(|this, _, _, cx| this.toggle_agent(cx))),
            );

        let left = if window.is_fullscreen() {
            px(16.)
        } else {
            px(TRAFFIC_LIGHTS_WIDTH)
        };
        TitleBar::new()
            .h(px(TOOLBAR_HEIGHT))
            .pl(left)
            .pr(px(12.))
            .bg(colors.desk)
            .border_color(gpui::transparent_black())
            .child(project)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .px_4()
                    .flex()
                    .justify_center()
                    .child(search),
            )
            .child(toggles)
    }

    fn render_sidebar(&self, colors: Colors, cx: &mut Context<Self>) -> Div {
        let strings = self.strings();
        let switch = div()
            .flex_none()
            .flex()
            .gap(px(2.))
            .p(px(3.))
            .rounded(px(9.))
            .border_1()
            .border_color(colors.border)
            .bg(colors.editor)
            .children(SideView::ALL.into_iter().enumerate().map(|(index, view)| {
                let count = match view {
                    SideView::SourceControl => self.git_change_count(),
                    _ => 0,
                };
                div()
                    .relative()
                    .flex_1()
                    .flex()
                    .child(
                        Button::new(("side-view", index))
                            .ghost()
                            .small()
                            .flex_1()
                            .icon(Icon::new(side_view_icon(view)))
                            .selected(view == self.layout.side_view)
                            .tooltip(side_view_title(view, strings))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.layout.select_side_view(view);
                                cx.notify();
                            })),
                    )
                    .when(count > 0, |button| {
                        button.child(
                            div()
                                .absolute()
                                .top(px(-4.))
                                .right(px(-2.))
                                .min_w(px(15.))
                                .h(px(15.))
                                .px(px(3.))
                                .rounded_full()
                                .flex()
                                .items_center()
                                .justify_center()
                                .bg(colors.accent)
                                .text_color(cx.theme().primary_foreground)
                                .text_size(px(9.5))
                                .font_weight(FontWeight::SEMIBOLD)
                                .child(if count > 999 {
                                    "999+".to_owned()
                                } else {
                                    count.to_string()
                                }),
                        )
                    })
            }));

        let content: AnyElement = match self.layout.side_view {
            SideView::Explorer if self.project.root.is_some() => self.render_explorer(colors, cx),
            SideView::Explorer => self.render_open_editors(colors, cx).into_any_element(),
            SideView::Search => self.render_search(colors, cx),
            SideView::SourceControl => self.render_source_control(colors, cx),
            SideView::Extensions => self.render_extensions(colors).into_any_element(),
        };

        floating(colors.panel, colors.border)
            .w(px(SIDEBAR_WIDTH))
            .flex_none()
            .p_2()
            .gap_2()
            .child(switch)
            .child(
                // The Explorer scrolls its own list, the other views as a whole.
                div()
                    .id("sidebar-content")
                    .flex_1()
                    .min_h_0()
                    .when(
                        !matches!(self.layout.side_view, SideView::Explorer | SideView::Search)
                            || self.project.root.is_none(),
                        |content| content.overflow_y_scroll(),
                    )
                    .child(content),
            )
    }

    fn render_open_editors(&self, colors: Colors, cx: &mut Context<Self>) -> Div {
        let mut documents: Vec<(String, String, bool)> = self
            .documents
            .values()
            .map(|document| {
                (
                    document.uri.clone(),
                    self.display_name(document),
                    document.modified,
                )
            })
            .collect();
        documents.sort_by_key(|(uri, name, _)| (name.to_lowercase(), uri.clone()));
        let active = self.panes.get(self.active).map(|pane| pane.uri.clone());
        let rows =
            documents
                .into_iter()
                .enumerate()
                .map(|(index, (uri, name, modified))| {
                    let is_active = active.as_deref() == Some(uri.as_str());
                    div()
                        .id(("open-editor", index))
                        .h(px(24.))
                        .px_2()
                        .flex()
                        .items_center()
                        .gap_2()
                        .rounded(px(6.))
                        .text_sm()
                        .text_color(if is_active { colors.text } else { colors.muted })
                        .when(is_active, |row| row.bg(colors.selected))
                        .when(!is_active, |row| row.hover(|row| row.bg(colors.hover)))
                        .cursor_pointer()
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.show_document(&uri, window, cx)
                        }))
                        .child(div().flex_1().min_w_0().truncate().child(name))
                        .when(modified, |row| {
                            row.child(
                                div()
                                    .flex_none()
                                    .size(px(7.))
                                    .rounded_full()
                                    .bg(colors.muted),
                            )
                        })
                });
        div()
            .child(heading(self.strings().open_editors, colors))
            .children(rows)
    }

    fn render_extensions(&self, colors: Colors) -> Div {
        let strings = self.strings();
        let view = div().child(heading(strings.installed_extensions, colors));
        if self.installed.is_empty() {
            return view.child(note(strings.no_extensions.to_owned(), colors));
        }
        view.children(self.installed.iter().map(|extension| {
            div()
                .px_2()
                .py_1p5()
                .flex()
                .flex_col()
                .child(
                    div()
                        .flex()
                        .items_baseline()
                        .gap_2()
                        .child(
                            div()
                                .text_sm()
                                .text_color(colors.text)
                                .child(extension.name.clone()),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(colors.muted)
                                .child(extension.version.clone()),
                        ),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(colors.faint)
                        .truncate()
                        .child(extension.id.clone()),
                )
        }))
    }

    /// What the editors' panel shows with no file open: how to start, and
    /// the recent projects.
    fn render_welcome(&self, colors: Colors, cx: &mut Context<Self>) -> AnyElement {
        let strings = self.strings();
        let start = |id: &'static str, icon: IconName, label: &'static str, keys: &'static str| {
            div()
                .id(id)
                .h(px(32.))
                .px_2()
                .flex()
                .items_center()
                .gap_2p5()
                .rounded(px(8.))
                .text_sm()
                .text_color(colors.text)
                .cursor_pointer()
                .hover(|row| row.bg(colors.hover))
                .child(Icon::new(icon).small().text_color(colors.accent))
                .child(label)
                .child(
                    div()
                        .ml_auto()
                        .text_xs()
                        .text_color(colors.faint)
                        .child(keys),
                )
        };
        let recent = crate::recent::projects(cx);
        let recent_rows = recent
            .into_iter()
            .take(8)
            .enumerate()
            .map(|(index, project)| {
                let name = project
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_else(|| project.display().to_string());
                let location = project
                    .parent()
                    .map(crate::recent::short_path)
                    .unwrap_or_default();
                div()
                    .id(("recent-project", index))
                    .h(px(30.))
                    .px_2()
                    .flex()
                    .items_center()
                    .gap_2p5()
                    .rounded(px(8.))
                    .text_sm()
                    .cursor_pointer()
                    .hover(|row| row.bg(colors.hover))
                    .on_click(cx.listener(move |_, _, window, cx| {
                        window.dispatch_action(
                            Box::new(OpenProject {
                                path: project.clone(),
                            }),
                            cx,
                        )
                    }))
                    .child(Icon::new(IconName::Folder).small().text_color(colors.muted))
                    .child(div().text_color(colors.text).child(name))
                    .child(
                        div()
                            .min_w_0()
                            .truncate()
                            .text_xs()
                            .text_color(colors.faint)
                            .child(location),
                    )
            });
        let recent_rows: Vec<_> = recent_rows.collect();

        div()
            .id("welcome")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .child(
                div()
                    .w(px(400.))
                    .max_w_full()
                    .px_4()
                    .flex()
                    .flex_col()
                    .gap_6()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_3()
                            .child(
                                svg()
                                    .path(crate::assets::LOGO)
                                    .size(px(40.))
                                    .text_color(colors.muted),
                            )
                            .child(
                                div()
                                    .text_xl()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(colors.text)
                                    .child(if self.project.root.is_some() {
                                        self.project.name.clone()
                                    } else {
                                        "Keel".to_owned()
                                    }),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .child(
                                start(
                                    "welcome-open",
                                    IconName::FolderOpen,
                                    strings.welcome_open,
                                    "⌘O",
                                )
                                .on_click(cx.listener(
                                    |_, _, window, cx| window.dispatch_action(Box::new(Open), cx),
                                )),
                            )
                            .child(
                                start(
                                    "welcome-new-file",
                                    IconName::FilePlus,
                                    strings.welcome_new_file,
                                    "⌘N",
                                )
                                .on_click(
                                    cx.listener(|this, _, window, cx| this.new_file(window, cx)),
                                ),
                            ),
                    )
                    .when(!recent_rows.is_empty(), |welcome| {
                        welcome.child(
                            div()
                                .flex()
                                .flex_col()
                                .child(heading(strings.recent_projects, colors))
                                .children(recent_rows),
                        )
                    }),
            )
            .into_any_element()
    }

    /// A tab: pinned, preview in italics, a dot for unsaved changes, and a
    /// close button on the tab in view or under the pointer.
    fn render_tab(
        &self,
        group: usize,
        index: usize,
        tab: &super::tabs::Tab,
        colors: Colors,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        use super::tabs::{TabAction, TabCommand};
        let strings = self.strings();
        let (name, modified) = self
            .documents
            .get(&tab.uri)
            .map(|document| (self.display_name(document), document.modified))
            .unwrap_or_default();
        let in_view = self.groups[group].active == index;
        let focused_group = group == self.active;
        let hover_group: gpui::SharedString = format!("tab-{group}-{index}").into();
        let close = Button::new(("close-tab", group * 1000 + index))
            .ghost()
            .xsmall()
            .icon(Icon::new(if tab.pinned {
                IconName::Pin
            } else {
                IconName::X
            }))
            .tooltip(if tab.pinned {
                strings.tab_unpin
            } else {
                strings.close_editor
            })
            .on_click(cx.listener(move |this, _, window, cx| {
                let command = if this.groups[group].tabs[index].pinned {
                    TabCommand::TogglePin
                } else {
                    TabCommand::Close
                };
                this.run_tab_command(
                    &TabAction {
                        group,
                        tab: index,
                        command,
                    },
                    window,
                    cx,
                )
            }));
        let marker = if modified && !tab.pinned {
            div()
                .size(px(16.))
                .flex()
                .items_center()
                .justify_center()
                .group_hover(hover_group.clone(), |marker| marker.invisible())
                .child(div().size(px(7.)).rounded_full().bg(colors.muted))
                .into_any_element()
        } else {
            div().into_any_element()
        };
        let focus = self.focus_handle.clone();
        let menu_entry = move |label: &'static str, command: TabCommand| {
            (
                label,
                Box::new(TabAction {
                    group,
                    tab: index,
                    command,
                }) as Box<dyn gpui::Action>,
            )
        };
        let pin_label = if tab.pinned {
            strings.tab_unpin
        } else {
            strings.tab_pin
        };
        let entries = [
            menu_entry(strings.close_editor, TabCommand::Close),
            menu_entry(strings.tab_close_others, TabCommand::CloseOthers),
            menu_entry(strings.tab_close_right, TabCommand::CloseToTheRight),
            menu_entry(strings.tab_close_all, TabCommand::CloseAll),
            menu_entry(pin_label, TabCommand::TogglePin),
            menu_entry(strings.tab_split, TabCommand::Split),
            menu_entry(strings.explorer_copy_path, TabCommand::CopyPath),
            menu_entry(strings.explorer_reveal, TabCommand::Reveal),
        ];
        div()
            .id(("tab", group * 1000 + index))
            .group(hover_group.clone())
            .flex_none()
            .h(px(28.))
            .pl_3()
            .pr_1()
            .flex()
            .items_center()
            .gap_1()
            .rounded(px(8.))
            .text_sm()
            .text_color(if in_view { colors.text } else { colors.muted })
            .when(in_view && focused_group, |tab| tab.bg(colors.selected))
            .when(in_view && !focused_group, |tab| tab.bg(colors.hover))
            .when(!in_view, |tab| tab.hover(|tab| tab.bg(colors.hover)))
            .cursor_pointer()
            .on_click(
                cx.listener(move |this, event: &gpui::ClickEvent, window, cx| {
                    if event.click_count() >= 2
                        && let Some(tab) = this
                            .groups
                            .get_mut(group)
                            .and_then(|g| g.tabs.get_mut(index))
                    {
                        tab.preview = false;
                    }
                    this.activate_tab(group, index, true, window, cx)
                }),
            )
            .on_mouse_down(
                gpui::MouseButton::Middle,
                cx.listener(move |this, _, window, cx| this.close_tab(group, index, window, cx)),
            )
            .context_menu(move |menu, _, _| {
                let mut menu = menu.action_context(focus.clone());
                for (position, (label, action)) in entries.iter().enumerate() {
                    if position == 4 || position == 6 {
                        menu = menu.separator();
                    }
                    menu = menu.menu(*label, action.boxed_clone());
                }
                menu
            })
            .when(tab.pinned, |row| {
                row.child(Icon::new(IconName::Pin).xsmall().text_color(colors.faint))
            })
            .child(
                div()
                    .whitespace_nowrap()
                    .when(tab.preview, |name| name.italic())
                    .child(name),
            )
            .child(
                div()
                    .size(px(20.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(marker)
                    .child(
                        div()
                            .absolute()
                            .when(!in_view || (modified && !tab.pinned), |button| {
                                button
                                    .invisible()
                                    .group_hover(hover_group.clone(), |button| button.visible())
                            })
                            .child(close),
                    ),
            )
            .into_any_element()
    }

    /// The path of the file in view, under its tabs; a click on a folder
    /// shows it in the Explorer.
    fn render_breadcrumbs(
        &self,
        group: usize,
        colors: Colors,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let enabled = self
            .settings
            .get("breadcrumbs.enabled")
            .and_then(Value::as_bool)
            .unwrap_or(true);
        let root = self.project.root.as_ref()?;
        let pane = self.panes.get(group)?;
        let path = self.documents.get(&pane.uri)?.disk_path()?.to_path_buf();
        let relative = path.strip_prefix(root).ok()?;
        if !enabled {
            return None;
        }
        let parts: Vec<String> = relative
            .components()
            .map(|part| part.as_os_str().to_string_lossy().into_owned())
            .collect();
        let last = parts.len().saturating_sub(1);
        let mut folder = root.clone();
        let mut crumbs: Vec<AnyElement> = Vec::new();
        for (position, part) in parts.into_iter().enumerate() {
            if position > 0 {
                crumbs.push(
                    Icon::new(IconName::ChevronRight)
                        .xsmall()
                        .text_color(colors.faint)
                        .into_any_element(),
                );
            }
            if position == last {
                crumbs.push(div().text_color(colors.text).child(part).into_any_element());
                break;
            }
            folder.push(&part);
            let target = folder.clone();
            crumbs.push(
                div()
                    .id(("crumb", group * 1000 + position))
                    .cursor_pointer()
                    .hover(|crumb| crumb.text_color(colors.text))
                    .on_click(
                        cx.listener(move |this, _, _, cx| this.show_folder_in_tree(&target, cx)),
                    )
                    .child(part)
                    .into_any_element(),
            );
        }
        Some(
            div()
                .h(px(22.))
                .flex_none()
                .px_3()
                .flex()
                .items_center()
                .gap_1()
                .text_xs()
                .text_color(colors.muted)
                .overflow_hidden()
                .children(crumbs)
                .into_any_element(),
        )
    }

    fn render_editors(&self, colors: Colors, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let panes = self.panes.iter().enumerate().map(|(index, pane)| {
            let tabs: Vec<AnyElement> = self.groups[index]
                .tabs
                .iter()
                .enumerate()
                .map(|(position, tab)| self.render_tab(index, position, tab, colors, cx))
                .collect();
            div()
                .flex_1()
                .min_w_0()
                .h_full()
                .flex()
                .flex_col()
                .when(index > 0, |pane| {
                    pane.border_l_1().border_color(colors.border)
                })
                .child(
                    div()
                        .h(px(38.))
                        .flex_none()
                        .pr_2()
                        .flex()
                        .items_center()
                        .child(
                            div()
                                .id(("tab-strip", index))
                                .h_full()
                                .flex_1()
                                .min_w_0()
                                .px_2()
                                .flex()
                                .items_center()
                                .gap_0p5()
                                .overflow_x_scroll()
                                .children(tabs),
                        )
                        .children(self.render_preview_button(index, cx)),
                )
                .when_some(
                    self.render_breadcrumbs(index, colors, cx),
                    |pane, crumbs| pane.child(crumbs),
                )
                .child(
                    match self
                        .documents
                        .get(&pane.uri)
                        .map(|document| &document.backing)
                    {
                        Some(Backing::Image(path)) => div()
                            .flex_1()
                            .min_h_0()
                            .child(self.render_image(&pane.uri, path, colors, cx)),
                        Some(Backing::Diff(tab)) => div()
                            .flex_1()
                            .min_h_0()
                            .child(self.render_diff(tab, colors, cx)),
                        Some(Backing::Pdf(tab)) => div()
                            .flex_1()
                            .min_h_0()
                            .child(self.render_pdf(&pane.uri, tab, colors, window, cx)),
                        Some(Backing::Preview(tab)) => div()
                            .flex_1()
                            .min_h_0()
                            .child(self.render_markdown_preview(tab, colors, cx)),
                        _ => div()
                            .flex_1()
                            .min_h_0()
                            .relative()
                            .key_context(super::EDITOR_CONTEXT)
                            .child(
                                Editor::new(&pane.editor)
                                    .size_full()
                                    .bordered(false)
                                    .text_size(px(self.font_size)),
                            )
                            .children(self.render_hunk_bar(index, colors, cx))
                            .children(self.render_signature_help(index, colors, cx)),
                    },
                )
        });

        let content = if self.panes.is_empty() {
            self.render_welcome(colors, cx)
        } else {
            div()
                .flex_1()
                .min_h_0()
                .flex()
                .flex_row()
                .children(panes)
                .into_any_element()
        };
        let panel_takes_all = self.layout.panel && self.panel_maximized;
        floating(colors.editor, colors.border)
            .flex_1()
            .min_w_0()
            .when(!panel_takes_all, |main| main.child(content))
            .when(self.layout.panel, |main| {
                main.child(self.render_bottom_panel(colors, cx))
            })
            .on_drag_move::<super::terminals::PanelResize>(
                cx.listener(|this, event, _, cx| this.resize_panel(event, cx)),
            )
    }

    fn render_bottom_panel(&self, colors: Colors, cx: &mut Context<Self>) -> Div {
        let strings = self.strings();
        let tabs = PanelView::ALL
            .into_iter()
            .enumerate()
            .map(|(index, view)| {
                let is_active = view == self.layout.panel_view;
                div()
                    .id(("panel-view", index))
                    .h_full()
                    .flex()
                    .items_center()
                    .text_xs()
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(if is_active { colors.text } else { colors.faint })
                    .when(is_active, |tab| {
                        tab.border_b_2().border_color(colors.accent)
                    })
                    .cursor_pointer()
                    .on_click(cx.listener(move |this, _, window, cx| {
                        if view == PanelView::Terminal {
                            this.show_terminal(window, cx);
                            return;
                        }
                        this.layout.show_panel_view(view);
                        cx.notify();
                    }))
                    .child(panel_view_title(view, strings).to_uppercase())
                    .when(
                        view == PanelView::Problems && !self.problems.files.is_empty(),
                        |tab| {
                            let count = self
                                .problems
                                .files
                                .iter()
                                .map(|(_, problems)| problems.len())
                                .sum::<usize>();
                            tab.child(
                                div()
                                    .ml_1p5()
                                    .px_1p5()
                                    .rounded(px(8.))
                                    .bg(colors.hover)
                                    .text_color(colors.muted)
                                    .child(count.to_string()),
                            )
                        },
                    )
            })
            .collect::<Vec<_>>();
        let content: AnyElement = match self.layout.panel_view {
            PanelView::Output => self.render_output(colors, cx),
            PanelView::Problems => self.render_problems(colors, cx),
            PanelView::Terminal => match self.render_terminals(colors) {
                Some(terminal) => terminal,
                None => note(strings.no_terminal.to_owned(), colors).into_any_element(),
            },
            PanelView::Ports => self.render_ports(colors, cx),
        };
        let fills = self.layout.panel_view == PanelView::Terminal;
        let maximized = self.panel_maximized;
        div()
            .relative()
            .when(maximized, |panel| panel.flex_1().min_h_0())
            .when(!maximized, |panel| {
                panel.h(px(self.panel_height)).flex_none()
            })
            .flex()
            .flex_col()
            .border_t_1()
            .border_color(colors.border)
            .child(
                div()
                    .h(px(34.))
                    .flex_none()
                    .px_3p5()
                    .flex()
                    .items_center()
                    .gap_5()
                    .children(tabs)
                    .map(|header| match self.layout.panel_view {
                        PanelView::Terminal => {
                            header.child(self.render_terminal_header(colors, cx))
                        }
                        PanelView::Output => header.child(self.render_output_header(cx)),
                        _ => header.child(div().ml_auto()),
                    })
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_1()
                            .child(
                                Button::new("maximize-panel")
                                    .ghost()
                                    .xsmall()
                                    .icon(Icon::new(if maximized {
                                        IconName::Minimize
                                    } else {
                                        IconName::Maximize
                                    }))
                                    .tooltip(if maximized {
                                        strings.restore_panel
                                    } else {
                                        strings.maximize_panel
                                    })
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.toggle_maximized_panel(cx)
                                    })),
                            )
                            .child(
                                Button::new("close-panel")
                                    .ghost()
                                    .xsmall()
                                    .icon(Icon::new(IconName::X))
                                    .tooltip(strings.toggle_panel)
                                    .on_click(cx.listener(|this, _, _, cx| this.toggle_panel(cx))),
                            ),
                    ),
            )
            .when(!maximized, |panel| {
                panel.child(self.render_panel_resize(colors))
            })
            .child(if fills {
                // A terminal fills the panel and scrolls on its own.
                div().id("panel-content").flex_1().min_h_0().child(content)
            } else {
                div()
                    .id("panel-content")
                    .flex_1()
                    .min_h_0()
                    .px_1p5()
                    .overflow_y_scroll()
                    .child(content)
            })
    }

    fn render_agent(&self, colors: Colors, cx: &mut Context<Self>) -> Div {
        let strings = self.strings();
        let terminal = self.render_agent_terminal();
        let running = terminal.is_some();
        let header = div()
            .h(px(44.))
            .flex_none()
            .px_3p5()
            .flex()
            .items_center()
            .gap_2()
            .border_b_1()
            .border_color(colors.border)
            .child(div().size(px(7.)).rounded_full().bg(if running {
                cx.theme().success
            } else {
                colors.faint
            }))
            .child(
                div()
                    .text_sm()
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(colors.text)
                    .child("Claude Code"),
            )
            .child(div().ml_auto().map(|right| {
                if running {
                    right.child(
                        Button::new("close-claude-code")
                            .ghost()
                            .xsmall()
                            .icon(Icon::new(IconName::X))
                            .tooltip(strings.close_claude_code)
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.close_agent_terminal(window, cx)
                            })),
                    )
                } else {
                    right
                        .text_xs()
                        .text_color(colors.muted)
                        .child(strings.agent_not_connected)
                }
            }));
        let body = match terminal {
            Some(terminal) => div().flex_1().min_h_0().child(terminal),
            None => div()
                .flex_1()
                .min_h_0()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap_3()
                .px_6()
                .text_color(colors.faint)
                .child(Icon::new(IconName::Sparkles).large())
                .child(
                    Button::new("open-claude-code")
                        .primary()
                        .small()
                        .label(strings.open_claude_code)
                        .on_click(
                            cx.listener(|this, _, window, cx| this.open_claude_code(window, cx)),
                        ),
                )
                .child(
                    div()
                        .text_xs()
                        .text_center()
                        .text_color(colors.muted)
                        .child(strings.claude_code_hint),
                ),
        };
        div()
            .relative()
            .w(px(self.agent_width))
            .flex_none()
            .h_full()
            .child(
                floating(colors.panel, colors.border)
                    .size_full()
                    .child(header)
                    .child(body),
            )
            .child(self.render_agent_resize(colors))
    }
    pub(super) fn status_texts(&self, cx: &App) -> (Vec<String>, Vec<String>) {
        let strings = self.strings();
        let (mut left, mut right) = (Vec::new(), Vec::new());
        if let Some(branch) = &self.project.branch {
            left.push(branch.clone());
        }
        if let Some(extensions) = &self.extensions {
            let mut items: Vec<&StatusItem> = extensions
                .status_items
                .values()
                .filter(|item| item.visible)
                .collect();
            items.sort_by(|a, b| b.priority.total_cmp(&a.priority));
            for item in items {
                let text = strip_codicons(&item.text);
                if item.right {
                    right.push(text);
                } else {
                    left.push(text);
                }
            }
        }
        if let Some(message) = &self.status_message {
            left.push(message.clone());
        }
        // A server at work, such as rust-analyzer indexing.
        if let Some(progress) = self.languages.progress_text() {
            left.push(progress.to_owned());
        }
        let image = self.panes.get(self.active).is_some_and(|pane| {
            matches!(
                self.documents
                    .get(&pane.uri)
                    .map(|document| &document.backing),
                Some(Backing::Image(_) | Backing::Diff(_) | Backing::Preview(_) | Backing::Pdf(_))
            )
        });
        if let Some(uri) = self.active_pdf()
            && let Some(Backing::Pdf(tab)) =
                self.documents.get(&uri).map(|document| &document.backing)
        {
            right.push(strings.pdf_page_of(tab.current_page() + 1, tab.page_count()));
        }
        // An image: its size and its zoom, as VS Code shows them.
        if let Some((size, zoom)) = self.image_status() {
            right.push(size);
            right.push(zoom);
        }
        if let Some(pane) = self.panes.get(self.active).filter(|_| !image) {
            let position = pane.editor.read(cx).cursor_position();
            right.push(
                strings.line_column(position.line as usize + 1, position.character as usize + 1),
            );
            let tab_size = self.tab_size();
            right.push(strings.indentation(tab_size.tab_size, tab_size.hard_tabs));
            right.push("UTF-8".to_owned());
            if let Some(document) = self.documents.get(&pane.uri) {
                right.push(language_name(&document.language_id, strings));
            }
        }
        (left, right)
    }

    fn render_status_bar(&self, colors: Colors, cx: &mut Context<Self>) -> Div {
        let (left, right) = self.status_texts(cx);
        let branch = self.project.branch.is_some();
        let sync = self.sync_text();
        let has_repo = self.scm.repo.is_some();
        div()
            .h(px(STATUS_HEIGHT))
            .flex_none()
            .px_4()
            .flex()
            .items_center()
            .justify_between()
            .gap_4()
            .text_xs()
            .text_color(colors.muted)
            .child(div().flex().items_center().gap_4().min_w_0().children({
                let mut items: Vec<AnyElement> = left
                    .into_iter()
                    .enumerate()
                    .map(|(index, text)| {
                        let is_branch = branch && index == 0;
                        div()
                            .id(("status-left", index))
                            .flex()
                            .items_center()
                            .gap_1()
                            .truncate()
                            .when(is_branch, |item| {
                                item.text_color(colors.text)
                                    .child(Icon::new(IconName::GitBranch).xsmall())
                            })
                            .child(text)
                            .when(is_branch && has_repo, |item| {
                                item.cursor_pointer()
                                    .hover(|item| item.text_color(colors.accent))
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.choose_branch(window, cx)
                                    }))
                            })
                            .when_some(sync.clone().filter(|_| is_branch), |item, sync| {
                                item.child(
                                    div()
                                        .id("status-sync")
                                        .ml_2()
                                        .cursor_pointer()
                                        .hover(|item| item.text_color(colors.accent))
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            window.dispatch_action(Box::new(super::GitSync), cx);
                                            let _ = this;
                                        }))
                                        .child(sync),
                                )
                            })
                            .into_any_element()
                    })
                    .collect();
                // Errors and warnings come right after the branch, as in VS Code.
                let at = usize::from(branch).min(items.len());
                items.insert(at, self.render_problem_counts(colors, cx));
                items
            }))
            .child(
                div()
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap_4()
                    .children(right),
            )
    }
}

impl Render for Workbench {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !self.first_frame_drawn {
            self.first_frame_drawn = true;
            // Only the first window counts: later ones open long after start.
            static TIMED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
            if std::env::var_os("KEEL_STARTUP_TIMING").is_some()
                && !TIMED.swap(true, std::sync::atomic::Ordering::Relaxed)
            {
                window.on_next_frame(|_, _| {
                    if let Some(started) = crate::STARTED.get() {
                        eprintln!("keel: first frame after {:?}", started.elapsed());
                    }
                });
            }
        }
        let colors = Colors::new(cx);
        let toolbar = self.render_toolbar(colors, window, cx);
        let body = div()
            .flex_1()
            .min_h_0()
            .flex()
            .flex_row()
            .gap(px(GAP))
            .px(px(GAP))
            .pb(px(GAP))
            .when(self.layout.sidebar, |body| {
                body.child(self.render_sidebar(colors, cx))
            })
            .child(self.render_editors(colors, window, cx))
            .when(self.layout.agent, |body| {
                body.child(self.render_agent(colors, cx))
            })
            .on_drag_move::<super::claude_code::AgentResize>(
                cx.listener(|this, event, _, cx| this.resize_agent(event, cx)),
            );
        let status = self.render_status_bar(colors, cx);

        let root = div().track_focus(&self.focus_handle);
        let root = self.tab_actions(root, cx);
        let root = self.terminal_actions(root, cx);
        let root = self.npm_actions(root, cx);
        let root = self.source_control_actions(root, cx);
        let root = self.git_gutter_actions(root, cx);
        let root = self.language_server_actions(root, cx);
        let root = self.problem_actions(root, cx);
        let root = self.snippet_actions(root, cx);
        let root = self.signature_actions(root, cx);
        let root = self.code_navigation_actions(root, cx);
        let root = self.code_action_actions(root, cx);
        let root = self.markdown_preview_actions(root, cx);
        let root = self.pdf_actions(root, cx);
        let root = self.editing_actions(root, cx);
        self.explorer_actions(root, cx)
            .size_full()
            .flex()
            .flex_col()
            .bg(colors.desk)
            .text_color(colors.text)
            .on_action(cx.listener(Self::save))
            .on_action(cx.listener(Self::close_window))
            .on_action(cx.listener(Self::quit))
            .on_action(cx.listener(Self::show_commands))
            .on_action(
                cx.listener(|this, _: &SelectTheme, window, cx| this.choose_theme(window, cx)),
            )
            .on_action(
                cx.listener(|this, _: &SelectLanguage, window, cx| {
                    this.choose_language(window, cx)
                }),
            )
            .on_action(cx.listener(|this, _: &ToggleSidebar, _, cx| this.toggle_sidebar(cx)))
            .on_action(cx.listener(|this, _: &TogglePanel, _, cx| this.toggle_panel(cx)))
            .on_action(cx.listener(|this, _: &ToggleAgent, _, cx| this.toggle_agent(cx)))
            .on_action(cx.listener(|this, _: &ShowExplorer, _, cx| {
                this.show_side_view(SideView::Explorer, cx)
            }))
            .on_action(
                cx.listener(|this, _: &ShowSearch, window, cx| this.focus_search(window, cx)),
            )
            .on_action(cx.listener(|this, _: &ShowSourceControl, _, cx| {
                this.show_side_view(SideView::SourceControl, cx)
            }))
            .on_action(cx.listener(|this, _: &ShowExtensions, _, cx| {
                this.show_side_view(SideView::Extensions, cx)
            }))
            .on_action(cx.listener(|this, _: &ToggleProblems, _, cx| {
                this.toggle_panel_view(PanelView::Problems, cx)
            }))
            .on_action(cx.listener(|this, _: &ToggleOutput, _, cx| {
                this.toggle_panel_view(PanelView::Output, cx)
            }))
            .on_action(
                cx.listener(|this, _: &ToggleTerminal, window, cx| {
                    this.toggle_terminal(window, cx)
                }),
            )
            .on_action(cx.listener(|this, _: &NewFile, window, cx| this.new_file(window, cx)))
            .on_action(cx.listener(|this, _: &QuickOpen, window, cx| this.quick_open(window, cx)))
            .on_action(cx.listener(|this, _: &GoToLine, window, cx| this.ask_line(window, cx)))
            .on_action(
                cx.listener(|this, _: &OpenRecent, window, cx| this.choose_recent(window, cx)),
            )
            .child(toolbar)
            .child(body)
            .child(status)
    }
}

#[cfg(test)]
mod tests {
    use gpui::{Keystroke, TestAppContext, VisualTestContext};
    use gpui_component::input::Replace;

    use super::super::tests::test_settings;
    use super::*;
    use crate::i18n::{ENGLISH, ITALIAN};

    fn set_up(cx: &mut TestAppContext) {
        cx.update(|cx| {
            gpui_component::init(cx);
            language::register_grammars();
            crate::workbench_window::init(cx);
        });
    }

    fn english(folder: &std::path::Path) -> Settings {
        let file = folder.join("settings.json");
        std::fs::write(&file, r#"{ "keel.language": "en" }"#).unwrap();
        Settings::load(&file, None)
    }

    fn layout(view: &Entity<Workbench>, cx: &mut VisualTestContext) -> Layout {
        view.read_with(cx, |view, _| view.layout)
    }

    #[gpui::test]
    fn the_shortcuts_show_and_hide_the_panels(cx: &mut TestAppContext) {
        set_up(cx);
        let (view, cx) = cx
            .add_window_view(|window, cx| Workbench::new(None, test_settings(), None, window, cx));
        assert_eq!(layout(&view, cx), Layout::default());

        cx.simulate_keystrokes("cmd-b");
        assert!(!layout(&view, cx).sidebar);
        cx.simulate_keystrokes("cmd-j");
        assert!(layout(&view, cx).panel);
        cx.simulate_keystrokes("cmd-alt-b");
        assert!(!layout(&view, cx).agent);

        cx.simulate_keystrokes("cmd-shift-u");
        assert_eq!(layout(&view, cx).panel_view, PanelView::Output);
        assert!(layout(&view, cx).panel);
        cx.simulate_keystrokes("cmd-shift-u");
        assert!(!layout(&view, cx).panel);

        cx.simulate_keystrokes("cmd-shift-x");
        assert!(layout(&view, cx).sidebar);
        assert_eq!(layout(&view, cx).side_view, SideView::Extensions);
        cx.simulate_keystrokes("cmd-shift-e");
        assert_eq!(layout(&view, cx).side_view, SideView::Explorer);
        // Typing goes to the editor once a file is open.
        cx.simulate_keystrokes("cmd-n");
        cx.simulate_input("ok");
        assert!(view.read_with(cx, |view, _| view.has_unsaved_changes()));
    }

    #[gpui::test]
    fn cmd_shift_f_searches_the_project_and_replace_moves_to_cmd_alt_f(cx: &mut TestAppContext) {
        set_up(cx);
        let (view, cx) = cx
            .add_window_view(|window, cx| Workbench::new(None, test_settings(), None, window, cx));
        view.update(cx, |view, cx| {
            view.layout.select_side_view(SideView::Extensions);
            cx.notify();
        });

        cx.simulate_keystrokes("cmd-shift-f");
        assert_eq!(layout(&view, cx).side_view, SideView::Search);

        let replace_keys: Vec<Keystroke> = cx.update(|_, cx| {
            cx.key_bindings()
                .borrow()
                .bindings_for_action(&Replace)
                .filter_map(|binding| binding.keystrokes().first())
                .map(|keystroke| keystroke.inner().clone())
                .collect()
        });
        assert!(replace_keys.contains(&Keystroke::parse("cmd-alt-f").unwrap()));
        assert!(!replace_keys.contains(&Keystroke::parse("cmd-shift-f").unwrap()));
    }

    #[gpui::test]
    fn the_status_bar_shows_branch_cursor_indentation_and_language(cx: &mut TestAppContext) {
        set_up(cx);
        let folder = tempfile::tempdir().unwrap();
        let root = folder.path().join("GestionaleHospitality");
        std::fs::create_dir_all(root.join(".git")).unwrap();
        std::fs::write(root.join(".git/HEAD"), "ref: refs/heads/develop\n").unwrap();
        let path = root.join("app.ts");
        std::fs::write(&path, "let a = 1;\nlet total = 2;\n").unwrap();
        let document = Document::open(&path).unwrap();
        let settings = english(folder.path());

        let (view, cx) = cx.add_window_view(|window, cx| {
            Workbench::new(Some(document), settings, None, window, cx)
        });
        cx.simulate_keystrokes("down end");

        let (left, right) = view.read_with(cx, |view, cx| view.status_texts(cx));
        assert_eq!(left, ["develop"]);
        assert_eq!(right, ["Ln 2, Col 15", "Spaces: 2", "UTF-8", "TypeScript"]);
        view.read_with(cx, |view, _| {
            assert_eq!(view.project.name, "GestionaleHospitality");
        });
    }

    #[gpui::test]
    fn problems_with_the_settings_reach_the_output_view(cx: &mut TestAppContext) {
        set_up(cx);
        let folder = tempfile::tempdir().unwrap();
        let file = folder.path().join("settings.json");
        std::fs::write(&file, "{ \"editor.fontSize\": }").unwrap();
        let settings = Settings::load(&file, None);

        let (view, cx) =
            cx.add_window_view(|window, cx| Workbench::new(None, settings, None, window, cx));
        let lines = |view: &Workbench| -> Vec<String> {
            view.output.iter().map(|(_, line)| line.clone()).collect()
        };
        let output = view.read_with(cx, |view, _| lines(view));
        assert_eq!(output.len(), 1);
        assert!(output[0].contains("settings.json"), "{output:?}");

        view.update(cx, |view, _| {
            for line in 0..OUTPUT_LINES + 5 {
                view.log(format!("line {line}"));
            }
        });
        let output = view.read_with(cx, |view, _| lines(view));
        assert_eq!(output.len(), OUTPUT_LINES);
        assert_eq!(
            output.last().unwrap(),
            &format!("line {}", OUTPUT_LINES + 4)
        );
    }

    #[gpui::test]
    fn open_editors_keeps_closed_files_and_shows_them_again(cx: &mut TestAppContext) {
        set_up(cx);
        let folder = tempfile::tempdir().unwrap();
        let path = folder.path().join("page.tsx");
        std::fs::write(&path, "export {};\n").unwrap();
        let document = Document::open(&path).unwrap();
        let settings = english(folder.path());
        let (view, cx) = cx.add_window_view(|window, cx| {
            Workbench::new(Some(document), settings, None, window, cx)
        });
        let file_uri = view.read_with(cx, |view, _| view.panes[0].uri.clone());

        let untitled = view.update_in(cx, |view, window, cx| {
            let uri = view.add_untitled_document();
            view.show_document(&uri, window, cx);
            uri
        });
        view.read_with(cx, |view, _| {
            assert_eq!(view.panes.len(), 1);
            assert_eq!(view.panes[0].uri, untitled);
            assert!(view.documents.contains_key(&file_uri));
        });

        view.update_in(cx, |view, window, cx| {
            view.show_document(&file_uri, window, cx)
        });
        view.read_with(cx, |view, _| assert_eq!(view.panes[0].uri, file_uri));
    }

    #[gpui::test]
    fn an_empty_window_welcomes_and_cmd_n_opens_a_file(cx: &mut TestAppContext) {
        set_up(cx);
        let folder = tempfile::tempdir().unwrap();
        let settings = english(folder.path());
        let (view, cx) =
            cx.add_window_view(|window, cx| Workbench::new(None, settings, None, window, cx));
        view.read_with(cx, |view, _| {
            assert!(view.is_blank());
            assert!(view.panes.is_empty());
            assert_eq!(view.window_title(), "Keel");
        });

        cx.simulate_keystrokes("cmd-n");
        view.read_with(cx, |view, _| {
            assert_eq!(view.panes.len(), 1);
            assert_eq!(view.window_title(), "Untitled 1");
        });

        view.update_in(cx, |view, window, cx| view.close_pane(0, window, cx));
        view.read_with(cx, |view, _| assert!(view.panes.is_empty()));
        // The window still answers shortcuts with no editor in it.
        cx.simulate_keystrokes("cmd-b");
        assert!(!layout(&view, cx).sidebar);
    }

    #[gpui::test]
    fn a_project_window_opens_its_files_once(cx: &mut TestAppContext) {
        set_up(cx);
        let folder = tempfile::tempdir().unwrap();
        let root = folder.path().join("Silo");
        std::fs::create_dir_all(root.join("src")).unwrap();
        let file = root.join("src/main.rs");
        std::fs::write(&file, "fn main() {}\n").unwrap();
        let settings = english(folder.path());

        let opening = Opening::new(Some(root.clone()), Some(file.clone()));
        let (view, cx) =
            cx.add_window_view(|window, cx| Workbench::new(opening, settings, None, window, cx));
        view.read_with(cx, |view, _| {
            assert!(!view.is_blank());
            assert_eq!(view.window_title(), "main.rs — Silo");
            assert_eq!(view.documents.len(), 1);
        });

        view.update_in(cx, |view, window, cx| {
            view.open_file(&file, window, cx);
            view.open_file(&root.join("src"), window, cx);
        });
        view.read_with(cx, |view, _| {
            assert_eq!(view.documents.len(), 1);
            assert_eq!(view.panes.len(), 1);
            let (_, last) = view.output.last().unwrap();
            assert!(last.starts_with("Keel could not open"), "{last}");
        });
    }

    #[test]
    fn languages_have_their_names() {
        assert_eq!(
            language_name("typescriptreact", &ENGLISH),
            "TypeScript React"
        );
        assert_eq!(language_name("plaintext", &ITALIAN), "Testo normale");
        assert_eq!(language_name("zig", &ENGLISH), "zig");
    }
}
