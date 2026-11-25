//! The cards that open over Claude's message box: how Claude works (the
//! mode), the model with its reasoning and fast mode, and the usage (how
//! full the chat's context is, the plan's limits, what the chat cost). And
//! the small ring that shows the context in the message box.

use std::time::{Duration, Instant};

use ai::session_meta::{ConfigOption, LimitKind};
use gpui::{
    AnyElement, DragMoveEvent, PathBuilder, StatefulInteractiveElement as _, canvas, point,
};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::switch::Switch;
use gpui_component::{ActiveTheme as _, Icon, Sizable as _};
use gpui_kit_assets::IconName;

use super::agent_view::{claude_colour, spinner};
use super::shell::Colors;
use super::*;

/// The knob of the reasoning slider, while it is dragged.
#[derive(Clone)]
pub(super) struct EffortDrag;

impl gpui::Render for EffortDrag {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        gpui::Empty
    }
}

/// Which card is open over the message box.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ComposerPopover {
    Mode,
    Model,
    Usage,
}

/// Under this width the buttons under the message box keep to symbols.
const NARROW_PANEL: f32 = 400.;
/// How many models the model card shows before "More models".
const MAIN_MODELS: usize = 5;
/// A click on a card's button this soon after a click outside closed it
/// leaves it closed.
const REOPEN_GUARD: Duration = Duration::from_millis(300);

/// A mode's name, words and symbol in Keel's language, for the modes
/// Claude Code has; others keep the agent's name.
fn mode_look(
    value: &str,
    strings: &'static Strings,
) -> Option<(&'static str, &'static str, IconName)> {
    Some(match value {
        "default" => (
            strings.agent_mode_default,
            strings.agent_mode_default_hint,
            IconName::ShieldCheck,
        ),
        "acceptEdits" => (
            strings.agent_mode_accept,
            strings.agent_mode_accept_hint,
            IconName::Pencil,
        ),
        "plan" => (
            strings.agent_mode_plan,
            strings.agent_mode_plan_hint,
            IconName::ListChecks,
        ),
        "auto" => (
            strings.agent_mode_auto,
            strings.agent_mode_auto_hint,
            IconName::Sparkles,
        ),
        "bypassPermissions" => (
            strings.agent_mode_bypass,
            strings.agent_mode_bypass_hint,
            IconName::ShieldAlert,
        ),
        "dontAsk" => (
            strings.agent_mode_dont_ask,
            strings.agent_mode_dont_ask_hint,
            IconName::Ban,
        ),
        _ => return None,
    })
}

/// A reasoning level's short name in Keel's language.
pub(super) fn effort_name(value: &str, fallback: &str, strings: &'static Strings) -> String {
    match value {
        "default" => strings.agent_effort_auto,
        "low" => strings.agent_effort_low,
        "medium" => strings.agent_effort_medium,
        "high" => strings.agent_effort_high,
        "xhigh" => strings.agent_effort_xhigh,
        "max" => strings.agent_effort_max,
        _ => fallback,
    }
    .to_owned()
}

/// 313000 as "313k", 1000000 as "1M".
pub(super) fn short_count(count: u64) -> String {
    let trimmed = |value: f64, unit: &str| {
        let text = format!("{value:.1}");
        format!("{}{unit}", text.trim_end_matches(".0"))
    };
    match count {
        count if count >= 1_000_000 => trimmed(count as f64 / 1_000_000., "M"),
        count if count >= 1_000 => format!("{}k", (count as f64 / 1_000.).round()),
        count => count.to_string(),
    }
}

