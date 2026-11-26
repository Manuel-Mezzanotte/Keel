//! Keel, a fast and lightweight code editor for macOS.

mod assets;
mod cli;
mod crash;
#[cfg(test)]
mod editor_checks;
mod extension_state;
mod i18n;
mod instance;
mod keymap;
mod layout;
mod licenses;
mod mac;
mod menus;
mod motion;
mod paths;
mod project;
mod quick_pick;
mod recent;
mod session;
mod theme;
mod version;
mod windows;
mod workbench_window;

use std::path::PathBuf;
use std::process::ExitCode;

use futures::StreamExt as _;
use futures::channel::mpsc;
use gpui::{App, QuitMode};
use workbench::uri::Uri;

use crate::cli::Command;

/// When Keel started, for `KEEL_STARTUP_TIMING`.
pub static STARTED: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();

fn main() -> ExitCode {
    STARTED.get_or_init(std::time::Instant::now);
    let command = match cli::parse(std::env::args().skip(1)) {
        Ok(command) => command,
        Err(message) => {
            eprintln!("keel: {message}\n\n{}", cli::USAGE);
            return ExitCode::from(2);
        }
    };

    match command {
        Command::Help => {
            println!("{}", cli::USAGE);
            ExitCode::SUCCESS
        }
        Command::Version => {
            println!("Keel {}", version::full());
            ExitCode::SUCCESS
        }
        Command::FinishUpdate { pid, staged, app } => {
            finish_update(pid, &staged, &app);
            ExitCode::SUCCESS
        }
        Command::InstallExtension(package) => {
            match paths::install_extension(&paths::data_dir(), &package) {
                Ok(installed) => {
                    println!(
                        "Installed {} {} in {}",
                        installed.id(),
                        installed.manifest.version,
                        installed.root.display()
                    );
                    ExitCode::SUCCESS
                }
                Err(error) => {
                    eprintln!("keel: {error:#}");
                    ExitCode::FAILURE
                }
            }
        }
        Command::Open(paths_to_open) => {
            let mut paths_to_open: Vec<PathBuf> = paths_to_open
                .iter()
                .map(|path| paths::normalize(path))
                .collect();
            #[cfg(feature = "snapshots")]
            if let Some(out) = std::env::var_os("KEEL_SNAPSHOTS") {
                let mut paths_to_open = paths_to_open.into_iter();
                workbench_window::snapshots::run(
                    out.into(),
                    paths_to_open.next(),
                    paths_to_open.next(),
                );
                return ExitCode::SUCCESS;
            }
            let socket = instance::socket_path(&paths::data_dir());
            // A Keel already running opens the paths in its windows.
            let first = (!paths_to_open.is_empty()).then(|| paths_to_open.remove(0));
            if instance::forward(&socket, first.as_deref()) {
                for path in &paths_to_open {
                    instance::forward(&socket, Some(path));
                }
                return ExitCode::SUCCESS;
            }
            // `keel .` typed in a terminal opens Keel as an app of its own,
            // as `code .` does, and gives the prompt back at once.
            if let Some(app) = app_to_open_from_terminal() {
                let mut all: Vec<PathBuf> = first.into_iter().collect();
                all.extend(paths_to_open);
                return open_detached(&app, &all);
            }
            // Keel's file icons are read while the window opens.
            workbench_window::preload_keel_icons();
            run(first, paths_to_open, socket);
            ExitCode::SUCCESS
        }
    }
}

/// The app Keel runs from, when it was started from a terminal and runs
/// from inside `Keel.app`: `KEEL_FOREGROUND` keeps it in the terminal.
fn app_to_open_from_terminal() -> Option<PathBuf> {
    use std::io::IsTerminal as _;
    if !std::io::stdin().is_terminal() || std::env::var_os("KEEL_FOREGROUND").is_some() {
        return None;
    }
    let program = std::env::current_exe().ok()?.canonicalize().ok()?;
    let app = program.parent()?.parent()?.parent()?.to_path_buf();
    (app.extension().is_some_and(|extension| extension == "app")
        && app.join("Contents/Info.plist").is_file())
    .then_some(app)
}

/// Opens `app` through macOS, on `paths`, keeping the data folder of a
/// Keel tried on a folder of its own.
fn open_detached(app: &std::path::Path, paths: &[PathBuf]) -> ExitCode {
    let mut open = std::process::Command::new("/usr/bin/open");
    if let Some(data) = std::env::var_os("KEEL_DATA_DIR") {
        let mut variable = std::ffi::OsString::from("KEEL_DATA_DIR=");
        variable.push(data);
        open.arg("--env").arg(variable);
    }
    open.arg("-a").arg(app);
    if !paths.is_empty() {
        open.arg("--args").args(paths);
    }
    match open.status() {
        Ok(status) if status.success() => ExitCode::SUCCESS,
        _ => {
            eprintln!("keel: could not open {}", app.display());
            ExitCode::FAILURE
        }
    }
}

