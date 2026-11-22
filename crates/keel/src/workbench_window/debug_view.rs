//! What the debugger shows: the Run and Debug view with the variables, the
//! watched expressions, the call stack and the breakpoints; the Debug
//! Console in the panel; and the floating bar that goes on and stops.

use std::collections::HashMap;
use std::ops::Range;

use dap::protocol::Variable;
use gpui::{
    AnyElement, ClickEvent, Div, SharedString, StatefulInteractiveElement as _, uniform_list,
};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::checkbox::Checkbox;
use gpui_component::input::{Input, InputState};
use gpui_component::{ActiveTheme as _, Disableable as _, Icon, Sizable as _, WindowExt as _};
use gpui_kit_assets::IconName;

use super::debug::{LineKind, Session, SessionState, frame_file};
use super::shell::{Colors, section_header, view_header};
use super::*;

const ROW: f32 = 22.;
const CONSOLE_ROW: f32 = 20.;

/// A row of the variables: a scope, or a variable in one.
struct VariableRow {
    /// Where it is: `Locals/nomi/[0]`.
    path: String,
    depth: usize,
    name: String,
    value: String,
    kind: Option<String>,
    /// What holds it, to change it; 0 for a scope.
    container: i64,
    opens: bool,
    open: bool,
    scope: bool,
}

impl Session {
    fn variable_rows(&self, open: &HashSet<String>, closed: &HashSet<String>) -> Vec<VariableRow> {
        let mut rows = Vec::new();
        for scope in &self.scopes {
            let path = scope.name.clone();
            let shown = if scope.expensive {
                open.contains(&path)
            } else {
                !closed.contains(&path)
            };
            rows.push(VariableRow {
                path: path.clone(),
                depth: 0,
                name: scope.name.clone(),
                value: String::new(),
                kind: None,
                container: 0,
                opens: true,
                open: shown,
                scope: true,
            });
            if shown {
                rows_in(
                    &self.variables,
                    scope.variables_reference,
                    &path,
                    1,
                    open,
                    &mut rows,
                );
            }
        }
        rows
    }
}

fn rows_in(
    variables: &HashMap<i64, Vec<Variable>>,
    reference: i64,
    path: &str,
    depth: usize,
    open: &HashSet<String>,
    rows: &mut Vec<VariableRow>,
) {
    for variable in variables.get(&reference).into_iter().flatten() {
        let path = format!("{path}/{}", variable.name);
        let is_open = variable.variables_reference > 0 && open.contains(&path);
        rows.push(VariableRow {
            path: path.clone(),
            depth,
            name: variable.name.clone(),
            value: variable.value.clone(),
            kind: variable.kind.clone(),
            container: reference,
            opens: variable.variables_reference > 0,
            open: is_open,
            scope: false,
        });
        if is_open {
            rows_in(
                variables,
                variable.variables_reference,
                &path,
                depth + 1,
                open,
                rows,
            );
        }
    }
}

impl Workbench {
    // -----------------------------------------------------------------
    // The view

