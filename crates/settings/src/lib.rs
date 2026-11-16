//! Keel's settings, read from `settings.json` files in the format VS Code
//! uses: flat dotted keys, comments and trailing commas allowed.
//!
//! From lowest to highest priority: Keel's defaults, the user's file, the
//! project's `.vscode/settings.json`, the project's `.keel/settings.json`.

pub mod jsonc;

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result, bail};
use serde_json::{Map, Value, json};

pub use jsonc::{has_comments, parse as parse_jsonc};

/// Where a layer of settings comes from, from lowest to highest priority.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    Default,
    User,
    VsCodeProject,
    Project,
}

pub struct Settings {
    user_file: PathBuf,
    /// The project folder, whose `.vscode/settings.json` extensions write.
    project_root: Option<PathBuf>,
    layers: Vec<(Source, Map<String, Value>)>,
    /// Problems found while reading the files, for the user to see.
    pub warnings: Vec<String>,
}

/// Keel's own defaults.
pub fn defaults() -> Map<String, Value> {
    let defaults = json!({
        "workbench.colorTheme": "Mezzanotte",
        "keel.language": "auto",
        "editor.fontSize": 14,
        "editor.tabSize": 2,
        "editor.wordWrap": "off",
    });
    match defaults {
        Value::Object(map) => map,
        _ => Map::new(),
    }
}

impl Settings {
    /// Reads the user's settings and, when given, the project's.
    pub fn load(user_file: impl Into<PathBuf>, project_root: Option<&Path>) -> Self {
        let user_file = user_file.into();
        let mut warnings = Vec::new();
        let mut layers = vec![(Source::Default, defaults())];
        layers.push((Source::User, read(&user_file, &mut warnings)));
        if let Some(root) = project_root {
            let vscode = root.join(".vscode/settings.json");
            layers.push((Source::VsCodeProject, read(&vscode, &mut warnings)));
            let keel = root.join(".keel/settings.json");
            layers.push((Source::Project, read(&keel, &mut warnings)));
        }
        Self {
            user_file,
            project_root: project_root.map(Path::to_path_buf),
            layers,
            warnings,
        }
    }

    /// The value of a setting, from the layer with the highest priority.
    pub fn get(&self, key: &str) -> Option<&Value> {
        self.layers
            .iter()
            .rev()
            .find_map(|(_, values)| values.get(key))
    }

    pub fn string(&self, key: &str) -> Option<&str> {
        self.get(key).and_then(Value::as_str)
    }

    pub fn number(&self, key: &str) -> Option<f64> {
        self.get(key).and_then(Value::as_f64)
    }

    /// Which layer a setting's value comes from.
    pub fn source_of(&self, key: &str) -> Option<Source> {
        self.layers
            .iter()
            .rev()
            .find(|(_, values)| values.contains_key(key))
            .map(|(source, _)| *source)
    }

    /// Every setting with its winning value.
    pub fn merged(&self) -> Map<String, Value> {
        let mut merged = Map::new();
        for (_, values) in &self.layers {
            for (key, value) in values {
                merged.insert(key.clone(), value.clone());
            }
        }
        merged
    }

    /// Sets or removes (`None`) a value in the user's file. A file with
    /// comments is left alone, since rewriting it would lose them.
    pub fn update_user(&mut self, key: &str, value: Option<Value>) -> Result<()> {
        let file = self.user_file.clone();
        self.update_file(Source::User, &file, key, value)
    }

    /// Sets or removes a value in the project's `.vscode/settings.json`, as
    /// VS Code does for workspace settings, so Cursor sees it too.
    pub fn update_project(&mut self, key: &str, value: Option<Value>) -> Result<()> {
        let Some(root) = self.project_root.clone() else {
            bail!("no project is open, so {key} cannot be saved for it");
        };
        self.update_file(
            Source::VsCodeProject,
            &root.join(".vscode/settings.json"),
            key,
            value,
        )
    }

    /// Whether a project is open, with settings of its own.
    pub fn has_project(&self) -> bool {
        self.project_root.is_some()
    }

    /// The values of one layer, such as the user's file.
    pub fn layer(&self, source: Source) -> Map<String, Value> {
        self.layers
            .iter()
            .filter(|(layer, _)| *layer == source)
            .flat_map(|(_, values)| values.clone())
            .collect()
    }

    fn update_file(
        &mut self,
        source: Source,
        file: &Path,
        key: &str,
        value: Option<Value>,
    ) -> Result<()> {
        let existing = match fs::read_to_string(file) {
            Ok(text) => {
                if has_comments(&text) {
                    bail!(
                        "{} has comments; change {key} there by hand",
                        file.display()
                    );
                }
                text
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => "{}".to_owned(),
            Err(error) => return Err(error.into()),
        };
        let mut values = match parse_jsonc(&existing)
            .with_context(|| format!("{} is not valid JSON", file.display()))?
        {
            Value::Object(values) => values,
            _ => bail!("{} must contain an object", file.display()),
        };
        match value {
            Some(value) => values.insert(key.to_owned(), value),
            None => values.remove(key),
        };
        if let Some(parent) = file.parent() {
            fs::create_dir_all(parent)?;
        }
        let text = serde_json::to_string_pretty(&Value::Object(values.clone()))? + "\n";
        let temporary = file.with_extension("json.keel-save");
        fs::write(&temporary, text)?;
        fs::rename(&temporary, file)?;
        match self.layers.iter_mut().find(|(layer, _)| *layer == source) {
            Some((_, layer)) => *layer = values,
            None => self.layers.push((source, values)),
        }
        Ok(())
    }
}

fn read(path: &Path, warnings: &mut Vec<String>) -> Map<String, Value> {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Map::new(),
        Err(error) => {
            warnings.push(format!("{}: {error}", path.display()));
            return Map::new();
        }
    };
    if text.trim().is_empty() {
        return Map::new();
    }
    match parse_jsonc(&text) {
        Ok(Value::Object(values)) => values,
        Ok(_) => {
            warnings.push(format!(
                "{}: settings must be a JSON object",
                path.display()
            ));
            Map::new()
        }
        Err(error) => {
            warnings.push(format!("{}: {error}", path.display()));
            Map::new()
        }
    }
}

