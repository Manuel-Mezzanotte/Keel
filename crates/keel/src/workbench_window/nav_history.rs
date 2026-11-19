//! Where you have been, to go back: Back and Forward through the places
//! visited (Ctrl+- and Ctrl+Shift+-, and the mouse's side buttons), the
//! last place you edited, the recent places with a line of their code, as
//! JetBrains lists them, and Ctrl+Tab through the files used last. A place
//! counts when you change file or jump more than ten lines.

use gpui::{Div, KeyBinding, MouseButton, NavigationDirection};

use super::*;
use crate::quick_pick::{self, Matching, PickItem, PickOptions};

actions!(
    keel,
    [
        GoBack,
        GoForward,
        GoToLastEdit,
        ShowRecentPlaces,
        RecentEditors
    ]
);

/// How far the cursor jumps before the place it left counts.
const JUMP: u32 = 10;
/// How many places Back keeps.
const KEPT: usize = 60;

pub(crate) fn bindings() -> Vec<KeyBinding> {
    vec![
        // In the list of recent editors, Ctrl+Tab goes on and Ctrl+Shift+Tab back.
        KeyBinding::new("ctrl-tab", gpui_base::actions::SelectDown, Some("Command")),
        KeyBinding::new(
            "ctrl-shift-tab",
            gpui_base::actions::SelectUp,
            Some("Command"),
        ),
    ]
}

/// A place: a document and a cursor position, counted from zero.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Place {
    pub(super) uri: String,
    pub(super) line: u32,
    pub(super) column: u32,
}

#[derive(Default)]
pub(super) struct NavHistory {
    back: Vec<Place>,
    forward: Vec<Place>,
    /// Where the cursor is now, as last seen.
    current: Option<Place>,
    /// The last place text changed.
    last_edit: Option<Place>,
    /// Going back or forward: the move it makes is not a new place.
    moving: bool,
    /// The documents in the order they were last in front.
    pub(super) recent: Vec<String>,
}

impl NavHistory {
    /// The cursor moved: the place it left counts when it changed file or
    /// jumped far, and the way forward is forgotten.
    pub(super) fn moved(&mut self, place: Place) {
        if self.moving {
            self.current = Some(place);
            return;
        }
        if let Some(current) = self.current.take() {
            let far = current.uri != place.uri || current.line.abs_diff(place.line) > JUMP;
            if far {
                if self.back.last() != Some(&current) {
                    self.back.push(current);
                }
                if self.back.len() > KEPT {
                    self.back.remove(0);
                }
                self.forward.clear();
            }
        }
        self.current = Some(place);
    }

    /// The place to go back to, keeping the one left for Forward.
    pub(super) fn back(&mut self) -> Option<Place> {
        let target = self.back.pop()?;
        if let Some(current) = self.current.replace(target.clone()) {
            self.forward.push(current);
        }
        Some(target)
    }

    pub(super) fn forward(&mut self) -> Option<Place> {
        let target = self.forward.pop()?;
        if let Some(current) = self.current.replace(target.clone()) {
            self.back.push(current);
        }
        Some(target)
    }

    pub(super) fn edited(&mut self, place: Place) {
        self.last_edit = Some(place);
    }

    /// A document came in front.
    pub(super) fn shown(&mut self, uri: &str) {
        self.recent.retain(|recent| recent != uri);
        self.recent.insert(0, uri.to_owned());
        self.recent.truncate(KEPT);
    }

    /// The places visited, the latest first, one per line of a document.
    fn places(&self) -> Vec<Place> {
        let mut places: Vec<Place> = Vec::new();
        for place in self.current.iter().chain(self.back.iter().rev()) {
            if !places
                .iter()
                .any(|seen| seen.uri == place.uri && seen.line == place.line)
            {
                places.push(place.clone());
            }
        }
        places
    }
}

impl Workbench {
    pub(super) fn nav_actions(&self, root: Div, cx: &mut Context<Self>) -> Div {
        root.on_action(cx.listener(|this, _: &GoBack, window, cx| this.go_back(window, cx)))
            .on_action(cx.listener(|this, _: &GoForward, window, cx| this.go_forward(window, cx)))
            .on_action(cx.listener(|this, _: &GoToLastEdit, window, cx| {
                if let Some(place) = this.nav.last_edit.clone() {
                    this.go_to_place(&place, window, cx)
                }
            }))
            .on_action(cx.listener(|this, _: &ShowRecentPlaces, window, cx| {
                this.show_recent_places(window, cx)
            }))
            .on_action(cx.listener(|this, _: &RecentEditors, window, cx| {
                this.show_recent_editors(window, cx)
            }))
            // The side buttons of a mouse, as in a browser.
            .on_mouse_down(
                MouseButton::Navigate(NavigationDirection::Back),
                cx.listener(|this, _, window, cx| this.go_back(window, cx)),
            )
            .on_mouse_down(
                MouseButton::Navigate(NavigationDirection::Forward),
                cx.listener(|this, _, window, cx| this.go_forward(window, cx)),
            )
    }

