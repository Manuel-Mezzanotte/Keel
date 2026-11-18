//! What Keel draws over and beside an editor, as VS Code does:
//!
//! - **sticky scroll**: the first lines of the blocks the top of the view is
//!   inside (a function, a class...), kept at the top while scrolling;
//!   a click goes to the line;
//! - **the minimap**: the whole file in small, with the part in view
//!   shaded; a click or a drag scrolls there.
//!
//! Each is a view of its own that follows its editor, so scrolling redraws
//! only them, not the window.

use std::rc::Rc;
use std::time::{Duration, Instant};

use gpui::{
    Bounds, MouseButton, MouseDownEvent, MouseMoveEvent, Pixels, Render, Subscription, canvas,
    fill, point, size,
};
use gpui_component::ActiveTheme as _;
use gpui_component::input::RopeExt as _;
use gpui_component::input::{EditorState, Position};

use super::*;

/// VS Code's `editor.stickyScroll.maxLineCount`.
const MAX_STICKY_LINES: usize = 5;
/// The minimap's width, and the height of one line in it at most.
pub(super) const MINIMAP_WIDTH: f32 = 64.;
const MINIMAP_LINE: f32 = 2.;
/// How often the minimap measures the lines again while typing.
const MINIMAP_REBUILD: Duration = Duration::from_millis(250);

/// The lines that stay at the top: for the top line `first` of the view,
/// the first lines of the blocks (`(start, end)`, from the fold candidates)
/// that started above it and are still open, outermost first.
pub(super) fn sticky_lines(blocks: &[(usize, usize)], first: usize) -> Vec<usize> {
    let mut lines: Vec<usize> = Vec::new();
    // The headers cover lines of their own: look under them, as VS Code.
    for _ in 0..2 {
        let top = first + lines.len();
        lines = blocks
            .iter()
            .filter(|(start, end)| *start < top && *end > top)
            .map(|(start, _)| *start)
            .collect();
        lines.sort_unstable();
        lines.dedup();
        lines.truncate(MAX_STICKY_LINES);
    }
    lines
}

pub(super) struct StickyScroll {
    editor: Entity<EditorState>,
    font_size: f32,
    _observe: Subscription,
}

impl StickyScroll {
    pub(super) fn new(editor: Entity<EditorState>, font_size: f32, cx: &mut Context<Self>) -> Self {
        let observe = cx.observe(&editor, |_, _, cx| cx.notify());
        Self {
            editor,
            font_size,
            _observe: observe,
        }
    }

    pub(super) fn set_font_size(&mut self, font_size: f32, cx: &mut Context<Self>) {
        if self.font_size != font_size {
            self.font_size = font_size;
            cx.notify();
        }
    }
}

impl StickyScroll {
    /// The lines that stay at the top now.
    pub(super) fn lines(&self, cx: &App) -> Vec<usize> {
        let editor = self.editor.read(cx);
        let Some(visible) = editor.visible_buffer_lines() else {
            return Vec::new();
        };
        // At the very top nothing is hidden yet.
        if editor.scroll_offset().y >= px(0.) {
            return Vec::new();
        }
        let blocks: Vec<(usize, usize)> = editor
            .display_map()
            .fold_candidates()
            .iter()
            .map(|range| (range.start_line, range.end_line))
            .collect();
        sticky_lines(&blocks, visible.start)
    }
}

impl Render for StickyScroll {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let lines = self.lines(cx);
        let editor = self.editor.read(cx);
        let (Some(line_height), Some(gutter)) = (editor.line_height(), editor.gutter_width())
        else {
            return div().into_any_element();
        };
        if lines.is_empty() {
            return div().into_any_element();
        }
        let text = editor.text();
        let theme = cx.theme();
        let rows = lines.into_iter().map(|line| {
            let content: String = text
                .slice_line(line)
                .to_string()
                .trim_end_matches(['\n', '\r'])
                .replace('\t', "    ");
            let editor = self.editor.clone();
            div()
                .id(("sticky", line))
                .h(line_height)
                .flex()
                .items_center()
                .cursor_pointer()
                .hover(|row| row.bg(theme.muted))
                .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                    editor.update(cx, |editor, cx| {
                        editor.set_cursor_position(Position::new(line as u32, 0), window, cx);
                        editor.focus(window, cx);
                    });
                })
                .child(
                    div()
                        .w(gutter)
                        .flex_none()
                        .pr_2()
                        .text_right()
                        .text_color(theme.muted_foreground)
                        .child((line + 1).to_string()),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .text_color(theme.foreground)
                        .child(content),
                )
        });
        div()
            .absolute()
            .top_0()
            .left_0()
            .right_0()
            .bg(theme.background)
            .border_b_1()
            .border_color(theme.border)
            .shadow_sm()
            .font_family(theme.mono_font_family.clone())
            .text_size(px(self.font_size))
            .children(rows)
            .into_any_element()
    }
}