/// A ring filled clockwise from the top up to `fraction`.
pub(super) fn ring(
    fraction: f32,
    size: f32,
    width: f32,
    track: Hsla,
    fill: Hsla,
) -> impl IntoElement {
    let fraction = fraction.clamp(0., 1.);
    canvas(
        |_, _, _| {},
        move |bounds, _, window, _| {
            let radius = (size - width) / 2.;
            let center = bounds.center();
            let at = |turn: f32| {
                let angle = (turn - 0.25) * std::f32::consts::TAU;
                point(
                    center.x + px(radius * angle.cos()),
                    center.y + px(radius * angle.sin()),
                )
            };
            let arc = |from: f32, to: f32| {
                let mut path = PathBuilder::stroke(px(width));
                path.move_to(at(from));
                path.arc_to(
                    point(px(radius), px(radius)),
                    px(0.),
                    to - from > 0.5,
                    true,
                    at(to),
                );
                path.build().ok()
            };
            for (from, to) in [(0., 0.5), (0.5, 1.)] {
                if let Some(path) = arc(from, to) {
                    window.paint_path(path, track);
                }
            }
            if fraction > 0.005 {
                let end = fraction.min(0.999);
                let halves: &[(f32, f32)] = if end > 0.5 {
                    &[(0., 0.5), (0.5, end)]
                } else {
                    &[(0., end)]
                };
                for &(from, to) in halves {
                    if let Some(path) = arc(from, to) {
                        window.paint_path(path, fill);
                    }
                }
            }
        },
    )
    .size(px(size))
    .flex_none()
}

/// The level under `x` on a slider `width` wide with `count` levels of the
/// same width; before the start the first, past the end the last.
fn effort_index(x: f32, width: f32, count: usize) -> usize {
    if count == 0 {
        return 0;
    }
    let fraction = x / width.max(1.);
    ((fraction * count as f32).floor().max(0.) as usize).min(count - 1)
}

/// The colour of a bar that fills up: calm, then a warning, then red.
fn level_colour(fraction: f32, calm: Hsla, cx: &App) -> Hsla {
    match fraction {
        f if f >= 0.95 => cx.theme().danger,
        f if f >= 0.8 => cx.theme().warning,
        _ => calm,
    }
}

impl Workbench {
    /// A card's button: opens its card, or closes it.
    pub(super) fn toggle_composer_popover(
        &mut self,
        kind: ComposerPopover,
        cx: &mut Context<Self>,
    ) {
        let panel = &mut self.agent_panel;
        let just_closed = panel
            .popover_closed
            .take()
            .is_some_and(|(closed, at)| closed == kind && at.elapsed() < REOPEN_GUARD);
        if panel.popover == Some(kind) || just_closed {
            panel.popover = None;
        } else {
            panel.popover = Some(kind);
            if kind == ComposerPopover::Usage {
                self.refresh_plan_usage(false, cx);
            }
        }
        cx.notify();
    }

    fn close_composer_popover(&mut self, cx: &mut Context<Self>) {
        if let Some(kind) = self.agent_panel.popover.take() {
            self.agent_panel.popover_closed = Some((kind, Instant::now()));
            cx.notify();
        }
    }

