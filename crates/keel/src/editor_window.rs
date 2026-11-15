//! The editor window: one document in a code editor that fills the window.

use std::path::PathBuf;

use editor::Document;
use gpui::{
    App, AppContext as _, Context, Entity, InteractiveElement as _, IntoElement, KeyBinding,
    ParentElement as _, PromptLevel, Render, Styled as _, Subscription, Window, actions, div,
};
use gpui_component::input::{Editor, EditorState, InputEvent};
use language::Language;

actions!(keel, [Save, CloseWindow, Quit]);

/// Registers the key bindings of the editor window.
pub fn init(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("cmd-s", Save, None),
        KeyBinding::new("cmd-w", CloseWindow, None),
        KeyBinding::new("cmd-q", Quit, None),
    ]);
    // Reached only when no window handles Quit, so nothing is left unsaved.
    cx.on_action(|_: &Quit, cx| cx.quit());
}

/// What to do once the user has dealt with unsaved changes.
#[derive(Clone, Copy)]
enum AfterConfirm {
    CloseWindow,
    QuitApp,
}

pub struct EditorWindow {
    /// `None` while the buffer is untitled.
    document: Option<Document>,
    editor: Entity<EditorState>,
    modified: bool,
    /// Set when the user chose to discard changes, so closing goes through.
    discard_changes: bool,
    _subscriptions: Vec<Subscription>,
}

impl EditorWindow {
    pub fn new(document: Option<Document>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let language = document.as_ref().map_or(Language::PlainText, |document| {
            Language::for_path(document.path())
        });
        let text = document
            .as_ref()
            .map(|document| document.saved_text().to_owned())
            .unwrap_or_default();

        let editor = cx.new(|cx| {
            EditorState::new(window, cx)
                .language(language.highlighter_name())
                .default_value(text)
        });
        editor.update(cx, |editor, cx| editor.focus(window, cx));

        let subscription = cx.subscribe_in(
            &editor,
            window,
            |this, _, event: &InputEvent, window, cx| {
                if matches!(event, InputEvent::Change) {
                    this.refresh_modified(window, cx);
                }
            },
        );

        let this = cx.weak_entity();
        window.on_window_should_close(cx, move |window, cx| {
            this.update(cx, |this, cx| {
                this.confirm(AfterConfirm::CloseWindow, window, cx)
            })
            .unwrap_or(true)
        });

        let view = Self {
            document,
            editor,
            modified: false,
            discard_changes: false,
            _subscriptions: vec![subscription],
        };
        view.refresh_title(window);
        view
    }

    fn file_name(&self) -> String {
        self.document
            .as_ref()
            .map_or_else(|| "Untitled".to_owned(), Document::file_name)
    }

    fn current_text(&self, cx: &App) -> String {
        self.editor.read(cx).value().to_string()
    }

    fn refresh_modified(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let text = self.editor.read(cx).text();
        let saved = self.document.as_ref().map_or("", Document::saved_text);
        let modified = text != saved;
        if modified != self.modified {
            self.modified = modified;
            self.refresh_title(window);
            cx.notify();
        }
    }

    fn refresh_title(&self, window: &mut Window) {
        window.set_window_title(&format!("{} — Keel", self.file_name()));
        window.set_window_edited(self.modified);
        window.set_document_path(self.document.as_ref().map(Document::path));
    }

    fn save(&mut self, _: &Save, window: &mut Window, cx: &mut Context<Self>) {
        self.save_then(None, window, cx);
    }

    /// Saves the buffer, asking for a path first if it is untitled, then runs
    /// `after` if the save went through.
    fn save_then(
        &mut self,
        after: Option<AfterConfirm>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let text = self.current_text(cx);
        match self.document.as_mut() {
            Some(document) => match document.save(&text) {
                Ok(()) => {
                    self.modified = false;
                    self.refresh_title(window);
                    cx.notify();
                    if let Some(after) = after {
                        self.finish(after, window, cx);
                    }
                }
                Err(error) => {
                    let message = format!("Keel could not save {}", document.file_name());
                    show_error(&message, &error.to_string(), window, cx);
                }
            },
            None => self.save_as(text, after, window, cx),
        }
    }