/// One line in the minimap: where its text starts and how long it is, in
/// characters.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct MinimapLine {
    pub(super) indent: u16,
    pub(super) length: u16,
}

pub(super) fn measure_lines(text: &gpui_component::Rope) -> Vec<MinimapLine> {
    text.iter_lines()
        .map(|line| {
            let line = line.to_string();
            let line = line.trim_end_matches(['\n', '\r']);
            let indent = line.len() - line.trim_start().len();
            MinimapLine {
                indent: indent.min(u16::MAX as usize) as u16,
                length: line.trim_start().chars().count().min(u16::MAX as usize) as u16,
            }
        })
        .collect()
}

/// The shapes of the minimap for `lines` in a column `height` tall: lines
/// that fall on the same pixel row merge into one, so a file of any length
/// costs at most one rectangle per row. Each is `(top, height, indent,
/// length)` in pixels and characters.
pub(super) fn minimap_rows(lines: &[MinimapLine], height: f32) -> Vec<(f32, f32, u16, u16)> {
    if lines.is_empty() || height <= 0. {
        return Vec::new();
    }
    let step = (height / lines.len() as f32).min(MINIMAP_LINE);
    let mut rows: Vec<(f32, f32, u16, u16)> = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        if line.length == 0 {
            continue;
        }
        let top = (index as f32 * step).floor();
        match rows.last_mut() {
            Some(row) if row.0 == top => {
                row.2 = row.2.min(line.indent);
                row.3 = row.3.max(line.length);
            }
            _ => rows.push((top, step.max(1.), line.indent, line.length)),
        }
    }
    rows
}

pub(super) struct Minimap {
    editor: Entity<EditorState>,
    lines: Rc<Vec<MinimapLine>>,
    dirty: bool,
    built: Option<Instant>,
    _subscriptions: Vec<Subscription>,
}

impl Minimap {
    pub(super) fn new(editor: Entity<EditorState>, cx: &mut Context<Self>) -> Self {
        let observe = cx.observe(&editor, |_, _, cx| cx.notify());
        let changes = cx.subscribe(
            &editor,
            |this, _, event: &gpui_component::input::InputEvent, cx| {
                if matches!(event, gpui_component::input::InputEvent::Change) {
                    this.dirty = true;
                    cx.notify();
                }
            },
        );
        Self {
            editor,
            lines: Rc::default(),
            dirty: true,
            built: None,
            _subscriptions: vec![observe, changes],
        }
    }

    /// Measures the lines again, at most every quarter of a second while
    /// typing, so long files stay quick.
    fn refresh(&mut self, cx: &mut Context<Self>) {
        if !self.dirty {
            return;
        }
        if let Some(built) = self.built
            && built.elapsed() < MINIMAP_REBUILD
        {
            let wait = MINIMAP_REBUILD - built.elapsed();
            cx.spawn(async move |this, cx| {
                cx.background_executor().timer(wait).await;
                this.update(cx, |_, cx| cx.notify()).ok();
            })
            .detach();
            return;
        }
        self.lines = Rc::new(measure_lines(self.editor.read(cx).text()));
        self.dirty = false;
        self.built = Some(Instant::now());
    }

    /// Scrolls the editor so the line under `y` of the minimap is in the
    /// middle of the view.
    pub(super) fn scroll_to(
        editor: &Entity<EditorState>,
        y: f32,
        height: f32,
        lines: usize,
        cx: &mut App,
    ) {
        if lines == 0 || height <= 0. {
            return;
        }
        let step = (height / lines as f32).min(MINIMAP_LINE);
        let line = ((y / step) as usize).min(lines - 1);
        editor.update(cx, |editor, cx| {
            let (Some(line_height), Some(visible)) =
                (editor.line_height(), editor.visible_buffer_lines())
            else {
                return;
            };
            let row = editor.display_map().buffer_line_to_display_row(line);
            let half = visible.len() / 2;
            let top = row.saturating_sub(half) as f32;
            let x = editor.scroll_offset().x;
            editor.set_scroll_offset(point(x, -line_height * top), cx);
        });
    }
}

