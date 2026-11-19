//! Small movements that say what happened, as Apple's and Linear's apps
//! make them: a number that changes rolls in from below, rows that are on
//! their way pulse in grey, a row that arrives in its list slides in. All
//! stand still with Reduce Motion or `keel.animations` off.

use std::time::Duration;

use gpui::{Animation, AnimationExt as _, AnyElement, ElementId, Hsla, SharedString, relative};

use super::look::{Look, settle};
use super::*;

/// A number that rolls in from below when it changes.
pub(super) fn ticker(name: &str, value: usize, look: Look) -> AnyElement {
    let text = div().relative().child(value.to_string());
    if !look.motion {
        return text.into_any_element();
    }
    // A new id for each value starts the roll again.
    let id = ElementId::Name(SharedString::from(format!("ticker-{name}-{value}")));
    div()
        .overflow_hidden()
        .child(text.with_animation(
            id,
            Animation::new(Duration::from_millis(220)).with_easing(settle),
            |text, delta| text.top(px((1. - delta) * 7.)).opacity(0.25 + 0.75 * delta),
        ))
        .into_any_element()
}

/// Grey rows pulsing where a list is on its way, as GitHub and Linear
/// show them instead of an empty space.
pub(super) fn placeholder_rows(count: usize, color: Hsla, look: Look) -> AnyElement {
    let widths = [0.62, 0.48, 0.74, 0.55, 0.68, 0.4, 0.58, 0.7];
    let rows = div()
        .flex()
        .flex_col()
        .gap_2()
        .px_3()
        .py_2()
        .children((0..count).map(move |index| {
            div()
                .h(px(10.))
                .w(relative(widths[index % widths.len()]))
                .rounded(px(5.))
                .bg(color)
        }));
    if !look.motion {
        return rows.opacity(0.6).into_any_element();
    }
    rows.with_animation(
        "placeholder-rows",
        Animation::new(Duration::from_millis(1200)).repeat(),
        |rows, delta| {
            // Up and down again, softly.
            let wave = (delta * std::f32::consts::TAU).sin() * 0.5 + 0.5;
            rows.opacity(0.35 + 0.45 * wave)
        },
    )
    .into_any_element()
}

/// A row that just came into its list slides in from the left.
pub(super) fn arriving(row: AnyElement, key: &str, look: Look) -> AnyElement {
    if !look.motion {
        return row;
    }
    div()
        .relative()
        .child(row)
        .with_animation(
            ElementId::Name(SharedString::from(format!("arrive-{key}"))),
            Animation::new(Duration::from_millis(260)).with_easing(settle),
            |row, delta| row.left(px((1. - delta) * -14.)).opacity(delta),
        )
        .into_any_element()
}
