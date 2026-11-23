//! Keel's updates in the window: the check at start, if the Settings
//! allow it, and the one asked from the Settings or the menu; the window
//! that offers a new version, its download, and the restart that puts it
//! in place. The work itself is in the `update` crate.

use std::path::PathBuf;
use std::time::Duration;

use gpui::{Global, relative};
use gpui_component::WindowExt as _;
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::{Disableable as _, h_flex, v_flex};

use super::notices::Notice;
use super::*;

/// The setting that lets Keel look for a new version at start.
pub(super) const AUTOMATIC_SETTING: &str = "update.automatic";

/// Where updates come from and the app they replace.
#[derive(Clone)]
pub(super) struct Source {
    /// The list of releases.
    pub(super) feed: String,
    /// The app Keel runs from.
    pub(super) app: PathBuf,
    /// The key releases are checked with, as hex; `None` is Keel's own.
    pub(super) key: Option<String>,
    /// Whether Keel quits and opens again once the new version is ready.
    pub(super) restart: bool,
}

/// What every window of this Keel shares about updates.
#[derive(Default)]
struct Updates {
    /// Found once: `None` inside means Keel does not run from an app.
    source: Option<Option<Source>>,
    /// Whether the first window did what is done at start.
    started: bool,
    /// A new version ready to go in place when Keel quits: the app
    /// unpacked next to the old one, and the old one.
    pending: Option<(PathBuf, PathBuf)>,
}

impl Global for Updates {}

/// Where updates come from: Keel's site, for the app Keel runs from. A
/// Keel started outside an app, as with `cargo run`, is not updated.
fn source(cx: &mut App) -> Option<Source> {
    cx.default_global::<Updates>()
        .source
        .get_or_insert_with(|| {
            let executable = std::env::current_exe().ok()?;
            let app = update::running_app(&executable)?;
            Some(Source {
                feed: update::feed_url(),
                app,
                key: None,
                restart: true,
            })
        })
        .clone()
}

#[cfg(test)]
pub(super) fn set_source(source: Option<Source>, cx: &mut App) {
    cx.default_global::<Updates>().source = Some(source);
}

/// Run as Keel quits: with a new version ready, starts the process that
/// puts it in place and opens Keel again.
pub fn start_pending_update(cx: &mut App) {
    let Some((staged, app)) = cx
        .try_global::<Updates>()
        .and_then(|updates| updates.pending.clone())
    else {
        return;
    };
    if let Ok(executable) = std::env::current_exe() {
        let _ = update::start_finisher(&executable, &staged, &app);
    }
}

/// A new version offered in the window.
#[derive(Clone)]
pub(super) struct Offer {
    pub(super) release: update::Release,
    pub(super) phase: Phase,
}

#[derive(Clone, Debug, PartialEq)]
pub(super) enum Phase {
    /// Offered, waiting for a click.
    Ready,
    /// Coming down: the bytes so far.
    Downloading { received: u64 },
    /// Checked and unpacked: Keel is about to restart.
    Installing,
    /// Something went wrong, and why.
    Failed(String),
}

impl Workbench {
    /// Once the first window of this Keel is on screen: says how the last
    /// update went, and looks for a new version if the Settings allow it.
    pub(super) fn updates_at_start(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let updates = cx.default_global::<Updates>();
        if updates.started {
            return;
        }
        updates.started = true;
        if let Some(outcome) = self.data_dir.as_deref().and_then(update::take_outcome) {
            let strings = self.strings();
            let notice = match outcome.error {
                None => Notice::success(strings.update_done(&outcome.version)),
                Some(error) => Notice::error(strings.update_undone(&outcome.version))
                    .message(format!("{} {error}", strings.update_undone_detail)),
            };
            self.notify_user(notice, window, cx);
        }
        let automatic = self
            .settings
            .get(AUTOMATIC_SETTING)
            .and_then(Value::as_bool)
            .unwrap_or(true);
        if automatic {
            self.check_for_updates(false, window, cx);
        }
    }

