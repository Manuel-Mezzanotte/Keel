//! Breakpoints: where they are, the margin with their dots, and what the
//! debuggers made of them. They stay with their lines as the file is
//! edited, and come back with the project.

use std::cell::RefCell;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use dap::adapters::Adapter;
use dap::protocol::SourceBreakpoint;
use gpui::{
    AnyElement, Bounds, Div, MouseButton, MouseDownEvent, MouseMoveEvent, Pixels, SharedString,
    StatefulInteractiveElement as _, TextRun, canvas, point,
};
use gpui_component::input::{Input, InputState};
use gpui_component::menu::ContextMenuExt as _;
use gpui_component::{ActiveTheme as _, RopeExt as _, Sizable as _, WindowExt as _};
use serde::{Deserialize, Serialize};

use super::*;

actions!(
    keel,
    [
        ToggleBreakpoint,
        EditBreakpointCondition,
        EditBreakpointHits,
        EditLogpoint,
        ToggleBreakpointEnabled,
        RemoveBreakpointHere,
        RemoveAllBreakpoints
    ]
);

const MARGIN: f32 = 16.;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Breakpoint {
    pub path: PathBuf,
    /// From 0.
    pub line: u32,
    #[serde(default)]
    pub disabled: bool,
    /// Stops only where the expression is true.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub condition: Option<String>,
    /// Stops only after so many times.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hits: Option<String>,
    /// Writes instead of stopping.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub log: Option<String>,
}

#[derive(Default)]
pub(super) struct Breakpoints {
    pub(super) all: Vec<Breakpoint>,
    /// The line under the mouse in a margin, for the faint dot.
    hover: Option<(usize, usize)>,
    /// Where the menu was opened.
    menu: Option<(PathBuf, u32)>,
    /// Where the run goes to once, and no dot is drawn.
    pub(super) once: Option<(PathBuf, u32)>,
}

impl Breakpoints {
    pub(super) fn at(&self, path: &Path, line: u32) -> Option<usize> {
        self.all
            .iter()
            .position(|breakpoint| breakpoint.line == line && breakpoint.path == path)
    }

    /// The files with breakpoints.
    pub(super) fn files(&self) -> Vec<PathBuf> {
        let mut files: Vec<PathBuf> = Vec::new();
        let once = self.once.iter().map(|(path, _)| path);
        for path in self
            .all
            .iter()
            .map(|breakpoint| &breakpoint.path)
            .chain(once)
        {
            if !files.contains(path) {
                files.push(path.clone());
            }
        }
        files
    }

    /// A file's breakpoints as a debugger is asked for them, with the line
    /// each is at.
    pub(super) fn asked(&self, path: &Path) -> Vec<SourceBreakpoint> {
        let mut asked: Vec<SourceBreakpoint> = self
            .all
            .iter()
            .filter(|breakpoint| breakpoint.path == path && !breakpoint.disabled)
            .map(|breakpoint| SourceBreakpoint {
                line: breakpoint.line + 1,
                condition: breakpoint.condition.clone(),
                hit_condition: breakpoint.hits.clone(),
                log_message: breakpoint.log.clone(),
            })
            .collect();
        if let Some((once, line)) = &self.once
            && once == path
            && !asked.iter().any(|breakpoint| breakpoint.line == line + 1)
        {
            asked.push(SourceBreakpoint {
                line: line + 1,
                ..SourceBreakpoint::default()
            });
        }
        asked
    }
}

/// Where lines go when a text changes: those above the change stay, those
/// below move with it, those in it stay within it.
fn moved_lines(old: &str, new: &str) -> impl Fn(u32) -> u32 {
    let before: Vec<&str> = old.split('\n').collect();
    let after: Vec<&str> = new.split('\n').collect();
    let shorter = before.len().min(after.len());
    let top = before
        .iter()
        .zip(&after)
        .take_while(|(a, b)| a == b)
        .count();
    let bottom = before
        .iter()
        .rev()
        .zip(after.iter().rev())
        .take_while(|(a, b)| a == b)
        .count()
        .min(shorter - top);
    let (before, after) = (before.len(), after.len());
    move |line| {
        let at = line as usize;
        if at < top {
            return line;
        }
        if at >= before - bottom {
            return (at + after - before) as u32;
        }
        let last = (after - bottom).saturating_sub(1).max(top);
        at.min(last).min(after.saturating_sub(1)) as u32
    }
}

