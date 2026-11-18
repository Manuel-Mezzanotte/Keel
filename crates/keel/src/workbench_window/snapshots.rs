//! Keel's window drawn off screen into PNG files, to look at the interface
//! without showing anything on the screen. Built only with the `snapshots`
//! feature; `scripts/snapshots.sh` runs it with a data folder of its own.

use std::path::PathBuf;
use std::time::Duration;

use gpui::{AnyWindowHandle, AsyncApp, Bounds, QuitMode, point, size};
use gpui_component::Root;

use super::*;

/// What one picture shows: a change to the window before it is taken.
type Prepare = Box<dyn Fn(&mut Workbench, &mut Window, &mut Context<Workbench>)>;

struct Shot {
    name: &'static str,
    prepare: Prepare,
}

fn shot(
    name: &'static str,
    prepare: impl Fn(&mut Workbench, &mut Window, &mut Context<Workbench>) + 'static,
) -> Shot {
    Shot {
        name,
        prepare: Box::new(prepare),
    }
}

fn set_theme(name: &str, window: &mut Window, cx: &mut App) {
    let choice = crate::theme::ThemeChoice {
        theme: name.to_owned(),
        follow_system: false,
        dark: "Mezzanotte".to_owned(),
        light: "Chiaro".to_owned(),
    };
    crate::theme::apply(&choice, Some(window), cx);
}

fn set_style(
    workbench: &mut Workbench,
    style: &str,
    window: &mut Window,
    cx: &mut Context<Workbench>,
) {
    workbench.change_setting(
        "keel.interfaceStyle",
        Some(Value::String(style.to_owned())),
        window,
        cx,
    );
}

fn side(view: SideView) -> impl Fn(&mut Workbench, &mut Window, &mut Context<Workbench>) {
    move |workbench, window, cx| {
        window.close_all_dialogs(cx);
        workbench.layout.show_side_view(view);
        cx.notify();
    }
}

/// The pictures, in order; each starts from the window the last one left.
fn shots() -> Vec<Shot> {
    let mut shots = Vec::new();
    for (style, theme) in [
        ("floating", "Mezzanotte"),
        ("flat", "Mezzanotte"),
        ("floating", "Chiaro"),
        ("flat", "Chiaro"),
    ] {
        let tag = |view: &str| -> &'static str {
            Box::leak(
                format!("{style}-{theme}-{view}")
                    .to_lowercase()
                    .into_boxed_str(),
            )
        };
        shots.push(shot(tag("explorer"), move |workbench, window, cx| {
            window.close_all_dialogs(cx);
            set_theme(theme, window, cx);
            set_style(workbench, style, window, cx);
            workbench.layout.show_side_view(SideView::Explorer);
            workbench.layout.agent = true;
            workbench.layout.panel = false;
            cx.notify();
        }));
        shots.push(shot(tag("search"), side(SideView::Search)));
        shots.push(shot(tag("scm"), side(SideView::SourceControl)));
        shots.push(shot(tag("extensions"), side(SideView::Extensions)));
        shots.push(shot(tag("panel"), |workbench, _, cx| {
            workbench.layout.show_side_view(SideView::Explorer);
            workbench.layout.agent = false;
            workbench.layout.show_panel_view(PanelView::Problems);
            cx.notify();
        }));
        shots.push(shot(tag("quick-open"), |workbench, window, cx| {
            workbench.layout.panel = false;
            workbench.layout.agent = true;
            workbench.quick_open(window, cx);
        }));
        shots.push(shot(tag("settings"), |workbench, window, cx| {
            window.close_all_dialogs(cx);
            workbench.open_page(settings_view::PageKind::Settings, window, cx);
        }));
        shots.push(shot(tag("menu"), |workbench, window, cx| {
            // A right click on the first tab opens its menu.
            let look = workbench.look;
            let position = point(
                px(look.gap() * 2. + workbench.sidebar_width + 44.),
                px(look.toolbar_height() + look.gap() + look.bar_height() / 2.),
            );
            window.dispatch_event(
                gpui::PlatformInput::MouseDown(gpui::MouseDownEvent {
                    button: gpui::MouseButton::Right,
                    position,
                    modifiers: Default::default(),
                    click_count: 1,
                    first_mouse: false,
                }),
                cx,
            );
            window.dispatch_event(
                gpui::PlatformInput::MouseUp(gpui::MouseUpEvent {
                    button: gpui::MouseButton::Right,
                    position,
                    modifiers: Default::default(),
                    click_count: 1,
                }),
                cx,
            );
        }));
    }
    // The editors' other pages and the panel's terminal, in one style each.
    for (style, theme) in [("floating", "Mezzanotte"), ("flat", "Chiaro")] {
        let name = |what: &str| -> &'static str {
            Box::leak(
                format!("{style}-{theme}-{what}")
                    .to_lowercase()
                    .into_boxed_str(),
            )
        };
        shots.push(shot(name("find"), move |workbench, window, cx| {
            window.close_all_dialogs(cx);
            set_theme(theme, window, cx);
            set_style(workbench, style, window, cx);
            // Back to the code, with the find bar open on it.
            let code = workbench
                .groups
                .first()
                .and_then(|group| group.tabs.iter().position(|tab| tab.uri.ends_with(".rs")));
            if let Some(index) = code {
                workbench.activate_tab(0, index, true, window, cx);
            }
            workbench.open_find(true, window, cx);
        }));
        shots.push(shot(name("terminal"), |workbench, window, cx| {
            workbench.show_terminal(window, cx);
        }));
        shots.push(shot(name("graph"), |workbench, window, cx| {
            workbench.layout.panel = false;
            workbench.open_git_graph(window, cx);
        }));
        shots.push(shot(name("diff"), |workbench, window, cx| {
            let changed = workbench
                .scm
                .status
                .files
                .iter()
                .find(|file| file.is_changed())
                .map(|file| file.path.clone());
            if let Some(path) = changed {
                workbench.open_change(&path, source_control::Section::Changes, window, cx);
            }
        }));
    }
    // Last, with every tab closed: the welcome page.
    for (style, theme) in [("floating", "Mezzanotte"), ("flat", "Chiaro")] {
        let name: &'static str = Box::leak(
            format!("{style}-{theme}-welcome")
                .to_lowercase()
                .into_boxed_str(),
        );
        shots.push(shot(name, move |workbench, window, cx| {
            set_theme(theme, window, cx);
            set_style(workbench, style, window, cx);
            workbench.layout.agent = false;
            for _ in 0..64 {
                let Some(group) = workbench
                    .groups
                    .iter()
                    .rposition(|group| !group.tabs.is_empty())
                else {
                    break;
                };
                let last = workbench.groups[group].tabs.len() - 1;
                workbench.close_tab(group, last, window, cx);
            }
        }));
    }
    shots
}