    fn save_as(
        &mut self,
        text: String,
        after: Option<AfterConfirm>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let folder = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/"));
        let chosen = cx.prompt_for_new_path(&folder, Some("Untitled.txt"));
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(path))) = chosen.await else {
                return;
            };
            this.update_in(cx, |this, window, cx| {
                let mut document = match Document::open(&path) {
                    Ok(document) => document,
                    Err(error) => {
                        let message = format!("Keel could not use {}", path.display());
                        show_error(&message, &error.to_string(), window, cx);
                        return;
                    }
                };
                if let Err(error) = document.save(&text) {
                    let message = format!("Keel could not save {}", document.file_name());
                    show_error(&message, &error.to_string(), window, cx);
                    return;
                }
                let language = Language::for_path(&path);
                this.editor.update(cx, |editor, cx| {
                    editor.set_highlighter(language.highlighter_name(), cx);
                });
                this.document = Some(document);
                this.refresh_modified(window, cx);
                this.refresh_title(window);
                if let Some(after) = after {
                    this.finish(after, window, cx);
                }
            })
            .ok();
        })
        .detach();
    }

    fn close_window(&mut self, _: &CloseWindow, window: &mut Window, cx: &mut Context<Self>) {
        if self.confirm(AfterConfirm::CloseWindow, window, cx) {
            window.remove_window();
        }
    }

    fn quit(&mut self, _: &Quit, window: &mut Window, cx: &mut Context<Self>) {
        if self.confirm(AfterConfirm::QuitApp, window, cx) {
            cx.quit();
        }
    }

    /// Returns `true` when it is safe to go ahead right away. Otherwise asks
    /// whether to save the changes, returns `false`, and finishes later.
    fn confirm(
        &mut self,
        after: AfterConfirm,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if !self.modified || self.discard_changes {
            return true;
        }
        let message = format!(
            "Do you want to save the changes you made to {}?",
            self.file_name()
        );
        let answer = window.prompt(
            PromptLevel::Warning,
            &message,
            Some("Your changes will be lost if you don't save them."),
            &["Save", "Don't Save", "Cancel"],
            cx,
        );
        cx.spawn_in(window, async move |this, cx| {
            let Ok(choice) = answer.await else {
                return;
            };
            this.update_in(cx, |this, window, cx| match choice {
                0 => this.save_then(Some(after), window, cx),
                1 => {
                    this.discard_changes = true;
                    this.finish(after, window, cx);
                }
                _ => {}
            })
            .ok();
        })
        .detach();
        false
    }

    fn finish(&mut self, after: AfterConfirm, window: &mut Window, cx: &mut Context<Self>) {
        match after {
            AfterConfirm::CloseWindow => window.remove_window(),
            AfterConfirm::QuitApp => cx.quit(),
        }
    }
}

impl Render for EditorWindow {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .on_action(cx.listener(Self::save))
            .on_action(cx.listener(Self::close_window))
            .on_action(cx.listener(Self::quit))
            .child(Editor::new(&self.editor).size_full().bordered(false))
    }
}

/// Shows an error in a dialog with a single OK button.
fn show_error(message: &str, detail: &str, window: &mut Window, cx: &mut App) {
    // The dialog stays open on its own; there is no answer to wait for.
    let _dismissed = window.prompt(PromptLevel::Critical, message, Some(detail), &["OK"], cx);
}

#[cfg(test)]
mod tests {
    use gpui::TestAppContext;

    use super::*;

    fn set_up(cx: &mut TestAppContext) {
        cx.update(|cx| {
            gpui_component::init(cx);
            language::register_grammars();
            init(cx);
        });
    }

    #[gpui::test]
    fn typing_marks_the_window_modified_and_save_writes_the_file(cx: &mut TestAppContext) {
        set_up(cx);
        let folder = tempfile::tempdir().unwrap();
        let path = folder.path().join("app.ts");
        std::fs::write(&path, "let total = 1;\n").unwrap();
        let document = Document::open(&path).unwrap();

        let (view, cx) =
            cx.add_window_view(|window, cx| EditorWindow::new(Some(document), window, cx));
        assert!(!view.read_with(cx, |view, _| view.modified));

        cx.simulate_input("è");
        cx.run_until_parked();
        assert!(view.read_with(cx, |view, _| view.modified));
        let typed = view.read_with(cx, |view, cx| view.current_text(cx));
        assert!(
            typed.contains('è'),
            "the typed text is in the buffer: {typed:?}"
        );

        cx.dispatch_action(Save);
        cx.run_until_parked();
        assert!(!view.read_with(cx, |view, _| view.modified));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), typed);
    }

    #[gpui::test]
    fn undoing_back_to_the_saved_text_clears_the_modified_mark(cx: &mut TestAppContext) {
        set_up(cx);
        let folder = tempfile::tempdir().unwrap();
        let path = folder.path().join("index.tsx");
        std::fs::write(&path, "export const a = <div />;\n").unwrap();
        let document = Document::open(&path).unwrap();

        let (view, cx) =
            cx.add_window_view(|window, cx| EditorWindow::new(Some(document), window, cx));
        cx.simulate_input("x");
        cx.run_until_parked();
        assert!(view.read_with(cx, |view, _| view.modified));

        cx.simulate_keystrokes("cmd-z");
        cx.run_until_parked();
        assert!(!view.read_with(cx, |view, _| view.modified));
    }
}
