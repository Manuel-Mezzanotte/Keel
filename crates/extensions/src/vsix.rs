//! VSIX packages: the zip files VS Code extensions are distributed as.

use std::fs;
use std::io::{self, Read as _};
use std::path::{Component, Path, PathBuf};

use anyhow::{Context as _, Result, bail};

use crate::manifest::Manifest;

/// Files of the extension live under this folder inside the package.
const EXTENSION_FOLDER: &str = "extension/";

/// Refuse packages that would unpack to more than this, to stop zip bombs.
const MAX_UNPACKED_BYTES: u64 = 1024 * 1024 * 1024;

/// A VSIX package on disk.
pub struct Vsix {
    path: PathBuf,
    manifest: Manifest,
}

/// An extension unpacked in a folder, ready to be loaded.
#[derive(Clone, Debug)]
pub struct InstalledExtension {
    pub root: PathBuf,
    pub manifest: Manifest,
}

impl Vsix {
    /// Opens a package and reads its `package.json`.
    pub fn open(path: impl Into<PathBuf>) -> Result<Self> {
        let path = path.into();
        let mut archive = open_archive(&path)?;
        let mut entry = archive
            .by_name("extension/package.json")
            .with_context(|| format!("{} has no extension/package.json", path.display()))?;
        let mut bytes = Vec::new();
        entry.read_to_end(&mut bytes)?;
        let manifest = Manifest::from_json(&bytes)?;
        drop(entry);
        Ok(Self { path, manifest })
    }

    pub fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    /// Unpacks the extension files into `destination`, which must not exist
    /// yet or be empty. Entries that would land outside of it are refused.
    pub fn extract(&self, destination: &Path) -> Result<InstalledExtension> {
        let mut archive = open_archive(&self.path)?;
        fs::create_dir_all(destination)?;
        if fs::read_dir(destination)?.next().is_some() {
            bail!("{} is not empty", destination.display());
        }

        let mut unpacked: u64 = 0;
        for index in 0..archive.len() {
            let mut entry = archive.by_index(index)?;
            let name = entry.name()?.into_owned();
            let Some(relative) = extension_relative_path(&name)? else {
                continue;
            };
            let target = destination.join(&relative);
            if entry.is_dir() {
                fs::create_dir_all(&target)?;
                continue;
            }
            unpacked += entry.size();
            if unpacked > MAX_UNPACKED_BYTES {
                bail!("{} unpacks to more than 1 GB", self.path.display());
            }
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent)?;
            }
            let mut file = fs::File::create(&target)?;
            io::copy(&mut entry, &mut file)?;
        }
        InstalledExtension::load(destination)
    }
}

impl InstalledExtension {
    /// Reads an extension that is already unpacked in `root`.
    pub fn load(root: impl Into<PathBuf>) -> Result<Self> {
        let root = root.into();
        let bytes = fs::read(root.join("package.json"))
            .with_context(|| format!("{} has no package.json", root.display()))?;
        let manifest = Manifest::from_json(&bytes)?;
        Ok(Self { root, manifest })
    }

    pub fn id(&self) -> String {
        self.manifest.id()
    }
}

fn open_archive(path: &Path) -> Result<zip::ZipArchive<fs::File>> {
    let file = fs::File::open(path).with_context(|| format!("cannot open {}", path.display()))?;
    zip::ZipArchive::new(file).with_context(|| format!("{} is not a VSIX package", path.display()))
}

/// The path of a package entry relative to the extension root, or `None` for
/// entries outside `extension/`, like the package metadata.
fn extension_relative_path(name: &str) -> Result<Option<PathBuf>> {
    let Some(rest) = name.strip_prefix(EXTENSION_FOLDER) else {
        return Ok(None);
    };
    if rest.is_empty() {
        return Ok(None);
    }
    let relative = Path::new(rest);
    let safe = relative
        .components()
        .all(|component| matches!(component, Component::Normal(_)));
    if !safe || rest.contains('\\') {
        bail!("the package contains an unsafe path: {name}");
    }
    Ok(Some(relative.to_path_buf()))
}

