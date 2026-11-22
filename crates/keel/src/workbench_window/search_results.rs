//! The search's results in a tab of their own, as Zed has them: the lines
//! that match in every file, with a line around them, under the file's
//! name, all in one editor. What is typed there goes back into the files
//! with Cmd+S, each piece only where the file still has the lines it
//! showed.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::ops::Range;

use files::search::Query;
use gpui::Div;
use gpui_component::ActiveTheme as _;
use gpui_component::input::{RangeDecoration, RangeDecorationCollection, RangeDecorationStyle};
use regex::Regex;

use super::file_pieces::{Put, lines_of, with_pieces};
use super::*;

actions!(keel, [OpenSearchResults]);

/// Lines shown above and below each match, as VS Code's search editor.
const CONTEXT: usize = 1;

/// A piece of a file the tab shows. Its lines are without their endings,
/// which the file keeps as they are.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Excerpt {
    /// From the project's folder, with `/`.
    pub(super) path: String,
    /// Its first line in the file, from 0.
    pub(super) start: usize,
    /// The lines as the file has them.
    pub(super) original: Vec<String>,
    /// The lines as the tab first showed them: what differs was typed.
    shown: Vec<String>,
    /// The line over it in the tab. It stays the same while some piece
    /// waits to be saved, even when lines above moved it.
    header: String,
}

impl Excerpt {
    fn new(path: &str, start: usize, original: Vec<String>) -> Self {
        Self {
            header: header_of(path, start),
            path: path.to_owned(),
            start,
            shown: original.clone(),
            original,
        }
    }
}

fn header_of(path: &str, start: usize) -> String {
    format!("── {path}:{} ──", start + 1)
}

pub(super) struct ResultsTab {
    pub(super) title: String,
    pub(super) excerpts: Vec<Excerpt>,
    /// The text as last saved, to tell when it changed.
    pub(super) saved: String,
    pattern: Option<Regex>,
    /// The colours of each editor showing the tab.
    decor: HashMap<gpui::EntityId, RangeDecorationCollection>,
}

/// The pieces of a file around its matching lines, those that touch
/// joined into one.
fn excerpts_of(path: &str, text: &str, matching: &[usize]) -> Vec<Excerpt> {
    let file = lines_of(text);
    let mut ranges: Vec<Range<usize>> = Vec::new();
    for &line in matching.iter().filter(|line| **line < file.len()) {
        let range = line.saturating_sub(CONTEXT)..(line + CONTEXT + 1).min(file.len());
        match ranges.last_mut() {
            Some(last) if range.start <= last.end => last.end = last.end.max(range.end),
            _ => ranges.push(range),
        }
    }
    ranges
        .into_iter()
        .map(|range| {
            let start = range.start;
            let lines = file[range].iter().map(|(line, _)| (*line).to_owned());
            Excerpt::new(path, start, lines.collect())
        })
        .collect()
}

/// The tab's text: each piece under its header, a blank line between.
fn results_text(excerpts: &[Excerpt]) -> String {
    excerpts
        .iter()
        .map(|excerpt| format!("{}\n{}", excerpt.header, excerpt.shown.join("\n")))
        .collect::<Vec<_>>()
        .join("\n\n")
}

/// Each piece's lines as the tab has them now, found under their headers;
/// `None` for a piece whose header was changed, and for the one before
/// it, which no longer says where it ends.
fn edited_pieces(excerpts: &[Excerpt], text: &str) -> Vec<Option<Vec<String>>> {
    let lines: Vec<&str> = text
        .split('\n')
        .map(|line| line.strip_suffix('\r').unwrap_or(line))
        .collect();
    let mut at = Vec::with_capacity(excerpts.len());
    let mut from = 0;
    for excerpt in excerpts {
        let found = lines[from.min(lines.len())..]
            .iter()
            .position(|line| *line == excerpt.header)
            .map(|offset| from + offset);
        if let Some(found) = found {
            from = found + 1;
        }
        at.push(found);
    }
    (0..excerpts.len())
        .map(|index| {
            let start = at[index]? + 1;
            let end = match at.get(index + 1) {
                Some(next) => (*next)?,
                None => lines.len(),
            };
            let mut piece: Vec<String> = lines[start.min(end)..end]
                .iter()
                .map(|line| (*line).to_owned())
                .collect();
            // The blank line before the next header is the tab's own.
            if index + 1 < excerpts.len() && piece.last().is_some_and(String::is_empty) {
                piece.pop();
            }
            Some(piece)
        })
        .collect()
}

