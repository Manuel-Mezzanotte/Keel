//! Bookmarks on lines, as JetBrains' and Xcode's: F3 puts one on the
//! cursor's line or takes it away, Cmd+Alt+L and Cmd+Alt+J go to the next
//! and the one before across files, Cmd+F3 lists them all. A bookmark
//! keeps its line's text, so it follows the line as lines are added above
//! it; they come back with the window.

use std::path::Path;

use gpui::Div;

use crate::quick_pick::{self, PickItem};
use crate::session::BookmarkState;

use super::*;

actions!(
    keel,
    [
        ToggleBookmark,
        NextBookmark,
        PreviousBookmark,
        ListBookmarks,
        ClearBookmarks
    ]
);

/// How far from its line a bookmark looks for its text.
const NEARBY: usize = 300;

/// The line of a bookmark in `text`: its own while the text is still there,
/// else the nearest line with that text, else where it was.
pub(super) fn resolve(bookmark: &BookmarkState, text: &str) -> usize {
    let lines: Vec<&str> = text.lines().collect();
    let line = bookmark.line as usize;
    let wanted = bookmark.text.trim();
    let same = |at: usize| lines.get(at).is_some_and(|here| here.trim() == wanted);
    if wanted.is_empty() || same(line) {
        return line.min(lines.len().saturating_sub(1));
    }
    for distance in 1..=NEARBY {
        if same(line + distance) {
            return line + distance;
        }
        if line >= distance && same(line - distance) {
            return line - distance;
        }
    }
    line.min(lines.len().saturating_sub(1))
}

impl Workbench {
    pub(super) fn bookmark_actions(&self, root: Div, cx: &mut Context<Self>) -> Div {
        root.on_action(
            cx.listener(|this, _: &ToggleBookmark, window, cx| this.toggle_bookmark(window, cx)),
        )
        .on_action(
            cx.listener(|this, _: &NextBookmark, window, cx| {
                this.jump_to_bookmark(true, window, cx)
            }),
        )
        .on_action(cx.listener(|this, _: &PreviousBookmark, window, cx| {
            this.jump_to_bookmark(false, window, cx)
        }))
        .on_action(
            cx.listener(|this, _: &ListBookmarks, window, cx| this.list_bookmarks(window, cx)),
        )
        .on_action(cx.listener(|this, _: &ClearBookmarks, _, cx| this.clear_bookmarks(cx)))
    }

    /// The file and the cursor's line in the editor in front.
    fn cursor_place(&self, cx: &App) -> Option<(PathBuf, usize, String)> {
        let pane = self.panes.get(self.active)?;
        let path = self.documents.get(&pane.uri)?.disk_path()?.to_path_buf();
        let editor = pane.editor.read(cx);
        let line = editor.cursor_position().line as usize;
        let text = editor
            .value()
            .lines()
            .nth(line)
            .unwrap_or_default()
            .trim()
            .to_owned();
        Some((path, line, text))
    }

    /// The bookmarks of a file as they are now in its text, following their
    /// lines; returns the ranges of their lines.
    pub(super) fn bookmark_ranges(
        &mut self,
        path: &Path,
        text: &str,
    ) -> Vec<std::ops::Range<usize>> {
        let starts: Vec<usize> = std::iter::once(0)
            .chain(text.match_indices('\n').map(|(at, _)| at + 1))
            .collect();
        let mut ranges = Vec::new();
        for bookmark in self
            .bookmarks
            .iter_mut()
            .filter(|bookmark| bookmark.path == path)
        {
            let line = resolve(bookmark, text);
            bookmark.line = line as u32;
            let start = starts.get(line).copied().unwrap_or(text.len());
            let end = starts
                .get(line + 1)
                .map_or(text.len(), |next| next.saturating_sub(1))
                .max(start);
            ranges.push(start..end);
        }
        ranges
    }

    /// Draws the bookmarks again in every editor of a file.
    fn redraw_bookmarks(&mut self, path: &Path, cx: &mut Context<Self>) {
        let editors: Vec<(String, Entity<EditorState>)> = self
            .panes
            .iter()
            .filter(|pane| {
                self.documents
                    .get(&pane.uri)
                    .and_then(|document| document.disk_path())
                    == Some(path)
            })
            .map(|pane| (pane.editor_id.clone(), pane.editor.clone()))
            .collect();
        for (id, editor) in editors {
            self.redecorate(&id, &editor, false, cx);
        }
        self.session_changed(cx);
        cx.notify();
    }

    /// F3: a bookmark on the cursor's line, or none any more.
    fn toggle_bookmark(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        let Some((path, line, text)) = self.cursor_place(cx) else {
            return;
        };
        let had = self.bookmarks.len();
        self.bookmarks
            .retain(|bookmark| !(bookmark.path == path && bookmark.line as usize == line));
        if self.bookmarks.len() == had {
            self.bookmarks.push(BookmarkState {
                path: path.clone(),
                line: line as u32,
                text,
            });
        }
        self.redraw_bookmarks(&path, cx);
    }

