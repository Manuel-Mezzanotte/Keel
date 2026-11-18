//! The settings an extension declares in `contributes.configuration`, as
//! VS Code's settings editor shows them: a title made from the key, a
//! plain-text description, and the kind of control that edits the value.

use serde_json::Value;

use crate::InstalledExtension;

/// One setting of an extension.
#[derive(Clone, Debug, PartialEq)]
pub struct Setting {
    /// The full key, such as `vsauto.defaultTypingMode`.
    pub key: String,
    /// What the settings editor calls it, such as "Default Typing Mode".
    pub title: String,
    pub description: String,
    pub kind: SettingKind,
    /// The value when nobody sets it.
    pub default: Value,
}

#[derive(Clone, Debug, PartialEq)]
pub enum SettingKind {
    Boolean,
    Number {
        integer: bool,
        minimum: Option<f64>,
        maximum: Option<f64>,
    },
    /// One of a list of strings.
    Choice(Vec<Choice>),
    Text,
    /// A list, an object or a mix of types: edited in `settings.json`.
    Other,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Choice {
    pub value: String,
    pub label: String,
    pub description: String,
}

/// A titled group of settings: `contributes.configuration` may be a list.
#[derive(Clone, Debug, PartialEq)]
pub struct SettingSection {
    pub title: Option<String>,
    pub settings: Vec<Setting>,
}

/// The extension's settings, in sections as it declares them. Settings
/// with an `order` come first, by order, and the others by key, as in
/// VS Code. Deprecated settings are left out, as VS Code hides them.
pub fn settings(extension: &InstalledExtension) -> Vec<SettingSection> {
    sections(extension.manifest.raw())
}

/// See [`settings`]; from a `package.json`.
pub fn sections(package: &Value) -> Vec<SettingSection> {
    let configuration = &package["contributes"]["configuration"];
    let mut declared: Vec<(usize, &Value)> = match configuration {
        Value::Array(sections) => sections.iter().enumerate().collect(),
        Value::Object(_) => vec![(0, configuration)],
        _ => Vec::new(),
    };
    declared.sort_by(|a, b| {
        let order = |section: &Value| section["order"].as_f64().unwrap_or(f64::MAX);
        order(a.1).total_cmp(&order(b.1)).then(a.0.cmp(&b.0))
    });
    let keys: Vec<&str> = declared
        .iter()
        .filter_map(|(_, section)| section["properties"].as_object())
        .flat_map(|properties| properties.keys().map(String::as_str))
        .collect();
    let namespace = common_namespace(&keys);
    let mut sections: Vec<SettingSection> = declared
        .into_iter()
        .filter_map(|(_, section)| {
            let properties = section["properties"].as_object()?;
            let mut settings: Vec<(f64, Setting)> = properties
                .iter()
                .filter(|(_, property)| !is_hidden(property))
                .map(|(key, property)| {
                    let order = property["order"].as_f64().unwrap_or(f64::MAX);
                    (order, setting(key, property, namespace.as_deref()))
                })
                .collect();
            settings.sort_by(|(a, first), (b, second)| {
                a.total_cmp(b).then_with(|| first.key.cmp(&second.key))
            });
            let settings: Vec<Setting> = settings.into_iter().map(|(_, setting)| setting).collect();
            (!settings.is_empty()).then(|| SettingSection {
                title: section["title"].as_str().map(str::to_owned),
                settings,
            })
        })
        .collect();
    // One section is the page itself: its title would only repeat it.
    if let [only] = sections.as_mut_slice() {
        only.title = None;
    }
    sections
}

fn is_hidden(property: &Value) -> bool {
    property.get("deprecationMessage").is_some()
        || property.get("markdownDeprecationMessage").is_some()
        || property["included"] == Value::Bool(false)
}

fn setting(key: &str, property: &Value, namespace: Option<&str>) -> Setting {
    let description = property["markdownDescription"]
        .as_str()
        .or_else(|| property["description"].as_str())
        .map(plain_text)
        .unwrap_or_default();
    Setting {
        key: key.to_owned(),
        title: title(key, namespace),
        description,
        kind: kind(property),
        default: property.get("default").cloned().unwrap_or(Value::Null),
    }
}

fn kind(property: &Value) -> SettingKind {
    // `"type": ["string", "null"]` is a string that may be left empty.
    let types: Vec<&str> = match &property["type"] {
        Value::String(kind) => vec![kind.as_str()],
        Value::Array(kinds) => kinds
            .iter()
            .filter_map(Value::as_str)
            .filter(|kind| *kind != "null")
            .collect(),
        _ => Vec::new(),
    };
    let strings: Option<Vec<&str>> = property["enum"]
        .as_array()
        .map(|values| values.iter().map(Value::as_str).collect::<Option<Vec<_>>>())
        .unwrap_or(None);
    let single = match types.as_slice() {
        [kind] => Some(*kind),
        [] => match property.get("default") {
            Some(Value::Bool(_)) => Some("boolean"),
            Some(Value::Number(_)) => Some("number"),
            Some(Value::String(_)) => Some("string"),
            _ => None,
        },
        _ => None,
    };
    match (single, strings) {
        (Some("string") | None, Some(values)) if !values.is_empty() => {
            SettingKind::Choice(choices(property, &values))
        }
        (Some("boolean"), _) => SettingKind::Boolean,
        (Some(kind @ ("number" | "integer")), _) => SettingKind::Number {
            integer: kind == "integer",
            minimum: property["minimum"].as_f64(),
            maximum: property["maximum"].as_f64(),
        },
        (Some("string"), _) => SettingKind::Text,
        _ => SettingKind::Other,
    }
}

fn choices(property: &Value, values: &[&str]) -> Vec<Choice> {
    let nth = |field: &str, index: usize| property[field][index].as_str().map(plain_text);
    values
        .iter()
        .enumerate()
        .map(|(index, value)| Choice {
            value: (*value).to_owned(),
            label: nth("enumItemLabels", index).unwrap_or_else(|| (*value).to_owned()),
            description: nth("markdownEnumDescriptions", index)
                .or_else(|| nth("enumDescriptions", index))
                .unwrap_or_default(),
        })
        .collect()
}

/// The first part of the keys when all of them share it, such as
/// `vsauto`: the settings editor leaves it out of the titles.
fn common_namespace(keys: &[&str]) -> Option<String> {
    let first = keys.first()?.split_once('.')?.0;
    keys.iter()
        .all(|key| {
            key.split_once('.')
                .is_some_and(|(namespace, _)| namespace == first)
        })
        .then(|| first.to_owned())
}

/// `vsauto.multiSelectionRewrite.enabled` → "Multi Selection Rewrite ›
/// Enabled", without the extension's own namespace.
pub fn title(key: &str, namespace: Option<&str>) -> String {
    let rest = namespace
        .and_then(|namespace| key.strip_prefix(namespace)?.strip_prefix('.'))
        .unwrap_or(key);
    rest.split('.')
        .filter(|part| !part.is_empty())
        .map(words)
        .collect::<Vec<_>>()
        .join(" › ")
}

/// `defaultTypingMode` → "Default Typing Mode", `enableURL` → "Enable URL".
fn words(part: &str) -> String {
    let characters: Vec<char> = part.chars().collect();
    let mut words: Vec<String> = Vec::new();
    let mut current = String::new();
    for (index, &character) in characters.iter().enumerate() {
        if character == '_' || character == '-' || character == ' ' {
            if !current.is_empty() {
                words.push(std::mem::take(&mut current));
            }
            continue;
        }
        let previous = index.checked_sub(1).map(|index| characters[index]);
        let next = characters.get(index + 1);
        let starts_word = character.is_uppercase()
            && previous.is_some_and(|previous| {
                previous.is_lowercase()
                    || previous.is_ascii_digit()
                    || (previous.is_uppercase() && next.is_some_and(|next| next.is_lowercase()))
            });
        if starts_word && !current.is_empty() {
            words.push(std::mem::take(&mut current));
        }
        current.push(character);
    }
    if !current.is_empty() {
        words.push(current);
    }
    words
        .into_iter()
        .map(|word| {
            let mut characters = word.chars();
            match characters.next() {
                Some(first) => first.to_uppercase().chain(characters).collect(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Markdown made plain: links keep their text, code and emphasis lose
/// their marks, and `#other.setting#` references show the setting's key.
pub fn plain_text(markdown: &str) -> String {
    let mut text = String::with_capacity(markdown.len());
    let mut rest = markdown;
    while let Some(start) = rest.find('[') {
        text.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        let link = after.find("](").and_then(|middle| {
            let close = after[middle + 2..].find(')')?;
            Some((middle, middle + 2 + close))
        });
        match link {
            Some((middle, end)) if !after[..middle].contains('[') => {
                text.push_str(&after[..middle]);
                rest = &after[end + 1..];
            }
            _ => {
                text.push('[');
                rest = after;
            }
        }
    }
    text.push_str(rest);
    let text = text.replace("**", "").replace('`', "");
    // `#editor.fontSize#` is a link to another setting.
    let is_key = |part: &str| {
        part.contains('.')
            && !part.starts_with('.')
            && !part.ends_with('.')
            && part
                .chars()
                .all(|character| character.is_alphanumeric() || ".-_".contains(character))
    };
    let mut plain = String::with_capacity(text.len());
    let mut rest = text.as_str();
    while let Some(start) = rest.find('#') {
        plain.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        match after.find('#') {
            Some(end) if is_key(&after[..end]) => {
                plain.push_str(&after[..end]);
                rest = &after[end + 1..];
            }
            _ => {
                plain.push('#');
                rest = after;
            }
        }
    }
    plain.push_str(rest);
    plain.trim().to_owned()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn titles_as_the_settings_editor_makes_them() {
        assert_eq!(
            title("vsauto.defaultTypingMode", Some("vsauto")),
            "Default Typing Mode"
        );
        assert_eq!(
            title("vsauto.multiSelectionRewrite.enabled", Some("vsauto")),
            "Multi Selection Rewrite › Enabled"
        );
        assert_eq!(title("editor.fontSize", None), "Editor › Font Size");
        assert_eq!(title("x.enableURLChecks", Some("x")), "Enable URL Checks");
        assert_eq!(title("x.tab_size2", Some("x")), "Tab Size2");
        assert_eq!(title("x.use-icons", Some("x")), "Use Icons");
    }

    #[test]
    fn markdown_becomes_plain_text() {
        assert_eq!(
            plain_text(
                "Uses `fast` mode. See [the docs](https://example.com) and #vsauto.suggestionStyle#."
            ),
            "Uses fast mode. See the docs and vsauto.suggestionStyle."
        );
        assert_eq!(
            plain_text("**Bold** and [not a link"),
            "Bold and [not a link"
        );
        assert_eq!(plain_text("Issue #12 is # fixed"), "Issue #12 is # fixed");
    }

    #[test]
    fn settings_with_their_controls() {
        let package = json!({
            "contributes": { "configuration": {
                "title": "Ghost",
                "properties": {
                    "ghost.speed": {
                        "type": "string", "default": "human",
                        "enum": ["human", "fast"],
                        "enumDescriptions": ["Like a person", "Quick"],
                        "description": "How fast it types."
                    },
                    "ghost.askOnStart": { "type": "boolean", "default": false,
                        "markdownDescription": "Asks `every` time." },
                    "ghost.delay": { "type": "integer", "default": 40, "minimum": 0, "maximum": 500 },
                    "ghost.ratio": { "type": "number", "default": 0.5 },
                    "ghost.prefix": { "type": ["string", "null"], "default": null },
                    "ghost.ignored": { "type": "array", "default": [] },
                    "ghost.first": { "type": "boolean", "default": true, "order": 1 },
                    "ghost.old": { "type": "boolean", "deprecationMessage": "Gone." }
                }
            } }
        });
        let sections = sections(&package);
        assert_eq!(sections.len(), 1);
        assert_eq!(sections[0].title, None, "one section is the page itself");
        let settings = &sections[0].settings;
        let keys: Vec<&str> = settings
            .iter()
            .map(|setting| setting.key.as_str())
            .collect();
        assert_eq!(
            keys,
            [
                "ghost.first",
                "ghost.askOnStart",
                "ghost.delay",
                "ghost.ignored",
                "ghost.prefix",
                "ghost.ratio",
                "ghost.speed"
            ]
        );
        let find = |key: &str| settings.iter().find(|setting| setting.key == key).unwrap();
        assert_eq!(find("ghost.askOnStart").kind, SettingKind::Boolean);
        assert_eq!(find("ghost.askOnStart").description, "Asks every time.");
        assert_eq!(find("ghost.askOnStart").title, "Ask On Start");
        assert_eq!(
            find("ghost.delay").kind,
            SettingKind::Number {
                integer: true,
                minimum: Some(0.),
                maximum: Some(500.)
            }
        );
        assert!(matches!(
            find("ghost.ratio").kind,
            SettingKind::Number { integer: false, .. }
        ));
        assert_eq!(find("ghost.prefix").kind, SettingKind::Text);
        assert_eq!(find("ghost.ignored").kind, SettingKind::Other);
        let SettingKind::Choice(choices) = &find("ghost.speed").kind else {
            panic!("a choice");
        };
        assert_eq!(choices[1].value, "fast");
        assert_eq!(choices[1].label, "fast");
        assert_eq!(choices[0].description, "Like a person");
        assert_eq!(find("ghost.speed").default, json!("human"));
    }

    #[test]
    fn several_sections_keep_their_titles_and_order() {
        let package = json!({
            "contributes": { "configuration": [
                { "title": "Later", "order": 2, "properties": { "a.later": { "type": "string" } } },
                { "title": "Empty", "properties": {} },
                { "title": "First", "order": 1, "properties": { "a.first": { "type": "boolean" } } }
            ] }
        });
        let sections = sections(&package);
        let titles: Vec<Option<&str>> = sections
            .iter()
            .map(|section| section.title.as_deref())
            .collect();
        assert_eq!(titles, [Some("First"), Some("Later")]);
        assert_eq!(sections[0].settings[0].title, "First");
        assert!(super::sections(&json!({ "name": "none" })).is_empty());
    }
}
