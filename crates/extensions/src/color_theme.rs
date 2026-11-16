//! VS Code color themes (`contributes.themes`) turned into the themes of
//! gpui-component, the toolkit Keel draws with: the workbench colors map
//! onto its panels, lists, tabs and buttons, and the TextMate `tokenColors`
//! onto the tree-sitter highlight names of the editor.

use std::path::Path;

use anyhow::{Context as _, Result};
use serde_json::{Map, Value, json};

use crate::InstalledExtension;

/// A color theme an extension contributes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ColorThemeContribution {
    /// The name shown in the picker and saved in `workbench.colorTheme`.
    pub label: String,
    pub dark: bool,
    pub path: std::path::PathBuf,
}

/// The color themes of an extension, in the order it lists them.
pub fn color_themes(extension: &InstalledExtension) -> Vec<ColorThemeContribution> {
    let Some(themes) = extension.manifest.raw()["contributes"]["themes"].as_array() else {
        return Vec::new();
    };
    themes
        .iter()
        .filter_map(|theme| {
            let path = theme["path"].as_str()?;
            let label = theme["label"]
                .as_str()
                .or_else(|| theme["id"].as_str())?
                .to_owned();
            let ui = theme["uiTheme"].as_str().unwrap_or("vs-dark");
            Some(ColorThemeContribution {
                label,
                dark: ui == "vs-dark" || ui == "hc-black",
                path: extension.root.join(path),
            })
        })
        .collect()
}

/// Reads a theme file and what it includes, included first.
fn read_theme(path: &Path, depth: usize) -> Result<(Map<String, Value>, Vec<Value>)> {
    let text =
        std::fs::read_to_string(path).with_context(|| format!("cannot read {}", path.display()))?;
    let theme = settings::parse_jsonc(&text)
        .with_context(|| format!("{} is not a valid theme", path.display()))?;
    let (mut colors, mut tokens) = match theme["include"].as_str() {
        Some(include) if depth < 4 => {
            let base = path.parent().unwrap_or(Path::new(".")).join(include);
            read_theme(&base, depth + 1).unwrap_or_default()
        }
        _ => Default::default(),
    };
    if let Some(own) = theme["colors"].as_object() {
        colors.extend(own.clone());
    }
    if let Some(own) = theme["tokenColors"].as_array() {
        tokens.extend(own.iter().cloned());
    }
    Ok((colors, tokens))
}

/// The theme in gpui-component's format, ready for its theme registry.
pub fn convert(theme: &ColorThemeContribution) -> Result<Value> {
    let (colors, tokens) = read_theme(&theme.path, 0)?;
    Ok(convert_parts(&theme.label, theme.dark, &colors, &tokens))
}

/// `#RGB`, `#RGBA`, `#RRGGBB` or `#RRGGBBAA` as `#RRGGBB(AA)`.
fn normalize(color: &str) -> Option<String> {
    let hex = color.trim().strip_prefix('#')?;
    if !hex.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let full: String = match hex.len() {
        3 | 4 => hex.chars().flat_map(|c| [c, c]).collect(),
        6 | 8 => hex.to_owned(),
        _ => return None,
    };
    Some(format!("#{}", full.to_uppercase()))
}

/// The color with its alpha replaced.
fn with_alpha(color: &str, alpha: u8) -> String {
    format!("{}{alpha:02X}", &color[..7])
}