impl Render for Minimap {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.refresh(cx);
        let editor = self.editor.read(cx);
        let visible = editor.visible_buffer_lines();
        let theme = cx.theme();
        let ink = theme.muted_foreground.opacity(0.45);
        let shade = theme.foreground.opacity(0.08);
        let lines = self.lines.clone();
        let count = lines.len();
        // Where the minimap is on screen, for the mouse.
        let area: Rc<std::cell::Cell<Bounds<Pixels>>> = Rc::default();
        let paint_area = area.clone();
        let (down_area, move_area) = (area.clone(), area);
        let (down_editor, move_editor) = (self.editor.clone(), self.editor.clone());
        div()
            .id("minimap")
            .w(px(MINIMAP_WIDTH))
            .h_full()
            .flex_none()
            .border_l_1()
            .border_color(theme.border.opacity(0.5))
            .cursor_pointer()
            .child(
                canvas(
                    move |bounds, _, _| paint_area.set(bounds),
                    move |bounds: Bounds<Pixels>, _, window, _| {
                        let height = f32::from(bounds.size.height);
                        let width = f32::from(bounds.size.width) - 6.;
                        let step = (height / count.max(1) as f32).min(MINIMAP_LINE);
                        for (top, tall, indent, length) in minimap_rows(&lines, height) {
                            let left = (indent as f32).min(width);
                            let wide = (length as f32).min(width - left).max(1.);
                            window.paint_quad(fill(
                                Bounds::new(
                                    point(
                                        bounds.origin.x + px(3. + left),
                                        bounds.origin.y + px(top),
                                    ),
                                    size(px(wide), px(tall.min(1.5))),
                                ),
                                ink,
                            ));
                        }
                        if let Some(visible) = &visible {
                            let top = visible.start as f32 * step;
                            let tall = (visible.len() as f32 * step).max(4.);
                            window.paint_quad(fill(
                                Bounds::new(
                                    point(bounds.origin.x, bounds.origin.y + px(top)),
                                    size(bounds.size.width, px(tall)),
                                ),
                                shade,
                            ));
                        }
                    },
                )
                .size_full(),
            )
            .on_mouse_down(MouseButton::Left, move |event: &MouseDownEvent, _, cx| {
                let area = down_area.get();
                let y = f32::from(event.position.y - area.origin.y);
                Minimap::scroll_to(&down_editor, y, f32::from(area.size.height), count, cx);
            })
            .on_mouse_move(move |event: &MouseMoveEvent, _, cx| {
                if event.pressed_button != Some(MouseButton::Left) {
                    return;
                }
                let area = move_area.get();
                let y = f32::from(event.position.y - area.origin.y);
                Minimap::scroll_to(&move_editor, y, f32::from(area.size.height), count, cx);
            })
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_blocks_around_the_top_line_stay() {
        // fn a() {        0..10
        //   if x {        2..6
        //     ...
        let blocks = [(0, 10), (2, 6), (12, 20)];
        assert_eq!(sticky_lines(&blocks, 0), Vec::<usize>::new());
        assert_eq!(sticky_lines(&blocks, 1), [0]);
        assert_eq!(sticky_lines(&blocks, 3), [0, 2]);
        // Past the inner block's end, only the function stays.
        assert_eq!(sticky_lines(&blocks, 7), [0]);
        assert_eq!(sticky_lines(&blocks, 11), Vec::<usize>::new());
    }

    #[test]
    fn at_most_five_lines_stay() {
        let blocks: Vec<(usize, usize)> = (0..8).map(|start| (start, 100 - start)).collect();
        assert_eq!(sticky_lines(&blocks, 50), [0, 1, 2, 3, 4]);
    }

    #[test]
    fn long_files_merge_into_one_row_per_pixel() {
        let text = gpui_component::Rope::from("fn main() {\n    let a = 1;\n\n}\n");
        let lines = measure_lines(&text);
        assert_eq!(
            lines[1],
            MinimapLine {
                indent: 4,
                length: 10
            }
        );
        assert_eq!(lines[2].length, 0);
        // Few lines: two pixels each, empty ones left out.
        let rows = minimap_rows(&lines, 100.);
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[1].0, 2.);
        // 200,000 lines in 500 pixels: at most 500 rows.
        let many = vec![
            MinimapLine {
                indent: 0,
                length: 40
            };
            200_000
        ];
        assert!(minimap_rows(&many, 500.).len() <= 500);
    }
}
