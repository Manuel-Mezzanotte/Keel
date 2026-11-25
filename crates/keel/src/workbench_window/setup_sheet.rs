//! «Prepara Keel», right after the first start's welcome: three things that
//! make Keel one's own in a minute. The theme; the settings, shortcuts,
//! snippets, recent projects and extensions of VS Code or Cursor; and the
//! `keel` command in the Terminal. Each is there later too: the Settings,
//! the Extensions view, the command palette.

use std::path::{Path, PathBuf};

use gpui::{
    AnyElement, FocusHandle, KeyDownEvent, MouseButton, SharedString,
    StatefulInteractiveElement as _, relative,
};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::{Disableable as _, Icon, Sizable as _};
use gpui_kit_assets::IconName;

use super::other_editor::OtherEditor;
use super::shell::Colors;
use super::*;

actions!(keel, [InstallShellCommand]);

/// Where the `keel` command goes: on the PATH of every shell of macOS.
const COMMAND_DIR: &str = "/usr/local/bin";

/// How the `keel` command's installation went.
#[derive(Clone, Debug, PartialEq)]
pub(super) enum CommandState {
    Idle,
    Working,
    Done,
    Failed(String),
}

/// The sheet while it shows.
pub(super) struct SetupSheet {
    focus: FocusHandle,
    /// The editors whose things were brought in.
    imported: Vec<OtherEditor>,
}

/// What putting the link in `dir` found.
#[derive(Debug, PartialEq)]
pub(super) enum Linked {
    /// The link is there, to `program`.
    Done,
    /// The folder needs an administrator.
    NeedsAdmin,
    /// Something else is there by that name, left alone.
    Taken(PathBuf),
}

/// Puts a link to `program` called `keel` in `dir`, replacing an older
/// link but nothing else.
pub(super) fn link_command(program: &Path, dir: &Path) -> Linked {
    let link = dir.join("keel");
    match std::fs::symlink_metadata(&link) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            if std::fs::read_link(&link).ok().as_deref() == Some(program) {
                return Linked::Done;
            }
            if std::fs::remove_file(&link).is_err() {
                return Linked::NeedsAdmin;
            }
        }
        Ok(_) => return Linked::Taken(link),
        Err(_) => {}
    }
    if std::fs::create_dir_all(dir).is_err() {
        return Linked::NeedsAdmin;
    }
    match std::os::unix::fs::symlink(program, &link) {
        Ok(()) => Linked::Done,
        Err(_) => Linked::NeedsAdmin,
    }
}

/// Installs the `keel` command: a link to the program Keel runs from, in
/// `/usr/local/bin`; macOS asks for the administrator's password when the
/// folder is the system's.
pub(super) fn install_command(italian: bool) -> Result<(), String> {
    let program = std::env::current_exe()
        .and_then(|program| program.canonicalize())
        .map_err(|error| error.to_string())?;
    match link_command(&program, Path::new(COMMAND_DIR)) {
        Linked::Done => Ok(()),
        Linked::Taken(path) => Err(if italian {
            format!(
                "C'è già un altro programma in {}: lasciato com'è.",
                path.display()
            )
        } else {
            format!("Another program is at {}: left as it is.", path.display())
        }),
        Linked::NeedsAdmin => {
            let quoted =
                |text: &str| format!("\"{}\"", text.replace('\\', "\\\\").replace('"', "\\\""));
            let shell = format!(
                "mkdir -p {COMMAND_DIR} && ln -sfn '{}' {COMMAND_DIR}/keel",
                program.display().to_string().replace('\'', "'\\''")
            );
            let prompt = if italian {
                "Keel vuole mettere il comando keel nel Terminale."
            } else {
                "Keel wants to put the keel command in the Terminal."
            };
            let script = format!(
                "do shell script {} with prompt {} with administrator privileges",
                quoted(&shell),
                quoted(prompt)
            );
            let output = std::process::Command::new("/usr/bin/osascript")
                .arg("-e")
                .arg(script)
                .output()
                .map_err(|error| error.to_string())?;
            if output.status.success() {
                Ok(())
            } else {
                let error = String::from_utf8_lossy(&output.stderr).trim().to_owned();
                // Cancel in macOS's password window says -128.
                Err(if error.contains("-128") {
                    if italian {
                        "Annullato.".to_owned()
                    } else {
                        "Cancelled.".to_owned()
                    }
                } else {
                    error
                })
            }
        }
    }
}

impl Workbench {
    /// Shows the sheet, after the welcome.
    pub(super) fn open_setup(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let focus = cx.focus_handle();
        focus.focus(window, cx);
        self.setup = Some(SetupSheet {
            focus,
            imported: Vec::new(),
        });
        cx.notify();
    }

    fn close_setup(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.setup.take().is_some() {
            self.focus_handle.focus(window, cx);
            cx.notify();
        }
    }

    /// Settings, shortcuts, snippets and recent projects (after asking),
    /// then the extensions, shown in the Extensions view.
    fn import_everything_from(
        &mut self,
        editor: OtherEditor,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.import_settings_from(
            editor,
            Some(Box::new(move |this, window, cx| {
                this.import_extensions_from(editor, window, cx);
                if let Some(setup) = &mut this.setup
                    && !setup.imported.contains(&editor)
                {
                    setup.imported.push(editor);
                }
                cx.notify();
            })),
            window,
            cx,
        );
    }

