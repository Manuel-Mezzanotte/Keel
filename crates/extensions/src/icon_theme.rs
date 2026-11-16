//! VS Code file icon themes (`contributes.iconThemes`), such as Material
//! Icon Theme: which picture a file or folder gets, from its name, its
//! extension or its language, as VS Code picks it.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result};
use serde_json::Value;

use crate::InstalledExtension;

/// An icon theme an extension contributes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IconThemeContribution {
    /// Saved in `workbench.iconTheme`.
    pub id: String,
    pub label: String,
    pub path: PathBuf,
}

/// The icon themes of an extension.
pub fn icon_themes(extension: &InstalledExtension) -> Vec<IconThemeContribution> {
    let Some(themes) = extension.manifest.raw()["contributes"]["iconThemes"].as_array() else {
        return Vec::new();
    };
    themes
        .iter()
        .filter_map(|theme| {
            let id = theme["id"].as_str()?.to_owned();
            Some(IconThemeContribution {
                label: theme["label"].as_str().unwrap_or(&id).to_owned(),
                path: extension.root.join(theme["path"].as_str()?),
                id,
            })
        })
        .collect()
}

/// The associations of one variant of the theme (the main one, or the
/// overrides for light themes).
#[derive(Clone, Debug, Default)]
struct Associations {
    file: Option<String>,
    folder: Option<String>,
    folder_expanded: Option<String>,
    root_folder: Option<String>,
    root_folder_expanded: Option<String>,
    file_extensions: HashMap<String, String>,
    file_names: HashMap<String, String>,
    folder_names: HashMap<String, String>,
    folder_names_expanded: HashMap<String, String>,
    root_folder_names: HashMap<String, String>,
    root_folder_names_expanded: HashMap<String, String>,
    language_ids: HashMap<String, String>,
}

impl Associations {
    fn read(value: &Value) -> Self {
        let text = |key: &str| value[key].as_str().map(str::to_owned);
        // Names compare without case, as in VS Code.
        let map = |key: &str| -> HashMap<String, String> {
            value[key]
                .as_object()
                .map(|entries| {
                    entries
                        .iter()
                        .filter_map(|(name, icon)| {
                            Some((name.to_lowercase(), icon.as_str()?.to_owned()))
                        })
                        .collect()
                })
                .unwrap_or_default()
        };
        Self {
            file: text("file"),
            folder: text("folder"),
            folder_expanded: text("folderExpanded"),
            root_folder: text("rootFolder"),
            root_folder_expanded: text("rootFolderExpanded"),
            file_extensions: map("fileExtensions"),
            file_names: map("fileNames"),
            folder_names: map("folderNames"),
            folder_names_expanded: map("folderNamesExpanded"),
            root_folder_names: map("rootFolderNames"),
            root_folder_names_expanded: map("rootFolderNamesExpanded"),
            language_ids: map("languageIds"),
        }
    }
}

/// What an entry of the tree is, for picking its icon.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Entry<'a> {
    File {
        name: &'a str,
        /// The VS Code language identifier, such as `typescriptreact`.
        language: &'a str,
    },
    Folder {
        name: &'a str,
        expanded: bool,
        root: bool,
    },
}

/// A loaded icon theme.
#[derive(Clone, Debug, Default)]
pub struct IconTheme {
    pub id: String,
    /// Icon definition id to picture.
    icons: HashMap<String, PathBuf>,
    main: Associations,
    light: Associations,
    /// Whether the theme hides the folders' arrows.
    pub hides_explorer_arrows: bool,
}

impl IconTheme {
    pub fn load(contribution: &IconThemeContribution) -> Result<Self> {
        let text = std::fs::read_to_string(&contribution.path)
            .with_context(|| format!("cannot read {}", contribution.path.display()))?;
        let theme = settings::parse_jsonc(&text).with_context(|| {
            format!("{} is not a valid icon theme", contribution.path.display())
        })?;
        let folder = contribution.path.parent().unwrap_or(Path::new("."));
        let icons = theme["iconDefinitions"]
            .as_object()
            .map(|definitions| {
                definitions
                    .iter()
                    .filter_map(|(id, definition)| {
                        // Font glyphs (`fontCharacter`) are not drawn.
                        let path = definition["iconPath"].as_str()?;
                        Some((id.clone(), folder.join(path)))
                    })
                    .collect()
            })
            .unwrap_or_default();
        Ok(Self {
            id: contribution.id.clone(),
            icons,
            main: Associations::read(&theme),
            light: Associations::read(&theme["light"]),
            hides_explorer_arrows: theme["hidesExplorerArrows"].as_bool().unwrap_or(false),
        })
    }