    /// The cursor of `uri`'s editor moved.
    pub(super) fn note_cursor(&mut self, uri: &str, editor: &Entity<EditorState>, cx: &App) {
        let position = editor.read(cx).cursor_position();
        self.nav.moved(Place {
            uri: uri.to_owned(),
            line: position.line,
            column: position.character,
        });
    }

    pub(super) fn note_edit(
        &mut self,
        uri: &str,
        editor: &Entity<EditorState>,
        cx: &mut Context<Self>,
    ) {
        self.refresh_outline(true, cx);
        let position = editor.read(cx).cursor_position();
        self.nav.edited(Place {
            uri: uri.to_owned(),
            line: position.line,
            column: position.character,
        });
    }

    /// The document in front changed: first in Ctrl+Tab's list.
    pub(super) fn note_active_editor(&mut self, cx: &mut Context<Self>) {
        if let Some(uri) = self.panes.get(self.active).map(|pane| pane.uri.clone()) {
            self.nav.shown(&uri);
            if uri != self.outline_uri() {
                self.refresh_outline(false, cx);
            }
        }
    }

    fn go_back(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(place) = self.nav.back() {
            self.go_to_place(&place, window, cx);
        }
    }

    fn go_forward(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(place) = self.nav.forward() {
            self.go_to_place(&place, window, cx);
        }
    }

    /// Shows a place: its tab, or its file opened again.
    pub(super) fn go_to_place(
        &mut self,
        place: &Place,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.nav.moving = true;
        if self.documents.contains_key(&place.uri) {
            self.show_document(&place.uri, window, cx);
        } else if let Some(path) = Uri::parse(&place.uri).and_then(|uri| uri.file_path()) {
            if !path.is_file() {
                self.nav.moving = false;
                return;
            }
            self.open_file(&path, window, cx);
        } else {
            self.nav.moving = false;
            return;
        }
        self.go_to(place.line + 1, place.column + 1, window, cx);
        self.nav.moving = false;
        self.nav.current = Some(place.clone());
        cx.notify();
    }

    /// A document's name and the folder it is in, for the lists.
    fn place_label(&self, uri: &str) -> (String, Option<String>) {
        let name = self
            .documents
            .get(uri)
            .map(|document| self.display_name(document))
            .or_else(|| {
                Uri::parse(uri)
                    .and_then(|uri| uri.file_path())
                    .and_then(|path| {
                        path.file_name()
                            .map(|name| name.to_string_lossy().into_owned())
                    })
            })
            .unwrap_or_else(|| uri.to_owned());
        let folder = Uri::parse(uri)
            .and_then(|uri| uri.file_path())
            .and_then(|path| {
                let parent = path.parent()?.to_path_buf();
                let root = self.project.root.as_ref()?;
                Some(
                    parent
                        .strip_prefix(root)
                        .ok()?
                        .to_string_lossy()
                        .into_owned(),
                )
            })
            .filter(|folder| !folder.is_empty());
        (name, folder)
    }

    /// The line of code at a place, trimmed, for the recent places.
    fn line_at(&self, place: &Place) -> String {
        let text = self
            .documents
            .get(&place.uri)
            .map(|document| document.synced_text.clone())
            .or_else(|| {
                let path = Uri::parse(&place.uri)?.file_path()?;
                std::fs::read_to_string(path).ok()
            })
            .unwrap_or_default();
        let line = text
            .lines()
            .nth(place.line as usize)
            .unwrap_or_default()
            .trim();
        line.chars().take(90).collect()
    }