#[cfg(test)]
mod tests {
    use std::io::Write as _;

    use zip::write::SimpleFileOptions;

    use super::*;

    fn vsauto_ghost() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/extensions/vsauto-ghost-local-0.6.1-editor-layout.vsix")
    }

    #[test]
    fn reads_the_vsauto_ghost_package() {
        let vsix = Vsix::open(vsauto_ghost()).unwrap();
        let manifest = vsix.manifest();
        assert_eq!(manifest.id(), "manuel-local.vsauto-ghost");
        assert_eq!(manifest.version, "0.6.1");
        assert_eq!(manifest.main.as_deref(), Some("./out/extension.js"));
        assert!(
            manifest
                .commands
                .iter()
                .any(|command| command.command == "vsauto.start")
        );
        assert_eq!(manifest.keybindings.len(), 24);
        assert_eq!(manifest.configuration_defaults().len(), 20);
        // Its settings, as the Settings tab shows them.
        let sections = crate::configuration::sections(manifest.raw());
        assert_eq!(sections.len(), 1);
        let settings = &sections[0].settings;
        assert_eq!(settings.len(), 20);
        let typing = settings
            .iter()
            .find(|setting| setting.key == "vsauto.defaultTypingMode")
            .unwrap();
        assert_eq!(typing.title, "Default Typing Mode");
        let crate::configuration::SettingKind::Choice(choices) = &typing.kind else {
            panic!("a choice: {:?}", typing.kind);
        };
        let values: Vec<&str> = choices.iter().map(|choice| choice.value.as_str()).collect();
        assert_eq!(values, ["human", "normal", "fast", "turbo"]);
        assert!(
            settings
                .iter()
                .all(|setting| !setting.description.is_empty())
        );
    }

    #[test]
    fn extracts_the_extension_files() {
        let folder = tempfile::tempdir().unwrap();
        let destination = folder.path().join("vsauto-ghost");
        let installed = Vsix::open(vsauto_ghost())
            .unwrap()
            .extract(&destination)
            .unwrap();

        assert_eq!(installed.id(), "manuel-local.vsauto-ghost");
        assert!(destination.join("out/extension.js").is_file());
        assert!(destination.join("out/vendor/typescript.js").is_file());
        assert!(!destination.join("extension.vsixmanifest").exists());
    }

    fn package_with(entries: &[(&str, &str)]) -> (tempfile::TempDir, PathBuf) {
        let folder = tempfile::tempdir().unwrap();
        let path = folder.path().join("test.vsix");
        let mut writer = zip::ZipWriter::new(fs::File::create(&path).unwrap());
        for (name, contents) in entries {
            writer
                .start_file(*name, SimpleFileOptions::default())
                .unwrap();
            writer.write_all(contents.as_bytes()).unwrap();
        }
        writer.finish().unwrap();
        (folder, path)
    }

    #[test]
    fn refuses_entries_that_escape_the_destination() {
        let manifest = r#"{ "name": "evil", "publisher": "x", "version": "1.0.0" }"#;
        let (folder, path) = package_with(&[
            ("extension/package.json", manifest),
            ("extension/../../escaped.txt", "gotcha"),
        ]);
        let destination = folder.path().join("unpacked");

        let error = Vsix::open(&path)
            .unwrap()
            .extract(&destination)
            .unwrap_err();

        assert!(error.to_string().contains("unsafe path"), "{error}");
        assert!(!folder.path().join("escaped.txt").exists());
    }

    #[test]
    fn refuses_files_that_are_not_packages() {
        let folder = tempfile::tempdir().unwrap();
        let path = folder.path().join("not.vsix");
        fs::write(&path, "plain text").unwrap();
        assert!(Vsix::open(&path).is_err());

        let (_folder, path) = package_with(&[("readme.txt", "no manifest")]);
        assert!(Vsix::open(&path).is_err());
    }
}