impl Workbench {
    pub(super) fn breakpoint_actions(&self, root: Div, cx: &mut Context<Self>) -> Div {
        root.on_action(cx.listener(|this, _: &ToggleBreakpoint, _, cx| {
            if let Some((path, line)) = this.debug_cursor(cx) {
                this.toggle_breakpoint(&path, line, cx);
            }
        }))
        .on_action(cx.listener(|this, _: &RemoveAllBreakpoints, _, cx| {
            let files = this.debug.breakpoints.files();
            this.debug.breakpoints.all.clear();
            this.breakpoints_changed(&files, cx);
        }))
        .on_action(
            cx.listener(|this, _: &EditBreakpointCondition, window, cx| {
                this.ask_about_breakpoint(Ask::Condition, window, cx)
            }),
        )
        .on_action(cx.listener(|this, _: &EditBreakpointHits, window, cx| {
            this.ask_about_breakpoint(Ask::Hits, window, cx)
        }))
        .on_action(cx.listener(|this, _: &EditLogpoint, window, cx| {
            this.ask_about_breakpoint(Ask::Log, window, cx)
        }))
        .on_action(cx.listener(|this, _: &ToggleBreakpointEnabled, _, cx| {
            let Some((path, line)) = this.menu_place(cx) else {
                return;
            };
            if let Some(index) = this.debug.breakpoints.at(&path, line) {
                let breakpoint = &mut this.debug.breakpoints.all[index];
                breakpoint.disabled = !breakpoint.disabled;
                this.breakpoints_changed(&[path], cx);
            }
        }))
        .on_action(cx.listener(|this, _: &RemoveBreakpointHere, _, cx| {
            let Some((path, line)) = this.menu_place(cx) else {
                return;
            };
            if let Some(index) = this.debug.breakpoints.at(&path, line) {
                this.debug.breakpoints.all.remove(index);
                this.breakpoints_changed(&[path], cx);
            }
        }))
    }

    /// The file in front and the cursor's line.
    pub(super) fn debug_cursor(&self, cx: &App) -> Option<(PathBuf, u32)> {
        let pane = self.panes.get(self.active)?;
        let path = self.documents.get(&pane.uri)?.disk_path()?.to_path_buf();
        Some((path, pane.editor.read(cx).cursor_position().line))
    }

    /// The line the margin's menu was opened at, or the cursor's.
    fn menu_place(&mut self, cx: &App) -> Option<(PathBuf, u32)> {
        self.debug
            .breakpoints
            .menu
            .take()
            .or_else(|| self.debug_cursor(cx))
    }

    pub(super) fn toggle_breakpoint(&mut self, path: &Path, line: u32, cx: &mut Context<Self>) {
        let breakpoints = &mut self.debug.breakpoints;
        match breakpoints.at(path, line) {
            Some(index) => {
                breakpoints.all.remove(index);
            }
            None => breakpoints.all.push(Breakpoint {
                path: path.to_path_buf(),
                line,
                ..Breakpoint::default()
            }),
        }
        self.breakpoints_changed(&[path.to_path_buf()], cx);
    }

    /// Tells the debuggers, and keeps them with the project.
    pub(super) fn breakpoints_changed(&mut self, files: &[PathBuf], cx: &mut Context<Self>) {
        self.debug
            .breakpoints
            .all
            .sort_by(|a, b| (&a.path, a.line).cmp(&(&b.path, b.line)));
        for path in files {
            self.send_breakpoints(path, cx);
        }
        self.session_changed(cx);
        cx.notify();
    }

    /// Keeps the breakpoints of a file on their lines across an edit.
    pub(super) fn shift_breakpoints(&mut self, uri: &str, old: &str, new: &str) {
        let Some(path) = self
            .documents
            .get(uri)
            .and_then(|document| document.disk_path())
        else {
            return;
        };
        let all = &mut self.debug.breakpoints.all;
        if !all.iter().any(|breakpoint| breakpoint.path == path) {
            return;
        }
        let moved = moved_lines(old, new);
        let mut seen = HashSet::new();
        for breakpoint in all.iter_mut().filter(|breakpoint| breakpoint.path == path) {
            breakpoint.line = moved(breakpoint.line);
        }
        // Two on one line are one.
        all.retain(|breakpoint| {
            breakpoint.path != path || seen.insert((breakpoint.path.clone(), breakpoint.line))
        });
    }