/// The project a file belongs to: the nearest folder above it with `.git`,
/// `.vscode` or `.keel`; otherwise the file's own folder.
pub fn project_root_for(file: &Path) -> Option<PathBuf> {
    let start = file.parent()?;
    let mut folder = Some(start);
    while let Some(current) = folder {
        if [".git", ".vscode", ".keel"]
            .iter()
            .any(|marker| current.join(marker).exists())
        {
            return Some(current.to_path_buf());
        }
        folder = current.parent();
    }
    Some(start.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(path: &Path, text: &str) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }

    #[test]
    fn later_layers_win() {
        let folder = tempfile::tempdir().unwrap();
        let user = folder.path().join("user/settings.json");
        let project = folder.path().join("project");
        write(
            &user,
            r#"{ "editor.fontSize": 15, "vsauto.defaultTypingMode": "fast", }"#,
        );
        write(
            &project.join(".vscode/settings.json"),
            r#"{ "editor.tabSize": 4 } // VS Code"#,
        );
        write(
            &project.join(".keel/settings.json"),
            r#"{ "editor.fontSize": 16 }"#,
        );

        let settings = Settings::load(&user, Some(&project));

        assert_eq!(settings.number("editor.fontSize"), Some(16.0));
        assert_eq!(settings.source_of("editor.fontSize"), Some(Source::Project));
        assert_eq!(settings.number("editor.tabSize"), Some(4.0));
        assert_eq!(settings.string("vsauto.defaultTypingMode"), Some("fast"));
        assert_eq!(settings.string("workbench.colorTheme"), Some("Mezzanotte"));
        assert_eq!(
            settings.source_of("workbench.colorTheme"),
            Some(Source::Default)
        );
        assert!(settings.warnings.is_empty());
        assert_eq!(settings.merged()["editor.fontSize"], 16);
    }

    #[test]
    fn missing_files_are_fine_and_broken_ones_are_reported() {
        let folder = tempfile::tempdir().unwrap();
        let user = folder.path().join("settings.json");
        let settings = Settings::load(&user, None);
        assert!(settings.warnings.is_empty());
        assert_eq!(settings.number("editor.fontSize"), Some(14.0));

        write(&user, "{ \"editor.fontSize\": }");
        let settings = Settings::load(&user, None);
        assert_eq!(settings.warnings.len(), 1);
        assert_eq!(settings.number("editor.fontSize"), Some(14.0));
    }

    #[test]
    fn updates_go_to_the_user_file() {
        let folder = tempfile::tempdir().unwrap();
        let user = folder.path().join("keel/settings.json");
        let mut settings = Settings::load(&user, None);

        settings
            .update_user("vsauto.askOnStart", Some(json!(true)))
            .unwrap();
        assert_eq!(settings.get("vsauto.askOnStart"), Some(&json!(true)));
        let saved = parse_jsonc(&fs::read_to_string(&user).unwrap()).unwrap();
        assert_eq!(saved, json!({ "vsauto.askOnStart": true }));

        settings.update_user("vsauto.askOnStart", None).unwrap();
        assert_eq!(settings.get("vsauto.askOnStart"), None);
    }

    #[test]
    fn updates_can_go_to_the_project_file() {
        let folder = tempfile::tempdir().unwrap();
        let user = folder.path().join("user/settings.json");
        let project = folder.path().join("project");
        fs::create_dir_all(&project).unwrap();
        let mut settings = Settings::load(&user, Some(&project));
        settings
            .update_project("peacock.color", Some(json!("#42b883")))
            .unwrap();
        assert_eq!(settings.string("peacock.color"), Some("#42b883"));
        assert_eq!(
            settings.source_of("peacock.color"),
            Some(Source::VsCodeProject)
        );
        assert_eq!(
            settings.layer(Source::VsCodeProject)["peacock.color"],
            json!("#42b883")
        );
        assert!(settings.layer(Source::User).is_empty());
        let saved = fs::read_to_string(project.join(".vscode/settings.json")).unwrap();
        assert!(saved.contains("#42b883"), "{saved}");
        settings.update_project("peacock.color", None).unwrap();
        assert_eq!(settings.get("peacock.color"), None);

        let mut alone = Settings::load(&user, None);
        assert!(
            alone
                .update_project("peacock.color", Some(json!("#000000")))
                .is_err()
        );
    }

    #[test]
    fn files_with_comments_are_not_rewritten() {
        let folder = tempfile::tempdir().unwrap();
        let user = folder.path().join("settings.json");
        write(&user, "{\n  // mio\n  \"editor.fontSize\": 15\n}\n");
        let mut settings = Settings::load(&user, None);
        let error = settings
            .update_user("editor.fontSize", Some(json!(16)))
            .unwrap_err();
        assert!(error.to_string().contains("has comments"), "{error}");
        assert!(fs::read_to_string(&user).unwrap().contains("// mio"));
    }

    #[test]
    fn the_project_is_the_folder_with_git() {
        let folder = tempfile::tempdir().unwrap();
        let root = folder.path().join("GestionaleHospitality");
        fs::create_dir_all(root.join(".git")).unwrap();
        fs::create_dir_all(root.join("src/components")).unwrap();
        let file = root.join("src/components/booking-card.tsx");
        assert_eq!(project_root_for(&file), Some(root));
    }
}