/// gpui-component's colors from VS Code's: the first key found wins.
const COLORS: &[(&str, &[&str])] = &[
    ("background", &["editor.background"]),
    ("foreground", &["foreground", "editor.foreground"]),
    (
        "border",
        &[
            "panel.border",
            "editorGroup.border",
            "sideBar.border",
            "contrastBorder",
        ],
    ),
    (
        "muted.background",
        &["input.background", "sideBarSectionHeader.background"],
    ),
    (
        "muted.foreground",
        &["descriptionForeground", "disabledForeground"],
    ),
    ("accent.background", &["list.hoverBackground"]),
    ("accent.foreground", &["list.hoverForeground"]),
    ("primary.background", &["button.background", "focusBorder"]),
    ("primary.foreground", &["button.foreground"]),
    (
        "primary.hover.background",
        &["button.hoverBackground", "button.background"],
    ),
    ("primary.active.background", &["button.background"]),
    (
        "secondary.background",
        &["button.secondaryBackground", "input.background"],
    ),
    ("secondary.foreground", &["button.secondaryForeground"]),
    (
        "secondary.hover.background",
        &["button.secondaryHoverBackground", "list.hoverBackground"],
    ),
    (
        "secondary.active.background",
        &["list.activeSelectionBackground"],
    ),
    (
        "popover.background",
        &[
            "editorWidget.background",
            "dropdown.background",
            "menu.background",
        ],
    ),
    (
        "popover.foreground",
        &[
            "editorWidget.foreground",
            "dropdown.foreground",
            "menu.foreground",
        ],
    ),
    (
        "list.background",
        &["sideBar.background", "editor.background"],
    ),
    (
        "list.active.background",
        &[
            "list.activeSelectionBackground",
            "list.inactiveSelectionBackground",
        ],
    ),
    ("list.active.border", &["list.focusOutline", "focusBorder"]),
    ("list.hover.background", &["list.hoverBackground"]),
    (
        "list.even.background",
        &["sideBar.background", "editor.background"],
    ),
    (
        "list.head.background",
        &["sideBarSectionHeader.background", "sideBar.background"],
    ),
    (
        "selection.background",
        &["editor.selectionBackground", "selection.background"],
    ),
    ("caret", &["editorCursor.foreground"]),
    ("ring", &["focusBorder"]),
    ("input.border", &["input.border", "dropdown.border"]),
    ("link", &["textLink.foreground"]),
    (
        "link.hover",
        &["textLink.activeForeground", "textLink.foreground"],
    ),
    (
        "link.active",
        &["textLink.activeForeground", "textLink.foreground"],
    ),
    (
        "sidebar.background",
        &["sideBar.background", "editor.background"],
    ),
    ("sidebar.border", &["sideBar.border", "panel.border"]),
    ("sidebar.foreground", &["sideBar.foreground", "foreground"]),
    ("sidebar.accent.background", &["list.hoverBackground"]),
    (
        "sidebar.accent.foreground",
        &["list.hoverForeground", "sideBar.foreground"],
    ),
    ("sidebar.primary.background", &["button.background"]),
    ("sidebar.primary.foreground", &["button.foreground"]),
    (
        "title_bar.background",
        &[
            "titleBar.activeBackground",
            "sideBar.background",
            "editor.background",
        ],
    ),
    ("title_bar.border", &["titleBar.border", "panel.border"]),
    ("status_bar.background", &["statusBar.background"]),
    ("status_bar.border", &["statusBar.border", "panel.border"]),
    (
        "tab_bar.background",
        &["editorGroupHeader.tabsBackground", "sideBar.background"],
    ),
    (
        "tab_bar.segmented.background",
        &["editorGroupHeader.tabsBackground"],
    ),
    (
        "tab.background",
        &["tab.inactiveBackground", "editorGroupHeader.tabsBackground"],
    ),
    (
        "tab.active.background",
        &["tab.activeBackground", "editor.background"],
    ),
    (
        "tab.active.foreground",
        &["tab.activeForeground", "foreground"],
    ),
    ("tab.foreground", &["tab.inactiveForeground"]),
    (
        "scrollbar.thumb.background",
        &["scrollbarSlider.background"],
    ),
    (
        "scrollbar.thumb.hover.background",
        &["scrollbarSlider.hoverBackground"],
    ),
    ("overlay", &["widget.shadow"]),
    (
        "window.border",
        &["window.activeBorder", "contrastBorder", "panel.border"],
    ),
    (
        "danger.foreground",
        &["errorForeground", "editorError.foreground"],
    ),
    (
        "warning.foreground",
        &["editorWarning.foreground", "list.warningForeground"],
    ),
    ("info.foreground", &["editorInfo.foreground", "focusBorder"]),
    (
        "success.foreground",
        &[
            "gitDecoration.addedResourceForeground",
            "terminal.ansiGreen",
        ],
    ),
    ("base.red", &["terminal.ansiRed"]),
    ("base.green", &["terminal.ansiGreen"]),
    ("base.blue", &["terminal.ansiBlue"]),
    ("base.yellow", &["terminal.ansiYellow"]),
    ("base.magenta", &["terminal.ansiMagenta"]),
    ("base.cyan", &["terminal.ansiCyan"]),
    ("base.red.light", &["terminal.ansiBrightRed"]),
    ("base.green.light", &["terminal.ansiBrightGreen"]),
    ("base.blue.light", &["terminal.ansiBrightBlue"]),
    ("base.yellow.light", &["terminal.ansiBrightYellow"]),
    ("base.magenta.light", &["terminal.ansiBrightMagenta"]),
    ("base.cyan.light", &["terminal.ansiBrightCyan"]),
];

