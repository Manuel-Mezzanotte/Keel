//! Where Keel keeps its data, and installing extensions there.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result, bail};
use extensions::{InstalledExtension, Vsix};

/// Keel's data folder, like Sail's: `~/Library/Application Support/com.manuelmezzanotte.keel`.
/// `KEEL_DATA_DIR` overrides it, which tests use.
pub fn data_dir() -> PathBuf {
    if let Some(custom) = std::env::var_os("KEEL_DATA_DIR") {
        return PathBuf::from(custom);
    }
    std::env::home_dir()
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join("Library/Application Support/com.manuelmezzanotte.keel")
}

/// A path written the way Finder knows it: absolute, without `.` or `..`,
/// with symbolic links resolved. A file that does not exist yet keeps its
/// name in its resolved folder.
pub fn normalize(path: &Path) -> PathBuf {
    if let Ok(resolved) = fs::canonicalize(path) {
        return resolved;
    }
    let absolute = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
    match (absolute.parent(), absolute.file_name()) {
        (Some(folder), Some(name)) => match fs::canonicalize(folder) {
            Ok(folder) => folder.join(name),
            Err(_) => absolute,
        },
        _ => absolute,
    }
}

/// The user's settings, `settings.json` in the data folder.
pub fn user_settings_file(data: &Path) -> PathBuf {
    data.join("settings.json")
}

/// The user's key bindings, `keybindings.json` in the data folder.
pub fn user_keybindings_file(data: &Path) -> PathBuf {
    data.join("keybindings.json")
}

/// The projects opened lately, `recent.json` in the data folder.
pub fn recent_projects_file(data: &Path) -> PathBuf {
    data.join("recent.json")
}

/// Where Keel installs the language servers it needs.
pub fn servers_dir(data: &Path) -> PathBuf {
    data.join("servers")
}

pub fn extensions_dir(data: &Path) -> PathBuf {
    data.join("extensions")
}

/// Where packages and icons from Open VSX are kept while installing.
pub fn open_vsx_cache_dir(data: &Path) -> PathBuf {
    data.join("cache").join("open-vsx")
}

/// Where an extension keeps its global and workspace storage.
pub fn extension_storage_dir(data: &Path, id: &str) -> PathBuf {
    data.join("extension-storage").join(id)
}

/// Unpacks a VSIX into the extensions folder, as `publisher.name-version`.
/// The same version cannot be installed twice; a newer one sits next to the
/// older one and wins when Keel starts.
pub fn install_extension(data: &Path, package: &Path) -> Result<InstalledExtension> {
    let vsix = Vsix::open(package)?;
    let manifest = vsix.manifest();
    let folder = extensions_dir(data).join(format!("{}-{}", manifest.id(), manifest.version));
    if folder.exists() {
        bail!(
            "{} {} is already installed in {}",
            manifest.id(),
            manifest.version,
            folder.display()
        );
    }
    vsix.extract(&folder)
        .with_context(|| format!("cannot install {}", package.display()))
}

/// The installed extensions, keeping only the newest version of each.
pub fn installed_extensions(data: &Path) -> Vec<InstalledExtension> {
    let Ok(entries) = fs::read_dir(extensions_dir(data)) else {
        return Vec::new();
    };
    let mut newest: HashMap<String, InstalledExtension> = HashMap::new();
    for entry in entries.flatten() {
        let Ok(extension) = InstalledExtension::load(entry.path()) else {
            continue;
        };
        let id = extension.id();
        let keep = newest
            .get(&id)
            .is_none_or(|current| is_newer(&extension.manifest.version, &current.manifest.version));
        if keep {
            newest.insert(id, extension);
        }
    }
    let mut extensions: Vec<_> = newest.into_values().collect();
    extensions.sort_by_key(InstalledExtension::id);
    extensions
}

/// Compares dotted versions number by number, such as 0.10.0 > 0.9.2.
fn is_newer(candidate: &str, current: &str) -> bool {
    let parts = |version: &str| -> Vec<u64> {
        version
            .split(['.', '-'])
            .map(|part| part.parse().unwrap_or(0))
            .collect()
    };
    parts(candidate) > parts(current)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vsauto_ghost() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/extensions/vsauto-ghost-local-0.6.1-editor-layout.vsix")
    }

    #[test]
    fn installs_a_package_once() {
        let data = tempfile::tempdir().unwrap();
        let installed = install_extension(data.path(), &vsauto_ghost()).unwrap();
        assert_eq!(installed.id(), "manuel-local.vsauto-ghost");
        assert!(
            extensions_dir(data.path())
                .join("manuel-local.vsauto-ghost-0.6.1/out/extension.js")
                .is_file()
        );

        let again = install_extension(data.path(), &vsauto_ghost()).unwrap_err();
        assert!(again.to_string().contains("already installed"), "{again}");
    }

    #[test]
    fn keeps_the_newest_version_of_each_extension() {
        let data = tempfile::tempdir().unwrap();
        for version in ["0.9.2", "0.10.0", "0.2.0"] {
            let folder = extensions_dir(data.path()).join(format!("keel-tests.hello-{version}"));
            fs::create_dir_all(&folder).unwrap();
            fs::write(
                folder.join("package.json"),
                format!(
                    r#"{{ "name": "hello", "publisher": "keel-tests", "version": "{version}" }}"#
                ),
            )
            .unwrap();
        }
        let installed = installed_extensions(data.path());
        assert_eq!(installed.len(), 1);
        assert_eq!(installed[0].manifest.version, "0.10.0");
    }

    #[test]
    fn paths_are_normalized() {
        let folder = tempfile::tempdir().unwrap();
        let base = fs::canonicalize(folder.path()).unwrap();
        fs::create_dir_all(base.join("Keel")).unwrap();
        fs::create_dir_all(base.join("Silo")).unwrap();
        assert_eq!(normalize(&base.join("Keel/../Silo")), base.join("Silo"));
        assert_eq!(
            normalize(&base.join("Keel/./new.rs")),
            base.join("Keel/new.rs")
        );
    }

    #[test]
    fn no_extensions_folder_means_no_extensions() {
        let data = tempfile::tempdir().unwrap();
        assert!(installed_extensions(data.path()).is_empty());
    }
}