    /// Looks for a new version. Asked by hand (`manual`), it also says
    /// when there is none, or when the list could not be read; at start
    /// it stays quiet unless there is something to offer.
    pub(super) fn check_for_updates(
        &mut self,
        manual: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let strings = self.strings();
        let Some(source) = source(cx) else {
            if manual {
                let notice = Notice::info(strings.update_not_installed)
                    .message(strings.update_not_installed_detail);
                self.notify_user(notice, window, cx);
            }
            return;
        };
        // A version already offered is offered again.
        if manual && self.update_offer.is_some() {
            self.open_update_dialog(window, cx);
            return;
        }
        if self.checking_updates {
            return;
        }
        self.checking_updates = true;
        cx.notify();
        let feed = source.feed.clone();
        let task = cx.background_spawn(async move {
            let system = update::system_version().unwrap_or_else(|| "99".to_owned());
            update::fetch(&feed)
                .map(|feed| update::newest(&feed, crate::version::BUILD, &system).cloned())
        });
        cx.spawn_in(window, async move |this, cx| {
            let found = task.await;
            this.update_in(cx, |this, window, cx| {
                this.checking_updates = false;
                cx.notify();
                match found {
                    Ok(Some(release)) => this.offer_update(release, window, cx),
                    Ok(None) if manual => {
                        let notice = Notice::success(strings.update_latest)
                            .message(strings.update_latest_detail(crate::version::VERSION));
                        this.notify_user(notice, window, cx);
                    }
                    Ok(None) => {}
                    Err(error) if manual => {
                        let notice = Notice::error(strings.update_check_failed)
                            .message(format!("{error:#}"));
                        this.notify_user(notice, window, cx);
                    }
                    Err(error) => this.log(format!("{}: {error:#}", strings.update_check_failed)),
                }
            })
            .ok();
        })
        .detach();
    }

    fn offer_update(
        &mut self,
        release: update::Release,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.update_offer = Some(Offer {
            release,
            phase: Phase::Ready,
        });
        self.open_update_dialog(window, cx);
    }