    /// Installs the `keel` command off the window's thread: macOS's
    /// password window waits for the person.
    pub(super) fn install_shell_command(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.shell_command == CommandState::Working {
            return;
        }
        self.shell_command = CommandState::Working;
        cx.notify();
        let italian = self.language == UiLanguage::Italian;
        let task = cx.background_spawn(async move { install_command(italian) });
        cx.spawn_in(window, async move |this, cx| {
            let result = task.await;
            this.update(cx, |this, cx| {
                this.shell_command = match result {
                    Ok(()) => CommandState::Done,
                    Err(error) => CommandState::Failed(error),
                };
                let strings = this.strings();
                this.status_message = Some(match &this.shell_command {
                    CommandState::Done => strings.shell_command_done.to_owned(),
                    CommandState::Failed(error) => {
                        format!("{} {error}", strings.shell_command_failed)
                    }
                    _ => String::new(),
                });
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    pub(super) fn setup_actions(&self, root: gpui::Div, cx: &mut Context<Self>) -> gpui::Div {
        root.on_action(cx.listener(|this, _: &InstallShellCommand, window, cx| {
            this.install_shell_command(window, cx)
        }))
    }

    /// Whether the sheet is on screen.
    #[cfg(any(test, feature = "snapshots"))]
    pub(super) fn setup_shown(&self) -> bool {
        self.setup.is_some()
    }

    /// The sheet over the window, or nothing.
    pub(super) fn render_setup(
        &self,
        colors: Colors,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let setup = self.setup.as_ref()?;
        let italian = self.language == UiLanguage::Italian;
        let pick = |it: &'static str, en: &'static str| if italian { it } else { en };
        let workbench = cx.entity().downgrade();
        let handle = window.window_handle();

        let step = |number: &'static str, title: &'static str, words: &'static str| {
            div()
                .flex()
                .items_start()
                .gap(px(12.))
                .child(
                    div()
                        .flex_none()
                        .size(px(24.))
                        .rounded_full()
                        .flex()
                        .items_center()
                        .justify_center()
                        .bg(colors.accent.opacity(0.14))
                        .text_size(px(12.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(colors.accent)
                        .child(number),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .gap(px(2.))
                        .child(
                            div()
                                .text_size(px(14.))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(colors.text)
                                .child(title),
                        )
                        .child(
                            div()
                                .text_size(px(12.5))
                                .line_height(px(18.))
                                .text_color(colors.muted)
                                .child(words),
                        ),
                )
        };

        // 1. The theme, from its pictures.
        let theme = div()
            .flex()
            .flex_col()
            .gap(px(10.))
            .child(step(
                "1",
                pick("Il tuo tema", "Your theme"),
                pick(
                    "Chiaro di giorno, Mezzanotte di sera, Nero per chi lo vuole nero.",
                    "Chiaro by day, Mezzanotte at night, Nero for those who want it black.",
                ),
            ))
            .child(div().pl(px(36.)).child(super::interface_page::theme_picker(
                self, &workbench, handle, colors, cx,
            )));

        // 2. What the other editors have.
        let present: Vec<OtherEditor> = OtherEditor::ALL
            .into_iter()
            .filter(|editor| editor.is_present())
            .collect();
        let import_rows: Vec<AnyElement> = present
            .iter()
            .map(|&editor| {
                let done = setup.imported.contains(&editor);
                let label = if done {
                    pick("Importato", "Imported")
                } else {
                    pick("Importa", "Import")
                };
                div()
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .h(px(38.))
                    .px(px(12.))
                    .rounded(px(9.))
                    .border_1()
                    .border_color(colors.border.opacity(0.8))
                    .child(
                        Icon::new(IconName::Import)
                            .size(px(15.))
                            .text_color(colors.muted),
                    )
                    .child(
                        div()
                            .flex_1()
                            .text_size(px(13.5))
                            .text_color(colors.text)
                            .child(editor.name()),
                    )
                    .child(
                        Button::new(SharedString::from(format!(
                            "setup-import-{}",
                            editor.name()
                        )))
                        .small()
                        .when(done, |button| button.ghost().icon(IconName::Check))
                        .when(!done, |button| button.primary())
                        .label(label)
                        .on_click(cx.listener(
                            move |this, _, window, cx| {
                                this.import_everything_from(editor, window, cx)
                            },
                        )),
                    )
                    .into_any_element()
            })
            .collect();
        let import = div()
            .flex()
            .flex_col()
            .gap(px(10.))
            .child(step(
                "2",
                pick("Le tue cose", "Your things"),
                pick(
                    "Impostazioni, scorciatoie, frammenti, progetti recenti ed estensioni. Quello che hai già in Keel resta com'è.",
                    "Settings, shortcuts, snippets, recent projects and extensions. What you already have in Keel stays as it is.",
                ),
            ))
            .child(
                div()
                    .pl(px(36.))
                    .flex()
                    .flex_col()
                    .gap(px(6.))
                    .when(present.is_empty(), |rows| {
                        rows.child(
                            div()
                                .text_size(px(12.5))
                                .text_color(colors.faint)
                                .child(pick(
                                    "Su questo Mac non ho trovato né VS Code né Cursor.",
                                    "I found neither VS Code nor Cursor on this Mac.",
                                )),
                        )
                    })
                    .children(import_rows),
            );

        // 3. The `keel` command.
        let command_button = match &self.shell_command {
            CommandState::Done => Button::new("setup-command")
                .small()
                .ghost()
                .icon(IconName::Check)
                .label(pick("Installato", "Installed")),
            CommandState::Working => Button::new("setup-command")
                .small()
                .ghost()
                .loading(true)
                .disabled(true)
                .label(pick("Un momento…", "One moment…")),
            CommandState::Idle | CommandState::Failed(_) => Button::new("setup-command")
                .small()
                .primary()
                .label(pick("Installa", "Install"))
                .on_click(
                    cx.listener(|this, _, window, cx| this.install_shell_command(window, cx)),
                ),
        };
        let failure = match &self.shell_command {
            CommandState::Failed(error) => Some(error.clone()),
            _ => None,
        };
        let command = div()
            .flex()
            .flex_col()
            .gap(px(8.))
            .child(
                div()
                    .flex()
                    .items_start()
                    .gap(px(12.))
                    .child(div().flex_1().min_w_0().child(step(
                        "3",
                        pick("Il comando keel nel Terminale", "The keel command in the Terminal"),
                        pick(
                            "Poi `keel .` apre la cartella in cui sei. macOS può chiederti la password.",
                            "Then `keel .` opens the folder you are in. macOS may ask for your password.",
                        ),
                    )))
                    .child(div().flex_none().pt(px(2.)).child(command_button)),
            )
            .children(failure.map(|error| {
                div()
                    .pl(px(36.))
                    .text_size(px(12.))
                    .text_color(cx.theme().danger)
                    .child(error)
            }));

        let card = div()
            .id("setup-card")
            .w(px(560.))
            .max_w(relative(0.92))
            .max_h(relative(0.88))
            .overflow_y_scroll()
            .p(px(28.))
            .flex()
            .flex_col()
            .gap(px(22.))
            .rounded(px(18.))
            .bg(colors.editor)
            .border_1()
            .border_color(colors.border)
            .shadow_2xl()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(6.))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(10.))
                            .child(
                                gpui::svg()
                                    .path(crate::assets::LOGO)
                                    .size(px(22.))
                                    .text_color(colors.accent),
                            )
                            .child(
                                div()
                                    .text_size(px(22.))
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(colors.text)
                                    .child(pick("Prepara Keel", "Set up Keel")),
                            ),
                    )
                    .child(
                        div()
                            .text_size(px(13.))
                            .text_color(colors.muted)
                            .child(pick(
                                "Tre cose e sei a bordo. Tutto si cambia anche dopo, dalle Impostazioni.",
                                "Three things and you are aboard. All of it changes later too, in the Settings.",
                            )),
                    ),
            )
            .child(theme)
            .child(div().h(px(1.)).bg(colors.border.opacity(0.6)))
            .child(import)
            .child(div().h(px(1.)).bg(colors.border.opacity(0.6)))
            .child(command)
            .child(
                div().flex().justify_end().child(
                    Button::new("setup-done")
                        .primary()
                        .label(pick("Inizia a lavorare", "Start working"))
                        .on_click(cx.listener(|this, _, window, cx| this.close_setup(window, cx))),
                ),
            );
        let focus = setup.focus.clone();
        Some(
            div()
                .id("setup")
                .absolute()
                .inset_0()
                .occlude()
                .flex()
                .items_center()
                .justify_center()
                .bg(gpui::black().opacity(if colors.dark { 0.45 } else { 0.25 }))
                .track_focus(&focus)
                .key_context("KeelSetup")
                .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                    if event.keystroke.key == "escape" {
                        this.close_setup(window, cx);
                        cx.stop_propagation();
                    }
                }))
                .child(card)
                .into_any_element(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_command_is_a_link_that_replaces_only_an_older_link() {
        let folder = tempfile::tempdir().unwrap();
        let bin = folder.path().join("bin");
        let old = folder.path().join("Old.app/keel");
        let new = folder.path().join("Keel.app/keel");
        assert_eq!(link_command(&old, &bin), Linked::Done);
        assert_eq!(std::fs::read_link(bin.join("keel")).unwrap(), old);
        // A newer Keel replaces its own older link.
        assert_eq!(link_command(&new, &bin), Linked::Done);
        assert_eq!(std::fs::read_link(bin.join("keel")).unwrap(), new);
        assert_eq!(link_command(&new, &bin), Linked::Done, "already there");
        // Somebody else's `keel` stays.
        let other = folder.path().join("other");
        std::fs::create_dir_all(&other).unwrap();
        std::fs::write(other.join("keel"), "#!/bin/sh\n").unwrap();
        assert_eq!(
            link_command(&new, &other),
            Linked::Taken(other.join("keel"))
        );
    }
}
