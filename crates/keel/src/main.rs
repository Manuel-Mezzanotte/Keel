//! Keel, a fast and lightweight code editor for macOS.

mod cli;
#[cfg(test)]
mod editor_checks;
mod i18n;
mod paths;
mod quick_pick;
mod theme;
mod workbench_window;

use std::process::ExitCode;

use editor::Document;
use extensions::ExtensionHost;
use gpui::{
    App, AppContext as _, Bounds, QuitMode, TitlebarOptions, WindowBounds, WindowOptions, px, size,
};
use gpui_component::Root;

use crate::cli::Command;
use crate::workbench_window::{ExtensionSetup, Workbench};

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
            println!("Keel {}", env!("CARGO_PKG_VERSION"));
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
        Command::Open(path) => {
            let document = match path.map(Document::open).transpose() {
                Ok(document) => document,
                Err(error) => {
                    eprintln!("keel: {error}");
                    return ExitCode::FAILURE;
                }
            };
            run(document);
            ExitCode::SUCCESS
        }
    }
}

/// Starts the extension host with the installed extensions. Keel still runs
/// without extensions if the host cannot start.
fn extension_setup() -> Option<ExtensionSetup> {
    let data_dir = paths::data_dir();
    let installed = paths::installed_extensions(&data_dir);
    match ExtensionHost::start() {
        Ok((host, events)) => Some(ExtensionSetup {
            host,
            events,
            installed,
            data_dir,
        }),
        Err(error) => {
            eprintln!("keel: extensions are off, the extension host did not start: {error:#}");
            None
        }
    }
}

fn run(document: Option<Document>) {
    gpui_platform::application()
        .with_assets(gpui_kit_assets::Assets)
        .with_quit_mode(QuitMode::LastWindowClosed)
        .run(move |cx: &mut App| {
            gpui_component::init(cx);
            theme::init(cx);
            language::register_grammars();
            workbench_window::init(cx);

            let options = WindowOptions {
                titlebar: Some(TitlebarOptions {
                    title: Some("Keel".into()),
                    ..Default::default()
                }),
                window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                    None,
                    size(px(1200.), px(780.)),
                    cx,
                ))),
                ..Default::default()
            };
            let project = document
                .as_ref()
                .and_then(|document| std::path::absolute(document.path()).ok())
                .and_then(|path| settings::project_root_for(&path));
            let settings = settings::Settings::load(
                paths::user_settings_file(&paths::data_dir()),
                project.as_deref(),
            );
            for warning in &settings.warnings {
                eprintln!("keel: {warning}");
            }
            theme::apply(&theme::ThemeChoice::from_settings(&settings), None, cx);
            let setup = extension_setup();
            let opened = cx.open_window(options, move |window, cx| {
                let view = cx.new(|cx| Workbench::new(document, settings, setup, window, cx));
                cx.new(|cx| Root::new(view, window, cx))
            });
            if let Err(error) = opened {
                eprintln!("keel: could not open the window: {error}");
                cx.quit();
                return;
            }
            cx.activate(true);
        });
}
