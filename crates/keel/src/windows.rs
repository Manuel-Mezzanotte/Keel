//! Keel's windows: one per project, empty ones with the welcome screen, and
//! opening a path in the right one.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use extensions::ExtensionHost;
use gpui::{
    App, AppContext as _, Bounds, Global, PathPromptOptions, WeakEntity, WindowHandle, point, px,
    size,
};
use gpui_component::Root;

use crate::workbench_window::{
    ExtensionSetup, NewWindow, Open, OpenProject, Opening, Workbench, window_options,
};
use crate::{keymap, menus, paths, recent, session};

/// Where a path opens.
#[derive(Debug, PartialEq, Eq)]
pub enum Plan {
    /// In an open window, the `window`-th of the list given to [`plan`].
    Existing {
        window: usize,
        file: Option<PathBuf>,
    },
    /// In a new window.
    New {
        root: Option<PathBuf>,
        file: Option<PathBuf>,
    },
}

/// Decides where a path opens, given the projects of the open windows. A
/// folder is a project: its window comes forward, or a new one opens. A
/// file opens in the window whose project holds it, otherwise in a new
/// window for its own project. Nothing at all is an empty window.
pub fn plan(path: Option<&Path>, roots: &[Option<PathBuf>]) -> Plan {
    let Some(path) = path else {
        return Plan::New {
            root: None,
            file: None,
        };
    };
    if path.is_dir() {
        return match roots.iter().position(|root| root.as_deref() == Some(path)) {
            Some(window) => Plan::Existing { window, file: None },
            None => Plan::New {
                root: Some(path.to_path_buf()),
                file: None,
            },
        };
    }
    let holder = roots
        .iter()
        .enumerate()
        .filter_map(|(index, root)| {
            let root = root.as_deref()?;
            path.starts_with(root)
                .then(|| (index, root.components().count()))
        })
        .max_by_key(|(_, depth)| *depth);
    match holder {
        Some((window, _)) => Plan::Existing {
            window,
            file: Some(path.to_path_buf()),
        },
        None => Plan::New {
            root: settings::project_root_for(path),
            file: Some(path.to_path_buf()),
        },
    }
}

struct OpenWindow {
    handle: WindowHandle<Root>,
    workbench: WeakEntity<Workbench>,
    root: Option<PathBuf>,
}

#[derive(Default)]
struct Windows(Vec<OpenWindow>);

impl Global for Windows {}

/// Registers the actions that work without a window: Open, New Window and
/// the projects of File → Open Recent.
pub fn init(cx: &mut App) {
    cx.set_global(Windows::default());
    cx.on_action(|_: &NewWindow, cx| open(None, cx));
    cx.on_action(|action: &OpenProject, cx| open(Some(action.path.clone()), cx));
    cx.on_action(|_: &Open, cx| prompt_and_open(cx));
}

/// Asks for a file or a folder, then opens it.
pub fn prompt_and_open(cx: &mut App) {
    let chosen = cx.prompt_for_paths(PathPromptOptions {
        files: true,
        directories: true,
        multiple: false,
        prompt: None,
    });
    cx.spawn(async move |cx| {
        if let Ok(Ok(Some(paths))) = chosen.await
            && let Some(path) = paths.into_iter().next()
        {
            cx.update(|cx| open(Some(path), cx));
        }
    })
    .detach();
}

/// Opens what Keel starts with: the path given, or else the projects that
/// were open when Keel quit (`window.restoreWindows`, on by default).
pub fn open_startup(path: Option<PathBuf>, cx: &mut App) {
    if path.is_some() {
        open(path, cx);
        return;
    }
    let data = paths::data_dir();
    let settings = settings::Settings::load(paths::user_settings_file(&data), None);
    let restore = settings.string("window.restoreWindows") != Some("none");
    let projects: Vec<PathBuf> = session::load_windows(&data)
        .into_iter()
        .filter(|project| project.is_dir())
        .collect();
    if !restore || projects.is_empty() {
        open(None, cx);
        return;
    }
    for project in projects {
        open(Some(project), cx);
    }
}

