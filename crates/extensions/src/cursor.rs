//! Bringing the extensions of Cursor (or VS Code) into Keel: which ones
//! Cursor has, which Keel already does on its own, which cannot work in
//! Keel and why, and copying a locally built one across.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result, bail};
use serde_json::Value;

use crate::InstalledExtension;
use crate::marketplace::{Listing, Marketplace};

/// An extension installed in Cursor.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CursorExtension {
    /// `publisher.name`, as Cursor writes it (lower case).
    pub id: String,
    pub version: String,
    pub folder: PathBuf,
    /// Installed from a `.vsix` file rather than a gallery: not on any
    /// registry, so it is copied.
    pub local: bool,
}

/// Cursor's extensions folder: `~/.cursor/extensions`.
pub fn cursor_extensions_dir() -> Option<PathBuf> {
    std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".cursor/extensions"))
}

/// The extensions listed in `extensions.json` of `folder`, Cursor's own
/// record of what is installed.
pub fn cursor_extensions(folder: &Path) -> Result<Vec<CursorExtension>> {
    let file = folder.join("extensions.json");
    let text = std::fs::read_to_string(&file)
        .with_context(|| format!("cannot read {}", file.display()))?;
    let entries: Value = serde_json::from_str(&text)
        .with_context(|| format!("{} is not valid JSON", file.display()))?;
    let mut found: Vec<CursorExtension> = entries
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|entry| {
            let id = entry["identifier"]["id"].as_str()?.to_owned();
            let location = entry["relativeLocation"].as_str()?;
            Some(CursorExtension {
                version: entry["version"].as_str().unwrap_or_default().to_owned(),
                folder: folder.join(location),
                local: entry["metadata"]["source"].as_str() == Some("vsix"),
                id,
            })
        })
        .filter(|extension| extension.folder.join("package.json").is_file())
        .collect();
    found.sort_by(|a, b| a.id.cmp(&b.id));
    found.dedup_by(|a, b| a.id == b.id);
    Ok(found)
}

/// What Keel does by itself, in place of an extension.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BuiltIn {
    Prettier,
    GitGraph,
    ClaudeCode,
    Pdf,
    MarkdownPreview,
    Yaml,
    TypeScript,
}

/// Extensions Keel replaces with its own features.
pub fn built_in(id: &str) -> Option<BuiltIn> {
    match id.to_lowercase().as_str() {
        "esbenp.prettier-vscode" => Some(BuiltIn::Prettier),
        "mhutchie.git-graph" | "eamodio.gitlens" => Some(BuiltIn::GitGraph),
        "anthropic.claude-code" => Some(BuiltIn::ClaudeCode),
        "tomoki1207.pdf" => Some(BuiltIn::Pdf),
        "shd101wyy.markdown-preview-enhanced" | "yzhang.markdown-all-in-one" => {
            Some(BuiltIn::MarkdownPreview)
        }
        "redhat.vscode-yaml" => Some(BuiltIn::Yaml),
        "ms-vscode.vscode-typescript-next" => Some(BuiltIn::TypeScript),
        _ => None,
    }
}

/// Something an extension needs that Keel does not offer.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Blocker {
    /// Made for Cursor itself, such as Remote SSH.
    CursorOnly,
    Debugger,
    Notebooks,
    CustomEditors,
    /// Its own panels in the sidebar.
    Views,
    /// Pages drawn with HTML (webviews).
    Webviews,
    /// Starts a language server program.
    LanguageServer,
}

impl Blocker {
    /// Whether the extension is useless without it; otherwise only a part
    /// of it is missing.
    pub fn blocks(self) -> bool {
        !matches!(self, Blocker::Webviews | Blocker::CustomEditors)
    }
}

/// What Keel would do with one of Cursor's extensions.
#[derive(Clone, Debug, PartialEq)]
pub enum Verdict {
    /// Already installed in Keel.
    Installed,
    /// Keel does it by itself.
    BuiltIn(BuiltIn),
    /// Cannot work in Keel.
    Blocked(Vec<Blocker>),
    /// Needs another extension that cannot work in Keel.
    NeedsBlocked(String),
    /// Built locally: copied from Cursor's folder. `partial` lists what
    /// will not work in it.
    Local { partial: Vec<Blocker> },
    /// On Open VSX.
    Available {
        listing: Box<Listing>,
        partial: Vec<Blocker>,
    },
    /// Neither on Open VSX nor built locally.
    NotOnOpenVsx,
}

/// One of Cursor's extensions and what Keel would do with it.
#[derive(Clone, Debug, PartialEq)]
pub struct CursorEntry {
    pub extension: CursorExtension,
    pub name: String,
    pub description: String,
    pub verdict: Verdict,
}