    /// The window over Keel that offers the new version, drawn afresh from
    /// [`Self::update_offer`] as the download goes on.
    fn open_update_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if window.has_active_dialog(cx) {
            window.close_dialog(cx);
        }
        let workbench = cx.weak_entity();
        window.open_dialog(cx, move |dialog, _, cx| {
            let Some(view) = workbench.upgrade() else {
                return dialog;
            };
            let this = view.read(cx);
            let Some(offer) = this.update_offer.clone() else {
                return dialog;
            };
            let strings = this.strings();
            let italian = this.language == UiLanguage::Italian;
            let theme = cx.theme();
            let (muted, danger, accent, track) = (
                theme.muted_foreground,
                theme.danger,
                theme.primary,
                theme.border,
            );
            let busy = matches!(offer.phase, Phase::Downloading { .. } | Phase::Installing);
            let notes = if italian {
                &offer.release.notes.it
            } else {
                &offer.release.notes.en
            };

            let mut body = v_flex().gap_3().text_sm().child(
                div()
                    .text_color(muted)
                    .child(strings.update_current(crate::version::VERSION)),
            );
            if !notes.trim().is_empty() {
                body = body.child(
                    v_flex()
                        .gap_1()
                        .child(
                            div()
                                .font_weight(FontWeight::SEMIBOLD)
                                .child(strings.update_whats_new),
                        )
                        .children(notes.lines().map(|line| div().child(line.to_owned()))),
                );
            }
            match &offer.phase {
                Phase::Ready => {}
                Phase::Downloading { received } => {
                    let total = offer.release.size.max(1);
                    let done = (*received as f32 / total as f32).clamp(0., 1.);
                    body =
                        body.child(
                            v_flex()
                                .gap_1()
                                .child(div().h(px(6.)).w_full().rounded_full().bg(track).child(
                                    div().h_full().w(relative(done)).rounded_full().bg(accent),
                                ))
                                .child(
                                    div()
                                        .text_color(muted)
                                        .child(strings.update_downloading(*received, total)),
                                ),
                        );
                }
                Phase::Installing => {
                    body = body.child(div().text_color(muted).child(strings.update_restarting));
                }
                Phase::Failed(error) => {
                    body = body.child(
                        div()
                            .text_color(danger)
                            .child(format!("{}: {error}", strings.update_failed)),
                    );
                }
            }

            let later = {
                let workbench = workbench.clone();
                Button::new("update-later")
                    .label(if matches!(offer.phase, Phase::Failed(_)) {
                        strings.update_close
                    } else {
                        strings.update_later
                    })
                    .disabled(busy)
                    .on_click(move |_, window, cx| {
                        window.close_dialog(cx);
                        workbench
                            .update(cx, |this, cx| {
                                this.update_offer = None;
                                cx.notify();
                            })
                            .ok();
                    })
            };
            let install = {
                let workbench = workbench.clone();
                Button::new("update-install")
                    .primary()
                    .label(if matches!(offer.phase, Phase::Failed(_)) {
                        strings.update_retry
                    } else {
                        strings.update_install
                    })
                    .disabled(busy)
                    .on_click(move |_, window, cx| {
                        workbench
                            .update(cx, |this, cx| this.install_update(window, cx))
                            .ok();
                    })
            };
            dialog
                .w(px(440.))
                .title(strings.update_available(&offer.release.version))
                .close_button(!busy)
                .overlay_closable(!busy)
                .keyboard(!busy)
                .child(body)
                .footer(
                    h_flex()
                        .w_full()
                        .justify_end()
                        .gap_2()
                        .child(later)
                        .child(install),
                )
        });
    }

    /// Downloads the offered version, checks it and unpacks it next to
    /// the app; then Keel quits, keeping everything, and opens again new.
    pub(super) fn install_update(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let strings = self.strings();
        let Some(source) = source(cx) else {
            return;
        };
        let Some(offer) = self.update_offer.as_mut() else {
            return;
        };
        if matches!(offer.phase, Phase::Downloading { .. } | Phase::Installing) {
            return;
        }
        let release = offer.release.clone();
        let fail = |this: &mut Self, error: String, window: &mut Window, cx: &mut Context<Self>| {
            if let Some(offer) = this.update_offer.as_mut() {
                offer.phase = Phase::Failed(error);
            }
            cx.notify();
            window.refresh();
        };
        if update::can_replace(&source.app).is_err() {
            let folder = source
                .app
                .parent()
                .map(|folder| folder.display().to_string())
                .unwrap_or_default();
            fail(self, strings.update_cannot_write(&folder), window, cx);
            return;
        }
        let key = match source.key.as_deref() {
            Some(key) => update::parse_key(key),
            None => update::public_key(),
        };
        let key = match key {
            Ok(key) => key,
            Err(error) => {
                fail(self, format!("{error:#}"), window, cx);
                return;
            }
        };
        offer.phase = Phase::Downloading { received: 0 };
        cx.notify();
        window.refresh();

        let staging = update::staging_dir(&source.app);
        // How far the download is, from the size of the file it grows into.
        let partial = update::partial_file(&staging, &release);
        cx.spawn_in(window, async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(150))
                    .await;
                let received = std::fs::metadata(&partial)
                    .map(|metadata| metadata.len())
                    .unwrap_or(0);
                let going = this
                    .update_in(cx, |this, window, cx| match this.update_offer.as_mut() {
                        Some(Offer {
                            phase: Phase::Downloading { received: shown },
                            ..
                        }) => {
                            *shown = received;
                            cx.notify();
                            window.refresh();
                            true
                        }
                        _ => false,
                    })
                    .unwrap_or(false);
                if !going {
                    break;
                }
            }
        })
        .detach();

        let work = cx.background_spawn(async move {
            let zip = update::download(&release, &staging, &key)?;
            let app = update::unpack(&zip, &staging, &release)?;
            let _ = std::fs::remove_file(&zip);
            anyhow::Ok(app)
        });
        cx.spawn_in(window, async move |this, cx| {
            let ready = work.await;
            this.update_in(cx, |this, window, cx| match ready {
                Ok(staged) => {
                    if let Some(offer) = this.update_offer.as_mut() {
                        offer.phase = Phase::Installing;
                    }
                    cx.default_global::<Updates>().pending = Some((staged, source.app.clone()));
                    cx.notify();
                    window.refresh();
                    if source.restart {
                        // Quits as ⌘Q does, keeping the tabs and the unsaved
                        // files; the new Keel opens with them.
                        window.dispatch_action(Box::new(Quit), cx);
                    }
                }
                Err(error) => fail(this, format!("{error:#}"), window, cx),
            })
            .ok();
        })
        .detach();
    }

    /// The new version waiting to go in place, for the tests.
    #[cfg(test)]
    fn pending_update(cx: &App) -> Option<(PathBuf, PathBuf)> {
        cx.try_global::<Updates>()
            .and_then(|updates| updates.pending.clone())
    }
}