    /// The picture for `entry`, if the theme has one. `light` uses the
    /// theme's overrides for light color themes.
    pub fn icon(&self, entry: Entry, light: bool) -> Option<&Path> {
        let variants: &[&Associations] = if light {
            &[&self.light, &self.main]
        } else {
            &[&self.main]
        };
        let id = match entry {
            Entry::File { name, language } => {
                let name = name.to_lowercase();
                // The whole name, then each extension from the longest:
                // "app.test.tsx" tries "test.tsx", then "tsx".
                let extensions: Vec<&str> = name
                    .char_indices()
                    .filter(|(_, c)| *c == '.')
                    .map(|(at, _)| &name[at + 1..])
                    .filter(|extension| !extension.is_empty())
                    .collect();
                variants
                    .iter()
                    .find_map(|variant| variant.file_names.get(&name))
                    .or_else(|| {
                        extensions.iter().find_map(|extension| {
                            variants
                                .iter()
                                .find_map(|variant| variant.file_extensions.get(*extension))
                        })
                    })
                    .or_else(|| {
                        variants
                            .iter()
                            .find_map(|variant| variant.language_ids.get(language))
                    })
                    .or_else(|| variants.iter().find_map(|variant| variant.file.as_ref()))
            }
            Entry::Folder {
                name,
                expanded,
                root,
            } => {
                let name = name.to_lowercase();
                fn named<'a>(
                    variant: &'a Associations,
                    name: &str,
                    root: bool,
                    expanded: bool,
                ) -> Option<&'a String> {
                    match (root, expanded) {
                        (true, true) => variant.root_folder_names_expanded.get(name),
                        (true, false) => variant.root_folder_names.get(name),
                        (false, true) => variant.folder_names_expanded.get(name),
                        (false, false) => variant.folder_names.get(name),
                    }
                }
                fn plain(variant: &Associations, root: bool, expanded: bool) -> Option<&String> {
                    match (root, expanded) {
                        (true, true) => variant
                            .root_folder_expanded
                            .as_ref()
                            .or(variant.folder_expanded.as_ref()),
                        (true, false) => variant.root_folder.as_ref().or(variant.folder.as_ref()),
                        (false, true) => variant.folder_expanded.as_ref(),
                        (false, false) => variant.folder.as_ref(),
                    }
                }
                variants
                    .iter()
                    .find_map(|variant| named(variant, &name, root, expanded))
                    .or_else(|| {
                        variants
                            .iter()
                            .find_map(|variant| plain(variant, root, expanded))
                    })
            }
        }?;
        self.icons.get(id).map(PathBuf::as_path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn theme() -> (tempfile::TempDir, IconTheme) {
        let folder = tempfile::tempdir().unwrap();
        let dist = folder.path().join("dist");
        std::fs::create_dir_all(&dist).unwrap();
        std::fs::write(
            dist.join("icons.json"),
            r#"{
                "iconDefinitions": {
                    "file": { "iconPath": "./../icons/file.svg" },
                    "folder": { "iconPath": "./../icons/folder.svg" },
                    "folder-open": { "iconPath": "./../icons/folder-open.svg" },
                    "folder-src": { "iconPath": "./../icons/folder-src.svg" },
                    "folder-src-open": { "iconPath": "./../icons/folder-src-open.svg" },
                    "folder-root": { "iconPath": "./../icons/folder-root.svg" },
                    "react_ts": { "iconPath": "./../icons/react_ts.svg" },
                    "test-ts": { "iconPath": "./../icons/test-ts.svg" },
                    "typescript": { "iconPath": "./../icons/typescript.svg" },
                    "npm": { "iconPath": "./../icons/npm.svg" },
                    "rust": { "iconPath": "./../icons/rust.svg" },
                    "rust_light": { "iconPath": "./../icons/rust_light.svg" },
                    "glyph": { "fontCharacter": "\\E001" }
                },
                "file": "file",
                "folder": "folder",
                "folderExpanded": "folder-open",
                "rootFolder": "folder-root",
                "fileExtensions": { "tsx": "react_ts", "test.ts": "test-ts", "ts": "typescript", "x": "glyph" },
                "fileNames": { "package.json": "npm" },
                "folderNames": { "src": "folder-src" },
                "folderNamesExpanded": { "src": "folder-src-open" },
                "languageIds": { "rust": "rust" },
                "light": { "languageIds": { "rust": "rust_light" } }
            }"#,
        )
        .unwrap();
        let theme = IconTheme::load(&IconThemeContribution {
            id: "material-icon-theme".into(),
            label: "Material Icon Theme".into(),
            path: dist.join("icons.json"),
        })
        .unwrap();
        (folder, theme)
    }

    fn file_icon(theme: &IconTheme, name: &str, language: &str, light: bool) -> Option<String> {
        theme
            .icon(Entry::File { name, language }, light)
            .map(|path| path.file_name().unwrap().to_string_lossy().into_owned())
    }

    fn folder_icon(theme: &IconTheme, name: &str, expanded: bool, root: bool) -> Option<String> {
        theme
            .icon(
                Entry::Folder {
                    name,
                    expanded,
                    root,
                },
                false,
            )
            .map(|path| path.file_name().unwrap().to_string_lossy().into_owned())
    }

    #[test]
    fn files_get_icons_by_name_extension_and_language() {
        let (_folder, theme) = theme();
        let icon = |name, language| file_icon(&theme, name, language, false);
        assert_eq!(icon("package.json", "json").as_deref(), Some("npm.svg"));
        assert_eq!(icon("Package.JSON", "json").as_deref(), Some("npm.svg"));
        assert_eq!(
            icon("booking-card.tsx", "typescriptreact").as_deref(),
            Some("react_ts.svg")
        );
        assert_eq!(
            icon("dates.test.ts", "typescript").as_deref(),
            Some("test-ts.svg")
        );
        assert_eq!(
            icon("dates.ts", "typescript").as_deref(),
            Some("typescript.svg")
        );
        assert_eq!(icon("main.rs", "rust").as_deref(), Some("rust.svg"));
        assert_eq!(
            file_icon(&theme, "main.rs", "rust", true).as_deref(),
            Some("rust_light.svg")
        );
        assert_eq!(
            icon("notes.weird", "plaintext").as_deref(),
            Some("file.svg")
        );
        // A font glyph has no picture.
        assert_eq!(icon("a.x", "plaintext"), None);
    }

    #[test]
    fn folders_get_icons_by_name_and_state() {
        let (_folder, theme) = theme();
        assert_eq!(
            folder_icon(&theme, "src", false, false).as_deref(),
            Some("folder-src.svg")
        );
        assert_eq!(
            folder_icon(&theme, "SRC", true, false).as_deref(),
            Some("folder-src-open.svg")
        );
        assert_eq!(
            folder_icon(&theme, "docs", false, false).as_deref(),
            Some("folder.svg")
        );
        assert_eq!(
            folder_icon(&theme, "docs", true, false).as_deref(),
            Some("folder-open.svg")
        );
        assert_eq!(
            folder_icon(&theme, "Keel", false, true).as_deref(),
            Some("folder-root.svg")
        );
        // No expanded root icon: the expanded folder.
        assert_eq!(
            folder_icon(&theme, "Keel", true, true).as_deref(),
            Some("folder-open.svg")
        );
    }

    /// See `real_color_themes`.
    #[test]
    #[ignore]
    fn real_icon_themes() {
        let folder = std::path::PathBuf::from(std::env::var("KEEL_REAL_EXTENSIONS").unwrap());
        for entry in std::fs::read_dir(folder).unwrap().flatten() {
            let Ok(extension) = InstalledExtension::load(entry.path().join("extension")) else {
                continue;
            };
            for contribution in icon_themes(&extension) {
                let started = std::time::Instant::now();
                let theme = IconTheme::load(&contribution).unwrap();
                println!("{}: loaded in {:?}", contribution.label, started.elapsed());
                for (name, language) in [
                    ("package.json", "json"),
                    ("booking-card.tsx", "typescriptreact"),
                    ("main.rs", "rust"),
                    ("Cargo.toml", "toml"),
                    ("README.md", "markdown"),
                    ("weird.zzz", "plaintext"),
                ] {
                    let icon = file_icon(&theme, name, language, false).unwrap();
                    println!("  {name} -> {icon}");
                    assert!(
                        theme
                            .icon(Entry::File { name, language }, false)
                            .unwrap()
                            .is_file()
                    );
                }
                for name in ["src", "node_modules", "docs", "whatever"] {
                    println!(
                        "  {name}/ -> {}",
                        folder_icon(&theme, name, false, false).unwrap()
                    );
                }
            }
        }
    }
}