    /// Asks for a condition, a count or a message, at the menu's line.
    fn ask_about_breakpoint(&mut self, ask: Ask, window: &mut Window, cx: &mut Context<Self>) {
        let Some((path, line)) = self.menu_place(cx) else {
            return;
        };
        let strings = self.strings();
        let known = self
            .debug
            .breakpoints
            .at(&path, line)
            .map(|index| self.debug.breakpoints.all[index].clone());
        let (title, value) = match ask {
            Ask::Condition => (
                strings.breakpoint_condition_hint,
                known.and_then(|known| known.condition),
            ),
            Ask::Hits => (
                strings.breakpoint_hits_hint,
                known.and_then(|known| known.hits),
            ),
            Ask::Log => (
                strings.breakpoint_log_hint,
                known.and_then(|known| known.log),
            ),
        };
        let field = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(title)
                .default_value(value.unwrap_or_default())
        });
        let this = cx.weak_entity();
        let shown = field.clone();
        window.open_dialog(cx, move |dialog, _, _| {
            let (this, read, path) = (this.clone(), shown.clone(), path.clone());
            dialog
                .w(px(460.))
                .title(title)
                .on_ok(move |_, _, cx| {
                    let text = read.read(cx).value().trim().to_owned();
                    this.update(cx, |this, cx| {
                        this.set_breakpoint_detail(&path, line, ask, text, cx)
                    })
                    .ok();
                    true
                })
                .child(Input::new(&shown).small())
        });
        field.update(cx, |field, cx| field.focus(window, cx));
    }

    /// A breakpoint's condition, count or message; empty takes it away.
    pub(super) fn set_breakpoint_detail(
        &mut self,
        path: &Path,
        line: u32,
        ask: Ask,
        text: String,
        cx: &mut Context<Self>,
    ) {
        let text = (!text.is_empty()).then_some(text);
        let breakpoints = &mut self.debug.breakpoints;
        let index = match breakpoints.at(path, line) {
            Some(index) => index,
            None if text.is_none() => return,
            None => {
                breakpoints.all.push(Breakpoint {
                    path: path.to_path_buf(),
                    line,
                    ..Breakpoint::default()
                });
                breakpoints.all.len() - 1
            }
        };
        let breakpoint = &mut breakpoints.all[index];
        match ask {
            Ask::Condition => breakpoint.condition = text,
            Ask::Hits => breakpoint.hits = text,
            Ask::Log => breakpoint.log = text,
        }
        self.breakpoints_changed(&[path.to_path_buf()], cx);
    }

    /// The margin of breakpoints beside an editor, for files a debugger
    /// can run, with an arrow at the line it is stopped at.
    pub(super) fn render_breakpoint_margin(
        &self,
        index: usize,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let pane = self.panes.get(index)?;
        let document = self.documents.get(&pane.uri)?;
        if !matches!(document.backing, Backing::File(_)) {
            return None;
        }
        let path = document.disk_path()?.to_path_buf();
        let has = self
            .debug
            .breakpoints
            .all
            .iter()
            .any(|breakpoint| breakpoint.path == path);
        let shown = self
            .settings
            .get("editor.glyphMargin")
            .and_then(Value::as_bool)
            .unwrap_or(true);
        if !has && (!shown || Adapter::of_language(&document.language_id).is_none()) {
            return None;
        }
        let editor = pane.editor.clone();
        let theme = cx.theme();
        let (red, grey, yellow, green) = (
            theme.danger,
            theme.muted_foreground,
            theme.warning,
            theme.success,
        );
        let hover = self
            .debug
            .breakpoints
            .hover
            .filter(|(pane, _)| *pane == index)
            .map(|(_, row)| row);
        let dots: Vec<(usize, Breakpoint, bool)> = self
            .debug
            .breakpoints
            .all
            .iter()
            .filter(|breakpoint| breakpoint.path == path)
            .map(|breakpoint| {
                let refused = self.breakpoint_refused(&breakpoint.path, breakpoint.line);
                (
                    breakpoint.line as usize,
                    breakpoint.clone(),
                    refused.is_some(),
                )
            })
            .collect();
        let stopped = self.stopped_line(&path);
        let rows: Rc<Rows> = Rc::default();
        let placed = rows.clone();
        // Painted after the editor is laid out: the dots never lag behind
        // the lines as it scrolls.
        let margin = canvas(
            |_, _, _| {},
            move |bounds, _, window, cx| {
                let state = editor.read(cx);
                let Some(visible) = state.visible_row_range() else {
                    return;
                };
                let text = state.text();
                let mut spots = Vec::new();
                let mut marks: Vec<(Bounds<Pixels>, usize)> = Vec::new();
                for row in visible {
                    let offset = text.line_start_offset(row);
                    let Some(at) = state.range_to_bounds(&(offset..offset)) else {
                        continue;
                    };
                    let spot = Bounds::new(
                        point(bounds.origin.x, at.origin.y),
                        gpui::size(bounds.size.width, at.size.height),
                    );
                    spots.push((spot, row));
                    marks.push((spot, row));
                }
                let size = px(10.);
                let dot = |spot: Bounds<Pixels>| {
                    Bounds::new(
                        point(
                            spot.origin.x + (spot.size.width - size) / 2.,
                            spot.origin.y + (spot.size.height - size) / 2.,
                        ),
                        gpui::size(size, size),
                    )
                };
                window.with_content_mask(Some(gpui::ContentMask { bounds }), |window| {
                    for (spot, row) in marks {
                        let set = dots.iter().find(|(line, _, _)| *line == row);
                        if let Some((_, breakpoint, refused)) = set {
                            let color = if breakpoint.disabled { grey } else { red };
                            // A logpoint is a diamond; one that asks
                            // something has a hole in it; one the debugger
                            // did not take is a ring.
                            let radius = if breakpoint.log.is_some() {
                                px(2.)
                            } else {
                                size / 2.
                            };
                            let bounds = dot(spot);
                            if *refused || breakpoint.disabled {
                                window.paint_quad(gpui::quad(
                                    bounds,
                                    radius,
                                    gpui::transparent_black(),
                                    px(1.5),
                                    if *refused { grey } else { color.opacity(0.8) },
                                    gpui::BorderStyle::default(),
                                ));
                            } else {
                                window.paint_quad(gpui::fill(bounds, color).corner_radii(radius));
                            }
                            if breakpoint.condition.is_some() || breakpoint.hits.is_some() {
                                let hole = Bounds::new(
                                    point(bounds.origin.x + px(2.5), bounds.origin.y + px(4.)),
                                    gpui::size(px(5.), px(2.)),
                                );
                                window.paint_quad(gpui::fill(hole, gpui::white()));
                            }
                        } else if hover == Some(row) {
                            window.paint_quad(
                                gpui::fill(dot(spot), red.opacity(0.35)).corner_radii(size / 2.),
                            );
                        }
                        if let Some((_, top)) = stopped.filter(|(line, _)| *line as usize == row) {
                            let color = if top { yellow } else { green };
                            let font = window.text_style().font();
                            let glyph = "➤";
                            let run = TextRun {
                                len: glyph.len(),
                                font,
                                color,
                                background_color: None,
                                underline: None,
                                strikethrough: None,
                            };
                            let size = px(13.);
                            let shaped = window.text_system().shape_line(
                                SharedString::from(glyph),
                                size,
                                &[run],
                                None,
                            );
                            let origin = point(
                                spot.origin.x + (spot.size.width - shaped.width) / 2.,
                                spot.origin.y + (spot.size.height - size * 1.3) / 2.,
                            );
                            let _ = shaped.paint(
                                origin,
                                size * 1.3,
                                gpui::TextAlign::Left,
                                None,
                                window,
                                cx,
                            );
                        }
                    }
                });
                *placed.borrow_mut() = spots;
            },
        )
        .size_full();
        let (clicked, hovered, opened) = (rows.clone(), rows.clone(), rows);
        let (toggle, menu) = (path.clone(), path);
        let strings = self.strings();
        Some(
            div()
                .id(("breakpoint-margin", index))
                .w(px(MARGIN))
                .h_full()
                .flex_none()
                .cursor_pointer()
                .on_mouse_move(cx.listener(move |this, event: &MouseMoveEvent, _, cx| {
                    let row = row_at(&hovered, event.position).map(|row| (index, row));
                    if this.debug.breakpoints.hover != row {
                        this.debug.breakpoints.hover = row;
                        cx.notify();
                    }
                }))
                .on_hover(cx.listener(|this, over: &bool, _, cx| {
                    if !*over && this.debug.breakpoints.hover.take().is_some() {
                        cx.notify();
                    }
                }))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                        if let Some(row) = row_at(&clicked, event.position) {
                            this.toggle_breakpoint(&toggle, row as u32, cx);
                        }
                    }),
                )
                .on_mouse_down(
                    MouseButton::Right,
                    cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                        this.debug.breakpoints.menu =
                            row_at(&opened, event.position).map(|row| (menu.clone(), row as u32));
                        cx.notify();
                    }),
                )
                .context_menu(move |menu, _, _| {
                    menu.menu(
                        strings.breakpoint_condition,
                        Box::new(EditBreakpointCondition),
                    )
                    .menu(strings.breakpoint_hits, Box::new(EditBreakpointHits))
                    .menu(strings.breakpoint_log, Box::new(EditLogpoint))
                    .separator()
                    .menu(
                        strings.breakpoint_toggle_enabled,
                        Box::new(ToggleBreakpointEnabled),
                    )
                    .menu(strings.breakpoint_remove, Box::new(RemoveBreakpointHere))
                    .menu(
                        strings.breakpoint_remove_all,
                        Box::new(RemoveAllBreakpoints),
                    )
                })
                .child(margin)
                .into_any_element(),
        )
    }
}