    pub(super) fn render_debug(&self, colors: Colors, cx: &mut Context<Self>) -> AnyElement {
        let strings = self.strings();
        let chosen = self.shown_configuration();
        let has_session = self.debug.session().is_some();
        let header = view_header(strings.view_debug, colors)
            .child(
                Button::new("debug-configure")
                    .ghost()
                    .small()
                    .icon(Icon::new(IconName::Settings))
                    .tooltip(strings.command_debug_configure)
                    .on_click(
                        cx.listener(|this, _, window, cx| this.configure_debugging(window, cx)),
                    ),
            )
            .child(
                Button::new("debug-console-toggle")
                    .ghost()
                    .small()
                    .icon(Icon::new(IconName::SquareTerminal))
                    .tooltip(strings.panel_debug_console)
                    .on_click(cx.listener(|this, _, window, cx| {
                        window.dispatch_action(Box::new(super::debug::ToggleDebugConsole), cx);
                        let _ = this;
                    })),
            );
        let start = div()
            .mx_1()
            .mt_1()
            .flex()
            .items_center()
            .gap_1()
            .child(
                Button::new("debug-start")
                    .primary()
                    .small()
                    .icon(Icon::new(IconName::Play))
                    .tooltip(strings.command_debug_start)
                    .on_click(
                        cx.listener(|this, _, window, cx| this.start_chosen(false, window, cx)),
                    ),
            )
            .child(
                div()
                    .id("debug-choose")
                    .flex_1()
                    .min_w_0()
                    .h(px(26.))
                    .px_2()
                    .flex()
                    .items_center()
                    .gap_1()
                    .rounded(px(6.))
                    .border_1()
                    .border_color(colors.border)
                    .cursor_pointer()
                    .hover(|choose| choose.bg(colors.hover))
                    .text_sm()
                    .child(
                        div().flex_1().min_w_0().truncate().child(
                            chosen
                                .clone()
                                .unwrap_or_else(|| strings.debug_no_configuration.to_owned()),
                        ),
                    )
                    .child(
                        Icon::new(IconName::ChevronDown)
                            .xsmall()
                            .text_color(colors.faint),
                    )
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.choose_configuration(false, window, cx)
                    })),
            );
        let mut view = div().w_full().flex().flex_col().child(header).child(start);
        if !has_session && chosen.is_none() {
            view = view.child(
                div()
                    .p_3()
                    .text_sm()
                    .text_color(colors.muted)
                    .child(strings.debug_nothing_detail),
            );
        }
        view = view
            .child(self.render_debug_section(
                "variables",
                strings.debug_variables,
                None,
                colors,
                cx,
                |this, cx| this.render_variables(colors, cx),
            ))
            .child(
                self.render_debug_section(
                    "watch",
                    strings.debug_watch,
                    Some(
                        Button::new("debug-watch-add")
                            .ghost()
                            .xsmall()
                            .icon(Icon::new(IconName::Plus))
                            .tooltip(strings.debug_watch_add)
                            .on_click(cx.listener(|this, _, window, cx| {
                                cx.stop_propagation();
                                this.ask_watch(window, cx)
                            }))
                            .into_any_element(),
                    ),
                    colors,
                    cx,
                    |this, cx| this.render_watches(colors, cx),
                ),
            )
            .child(self.render_debug_section(
                "stack",
                strings.debug_stack,
                None,
                colors,
                cx,
                |this, cx| this.render_stack(colors, cx),
            ))
            .child(
                self.render_debug_section(
                    "breakpoints",
                    strings.debug_breakpoints,
                    Some(
                        Button::new("debug-breakpoints-clear")
                            .ghost()
                            .xsmall()
                            .icon(Icon::new(IconName::Eraser))
                            .tooltip(strings.breakpoint_remove_all)
                            .on_click(cx.listener(|this, _, _, cx| {
                                cx.stop_propagation();
                                let files = this.debug.breakpoints.files();
                                this.debug.breakpoints.all.clear();
                                this.breakpoints_changed(&files, cx);
                            }))
                            .into_any_element(),
                    ),
                    colors,
                    cx,
                    |this, cx| this.render_breakpoint_list(colors, cx),
                ),
            );
        view.into_any_element()
    }

    /// A part of the view, under a header that closes it.
    fn render_debug_section(
        &self,
        id: &'static str,
        title: &str,
        action: Option<AnyElement>,
        colors: Colors,
        cx: &mut Context<Self>,
        content: impl FnOnce(&Self, &mut Context<Self>) -> Vec<AnyElement>,
    ) -> Div {
        let key = format!("§{id}");
        let closed = self.debug.closed.contains(&key);
        let rows = if closed {
            Vec::new()
        } else {
            content(self, cx)
        };
        div()
            .mt_2()
            .flex()
            .flex_col()
            .child(
                section_header(
                    SharedString::from(format!("debug-section-{id}")),
                    title,
                    if closed { 0. } else { 1. },
                    colors,
                )
                .children(action)
                .on_click(cx.listener(move |this, _, _, cx| {
                    if !this.debug.closed.remove(&key) {
                        this.debug.closed.insert(key.clone());
                    }
                    cx.notify();
                })),
            )
            .children(rows)
    }

    fn empty_row(&self, text: &str, colors: Colors) -> Vec<AnyElement> {
        vec![
            div()
                .px_3()
                .py_1()
                .text_xs()
                .text_color(colors.faint)
                .child(text.to_owned())
                .into_any_element(),
        ]
    }

    fn render_variables(&self, colors: Colors, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let strings = self.strings();
        let Some(session) = self
            .debug
            .session()
            .filter(|session| session.state == SessionState::Stopped)
        else {
            return self.empty_row(strings.debug_variables_empty, colors);
        };
        let editable = session.capabilities.supports_set_variable;
        let theme = cx.theme();
        let (name_color, value_color) = (theme.info, colors.text);
        let mono = theme.mono_font_family.clone();
        session
            .variable_rows(&self.debug.open, &self.debug.closed)
            .into_iter()
            .enumerate()
            .map(|(index, row)| {
                let VariableRow {
                    path,
                    depth,
                    name,
                    value,
                    kind,
                    container,
                    opens,
                    open,
                    scope,
                } = row;
                let tip: SharedString = match &kind {
                    Some(kind) => format!("{name}: {kind}\n{value}").into(),
                    None => format!("{name} = {value}").into(),
                };
                let (edit_name, edit_value) = (name.clone(), value.clone());
                div()
                    .id(("debug-variable", index))
                    .min_h(px(ROW))
                    .pl(px(6. + 12. * depth as f32))
                    .pr_2()
                    .flex()
                    .items_center()
                    .gap_1()
                    .rounded(px(4.))
                    .cursor_pointer()
                    .hover(|row| row.bg(colors.hover))
                    .text_xs()
                    .when(!scope, |row| row.font_family(mono.clone()))
                    .child(div().w(px(12.)).flex_none().when(opens, |arrow| {
                        arrow.child(
                            Icon::new(if open {
                                IconName::ChevronDown
                            } else {
                                IconName::ChevronRight
                            })
                            .xsmall()
                            .text_color(colors.faint),
                        )
                    }))
                    .child(
                        div()
                            .flex_none()
                            .max_w(px(180.))
                            .truncate()
                            .text_color(if scope { colors.muted } else { name_color })
                            .when(scope, |name| name.font_weight(gpui::FontWeight::SEMIBOLD))
                            .child(name),
                    )
                    .when(!scope && !value.is_empty(), |row| {
                        row.child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .text_color(value_color)
                                .child(value.lines().next().unwrap_or_default().to_owned()),
                        )
                    })
                    .when(!scope, |row| {
                        row.tooltip(move |window, cx| {
                            gpui_component::tooltip::Tooltip::new(tip.clone()).build(window, cx)
                        })
                    })
                    .on_click(cx.listener(move |this, event: &ClickEvent, window, cx| {
                        if event.click_count() >= 2 && !scope && editable {
                            this.ask_variable(
                                container,
                                edit_name.clone(),
                                edit_value.clone(),
                                window,
                                cx,
                            );
                        } else if opens {
                            this.toggle_variable(&path, scope, cx);
                        }
                    }))
                    .into_any_element()
            })
            .collect()
    }

    /// Asks a variable's new value.
    fn ask_variable(
        &mut self,
        container: i64,
        name: String,
        value: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let field = cx.new(|cx| InputState::new(window, cx).default_value(value));
        let this = cx.weak_entity();
        let shown = field.clone();
        window.open_dialog(cx, move |dialog, _, _| {
            let (this, read, changed) = (this.clone(), shown.clone(), name.clone());
            dialog
                .w(px(420.))
                .title(name.clone())
                .on_ok(move |_, _, cx| {
                    let value = read.read(cx).value().to_string();
                    this.update(cx, |this, cx| {
                        this.set_variable(container, changed.clone(), value, cx)
                    })
                    .ok();
                    true
                })
                .child(Input::new(&shown).small())
        });
        field.update(cx, |field, cx| field.focus(window, cx));
    }

    fn ask_watch(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let title = self.strings().debug_watch_add;
        let field = cx.new(|cx| InputState::new(window, cx).placeholder(title));
        let this = cx.weak_entity();
        let shown = field.clone();
        window.open_dialog(cx, move |dialog, _, _| {
            let (this, read) = (this.clone(), shown.clone());
            dialog
                .w(px(420.))
                .title(title)
                .on_ok(move |_, _, cx| {
                    let text = read.read(cx).value().trim().to_owned();
                    if !text.is_empty() {
                        this.update(cx, |this, cx| this.add_watch(text, cx)).ok();
                    }
                    true
                })
                .child(Input::new(&shown).small())
        });
        field.update(cx, |field, cx| field.focus(window, cx));
    }

    fn render_watches(&self, colors: Colors, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let strings = self.strings();
        if self.debug.watches.is_empty() {
            return self.empty_row(strings.debug_watch_empty, colors);
        }
        let theme = cx.theme();
        let (name_color, danger) = (theme.info, theme.danger);
        let mono = theme.mono_font_family.clone();
        self.debug
            .watches
            .iter()
            .enumerate()
            .map(|(index, watch)| {
                let (value, color) = match &watch.value {
                    Some(Ok(value)) => (value.result.clone(), colors.text),
                    Some(Err(error)) => (error.clone(), danger),
                    None => (strings.debug_watch_unknown.to_owned(), colors.faint),
                };
                let group = SharedString::from(format!("debug-watch-{index}"));
                div()
                    .id(("debug-watch", index))
                    .group(group.clone())
                    .min_h(px(ROW))
                    .pl(px(18.))
                    .pr_1()
                    .flex()
                    .items_center()
                    .gap_1()
                    .rounded(px(4.))
                    .hover(|row| row.bg(colors.hover))
                    .text_xs()
                    .font_family(mono.clone())
                    .child(
                        div()
                            .flex_none()
                            .max_w(px(160.))
                            .truncate()
                            .text_color(name_color)
                            .child(watch.expression.clone()),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_color(color)
                            .child(value),
                    )
                    .child(
                        div()
                            .flex_none()
                            .invisible()
                            .group_hover(group, |button| button.visible())
                            .child(
                                Button::new(("debug-watch-remove", index))
                                    .ghost()
                                    .xsmall()
                                    .icon(Icon::new(IconName::X))
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        if index < this.debug.watches.len() {
                                            this.debug.watches.remove(index);
                                            this.session_changed(cx);
                                            cx.notify();
                                        }
                                    })),
                            ),
                    )
                    .into_any_element()
            })
            .collect()
    }

    fn render_stack(&self, colors: Colors, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let strings = self.strings();
        if self.debug.sessions.is_empty() {
            return self.empty_row(strings.debug_stack_empty, colors);
        }
        let mut rows = Vec::new();
        let several = self.debug.sessions.len() > 1;
        for session in &self.debug.sessions {
            let id = session.id;
            let active = self.debug.active == Some(id);
            let state = match session.state {
                SessionState::Starting => strings.debug_state_starting.to_owned(),
                SessionState::Running => strings.debug_state_running.to_owned(),
                SessionState::Stopped => session
                    .stop
                    .clone()
                    .unwrap_or_else(|| strings.debug_state_stopped.to_owned()),
            };
            rows.push(
                div()
                    .id(("debug-session", id))
                    .min_h(px(ROW))
                    .px_2()
                    .flex()
                    .items_center()
                    .gap_2()
                    .rounded(px(4.))
                    .when(several, |row| row.cursor_pointer())
                    .when(several && active, |row| row.bg(colors.selected))
                    .text_xs()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_color(colors.text)
                            .child(session.name.clone()),
                    )
                    .child(
                        div()
                            .flex_none()
                            .max_w(px(140.))
                            .truncate()
                            .text_color(if session.state == SessionState::Stopped {
                                cx.theme().warning
                            } else {
                                colors.faint
                            })
                            .child(state),
                    )
                    .on_click(cx.listener(move |this, _, window, cx| {
                        let frame = this
                            .debug
                            .sessions
                            .iter()
                            .find(|session| session.id == id)
                            .map_or(0, |session| session.frame);
                        this.select_frame(id, frame, false, window, cx);
                    }))
                    .into_any_element(),
            );
            if session.state != SessionState::Stopped {
                continue;
            }
            // The other threads, when there are some.
            if session.threads.len() > 1 {
                for thread in &session.threads {
                    let (thread_id, chosen) = (thread.id, session.thread == Some(thread.id));
                    rows.push(
                        div()
                            .id(SharedString::from(format!("debug-thread-{id}-{thread_id}")))
                            .min_h(px(ROW))
                            .pl_4()
                            .pr_2()
                            .flex()
                            .items_center()
                            .rounded(px(4.))
                            .cursor_pointer()
                            .hover(|row| row.bg(colors.hover))
                            .text_xs()
                            .text_color(if chosen { colors.text } else { colors.muted })
                            .child(div().truncate().child(thread.name.clone()))
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.select_thread(id, thread_id, window, cx)
                            }))
                            .into_any_element(),
                    );
                }
            }
            for (index, frame) in session.frames.iter().enumerate() {
                let chosen = active && session.frame == index;
                let file = frame_file(frame);
                let place = match &file {
                    Some(path) => format!(
                        "{}:{}",
                        path.file_name()
                            .map(|name| name.to_string_lossy().into_owned())
                            .unwrap_or_default(),
                        frame.line
                    ),
                    None => frame
                        .source
                        .as_ref()
                        .and_then(|source| source.name.clone())
                        .unwrap_or_default(),
                };
                let known = file.is_some();
                rows.push(
                    div()
                        .id(SharedString::from(format!("debug-frame-{id}-{index}")))
                        .min_h(px(ROW))
                        .pl(px(26.))
                        .pr_2()
                        .flex()
                        .items_center()
                        .gap_2()
                        .rounded(px(4.))
                        .cursor_pointer()
                        .when(chosen, |row| row.bg(colors.selected))
                        .hover(|row| row.bg(colors.hover))
                        .text_xs()
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .text_color(if known { colors.text } else { colors.faint })
                                .child(frame.name.clone()),
                        )
                        .child(
                            div()
                                .flex_none()
                                .max_w(px(130.))
                                .truncate()
                                .text_color(colors.faint)
                                .child(place),
                        )
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.select_frame(id, index, true, window, cx)
                        }))
                        .into_any_element(),
                );
            }
        }
        rows
    }

    fn render_breakpoint_list(&self, colors: Colors, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let strings = self.strings();
        let mut rows = Vec::new();
        // The exceptions the debugger can stop at.
        let filters = self
            .debug
            .session()
            .map(|session| session.capabilities.exception_breakpoint_filters.clone())
            .unwrap_or_default();
        for (index, filter) in filters.into_iter().enumerate() {
            let on = self.exception_wanted(&filter.filter, filter.default);
            rows.push(
                div()
                    .min_h(px(ROW))
                    .px_2()
                    .flex()
                    .items_center()
                    .text_xs()
                    .text_color(colors.muted)
                    .child(
                        Checkbox::new(("debug-exception", index))
                            .checked(on)
                            .label(filter.label.clone())
                            .on_click(cx.listener(move |this, on: &bool, _, cx| {
                                this.want_exception(&filter.filter, *on, cx)
                            })),
                    )
                    .into_any_element(),
            );
        }
        if self.debug.breakpoints.all.is_empty() && rows.is_empty() {
            return self.empty_row(strings.debug_breakpoints_empty, colors);
        }
        let (red, grey) = (cx.theme().danger, colors.faint);
        for (index, breakpoint) in self.debug.breakpoints.all.iter().enumerate() {
            let name = breakpoint
                .path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default();
            let refused = self.breakpoint_refused(&breakpoint.path, breakpoint.line);
            let detail = breakpoint
                .condition
                .clone()
                .or(breakpoint.hits.clone())
                .or(breakpoint.log.clone())
                .or(refused.clone())
                .unwrap_or_default();
            let (path, line) = (breakpoint.path.clone(), breakpoint.line);
            let (toggled, removed) = (path.clone(), path.clone());
            let group = SharedString::from(format!("debug-breakpoint-{index}"));
            rows.push(
                div()
                    .id(("debug-breakpoint", index))
                    .group(group.clone())
                    .min_h(px(ROW))
                    .px_2()
                    .flex()
                    .items_center()
                    .gap_2()
                    .rounded(px(4.))
                    .cursor_pointer()
                    .hover(|row| row.bg(colors.hover))
                    .text_xs()
                    .child(
                        Checkbox::new(("debug-breakpoint-on", index))
                            .checked(!breakpoint.disabled)
                            .on_click(cx.listener(move |this, on: &bool, _, cx| {
                                cx.stop_propagation();
                                if let Some(index) = this.debug.breakpoints.at(&toggled, line) {
                                    this.debug.breakpoints.all[index].disabled = !*on;
                                    this.breakpoints_changed(std::slice::from_ref(&toggled), cx);
                                }
                            })),
                    )
                    .child(div().flex_none().size(px(8.)).rounded_full().bg(
                        if breakpoint.disabled || refused.is_some() {
                            grey
                        } else {
                            red
                        },
                    ))
                    .child(
                        div()
                            .flex_none()
                            .max_w(px(160.))
                            .truncate()
                            .text_color(colors.text)
                            .child(format!("{name}:{}", line + 1)),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_color(colors.faint)
                            .child(detail),
                    )
                    .child(
                        div()
                            .flex_none()
                            .invisible()
                            .group_hover(group, |button| button.visible())
                            .child(
                                Button::new(("debug-breakpoint-remove", index))
                                    .ghost()
                                    .xsmall()
                                    .icon(Icon::new(IconName::X))
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        cx.stop_propagation();
                                        this.toggle_breakpoint(&removed, line, cx);
                                    })),
                            ),
                    )
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.open_file(&path, window, cx);
                        this.go_to(line + 1, 1, window, cx);
                    }))
                    .into_any_element(),
            );
        }
        rows
    }

    // -----------------------------------------------------------------
    // The console

    pub(super) fn render_debug_console(
        &self,
        colors: Colors,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(input) = self.debug.console.input.clone() else {
            return div().into_any_element();
        };
        let strings = self.strings();
        let count = self.debug.console.lines.len();
        let mono = cx.theme().mono_font_family.clone();
        let list = uniform_list(
            "debug-console",
            count,
            cx.processor(move |this, range: Range<usize>, _, cx| {
                this.render_console_lines(range, colors, cx)
            }),
        )
        .track_scroll(&self.debug.console.scroll)
        .size_full();
        div()
            .size_full()
            .flex()
            .flex_col()
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .font_family(mono.clone())
                    .text_sm()
                    .when(count == 0, |body| {
                        body.child(
                            div()
                                .p_3()
                                .text_color(colors.faint)
                                .child(strings.debug_console_empty),
                        )
                    })
                    .when(count > 0, |body| body.child(list)),
            )
            .child(
                div()
                    .flex_none()
                    .h(px(34.))
                    .px_3()
                    .flex()
                    .items_center()
                    .gap_2()
                    .border_t_1()
                    .border_color(colors.border)
                    .font_family(mono)
                    .child(
                        Icon::new(IconName::ChevronRight)
                            .xsmall()
                            .text_color(colors.accent),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .child(Input::new(&input).appearance(false).small()),
                    )
                    .child(
                        Button::new("debug-console-clear")
                            .ghost()
                            .xsmall()
                            .icon(Icon::new(IconName::Eraser))
                            .tooltip(strings.debug_console_clear)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.debug.console.lines.clear();
                                cx.notify();
                            })),
                    ),
            )
            .into_any_element()
    }

    fn render_console_lines(
        &self,
        range: Range<usize>,
        colors: Colors,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let theme = cx.theme();
        let (danger, info) = (theme.danger, theme.info);
        range
            .filter_map(|index| {
                let line = self.debug.console.lines.get(index)?;
                let color = match line.kind {
                    LineKind::Output => colors.text,
                    LineKind::Error => danger,
                    LineKind::Note => colors.faint,
                    LineKind::Typed => colors.accent,
                    LineKind::Answer => info,
                };
                let copied = line.text.clone();
                Some(
                    div()
                        .id(("debug-line", index))
                        .h(px(CONSOLE_ROW))
                        .px_3()
                        .flex()
                        .items_center()
                        .whitespace_nowrap()
                        .text_color(color)
                        .hover(|row| row.bg(colors.hover))
                        .when(line.kind == LineKind::Typed, |row| {
                            row.child(div().text_color(colors.faint).child("› "))
                        })
                        .child(line.text.clone())
                        // A double click copies the line.
                        .on_click(cx.listener(move |_, event: &ClickEvent, _, cx| {
                            if event.click_count() == 2 {
                                cx.write_to_clipboard(gpui::ClipboardItem::new_string(
                                    copied.clone(),
                                ));
                            }
                        }))
                        .into_any_element(),
                )
            })
            .collect()
    }

    // -----------------------------------------------------------------
    // The bar

    /// The floating bar over the editors, while something is debugged.
    pub(super) fn render_debug_bar(
        &self,
        colors: Colors,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let session = self.debug.session()?;
        let strings = self.strings();
        let theme = cx.theme();
        let stopped = session.state == SessionState::Stopped;
        let running = session.state == SessionState::Running;
        let (go, halt, again) = (theme.info, theme.danger, theme.success);
        let button = |id: &'static str,
                      icon: IconName,
                      tip: &'static str,
                      color: Hsla,
                      on: bool,
                      action: Box<dyn gpui::Action>| {
            Button::new(id)
                .ghost()
                .small()
                .icon(Icon::new(icon).text_color(if on { color } else { colors.faint }))
                .tooltip(tip)
                .disabled(!on)
                .on_click(move |_, window, cx| {
                    window.dispatch_action(action.boxed_clone(), cx);
                })
        };
        use super::debug::{
            DebugPause, DebugRestart, DebugStart, DebugStepInto, DebugStepOut, DebugStepOver,
            DebugStop,
        };
        Some(
            div()
                .absolute()
                .top(px(self.look.bar_height() - 2.))
                .left_0()
                .right_0()
                .flex()
                .justify_center()
                .child(
                    div()
                        .id("debug-bar")
                        .occlude()
                        .px_1()
                        .py_0p5()
                        .flex()
                        .items_center()
                        .gap_0p5()
                        .rounded(px(9.))
                        .border_1()
                        .border_color(colors.border)
                        .bg(theme.popover)
                        .shadow_md()
                        .child(if running {
                            button(
                                "debug-pause",
                                IconName::Pause,
                                strings.command_debug_pause,
                                go,
                                true,
                                Box::new(DebugPause),
                            )
                        } else {
                            button(
                                "debug-continue",
                                IconName::Play,
                                strings.debug_continue,
                                go,
                                stopped,
                                Box::new(DebugStart),
                            )
                        })
                        .child(button(
                            "debug-over",
                            IconName::RedoDot,
                            strings.command_debug_step_over,
                            go,
                            stopped,
                            Box::new(DebugStepOver),
                        ))
                        .child(button(
                            "debug-into",
                            IconName::ArrowDownToDot,
                            strings.command_debug_step_into,
                            go,
                            stopped,
                            Box::new(DebugStepInto),
                        ))
                        .child(button(
                            "debug-out",
                            IconName::ArrowUpFromDot,
                            strings.command_debug_step_out,
                            go,
                            stopped,
                            Box::new(DebugStepOut),
                        ))
                        .child(button(
                            "debug-restart",
                            IconName::RotateCcw,
                            strings.command_debug_restart,
                            again,
                            true,
                            Box::new(DebugRestart),
                        ))
                        .child(button(
                            "debug-stop",
                            IconName::Square,
                            strings.command_debug_stop,
                            halt,
                            true,
                            Box::new(DebugStop),
                        )),
                )
                .into_any_element(),
        )
    }
}