/// Run by the Keel that asked for an update, as it quits: waits for it to
/// be gone, puts the new app in place, leaves a word on how it went for the
/// next Keel, and opens it. On failure the old Keel opens again.
fn finish_update(pid: i32, staged: &std::path::Path, app: &std::path::Path) {
    let version = staged
        .join("Contents/Info.plist")
        .exists()
        .then(|| {
            std::process::Command::new("/usr/bin/plutil")
                .args(["-extract", "CFBundleShortVersionString", "raw", "-o", "-"])
                .arg(staged.join("Contents/Info.plist"))
                .output()
                .ok()
                .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_owned())
        })
        .flatten()
        .unwrap_or_default();
    let error = update::finish(pid, staged, app, std::time::Duration::from_secs(120))
        .err()
        .map(|error| format!("{error:#}"));
    let _ = update::write_outcome(&paths::data_dir(), &update::Outcome { version, error });
    let mut open = std::process::Command::new("/usr/bin/open");
    // A Keel tried on a data folder of its own opens again on the same one.
    if let Some(data) = std::env::var_os("KEEL_DATA_DIR") {
        let mut variable = std::ffi::OsString::from("KEEL_DATA_DIR=");
        variable.push(data);
        open.arg("--env").arg(variable);
    }
    let _ = open.arg(app).status();
}

fn run(path: Option<PathBuf>, more: Vec<PathBuf>, socket: PathBuf) {
    // Asked before Keel writes anything in its folder.
    let welcome = workbench_window::first_start(&paths::data_dir());
    // Before anything else, so a crash at the start is kept too.
    if let Some(unclean) = crash::start(&paths::data_dir()) {
        eprintln!(
            "keel: the last run did not end normally{}",
            unclean
                .log
                .map(|log| format!("; crash log: {}", log.display()))
                .unwrap_or_default()
        );
    }
    // Paths to open arrive from other `keel` commands and from Finder.
    let (requests, mut incoming) = mpsc::unbounded::<instance::OpenRequest>();
    let from_finder = requests.clone();
    let app = gpui_platform::application()
        .with_assets(assets::KeelAssets)
        .with_quit_mode(QuitMode::LastWindowClosed);
    app.on_open_urls(move |urls| {
        for url in urls {
            if let Some(path) = Uri::parse(&url).and_then(|uri| uri.file_path()) {
                let _ = from_finder.unbounded_send(Some(path));
            }
        }
    });
    app.on_reopen(|cx| {
        if cx.windows().is_empty() {
            windows::open(None, cx);
        }
    });
    app.run(move |cx: &mut App| {
        gpui_component::init(cx);
        theme::init(cx);
        language::register_grammars();
        let data = paths::data_dir();
        let (user_bindings, mut warnings) =
            keymap::read_user_bindings(&paths::user_keybindings_file(&data));
        warnings.extend(workbench_window::init_with_user_bindings(
            &user_bindings,
            cx,
        ));
        for warning in &warnings {
            eprintln!("keel: {warning}");
        }
        cx.on_app_quit(|cx| {
            crash::finish();
            // An update the user asked for is put in place once Keel is gone.
            workbench_window::start_pending_update(cx);
            async {}
        })
        .detach();
        recent::init(paths::recent_projects_file(&data), cx);
        windows::init(cx);
        menus::init(cx);

        // The theme and the menus follow the user's settings; each window
        // reads its project's settings too.
        let settings = settings::Settings::load(paths::user_settings_file(&data), None);
        theme::apply(&theme::ThemeChoice::from_settings(&settings), None, cx);
        theme::apply_caret(settings.string("editor.cursorBlinking"), cx);
        menus::set(
            i18n::UiLanguage::from_setting(
                settings.string("keel.language"),
                i18n::UiLanguage::system_locale().as_deref(),
            ),
            cx,
        );

        if let Err(error) = instance::listen(&socket, requests) {
            eprintln!("keel: other keel commands will start their own Keel: {error}");
        }
        cx.spawn(async move |cx| {
            while let Some(path) = incoming.next().await {
                cx.update(|cx| windows::open(path, cx));
            }
        })
        .detach();

        // The very first start opens with the welcome.
        if welcome {
            cx.set_global(workbench_window::WelcomePending);
        }
        windows::open_startup(path, cx);
        // The other paths of `keel a.ts b.ts`, each in its project's window.
        for path in more {
            windows::open(Some(path), cx);
        }
        cx.activate(true);
    });
}
