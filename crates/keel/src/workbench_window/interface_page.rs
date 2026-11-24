//! The Interface page of Settings: the window's style and its colors,
//! chosen by looking at them rather than by name, and its movements.

use std::rc::Rc;

use gpui::{
    AnyElement, AnyWindowHandle, Div, SharedString, StatefulInteractiveElement as _, WeakEntity,
    relative,
};
use gpui_component::{Icon, Sizable as _, theme::ThemeRegistry};
use gpui_kit_assets::IconName;

use super::look::{InterfaceStyle, STYLE_SETTING};
use super::settings_view::writer;
use super::shell::Colors;
use super::*;

/// Runs `change` on the window a moment later: the page's controls call it
/// while the window is busy with the click.
fn later(
    workbench: &WeakEntity<Workbench>,
    handle: AnyWindowHandle,
    change: impl Fn(&mut Workbench, &mut Window, &mut Context<Workbench>) + 'static,
) -> Rc<dyn Fn(&mut App)> {
    let workbench = workbench.clone();
    let change = Rc::new(change);
    Rc::new(move |cx: &mut App| {
        let workbench = workbench.clone();
        let change = change.clone();
        cx.defer(move |cx| {
            let _ = handle.update(cx, |_, window, cx| {
                workbench
                    .update(cx, |workbench, cx| change(workbench, window, cx))
                    .ok();
            });
        });
    })
}

/// What each style is called, and says of itself.
fn style_words(style: InterfaceStyle, italian: bool) -> (&'static str, &'static str) {
    match (style, italian) {
        (InterfaceStyle::Floating, true) => (
            "Pannelli sospesi",
            "Card arrotondate su uno sfondo più scuro, come il Finder di macOS.",
        ),
        (InterfaceStyle::Floating, false) => (
            "Floating Panels",
            "Rounded cards on a darker background, as macOS's Finder.",
        ),
        (InterfaceStyle::Flat, true) => (
            "Piatto",
            "Pannelli uno accanto all'altro, divisi da linee sottili, come Cursor.",
        ),
        (InterfaceStyle::Flat, false) => (
            "Flat",
            "Panels side by side, divided by thin lines, as Cursor.",
        ),
    }
}

/// A small drawing of the window in `style`, in the theme's colors.
fn style_preview(style: InterfaceStyle, colors: Colors) -> Div {
    let flat = style == InterfaceStyle::Flat;
    let look = super::look::Look {
        style,
        motion: false,
        ..super::look::Look::default()
    };
    let desk = look.desk(colors, colors.dark);
    let line =
        |width: f32, color: Hsla| div().h(px(3.)).w(relative(width)).rounded_full().bg(color);
    let code = div()
        .flex()
        .flex_col()
        .gap(px(4.))
        .p(px(6.))
        .child(line(0.55, colors.accent.opacity(0.8)))
        .child(line(0.8, colors.muted.opacity(0.5)))
        .child(line(0.4, colors.muted.opacity(0.5)))
        .child(line(0.65, colors.accent.opacity(0.45)))
        .child(line(0.3, colors.muted.opacity(0.5)));
    let card = |background: Hsla| {
        div().h_full().bg(background).when(!flat, |card| {
            card.rounded(px(5.)).border_1().border_color(colors.border)
        })
    };
    let rows = |count: usize| {
        div()
            .flex()
            .flex_col()
            .gap(px(4.))
            .p(px(5.))
            .children((0..count).map(|index| {
                div()
                    .h(px(3.))
                    .w(relative(if index % 2 == 0 { 0.8 } else { 0.55 }))
                    .rounded_full()
                    .bg(colors.muted.opacity(0.4))
            }))
    };
    div()
        .w_full()
        .h(px(112.))
        .flex()
        .flex_col()
        .overflow_hidden()
        .rounded(px(8.))
        .border_1()
        .border_color(colors.border)
        .bg(desk)
        // The toolbar, with the traffic lights.
        .child(
            div()
                .h(px(16.))
                .flex_none()
                .px(px(6.))
                .flex()
                .items_center()
                .gap(px(3.))
                .when(flat, |bar| bar.border_b_1().border_color(colors.border))
                .children(
                    [
                        gpui::rgb(0xFF5F57),
                        gpui::rgb(0xFEBC2E),
                        gpui::rgb(0x28C840),
                    ]
                    .map(|color| div().size(px(5.)).rounded_full().bg(color)),
                ),
        )
        .child(
            div()
                .flex_1()
                .min_h_0()
                .flex()
                .when(!flat, |body| body.gap(px(4.)).px(px(4.)).pb(px(4.)))
                .child(
                    card(colors.panel)
                        .w(relative(0.26))
                        .when(flat, |side| side.border_r_1().border_color(colors.border))
                        .child(rows(5)),
                )
                .child(
                    card(colors.editor)
                        .flex_1()
                        .child(
                            div()
                                .h(px(10.))
                                .flex()
                                .when(flat, |tabs| {
                                    tabs.bg(colors.panel)
                                        .border_b_1()
                                        .border_color(colors.border)
                                })
                                .child(
                                    div()
                                        .w(px(26.))
                                        .h_full()
                                        .bg(colors.editor)
                                        .when(flat, |tab| {
                                            tab.border_r_1().border_color(colors.border)
                                        })
                                        .when(!flat, |tab| {
                                            tab.m(px(2.)).rounded(px(3.)).bg(colors.selected)
                                        }),
                                ),
                        )
                        .child(code),
                )
                .child(
                    card(colors.panel)
                        .w(relative(0.22))
                        .when(flat, |agent| agent.border_l_1().border_color(colors.border))
                        .child(rows(3)),
                ),
        )
        .when(flat, |window| {
            window.child(
                div()
                    .h(px(7.))
                    .flex_none()
                    .border_t_1()
                    .border_color(colors.border),
            )
        })
}