impl Workbench {
    pub(super) fn search_results_actions(&self, root: Div, cx: &mut Context<Self>) -> Div {
        root.on_action(cx.listener(|this, _: &OpenSearchResults, window, cx| {
            this.open_search_results(window, cx)
        }))
    }

    /// The results in view, in a tab that can be edited.
    pub(super) fn open_search_results(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(root) = self.project.root.clone() else {
            return;
        };
        let query: Query = self.search_query(cx);
        let Some(pattern) = query.pattern().ok().filter(|_| !query.text.is_empty()) else {
            return;
        };
        let mut excerpts = Vec::new();
        let files: Vec<String> = self
            .search_results()
            .files
            .iter()
            .map(|file| file.path.clone())
            .collect();
        for path in files {
            let Some(text) = self.text_for_results(&root.join(&path)) else {
                continue;
            };
            // Looked for again: the file may have changed since the search.
            let lines: Vec<usize> = files::search::matching_lines(&text, &pattern)
                .iter()
                .map(|line| line.line)
                .collect();
            excerpts.extend(excerpts_of(&path, &text, &lines));
        }
        if excerpts.is_empty() {
            return;
        }
        let text = results_text(&excerpts);
        let strings = self.strings();
        let title = super::notices::fill(strings.search_results_title, &[("query", &query.text)]);
        let uri = format!("keel-results:{}", self.next_untitled);
        self.next_untitled += 1;
        let tab = ResultsTab {
            title,
            excerpts,
            saved: text.clone(),
            pattern: Some(pattern),
            decor: HashMap::new(),
        };
        let backing = Backing::Results(Box::new(tab));
        let uri = self.add_document(uri, backing, "plaintext".to_owned(), text);
        let column = self.active.min(self.panes.len().saturating_sub(1));
        self.show_tab(&uri, column, OpenAs::Normal, true, window, cx);
        // What the editor really shows is what typing is measured from.
        let text = self
            .editors_showing(&uri)
            .first()
            .map(|editor| editor.read(cx).value().to_string());
        if let (Some(text), Some(document)) = (text, self.documents.get_mut(&uri))
            && let Backing::Results(tab) = &mut document.backing
        {
            let pieces = edited_pieces(&tab.excerpts, &text);
            for (excerpt, shown) in tab.excerpts.iter_mut().zip(pieces) {
                if let Some(shown) = shown {
                    excerpt.shown = shown;
                }
            }
            tab.saved = text.clone();
            document.synced_text = text;
        }
        self.color_search_results(&uri, cx);
    }

    /// Opens the file of the piece under the cursor, at its line.
    pub(super) fn open_result_under_cursor(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let (Some(root), Some(pane)) = (self.project.root.clone(), self.panes.get(self.active))
        else {
            return false;
        };
        let Some(Backing::Results(tab)) = self
            .documents
            .get(&pane.uri)
            .map(|document| &document.backing)
        else {
            return false;
        };
        let editor = pane.editor.read(cx);
        let cursor = editor.cursor_position();
        let text = editor.value().to_string();
        // The last header at the cursor or above it.
        let found = text
            .split('\n')
            .take(cursor.line as usize + 1)
            .enumerate()
            .filter_map(|(row, line)| {
                let excerpt = tab.excerpts.iter().find(|excerpt| excerpt.header == line)?;
                Some((row, excerpt))
            })
            .last();
        let Some((row, excerpt)) = found else {
            return true;
        };
        let inside = (cursor.line as usize).saturating_sub(row + 1);
        let line = excerpt.start + inside.min(excerpt.original.len().saturating_sub(1));
        let column = if cursor.line as usize > row {
            cursor.character
        } else {
            0
        };
        let path = root.join(&excerpt.path);
        self.open_file(&path, window, cx);
        self.go_to(line as u32 + 1, column + 1, window, cx);
        true
    }

    /// A file's text as Keel has it: its tab's, or the disk's when it is
    /// UTF-8.
    fn text_for_results(&self, path: &Path) -> Option<String> {
        let uri = Uri::file(path).to_string();
        match self.documents.get(&uri) {
            Some(document) if matches!(document.backing, Backing::File(_)) => {
                Some(document.synced_text.clone())
            }
            _ => String::from_utf8(std::fs::read(path).ok()?).ok(),
        }
    }