/// The editor's colors around the text.
const EDITOR: &[(&str, &[&str])] = &[
    (
        "editor.active_line.background",
        &["editor.lineHighlightBackground"],
    ),
    ("editor.line_number", &["editorLineNumber.foreground"]),
    (
        "editor.active_line_number",
        &["editorLineNumber.activeForeground", "editor.foreground"],
    ),
    ("editor.invisible", &["editorWhitespace.foreground"]),
    (
        "created",
        &[
            "editorGutter.addedBackground",
            "gitDecoration.addedResourceForeground",
        ],
    ),
    (
        "deleted",
        &[
            "editorGutter.deletedBackground",
            "gitDecoration.deletedResourceForeground",
        ],
    ),
    (
        "modified",
        &[
            "editorGutter.modifiedBackground",
            "gitDecoration.modifiedResourceForeground",
        ],
    ),
    ("error", &["editorError.foreground", "errorForeground"]),
    ("warning", &["editorWarning.foreground"]),
    ("info", &["editorInfo.foreground"]),
    ("hint", &["editorHint.foreground", "editorInfo.foreground"]),
];

/// For each highlight name of the editor, the TextMate scopes that color
/// it, most fitting first.
const SYNTAX: &[(&str, &[&str])] = &[
    ("comment", &["comment"]),
    ("comment.doc", &["comment.block.documentation", "comment"]),
    (
        "keyword",
        &["keyword.control", "keyword", "storage.type", "storage"],
    ),
    ("operator", &["keyword.operator", "keyword"]),
    ("string", &["string.quoted", "string"]),
    ("string.escape", &["constant.character.escape", "string"]),
    ("string.regex", &["string.regexp", "string"]),
    ("string.special", &["string.other", "string"]),
    ("number", &["constant.numeric", "constant"]),
    (
        "boolean",
        &["constant.language.boolean", "constant.language", "constant"],
    ),
    (
        "constant",
        &["variable.other.constant", "constant.other", "constant"],
    ),
    (
        "type",
        &["entity.name.type", "support.type", "storage.type"],
    ),
    (
        "constructor",
        &[
            "entity.name.function.constructor",
            "entity.name.class",
            "entity.name.type",
        ],
    ),
    ("enum", &["entity.name.type.enum", "entity.name.type"]),
    (
        "variant",
        &["variable.other.enummember", "constant.other", "constant"],
    ),
    (
        "function",
        &[
            "entity.name.function",
            "support.function",
            "meta.function-call",
        ],
    ),
    ("tag", &["entity.name.tag"]),
    ("attribute", &["entity.other.attribute-name"]),
    (
        "property",
        &[
            "variable.other.property",
            "support.type.property-name",
            "meta.object-literal.key",
            "variable.other.object.property",
        ],
    ),
    (
        "variable",
        &["variable.other.readwrite", "variable.other", "variable"],
    ),
    ("variable.special", &["variable.language", "variable"]),
    ("punctuation", &["punctuation"]),
    (
        "punctuation.bracket",
        &["punctuation.definition.block", "meta.brace", "punctuation"],
    ),
    (
        "punctuation.delimiter",
        &[
            "punctuation.separator",
            "punctuation.terminator",
            "punctuation",
        ],
    ),
    (
        "punctuation.special",
        &[
            "punctuation.definition.template-expression",
            "punctuation.section.embedded",
            "punctuation",
        ],
    ),
    ("embedded", &["meta.embedded", "source"]),
    ("title", &["markup.heading", "entity.name.section"]),
    ("emphasis", &["markup.italic"]),
    ("emphasis.strong", &["markup.bold"]),
    ("link_text", &["string.other.link", "markup.underline.link"]),
    ("link_uri", &["markup.underline.link"]),
    (
        "text.literal",
        &["markup.inline.raw", "markup.raw", "markup.fenced_code"],
    ),
    ("label", &["entity.name.label"]),
];

