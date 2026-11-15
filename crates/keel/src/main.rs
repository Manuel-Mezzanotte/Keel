//! Keel, a fast and lightweight code editor for macOS.

mod cli;
#[cfg(test)]
mod editor_checks;
mod paths;
mod quick_pick;
mod workbench_window;

use std::process::ExitCode;

use editor::Document;
use extensions::ExtensionHost;
use gpui::{
    App, AppContext as _, Bounds, QuitMode, TitlebarOptions, WindowBounds, WindowOptions, px, size,
};
use gpui_component::{Root, Theme, ThemeMode};

use crate::cli::Command;
use crate::workbench_window::{ExtensionSetup, Workbench};

fn main() -> ExitCode {
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
            Theme::change(ThemeMode::Dark, None, cx);
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
            let setup = extension_setup();
            let opened = cx.open_window(options, move |window, cx| {
                let view = cx.new(|cx| Workbench::new(document, setup, window, cx));
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
