//! Keeps an eye on the extension host: every few seconds it must answer a
//! ping. A host stuck in an extension's code (an endless loop) or one whose
//! thread has ended is stopped and started again, and the Output says which
//! call it was busy with. After too many restarts in a short time the
//! extensions stay off until restarted by hand.

use std::time::Instant;

use futures::FutureExt as _;

use super::*;

/// How many restarts are allowed within [`RESTART_WINDOW`].
const MAX_RESTARTS: usize = 3;
const RESTART_WINDOW: Duration = Duration::from_secs(10 * 60);

pub(super) struct HostWatchdog {
    /// Time between two pings.
    pub(super) interval: Duration,
    /// How long a ping may stay unanswered, in real time.
    pub(super) timeout: Duration,
    restarts: Vec<Instant>,
    /// Bumped by every new host, so the old watch ends.
    generation: u64,
}

impl Default for HostWatchdog {
    fn default() -> Self {
        Self {
            interval: Duration::from_secs(5),
            timeout: Duration::from_secs(10),
            restarts: Vec::new(),
            generation: 0,
        }
    }
}

impl Workbench {
    /// Starts watching the host just started.
    pub(super) fn watch_host(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.watchdog.generation += 1;
        let generation = self.watchdog.generation;
        let (interval, timeout) = (self.watchdog.interval, self.watchdog.timeout);
        cx.spawn_in(window, async move |this, cx| {
            loop {
                cx.background_executor().timer(interval).await;
                let ping = this.update(cx, |this, _| {
                    let extensions = this.extensions.as_ref()?;
                    (this.watchdog.generation == generation)
                        .then(|| (extensions.host.is_running(), extensions.host.ping()))
                });
                let Ok(Some((running, ping))) = ping else {
                    return;
                };
                let mut answered = false;
                if running {
                    let started = Instant::now();
                    let mut ping = Box::pin(ping.fuse());
                    loop {
                        let mut tick = cx
                            .background_executor()
                            .timer(Duration::from_millis(100))
                            .fuse();
                        futures::select_biased! {
                            result = ping => {
                                answered = result.is_ok();
                                break;
                            }
                            _ = tick => {}
                        }
                        if started.elapsed() >= timeout {
                            break;
                        }
                    }
                }
                if answered {
                    continue;
                }
                this.update_in(cx, |this, window, cx| {
                    if this.watchdog.generation == generation {
                        this.host_stuck(window, cx);
                    }
                })
                .ok();
                return;
            }
        })
        .detach();
    }

    /// Restarts a host that stopped answering, unless it happened too often.
    fn host_stuck(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let strings = self.strings();
        let busy = self
            .extensions
            .as_ref()
            .map(|extensions| extensions.host.waiting_for())
            .unwrap_or_default();
        let what = busy.first().cloned().unwrap_or_else(|| "?".to_owned());
        let now = Instant::now();
        self.watchdog
            .restarts
            .retain(|restart| now.duration_since(*restart) < RESTART_WINDOW);
        self.watchdog.restarts.push(now);
        if self.watchdog.restarts.len() > MAX_RESTARTS {
            // Off until restarted by hand.
            self.watchdog.generation += 1;
            self.extensions = None;
            self.clear_extension_diagnostics(None, cx);
            self.log(format!("{} ({what})", strings.extensions_gave_up));
            self.report(strings.extensions_gave_up.to_owned());
            cx.notify();
            return;
        }
        let message = strings.extensions_restarted(&what);
        self.log(message.clone());
        self.status_message = Some(message);
        self.restart_extensions(window, cx);
    }

    /// "Restart Extension Host" by hand: the count of restarts starts over.
    pub(super) fn restart_extensions_by_hand(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.watchdog.restarts.clear();
        self.restart_extensions(window, cx);
    }
}

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use gpui::{TestAppContext, VisualTestContext};

    use super::super::tests::test_settings;
    use super::*;

    fn wait_until(
        cx: &mut VisualTestContext,
        what: &str,
        check: impl Fn(&mut VisualTestContext) -> bool,
    ) {
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            cx.executor().advance_clock(Duration::from_millis(100));
            cx.run_until_parked();
            if check(cx) {
                return;
            }
            assert!(Instant::now() < deadline, "timed out waiting for {what}");
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    #[gpui::test]
    fn a_stuck_extension_host_is_restarted(cx: &mut TestAppContext) {
        cx.executor().allow_parking();
        cx.update(|cx| {
            gpui_component::init(cx);
            language::register_grammars();
            init(cx);
        });
        let data = tempfile::tempdir().unwrap();
        let root = crate::paths::extensions_dir(data.path()).join("keel-tests.stuck-1.0.0");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(
            root.join("package.json"),
            r#"{ "name": "stuck", "publisher": "keel-tests", "version": "1.0.0",
                 "main": "./extension.js", "activationEvents": ["*"],
                 "contributes": { "commands": [{ "command": "stuck.loop", "title": "Loop" }] } }"#,
        )
        .unwrap();
        std::fs::write(
            root.join("extension.js"),
            r#"const vscode = require("vscode");
               exports.activate = (context) => {
                 context.subscriptions.push(vscode.commands.registerCommand("stuck.loop", () => { while (true) {} }));
               };"#,
        )
        .unwrap();
        let mut setup = crate::windows::extension_setup(data.path()).unwrap();
        setup.marketplace = None;
        let (view, cx) = cx.add_window_view(|window, cx| {
            let mut workbench = Workbench::new(None, test_settings(), None, window, cx);
            workbench.watchdog.interval = Duration::from_secs(1);
            workbench.watchdog.timeout = Duration::from_millis(400);
            workbench.start_extensions(setup, window, cx);
            workbench
        });
        wait_until(cx, "the command", |cx| {
            view.read_with(cx, |view, _| {
                view.extensions
                    .as_ref()
                    .is_some_and(|extensions| extensions.commands.contains("stuck.loop"))
            })
        });
        let first = view.read_with(cx, |view, _| view.watchdog.generation);

        view.update_in(cx, |view, window, cx| {
            view.run_extension_command("stuck.loop".to_owned(), Vec::new(), window, cx)
        });
        wait_until(cx, "a new host", |cx| {
            view.read_with(cx, |view, _| view.watchdog.generation > first)
        });
        view.read_with(cx, |view, _| {
            let log = view
                .output
                .iter()
                .map(|(_, line)| line.as_str())
                .collect::<Vec<_>>()
                .join("\n");
            assert!(log.contains("commands/execute stuck.loop"), "{log}");
            assert!(view.extensions.as_ref().unwrap().host.is_running());
        });
        // The new host works: the command is registered again.
        wait_until(cx, "the command again", |cx| {
            view.read_with(cx, |view, _| {
                view.extensions
                    .as_ref()
                    .is_some_and(|extensions| extensions.commands.contains("stuck.loop"))
            })
        });
    }
}
