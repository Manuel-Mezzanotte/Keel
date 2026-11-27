//! The Output and Ports views of the bottom panel: Keel's and the
//! extensions' messages, and the ports that programs started in Keel's
//! terminals listen on, as VS Code shows them.

use std::time::Duration;

use gpui::{AnyElement, ClickEvent, Div, StatefulInteractiveElement as _};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::{Icon, Selectable as _, Sizable as _};
use gpui_kit_assets::IconName;
use terminal::ListeningPort;

use super::shell::{Colors, note};
use super::*;

/// How often the Ports view looks again, while terminals are open.
const PORTS_INTERVAL: Duration = Duration::from_secs(3);

/// Where a message of the Output view comes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum OutputChannel {
    Keel,
    Extensions,
    /// The language servers' messages, each line starting with its name.
    Languages,
}

impl Workbench {
    /// Adds a line to a channel of the Output view.
    pub(super) fn log_to(&mut self, channel: OutputChannel, line: impl Into<String>) {
        self.output.push((channel, line.into()));
        if self.output.len() > OUTPUT_LINES {
            let extra = self.output.len() - OUTPUT_LINES;
            self.output.drain(..extra);
        }
    }

    fn output_lines(&self) -> impl Iterator<Item = &String> {
        self.output
            .iter()
            .filter(|(channel, _)| *channel == self.output_channel)
            .map(|(_, line)| line)
    }