/// The projects of the open windows, to reopen at the next start.
fn remember_windows(windows: &[Option<PathBuf>]) {
    let projects: Vec<PathBuf> = windows.iter().flatten().cloned().collect();
    if let Err(error) = session::save_windows(&paths::data_dir(), &projects) {
        eprintln!("keel: could not keep the open windows: {error}");
    }
}

/// A window is closing: the others are remembered, or, if it is the last
/// one, this one, so Keel reopens where you left.
pub fn window_closing(workbench: gpui::EntityId, cx: &mut App) {
    let Some(windows) = cx.try_global::<Windows>() else {
        return;
    };
    let closing = windows
        .0
        .iter()
        .find(|window| window.workbench.entity_id() == workbench)
        .map(|window| window.root.clone());
    let others: Vec<Option<PathBuf>> = windows
        .0
        .iter()
        .filter(|window| window.workbench.entity_id() != workbench)
        .filter(|window| window.workbench.upgrade().is_some())
        .map(|window| window.root.clone())
        .collect();
    if others.iter().any(Option::is_some) {
        remember_windows(&others);
    } else if let Some(closing) = closing {
        remember_windows(&[closing]);
    }
}

/// Quits keeping everything: every window's session and unsaved changes,
/// and which windows were open. `asking` is the window that asked, which
/// already kept its own.
pub fn quit_keeping_everything(asking: gpui::EntityId, cx: &mut App) {
    if let Some(windows) = cx.try_global::<Windows>() {
        let open: Vec<_> = windows
            .0
            .iter()
            .filter_map(|window| Some((window.workbench.upgrade()?, window.root.clone())))
            .collect();
        for (workbench, _) in &open {
            if workbench.entity_id() != asking {
                workbench.read(cx).persist_now(cx);
            }
        }
        let roots: Vec<Option<PathBuf>> = open.into_iter().map(|(_, root)| root).collect();
        remember_windows(&roots);
    }
    cx.quit();
}

/// Opens a path, or an empty window for `None`, where [`plan`] says.
pub fn open(path: Option<PathBuf>, cx: &mut App) {
    let path = path.map(|path| paths::normalize(&path));
    let windows = &mut cx.global_mut::<Windows>().0;
    windows.retain(|window| window.workbench.upgrade().is_some());
    let roots: Vec<Option<PathBuf>> = windows.iter().map(|window| window.root.clone()).collect();
    match plan(path.as_deref(), &roots) {
        Plan::Existing { window, file } => {
            let window = &cx.global::<Windows>().0[window];
            let (handle, workbench) = (window.handle, window.workbench.clone());
            let _ = handle.update(cx, |_, window, cx| {
                window.activate_window();
                if let Some(file) = file {
                    let _ = workbench
                        .update(cx, |workbench, cx| workbench.open_file(&file, window, cx));
                }
            });
        }
        Plan::New { root, file } => open_window(root, file, cx),
    }
}

/// The active window, when it shows nothing: no project and no files.
/// Opening a project from it replaces it, as VS Code does.
fn blank_active_window(cx: &App) -> Option<WindowHandle<Root>> {
    let active = cx.active_window()?;
    cx.global::<Windows>().0.iter().find_map(|window| {
        let blank = window
            .workbench
            .upgrade()
            .is_some_and(|workbench| workbench.read(cx).is_blank());
        (window.handle.window_id() == active.window_id() && blank).then_some(window.handle)
    })
}