    /// Recent Places: where you were, each with its line of code.
    fn show_recent_places(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let places = self.nav.places();
        if places.is_empty() {
            return;
        }
        let items = places
            .iter()
            .map(|place| {
                let (name, _) = self.place_label(&place.uri);
                let mut item = PickItem::new(format!("{name}:{}", place.line + 1));
                item.detail = Some(self.line_at(place));
                item
            })
            .collect();
        let title = self.strings().nav_recent_places.to_owned();
        let chosen = quick_pick::show(Some(title), None, items, window, cx);
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Some(picked)) = chosen.await else {
                return;
            };
            this.update_in(cx, |this, window, cx| {
                this.go_to_place(&places[picked], window, cx)
            })
            .ok();
        })
        .detach();
    }

    /// Ctrl+Tab: the open documents, the last used first, with the one
    /// before this highlighted; Ctrl+Tab again goes on, and letting Ctrl
    /// go opens it.
    fn show_recent_editors(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let open: Vec<String> = self
            .groups
            .iter()
            .flat_map(|group| group.tabs.iter().map(|tab| tab.uri.clone()))
            .collect();
        let mut uris: Vec<String> = self
            .nav
            .recent
            .iter()
            .filter(|uri| open.contains(uri))
            .cloned()
            .collect();
        for uri in open {
            if !uris.contains(&uri) {
                uris.push(uri);
            }
        }
        if uris.is_empty() {
            return;
        }
        let items = uris
            .iter()
            .map(|uri| {
                let (name, folder) = self.place_label(uri);
                let mut item = PickItem::new(name);
                item.detail = folder;
                item
            })
            .collect();
        let options = PickOptions {
            selected: Some(usize::from(uris.len() > 1)),
            confirm_on_ctrl_release: true,
            ..Default::default()
        };
        let chosen = quick_pick::show_with_options(
            None,
            None,
            items,
            Matching::Text,
            None,
            options,
            window,
            cx,
        );
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Some(choice)) = chosen.await else {
                return;
            };
            let uri = uris[choice.index].clone();
            this.update_in(cx, |this, window, cx| this.show_document(&uri, window, cx))
                .ok();
        })
        .detach();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn place(uri: &str, line: u32) -> Place {
        Place {
            uri: uri.to_owned(),
            line,
            column: 0,
        }
    }

    #[test]
    fn far_moves_are_places_to_go_back_to() {
        let mut nav = NavHistory::default();
        nav.moved(place("a", 0));
        nav.moved(place("a", 3));
        nav.moved(place("a", 5));
        // A short move is not a place of its own.
        nav.moved(place("a", 50));
        nav.moved(place("b", 1));
        assert_eq!(nav.back(), Some(place("a", 50)));
        assert_eq!(nav.back(), Some(place("a", 5)));
        assert_eq!(nav.back(), None);
        assert_eq!(nav.forward(), Some(place("a", 50)));
        assert_eq!(nav.forward(), Some(place("b", 1)));
        assert_eq!(nav.forward(), None);
        // A new place after going back forgets the way forward.
        nav.back();
        nav.moved(place("c", 0));
        assert_eq!(nav.forward(), None);
    }

    #[test]
    fn recent_documents_come_first() {
        let mut nav = NavHistory::default();
        for uri in ["a", "b", "c", "a"] {
            nav.shown(uri);
        }
        assert_eq!(nav.recent, ["a", "c", "b"]);
    }

    #[gpui::test]
    fn back_and_forward_go_between_files(cx: &mut gpui::TestAppContext) {
        cx.update(|cx| {
            gpui_component::init(cx);
            language::register_grammars();
            crate::workbench_window::init(cx);
        });
        let folder = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(folder.path()).unwrap();
        let long: String = (1..=80).map(|line| format!("riga {line}\n")).collect();
        std::fs::write(root.join("a.txt"), &long).unwrap();
        std::fs::write(root.join("b.txt"), "b\n").unwrap();
        std::fs::write(root.join("c.txt"), "c\n").unwrap();
        let settings_file = root.join(".keel-settings.json");
        std::fs::write(&settings_file, r#"{ "keel.language": "en" }"#).unwrap();
        let settings = Settings::load(&settings_file, None);
        let opening = Opening::new(Some(root.clone()), None);
        let (view, cx) =
            cx.add_window_view(|window, cx| Workbench::new(opening, settings, None, window, cx));
        let active = |view: &Entity<Workbench>, cx: &mut gpui::VisualTestContext| {
            view.read_with(cx, |view, cx| {
                let pane = &view.panes[view.active];
                let line = pane.editor.read(cx).cursor_position().line;
                (pane.uri.rsplit('/').next().unwrap().to_owned(), line)
            })
        };
        view.update_in(cx, |view, window, cx| {
            view.open_file(&root.join("a.txt"), window, cx);
            view.go_to(50, 1, window, cx);
        });
        cx.run_until_parked();
        view.update_in(cx, |view, window, cx| {
            view.open_file(&root.join("b.txt"), window, cx)
        });
        cx.run_until_parked();
        cx.simulate_keystrokes("ctrl--");
        assert_eq!(active(&view, cx), ("a.txt".to_owned(), 49));
        cx.simulate_keystrokes("ctrl-shift--");
        assert_eq!(active(&view, cx).0, "b.txt");

        // Ctrl+1 the first tab, Close Saved leaves the one with changes.
        view.update_in(cx, |view, window, cx| {
            view.open_file(&root.join("c.txt"), window, cx)
        });
        cx.simulate_input("x");
        view.update_in(cx, |view, window, cx| {
            for group in &mut view.groups {
                for tab in &mut group.tabs {
                    tab.preview = false;
                }
            }
            let _ = window;
            cx.notify();
        });
        cx.simulate_keystrokes("ctrl-1");
        assert_eq!(active(&view, cx).0, "a.txt");
        cx.dispatch_action(super::super::tabs::CloseSavedEditors);
        cx.run_until_parked();
        let left: Vec<String> = view.read_with(cx, |view, _| {
            view.groups
                .iter()
                .flat_map(|group| &group.tabs)
                .map(|tab| tab.uri.rsplit('/').next().unwrap().to_owned())
                .collect()
        });
        assert_eq!(left, ["c.txt"]);
    }
}