    /// Cmd+Alt+L and Cmd+Alt+J: the next bookmark after the cursor, or the
    /// one before it, going on to the other files and round.
    fn jump_to_bookmark(&mut self, forward: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.bookmarks.is_empty() {
            self.status_message = Some(self.strings().bookmarks_none.to_owned());
            cx.notify();
            return;
        }
        let mut all: Vec<(PathBuf, u32)> = self
            .bookmarks
            .iter()
            .map(|bookmark| (bookmark.path.clone(), bookmark.line))
            .collect();
        all.sort();
        let here = self
            .cursor_place(cx)
            .map(|(path, line, _)| (path, line as u32));
        let target = match &here {
            Some(here) if forward => all.iter().find(|place| *place > here).or(all.first()),
            Some(here) => all.iter().rev().find(|place| *place < here).or(all.last()),
            None => all.first(),
        }
        .cloned();
        if let Some((path, line)) = target {
            self.open_file(&path, window, cx);
            self.go_to(line + 1, 1, window, cx);
        }
    }

    /// Cmd+F3: every bookmark, to go to one.
    fn list_bookmarks(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let strings = self.strings();
        if self.bookmarks.is_empty() {
            self.status_message = Some(strings.bookmarks_none.to_owned());
            cx.notify();
            return;
        }
        let mut places = self.bookmarks.clone();
        places.sort_by(|a, b| (&a.path, a.line).cmp(&(&b.path, b.line)));
        let items = places
            .iter()
            .map(|bookmark| {
                let name = bookmark
                    .path
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_default();
                let mut item =
                    PickItem::new(format!("{name}:{}  {}", bookmark.line + 1, bookmark.text));
                item.detail = Some(self.shown_path(&Uri::file(&bookmark.path).to_string()));
                item
            })
            .collect();
        let chosen = quick_pick::show(
            Some(strings.bookmarks_title.to_owned()),
            None,
            items,
            window,
            cx,
        );
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Some(picked)) = chosen.await else {
                return;
            };
            let place = &places[picked];
            this.update_in(cx, |this, window, cx| {
                this.open_file(&place.path, window, cx);
                this.go_to(place.line + 1, 1, window, cx);
            })
            .ok();
        })
        .detach();
    }

    /// Takes away the bookmarks of the file in front.
    fn clear_bookmarks(&mut self, cx: &mut Context<Self>) {
        let Some((path, _, _)) = self.cursor_place(cx) else {
            return;
        };
        self.bookmarks.retain(|bookmark| bookmark.path != path);
        self.redraw_bookmarks(&path, cx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bookmark(line: u32, text: &str) -> BookmarkState {
        BookmarkState {
            path: PathBuf::from("/app.ts"),
            line,
            text: text.to_owned(),
        }
    }

    #[gpui::test]
    fn f3_marks_lines_that_follow_edits_and_are_gone_through(cx: &mut gpui::TestAppContext) {
        let (_folder, path, view, cx) = super::super::tests::window_with_file(
            cx,
            "app.ts",
            "const a = 1;\nfunction total() {\n  return 2;\n}\n",
        );
        cx.simulate_keystrokes("down f3");
        cx.run_until_parked();
        let lines = |view: &Entity<Workbench>, cx: &mut gpui::VisualTestContext| {
            view.read_with(cx, |view, _| {
                view.bookmarks
                    .iter()
                    .map(|bookmark| bookmark.line)
                    .collect::<Vec<_>>()
            })
        };
        assert_eq!(lines(&view, cx), [1]);
        // Two lines typed above it: it goes down with its line.
        cx.simulate_keystrokes("up");
        cx.simulate_input("// one\n// two\n");
        cx.executor()
            .advance_clock(std::time::Duration::from_secs(1));
        cx.run_until_parked();
        assert_eq!(lines(&view, cx), [3]);
        let state = view.read_with(cx, |view, cx| view.window_state(cx));
        assert_eq!(state.bookmarks[0].path, path);
        // From the top, Cmd+Alt+L goes to it; F3 there takes it away.
        cx.simulate_keystrokes("cmd-up cmd-alt-l");
        let line = view.read_with(cx, |view, cx| {
            view.panes[0].editor.read(cx).cursor_position().line
        });
        assert_eq!(line, 3);
        cx.simulate_keystrokes("f3");
        assert!(view.read_with(cx, |view, _| view.bookmarks.is_empty()));
    }

    #[test]
    fn a_bookmark_follows_its_line() {
        let text = "a\nfunction total() {\nb\n";
        assert_eq!(resolve(&bookmark(1, "function total() {"), text), 1);
        // Two lines added above: it is two lines down.
        let moved = "x\ny\na\nfunction total() {\nb\n";
        assert_eq!(resolve(&bookmark(1, "function total() {"), moved), 3);
        // Its text is gone: it stays where it was.
        assert_eq!(resolve(&bookmark(1, "gone"), moved), 1);
        assert_eq!(resolve(&bookmark(40, "gone"), moved), 4);
    }
}
