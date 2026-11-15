//! The projects opened lately, for File → Open Recent and the welcome
//! screen. They are kept in `recent.json` in Keel's data folder.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use gpui::{App, Global};
use serde_json::{Value, json};

/// How many projects the list keeps.
const LIMIT: usize = 20;

pub struct RecentProjects {
    file: PathBuf,
    projects: Vec<PathBuf>,
}

impl Global for RecentProjects {}

impl RecentProjects {
    /// Reads the list. A missing or broken file means an empty list.
    pub fn load(file: impl Into<PathBuf>) -> Self {
        let file = file.into();
        let projects = fs::read_to_string(&file)
            .ok()
            .and_then(|text| serde_json::from_str::<Value>(&text).ok())
            .and_then(|value| {
                value["projects"].as_array().map(|projects| {
                    projects
                        .iter()
                        .filter_map(Value::as_str)
                        .map(PathBuf::from)
                        .collect()
                })
            })
            .unwrap_or_default();
        Self { file, projects }
    }

    /// The projects, the latest first, leaving out folders that are gone.
    pub fn existing(&self) -> Vec<PathBuf> {
        self.projects
            .iter()
            .filter(|project| project.is_dir())
            .cloned()
            .collect()
    }

    /// Puts a project at the top of the list and saves the list.
    pub fn add(&mut self, project: &Path) -> io::Result<()> {
        self.projects.retain(|known| known != project);
        self.projects.insert(0, project.to_path_buf());
        self.projects.truncate(LIMIT);
        self.save()
    }

    fn save(&self) -> io::Result<()> {
        if let Some(folder) = self.file.parent() {
            fs::create_dir_all(folder)?;
        }
        let projects: Vec<String> = self
            .projects
            .iter()
            .map(|project| project.to_string_lossy().into_owned())
            .collect();
        let text = serde_json::to_string_pretty(&json!({ "projects": projects }))? + "\n";
        let temporary = self.file.with_extension("json.keel-save");
        fs::write(&temporary, text)?;
        fs::rename(&temporary, &self.file)
    }
}

/// Loads the list as a global, for the windows and the menu bar.
pub fn init(file: PathBuf, cx: &mut App) {
    cx.set_global(RecentProjects::load(file));
}

/// The recent projects, or none when the list was never loaded (in tests).
pub fn projects(cx: &App) -> Vec<PathBuf> {
    cx.try_global::<RecentProjects>()
        .map(RecentProjects::existing)
        .unwrap_or_default()
}

/// Remembers a project and refreshes the menu bar's Open Recent.
pub fn remember(project: &Path, cx: &mut App) {
    if !cx.has_global::<RecentProjects>() {
        return;
    }
    if let Err(error) = cx.global_mut::<RecentProjects>().add(project) {
        eprintln!("keel: could not save the recent projects: {error}");
    }
}

/// A project's folder written with `~` for the home folder, as Finder shows it.
pub fn short_path(path: &Path) -> String {
    match std::env::home_dir().and_then(|home| path.strip_prefix(home).ok().map(Path::to_path_buf))
    {
        Some(relative) if relative.as_os_str().is_empty() => "~".to_owned(),
        Some(relative) => format!("~/{}", relative.display()),
        None => path.display().to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_latest_project_comes_first_once() {
        let data = tempfile::tempdir().unwrap();
        let file = data.path().join("recent.json");
        let projects: Vec<PathBuf> = ["Keel", "Silo", "GestionaleHospitality"]
            .iter()
            .map(|name| data.path().join(name))
            .collect();
        for project in &projects {
            fs::create_dir_all(project).unwrap();
        }

        let mut recent = RecentProjects::load(&file);
        assert!(recent.existing().is_empty());
        for project in &projects {
            recent.add(project).unwrap();
        }
        recent.add(&projects[0]).unwrap();

        let reloaded = RecentProjects::load(&file);
        assert_eq!(
            reloaded.existing(),
            [
                projects[0].clone(),
                projects[2].clone(),
                projects[1].clone()
            ]
        );
    }

    #[test]
    fn the_list_is_short_and_skips_deleted_folders() {
        let data = tempfile::tempdir().unwrap();
        let mut recent = RecentProjects::load(data.path().join("recent.json"));
        for index in 0..LIMIT + 5 {
            let project = data.path().join(format!("project-{index}"));
            fs::create_dir_all(&project).unwrap();
            recent.add(&project).unwrap();
        }
        assert_eq!(recent.existing().len(), LIMIT);

        recent.add(&data.path().join("gone")).unwrap();
        assert_eq!(recent.existing().len(), LIMIT - 1);
    }

    #[test]
    fn a_broken_file_is_an_empty_list() {
        let data = tempfile::tempdir().unwrap();
        let file = data.path().join("recent.json");
        fs::write(&file, "{ not json").unwrap();
        assert!(RecentProjects::load(&file).existing().is_empty());
    }

    #[test]
    fn paths_in_the_home_folder_use_a_tilde() {
        let home = std::env::home_dir().unwrap();
        assert_eq!(
            short_path(&home.join("Desktop/MiniPCProgram/Keel")),
            "~/Desktop/MiniPCProgram/Keel"
        );
        assert_eq!(short_path(Path::new("/opt/projects")), "/opt/projects");
    }
}
