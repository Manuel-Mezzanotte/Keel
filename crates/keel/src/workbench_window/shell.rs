//! The window's frame, in either style of [`super::look`]: the toolbar
//! beside the traffic lights, the sidebar, the editors with the bottom
//! panel, Claude Code on the right and the status bar. In the floating
//! style they are rounded panels on the window's background (sketch D); in
//! the flat one they touch, divided by hairlines.

use gpui::{
    Animation, AnimationExt as _, AnyElement, Bounds, Div, FontWeight, OperatingSystem, Pixels,
    Render, SharedString, StatefulInteractiveElement as _, TitlebarOptions, WindowBounds,
    WindowOptions, relative, size, svg,
};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::menu::ContextMenuExt as _;
use gpui_component::{Icon, Selectable as _, Sizable as _, TitleBar};
use gpui_kit_assets::IconName;

use super::look::{self, Look};
use super::*;

/// The sidebar's width at first, and the range it can be dragged to.
pub(super) const SIDEBAR_WIDTH: f32 = 240.;
const SIDEBAR_MIN_WIDTH: f32 = 180.;
const SIDEBAR_MAX_WIDTH: f32 = 600.;
/// How long the sidebar and the panels take to slide in and out.
const SLIDE_MS: u64 = 220;

/// The handle on the sidebar's right edge.
#[derive(Clone)]
pub(super) struct SidebarResize;

impl Render for SidebarResize {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        gpui::Empty
    }
}
/// Room for the traffic lights before the toolbar's content.
const TRAFFIC_LIGHTS_WIDTH: f32 = 88.;

/// The window: the toolbar is drawn by Keel, with macOS's traffic lights
/// centred in it.
pub(crate) fn window_options(bounds: Bounds<Pixels>, look: Look) -> WindowOptions {
    WindowOptions {
        titlebar: Some(TitlebarOptions {
            title: Some("Keel".into()),
            appears_transparent: true,
            traffic_light_position: Some(look.traffic_lights()),
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
    /// A dark theme, for the shades that depend on it.
    pub(super) dark: bool,
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
            dark: theme.is_dark(),
        }
    }

    /// The same colors with the window's background of `look`.
    pub(super) fn with_look(mut self, look: Look) -> Self {
        self.desk = look.desk(self, self.dark);
        self
    }
}

/// One of the window's panels: rounded and floating on the window's
/// background, or flat, touching its neighbours.
fn panel(look: Look, background: Hsla, colors: Colors, dark: bool) -> Div {
    div()
        .h_full()
        .flex()
        .flex_col()
        .overflow_hidden()
        .bg(background)
        .when(!look.flat(), |panel| {
            panel
                .rounded(px(look.radius()))
                .border_1()
                .border_color(colors.border)
                .shadow(look.panel_shadow(dark))
        })
}

pub(super) fn heading(text: &str, colors: Colors) -> Div {
    // One line always: a narrow sidebar cuts the title, it never wraps it.
    div()
        .px_2()
        .pt_1()
        .pb_1()
        .min_w_0()
        .truncate()
        .text_xs()
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(colors.faint)
        .child(text.to_uppercase())
}

/// The title row on top of a view of the sidebar: its name on the left,
/// its buttons on the right, as tall in every view.
pub(super) fn view_header(title: &str, colors: Colors) -> Div {
    div()
        .flex_none()
        .h(px(30.))
        .pl_2()
        .flex()
        .items_center()
        .gap_0p5()
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .text_xs()
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(colors.faint)
                .child(title.to_uppercase()),
        )
}

/// A key in the style macOS draws them in menus, such as `⌘P`.
pub(super) fn key_hint(keys: impl Into<SharedString>, colors: Colors) -> Div {
    div()
        .flex_none()
        .h(px(18.))
        .px(px(5.))
        .flex()
        .items_center()
        .rounded(px(5.))
        .border_1()
        .border_color(colors.border)
        .text_size(px(11.))
        .text_color(colors.faint)
        .child(keys.into())
}

/// An empty view's picture and words: a symbol in a soft circle, a title
/// and a line that says what to do.
pub(super) fn empty_state(icon: IconName, title: &str, hint: &str, colors: Colors) -> Div {
    div()
        .flex_none()
        .px_4()
        .py_5()
        .flex()
        .flex_col()
        .items_center()
        .gap_2()
        .text_center()
        .child(
            div()
                .size(px(44.))
                .rounded_full()
                .flex()
                .items_center()
                .justify_center()
                .bg(colors.accent.opacity(0.12))
                .child(Icon::new(icon).size(px(20.)).text_color(colors.accent)),
        )
        .child(
            div()
                .pt_1()
                .text_sm()
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(colors.text)
                .child(title.to_owned()),
        )
        .child(
            div()
                .text_xs()
                .text_color(colors.muted)
                .child(hint.to_owned()),
        )
}

/// A row that runs a command, with its symbol and its keys: for the tools
/// the empty views offer.
pub(super) fn tool_row(
    id: impl Into<gpui::ElementId>,
    icon: IconName,
    label: &str,
    keys: Option<String>,
    colors: Colors,
) -> gpui::Stateful<Div> {
    div()
        .id(id)
        .h(px(30.))
        .px_2()
        .flex()
        .items_center()
        .gap_2()
        .rounded(px(7.))
        .text_sm()
        .text_color(colors.text)
        .cursor_pointer()
        .hover(|row| row.bg(colors.hover))
        .active(|row| row.bg(colors.selected))
        .child(Icon::new(icon).small().text_color(colors.muted))
        .child(div().flex_1().min_w_0().truncate().child(label.to_owned()))
        .children(keys.map(|keys| key_hint(keys, colors)))
}

/// The keys of an action as people read them on a Mac, such as `⇧⌘O`: the
/// ones the window answers to now, the user's included.
pub(super) fn keys_for(action: &dyn gpui::Action, window: &Window) -> Option<String> {
    let binding = window.highest_precedence_binding_for_action(action)?;
    let text = binding
        .keystrokes()
        .iter()
        .map(|keystroke| keystroke.inner().unparse(OperatingSystem::Mac))
        .collect::<Vec<_>>()
        .join(" ");
    Some(super::keybindings_view::readable(&text))
}