/// Looks at every extension of Cursor: `installed` holds the lower-case
/// ids Keel has; `market` is asked for the rest, a few at a time.
pub fn analyze(
    found: &[CursorExtension],
    installed: &HashSet<String>,
    market: Option<&Marketplace>,
) -> Vec<CursorEntry> {
    let loaded: Vec<Option<InstalledExtension>> = found
        .iter()
        .map(|extension| InstalledExtension::load(&extension.folder).ok())
        .collect();
    let mut verdicts: Vec<Option<Verdict>> = found
        .iter()
        .zip(&loaded)
        .map(|(extension, manifest)| {
            if installed.contains(&extension.id.to_lowercase()) {
                return Some(Verdict::Installed);
            }
            if let Some(feature) = built_in(&extension.id) {
                return Some(Verdict::BuiltIn(feature));
            }
            let blocking: Vec<Blocker> = manifest
                .as_ref()
                .map(blockers)
                .unwrap_or_default()
                .into_iter()
                .filter(|blocker| blocker.blocks())
                .collect();
            (!blocking.is_empty()).then_some(Verdict::Blocked(blocking))
        })
        .collect();
    // Needing a blocked extension blocks too; repeated for chains.
    let blocked = |verdicts: &[Option<Verdict>]| -> HashSet<String> {
        found
            .iter()
            .zip(verdicts)
            .filter(|(_, verdict)| {
                matches!(
                    verdict,
                    Some(Verdict::Blocked(_) | Verdict::NeedsBlocked(_))
                )
            })
            .map(|(extension, _)| extension.id.to_lowercase())
            .collect()
    };
    loop {
        let stopped = blocked(&verdicts);
        let mut changed = false;
        for (index, manifest) in loaded.iter().enumerate() {
            if verdicts[index].is_some() {
                continue;
            }
            let needs = manifest
                .as_ref()
                .and_then(|manifest| {
                    manifest.manifest.raw()["extensionDependencies"]
                        .as_array()
                        .cloned()
                })
                .unwrap_or_default();
            if let Some(need) = needs
                .iter()
                .filter_map(Value::as_str)
                .find(|need| stopped.contains(&need.to_lowercase()))
            {
                verdicts[index] = Some(Verdict::NeedsBlocked(need.to_owned()));
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    // The rest: copied when local, otherwise looked up on Open VSX.
    let pending: Vec<usize> = (0..found.len())
        .filter(|index| verdicts[*index].is_none())
        .collect();
    let mut listings: HashMap<usize, Option<Listing>> = HashMap::new();
    if let Some(market) = market {
        let remote: Vec<usize> = pending
            .iter()
            .copied()
            .filter(|index| !found[*index].local)
            .collect();
        std::thread::scope(|scope| {
            let handles: Vec<_> = remote
                .chunks(remote.len().div_ceil(8).max(1))
                .map(|chunk| {
                    scope.spawn(move || {
                        chunk
                            .iter()
                            .map(|index| (*index, market.latest(&found[*index].id).ok()))
                            .collect::<Vec<_>>()
                    })
                })
                .collect();
            for handle in handles {
                listings.extend(handle.join().unwrap_or_default());
            }
        });
    }
    for index in pending {
        let partial: Vec<Blocker> = loaded[index].as_ref().map(blockers).unwrap_or_default();
        verdicts[index] = Some(if found[index].local {
            Verdict::Local { partial }
        } else {
            match listings.remove(&index).flatten() {
                Some(listing) => Verdict::Available {
                    listing: Box::new(listing),
                    partial,
                },
                None => Verdict::NotOnOpenVsx,
            }
        });
    }
    found
        .iter()
        .zip(loaded)
        .zip(verdicts)
        .map(|((extension, manifest), verdict)| {
            let raw = manifest
                .as_ref()
                .map(|manifest| manifest.manifest.raw().clone())
                .unwrap_or_default();
            let name = raw["displayName"]
                .as_str()
                .filter(|name| !name.starts_with('%'))
                .map(str::to_owned)
                .unwrap_or_else(|| extension.id.clone());
            let description = raw["description"]
                .as_str()
                .filter(|text| !text.starts_with('%'))
                .unwrap_or_default()
                .to_owned();
            CursorEntry {
                extension: extension.clone(),
                name,
                description,
                verdict: verdict.unwrap_or(Verdict::NotOnOpenVsx),
            }
        })
        .collect()
}

/// What keeps `extension` from working in Keel, from its `package.json`
/// and its main script.
pub fn blockers(extension: &InstalledExtension) -> Vec<Blocker> {
    let raw = extension.manifest.raw();
    let contributes = &raw["contributes"];
    let has = |key: &str| match &contributes[key] {
        Value::Array(items) => !items.is_empty(),
        Value::Object(items) => !items.is_empty(),
        _ => false,
    };
    let mut found = Vec::new();
    if extension
        .manifest
        .publisher
        .eq_ignore_ascii_case("anysphere")
    {
        found.push(Blocker::CursorOnly);
    }
    if has("debuggers") {
        found.push(Blocker::Debugger);
    }
    if has("notebooks") || has("notebookRenderer") {
        found.push(Blocker::Notebooks);
    }
    if has("customEditors") {
        found.push(Blocker::CustomEditors);
    }
    if has("viewsContainers") {
        found.push(Blocker::Views);
    }
    let main = extension
        .manifest
        .main
        .as_deref()
        .map(|main| extension.root.join(main))
        .and_then(|main| {
            [
                main.clone(),
                main.with_extension("js"),
                PathBuf::from(format!("{}.js", main.display())),
            ]
            .into_iter()
            .find(|candidate| candidate.is_file())
        })
        .and_then(|main| std::fs::read(main).ok())
        .unwrap_or_default();
    let contains = |needle: &[u8]| main.windows(needle.len()).any(|window| window == needle);
    if contains(b"createWebviewPanel") || contains(b"registerWebviewViewProvider") {
        found.push(Blocker::Webviews);
    }
    if contains(b"vscode-languageclient") || contains(b"LanguageClient(") {
        found.push(Blocker::LanguageServer);
    }
    found.sort();
    found.dedup();
    found
}

/// Copies an unpacked extension from Cursor into Keel's `extensions`
/// folder, as `publisher.name-version`.
pub fn copy_extension(from: &Path, extensions_dir: &Path) -> Result<InstalledExtension> {
    let source = InstalledExtension::load(from)?;
    let target = extensions_dir.join(format!("{}-{}", source.id(), source.manifest.version));
    if target.exists() {
        bail!(
            "{} {} is already installed",
            source.id(),
            source.manifest.version
        );
    }
    copy_folder(from, &target).with_context(|| format!("cannot copy {}", from.display()))?;
    InstalledExtension::load(&target)
}

fn copy_folder(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let kind = entry.file_type()?;
        let target = to.join(entry.file_name());
        if kind.is_dir() {
            copy_folder(&entry.path(), &target)?;
        } else if kind.is_file() {
            std::fs::copy(entry.path(), target)?;
        }
        // Links are left out: an extension's own files are plain files.
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_extension(
        folder: &Path,
        id: &str,
        version: &str,
        contributes: &str,
        main: &str,
    ) -> PathBuf {
        let (publisher, name) = id.split_once('.').unwrap();
        let root = folder.join(format!("{id}-{version}-universal"));
        std::fs::create_dir_all(root.join("dist")).unwrap();
        std::fs::write(
            root.join("package.json"),
            format!(
                r#"{{ "name": "{name}", "publisher": "{publisher}", "version": "{version}",
                     "main": "./dist/extension", "contributes": {contributes} }}"#
            ),
        )
        .unwrap();
        std::fs::write(root.join("dist/extension.js"), main).unwrap();
        root
    }

    #[test]
    fn cursors_list_is_read_and_blockers_found() {
        let folder = tempfile::tempdir().unwrap();
        let theme = write_extension(
            folder.path(),
            "dracula-theme.theme-dracula",
            "2.25.1",
            r#"{ "themes": [] }"#,
            "",
        );
        let java = write_extension(
            folder.path(),
            "redhat.java",
            "1.58.0",
            r#"{ "debuggers": [{ "type": "java" }], "viewsContainers": { "activitybar": [{ "id": "java" }] } }"#,
            "const { LanguageClient } = require('vscode-languageclient/node');",
        );
        let ssh = write_extension(folder.path(), "anysphere.remote-ssh", "1.1.17", "{}", "");
        let ghost = write_extension(
            folder.path(),
            "manuel-local.vsauto-ghost",
            "0.6.1",
            "{}",
            "panel.webview; vscode.window.createWebviewPanel(",
        );
        std::fs::write(
            folder.path().join("extensions.json"),
            serde_json::json!([
                { "identifier": { "id": "redhat.java" }, "version": "1.58.0", "relativeLocation": java.file_name().unwrap().to_str().unwrap(), "metadata": { "source": "gallery" } },
                { "identifier": { "id": "dracula-theme.theme-dracula" }, "version": "2.25.1", "relativeLocation": theme.file_name().unwrap().to_str().unwrap(), "metadata": { "source": "gallery" } },
                { "identifier": { "id": "anysphere.remote-ssh" }, "version": "1.1.17", "relativeLocation": ssh.file_name().unwrap().to_str().unwrap() },
                { "identifier": { "id": "manuel-local.vsauto-ghost" }, "version": "0.6.1", "relativeLocation": ghost.file_name().unwrap().to_str().unwrap(), "metadata": { "source": "vsix" } },
                { "identifier": { "id": "gone.missing" }, "version": "1.0.0", "relativeLocation": "gone.missing-1.0.0" }
            ])
            .to_string(),
        )
        .unwrap();

        let found = cursor_extensions(folder.path()).unwrap();
        let ids: Vec<&str> = found
            .iter()
            .map(|extension| extension.id.as_str())
            .collect();
        assert_eq!(
            ids,
            [
                "anysphere.remote-ssh",
                "dracula-theme.theme-dracula",
                "manuel-local.vsauto-ghost",
                "redhat.java"
            ]
        );
        assert!(found[2].local);
        assert!(!found[1].local);

        let blockers_of = |root: &Path| blockers(&InstalledExtension::load(root).unwrap());
        assert!(blockers_of(&theme).is_empty());
        assert_eq!(
            blockers_of(&java),
            [Blocker::Debugger, Blocker::Views, Blocker::LanguageServer]
        );
        assert_eq!(blockers_of(&ssh), [Blocker::CursorOnly]);
        assert_eq!(blockers_of(&ghost), [Blocker::Webviews]);
        assert_eq!(built_in("esbenp.prettier-vscode"), Some(BuiltIn::Prettier));
        assert_eq!(built_in("dracula-theme.theme-dracula"), None);

        // Open VSX made of files has Dracula only.
        let registry = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(registry.path().join("api/dracula-theme")).unwrap();
        std::fs::write(
            registry.path().join("api/dracula-theme/theme-dracula"),
            r#"{ "namespace": "dracula-theme", "name": "theme-dracula", "version": "2.25.1" }"#,
        )
        .unwrap();
        let market = Marketplace::with_base(format!("file://{}", registry.path().display()));
        let report = analyze(&found, &HashSet::new(), Some(&market));
        let verdicts: Vec<(&str, &Verdict)> = report
            .iter()
            .map(|entry| (entry.extension.id.as_str(), &entry.verdict))
            .collect();
        assert_eq!(
            verdicts[0],
            (
                "anysphere.remote-ssh",
                &Verdict::Blocked(vec![Blocker::CursorOnly])
            )
        );
        assert!(
            matches!(verdicts[1].1, Verdict::Available { listing, partial } if listing.version == "2.25.1" && partial.is_empty())
        );
        assert_eq!(
            verdicts[2].1,
            &Verdict::Local {
                partial: vec![Blocker::Webviews]
            }
        );
        assert_eq!(
            verdicts[3].1,
            &Verdict::Blocked(vec![
                Blocker::Debugger,
                Blocker::Views,
                Blocker::LanguageServer
            ])
        );
        let installed: HashSet<String> = ["dracula-theme.theme-dracula".to_owned()].into();
        let report = analyze(&found, &installed, None);
        assert_eq!(report[1].verdict, Verdict::Installed);

        let keel = tempfile::tempdir().unwrap();
        let copied = copy_extension(&ghost, keel.path()).unwrap();
        assert_eq!(
            copied.root,
            keel.path().join("manuel-local.vsauto-ghost-0.6.1")
        );
        assert!(copied.root.join("dist/extension.js").is_file());
        assert!(copy_extension(&ghost, keel.path()).is_err());
    }

    /// Prints what Keel would do with each extension of the real Cursor:
    /// `cargo test -p extensions -- --ignored real_cursor --nocapture`.
    #[test]
    #[ignore]
    fn real_cursor_report() {
        let folder = cursor_extensions_dir().unwrap();
        let found = cursor_extensions(&folder).unwrap();
        let started = std::time::Instant::now();
        let report = analyze(&found, &HashSet::new(), Some(&Marketplace::open_vsx()));
        println!(
            "{} extensions looked at in {:?}",
            report.len(),
            started.elapsed()
        );
        for entry in report {
            let verdict = match &entry.verdict {
                Verdict::Available { listing, partial } => {
                    format!("Available {} {partial:?}", listing.version)
                }
                other => format!("{other:?}"),
            };
            println!("{:45} {verdict}", entry.extension.id);
        }
    }
}