    /// The channel switch and the clear button, for the panel's header.
    pub(super) fn render_output_header(&self, cx: &mut Context<Self>) -> Div {
        let strings = self.strings();
        let channel = |id: &'static str, label: &'static str, which: OutputChannel| {
            Button::new(id)
                .ghost()
                .small()
                .label(label)
                .selected(self.output_channel == which)
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.output_channel = which;
                    cx.notify();
                }))
        };
        div()
            .ml_auto()
            .flex()
            .items_center()
            .gap_1()
            .child(channel("output-keel", "Keel", OutputChannel::Keel))
            .child(channel(
                "output-extensions",
                strings.output_extensions,
                OutputChannel::Extensions,
            ))
            .child(channel(
                "output-languages",
                strings.output_languages,
                OutputChannel::Languages,
            ))
            .child(
                Button::new("output-clear")
                    .ghost()
                    .small()
                    .icon(Icon::new(IconName::Trash))
                    .tooltip(strings.output_clear)
                    .on_click(cx.listener(|this, _, _, cx| {
                        let shown = this.output_channel;
                        this.output.retain(|(channel, _)| *channel != shown);
                        cx.notify();
                    })),
            )
    }

    pub(super) fn render_output(&self, colors: Colors, cx: &App) -> AnyElement {
        if self.output_lines().next().is_none() {
            return note(self.strings().no_output.to_owned(), colors).into_any_element();
        }
        div()
            .font_family(cx.theme().mono_font_family.clone())
            .text_xs()
            .text_color(colors.muted)
            .px_2()
            .children(self.output_lines().map(|line| div().child(line.clone())))
            .into_any_element()
    }

    // -----------------------------------------------------------------
    // Ports

    /// The processes the terminals started: the panel's and Claude Code's.
    fn terminal_pids(&self, cx: &App) -> Vec<u32> {
        self.terminals
            .iter()
            .map(|terminal| &terminal.view)
            .chain(self.agent_terminal.as_ref().map(|agent| &agent.view))
            .map(|view| view.read(cx).pid())
            .collect()
    }

    /// Looks for ports every few seconds while terminals are open.
    pub(super) fn watch_ports(&mut self, cx: &mut Context<Self>) {
        if self.watching_ports {
            return;
        }
        self.watching_ports = true;
        self.ports_task = cx.spawn(async move |this, cx| {
            loop {
                let Ok(pids) = this.update(cx, |this, cx| this.terminal_pids(cx)) else {
                    return;
                };
                if pids.is_empty() {
                    this.update(cx, |this, cx| {
                        this.watching_ports = false;
                        this.ports_changed(Vec::new(), cx);
                    })
                    .ok();
                    return;
                }
                let ports = cx
                    .background_spawn(async move { terminal::listening_ports(&pids) })
                    .await;
                if this
                    .update(cx, |this, cx| this.ports_changed(ports, cx))
                    .is_err()
                {
                    return;
                }
                cx.background_executor().timer(PORTS_INTERVAL).await;
            }
        });
    }

    /// New ports are announced in the status bar, as VS Code does with a
    /// notification.
    pub(super) fn ports_changed(&mut self, ports: Vec<ListeningPort>, cx: &mut Context<Self>) {
        if ports == self.ports {
            return;
        }
        let new: Vec<&ListeningPort> = ports
            .iter()
            .filter(|port| !self.ports.iter().any(|known| known.port == port.port))
            .collect();
        if let Some(port) = new.first() {
            self.status_message = Some(self.strings().port_opened(port.port, &port.process));
        }
        self.ports = ports;
        cx.notify();
    }

    fn open_port(&self, port: u16, cx: &App) {
        cx.open_url(&format!("http://localhost:{port}"));
    }

    pub(super) fn render_ports(&self, colors: Colors, cx: &mut Context<Self>) -> AnyElement {
        let strings = self.strings();
        if self.ports.is_empty() {
            return note(strings.no_ports.to_owned(), colors).into_any_element();
        }
        let cell = |width: f32| div().w(px(width)).flex_none().truncate();
        let header = div()
            .h(px(24.))
            .px_2()
            .flex()
            .items_center()
            .text_xs()
            .text_color(colors.faint)
            .child(cell(90.).child(strings.port))
            .child(cell(160.).child(strings.port_address))
            .child(div().flex_1().child(strings.port_process));
        let rows = self.ports.iter().map(|port| {
            let number = port.port;
            div()
                .id(("port", number as usize))
                .h(px(26.))
                .px_2()
                .flex()
                .items_center()
                .rounded(px(6.))
                .text_sm()
                .cursor_pointer()
                .hover(|row| row.bg(colors.hover))
                .on_click(
                    cx.listener(move |this, _: &ClickEvent, _, cx| this.open_port(number, cx)),
                )
                .child(
                    cell(90.)
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(number.to_string()),
                )
                .child(
                    cell(160.)
                        .text_color(colors.muted)
                        .child(format!("localhost:{number}")),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_color(colors.muted)
                        .child(format!("{} ({})", port.process, port.pid)),
                )
                .child(
                    Button::new(("preview-port", number as usize))
                        .ghost()
                        .small()
                        .icon(Icon::new(IconName::AppWindow))
                        .tooltip(strings.web_open_in_keel)
                        .on_click(cx.listener(move |this, _, window, cx| {
                            cx.stop_propagation();
                            this.open_web_page(&format!("localhost:{number}"), window, cx)
                        })),
                )
                .child(
                    Button::new(("open-port", number as usize))
                        .ghost()
                        .small()
                        .icon(Icon::new(IconName::Globe))
                        .tooltip(strings.open_in_browser)
                        .on_click(cx.listener(move |this, _, _, cx| this.open_port(number, cx))),
                )
        });
        div()
            .flex()
            .flex_col()
            .child(header)
            .children(rows)
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use std::net::TcpListener;
    use std::time::Instant;

    use gpui::TestAppContext;

    use super::*;

    #[gpui::test]
    fn a_port_opened_in_a_terminal_shows_up(cx: &mut TestAppContext) {
        cx.executor().allow_parking();
        cx.update(|cx| {
            gpui_component::init(cx);
            crate::workbench_window::init(cx);
        });
        let folder = tempfile::tempdir().unwrap();
        let base = std::fs::canonicalize(folder.path()).unwrap();
        let settings = Settings::load(base.join("settings.json"), None);
        let opening = Opening::new(Some(base.clone()), None);
        let (view, cx) =
            cx.add_window_view(|window, cx| Workbench::new(opening, settings, None, window, cx));

        // A free port, given to a server started in the terminal.
        let port = {
            let probe = TcpListener::bind("127.0.0.1:0").unwrap();
            probe.local_addr().unwrap().port()
        };
        let server = format!(
            "exec python3 -c 'import socket,time; s=socket.socket(); s.bind((\"127.0.0.1\", {port})); s.listen(); time.sleep(30)'"
        );
        view.update_in(cx, |view, window, cx| {
            let mut spawn = super::super::terminal_view::shell_spawn(base.clone(), cx);
            spawn.command = Some(("/bin/sh".into(), vec!["-c".into(), server]));
            view.add_terminal(spawn, None, true, window, cx).unwrap();
        });

        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            cx.run_until_parked();
            let found = view.read_with(cx, |view, _| {
                view.ports.iter().any(|found| found.port == port)
            });
            if found {
                break;
            }
            assert!(Instant::now() < deadline, "port {port} not found");
            cx.executor().advance_clock(PORTS_INTERVAL);
            std::thread::sleep(Duration::from_millis(100));
        }
        let status = view.read_with(cx, |view, _| view.status_message.clone().unwrap());
        assert!(status.contains(&port.to_string()), "{status}");
    }

    #[gpui::test]
    fn extension_messages_have_their_own_channel(cx: &mut TestAppContext) {
        cx.update(|cx| {
            gpui_component::init(cx);
            crate::workbench_window::init(cx);
        });
        let folder = tempfile::tempdir().unwrap();
        let settings = Settings::load(folder.path().join("settings.json"), None);
        let (view, cx) = cx.add_window_view(|window, cx| {
            Workbench::new(Opening::new(None, None), settings, None, window, cx)
        });
        view.update(cx, |view, _| {
            view.log_to(OutputChannel::Keel, "keel line");
            view.log_to(OutputChannel::Extensions, "[info] extension line");
            assert_eq!(view.output_lines().collect::<Vec<_>>(), ["keel line"]);
            view.output_channel = OutputChannel::Extensions;
            assert_eq!(
                view.output_lines().collect::<Vec<_>>(),
                ["[info] extension line"]
            );
        });
    }
}