fn open_window(root: Option<PathBuf>, file: Option<PathBuf>, cx: &mut App) {
    let replaced = if root.is_some() || file.is_some() {
        blank_active_window(cx)
    } else {
        None
    };
    let data = paths::data_dir();
    let settings = settings::Settings::load(paths::user_settings_file(&data), root.as_deref());
    for warning in &settings.warnings {
        eprintln!("keel: {warning}");
    }
    let setup = extension_setup(&data);

    // Each new window sits a little lower and to the right of the last one.
    let cascade = px(28.) * cx.global::<Windows>().0.len().min(8) as f32;
    let mut bounds = Bounds::centered(None, size(px(1280.), px(820.)), cx);
    bounds.origin += point(cascade, cascade);

    let created: Rc<RefCell<Option<WeakEntity<Workbench>>>> = Rc::default();
    let opening = Opening::new(root.clone(), file).with_data_dir(data.clone());
    let look = crate::workbench_window::look::Look::from_settings(&settings);
    let opened = cx.open_window(window_options(bounds, look), {
        let created = created.clone();
        move |window, cx| {
            let view = cx.new(|cx| {
                let mut workbench = Workbench::new(opening, settings, setup, window, cx);
                if let Some(ide_dir) = ai::ide::default_ide_dir() {
                    workbench.start_claude_ide(ide_dir, window, cx);
                }
                workbench
            });
            *created.borrow_mut() = Some(view.downgrade());
            cx.new(|cx| Root::new(view, window, cx))
        }
    });
    let handle = match opened {
        Ok(handle) => handle,
        Err(error) => {
            eprintln!("keel: could not open the window: {error}");
            return;
        }
    };
    if let Some(workbench) = created.take() {
        // The first window of the very first start shows the welcome.
        if cx.has_global::<crate::workbench_window::WelcomePending>() {
            cx.remove_global::<crate::workbench_window::WelcomePending>();
            let shown = workbench.clone();
            let _ = handle.update(cx, |_, window, cx| {
                shown
                    .update(cx, |workbench, cx| workbench.start_intro(window, cx))
                    .ok();
            });
        }
        cx.global_mut::<Windows>().0.push(OpenWindow {
            handle,
            workbench,
            root: root.clone(),
        });
    }
    if let Some(root) = &root {
        recent::remember(root, cx);
        menus::refresh(cx);
    }
    let open: Vec<Option<PathBuf>> = cx
        .global::<Windows>()
        .0
        .iter()
        .filter(|window| window.workbench.upgrade().is_some())
        .map(|window| window.root.clone())
        .collect();
    remember_windows(&open);
    if let Some(replaced) = replaced {
        let _ = replaced.update(cx, |_, window, _| window.remove_window());
    }
}

/// Starts an extension host for a window, with the installed extensions.
/// The window still opens without extensions if the host cannot start.
pub(crate) fn extension_setup(data: &Path) -> Option<ExtensionSetup> {
    let (user_bindings, _) = keymap::read_user_bindings(&paths::user_keybindings_file(data));
    let installed = paths::installed_extensions(data);
    match ExtensionHost::start() {
        Ok((host, events)) => Some(ExtensionSetup {
            user_bindings,
            host,
            events,
            installed,
            data_dir: data.to_path_buf(),
            marketplace: Some(extensions::marketplace::Marketplace::open_vsx()),
        }),
        Err(error) => {
            eprintln!("keel: extensions are off, the extension host did not start: {error:#}");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    #[test]
    fn folders_and_files_open_in_the_right_window() {
        let folder = tempfile::tempdir().unwrap();
        let keel = folder.path().join("Keel");
        let crates = keel.join("crates/editor");
        let silo = folder.path().join("Silo");
        fs::create_dir_all(keel.join(".git")).unwrap();
        fs::create_dir_all(&crates).unwrap();
        fs::create_dir_all(silo.join(".git")).unwrap();
        let file = crates.join("lib.rs");
        fs::write(&file, "").unwrap();
        let loose = folder.path().join("notes.md");

        assert_eq!(
            plan(None, &[]),
            Plan::New {
                root: None,
                file: None
            }
        );
        // A folder already open comes forward; another opens its own window.
        let roots = [None, Some(keel.clone())];
        assert_eq!(
            plan(Some(&keel), &roots),
            Plan::Existing {
                window: 1,
                file: None
            }
        );
        assert_eq!(
            plan(Some(&silo), &roots),
            Plan::New {
                root: Some(silo.clone()),
                file: None
            }
        );
        // A file goes to the deepest project that holds it.
        let nested = [Some(keel.clone()), Some(keel.join("crates"))];
        assert_eq!(
            plan(Some(&file), &nested),
            Plan::Existing {
                window: 1,
                file: Some(file.clone())
            }
        );
        // A file outside every open project brings its own project.
        assert_eq!(
            plan(Some(&file), &[Some(silo.clone())]),
            Plan::New {
                root: Some(keel.clone()),
                file: Some(file.clone())
            }
        );
        assert_eq!(
            plan(Some(&loose), &[]),
            Plan::New {
                root: Some(folder.path().to_path_buf()),
                file: Some(loose.clone())
            }
        );
    }
}