#[cfg(test)]
mod tests {
    use gpui::{TestAppContext, VisualTestContext};

    use super::*;

    /// A window with Keel's root, so dialogs and notices show; its data in
    /// a folder of its own.
    fn window<'a>(
        cx: &'a mut TestAppContext,
        settings: &str,
    ) -> (
        tempfile::TempDir,
        Entity<Workbench>,
        &'a mut VisualTestContext,
    ) {
        cx.update(|cx| {
            gpui_component::init(cx);
            language::register_grammars();
            crate::theme::init(cx);
            init(cx);
        });
        let folder = tempfile::tempdir().unwrap();
        let settings_file = folder.path().join("settings.json");
        std::fs::write(&settings_file, settings).unwrap();
        let path = folder.path().join("notes.md");
        std::fs::write(&path, "# Notes\n").unwrap();
        let document = Document::open(&path).unwrap();
        let mut workbench = None;
        let (_root, cx) = cx.add_window_view(|window, cx| {
            let settings = Settings::load(&settings_file, None);
            let view = cx.new(|cx| Workbench::new(Some(document), settings, None, window, cx));
            workbench = Some(view.clone());
            gpui_component::Root::new(view, window, cx)
        });
        (folder, workbench.unwrap(), cx)
    }

    fn titles(view: &Entity<Workbench>, cx: &mut VisualTestContext) -> Vec<String> {
        view.read_with(cx, |view, _| {
            view.notices
                .list
                .iter()
                .map(|notice| notice.title.clone())
                .collect()
        })
    }

    fn phase(view: &Entity<Workbench>, cx: &mut VisualTestContext) -> Option<Phase> {
        view.read_with(cx, |view, _| {
            view.update_offer.as_ref().map(|offer| offer.phase.clone())
        })
    }

    /// An installed Keel in `dir`, fed by a list with `build`.
    fn installed(dir: &std::path::Path, build: u64) -> Source {
        let applications = dir.join("Applications");
        std::fs::create_dir_all(applications.join("Keel.app")).unwrap();
        Source {
            feed: update::test_support::feed_with(dir, build),
            app: applications.join("Keel.app"),
            key: Some(update::test_support::public_key_hex()),
            restart: false,
        }
    }

    #[gpui::test]
    fn a_keel_outside_an_app_says_it_is_not_updated(cx: &mut TestAppContext) {
        let (_folder, view, cx) = window(cx, r#"{ "keel.language": "it" }"#);
        cx.update(|_, cx| set_source(None, cx));
        view.update_in(cx, |view, window, cx| {
            view.check_for_updates(true, window, cx)
        });
        cx.run_until_parked();
        assert_eq!(
            titles(&view, cx),
            ["Gli aggiornamenti arrivano solo a Keel.app"]
        );
    }

    #[gpui::test]
    fn a_new_version_is_offered_downloaded_and_made_ready(cx: &mut TestAppContext) {
        let (folder, view, cx) = window(cx, r#"{ "keel.language": "it" }"#);
        let source = installed(folder.path(), 2);
        let app = source.app.clone();
        cx.update(|_, cx| set_source(Some(source), cx));

        view.update_in(cx, |view, window, cx| {
            view.check_for_updates(true, window, cx)
        });
        cx.run_until_parked();
        assert_eq!(phase(&view, cx), Some(Phase::Ready));
        assert!(cx.update(|window, cx| window.has_active_dialog(cx)));
        view.read_with(cx, |view, _| {
            let offer = view.update_offer.as_ref().unwrap();
            assert_eq!(offer.release.build, 2);
            assert_eq!(offer.release.notes.it, "Novità di prova.");
        });

        view.update_in(cx, |view, window, cx| view.install_update(window, cx));
        cx.run_until_parked();
        assert_eq!(phase(&view, cx), Some(Phase::Installing));
        let (staged, replaced) = cx.update(|_, cx| Workbench::pending_update(cx)).unwrap();
        assert_eq!(replaced, app);
        assert!(staged.join("Contents/MacOS/keel").exists());
        assert!(staged.starts_with(update::staging_dir(&app)));
    }

    #[gpui::test]
    fn the_same_version_is_up_to_date(cx: &mut TestAppContext) {
        let (folder, view, cx) = window(cx, r#"{ "keel.language": "it" }"#);
        let source = installed(folder.path(), crate::version::BUILD);
        cx.update(|_, cx| set_source(Some(source), cx));
        view.update_in(cx, |view, window, cx| {
            view.check_for_updates(true, window, cx)
        });
        cx.run_until_parked();
        assert_eq!(phase(&view, cx), None);
        assert_eq!(titles(&view, cx), ["Keel è aggiornato"]);
    }

    #[gpui::test]
    fn a_release_signed_by_someone_else_is_refused(cx: &mut TestAppContext) {
        let (folder, view, cx) = window(cx, r#"{ "keel.language": "it" }"#);
        let mut source = installed(folder.path(), 2);
        // Another key: the release no longer matches.
        source.key = Some("11".repeat(32));
        cx.update(|_, cx| set_source(Some(source.clone()), cx));
        view.update_in(cx, |view, window, cx| {
            view.check_for_updates(true, window, cx)
        });
        cx.run_until_parked();
        view.update_in(cx, |view, window, cx| view.install_update(window, cx));
        cx.run_until_parked();
        match phase(&view, cx) {
            Some(Phase::Failed(error)) => assert!(error.contains("not signed"), "{error}"),
            other => panic!("expected a failure, got {other:?}"),
        }
        assert!(cx.update(|_, cx| Workbench::pending_update(cx)).is_none());
        assert!(source.app.exists(), "the installed Keel is untouched");
    }

    #[gpui::test]
    fn at_start_it_looks_only_if_the_settings_allow_it(cx: &mut TestAppContext) {
        let (folder, view, cx) = window(cx, r#"{ "update.automatic": false }"#);
        let source = installed(folder.path(), 2);
        cx.update(|_, cx| {
            set_source(Some(source), cx);
            cx.default_global::<Updates>().started = false;
        });
        view.update_in(cx, |view, window, cx| view.updates_at_start(window, cx));
        cx.run_until_parked();
        assert_eq!(phase(&view, cx), None, "switched off, nothing is asked");

        view.update_in(cx, |view, window, cx| {
            view.change_setting(AUTOMATIC_SETTING, Some(Value::Bool(true)), window, cx);
        });
        cx.update(|_, cx| cx.default_global::<Updates>().started = false);
        view.update_in(cx, |view, window, cx| view.updates_at_start(window, cx));
        cx.run_until_parked();
        assert_eq!(phase(&view, cx), Some(Phase::Ready));
        // Once per Keel: another window does not ask again.
        view.update_in(cx, |view, _, _| view.update_offer = None);
        view.update_in(cx, |view, window, cx| view.updates_at_start(window, cx));
        cx.run_until_parked();
        assert_eq!(phase(&view, cx), None);
    }

    #[gpui::test]
    fn the_next_keel_says_how_the_update_went(cx: &mut TestAppContext) {
        let (folder, view, cx) = window(
            cx,
            r#"{ "keel.language": "it", "update.automatic": false }"#,
        );
        let data = folder.path().join("data");
        update::write_outcome(
            &data,
            &update::Outcome {
                version: "0.2.0".to_owned(),
                error: None,
            },
        )
        .unwrap();
        cx.update(|_, cx| cx.default_global::<Updates>().started = false);
        view.update_in(cx, |view, window, cx| {
            view.data_dir = Some(data.clone());
            view.updates_at_start(window, cx);
        });
        cx.run_until_parked();
        assert_eq!(titles(&view, cx), ["Keel è aggiornato alla 0.2.0"]);
        assert!(update::take_outcome(&data).is_none(), "said once");
    }
}
