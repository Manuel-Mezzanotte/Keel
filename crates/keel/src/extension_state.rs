//! Which installed extensions are turned off: everywhere, or only in some
//! projects, as VS Code's "Disable" and "Disable (Workspace)". Kept in
//! `extension-state.json` in Keel's data folder.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use anyhow::Result;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtensionState {
    /// Off everywhere.
    #[serde(default)]
    pub disabled: BTreeSet<String>,
    /// Off in a project, by the project's folder.
    #[serde(default)]
    pub disabled_in: BTreeMap<String, BTreeSet<String>>,
}

/// Where an extension is turned on or off.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Scope {
    Everywhere,
    Project(PathBuf),
}

fn file(data: &Path) -> PathBuf {
    data.join("extension-state.json")
}

/// Extension ids compare without case, as on the Marketplace.
fn key(id: &str) -> String {
    id.to_lowercase()
}

impl ExtensionState {
    /// Reads the state; a missing or broken file means everything is on.
    pub fn load(data: &Path) -> Self {
        std::fs::read_to_string(file(data))
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, data: &Path) -> Result<()> {
        std::fs::create_dir_all(data)?;
        std::fs::write(file(data), serde_json::to_string_pretty(self)?)?;
        Ok(())
    }

    /// Whether `id` runs in a window on `project`.
    pub fn is_enabled(&self, id: &str, project: Option<&Path>) -> bool {
        let id = key(id);
        if self.disabled.contains(&id) {
            return false;
        }
        project.is_none_or(|project| {
            self.disabled_in
                .get(&project.to_string_lossy().into_owned())
                .is_none_or(|ids| !ids.contains(&id))
        })
    }

    /// Turns `id` on or off. Turning it on clears both kinds of off.
    pub fn set(&mut self, id: &str, enabled: bool, scope: &Scope) {
        let id = key(id);
        if enabled {
            self.disabled.remove(&id);
            if let Scope::Project(project) = scope {
                let project = project.to_string_lossy().into_owned();
                if let Some(ids) = self.disabled_in.get_mut(&project) {
                    ids.remove(&id);
                    if ids.is_empty() {
                        self.disabled_in.remove(&project);
                    }
                }
            }
            return;
        }
        match scope {
            Scope::Everywhere => {
                self.disabled.insert(id);
            }
            Scope::Project(project) => {
                self.disabled_in
                    .entry(project.to_string_lossy().into_owned())
                    .or_default()
                    .insert(id);
            }
        }
    }

    /// Forgets an uninstalled extension.
    pub fn forget(&mut self, id: &str) {
        let id = key(id);
        self.disabled.remove(&id);
        for ids in self.disabled_in.values_mut() {
            ids.remove(&id);
        }
        self.disabled_in.retain(|_, ids| !ids.is_empty());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn off_everywhere_or_in_one_project() {
        let data = tempfile::tempdir().unwrap();
        let hospitality = Path::new("/progetti/Hospitality");
        let salon = Path::new("/progetti/Salon");
        let mut state = ExtensionState::load(data.path());
        assert!(state.is_enabled("Dracula-Theme.theme-dracula", Some(hospitality)));
        state.set(
            "dracula-theme.theme-dracula",
            false,
            &Scope::Project(hospitality.into()),
        );
        assert!(!state.is_enabled("Dracula-Theme.theme-dracula", Some(hospitality)));
        assert!(state.is_enabled("Dracula-Theme.theme-dracula", Some(salon)));
        state.set("PKief.material-icon-theme", false, &Scope::Everywhere);
        assert!(!state.is_enabled("pkief.material-icon-theme", None));
        state.save(data.path()).unwrap();

        let mut read = ExtensionState::load(data.path());
        assert_eq!(read, state);
        read.set(
            "dracula-theme.theme-dracula",
            true,
            &Scope::Project(hospitality.into()),
        );
        assert!(read.disabled_in.is_empty());
        read.forget("pkief.material-icon-theme");
        assert!(read.disabled.is_empty());
    }
}