/// A token rule's style: color, italic, bold.
#[derive(Clone, Debug, Default, PartialEq)]
struct TokenStyle {
    color: Option<String>,
    font_style: Option<String>,
}

/// The rules of `tokenColors`, as (selector, style), in file order.
fn token_rules(tokens: &[Value]) -> Vec<(String, TokenStyle)> {
    let mut rules = Vec::new();
    for token in tokens {
        let settings = &token["settings"];
        let style = TokenStyle {
            color: settings["foreground"].as_str().and_then(normalize),
            font_style: settings["fontStyle"].as_str().map(str::to_owned),
        };
        let selectors: Vec<String> = match &token["scope"] {
            Value::String(scope) => scope
                .split(',')
                .map(|part| part.trim().to_owned())
                .collect(),
            Value::Array(scopes) => scopes
                .iter()
                .filter_map(Value::as_str)
                .flat_map(|scope| scope.split(','))
                .map(|part| part.trim().to_owned())
                .collect(),
            _ => Vec::new(),
        };
        for selector in selectors {
            // Only plain scopes: "a.b", not "a b" or "a - b".
            if !selector.is_empty() && !selector.contains(' ') {
                rules.push((selector, style.clone()));
            }
        }
    }
    rules
}

/// The style for `scope`: the rule with the longest matching selector,
/// the later one among equals, as TextMate does.
fn style_for(rules: &[(String, TokenStyle)], scope: &str) -> Option<TokenStyle> {
    let mut best: Option<(usize, &TokenStyle)> = None;
    for (selector, style) in rules {
        let matches = scope == selector
            || (scope.starts_with(selector.as_str())
                && scope.as_bytes().get(selector.len()) == Some(&b'.'));
        if matches
            && style.color.is_some()
            && best.is_none_or(|(length, _)| selector.len() >= length)
        {
            best = Some((selector.len(), style));
        }
    }
    best.map(|(_, style)| style.clone())
}

