//! Keel's color themes: Mezzanotte (the default, with Sail's midnight blues),
//! Chiaro and Nero (with Sail's blacks).

use gpui::{App, SharedString, Window};
use gpui_component::{Theme, ThemeMode, ThemeRegistry};
use settings::Settings;

const THEMES: &str = include_str!("../themes/keel.json");

/// The themes Keel ships, in the order the picker shows them.
pub const NAMES: [&str; 3] = ["Mezzanotte", "Chiaro", "Nero"];
const DEFAULT_DARK: &str = "Mezzanotte";
const DEFAULT_LIGHT: &str = "Chiaro";

/// Registers Keel's themes. Call after `gpui_component::init`.
pub fn init(cx: &mut App) {
    if let Err(error) = ThemeRegistry::global_mut(cx).load_themes_from_str(THEMES) {
        eprintln!("keel: the built-in themes could not be read: {error}");
    }
}

/// Which theme to show, from the settings VS Code uses for this.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ThemeChoice {
    /// `workbench.colorTheme`.
    pub theme: String,
    /// `window.autoDetectColorScheme`: follow light and dark mode of macOS.
    pub follow_system: bool,
    /// `workbench.preferredDarkColorTheme`.
    pub dark: String,
    /// `workbench.preferredLightColorTheme`.
    pub light: String,
}

impl ThemeChoice {
    pub fn from_settings(settings: &Settings) -> Self {
        let text = |key: &str, default: &str| settings.string(key).unwrap_or(default).to_owned();
        Self {
            theme: text("workbench.colorTheme", DEFAULT_DARK),
            follow_system: settings
                .get("window.autoDetectColorScheme")
                .and_then(|value| value.as_bool())
                .unwrap_or(false),
            dark: text("workbench.preferredDarkColorTheme", DEFAULT_DARK),
            light: text("workbench.preferredLightColorTheme", DEFAULT_LIGHT),
        }
    }
}

/// Shows the chosen theme, or the one matching macOS when following it.
/// Unknown names fall back to Mezzanotte and Chiaro.
pub fn apply(choice: &ThemeChoice, window: Option<&mut Window>, cx: &mut App) {
    let registry = ThemeRegistry::global(cx);
    let find = |name: &str| {
        registry
            .themes()
            .get(&SharedString::from(name.to_owned()))
            .cloned()
    };
    let dark = find(&choice.dark).or_else(|| find(DEFAULT_DARK));
    let light = find(&choice.light).or_else(|| find(DEFAULT_LIGHT));
    let chosen = find(&choice.theme).or_else(|| find(DEFAULT_DARK));

    if choice.follow_system {
        let theme = Theme::global_mut(cx);
        if let Some(dark) = dark {
            theme.dark_theme = dark;
        }
        if let Some(light) = light {
            theme.light_theme = light;
        }
        Theme::sync_system_appearance(window, cx);
        return;
    }
    let Some(chosen) = chosen else {
        return;
    };
    let mode = chosen.mode;
    let theme = Theme::global_mut(cx);
    if mode.is_dark() {
        theme.dark_theme = chosen;
        Theme::change(ThemeMode::Dark, window, cx);
    } else {
        theme.light_theme = chosen;
        Theme::change(ThemeMode::Light, window, cx);
    }
}

/// Whether a theme is dark, for picking which preferred-theme setting to change.
pub fn is_dark(name: &str) -> bool {
    name != "Chiaro"
}

#[cfg(test)]
mod tests {
    use gpui::{Hsla, TestAppContext, rgb};
    use gpui_component::ActiveTheme as _;
    use gpui_component::theme::ThemeSet;

    use super::*;

    #[test]
    fn the_theme_file_has_the_three_themes() {
        let set: ThemeSet = serde_json::from_str(THEMES).unwrap();
        let names: Vec<_> = set
            .themes
            .iter()
            .map(|theme| (theme.name.to_string(), theme.mode.is_dark()))
            .collect();
        assert_eq!(
            names,
            [
                ("Mezzanotte".to_owned(), true),
                ("Chiaro".to_owned(), false),
                ("Nero".to_owned(), true)
            ]
        );
    }

    fn choice(theme: &str) -> ThemeChoice {
        ThemeChoice {
            theme: theme.to_owned(),
            follow_system: false,
            dark: DEFAULT_DARK.to_owned(),
            light: DEFAULT_LIGHT.to_owned(),
        }
    }

    #[gpui::test]
    fn applying_a_theme_changes_the_colors(cx: &mut TestAppContext) {
        cx.update(|cx| {
            gpui_component::init(cx);
            init(cx);

            apply(&choice("Mezzanotte"), None, cx);
            assert!(cx.theme().is_dark());
            assert_eq!(cx.theme().background, Hsla::from(rgb(0x070B14)));

            apply(&choice("Chiaro"), None, cx);
            assert!(!cx.theme().is_dark());
            assert_eq!(cx.theme().background, Hsla::from(rgb(0xFFFFFF)));

            apply(&choice("Nero"), None, cx);
            assert_eq!(cx.theme().background, Hsla::from(rgb(0x0A0A0A)));

            // An unknown name falls back to Mezzanotte.
            apply(&choice("Solarized"), None, cx);
            assert_eq!(cx.theme().background, Hsla::from(rgb(0x070B14)));
        });
    }

    #[test]
    fn the_choice_comes_from_the_settings() {
        let settings = Settings::load("/nonexistent/keel/settings.json", None);
        assert_eq!(ThemeChoice::from_settings(&settings), choice("Mezzanotte"));
    }
}
