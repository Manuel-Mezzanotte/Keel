//! The two styles of the window, chosen with `keel.interfaceStyle`:
//! - `floating`, sketch D: rounded panels floating on the window's
//!   background, as Finder and Xcode on macOS Tahoe;
//! - `flat`: the panels edge to edge, divided by hairlines, as Cursor and
//!   VS Code draw them.
//!
//! The controls, the views and the overlays are the same in both: only the
//! frame around them changes. `keel.animations` turns the movements off, as
//! macOS's Reduce Motion does.

use std::time::Duration;

use gpui::{App, BoxShadow, ElementId, Hsla, Pixels, Window, hsla, point, px};
use settings::Settings;

use super::shell::Colors;

pub(crate) const STYLE_SETTING: &str = "keel.interfaceStyle";
pub(crate) const MOTION_SETTING: &str = "keel.animations";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum InterfaceStyle {
    Floating,
    Flat,
}

impl InterfaceStyle {
    pub(crate) const ALL: [InterfaceStyle; 2] = [InterfaceStyle::Floating, InterfaceStyle::Flat];

    /// The value in `settings.json`.
    pub(crate) fn id(self) -> &'static str {
        match self {
            InterfaceStyle::Floating => "floating",
            InterfaceStyle::Flat => "flat",
        }
    }

    /// Anything but `flat` is the floating panels, Keel's first style.
    pub(crate) fn from_id(id: Option<&str>) -> Self {
        match id {
            Some("flat") => InterfaceStyle::Flat,
            _ => InterfaceStyle::Floating,
        }
    }
}

/// How the window looks: its style, and whether things move.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Look {
    pub(crate) style: InterfaceStyle,
    pub(crate) motion: bool,
}

/// Whether things move when nobody says otherwise. Tests run still: their
/// clock moves only when they move it, and a panel halfway in would stay so.
const MOTION_BY_DEFAULT: bool = cfg!(not(test));

impl Default for Look {
    fn default() -> Self {
        Self {
            style: InterfaceStyle::Floating,
            motion: MOTION_BY_DEFAULT,
        }
    }
}

/// The toolbar is lower in the flat style, as Cursor's title bar.
const FLOATING_TOOLBAR: f32 = 52.;
const FLAT_TOOLBAR: f32 = 40.;

impl Look {
    pub(crate) fn from_settings(settings: &Settings) -> Self {
        Self {
            style: InterfaceStyle::from_id(settings.string(STYLE_SETTING)),
            motion: settings
                .get(MOTION_SETTING)
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(MOTION_BY_DEFAULT),
        }
    }

    pub(crate) fn flat(self) -> bool {
        self.style == InterfaceStyle::Flat
    }

    /// The space between the panels and around them.
    pub(crate) fn gap(self) -> f32 {
        if self.flat() { 0. } else { 8. }
    }

    /// The corners of the panels.
    pub(crate) fn radius(self) -> f32 {
        if self.flat() { 0. } else { 12. }
    }

    pub(crate) fn toolbar_height(self) -> f32 {
        if self.flat() {
            FLAT_TOOLBAR
        } else {
            FLOATING_TOOLBAR
        }
    }

    pub(crate) fn status_height(self) -> f32 {
        if self.flat() { 24. } else { 26. }
    }

    /// The rows on top of the panels: the editors' tabs, the bottom
    /// panel's and Claude Code's headers. In the flat style the sidebar's
    /// switch too, so that their lines meet across the window.
    pub(crate) fn bar_height(self) -> f32 {
        if self.flat() { 36. } else { 40. }
    }

    /// Where macOS's traffic lights sit: centred in the toolbar.
    pub(crate) fn traffic_lights(self) -> gpui::Point<Pixels> {
        point(px(18.), px((self.toolbar_height() - 14.) / 2.))
    }