/// The two styles side by side, each drawn small; a click chooses one.
pub(super) fn style_picker(
    workbench_now: &Workbench,
    workbench: &WeakEntity<Workbench>,
    handle: AnyWindowHandle,
    italian: bool,
    colors: Colors,
) -> AnyElement {
    let write = writer(workbench, handle, STYLE_SETTING.into());
    let now = workbench_now.look.style;
    div()
        .w_full()
        .flex()
        .gap_3()
        .children(
            InterfaceStyle::ALL
                .into_iter()
                .enumerate()
                .map(|(index, style)| {
                    let on = style == now;
                    let write = write.clone();
                    let (title, words) = style_words(style, italian);
                    div()
                        .id(("interface-style", index))
                        .flex_1()
                        .min_w_0()
                        .p_2()
                        .flex()
                        .flex_col()
                        .gap_2()
                        .rounded(px(12.))
                        .border_2()
                        .border_color(if on {
                            colors.accent
                        } else {
                            colors.border.opacity(0.)
                        })
                        .bg(if on {
                            colors.accent.opacity(0.06)
                        } else {
                            gpui::transparent_black()
                        })
                        .cursor_pointer()
                        .when(!on, |card| {
                            card.hover(|card| card.bg(colors.hover.opacity(0.6)))
                        })
                        .on_click(move |_, _, cx| write(Value::String(style.id().to_owned()), cx))
                        .child(style_preview(style, colors))
                        .child(
                            div()
                                .px_1()
                                .flex()
                                .items_center()
                                .gap_2()
                                .child(radio(on, colors))
                                .child(
                                    div()
                                        .text_sm()
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_color(colors.text)
                                        .child(title),
                                ),
                        )
                        .child(
                            div()
                                .px_1()
                                .pb_1()
                                .text_xs()
                                .text_color(colors.muted)
                                .child(words),
                        )
                }),
        )
        .into_any_element()
}

/// A round mark, filled when chosen, as macOS's radio buttons.
fn radio(on: bool, colors: Colors) -> Div {
    div()
        .size(px(16.))
        .flex_none()
        .rounded_full()
        .border_1()
        .border_color(if on { colors.accent } else { colors.border })
        .flex()
        .items_center()
        .justify_center()
        .when(on, |mark| {
            mark.bg(colors.accent)
                .child(div().size(px(6.)).rounded_full().bg(gpui::white()))
        })
}

/// A theme as a small picture: its editor, its sidebar and its accent.
struct Swatch {
    name: SharedString,
    background: Hsla,
    sidebar: Hsla,
    accent: Hsla,
    text: Hsla,
}

