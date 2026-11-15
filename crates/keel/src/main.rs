//! Keel, a fast and lightweight code editor for macOS.

mod cli;
mod editor_window;

use std::process::ExitCode;

use editor::Document;
use gpui::{
    App, AppContext as _, Bounds, QuitMode, TitlebarOptions, WindowBounds, WindowOptions, px, size,
};
use gpui_component::{Root, Theme, ThemeMode};

use crate::cli::Command;
use crate::editor_window::EditorWindow;

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

fn run(document: Option<Document>) {
    gpui_platform::application()
        .with_assets(gpui_kit_assets::Assets)
        .with_quit_mode(QuitMode::LastWindowClosed)
        .run(move |cx: &mut App| {
            gpui_component::init(cx);
            Theme::change(ThemeMode::Dark, None, cx);
            language::register_grammars();
            editor_window::init(cx);

            let options = WindowOptions {
                titlebar: Some(TitlebarOptions {
                    title: Some("Keel".into()),
                    ..Default::default()
                }),
                window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                    None,
                    size(px(1100.), px(760.)),
                    cx,
                ))),
                ..Default::default()
            };
            let opened = cx.open_window(options, move |window, cx| {
                let view = cx.new(|cx| EditorWindow::new(document, window, cx));
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