    /// The headers and the matches of a results tab, coloured again.
    pub(super) fn color_search_results(&mut self, uri: &str, cx: &mut Context<Self>) {
        let Some(Backing::Results(tab)) = self.documents.get(uri).map(|document| &document.backing)
        else {
            return;
        };
        let theme = cx.theme();
        let (header_color, match_color) =
            (theme.primary.opacity(0.14), theme.warning.opacity(0.32));
        let headers: HashSet<String> = tab
            .excerpts
            .iter()
            .map(|excerpt| excerpt.header.clone())
            .collect();
        let pattern = tab.pattern.clone();
        for editor in self.editors_showing(uri) {
            let text = editor.read(cx).value().to_string();
            let mut decorations = Vec::new();
            let mut offset = 0;
            for line in text.split('\n') {
                let end = offset + line.len();
                if headers.contains(line) {
                    decorations.push(
                        RangeDecoration::new(offset..(end + 1).min(text.len()))
                            .with_style(RangeDecorationStyle::Fill)
                            .with_color(header_color),
                    );
                } else if let Some(pattern) = &pattern {
                    for found in pattern.find_iter(line).filter(|found| !found.is_empty()) {
                        decorations.push(
                            RangeDecoration::new(offset + found.start()..offset + found.end())
                                .with_style(RangeDecorationStyle::Fill)
                                .with_color(match_color),
                        );
                    }
                }
                offset = end + 1;
            }
            let Some(Backing::Results(tab)) = self
                .documents
                .get_mut(uri)
                .map(|document| &mut document.backing)
            else {
                return;
            };
            let id = editor.entity_id();
            match tab.decor.get(&id) {
                Some(decor) => decor.set(decorations, cx),
                None => {
                    let decor = editor.update(cx, |editor, cx| {
                        editor.create_range_decorations_collection(decorations, cx)
                    });
                    tab.decor.insert(id, decor);
                }
            }
        }
    }