/// How far a section's chevron has turned, 0 closed to 1 open, gliding
/// when the section opens or closes.
pub(super) fn chevron_turn(
    id: impl Into<gpui::ElementId>,
    open: bool,
    look: Look,
    window: &mut Window,
    cx: &mut App,
) -> f32 {
    look::glide(
        id,
        if open { 1. } else { 0. },
        look.duration(180),
        window,
        cx,
    )
}

/// The header of a section of the sidebar, as Finder's: a chevron that
/// closes and opens it (the whole row is the button), turned `turn` of the
/// way from pointing right (closed) to pointing down (open), the title, and
/// on the right whatever actions the section has.
pub(super) fn section_header(
    id: impl Into<gpui::ElementId>,
    title: &str,
    turn: f32,
    colors: Colors,
) -> gpui::Stateful<Div> {
    div()
        .id(id)
        .h(px(26.))
        .px_1()
        .flex()
        .items_center()
        .gap_1()
        .rounded(px(6.))
        .cursor_pointer()
        .hover(|row| row.bg(colors.hover))
        .text_xs()
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(colors.faint)
        .child(
            Icon::new(IconName::ChevronRight)
                .xsmall()
                .rotate(gpui::radians(
                    turn.clamp(0., 1.) * std::f32::consts::FRAC_PI_2,
                ))
                .text_color(colors.faint),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .child(title.to_uppercase()),
        )
}