/// Draws a frame, so that what changed starts moving.
fn draw(handle: AnyWindowHandle, cx: &mut AsyncApp) {
    let _ = cx.update_window(handle, |_, window, cx| {
        window.refresh();
        window.draw(cx).clear(cx);
    });
}

/// Draws the window now and writes what it shows to `file`.
fn take(handle: AnyWindowHandle, file: PathBuf, cx: &mut AsyncApp) {
    let result = cx.update_window(handle, |_, window, cx| {
        // Twice: lists measure themselves on the first frame.
        for _ in 0..2 {
            window.refresh();
            window.draw(cx).clear(cx);
        }
        window.render_to_image()
    });
    match result {
        Ok(Ok(image)) => match image.save(&file) {
            Ok(()) => println!("{}", file.display()),
            Err(error) => eprintln!("keel: could not write {}: {error}", file.display()),
        },
        Ok(Err(error)) => eprintln!("keel: could not draw {}: {error:#}", file.display()),
        Err(error) => eprintln!("keel: the window is gone: {error:#}"),
    }
}

/// Opens `project` (and `file`) in a window nobody sees, takes the
/// pictures into `out`, and quits.
pub(crate) fn run(out: PathBuf, project: Option<PathBuf>, file: Option<PathBuf>) {
    let _ = std::fs::create_dir_all(&out);
    let app = gpui_platform::application()
        .with_assets(crate::assets::KeelAssets)
        .with_quit_mode(QuitMode::Explicit);
    app.run(move |cx: &mut App| {
        gpui_component::init(cx);
        crate::theme::init(cx);
        language::register_grammars();
        for warning in init_with_user_bindings(&[], cx) {
            eprintln!("keel: {warning}");
        }
        let data = paths::data_dir();
        crate::recent::init(paths::recent_projects_file(&data), cx);
        let settings = Settings::load(paths::user_settings_file(&data), project.as_deref());
        crate::theme::apply(
            &crate::theme::ThemeChoice::from_settings(&settings),
            None,
            cx,
        );
        crate::menus::set(
            UiLanguage::from_setting(settings.string("keel.language"), None),
            cx,
        );
        let setup = crate::windows::extension_setup(&data);

        let bounds = Bounds::new(point(px(-10000.), px(-10000.)), size(px(1440.), px(900.)));
        let look = super::look::Look::from_settings(&settings);
        let mut options = window_options(bounds, look);
        options.show = false;
        options.focus = false;
        let opening = Opening::new(project.clone(), file.clone()).with_data_dir(data);
        let created: Rc<RefCell<Option<Entity<Workbench>>>> = Rc::default();
        let opened = cx.open_window(options, {
            let created = created.clone();
            move |window, cx| {
                let view = cx.new(|cx| Workbench::new(opening, settings, setup, window, cx));
                *created.borrow_mut() = Some(view.clone());
                cx.new(|cx| Root::new(view, window, cx))
            }
        });
        let (Ok(handle), Some(workbench)) = (opened, created.take()) else {
            eprintln!("keel: could not open the window");
            cx.quit();
            return;
        };
        let handle: AnyWindowHandle = handle.into();
        cx.spawn(async move |cx| {
            // The tree, git and the extensions arrive in the background.
            cx.background_executor()
                .timer(Duration::from_millis(2500))
                .await;
            // `KEEL_SNAPSHOTS_ONLY=scm,search` takes only the pictures whose
            // names hold one of the words; the others still set the window up.
            let only: Vec<String> = std::env::var("KEEL_SNAPSHOTS_ONLY")
                .unwrap_or_default()
                .split(',')
                .filter(|word| !word.is_empty())
                .map(str::to_owned)
                .collect();
            for shot in shots() {
                // Esc first, outside the window's own update: it closes a
                // menu the last picture left open.
                let _ = cx.update_window(handle, |_, window, cx| {
                    window.dispatch_keystroke(gpui::Keystroke::parse("escape").unwrap(), cx);
                });
                let _ = cx.update_window(handle, |_, window, cx| {
                    workbench.update(cx, |workbench, cx| (shot.prepare)(workbench, window, cx))
                });
                if !only.is_empty() && !only.iter().any(|word| shot.name.contains(word.as_str())) {
                    draw(handle, cx);
                    continue;
                }
                // Animations start on the first frame drawn: let them end.
                for _ in 0..6 {
                    draw(handle, cx);
                    cx.background_executor()
                        .timer(Duration::from_millis(120))
                        .await;
                }
                take(handle, out.join(format!("{}.png", shot.name)), cx);
            }
            cx.update(|cx| cx.quit());
        })
        .detach();
    });
}