fn convert_parts(label: &str, dark: bool, colors: &Map<String, Value>, tokens: &[Value]) -> Value {
    let get = |key: &str| colors.get(key).and_then(Value::as_str).and_then(normalize);
    let first = |keys: &[&str]| keys.iter().find_map(|key| get(key));
    let fallback_background = if dark { "#1E1E1E" } else { "#FFFFFF" };
    let fallback_foreground = if dark { "#D4D4D4" } else { "#333333" };
    let background = get("editor.background").unwrap_or_else(|| fallback_background.to_owned());
    let foreground = first(&["foreground", "editor.foreground"])
        .unwrap_or_else(|| fallback_foreground.to_owned());

    let mut out = Map::new();
    for (name, keys) in COLORS {
        if let Some(color) = first(keys) {
            out.insert((*name).to_owned(), json!(color));
        }
    }
    // What a theme leaves out comes from its own two colors, so nothing
    // falls back to another theme's palette.
    let derived = [
        ("background", background.clone()),
        ("foreground", foreground.clone()),
        ("border", with_alpha(&foreground, 0x1F)),
        ("muted.background", with_alpha(&foreground, 0x14)),
        ("muted.foreground", with_alpha(&foreground, 0x99)),
        ("accent.background", with_alpha(&foreground, 0x14)),
        ("accent.foreground", foreground.clone()),
        ("list.hover.background", with_alpha(&foreground, 0x14)),
        ("list.active.background", with_alpha(&foreground, 0x24)),
        ("secondary.foreground", foreground.clone()),
        ("popover.foreground", foreground.clone()),
        ("tab.foreground", with_alpha(&foreground, 0x99)),
        ("selection.background", with_alpha(&foreground, 0x33)),
        ("caret", foreground.clone()),
        ("scrollbar.background", with_alpha(&background, 0x00)),
        ("scrollbar.thumb.background", with_alpha(&foreground, 0x33)),
        (
            "scrollbar.thumb.hover.background",
            with_alpha(&foreground, 0x55),
        ),
        ("input.border", with_alpha(&foreground, 0x1F)),
    ];
    for (name, color) in derived {
        out.entry(name.to_owned()).or_insert(json!(color));
    }
    for name in [
        "popover.background",
        "list.background",
        "sidebar.background",
        "title_bar.background",
        "status_bar.background",
        "tab_bar.background",
        "tab.background",
        "tab.active.background",
        "list.even.background",
        "list.head.background",
    ] {
        out.entry(name.to_owned()).or_insert(json!(background));
    }
    // The tinted backgrounds of messages, from their colors.
    for kind in ["danger", "warning", "info", "success"] {
        if let Some(color) = out
            .get(&format!("{kind}.foreground"))
            .and_then(Value::as_str)
            .map(str::to_owned)
        {
            out.entry(format!("{kind}.background"))
                .or_insert(json!(with_alpha(&color, 0x26)));
        }
    }

    let mut highlight = Map::new();
    highlight.insert("editor.background".into(), json!(background));
    highlight.insert(
        "editor.foreground".into(),
        json!(get("editor.foreground").unwrap_or_else(|| foreground.clone())),
    );
    for (name, keys) in EDITOR {
        if let Some(color) = first(keys) {
            highlight.insert((*name).to_owned(), json!(color));
        }
    }
    for kind in ["created", "deleted", "modified"] {
        if let Some(color) = highlight
            .get(kind)
            .and_then(Value::as_str)
            .map(str::to_owned)
        {
            highlight.insert(
                format!("{kind}.background"),
                json!(with_alpha(&color, 0x26)),
            );
        }
    }

    let rules = token_rules(tokens);
    let mut syntax = Map::new();
    for (name, scopes) in SYNTAX {
        let Some(style) = scopes.iter().find_map(|scope| style_for(&rules, scope)) else {
            continue;
        };
        let mut entry = Map::new();
        if let Some(color) = style.color {
            entry.insert("color".into(), json!(color));
        }
        let font_style = style.font_style.unwrap_or_default();
        if font_style.contains("italic") {
            entry.insert("font_style".into(), json!("italic"));
        }
        if font_style.contains("bold") {
            entry.insert("font_weight".into(), json!(700));
        }
        syntax.insert((*name).to_owned(), Value::Object(entry));
    }
    highlight.insert("syntax".into(), Value::Object(syntax));

    json!({
        "name": label,
        "mode": if dark { "dark" } else { "light" },
        "colors": out,
        "highlight": highlight,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colors_are_normalized() {
        assert_eq!(normalize("#abc").as_deref(), Some("#AABBCC"));
        assert_eq!(normalize("#abcd").as_deref(), Some("#AABBCCDD"));
        assert_eq!(normalize("#282a36").as_deref(), Some("#282A36"));
        assert_eq!(normalize("#44475A75").as_deref(), Some("#44475A75"));
        assert_eq!(normalize("red"), None);
        assert_eq!(normalize("#12345"), None);
    }

    #[test]
    fn the_most_specific_token_rule_wins() {
        let tokens = vec![
            json!({ "scope": "keyword", "settings": { "foreground": "#FF79C6" } }),
            json!({ "scope": ["keyword.operator", "meta.tag punctuation"], "settings": { "foreground": "#aaa" } }),
            json!({ "scope": "comment, punctuation.definition.comment", "settings": { "foreground": "#6272A4", "fontStyle": "italic" } }),
            json!({ "name": "no scope", "settings": { "foreground": "#123456" } }),
        ];
        let rules = token_rules(&tokens);
        assert_eq!(rules.len(), 4, "the descendant selector is left out");
        assert_eq!(
            style_for(&rules, "keyword.control.flow")
                .unwrap()
                .color
                .as_deref(),
            Some("#FF79C6")
        );
        assert_eq!(
            style_for(&rules, "keyword.operator.arithmetic")
                .unwrap()
                .color
                .as_deref(),
            Some("#AAAAAA")
        );
        assert!(style_for(&rules, "keywordish").is_none());
        let comment = style_for(&rules, "comment.line").unwrap();
        assert_eq!(comment.font_style.as_deref(), Some("italic"));
    }

    #[test]
    fn a_vscode_theme_becomes_a_keel_theme() {
        let folder = tempfile::tempdir().unwrap();
        std::fs::write(
            folder.path().join("base.json"),
            r##"{ "colors": { "sideBar.background": "#21222C", "button.background": "#bd93f9" } }"##,
        )
        .unwrap();
        // Comments and an include, as VS Code's own themes have.
        std::fs::write(
            folder.path().join("theme.json"),
            r##"{
                // Dracula-like
                "include": "./base.json",
                "colors": {
                    "editor.background": "#282A36",
                    "editor.foreground": "#F8F8F2",
                    "editor.lineHighlightBackground": "#44475A75",
                    "statusBar.background": "#191A21",
                },
                "tokenColors": [
                    { "scope": "comment", "settings": { "foreground": "#6272A4", "fontStyle": "italic" } },
                    { "scope": "keyword", "settings": { "foreground": "#FF79C6" } },
                    { "scope": "entity.name.function", "settings": { "foreground": "#50FA7B", "fontStyle": "bold" } },
                    { "scope": "string", "settings": { "foreground": "#F1FA8C" } }
                ]
            }"##,
        )
        .unwrap();
        let contribution = ColorThemeContribution {
            label: "Dracula Theme".into(),
            dark: true,
            path: folder.path().join("theme.json"),
        };
        let theme = convert(&contribution).unwrap();
        assert_eq!(theme["name"], "Dracula Theme");
        assert_eq!(theme["mode"], "dark");
        assert_eq!(theme["colors"]["background"], "#282A36");
        assert_eq!(theme["colors"]["sidebar.background"], "#21222C");
        assert_eq!(theme["colors"]["primary.background"], "#BD93F9");
        assert_eq!(theme["colors"]["status_bar.background"], "#191A21");
        assert_eq!(theme["colors"]["border"], "#F8F8F21F");
        assert_eq!(
            theme["highlight"]["editor.active_line.background"],
            "#44475A75"
        );
        let syntax = &theme["highlight"]["syntax"];
        assert_eq!(
            syntax["comment"],
            json!({ "color": "#6272A4", "font_style": "italic" })
        );
        assert_eq!(syntax["keyword"]["color"], "#FF79C6");
        assert_eq!(syntax["operator"]["color"], "#FF79C6");
        assert_eq!(
            syntax["function"],
            json!({ "color": "#50FA7B", "font_weight": 700 })
        );
        assert_eq!(syntax["string.escape"]["color"], "#F1FA8C");
        assert!(syntax.get("tag").is_none());

        // gpui-component reads it.
        let set = json!({ "name": "Dracula", "themes": [theme] });
        let parsed: gpui_component::theme::ThemeSet = serde_json::from_value(set).unwrap();
        assert_eq!(parsed.themes.len(), 1);
    }

    /// Real themes from VSIX packages unpacked in `$KEEL_REAL_EXTENSIONS`:
    /// `KEEL_REAL_EXTENSIONS=... cargo test -p extensions -- --ignored real`.
    #[test]
    #[ignore]
    fn real_color_themes() {
        let folder = std::path::PathBuf::from(std::env::var("KEEL_REAL_EXTENSIONS").unwrap());
        for entry in std::fs::read_dir(folder).unwrap().flatten() {
            let Ok(extension) = InstalledExtension::load(entry.path().join("extension")) else {
                continue;
            };
            for theme in color_themes(&extension) {
                let started = std::time::Instant::now();
                let converted = convert(&theme).unwrap();
                let set = json!({ "name": "x", "themes": [converted.clone()] });
                let _: gpui_component::theme::ThemeSet = serde_json::from_value(set).unwrap();
                println!(
                    "{}: {} colors, {} syntax, {:?}, background {}, keyword {}",
                    theme.label,
                    converted["colors"].as_object().unwrap().len(),
                    converted["highlight"]["syntax"].as_object().unwrap().len(),
                    started.elapsed(),
                    converted["colors"]["background"],
                    converted["highlight"]["syntax"]["keyword"],
                );
            }
        }
    }
}