    /// The mode's button under the message box.
    pub(super) fn render_mode_button(
        &self,
        colors: Colors,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let strings = self.strings();
        let mode = self.agent_option("mode")?;
        let current = mode.current.as_deref().unwrap_or_default();
        let (name, icon) = match mode_look(current, strings) {
            Some((name, _, icon)) => (name.to_owned(), icon),
            None => (
                mode.current_name().unwrap_or(strings.agent_mode).to_owned(),
                IconName::ShieldCheck,
            ),
        };
        let open = self.agent_panel.popover == Some(ComposerPopover::Mode);
        let risky = current == "bypassPermissions";
        // In a narrow panel only the symbol, so the send button stays in.
        let narrow = self.agent_width < NARROW_PANEL;
        Some(
            Button::new("agent-mode")
                .ghost()
                .xsmall()
                .tooltip(name.clone())
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_1()
                        .text_color(if risky {
                            cx.theme().warning
                        } else {
                            colors.muted
                        })
                        .child(Icon::new(icon).xsmall())
                        .when(!narrow, |label| label.child(name)),
                )
                .when(open, |button| button.bg(colors.hover))
                .on_click(cx.listener(|this, _, _, cx| {
                    this.toggle_composer_popover(ComposerPopover::Mode, cx)
                }))
                .into_any_element(),
        )
    }

    /// The model's button: "Opus 5.5 · High".
    pub(super) fn render_model_button(
        &self,
        colors: Colors,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let strings = self.strings();
        let model = self.agent_option("model")?;
        let name = model
            .current_name()
            .unwrap_or(strings.agent_model)
            .split(" (")
            .next()
            .unwrap_or_default()
            .to_owned();
        let effort = self.agent_option("thought_level").and_then(|effort| {
            let current = effort.current.as_deref()?;
            (current != "default")
                .then(|| effort_name(current, effort.current_name().unwrap_or(current), strings))
        });
        let fast = self
            .agent_option("fast")
            .is_some_and(|fast| fast.current.as_deref() == Some("on"));
        let open = self.agent_panel.popover == Some(ComposerPopover::Model);
        Some(
            Button::new("agent-model")
                .ghost()
                .xsmall()
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_1()
                        .text_color(colors.muted)
                        .when(fast, |label| {
                            label.child(
                                Icon::new(IconName::Zap)
                                    .xsmall()
                                    .text_color(claude_colour()),
                            )
                        })
                        .child(match effort {
                            Some(effort) => format!("{name} · {effort}"),
                            None => name,
                        })
                        .child(
                            Icon::new(IconName::ChevronUp)
                                .xsmall()
                                .text_color(colors.faint),
                        ),
                )
                .when(open, |button| button.bg(colors.hover))
                .on_click(cx.listener(|this, _, _, cx| {
                    this.toggle_composer_popover(ComposerPopover::Model, cx)
                }))
                .into_any_element(),
        )
    }

    /// The context's ring in the message box, with its percentage.
    pub(super) fn render_context_button(
        &self,
        colors: Colors,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let panel = &self.agent_panel;
        panel.session.as_ref()?;
        let fraction = panel
            .context
            .as_ref()
            .map_or(0., |context| context.fraction());
        let open = panel.popover == Some(ComposerPopover::Usage);
        let fill = level_colour(fraction, colors.muted, cx);
        Some(
            div()
                .id("agent-context")
                .h(px(22.))
                .px(px(5.))
                .flex()
                .items_center()
                .gap(px(5.))
                .rounded(px(6.))
                .cursor_pointer()
                .when(open, |button| button.bg(colors.hover))
                .hover(|button| button.bg(colors.hover))
                .on_click(cx.listener(|this, _, _, cx| {
                    this.toggle_composer_popover(ComposerPopover::Usage, cx)
                }))
                .child(ring(fraction, 14., 2.2, colors.border, fill))
                .when(self.agent_width >= NARROW_PANEL, |button| {
                    button.child(
                        div()
                            .text_size(px(11.))
                            .text_color(colors.faint)
                            .child(format!("{:.0}%", fraction * 100.)),
                    )
                })
                .into_any_element(),
        )
    }

    /// The card open over the message box, if any.
    pub(super) fn render_composer_popover(
        &self,
        colors: Colors,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let kind = self.agent_panel.popover?;
        let body = match kind {
            ComposerPopover::Mode => self.render_mode_card(colors, cx)?,
            ComposerPopover::Model => self.render_model_card(colors, cx)?,
            ComposerPopover::Usage => self.render_usage_card(colors, cx),
        };
        Some(
            div()
                .id("agent-popover")
                .occlude()
                .absolute()
                .left(px(10.))
                .right(px(10.))
                .bottom(gpui::relative(1.))
                .max_h(px(560.))
                .overflow_y_scroll()
                .p(px(6.))
                .flex()
                .flex_col()
                .gap(px(2.))
                .rounded(px(12.))
                .border_1()
                .border_color(colors.border)
                .bg(cx.theme().popover)
                .shadow_lg()
                .on_mouse_down_out(cx.listener(|this, _, _, cx| this.close_composer_popover(cx)))
                .child(body)
                .into_any_element(),
        )
    }

    fn render_mode_card(&self, colors: Colors, cx: &mut Context<Self>) -> Option<AnyElement> {
        let strings = self.strings();
        let mode = self.agent_option("mode")?.clone();
        let current = mode.current.clone().unwrap_or_default();
        let mut card = div()
            .flex()
            .flex_col()
            .gap(px(2.))
            .child(card_heading(strings.agent_mode_title, colors));
        for (index, value) in mode.values.iter().enumerate() {
            let (name, hint, icon) = match mode_look(&value.value, strings) {
                Some((name, hint, icon)) => (name.to_owned(), Some(hint.to_owned()), icon),
                None => (
                    value.name.clone(),
                    value.description.clone(),
                    IconName::ShieldCheck,
                ),
            };
            let risky = value.value == "bypassPermissions";
            let chosen = value.value.clone();
            let id = mode.id.clone();
            card = card.child(
                option_row(
                    ("agent-mode-row", index),
                    Some(icon),
                    name,
                    hint,
                    current == value.value,
                    risky.then(|| cx.theme().warning),
                    colors,
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.agent_panel.popover = None;
                    this.set_agent_option(id.clone(), chosen.clone(), cx);
                })),
            );
        }
        Some(card.into_any_element())
    }

    fn render_model_card(&self, colors: Colors, cx: &mut Context<Self>) -> Option<AnyElement> {
        let strings = self.strings();
        let model: ConfigOption = self.agent_option("model")?.clone();
        let effort = self.agent_option("thought_level").cloned();
        let fast = self.agent_option("fast").cloned();
        let current = model.current.clone().unwrap_or_default();
        let more = self.agent_panel.more_models;
        let mut card = div()
            .flex()
            .flex_col()
            .gap(px(2.))
            .child(card_heading(strings.agent_model, colors));
        let hidden = model.values.len().saturating_sub(MAIN_MODELS);
        for (index, value) in model.values.iter().enumerate() {
            let chosen_now = value.value == current;
            if index >= MAIN_MODELS && !more && !chosen_now {
                continue;
            }
            let (id, chosen) = (model.id.clone(), value.value.clone());
            let name = value.name.split(" (").next().unwrap_or_default().to_owned();
            // "Default (recommended)": the words in brackets go under it.
            let hint = value.description.clone().or_else(|| {
                value
                    .name
                    .split_once(" (")
                    .map(|(_, rest)| rest.trim_end_matches(')').to_owned())
            });
            card = card.child(
                option_row(
                    ("agent-model-row", index),
                    None,
                    name,
                    hint,
                    chosen_now,
                    None,
                    colors,
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.agent_panel.popover = None;
                    this.set_agent_option(id.clone(), chosen.clone(), cx);
                })),
            );
        }
        if hidden > 0 {
            card = card.child(
                div()
                    .id("agent-more-models")
                    .px(px(8.))
                    .py(px(5.))
                    .flex()
                    .items_center()
                    .gap_1()
                    .rounded(px(8.))
                    .text_size(px(12.))
                    .text_color(colors.muted)
                    .cursor_pointer()
                    .hover(|row| row.bg(colors.hover))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.agent_panel.more_models = !this.agent_panel.more_models;
                        cx.notify();
                    }))
                    .child(
                        Icon::new(if more {
                            IconName::ChevronUp
                        } else {
                            IconName::ChevronDown
                        })
                        .xsmall(),
                    )
                    .child(if more {
                        strings.agent_fewer_models.to_owned()
                    } else {
                        strings.agent_more_models(hidden)
                    }),
            );
        }
        if let Some(effort) = effort {
            card = card
                .child(divider(colors))
                .child(self.render_effort_bar(&effort, colors, cx));
        }
        if let Some(fast) = fast {
            let on = fast.current.as_deref() == Some("on");
            let id = fast.id.clone();
            card =
                card.child(divider(colors)).child(
                    div()
                        .px(px(8.))
                        .py(px(6.))
                        .flex()
                        .items_center()
                        .gap(px(10.))
                        .child(Icon::new(IconName::Zap).small().text_color(claude_colour()))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .flex()
                                .flex_col()
                                .child(
                                    div()
                                        .text_size(px(13.))
                                        .text_color(colors.text)
                                        .child(strings.agent_fast),
                                )
                                .child(
                                    div()
                                        .text_size(px(11.5))
                                        .text_color(colors.faint)
                                        .child(strings.agent_fast_hint),
                                ),
                        )
                        .child(Switch::new("agent-fast").checked(on).small().on_click(
                            cx.listener(move |this, checked: &bool, _, cx| {
                                let value = if *checked { "on" } else { "off" };
                                this.set_agent_option(id.clone(), value.to_owned(), cx)
                            }),
                        )),
                );
        }
        Some(card.into_any_element())
    }

    /// The reasoning as a slider: a rounded track filled up to the level
    /// chosen, with a knob to drag and a stop for each level, its name
    /// under it; a click on a stop or a name picks it too. "Auto" (Claude
    /// Code's own choice) is a switch beside the heading.
    fn render_effort_bar(
        &self,
        effort: &ConfigOption,
        colors: Colors,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let strings = self.strings();
        let current = effort.current.clone().unwrap_or_default();
        let levels: Vec<_> = effort
            .values
            .iter()
            .filter(|value| value.value != "default")
            .cloned()
            .collect();
        let has_auto = effort.values.iter().any(|value| value.value == "default");
        let chosen = levels.iter().position(|value| value.value == current);
        let count = levels.len().max(1);
        // Each level has a cell of the same width; its stop is the cell's
        // middle, so the names line up under the stops.
        let stop = |index: usize| (index as f32 + 0.5) / count as f32;
        let id = effort.id.clone();
        let auto_on = chosen.is_none();
        let accent = claude_colour();
        let name_of =
            |value: &ai::session_meta::OptionValue| effort_name(&value.value, &value.name, strings);

        let heading = div()
            .flex()
            .items_center()
            .child(
                div()
                    .flex_1()
                    .child(card_heading(strings.agent_effort, colors)),
            )
            .when(has_auto, |row| {
                let option = id.clone();
                let first = levels
                    .iter()
                    .find(|value| value.value == "high")
                    .or(levels.first())
                    .map(|value| value.value.clone());
                row.child(
                    div()
                        .pr(px(8.))
                        .flex()
                        .items_center()
                        .gap(px(6.))
                        .text_size(px(11.5))
                        .text_color(if auto_on { colors.text } else { colors.muted })
                        .child(strings.agent_effort_auto)
                        .child(
                            Switch::new("agent-effort-auto")
                                .checked(auto_on)
                                .xsmall()
                                .on_click(cx.listener(move |this, on: &bool, _, cx| {
                                    // Off: back to a level, "High" when there is one.
                                    let value = if *on {
                                        Some("default".to_owned())
                                    } else {
                                        first.clone()
                                    };
                                    if let Some(value) = value {
                                        this.set_agent_option(option.clone(), value, cx)
                                    }
                                })),
                        ),
                )
            });

        let mut slider = div()
            .id("agent-effort-slider")
            .relative()
            .h(px(28.))
            .cursor_pointer()
            .on_drag(EffortDrag, |drag, _, _, cx| cx.new(|_| drag.clone()))
            .on_drag_move::<EffortDrag>(cx.listener({
                let (id, levels) = (id.clone(), levels.clone());
                move |this, event: &DragMoveEvent<EffortDrag>, _, cx| {
                    let bounds = event.bounds;
                    let x = f32::from(event.event.position.x - bounds.left());
                    let index = effort_index(x, f32::from(bounds.size.width), levels.len());
                    if let Some(value) = levels.get(index) {
                        let chosen = this
                            .agent_option(&id)
                            .and_then(|option| option.current.clone());
                        if chosen.as_deref() != Some(value.value.as_str()) {
                            this.set_agent_option(id.clone(), value.value.clone(), cx);
                        }
                    }
                }
            }))
            // The track.
            .child(
                div()
                    .absolute()
                    .left_0()
                    .right_0()
                    .top(px(7.))
                    .h(px(14.))
                    .p(px(2.))
                    .rounded_full()
                    .border_1()
                    .border_color(colors.muted.opacity(if auto_on { 0.35 } else { 0.6 }))
                    .bg(colors.hover.opacity(0.4))
                    .children(chosen.map(|index| {
                        div()
                            .h_full()
                            .w(gpui::relative(stop(index)))
                            .rounded_full()
                            .bg(accent)
                    })),
            );
        // A dot on each stop, light on the filled part.
        for index in 0..levels.len() {
            let filled = chosen.is_some_and(|chosen| index < chosen);
            slider = slider.child(
                div()
                    .absolute()
                    .top(px(12.))
                    .left(gpui::relative(stop(index)))
                    .ml(px(-2.))
                    .size(px(4.))
                    .rounded_full()
                    .bg(if filled {
                        gpui::white().opacity(0.75)
                    } else {
                        colors.muted.opacity(0.55)
                    }),
            );
        }
        // The knob, on the stop chosen.
        if let Some(index) = chosen {
            slider = slider.child(
                div()
                    .absolute()
                    .top(px(4.))
                    .left(gpui::relative(stop(index)))
                    .ml(px(-10.))
                    .size(px(20.))
                    .rounded_full()
                    .border_2()
                    .border_color(accent)
                    .bg(gpui::white())
                    .shadow_md(),
            );
        }
        // The cells to click, over the track.
        let mut cells = div().absolute().inset_0().flex();
        let mut names = div().flex();
        for (index, value) in levels.iter().enumerate() {
            let on = chosen == Some(index);
            let pick = || {
                let (option, level) = (id.clone(), value.value.clone());
                cx.listener(move |this: &mut Workbench, _: &gpui::ClickEvent, _, cx| {
                    this.set_agent_option(option.clone(), level.clone(), cx)
                })
            };
            cells = cells.child(
                div()
                    .id(("agent-effort", index))
                    .flex_1()
                    .h_full()
                    .on_click(pick()),
            );
            names = names.child(
                div()
                    .id(("agent-effort-name", index))
                    .flex_1()
                    .flex()
                    .justify_center()
                    .py(px(2.))
                    .rounded(px(6.))
                    .cursor_pointer()
                    .text_size(px(11.))
                    .text_color(if on { colors.text } else { colors.faint })
                    .when(on, |name| name.font_weight(FontWeight::SEMIBOLD))
                    .when(!on, |name| name.hover(|name| name.text_color(colors.muted)))
                    .on_click(pick())
                    .child(name_of(value)),
            );
        }
        let slider = slider.child(cells);
        let hint = match chosen.and_then(|index| levels.get(index)) {
            Some(value) => match value.value.as_str() {
                "low" => strings.agent_effort_low_hint,
                "medium" => strings.agent_effort_medium_hint,
                "high" => strings.agent_effort_high_hint,
                "xhigh" => strings.agent_effort_xhigh_hint,
                "max" => strings.agent_effort_max_hint,
                _ => "",
            },
            None => strings.agent_effort_auto_hint,
        };
        div()
            .flex()
            .flex_col()
            .child(heading)
            .child(
                div()
                    .px(px(10.))
                    .pt(px(2.))
                    .pb(px(8.))
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .child(slider)
                    .child(names)
                    .child(
                        div()
                            .pt(px(4.))
                            .text_size(px(11.5))
                            .text_color(colors.muted)
                            .child(hint),
                    ),
            )
            .into_any_element()
    }

    /// The context of the chat, the plan's limits, the cost.
    fn render_usage_card(&self, colors: Colors, cx: &mut Context<Self>) -> AnyElement {
        let strings = self.strings();
        let panel = &self.agent_panel;
        let italian = self.language == UiLanguage::Italian;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |elapsed| elapsed.as_secs() as i64);
        let context = panel.context.clone();
        let fraction = context.as_ref().map_or(0., |context| context.fraction());
        let fill = level_colour(fraction, claude_colour(), cx);
        let mut card = div().p(px(6.)).flex().flex_col().gap(px(12.)).child(
            div()
                .flex()
                .items_center()
                .gap(px(12.))
                .child(ring(fraction, 40., 4., colors.border, fill))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .gap(px(1.))
                        .child(
                            div()
                                .flex()
                                .items_baseline()
                                .gap_2()
                                .child(
                                    div()
                                        .text_size(px(12.))
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_color(colors.muted)
                                        .child(strings.agent_context),
                                )
                                .child(
                                    div()
                                        .text_size(px(18.))
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_color(colors.text)
                                        .child(format!("{:.0}%", fraction * 100.)),
                                ),
                        )
                        .children(context.as_ref().map(|context| {
                            div().text_size(px(12.)).text_color(colors.faint).child(
                                strings.agent_context_tokens(
                                    &short_count(context.used),
                                    &short_count(context.size),
                                ),
                            )
                        })),
                ),
        );
        if panel.uses_api_key {
            if let Some(cost) = context.as_ref().and_then(|context| context.cost) {
                card = card.child(
                    div()
                        .text_size(px(13.))
                        .text_color(colors.text)
                        .child(strings.agent_chat_cost(&format!("${cost:.2}"))),
                );
            }
            return card
                .child(
                    div()
                        .text_size(px(12.))
                        .line_height(px(17.))
                        .text_color(colors.muted)
                        .child(strings.agent_key_billing),
                )
                .child(
                    div().child(
                        Button::new("agent-open-console")
                            .outline()
                            .small()
                            .icon(Icon::new(IconName::ExternalLink))
                            .label(strings.agent_open_console)
                            .on_click(|_, _, cx| {
                                cx.open_url("https://console.anthropic.com/settings/billing")
                            }),
                    ),
                )
                .into_any_element();
        }
        let usage = panel.plan_usage.clone();
        let mut limits = div().flex().flex_col().gap(px(10.));
        let limit_row = |label: String, percent: f32, resets: Option<i64>| {
            let fraction = percent / 100.;
            let bar = level_colour(fraction, claude_colour(), cx);
            div()
                .flex()
                .flex_col()
                .gap(px(5.))
                .child(
                    div()
                        .flex()
                        .items_baseline()
                        .gap_2()
                        .child(
                            div()
                                .flex_1()
                                .text_size(px(12.5))
                                .text_color(colors.text)
                                .child(label),
                        )
                        .child(
                            div()
                                .text_size(px(12.5))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(colors.text)
                                .child(format!("{percent:.0}%")),
                        ),
                )
                .child(
                    div().h(px(5.)).rounded_full().bg(colors.hover).child(
                        div()
                            .h_full()
                            .w(gpui::relative(fraction.clamp(0., 1.)))
                            .rounded_full()
                            .bg(bar),
                    ),
                )
                .children(resets.filter(|resets| *resets > now).map(|resets| {
                    div().text_size(px(11.)).text_color(colors.faint).child(
                        strings.agent_resets_in(&super::scm_graph::relative_time(
                            now, resets, italian,
                        )),
                    )
                }))
        };
        let mut any = false;
        if let Some(usage) = &usage {
            let mut ordered = usage.limits.clone();
            ordered.sort_by_key(|limit| match limit.kind {
                LimitKind::FiveHours => 0,
                LimitKind::Week => 1,
                LimitKind::WeekOf(_) => 2,
            });
            for limit in ordered {
                let label = match &limit.kind {
                    LimitKind::FiveHours => strings.agent_limit_five_hours.to_owned(),
                    LimitKind::Week => strings.agent_limit_week.to_owned(),
                    LimitKind::WeekOf(model) => strings.agent_limit_week_of(model),
                };
                limits = limits.child(limit_row(label, limit.percent, limit.resets));
                any = true;
            }
        }
        let plan = usage
            .as_ref()
            .and_then(|usage| usage.plan.clone())
            .map(|plan| {
                let mut letters = plan.chars();
                let first = letters
                    .next()
                    .map(|first| first.to_uppercase().collect::<String>());
                strings.agent_plan_name(&format!(
                    "{}{}",
                    first.unwrap_or_default(),
                    letters.as_str()
                ))
            });
        card = card.child(divider(colors)).children(plan.map(|plan| {
            div()
                .text_size(px(11.))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(colors.faint)
                .child(plan.to_uppercase())
        }));
        if any {
            card = card.child(limits);
        } else if panel.reading_usage {
            card = card.child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .text_size(px(12.))
                    .text_color(colors.muted)
                    .child(spinner(colors.accent, self.look.motion))
                    .child(strings.agent_reading_limits),
            );
        } else {
            card = card.child(
                div()
                    .text_size(px(12.))
                    .text_color(colors.muted)
                    .child(strings.agent_limits_unavailable),
            );
        }
        let updated = panel.usage_read.map(|read| {
            let seconds = read.elapsed().as_secs() as i64;
            if seconds < 60 {
                strings.agent_updated_now.to_owned()
            } else {
                strings.agent_updated(&super::scm_graph::relative_time(
                    now - seconds,
                    now,
                    italian,
                ))
            }
        });
        card.child(
            div()
                .flex()
                .items_center()
                .child(
                    div()
                        .flex_1()
                        .text_size(px(11.))
                        .text_color(colors.faint)
                        .children(updated),
                )
                .child(
                    Button::new("agent-usage-refresh")
                        .ghost()
                        .xsmall()
                        .icon(Icon::new(IconName::RefreshCw))
                        .tooltip(strings.agent_refresh)
                        .loading(panel.reading_usage)
                        .on_click(cx.listener(|this, _, _, cx| this.refresh_plan_usage(true, cx))),
                ),
        )
        .into_any_element()
    }
}