    /// The window's own background: around the floating panels a shade
    /// apart from them, so they stand out as Finder's sidebar does; in the
    /// flat style the color of the bars, which touch it.
    pub(super) fn desk(self, colors: Colors, dark: bool) -> Hsla {
        if self.flat() {
            return colors.panel;
        }
        let base = colors.panel;
        if dark {
            // Darker than both the sidebar and the editors.
            let floor = colors.editor.l.min(base.l);
            hsla(base.h, base.s, (floor * 0.72).max(0.02), base.a)
        } else {
            hsla(base.h, base.s, (base.l - 0.045).max(0.), base.a)
        }
    }

    /// The soft shadow under a floating panel; none in the flat style.
    pub(super) fn panel_shadow(self, dark: bool) -> Vec<BoxShadow> {
        if self.flat() {
            return Vec::new();
        }
        let strength = if dark { 0.34 } else { 0.07 };
        vec![
            BoxShadow {
                color: hsla(0., 0., 0., strength),
                offset: point(px(0.), px(1.)),
                blur_radius: px(2.),
                spread_radius: px(0.),
                inset: false,
            },
            BoxShadow {
                color: hsla(0., 0., 0., strength * 0.6),
                offset: point(px(0.), px(6.)),
                blur_radius: px(18.),
                spread_radius: px(-6.),
                inset: false,
            },
        ]
    }

    /// How long a movement takes: none when animations are off.
    pub(crate) fn duration(self, millis: u64) -> Duration {
        if self.motion {
            Duration::from_millis(millis)
        } else {
            Duration::ZERO
        }
    }
}

/// Apple's curve for things that slide into place: quick, then settling.
pub(crate) fn settle(t: f32) -> f32 {
    gpui_base::animation::cubic_bezier(0.32, 0.72, 0., 1.)(t)
}

/// A number that moves to `target` over `duration` whenever the target
/// changes, sampled while drawing. It must be sampled on every frame, even
/// while what it moves is hidden, or it forgets where it was.
pub(crate) fn glide(
    id: impl Into<ElementId>,
    target: f32,
    duration: Duration,
    window: &mut Window,
    cx: &mut App,
) -> f32 {
    let policy = gpui_base::Transition::new(duration).ease(settle);
    gpui_base::transition(id.into(), target, policy, window, cx)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings(text: &str) -> Settings {
        let folder = tempfile::tempdir().unwrap();
        let file = folder.path().join("settings.json");
        std::fs::write(&file, text).unwrap();
        Settings::load(&file, None)
    }

    #[test]
    fn the_floating_panels_are_the_first_style() {
        assert_eq!(Look::from_settings(&settings("{}")), Look::default());
        let flat = Look::from_settings(&settings(
            r#"{ "keel.interfaceStyle": "flat", "keel.animations": false }"#,
        ));
        assert!(flat.flat() && !flat.motion);
        assert_eq!(flat.duration(200), Duration::ZERO);
        // A value Keel does not know keeps the floating panels.
        let odd = Look::from_settings(&settings(r#"{ "keel.interfaceStyle": "glass" }"#));
        assert_eq!(odd.style, InterfaceStyle::Floating);
    }

    #[test]
    fn the_flat_style_has_no_gaps_and_no_corners() {
        let floating = Look::default();
        let flat = Look {
            style: InterfaceStyle::Flat,
            motion: true,
        };
        assert_eq!((flat.gap(), flat.radius()), (0., 0.));
        assert!(floating.gap() > 0. && floating.radius() > 0.);
        assert!(flat.panel_shadow(true).is_empty());
        // The traffic lights stay centred in each toolbar.
        for look in [floating, flat] {
            let y = f32::from(look.traffic_lights().y);
            assert_eq!(y * 2. + 14., look.toolbar_height());
        }
    }

    #[test]
    fn the_curve_starts_and_ends_in_place() {
        assert!(settle(0.).abs() < 0.001);
        assert!((settle(1.) - 1.).abs() < 0.001);
        assert!(settle(0.5) > 0.5, "quick at first");
    }
}