    /// Cmd+S on a results tab: each piece that was typed in goes back into
    /// its file, where the file still has the lines it showed. A file open
    /// with changes of its own gets them in its tab, and saving it stays
    /// yours. A piece that cannot go in stays in the tab as you left it.
    pub(super) fn save_search_results(
        &mut self,
        uri: &str,
        after: Option<AfterConfirm>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(root) = self.project.root.clone() else {
            return;
        };
        let Some(document) = self.documents.get(uri) else {
            return;
        };
        let Backing::Results(tab) = &document.backing else {
            return;
        };
        let text = document.synced_text.clone();
        let mut excerpts = tab.excerpts.clone();
        let edited = edited_pieces(&excerpts, &text);
        let mut by_file: BTreeMap<String, Vec<usize>> = BTreeMap::new();
        let mut skipped = 0;
        for (index, piece) in edited.iter().enumerate() {
            match piece {
                None => skipped += 1,
                Some(lines) if *lines != excerpts[index].shown => by_file
                    .entry(excerpts[index].path.clone())
                    .or_default()
                    .push(index),
                Some(_) => {}
            }
        }
        let mut written = 0;
        for (path, changed) in by_file {
            let file = root.join(&path);
            let Some(current) = self.text_to_change(&file) else {
                skipped += changed.len();
                continue;
            };
            let pieces: Vec<Put> = changed
                .iter()
                .map(|&index| Put {
                    start: excerpts[index].start,
                    original: &excerpts[index].original,
                    new: edited[index].as_deref().unwrap_or_default(),
                })
                .collect();
            let (new_text, put) = with_pieces(&current, &pieces);
            drop(pieces);
            skipped += put.iter().filter(|put| !**put).count();
            let put: Vec<usize> = changed
                .iter()
                .zip(put)
                .filter_map(|(index, put)| put.then_some(*index))
                .collect();
            if put.is_empty() {
                continue;
            }
            if !self.write_pieces(&file, &new_text, window, cx) {
                skipped += put.len();
                continue;
            }
            written += 1;
            // Where the file's pieces start now, from where they started.
            let gains: Vec<(usize, isize)> = put
                .iter()
                .map(|&index| {
                    let new = edited[index].as_ref().map_or(0, Vec::len) as isize;
                    (
                        excerpts[index].start,
                        new - excerpts[index].original.len() as isize,
                    )
                })
                .collect();
            for excerpt in excerpts.iter_mut().filter(|excerpt| excerpt.path == path) {
                let shift: isize = gains
                    .iter()
                    .filter(|(start, _)| *start < excerpt.start)
                    .map(|(_, gain)| gain)
                    .sum();
                excerpt.start = excerpt.start.saturating_add_signed(shift);
            }
            for index in put {
                let lines = edited[index].clone().unwrap_or_default();
                excerpts[index].original = lines.clone();
                excerpts[index].shown = lines;
            }
        }
        let done = skipped == 0;
        // All in: the headers say the lines where the pieces are now. With
        // a piece left out, the tab stays as you typed it.
        let mut rewritten = None;
        if done {
            for excerpt in &mut excerpts {
                excerpt.header = header_of(&excerpt.path, excerpt.start);
            }
            rewritten = Some(results_text(&excerpts));
        }
        if let Some(document) = self.documents.get_mut(uri)
            && let Backing::Results(tab) = &mut document.backing
        {
            tab.excerpts = excerpts;
            if let Some(text) = &rewritten {
                tab.saved = text.clone();
                document.synced_text = text.clone();
                document.modified = false;
            }
        }
        if let Some(text) = &rewritten {
            for editor in self.editors_showing(uri) {
                tabs::replace_text(&editor, text, window, cx);
            }
        }
        self.color_search_results(uri, cx);
        let strings = self.strings();
        let message = if done {
            strings.search_results_saved
        } else {
            strings.search_results_skipped
        };
        self.status_message = Some(super::notices::fill(
            message,
            &[
                ("files", &written.to_string()),
                ("pieces", &skipped.to_string()),
            ],
        ));
        cx.notify();
        if done && let Some(after) = after {
            self.finish(after, window, cx);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::time::Instant;

    use gpui::{TestAppContext, VisualTestContext};

    use super::*;

    /// A project with `files`, and a results tab for `total` in front.
    fn results<'a>(
        cx: &'a mut TestAppContext,
        files: &[(&str, &str)],
        open: &[&str],
    ) -> (
        tempfile::TempDir,
        PathBuf,
        Entity<Workbench>,
        &'a mut VisualTestContext,
    ) {
        cx.executor().allow_parking();
        cx.update(|cx| {
            gpui_component::init(cx);
            crate::workbench_window::init(cx);
        });
        let folder = tempfile::tempdir().unwrap();
        let base = fs::canonicalize(folder.path()).unwrap();
        let root = base.join("app");
        fs::create_dir(&root).unwrap();
        for (name, text) in files {
            fs::write(root.join(name), text).unwrap();
        }
        let settings = Settings::load(base.join("settings.json"), None);
        let opening = Opening::new(Some(root.clone()), None);
        let (view, cx) =
            cx.add_window_view(|window, cx| Workbench::new(opening, settings, None, window, cx));
        for name in open {
            let path = root.join(name);
            view.update_in(cx, |view, window, cx| view.open_file(&path, window, cx));
        }
        view.update_in(cx, |view, window, cx| {
            let query = view.search.query.clone();
            query.update(cx, |query, cx| query.set_value("total", window, cx));
            view.run_search(cx);
        });
        let deadline = Instant::now() + Duration::from_secs(10);
        while view.read_with(cx, |view, _| view.search_results().files.len()) < files.len() {
            assert!(Instant::now() < deadline, "the search did not end");
            cx.executor().advance_clock(Duration::from_millis(300));
            cx.run_until_parked();
            std::thread::sleep(Duration::from_millis(10));
        }
        view.update_in(cx, |view, window, cx| view.open_search_results(window, cx));
        (folder, root, view, cx)
    }

    fn results_uri(view: &Entity<Workbench>, cx: &mut VisualTestContext) -> String {
        view.read_with(cx, |view, _| {
            let uri = view.panes[view.active].uri.clone();
            assert!(uri.starts_with("keel-results:"), "{uri}");
            uri
        })
    }

    fn tab_text(view: &Entity<Workbench>, uri: &str, cx: &mut VisualTestContext) -> String {
        view.read_with(cx, |view, cx| {
            view.editors_showing(uri)[0].read(cx).value().to_string()
        })
    }

    /// Types over the tab's text: `change` says what it becomes.
    fn retype(
        view: &Entity<Workbench>,
        uri: &str,
        cx: &mut VisualTestContext,
        change: impl Fn(String) -> String,
    ) {
        let text = change(tab_text(view, uri, cx));
        view.update_in(cx, |view, window, cx| {
            let editor = view.editors_showing(uri)[0].clone();
            tabs::replace_text(&editor, &text, window, cx);
        });
        cx.run_until_parked();
        assert_eq!(tab_text(view, uri, cx), text);
    }

    fn modified(view: &Entity<Workbench>, uri: &str, cx: &mut VisualTestContext) -> bool {
        view.read_with(cx, |view, _| view.documents[uri].modified)
    }

    #[gpui::test]
    fn what_is_typed_in_the_results_goes_into_the_files(cx: &mut TestAppContext) {
        let (_folder, root, view, cx) = results(
            cx,
            &[
                (
                    "a.txt",
                    "uno\ntotal = 1\ntre\nquattro\ncinque\nsei\ntotal = 2\notto\n",
                ),
                ("b.txt", "prima\r\ntotal\r\ndopo\r\n"),
                ("c.txt", "total senza a capo"),
            ],
            &[],
        );
        let uri = results_uri(&view, cx);
        assert!(!modified(&view, &uri, cx));
        let text = tab_text(&view, &uri, cx);
        assert_eq!(
            text,
            "── a.txt:1 ──\nuno\ntotal = 1\ntre\n\n── a.txt:6 ──\nsei\ntotal = 2\notto\n\n── b.txt:1 ──\nprima\ntotal\ndopo\n\n── c.txt:1 ──\ntotal senza a capo"
        );
        // Saved as it is: no file is written again.
        let before = fs::metadata(root.join("b.txt"))
            .unwrap()
            .modified()
            .unwrap();
        view.update_in(cx, |view, window, cx| view.save(&Save, window, cx));
        assert_eq!(
            fs::metadata(root.join("b.txt"))
                .unwrap()
                .modified()
                .unwrap(),
            before
        );

        retype(&view, &uri, cx, |text| {
            text.replace("total = 1", "total = 1\ntotal = 1.5")
                .replace("total = 2", "somma = 2")
                .replace("prima\ntotal\ndopo", "prima\nTOTALE\ndopo")
                .replace("total senza a capo", "total sempre senza")
        });
        assert!(modified(&view, &uri, cx));
        view.update_in(cx, |view, window, cx| view.save(&Save, window, cx));
        cx.run_until_parked();
        assert_eq!(
            fs::read_to_string(root.join("a.txt")).unwrap(),
            "uno\ntotal = 1\ntotal = 1.5\ntre\nquattro\ncinque\nsei\nsomma = 2\notto\n"
        );
        assert_eq!(
            fs::read_to_string(root.join("b.txt")).unwrap(),
            "prima\r\nTOTALE\r\ndopo\r\n"
        );
        assert_eq!(
            fs::read_to_string(root.join("c.txt")).unwrap(),
            "total sempre senza"
        );
        // The header of the piece below says its new line, and it saves
        // again from there.
        assert!(!modified(&view, &uri, cx));
        assert!(tab_text(&view, &uri, cx).contains("── a.txt:7 ──\nsei\nsomma = 2"));
        // F12 on a piece's line: the file, at that line.
        view.update_in(cx, |view, window, cx| {
            let editor = view.editors_showing(&uri)[0].clone();
            let row = editor
                .read(cx)
                .value()
                .lines()
                .position(|line| line == "somma = 2")
                .unwrap();
            editor.update(cx, |editor, cx| {
                editor.set_cursor_position(
                    gpui_component::input::Position::new(row as u32, 3),
                    window,
                    cx,
                )
            });
            assert!(view.open_result_under_cursor(window, cx));
            let pane = &view.panes[view.active];
            assert!(pane.uri.ends_with("a.txt"));
            let cursor = pane.editor.read(cx).cursor_position();
            assert_eq!((cursor.line, cursor.character), (7, 3));
        });
        view.update_in(cx, |view, window, cx| {
            let (group, tab) = view.find_tab(&uri, 0).unwrap();
            view.activate_tab(group, tab, true, window, cx);
        });
        retype(&view, &uri, cx, |text| {
            text.replace("somma = 2", "somma = 3")
        });
        view.update_in(cx, |view, window, cx| view.save(&Save, window, cx));
        assert!(
            fs::read_to_string(root.join("a.txt"))
                .unwrap()
                .ends_with("sei\nsomma = 3\notto\n")
        );
    }

    #[gpui::test]
    fn nothing_written_meanwhile_is_lost(cx: &mut TestAppContext) {
        let (_folder, root, view, cx) = results(
            cx,
            &[
                ("a.txt", "uno\ntotal\ntre\n"),
                ("b.txt", "uno\ntotal\ntre\n"),
                ("c.txt", "uno\ntotal\ntre\nquattro\ncinque\n"),
            ],
            &["c.txt"],
        );
        let uri = results_uri(&view, cx);
        // b.txt changes on disk, on the very line; c.txt is being written
        // in its own tab, further down.
        fs::write(root.join("b.txt"), "uno\ntotal, da un'altra parte\ntre\n").unwrap();
        let c = Uri::file(&root.join("c.txt")).to_string();
        view.update_in(cx, |view, window, cx| {
            view.set_document_text(&c, "uno\ntotal\ntre\nquattro\nCINQUE\n", window, cx)
        });
        retype(&view, &uri, cx, |text| text.replace("total", "SOMMA"));
        view.update_in(cx, |view, window, cx| view.save(&Save, window, cx));
        cx.run_until_parked();

        assert_eq!(
            fs::read_to_string(root.join("a.txt")).unwrap(),
            "uno\nSOMMA\ntre\n"
        );
        assert_eq!(
            fs::read_to_string(root.join("b.txt")).unwrap(),
            "uno\ntotal, da un'altra parte\ntre\n",
            "a file changed outside is left alone"
        );
        // c.txt took the piece in its tab, with the changes there; its
        // save stays the user's.
        assert_eq!(
            fs::read_to_string(root.join("c.txt")).unwrap(),
            "uno\ntotal\ntre\nquattro\ncinque\n"
        );
        view.read_with(cx, |view, _| {
            assert_eq!(
                view.documents[&c].synced_text,
                "uno\nSOMMA\ntre\nquattro\nCINQUE\n"
            );
            assert!(view.documents[&c].modified);
        });
        // What could not go into b.txt is still in the tab, unsaved, and
        // closing the tab asks.
        assert!(modified(&view, &uri, cx));
        assert!(tab_text(&view, &uri, cx).contains("── b.txt:1 ──\nuno\nSOMMA\ntre"));
        // Again: only b.txt is left to write, and still refuses.
        view.update_in(cx, |view, window, cx| view.save(&Save, window, cx));
        assert_eq!(
            fs::read_to_string(root.join("a.txt")).unwrap(),
            "uno\nSOMMA\ntre\n"
        );
        // b.txt back as it was: now it goes in, and the tab is saved.
        fs::write(root.join("b.txt"), "uno\ntotal\ntre\n").unwrap();
        view.update_in(cx, |view, window, cx| view.save(&Save, window, cx));
        assert_eq!(
            fs::read_to_string(root.join("b.txt")).unwrap(),
            "uno\nSOMMA\ntre\n"
        );
        assert!(!modified(&view, &uri, cx));
    }

    #[test]
    fn pieces_around_the_matches_join_when_they_touch() {
        let text = "uno\ndue\ntre\nquattro\ncinque\nsei\nsette\notto\nnove\n";
        let pieces = excerpts_of("a.txt", text, &[1, 3, 7]);
        assert_eq!(pieces.len(), 2);
        assert_eq!((pieces[0].start, pieces[0].original.len()), (0, 5));
        assert_eq!(
            (pieces[1].start, pieces[1].original.clone()),
            (6, vec!["sette".into(), "otto".into(), "nove".into()])
        );
        let tab = results_text(&pieces);
        assert_eq!(
            tab,
            "── a.txt:1 ──\nuno\ndue\ntre\nquattro\ncinque\n\n── a.txt:7 ──\nsette\notto\nnove"
        );
        // Read back as they are.
        let read = edited_pieces(&pieces, &tab);
        assert_eq!(read[0].as_ref(), Some(&pieces[0].original));
        assert_eq!(read[1].as_ref(), Some(&pieces[1].original));
        // A line added in the first piece.
        let edited = tab.replace("tre\n", "tre\ntre e mezzo\n");
        let read = edited_pieces(&pieces, &edited);
        assert_eq!(read[0].as_ref().unwrap().len(), 6);
        assert_eq!(read[1].as_ref(), Some(&pieces[1].original));
        // The second header changed: neither piece knows its lines now.
        let read = edited_pieces(&pieces, &tab.replace("a.txt:7", "b.txt:7"));
        assert_eq!(read, [None, None]);
        // The first header changed: the second piece is still sure.
        let read = edited_pieces(&pieces, &tab.replace("a.txt:1", "a.txt:2"));
        assert_eq!(read[0], None);
        assert_eq!(read[1].as_ref(), Some(&pieces[1].original));
    }
}
