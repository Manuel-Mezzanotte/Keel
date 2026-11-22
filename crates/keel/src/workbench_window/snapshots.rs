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
        if style == "floating" {
            // Commands with their dots: one fails, with its fix and Claude.
            shots.push(shot(name("terminal-failed"), |workbench, _, cx| {
                let Some(view) = workbench.active_terminal_view().cloned() else {
                    return;
                };
                // After the Esc of the picture before has gone by, so zsh
                // does not read it as Meta.
                cx.spawn(async move |_, cx| {
                    cx.background_executor()
                        .timer(Duration::from_millis(450))
                        .await;
                    view.update(cx, |view, _| {
                        view.run("echo pronto; echo 'Error: listen EADDRINUSE: address already in use :::3000'; false")
                    });
                })
                .detach();
            }));
            shots.push(shot(name("terminal-menu"), |workbench, _, cx| {
                if let Some(view) = workbench.active_terminal_view().cloned() {
                    view.update(cx, |view, cx| view.open_last_dot_menu(cx));
                }
            }));
            // A long output scrolled into: its command stays at the top.
            shots.push(shot(name("terminal-sticky"), |workbench, _, cx| {
                let Some(view) = workbench.active_terminal_view().cloned() else {
                    return;
                };
                cx.spawn(async move |_, cx| {
                    cx.background_executor()
                        .timer(Duration::from_millis(450))
                        .await;
                    view.update(cx, |view, _| view.run("seq 1 200"));
                    cx.background_executor()
                        .timer(Duration::from_millis(200))
                        .await;
                    view.update(cx, |view, cx| view.scroll_for_snapshot(60, cx));
                })
                .detach();
            }));
        }
        if style == "floating" {
            // Cmd+I over some lines of the code, a request typed.
            shots.push(shot(name("inline-edit"), |workbench, window, cx| {
                workbench.layout.panel = false;
                let code = workbench
                    .groups
                    .first()
                    .and_then(|group| group.tabs.iter().position(|tab| tab.uri.ends_with(".rs")));
                if let Some(index) = code {
                    workbench.activate_tab(0, index, true, window, cx);
                }
                if let Some(editor) = workbench.active_editor() {
                    editor.update(cx, |editor, cx| editor.set_selected_range(380..560, cx));
                }
                workbench.start_inline_edit(window, cx);
                if let Some(edit) = &workbench.inline_edit {
                    edit.input.update(cx, |input, cx| {
                        input.set_value("Aggiungi un commento a ogni variante", window, cx)
                    });
                }
            }));
        }
        if style == "floating" {
            // The search's results in a tab, to edit.
            shots.push(shot(name("results-tab"), |workbench, window, cx| {
                let query = workbench.search.query.clone();
                query.update(cx, |query, cx| {
                    query.set_value("fn excerpts_of", window, cx)
                });
                workbench.run_search(cx);
                cx.spawn_in(window, async move |this, cx| {
                    cx.background_executor()
                        .timer(Duration::from_millis(450))
                        .await;
                    this.update_in(cx, |this, window, cx| this.open_search_results(window, cx))
                        .ok();
                })
                .detach();
            }));
            // The blame's card: the whole commit of the line.
            shots.push(shot(name("blame-card"), |workbench, window, cx| {
                let folder = std::env::current_dir().unwrap_or_default();
                workbench.open_file(&folder.join("crates/editor/src/encoding.rs"), window, cx);
                workbench.go_to(30, 1, window, cx);
                workbench.toggle_blame(cx);
                cx.spawn_in(window, async move |this, cx| {
                    cx.background_executor()
                        .timer(Duration::from_millis(300))
                        .await;
                    this.update_in(cx, |this, window, cx| {
                        let Some(hash) = this.current_line_hash(cx) else {
                            return;
                        };
                        let anchor = gpui::point(
                            px(420.),
                            window.viewport_size().height - px(this.look.status_height()),
                        );
                        this.open_blame_card(hash, anchor, gpui::Anchor::BottomLeft, cx);
                    })
                    .ok();
                })
                .detach();
            }));
            // The file given stopped at a line, for real: KEEL_DEBUG_LINE
            // (from 1) and KEEL_DEBUG_NAME, the configuration.
            shots.push(shot(name("debug-live"), |workbench, window, cx| {
                workbench.close_blame_card(cx);
                let Some(path) = workbench
                    .groups
                    .first()
                    .and_then(|group| group.tabs.first())
                    .and_then(|tab| workbench.documents.get(&tab.uri))
                    .and_then(|document| document.disk_path().map(Path::to_path_buf))
                else {
                    return;
                };
                let Some(line) = std::env::var("KEEL_DEBUG_LINE")
                    .ok()
                    .and_then(|line| line.parse::<u32>().ok())
                else {
                    return;
                };
                workbench.activate_tab(0, 0, true, window, cx);
                workbench.toggle_breakpoint(&path, line - 1, cx);
                workbench.add_watch("valori.len()".to_owned(), cx);
                workbench.layout.show_side_view(SideView::Debug);
                workbench.layout.show_panel_view(PanelView::DebugConsole);
                let name = std::env::var("KEEL_DEBUG_NAME").unwrap_or_default();
                workbench.start_named(&name, false, window, cx);
            }));
            // The project's build, for real, with its problems: needs
            // KEEL_SNAPSHOTS_WAIT.
            shots.push(shot(name("build-live"), |workbench, window, cx| {
                workbench.close_blame_card(cx);
                workbench.run_task("cargo build", window, cx);
            }));
            shots.push(shot(name("build-problems-live"), |workbench, _, cx| {
                workbench.layout.show_panel_view(PanelView::Problems);
                cx.notify();
            }));
            // The right column of the changes, edited: on the file given,
            // in a repository where it has changed.
            shots.push(shot(name("diff-edit-live"), |workbench, window, cx| {
                workbench.close_blame_card(cx);
                let Some(path) = workbench
                    .groups
                    .first()
                    .and_then(|group| group.tabs.first())
                    .and_then(|tab| workbench.documents.get(&tab.uri))
                    .and_then(|document| document.disk_path().map(Path::to_path_buf))
                else {
                    return;
                };
                let Some(relative) = workbench
                    .scm
                    .repo
                    .as_ref()
                    .and_then(|repo| repo.relative(&path))
                else {
                    return;
                };
                workbench.open_diff(&relative, super::diff_view::DiffSource::Changes, window, cx);
                let uri =
                    super::diff_view::diff_uri(&relative, &super::diff_view::DiffSource::Changes);
                workbench.edit_diff_lines(&uri, 0, None, Some(0), window, cx);
            }));
            // The real server's lenses and types, on the file given:
            // with KEEL_SNAPSHOTS_WAIT and the servers on.
            shots.push(shot(name("line-hints-live"), |workbench, window, cx| {
                workbench.close_blame_card(cx);
                workbench.change_setting(
                    "editor.inlayHints.enabled",
                    Some(serde_json::json!("on")),
                    window,
                    cx,
                );
                workbench.focus_group(0, window, cx);
                workbench.activate_tab(0, 0, true, window, cx);
            }));
            // Rulers at 80 and 100 columns, the second in its own color.
            shots.push(shot(name("rulers"), |workbench, window, cx| {
                workbench.close_blame_card(cx);
                let value = serde_json::json!([80, { "column": 100, "color": "#e0707060" }]);
                workbench.change_setting("editor.rulers", Some(value), window, cx);
                let folder = std::env::current_dir().unwrap_or_default();
                workbench.open_file(
                    &folder.join("crates/keel/src/workbench_window/tabs.rs"),
                    window,
                    cx,
                );
                workbench.go_to(800, 1, window, cx);
            }));
            // Groups side by side and stacked.
            shots.push(shot(name("groups-grid"), |workbench, window, cx| {
                workbench.change_setting("editor.rulers", None, window, cx);
                let folder = std::env::current_dir().unwrap_or_default();
                workbench.open_file(&folder.join("crates/editor/src/encoding.rs"), window, cx);
                let group = workbench.active;
                workbench.split_editor(group, super::tabs::Side::Right, window, cx);
                workbench.open_file(&folder.join("crates/git/src/lib.rs"), window, cx);
                let group = workbench.active;
                workbench.split_editor(group, super::tabs::Side::Below, window, cx);
                workbench.open_file(&folder.join("Cargo.toml"), window, cx);
            }));
            // A file with tests: the ▶ in its margin.
            shots.push(shot(name("tests-margin"), |workbench, window, cx| {
                workbench.close_inline_edit(false, window, cx);
                let file = std::env::current_dir()
                    .unwrap_or_default()
                    .join("crates/editor/src/text_edits.rs");
                workbench.open_file(&file, window, cx);
                workbench.go_to(660, 1, window, cx);
            }));
        }
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
        shots.push(shot(name("diff-inline"), |workbench, _, cx| {
            workbench.set_diff_split(false, cx);
        }));
        shots.push(shot(name("diff-wide"), |workbench, _, cx| {
            workbench.set_diff_split(true, cx);
            workbench.layout.agent = false;
            cx.notify();
        }));
        shots.push(shot(name("all-changes"), |workbench, window, cx| {
            workbench.set_diff_split(true, cx);
            workbench.open_diff("", super::diff_view::DiffSource::AllChanges, window, cx);
        }));
        shots.push(shot(name("notices"), |workbench, window, cx| {
            workbench.notify_pulled((2, 5), window, cx);
            workbench.notify_pushed(
                super::git_feedback::Pushed {
                    commits: 3,
                    published: false,
                    branch: Some("develop".to_owned()),
                    upstream: Some("origin/develop".to_owned()),
                },
                window,
                cx,
            );
        }));
        shots.push(shot(name("notice-center"), |workbench, window, cx| {
            workbench.toggle_notice_center(window, cx);
        }));
        shots.push(shot(name("after-notices"), |workbench, window, cx| {
            workbench.notices.open = false;
            window.clear_notifications(cx);
            cx.notify();
        }));
    }
    // The Claude panel: its start with the project's real chats, read
    // from disk, then a chat with the tests' fake agent, so nothing is
    // sent to Claude.
    for (round, (style, theme)) in [("floating", "Mezzanotte"), ("flat", "Chiaro")]
        .into_iter()
        .enumerate()
    {
        let name = |what: &str| -> &'static str {
            Box::leak(
                format!("{style}-{theme}-{what}")
                    .to_lowercase()
                    .into_boxed_str(),
            )
        };
        shots.push(shot(name("claude-home"), move |workbench, window, cx| {
            window.close_all_dialogs(cx);
            set_theme(theme, window, cx);
            set_style(workbench, style, window, cx);
            workbench.layout.panel = false;
            workbench.layout.agent = true;
            workbench.agent_panel.view = agent_panel::AgentView::Agent;
            workbench.agent_panel.show_chats = false;
            // The last round's edit is still asked: it ends here.
            workbench.cancel_agent(cx);
            workbench.agent_panel.reset_thread();
            workbench.agent_panel.session = None;
            cx.notify();
        }));
        shots.push(shot(name("claude-chats"), |workbench, _, cx| {
            workbench.agent_panel.show_chats = true;
            cx.notify();
        }));
        shots.push(shot(name("claude-chat"), move |workbench, window, cx| {
            workbench.agent_panel.show_chats = false;
            if round == 0 {
                workbench.agent_panel.fake = true;
                workbench.open_agent(window, cx);
            } else {
                workbench.new_agent_conversation(window, cx);
            }
            workbench.agent_panel.input.update(cx, |input, cx| {
                input.set_value("Come è organizzato il pannello di destra?", window, cx)
            });
            workbench.send_to_agent(window, cx);
        }));
        shots.push(shot(name("claude-tools"), |workbench, window, cx| {
            // A file of the snapshots' own data folder: the edit is only
            // asked, never allowed.
            let file = paths::data_dir().join("prezzi.ts");
            let _ = std::fs::write(&file, "const price = 90;\n");
            workbench.agent_panel.input.update(cx, |input, cx| {
                input.set_value(format!("edit {} 90 95", file.display()), window, cx)
            });
            workbench.send_to_agent(window, cx);
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
                // KEEL_SNAPSHOTS_WAIT: milliseconds more, for a language
                // server to read a small project.
                if let Some(wait) = std::env::var("KEEL_SNAPSHOTS_WAIT")
                    .ok()
                    .and_then(|wait| wait.parse::<u64>().ok())
                {
                    cx.background_executor()
                        .timer(Duration::from_millis(wait))
                        .await;
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