/// Buttons inside a clickable row, such as a section's header: their
/// clicks stay theirs and do not also close the section.
pub(super) fn own_clicks(id: impl Into<gpui::ElementId>) -> gpui::Stateful<Div> {
    div()
        .id(id)
        .flex()
        .items_center()
        .on_click(|_, _, cx| cx.stop_propagation())
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
        "cuda-cpp" => "CUDA C++",
        "sql" => "SQL",
        "makefile" => "Makefile",
        "xml" => "XML",
        "dockerfile" => "Dockerfile",
        "properties" => "Properties",
        "spring-boot-properties" => "Spring Boot Properties",
        "spring-boot-properties-yaml" => "Spring Boot Properties YAML",
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
        let look = self.look;
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
                            .line_height(px(17.))
                            .child(self.project.name.clone()),
                    )
                    .when_some(self.project.branch.clone(), |project, branch| {
                        project.child(
                            div()
                                .text_xs()
                                .line_height(px(14.))
                                .text_color(colors.muted)
                                .child(branch),
                        )
                    }),
            );

        let has_project = self.project.root.is_some();
        let keys = if has_project {
            keys_for(&QuickOpen, window)
        } else {
            keys_for(&ShowCommands, window)
        };
        let search = div()
            .id("toolbar-search")
            .w_full()
            .max_w(px(if look.flat() { 520. } else { 460. }))
            .h(px(if look.flat() { 28. } else { 32. }))
            .pl_2p5()
            .pr(px(6.))
            .flex()
            .items_center()
            .gap_2()
            .rounded(px(if look.flat() { 7. } else { 9. }))
            .border_1()
            .border_color(colors.border)
            .bg(if look.flat() {
                colors.editor
            } else {
                colors.panel
            })
            .text_sm()
            .text_color(colors.faint)
            .cursor_pointer()
            .hover(|search| {
                search
                    .text_color(colors.muted)
                    .border_color(colors.accent.opacity(0.5))
            })
            .on_click(cx.listener(|this, _, window, cx| this.quick_open(window, cx)))
            .child(Icon::new(IconName::Search).small())
            .child(div().flex_1().min_w_0().truncate().child(if has_project {
                strings.search_files
            } else {
                strings.search_commands
            }))
            .children(keys.map(|keys| key_hint(keys, colors)));

        let toggle = |id: &'static str, icon: IconName, on: bool, tooltip: &'static str| {
            Button::new(id)
                .ghost()
                .with_size(gpui_component::Size::Medium)
                .icon(Icon::new(icon))
                .selected(on)
                .tooltip(tooltip)
        };
        let toggles = div()
            .flex_none()
            .flex()
            .items_center()
            .gap_0p5()
            .child(
                toggle(
                    "toggle-sidebar",
                    IconName::PanelLeft,
                    self.layout.sidebar,
                    strings.toggle_sidebar,
                )
                .on_click(cx.listener(|this, _, _, cx| this.toggle_sidebar(cx))),
            )
            .child(
                toggle(
                    "toggle-panel",
                    IconName::PanelBottom,
                    self.layout.panel,
                    strings.toggle_panel,
                )
                .on_click(cx.listener(|this, _, _, cx| this.toggle_panel(cx))),
            )
            .child(
                toggle(
                    "toggle-agent",
                    IconName::PanelRight,
                    self.layout.agent,
                    strings.toggle_agent,
                )
                .on_click(cx.listener(|this, _, _, cx| this.toggle_agent(cx))),
            );

        let left = if window.is_fullscreen() {
            px(16.)
        } else {
            px(TRAFFIC_LIGHTS_WIDTH)
        };
        let custom = self.custom_colors(cx);
        TitleBar::new()
            .h(px(look.toolbar_height()))
            .pl(left)
            .pr(px(if look.flat() { 8. } else { 10. }))
            .bg(custom.title_background.unwrap_or(colors.desk))
            .when_some(custom.title_foreground, |bar, color| bar.text_color(color))
            .border_color(if look.flat() {
                colors.border
            } else {
                gpui::transparent_black()
            })
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

    /// The switch on top of the sidebar: a capsule with a sliding thumb in
    /// the floating style, a row of tabs with a sliding underline in the
    /// flat one.
    fn render_side_switch(
        &self,
        colors: Colors,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let strings = self.strings();
        let look = self.look;
        let views = SideView::ALL.len() as f32;
        let chosen = SideView::ALL
            .iter()
            .position(|view| *view == self.layout.side_view)
            .unwrap_or(0) as f32;
        // Where the thumb is now, gliding to the chosen view.
        let at = look::glide("side-switch", chosen, look.duration(240), window, cx);
        let segments = SideView::ALL.into_iter().enumerate().map(|(index, view)| {
            let count = match view {
                SideView::SourceControl => self.git_change_count(),
                _ => 0,
            };
            let on = view == self.layout.side_view;
            div()
                .id(("side-view", index))
                .relative()
                .flex_1()
                .h_full()
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(7.))
                .cursor_pointer()
                .text_color(if on { colors.text } else { colors.muted })
                .when(!on, |segment| {
                    segment.hover(|segment| segment.text_color(colors.text))
                })
                .tooltip(move |window, cx| {
                    gpui_component::tooltip::Tooltip::new(side_view_title(view, strings))
                        .build(window, cx)
                })
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.layout.select_side_view(view);
                    cx.notify();
                }))
                .child(Icon::new(side_view_icon(view)).with_size(gpui_component::Size::Medium))
                .when(count > 0, |segment| {
                    segment.child(
                        div()
                            .absolute()
                            .top(px(if look.flat() { 3. } else { 0. }))
                            .left(relative(0.5))
                            .ml(px(5.))
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
        });
        if look.flat() {
            // Tabs as tall as the editors' tab strip, so the lines meet.
            return div()
                .flex_none()
                .h(px(look.bar_height()))
                .px_1()
                .border_b_1()
                .border_color(colors.border)
                .child(
                    div()
                        .relative()
                        .size_full()
                        .flex()
                        .children(segments)
                        .child(
                            div()
                                .absolute()
                                .bottom_0()
                                .h(px(2.))
                                .left(relative(at / views))
                                .w(relative(1. / views))
                                .flex()
                                .justify_center()
                                .child(div().w(px(22.)).h_full().rounded_full().bg(colors.accent)),
                        ),
                );
        }
        div()
            .flex_none()
            .h(px(34.))
            .p(px(3.))
            .rounded(px(10.))
            .bg(colors.desk)
            .border_1()
            .border_color(colors.border.opacity(0.6))
            .child(
                div()
                    .relative()
                    .size_full()
                    .flex()
                    // The thumb under the chosen view, raised as macOS's
                    // segmented controls.
                    .child(
                        div()
                            .absolute()
                            .top_0()
                            .bottom_0()
                            .left(relative(at / views))
                            .w(relative(1. / views))
                            .rounded(px(7.))
                            .bg(colors.selected)
                            .border_1()
                            .border_color(colors.accent.opacity(0.18)),
                    )
                    .children(segments),
            )
    }

    fn render_sidebar(
        &self,
        colors: Colors,
        open: f32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let look = self.look;
        let switch = self.render_side_switch(colors, window, cx);
        let content: AnyElement = match self.layout.side_view {
            SideView::Explorer if self.project.root.is_some() => {
                self.render_explorer(colors, window, cx)
            }
            SideView::Explorer => self.render_open_editors(colors, cx).into_any_element(),
            SideView::Search => self.render_search(colors, window, cx),
            SideView::SourceControl => self.render_source_control(colors, window, cx),
            SideView::Extensions => self.render_extensions(colors, window, cx),
        };
        let view_index = SideView::ALL
            .iter()
            .position(|view| *view == self.layout.side_view)
            .unwrap_or(0);
        // A new view fades in, rising a little.
        let duration = look.duration(180);
        let content = div()
            .id("sidebar-content")
            .flex_1()
            .min_h_0()
            .when(look.flat(), |content| content.px_2().pt_1())
            // The Explorer scrolls its own list, the other views as a whole.
            .when(
                !matches!(
                    self.layout.side_view,
                    SideView::Explorer | SideView::Search | SideView::SourceControl
                ) || self.project.root.is_none(),
                |content| content.overflow_y_scroll().track_scroll(&self.side_scroll),
            )
            .child(content);
        let content: AnyElement = if duration.is_zero() {
            content.into_any_element()
        } else {
            content
                .with_animation(
                    ("sidebar-view-in", view_index),
                    Animation::new(duration).with_easing(look::settle),
                    |content, delta| {
                        content
                            .opacity(0.35 + 0.65 * delta)
                            .mt(px(4. * (1. - delta)))
                    },
                )
                .into_any_element()
        };

        let sidebar = panel(look, colors.panel, colors, colors.dark)
            .debug_selector(|| "sidebar".to_owned())
            .flex_none()
            .w(px(self.sidebar_width))
            .when(!look.flat(), |sidebar| sidebar.p_2().gap_2())
            .when(look.flat(), |sidebar| {
                sidebar.border_r_1().border_color(colors.border)
            })
            .child(switch)
            .child(content);
        // Drag the edge to resize, double-click it to go back to the first
        // width. In the floating style the handle is the gap on the right,
        // as Finder's sidebar edge; in the flat one, the line itself.
        let handle = div()
            .id("sidebar-resize")
            .absolute()
            .top_0()
            .bottom_0()
            .right_0()
            .w(px(if look.flat() { 5. } else { look.gap() }))
            .cursor_col_resize()
            .when(look.flat(), |handle| {
                handle.hover(|handle| handle.bg(colors.accent.opacity(0.35)))
            })
            .on_drag(SidebarResize, |resize, _, _, cx| cx.new(|_| resize.clone()))
            .on_click(cx.listener(|this, event: &gpui::ClickEvent, _, cx| {
                if event.click_count() >= 2 {
                    this.sidebar_width = SIDEBAR_WIDTH;
                    this.session_changed(cx);
                    cx.notify();
                }
            }));
        let full = self.sidebar_width + look.gap();
        // Slides in from the left edge: the room grows, the panel keeps its
        // width and is uncovered from its right side.
        div()
            .h_full()
            .flex_none()
            .w(px(full * open))
            .overflow_hidden()
            .flex()
            .justify_end()
            .child(
                div()
                    .relative()
                    .flex_none()
                    .w(px(full))
                    .h_full()
                    .pr(px(look.gap()))
                    .when(open < 1., |room| room.opacity(0.4 + 0.6 * open))
                    .child(sidebar)
                    .child(handle),
            )
    }

    /// Follows the drag of the sidebar's edge; `bounds` is the row of
    /// panels, which starts one gap from the window's edge.
    pub(super) fn resize_sidebar(
        &mut self,
        event: &gpui::DragMoveEvent<SidebarResize>,
        cx: &mut Context<Self>,
    ) {
        let bounds = event.bounds;
        let width = f32::from(event.event.position.x - bounds.left()) - self.look.gap();
        let room = (f32::from(bounds.size.width) * 0.5).max(SIDEBAR_MIN_WIDTH);
        self.sidebar_width = width.clamp(SIDEBAR_MIN_WIDTH, SIDEBAR_MAX_WIDTH.min(room));
        self.session_changed(cx);
        cx.notify();
    }

    /// The Explorer with no folder open: the files open in the window, and
    /// how to open a folder.
    fn render_open_editors(&self, colors: Colors, cx: &mut Context<Self>) -> Div {
        let strings = self.strings();
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
        let has_documents = !documents.is_empty();
        let rows =
            documents
                .into_iter()
                .enumerate()
                .map(|(index, (uri, name, modified))| {
                    let is_active = active.as_deref() == Some(uri.as_str());
                    div()
                        .id(("open-editor", index))
                        .h(px(26.))
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
            .flex()
            .flex_col()
            .when(has_documents, |view| {
                view.child(view_header(strings.open_editors, colors))
                    .children(rows)
            })
            .child(empty_state(
                IconName::FolderOpen,
                strings.no_folder_title,
                strings.no_folder,
                colors,
            ))
            .child(
                div().px_2().flex().justify_center().child(
                    Button::new("sidebar-open-folder")
                        .primary()
                        .icon(Icon::new(IconName::FolderOpen))
                        .label(strings.open_folder)
                        .on_click(|_, window, cx| window.dispatch_action(Box::new(Open), cx)),
                ),
            )
    }

    /// What the editors' panel shows with no file open: how to start, with
    /// the keys of each way, and the recent projects.
    fn render_welcome(
        &self,
        colors: Colors,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let strings = self.strings();
        let has_project = self.project.root.is_some();
        let start = |id: &'static str,
                     icon: IconName,
                     label: &'static str,
                     action: Box<dyn gpui::Action>| {
            let keys = keys_for(action.as_ref(), window);
            tool_row(id, icon, label, keys, colors)
                .h(px(34.))
                .on_click(move |_, window, cx| window.dispatch_action(action.boxed_clone(), cx))
        };
        let mut actions: Vec<gpui::Stateful<Div>> = Vec::new();
        if has_project {
            actions.push(start(
                "welcome-go-to-file",
                IconName::Search,
                strings.welcome_go_to_file,
                Box::new(QuickOpen),
            ));
        }
        actions.push(start(
            "welcome-open",
            IconName::FolderOpen,
            strings.welcome_open,
            Box::new(Open),
        ));
        actions.push(start(
            "welcome-new-file",
            IconName::FilePlus,
            strings.welcome_new_file,
            Box::new(NewFile),
        ));
        actions.push(start(
            "welcome-commands",
            IconName::Command,
            strings.welcome_commands,
            Box::new(ShowCommands),
        ));
        actions.push(start(
            "welcome-claude",
            IconName::Sparkles,
            strings.welcome_claude,
            Box::new(super::OpenClaudeCode),
        ));
        actions.push(start(
            "welcome-settings",
            IconName::Settings,
            strings.welcome_settings,
            Box::new(OpenSettings),
        ));
        let recent = crate::recent::projects(cx);
        let recent_rows: Vec<_> = recent
            .into_iter()
            .filter(|project| Some(project) != self.project.root.as_ref())
            .take(6)
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
                    .h(px(34.))
                    .px_2()
                    .flex()
                    .items_center()
                    .gap_2()
                    .rounded(px(7.))
                    .text_sm()
                    .cursor_pointer()
                    .hover(|row| row.bg(colors.hover))
                    .active(|row| row.bg(colors.selected))
                    .on_click(cx.listener(move |_, _, window, cx| {
                        window.dispatch_action(
                            Box::new(OpenProject {
                                path: project.clone(),
                            }),
                            cx,
                        )
                    }))
                    .child(Icon::new(IconName::Folder).small().text_color(colors.muted))
                    .child(div().flex_none().text_color(colors.text).child(name))
                    .child(
                        div()
                            .min_w_0()
                            .truncate()
                            .text_xs()
                            .text_color(colors.faint)
                            .child(location),
                    )
            })
            .collect();
        let title = if has_project {
            self.project.name.clone()
        } else {
            "Keel".to_owned()
        };
        let subtitle = match (&self.project.branch, has_project) {
            (Some(branch), true) => branch.clone(),
            _ => strings.welcome_tagline.to_owned(),
        };
        let duration = self.look.duration(260);
        let page = div()
            .w(px(420.))
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
                        div()
                            .size(px(52.))
                            .flex_none()
                            .rounded(px(14.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .bg(colors.accent.opacity(0.12))
                            .child(
                                svg()
                                    .path(crate::assets::LOGO)
                                    .size(px(30.))
                                    .text_color(colors.accent),
                            ),
                    )
                    .child(
                        div()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .child(
                                div()
                                    .text_xl()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(colors.text)
                                    .truncate()
                                    .child(title),
                            )
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(colors.muted)
                                    .truncate()
                                    .child(subtitle),
                            ),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .child(heading(strings.welcome_start, colors))
                    .children(actions),
            )
            .when(!recent_rows.is_empty(), |welcome| {
                welcome.child(
                    div()
                        .flex()
                        .flex_col()
                        .child(heading(strings.recent_projects, colors))
                        .children(recent_rows),
                )
            });
        let page: AnyElement = if duration.is_zero() {
            page.into_any_element()
        } else {
            page.with_animation(
                "welcome-in",
                Animation::new(duration).with_easing(look::settle),
                |page, delta| page.opacity(delta).mt(px(10. * (1. - delta))),
            )
            .into_any_element()
        };
        div()
            .id("welcome")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .child(page)
            .into_any_element()
    }

    /// A tab: pinned, preview in italics, a dot for unsaved changes, and a
    /// close button on the tab in view or under the pointer.
    /// The icon in front of a tab's name: the file's, as in the Explorer,
    /// or a symbol for the pages Keel draws.
    fn tab_icon(&self, document: &OpenDocument, colors: Colors, cx: &App) -> Option<AnyElement> {
        let symbol = |name: IconName| {
            Some(
                Icon::new(name)
                    .small()
                    .flex_none()
                    .text_color(colors.muted)
                    .into_any_element(),
            )
        };
        match &document.backing {
            Backing::Page(super::settings_view::PageKind::Settings) => symbol(IconName::Settings),
            Backing::Page(super::settings_view::PageKind::Keybindings) => {
                symbol(IconName::Keyboard)
            }
            Backing::ExtensionDetails(_) => symbol(IconName::Blocks),
            Backing::Graph(_) => symbol(IconName::GitGraph),
            Backing::Untitled => Some(self.file_icon_element(Path::new("untitled.txt"), cx)),
            _ => {
                let path = document
                    .disk_path()
                    .map(Path::to_path_buf)
                    .unwrap_or_else(|| PathBuf::from(document.name()));
                Some(self.file_icon_element(&path, cx))
            }
        }
    }

    fn render_tab(
        &self,
        group: usize,
        index: usize,
        tab: &super::tabs::Tab,
        colors: Colors,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        use super::tabs::{TabAction, TabCommand};
        let strings = self.strings();
        let (name, modified) = self
            .documents
            .get(&tab.uri)
            .map(|document| (self.display_name(document), document.modified))
            .unwrap_or_default();
        // Errors, warnings and git's colour on the name, as in the Explorer.
        let tinted = self
            .documents
            .get(&tab.uri)
            .and_then(|document| document.disk_path())
            .and_then(|path| match self.problems.marks.get(path) {
                Some((true, _)) => Some(cx.theme().danger),
                Some((false, count)) if *count > 0 => Some(cx.theme().warning),
                _ => self.git_marks.file(path).map(|mark| mark.color(cx)),
            });
        // The dot of unsaved changes fades away when the file is saved,
        // instead of blinking out at each automatic save.
        let dot = look::glide(
            gpui::ElementId::Name(format!("tab-dot-{}", tab.uri).into()),
            if modified { 1. } else { 0. },
            self.look.duration(220),
            window,
            cx,
        );
        let icon = self
            .documents
            .get(&tab.uri)
            .and_then(|document| self.tab_icon(document, colors, cx));
        let in_view = self.groups[group].active == index;
        let focused_group = group == self.active;
        let flat = self.look.flat();
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
        let marker = if dot > 0. && !tab.pinned {
            div()
                .size(px(16.))
                .flex()
                .items_center()
                .justify_center()
                .opacity(dot)
                .group_hover(hover_group.clone(), |marker| marker.invisible())
                .child(
                    div()
                        .size(px(7. * (0.6 + 0.4 * dot)))
                        .rounded_full()
                        .bg(colors.muted),
                )
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
        let dragged = super::tabs::DraggedTab {
            group,
            index,
            name: name.clone(),
        };
        div()
            .id(("tab", group * 1000 + index))
            .group(hover_group.clone())
            .on_drag(dragged, |dragged, _, _, cx| cx.new(|_| dragged.clone()))
            .drag_over::<super::tabs::DraggedTab>(move |style, _, _, _| {
                style.border_l_2().border_color(colors.accent)
            })
            .on_drop(
                cx.listener(move |this, dragged: &super::tabs::DraggedTab, window, cx| {
                    this.move_tab(dragged.group, dragged.index, group, index, window, cx)
                }),
            )
            .flex_none()
            .relative()
            .flex()
            .items_center()
            .gap_1p5()
            .text_sm()
            .text_color(if in_view { colors.text } else { colors.muted })
            .when(!flat, |tab| {
                // A capsule, as Safari's tabs.
                tab.h(px(30.))
                    .pl(px(10.))
                    .pr_1()
                    .rounded(px(8.))
                    .when(in_view && focused_group, |tab| tab.bg(colors.selected))
                    .when(in_view && !focused_group, |tab| tab.bg(colors.hover))
                    .when(!in_view, |tab| tab.hover(|tab| tab.bg(colors.hover)))
            })
            .when(flat, |tab| {
                // A cell of the strip, as VS Code's: the one in view has the
                // editor's color and no line under it, so it opens onto the
                // editor; the focused group's is marked on top.
                tab.h_full()
                    .pl_3()
                    .pr_1p5()
                    .border_r_1()
                    .border_color(colors.border)
                    .when(in_view, |tab| tab.bg(colors.editor))
                    .when(!in_view, |tab| {
                        tab.border_b_1()
                            .hover(|tab| tab.bg(colors.hover.opacity(0.5)))
                    })
                    .when(in_view && focused_group, |tab| {
                        tab.child(
                            div()
                                .absolute()
                                .top_0()
                                .left_0()
                                .right_0()
                                .h(px(2.))
                                .bg(colors.accent),
                        )
                    })
            })
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
            .children(icon)
            .child(
                div()
                    .whitespace_nowrap()
                    .when(tab.preview, |name| name.italic())
                    .when_some(tinted, |name, color| {
                        name.text_color(if in_view { color } else { color.opacity(0.8) })
                    })
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

    fn render_editors(
        &self,
        colors: Colors,
        panel_open: f32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let look = self.look;
        let flat = look.flat();
        let panes = self.panes.iter().enumerate().map(|(index, pane)| {
            let tabs: Vec<AnyElement> = self.groups[index]
                .tabs
                .iter()
                .enumerate()
                .map(|(position, tab)| self.render_tab(index, position, tab, colors, window, cx))
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
                        .h(px(look.bar_height()))
                        .flex_none()
                        .flex()
                        .items_center()
                        .when(!flat, |strip| strip.pr_2())
                        .when(flat, |strip| strip.bg(colors.panel))
                        .child(
                            div()
                                .id(("tab-strip", index))
                                .h_full()
                                .flex_1()
                                .min_w_0()
                                .flex()
                                .items_center()
                                .when(!flat, |strip| strip.px_2().gap_0p5())
                                .overflow_x_scroll()
                                // Dropped after the last tab: at the end.
                                .on_drop(cx.listener(
                                    move |this, dragged: &super::tabs::DraggedTab, window, cx| {
                                        let end = this
                                            .groups
                                            .get(index)
                                            .map_or(0, |group| group.tabs.len());
                                        this.move_tab(
                                            dragged.group,
                                            dragged.index,
                                            index,
                                            end,
                                            window,
                                            cx,
                                        )
                                    },
                                ))
                                .children(tabs)
                                // The strip's line goes on after the last tab.
                                .when(flat, |strip| {
                                    strip.child(
                                        div()
                                            .flex_1()
                                            .min_w(px(8.))
                                            .h_full()
                                            .border_b_1()
                                            .border_color(colors.border),
                                    )
                                }),
                        )
                        .children(self.render_preview_button(index, cx).map(|button| {
                            div()
                                .flex_none()
                                .h_full()
                                .px_1()
                                .flex()
                                .items_center()
                                .when(flat, |area| area.border_b_1().border_color(colors.border))
                                .child(button)
                        })),
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
                        Some(Backing::ExtensionDetails(tab)) => div()
                            .flex_1()
                            .min_h_0()
                            .child(self.render_extension_details(tab, colors, cx)),
                        Some(Backing::Graph(tab)) => div()
                            .flex_1()
                            .min_h_0()
                            .child(self.render_git_graph(tab, colors, cx)),
                        Some(Backing::Pdf(tab)) => div()
                            .flex_1()
                            .min_h_0()
                            .child(self.render_pdf(&pane.uri, tab, colors, window, cx)),
                        Some(Backing::Page(kind)) => div().flex_1().min_h_0().child(match kind {
                            super::settings_view::PageKind::Settings => {
                                self.render_settings_page(window, cx)
                            }
                            super::settings_view::PageKind::Keybindings => {
                                self.render_keybindings_page(colors, cx)
                            }
                        }),
                        Some(Backing::Preview(tab)) => div()
                            .flex_1()
                            .min_h_0()
                            .child(self.render_markdown_preview(tab, colors, cx)),
                        backing => div()
                            .flex_1()
                            .min_h_0()
                            .relative()
                            .flex()
                            .key_context(super::EDITOR_CONTEXT)
                            .children(self.render_blame_column(index, cx))
                            .child(
                                div()
                                    .relative()
                                    .flex_1()
                                    .min_w_0()
                                    .h_full()
                                    .flex()
                                    .child(
                                        Editor::new(&pane.editor)
                                            // A library's class is there to read.
                                            .readonly(matches!(backing, Some(Backing::Library)))
                                            .flex_1()
                                            .min_w_0()
                                            .h_full()
                                            .bordered(false)
                                            .text_size(px(self.font_size)),
                                    )
                                    .children(pane.sticky.clone()),
                            )
                            .children(pane.minimap.clone())
                            .children(self.render_hunk_bar(index, colors, cx))
                            .children(self.render_find_bar(index, colors, cx))
                            .children(self.render_signature_help(index, colors, cx)),
                    },
                )
        });

        let content = if self.panes.is_empty() {
            self.render_welcome(colors, window, cx)
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
        panel(look, colors.editor, colors, colors.dark)
            .flex_1()
            .min_w_0()
            .when(!panel_takes_all, |main| main.child(content))
            .when(panel_open > 0., |main| {
                main.child(self.render_bottom_panel(colors, panel_open, cx))
            })
            .on_drag_move::<super::terminals::PanelResize>(
                cx.listener(|this, event, _, cx| this.resize_panel(event, cx)),
            )
    }

    fn render_bottom_panel(&self, colors: Colors, open: f32, cx: &mut Context<Self>) -> Div {
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
                    // The tab in front sits on its line; the others keep its
                    // room, so the names do not jump.
                    .when(!is_active, |tab| {
                        tab.border_b_2()
                            .border_color(gpui::transparent_black())
                            .hover(|tab| tab.text_color(colors.muted))
                    })
                    .pt(px(2.))
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
        let button = |id: &'static str, icon: IconName, tooltip: &'static str| {
            Button::new(id)
                .ghost()
                .small()
                .icon(Icon::new(icon))
                .tooltip(tooltip)
        };
        let header = div()
            .h(px(self.look.bar_height()))
            .flex_none()
            .pl_3()
            .pr_1p5()
            .flex()
            .items_center()
            .gap_4()
            .children(tabs)
            .map(|header| match self.layout.panel_view {
                PanelView::Terminal => header.child(self.render_terminal_header(colors, cx)),
                PanelView::Output => header.child(self.render_output_header(cx)),
                _ => header.child(div().ml_auto()),
            })
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_0p5()
                    .child(
                        button(
                            "maximize-panel",
                            if maximized {
                                IconName::Minimize
                            } else {
                                IconName::Maximize
                            },
                            if maximized {
                                strings.restore_panel
                            } else {
                                strings.maximize_panel
                            },
                        )
                        .on_click(cx.listener(|this, _, _, cx| this.toggle_maximized_panel(cx))),
                    )
                    .child(
                        button("close-panel", IconName::X, strings.toggle_panel)
                            .on_click(cx.listener(|this, _, _, cx| this.toggle_panel(cx))),
                    ),
            );
        let body = div()
            .flex()
            .flex_col()
            .when(maximized, |body| body.flex_1().min_h_0())
            // While it slides, the panel keeps its height: the terminal
            // inside is not resized at every frame.
            .when(!maximized, |body| body.flex_none().h(px(self.panel_height)))
            .child(header)
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
            });
        div()
            .relative()
            .flex()
            .flex_col()
            .when(maximized, |panel| panel.flex_1().min_h_0())
            // Slides up from the bottom edge, as a drawer.
            .when(!maximized, |panel| {
                panel
                    .flex_none()
                    .h(px(self.panel_height * open))
                    .overflow_hidden()
                    .justify_end()
            })
            .border_t_1()
            .border_color(colors.border)
            .child(body)
            .when(!maximized, |panel| {
                panel.child(self.render_panel_resize(colors))
            })
    }

    fn render_agent(
        &self,
        colors: Colors,
        open: f32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        use super::agent_panel::AgentView;
        let strings = self.strings();
        let look = self.look;
        let terminal = self.render_agent_terminal();
        let running = terminal.is_some();
        let showing_agent = self.agent_panel.view == AgentView::Agent;
        // Chat or terminal, as two symbols side by side.
        let switch = |id: &'static str, icon: IconName, tooltip: &'static str, view: AgentView| {
            Button::new(id)
                .ghost()
                .small()
                .icon(Icon::new(icon))
                .tooltip(tooltip)
                .selected(self.agent_panel.view == view)
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.agent_panel.view = view;
                    if view == AgentView::Agent {
                        this.agent_panel
                            .input
                            .update(cx, |input, cx| input.focus(window, cx));
                    }
                    cx.notify();
                }))
        };
        let chat_switch = switch(
            "agent-view-agent",
            IconName::MessageSquare,
            strings.agent_view_agent,
            AgentView::Agent,
        );
        let terminal_switch = switch(
            "agent-view-terminal",
            IconName::SquareTerminal,
            strings.agent_view_terminal,
            AgentView::Terminal,
        );
        // The chat's own buttons, or the one that closes the terminal.
        let actions: Option<AnyElement> = if showing_agent {
            Some(self.render_agent_header_buttons(cx))
        } else if running {
            Some(
                Button::new("close-claude-code")
                    .ghost()
                    .small()
                    .icon(Icon::new(IconName::X))
                    .tooltip(strings.close_claude_code)
                    .on_click(
                        cx.listener(|this, _, window, cx| this.close_agent_terminal(window, cx)),
                    )
                    .into_any_element(),
            )
        } else {
            None
        };
        let connected = running || self.agent_panel.agent.is_some();
        let title = if showing_agent {
            if self.agent_panel.show_chats {
                strings.agent_chats.to_owned()
            } else {
                self.agent_title()
            }
        } else {
            "Claude Code".to_owned()
        };
        let header = div()
            .h(px(look.bar_height()))
            .flex_none()
            .pl_2p5()
            .pr_1p5()
            .flex()
            .items_center()
            .gap_1()
            .border_b_1()
            .border_color(colors.border)
            // Claude's mark, with a dot while it is connected.
            .child(
                div()
                    .relative()
                    .size(px(22.))
                    .flex_none()
                    .rounded_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .bg(colors.accent.opacity(0.16))
                    .child(
                        Icon::new(IconName::Sparkles)
                            .xsmall()
                            .text_color(colors.accent),
                    )
                    .when(connected, |mark| {
                        mark.child(
                            div()
                                .absolute()
                                .bottom(px(-1.))
                                .right(px(-1.))
                                .size(px(8.))
                                .rounded_full()
                                .border_2()
                                .border_color(colors.panel)
                                .bg(cx.theme().success),
                        )
                    }),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .ml_1()
                    .truncate()
                    .text_sm()
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(colors.text)
                    .child(title),
            )
            .children(actions)
            .child(div().w(px(1.)).h(px(16.)).mx_0p5().bg(colors.border))
            .child(chat_switch)
            .child(terminal_switch);
        let body = match (showing_agent, terminal) {
            (true, _) => div()
                .flex_1()
                .min_h_0()
                .flex()
                .flex_col()
                .child(self.render_agent_conversation(colors, window, cx)),
            (false, Some(terminal)) => div().flex_1().min_h_0().child(terminal),
            (false, None) => div()
                .flex_1()
                .min_h_0()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap_1()
                .px_4()
                .child(empty_state(
                    IconName::SquareTerminal,
                    strings.claude_code_title,
                    strings.claude_code_hint,
                    colors,
                ))
                .child(
                    Button::new("open-claude-code")
                        .primary()
                        .icon(Icon::new(IconName::SquareTerminal))
                        .label(strings.open_claude_code)
                        .on_click(
                            cx.listener(|this, _, window, cx| this.open_claude_code(window, cx)),
                        ),
                ),
        };
        let card = panel(look, colors.panel, colors, colors.dark)
            .flex_none()
            .w(px(self.agent_width))
            .when(look.flat(), |card| {
                card.border_l_1().border_color(colors.border)
            })
            .child(header)
            .child(body);
        let full = self.agent_width + look.gap();
        // Slides in from the right edge, as the sidebar from the left.
        div()
            .h_full()
            .flex_none()
            .w(px(full * open))
            .overflow_hidden()
            .flex()
            .child(
                div()
                    .relative()
                    .flex_none()
                    .w(px(full))
                    .h_full()
                    .pl(px(look.gap()))
                    .when(open < 1., |room| room.opacity(0.4 + 0.6 * open))
                    .child(card)
                    .child(self.render_agent_resize(colors)),
            )
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
        // Who wrote the line under the cursor, while the column is on.
        if let Some(blame) = self.current_line_blame(cx) {
            left.push(blame);
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
                Some(
                    Backing::Image(_)
                        | Backing::Diff(_)
                        | Backing::Preview(_)
                        | Backing::Pdf(_)
                        | Backing::Graph(_)
                        | Backing::ExtensionDetails(_)
                )
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
            let language_id = self
                .documents
                .get(&pane.uri)
                .map(|document| document.language_id.clone())
                .unwrap_or_default();
            let tab_size = self.tab_size_for(&language_id);
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
        let custom = self.custom_colors(cx);
        let look = self.look;
        div()
            .h(px(look.status_height()))
            .flex_none()
            .px(px(if look.flat() { 6. } else { 10. }))
            .flex()
            .items_center()
            .justify_between()
            .gap_3()
            .text_xs()
            .text_color(custom.status_foreground.unwrap_or(colors.muted))
            .when(look.flat(), |bar| {
                bar.bg(colors.panel)
                    .border_t_1()
                    .border_color(colors.border)
            })
            .when_some(custom.status_background, |bar, color| bar.bg(color))
            .child(div().flex().items_center().gap_1().min_w_0().children({
                let mut items: Vec<AnyElement> = left
                    .into_iter()
                    .enumerate()
                    .map(|(index, text)| {
                        let is_branch = branch && index == 0;
                        div()
                            .id(("status-left", index))
                            .h(px(20.))
                            .px(px(6.))
                            .rounded(px(5.))
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
                                    .hover(|item| item.bg(colors.hover))
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.choose_branch(window, cx)
                                    }))
                            })
                            .when_some(sync.clone().filter(|_| is_branch), |item, sync| {
                                item.child(
                                    div()
                                        .id("status-sync")
                                        .ml_1()
                                        .px_1()
                                        .rounded(px(4.))
                                        .cursor_pointer()
                                        .hover(|item| item.bg(colors.hover).text_color(colors.text))
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
                    .gap_1()
                    .children(right.into_iter().map(|text| {
                        div()
                            .h(px(20.))
                            .px(px(6.))
                            .flex()
                            .items_center()
                            .child(text)
                            .into_any_element()
                    }))
                    .child(self.render_bell(colors, cx)),
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
        let look = self.look;
        let colors = Colors::new(cx).with_look(look);
        // How far each panel is open, gliding when it is shown or hidden.
        // Sampled on every frame, hidden or not, to know where it was.
        let slide = look.duration(SLIDE_MS);
        let open = |shown: bool| if shown { 1. } else { 0. };
        let sidebar_open =
            look::glide("sidebar-open", open(self.layout.sidebar), slide, window, cx);
        let agent_open = look::glide("agent-open", open(self.layout.agent), slide, window, cx);
        let panel_open = look::glide("panel-open", open(self.layout.panel), slide, window, cx);
        let toolbar = self.render_toolbar(colors, window, cx);
        let gap = look.gap();
        let body = div()
            .flex_1()
            .min_h_0()
            .flex()
            .flex_row()
            // The gaps between the panels belong to the sidebar and to
            // Claude Code, so they close with them.
            .pl(px(if sidebar_open > 0. { 0. } else { gap }))
            .pr(px(if agent_open > 0. { 0. } else { gap }))
            .when(!look.flat(), |body| body.pb(px(gap)))
            .when(sidebar_open > 0., |body| {
                body.child(div().flex_none().w(px(gap)))
                    .child(self.render_sidebar(colors, sidebar_open, window, cx))
            })
            .child(self.render_editors(colors, panel_open, window, cx))
            .when(agent_open > 0., |body| {
                body.child(self.render_agent(colors, agent_open, window, cx))
                    .child(div().flex_none().w(px(gap)))
            })
            .on_drag_move::<super::claude_code::AgentResize>(
                cx.listener(|this, event, _, cx| this.resize_agent(event, cx)),
            )
            .on_drag_move::<SidebarResize>(
                cx.listener(|this, event, _, cx| this.resize_sidebar(event, cx)),
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
        let root = self.git_graph_actions(root, cx);
        let root = self.commit_view_actions(root, cx);
        let root = self.blame_actions(root, cx);
        let root = self.extensions_view_actions(root, cx);
        let root = self.contributions_actions(root, cx);
        let root = self.cursor_import_actions(root, cx);
        let root = self.claude_ide_actions(root, cx);
        let root = self.claude_diff_actions(root, cx);
        let root = self.agent_panel_actions(root, cx);
        let root = self.editing_actions(root, cx);
        let root = self.find_actions(root, cx);
        let root = self.notice_actions(root, cx);
        let root = self.file_command_actions(root, cx);
        let notice_center = self.render_notice_center(colors, window, cx);
        self.explorer_actions(root, cx)
            .relative()
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
            .on_action(cx.listener(|this, _: &OpenSettings, window, cx| {
                this.open_page(super::settings_view::PageKind::Settings, window, cx)
            }))
            .on_action(cx.listener(|this, _: &ImportCursorSettings, window, cx| {
                this.import_cursor_settings(window, cx)
            }))
            .on_action(cx.listener(|this, _: &OpenSettingsJson, window, cx| {
                this.open_settings_json(window, cx)
            }))
            .on_action(cx.listener(|this, _: &OpenKeybindings, window, cx| {
                this.reload_user_keybindings();
                this.open_page(super::settings_view::PageKind::Keybindings, window, cx)
            }))
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
            .children(notice_center)
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

    /// The two styles: floating panels keep a gap from the window's edge
    /// and from each other, flat ones touch them; the setting changes the
    /// window at once and the toolbar's height with it.
    #[gpui::test]
    fn the_flat_style_puts_the_panels_edge_to_edge(cx: &mut TestAppContext) {
        set_up(cx);
        let folder = tempfile::tempdir().unwrap();
        let settings = english(folder.path());
        let (view, cx) =
            cx.add_window_view(|window, cx| Workbench::new(None, settings, None, window, cx));
        let sidebar_at = |cx: &mut VisualTestContext| {
            cx.update(|window, _| window.refresh());
            cx.run_until_parked();
            cx.debug_bounds("sidebar").expect("the sidebar is drawn")
        };
        let floating = sidebar_at(cx);
        assert_eq!(f32::from(floating.left()), 8.);
        assert_eq!(f32::from(floating.top()), 52. + 0.);

        view.update_in(cx, |view, window, cx| {
            view.change_setting(
                crate::workbench_window::look::STYLE_SETTING,
                Some(json!("flat")),
                window,
                cx,
            )
        });
        assert!(view.read_with(cx, |view, _| view.look.flat()));
        let flat = sidebar_at(cx);
        assert_eq!(f32::from(flat.left()), 0.);
        assert_eq!(f32::from(flat.top()), 40.);
        // As wide as before: only the frame around it changed.
        assert_eq!(flat.size.width, floating.size.width);
    }

    /// The sidebar slides out over a moment, then is not drawn at all; it
    /// slides back in from the same edge.
    #[gpui::test]
    fn the_sidebar_slides_out_and_back(cx: &mut TestAppContext) {
        set_up(cx);
        let folder = tempfile::tempdir().unwrap();
        let settings = english(folder.path());
        let (view, cx) =
            cx.add_window_view(|window, cx| Workbench::new(None, settings, None, window, cx));
        view.update(cx, |view, _| view.look.motion = true);
        let frame = |cx: &mut VisualTestContext| {
            cx.update(|window, _| window.refresh());
            cx.run_until_parked();
        };
        frame(cx);
        let shown = cx.debug_bounds("sidebar").unwrap();

        cx.simulate_keystrokes("cmd-b");
        frame(cx);
        cx.executor()
            .advance_clock(Duration::from_millis(SLIDE_MS / 3));
        frame(cx);
        // On its way: still drawn, partly past the window's left edge.
        let sliding = cx
            .debug_bounds("sidebar")
            .expect("still drawn while it slides");
        assert!(sliding.left() < shown.left(), "{sliding:?}");
        cx.executor()
            .advance_clock(Duration::from_millis(SLIDE_MS * 2));
        frame(cx);
        assert!(cx.debug_bounds("sidebar").is_none(), "gone once it is out");

        cx.simulate_keystrokes("cmd-b");
        frame(cx);
        cx.executor()
            .advance_clock(Duration::from_millis(SLIDE_MS * 2));
        frame(cx);
        assert_eq!(cx.debug_bounds("sidebar"), Some(shown));
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