fn card_heading(text: &'static str, colors: Colors) -> gpui::Div {
    div()
        .px(px(8.))
        .pt(px(4.))
        .pb(px(2.))
        .text_size(px(11.))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(colors.faint)
        .child(text.to_uppercase())
}

fn divider(colors: Colors) -> gpui::Div {
    div().mx(px(8.)).my(px(4.)).h(px(1.)).bg(colors.border)
}

/// A choice in a card: its symbol, name and words, a tick when chosen.
fn option_row(
    id: impl Into<gpui::ElementId>,
    icon: Option<IconName>,
    name: String,
    hint: Option<String>,
    chosen: bool,
    tint: Option<Hsla>,
    colors: Colors,
) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .px(px(8.))
        .py(px(5.))
        .flex()
        .items_center()
        .gap(px(10.))
        .rounded(px(8.))
        .cursor_pointer()
        .when(chosen, |row| row.bg(claude_colour().opacity(0.12)))
        .when(!chosen, |row| {
            row.hover(|row| row.bg(colors.hover.opacity(0.7)))
        })
        .children(icon.map(|icon| {
            Icon::new(icon)
                .small()
                .flex_none()
                .text_color(tint.unwrap_or(if chosen { colors.text } else { colors.muted }))
        }))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .child(
                    div()
                        .text_size(px(13.))
                        .text_color(tint.unwrap_or(colors.text))
                        .font_weight(if chosen {
                            FontWeight::MEDIUM
                        } else {
                            FontWeight::NORMAL
                        })
                        .child(name),
                )
                .children(hint.map(|hint| {
                    div()
                        .truncate()
                        .text_size(px(11.))
                        .text_color(colors.faint)
                        .child(hint)
                })),
        )
        .when(chosen, |row| {
            row.child(
                Icon::new(IconName::Check)
                    .small()
                    .flex_none()
                    .text_color(claude_colour()),
            )
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_slider_level_under_the_pointer() {
        assert_eq!(effort_index(-20., 500., 5), 0);
        assert_eq!(effort_index(99., 500., 5), 0);
        assert_eq!(effort_index(100., 500., 5), 1);
        assert_eq!(effort_index(250., 500., 5), 2);
        assert_eq!(effort_index(499., 500., 5), 4);
        assert_eq!(effort_index(900., 500., 5), 4);
        assert_eq!(effort_index(10., 500., 0), 0);
    }

    #[test]
    fn counts_are_short() {
        assert_eq!(short_count(313_000), "313k");
        assert_eq!(short_count(1_000_000), "1M");
        assert_eq!(short_count(1_500_000), "1.5M");
        assert_eq!(short_count(950), "950");
    }
}
