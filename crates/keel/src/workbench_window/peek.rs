//! Peek, as VS Code's: a definition or the references, in a frame at the
//! bottom of the editor, without leaving the file. Each place shows its
//! lines with their colours, the place's own line marked; with several,
//! the list sits at the left. Open goes there for real; Esc closes it.

use std::collections::HashMap;
use std::rc::Rc;

use gpui::{AnyElement, SharedString, StatefulInteractiveElement as _, StyledText, relative};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::{Icon, Sizable as _};
use gpui_kit_assets::IconName;

use super::code_navigation::Place;
use super::diff_pane::Side;
use super::shell::Colors;
use super::*;

/// The lines shown above a place, and in all.
const ABOVE: usize = 6;
const SHOWN: usize = 18;

/// A file's lines and their colours.
type Code = Rc<(Vec<String>, Side)>;

pub(super) struct Peek {
    title: String,
    pub(super) places: Vec<Place>,
    selected: usize,
    /// The pane it opened in.
    pane: usize,
    /// Each file's lines with their colours, made when first shown.
    sides: std::cell::RefCell<HashMap<String, Code>>,
}

impl Workbench {
    pub(super) fn open_peek(&mut self, title: String, places: Vec<Place>, cx: &mut Context<Self>) {
        self.peek = Some(Peek {
            title,
            places,
            selected: 0,
            pane: self.active,
            sides: Default::default(),
        });
        cx.notify();
    }

    pub(super) fn close_peek(&mut self, cx: &mut Context<Self>) -> bool {
        let open = self.peek.take().is_some();
        if open {
            cx.notify();
        }
        open
    }

    fn peek_open_place(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(peek) = self.peek.take() else {
            return;
        };
        if let Some(place) = peek.places.get(peek.selected).cloned() {
            self.open_lsp_range(&place.uri, place.range, OpenAs::Normal, window, cx);
        }
        cx.notify();
    }

    /// The frame over pane `index`, when the peek opened there.
    pub(super) fn render_peek(
        &self,
        index: usize,
        colors: Colors,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let peek = self.peek.as_ref().filter(|peek| peek.pane == index)?;
        let strings = self.strings();
        let place = peek.places.get(peek.selected)?;
        let side = {
            let mut sides = peek.sides.borrow_mut();
            sides
                .entry(place.uri.clone())
                .or_insert_with(|| {
                    let text = self.text_of(&place.uri).unwrap_or_default();
                    let lines: Vec<String> = text.lines().map(str::to_owned).collect();
                    let language = Uri::parse(&place.uri)
                        .and_then(|uri| uri.file_path())
                        .map(|path| Language::for_path(&path).highlighter_name())
                        .unwrap_or_default();
                    let side = Side::new(&lines, language);
                    Rc::new((lines, side))
                })
                .clone()
        };
        let (lines, side) = (&side.0, &side.1);
        let target = place.range.start.line as usize;
        let first = target.saturating_sub(ABOVE);
        let font = cx.theme().mono_font_family.clone();
        let font_size = px(self.font_size.max(9.) - 1.);
        let line_height = (font_size * 1.55).round();
        let code: Vec<AnyElement> = (first..(first + SHOWN).min(lines.len()))
            .map(|line| {
                let text = lines[line].clone();
                let styles: Vec<_> = side
                    .styles(line, cx)
                    .iter()
                    .filter(|(range, _)| range.end <= text.len())
                    .cloned()
                    .collect();
                div()
                    .flex()
                    .h(line_height)
                    .when(line == target, |row| row.bg(colors.accent.opacity(0.14)))
                    .child(
                        div()
                            .flex_none()
                            .w(px(48.))
                            .pr_2()
                            .text_right()
                            .text_color(colors.faint)
                            .child((line + 1).to_string()),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .whitespace_nowrap()
                            .overflow_hidden()
                            .child(
                                StyledText::new(SharedString::from(text)).with_highlights(styles),
                            ),
                    )
                    .into_any_element()
            })
            .collect();
        let list = (peek.places.len() > 1).then(|| {
            div()
                .id("peek-places")
                .flex_none()
                .w(px(260.))
                .h_full()
                .overflow_y_scroll()
                .border_r_1()
                .border_color(colors.border)
                .py_1()
                .children(peek.places.iter().enumerate().map(|(position, place)| {
                    let text = self
                        .text_of(&place.uri)
                        .and_then(|text| {
                            text.lines()
                                .nth(place.range.start.line as usize)
                                .map(|line| line.trim().to_owned())
                        })
                        .unwrap_or_default();
                    let name = self.shown_path(&place.uri);
                    let name = name.rsplit('/').next().unwrap_or(&name).to_owned();
                    div()
                        .id(("peek-place", position))
                        .mx_1()
                        .px_2()
                        .py_1()
                        .rounded(px(6.))
                        .cursor_pointer()
                        .when(position == peek.selected, |row| row.bg(colors.selected))
                        .hover(|row| row.bg(colors.hover))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if let Some(peek) = &mut this.peek {
                                peek.selected = position;
                            }
                            cx.notify();
                        }))
                        .child(
                            div()
                                .text_xs()
                                .text_color(colors.muted)
                                .child(format!("{name}:{}", place.range.start.line + 1)),
                        )
                        .child(div().text_sm().truncate().child(text))
                        .into_any_element()
                }))
        });
        let header = div()
            .h(px(32.))
            .flex_none()
            .px_3()
            .flex()
            .items_center()
            .gap_2()
            .border_b_1()
            .border_color(colors.border)
            .child(
                Icon::new(IconName::TextSearch)
                    .small()
                    .text_color(colors.accent),
            )
            .child(
                div()
                    .text_sm()
                    .font_weight(FontWeight::MEDIUM)
                    .child(peek.title.clone()),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_xs()
                    .text_color(colors.muted)
                    .child(format!("{}:{}", self.shown_path(&place.uri), target + 1)),
            )
            .child(
                Button::new("peek-open")
                    .ghost()
                    .small()
                    .label(strings.peek_open)
                    .on_click(cx.listener(|this, _, window, cx| this.peek_open_place(window, cx))),
            )
            .child(
                Button::new("peek-close")
                    .ghost()
                    .small()
                    .icon(Icon::new(IconName::X))
                    .tooltip(strings.peek_close)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.close_peek(cx);
                    })),
            );
        Some(
            div()
                .id("peek")
                .absolute()
                .left_0()
                .right_0()
                .bottom_0()
                .h(relative(0.46))
                .flex()
                .flex_col()
                .bg(colors.panel)
                .border_t_2()
                .border_color(colors.accent.opacity(0.7))
                .shadow_lg()
                .occlude()
                .child(header)
                .child(
                    div().flex_1().min_h_0().flex().children(list).child(
                        div()
                            .id("peek-code")
                            .flex_1()
                            .min_w_0()
                            .py_1()
                            .font_family(font)
                            .text_size(font_size)
                            .line_height(line_height)
                            .text_color(colors.text)
                            .overflow_y_scroll()
                            .children(code),
                    ),
                )
                .into_any_element(),
        )
    }
}