/// What is asked about a breakpoint.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Ask {
    Condition,
    Hits,
    Log,
}

/// Where each line of a margin was painted.
type Rows = RefCell<Vec<(Bounds<Pixels>, usize)>>;

fn row_at(rows: &Rows, position: gpui::Point<Pixels>) -> Option<usize> {
    rows.borrow()
        .iter()
        .find(|(bounds, _)| bounds.contains(&position))
        .map(|(_, row)| *row)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn breakpoints_stay_with_their_lines() {
        let old = "uno\ndue\ntre\nquattro\n";
        // A line above: everything below moves.
        let moved = moved_lines(old, "zero\nuno\ndue\ntre\nquattro\n");
        assert_eq!([0, 2, 3].map(&moved), [1, 3, 4]);
        // Enter at the start of a line: its text takes it along.
        let moved = moved_lines(old, "uno\n\ndue\ntre\nquattro\n");
        assert_eq!([0, 1, 2].map(&moved), [0, 2, 3]);
        // Typing on a line: it stays.
        let moved = moved_lines(old, "uno\ndue e mezzo\ntre\nquattro\n");
        assert_eq!([0, 1, 2].map(&moved), [0, 1, 2]);
        // Lines taken away: theirs goes to what is left there.
        let moved = moved_lines(old, "uno\nquattro\n");
        assert_eq!([0, 1, 2, 3].map(&moved), [0, 1, 1, 1]);
        let moved = moved_lines(old, "");
        assert_eq!(moved(3), 0);
    }

    #[test]
    fn what_a_debugger_is_asked() {
        let mut breakpoints = Breakpoints::default();
        let path = PathBuf::from("/a.rs");
        breakpoints.all = vec![
            Breakpoint {
                path: path.clone(),
                line: 3,
                condition: Some("x > 1".into()),
                ..Breakpoint::default()
            },
            Breakpoint {
                path: path.clone(),
                line: 5,
                disabled: true,
                ..Breakpoint::default()
            },
        ];
        breakpoints.once = Some((path.clone(), 9));
        let asked = breakpoints.asked(&path);
        assert_eq!(
            asked.iter().map(|asked| asked.line).collect::<Vec<_>>(),
            [4, 10]
        );
        assert_eq!(asked[0].condition.as_deref(), Some("x > 1"));
        assert_eq!(breakpoints.files(), [path]);
        // Kept as VS Code's would be, with only what they have.
        let text = serde_json::to_string(&breakpoints.all[1]).unwrap();
        assert_eq!(text, r#"{"path":"/a.rs","line":5,"disabled":true}"#);
    }
}