/// The themes Keel offers, its own three first, as the theme command
/// lists them. A theme not loaded yet is drawn in the colors on screen.
fn swatches(names: &[String], colors: Colors, cx: &App) -> Vec<Swatch> {
    let parse = |color: &Option<SharedString>, fallback: Hsla| {
        color
            .as_ref()
            .and_then(|color| gpui_component::theme::try_parse_color(color).ok())
            .unwrap_or(fallback)
    };
    let registry = ThemeRegistry::global(cx);
    names
        .iter()
        .map(|name| {
            let name: SharedString = name.clone().into();
            match registry.themes().get(&name) {
                Some(theme) => {
                    let dark = theme.mode.is_dark();
                    let background = parse(
                        &theme.colors.background,
                        if dark { gpui::black() } else { gpui::white() },
                    );
                    Swatch {
                        name,
                        background,
                        sidebar: parse(&theme.colors.sidebar, background),
                        accent: parse(&theme.colors.primary, colors.accent),
                        text: parse(
                            &theme.colors.foreground,
                            if dark { gpui::white() } else { gpui::black() },
                        ),
                    }
                }
                None => Swatch {
                    name,
                    background: colors.editor,
                    sidebar: colors.panel,
                    accent: colors.accent,
                    text: colors.text,
                },
            }
        })
        .collect()
}

/// The themes as small pictures; a click shows one at once.
pub(super) fn theme_picker(
    workbench_now: &Workbench,
    workbench: &WeakEntity<Workbench>,
    handle: AnyWindowHandle,
    colors: Colors,
    cx: &App,
) -> AnyElement {
    // The theme on screen, which is the preferred one for macOS's mode
    // when Keel follows it.
    let shown = |cx: &App| -> String {
        let theme = gpui_component::Theme::global(cx);
        let current = if theme.mode.is_dark() {
            &theme.dark_theme.name
        } else {
            &theme.light_theme.name
        };
        current.to_string()
    };
    let current = shown(cx);
    let names = workbench_now.theme_names();
    div()
        .w_full()
        .flex()
        .flex_wrap()
        .gap_2()
        .children(
            swatches(&names, colors, cx)
                .into_iter()
                .enumerate()
                .map(|(index, swatch)| {
                    let on = swatch.name.as_ref() == current;
                    let name = swatch.name.to_string();
                    let choose = later(workbench, handle, move |workbench, window, cx| {
                        workbench.set_theme(&name, window, cx)
                    });
                    div()
                        .id(("interface-theme", index))
                        .w(px(132.))
                        .p_1()
                        .flex()
                        .flex_col()
                        .gap_1p5()
                        .rounded(px(10.))
                        .border_2()
                        .border_color(if on {
                            colors.accent
                        } else {
                            colors.border.opacity(0.)
                        })
                        .cursor_pointer()
                        .when(!on, |card| {
                            card.hover(|card| card.bg(colors.hover.opacity(0.6)))
                        })
                        .on_click(move |_, _, cx| choose(cx))
                        .child(
                            div()
                                .h(px(64.))
                                .flex()
                                .overflow_hidden()
                                .rounded(px(7.))
                                .border_1()
                                .border_color(colors.border)
                                .bg(swatch.background)
                                .child(div().w(px(30.)).h_full().bg(swatch.sidebar))
                                .child(
                                    div()
                                        .flex_1()
                                        .p(px(7.))
                                        .flex()
                                        .flex_col()
                                        .gap(px(5.))
                                        .child(
                                            div()
                                                .h(px(4.))
                                                .w(relative(0.6))
                                                .rounded_full()
                                                .bg(swatch.accent),
                                        )
                                        .child(
                                            div()
                                                .h(px(4.))
                                                .w(relative(0.85))
                                                .rounded_full()
                                                .bg(swatch.text.opacity(0.55)),
                                        )
                                        .child(
                                            div()
                                                .h(px(4.))
                                                .w(relative(0.45))
                                                .rounded_full()
                                                .bg(swatch.text.opacity(0.35)),
                                        ),
                                ),
                        )
                        .child(
                            div()
                                .px_1()
                                .flex()
                                .items_center()
                                .gap_1()
                                .text_xs()
                                .text_color(if on { colors.text } else { colors.muted })
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .truncate()
                                        .child(swatch.name.clone()),
                                )
                                .when(on, |label| {
                                    label.child(
                                        Icon::new(IconName::Check)
                                            .xsmall()
                                            .text_color(colors.accent),
                                    )
                                }),
                        )
                }),
        )
        .into_any_element()
}
