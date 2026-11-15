//! The `package.json` of a VS Code extension.

use anyhow::{Context as _, Result};
use serde::Deserialize;
use serde_json::{Map, Value};

/// The parts of `package.json` Keel reads, plus the raw JSON, which extensions
/// can see as `extension.packageJSON`.
#[derive(Clone, Debug)]
pub struct Manifest {
    pub name: String,
    pub publisher: String,
    pub version: String,
    pub display_name: Option<String>,
    /// Entry point for desktop extensions, relative to the extension root.
    pub main: Option<String>,
    pub activation_events: Vec<String>,
    pub commands: Vec<CommandContribution>,
    pub keybindings: Vec<KeybindingContribution>,
    raw: Value,
}

/// A command listed under `contributes.commands`.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub struct CommandContribution {
    pub command: String,
    #[serde(default)]
    pub title: String,
}

/// A key binding listed under `contributes.keybindings`.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub struct KeybindingContribution {
    pub command: String,
    #[serde(default)]
    pub key: String,
    #[serde(default)]
    pub mac: Option<String>,
    #[serde(default)]
    pub when: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawManifest {
    name: String,
    publisher: String,
    version: String,
    #[serde(default)]
    display_name: Option<String>,
    #[serde(default)]
    main: Option<String>,
    #[serde(default)]
    activation_events: Vec<String>,
    #[serde(default)]
    contributes: RawContributes,
}

#[derive(Default, Deserialize)]
struct RawContributes {
    #[serde(default)]
    commands: Vec<CommandContribution>,
    #[serde(default)]
    keybindings: Vec<KeybindingContribution>,
}

impl Manifest {
    /// Parses the contents of a `package.json` file.
    pub fn from_json(bytes: &[u8]) -> Result<Self> {
        let raw: Value = serde_json::from_slice(bytes).context("package.json is not valid JSON")?;
        let parsed: RawManifest = serde_json::from_value(raw.clone())
            .context("package.json is missing required fields")?;
        Ok(Self {
            name: parsed.name,
            publisher: parsed.publisher,
            version: parsed.version,
            display_name: parsed.display_name,
            main: parsed.main,
            activation_events: parsed.activation_events,
            commands: parsed.contributes.commands,
            keybindings: parsed.contributes.keybindings,
            raw,
        })
    }

    /// The extension identifier, `publisher.name`, as VS Code writes it.
    pub fn id(&self) -> String {
        format!("{}.{}", self.publisher, self.name)
    }

    /// The whole `package.json`.
    pub fn raw(&self) -> &Value {
        &self.raw
    }

    /// Default values of the settings the extension declares, keyed by their
    /// full name, such as `vsauto.defaultTypingMode`.
    ///
    /// `contributes.configuration` can be a single object or a list of them.
    pub fn configuration_defaults(&self) -> Map<String, Value> {
        let mut defaults = Map::new();
        let configuration = &self.raw["contributes"]["configuration"];
        let sections: Vec<&Value> = match configuration {
            Value::Array(sections) => sections.iter().collect(),
            Value::Object(_) => vec![configuration],
            _ => Vec::new(),
        };
        for section in sections {
            if let Some(properties) = section["properties"].as_object() {
                for (key, property) in properties {
                    if let Some(default) = property.get("default") {
                        defaults.insert(key.clone(), default.clone());
                    }
                }
            }
        }
        defaults
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PACKAGE_JSON: &str = r#"{
        "name": "hello",
        "publisher": "keel-tests",
        "version": "1.2.3",
        "main": "./out/extension.js",
        "activationEvents": ["onCommand:hello.say"],
        "contributes": {
            "commands": [{ "command": "hello.say", "title": "Say hello" }],
            "keybindings": [{ "command": "hello.say", "key": "ctrl+h", "mac": "cmd+h", "when": "editorTextFocus" }],
            "configuration": {
                "properties": {
                    "hello.name": { "type": "string", "default": "Keel" },
                    "hello.loud": { "type": "boolean" }
                }
            }
        }
    }"#;

    #[test]
    fn reads_the_fields_keel_needs() {
        let manifest = Manifest::from_json(PACKAGE_JSON.as_bytes()).unwrap();
        assert_eq!(manifest.id(), "keel-tests.hello");
        assert_eq!(manifest.version, "1.2.3");
        assert_eq!(manifest.main.as_deref(), Some("./out/extension.js"));
        assert_eq!(manifest.activation_events, ["onCommand:hello.say"]);
        assert_eq!(manifest.commands[0].command, "hello.say");
        assert_eq!(manifest.keybindings[0].mac.as_deref(), Some("cmd+h"));
        assert_eq!(manifest.raw()["name"], "hello");
    }

    #[test]
    fn collects_setting_defaults_from_one_or_many_sections() {
        let manifest = Manifest::from_json(PACKAGE_JSON.as_bytes()).unwrap();
        let defaults = manifest.configuration_defaults();
        assert_eq!(defaults["hello.name"], "Keel");
        assert!(!defaults.contains_key("hello.loud"));

        let list = r#"{ "name": "a", "publisher": "b", "version": "0.0.1",
            "contributes": { "configuration": [
                { "properties": { "a.one": { "default": 1 } } },
                { "properties": { "a.two": { "default": [2] } } }
            ] } }"#;
        let defaults = Manifest::from_json(list.as_bytes())
            .unwrap()
            .configuration_defaults();
        assert_eq!(defaults["a.one"], 1);
        assert_eq!(defaults["a.two"], serde_json::json!([2]));
    }

    #[test]
    fn rejects_broken_manifests() {
        assert!(Manifest::from_json(b"not json").is_err());
        assert!(Manifest::from_json(br#"{ "name": "only-a-name" }"#).is_err());
    }
}
