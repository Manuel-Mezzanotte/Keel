//! Writing, as in VS Code: brackets coloured by depth; the other places of
//! the word under the cursor highlighted; errors, changes and those places
//! marked along the scrollbar; a selection surrounded by the quote or
//! bracket typed over it; a tag's partner renamed with it; and the commands
//! on lines and words: sort, unique, join, case, matching bracket, expand
//! and shrink the selection.

use std::collections::HashMap;
use std::ops::Range;

use editor::text_edits::{self, Case};
use gpui::Div;
use gpui_component::input::{
    OverviewRulerDecoration, OverviewRulerDecorationCollection, OverviewRulerLane, RangeDecoration,
    RangeDecorationCollection, RangeDecorationStyle,
};

use super::*;

actions!(
    keel,
    [
        SortLinesAscending,
        SortLinesDescending,
        DeleteDuplicateLines,
        JoinLines,
        TransformToUppercase,
        TransformToLowercase,
        TransformToTitlecase,
        TransformToSnakecase,
        TransformToCamelcase,
        TransformToKebabcase,
        JumpToBracket,
        ExpandSelection,
        ShrinkSelection
    ]
);

/// Bracket colours, as VS Code's defaults for dark and light themes.
const DARK_BRACKETS: [u32; 3] = [0xFFD700, 0xDA70D6, 0x179FFF];
const LIGHT_BRACKETS: [u32; 3] = [0x0431FA, 0x319331, 0x7B3814];
/// Files larger than this are not coloured nor searched for the word.
const LARGEST: usize = 600_000;
/// How long after typing or moving the decorations follow.
const PAUSE: Duration = Duration::from_millis(160);

/// What one editor shows on top of its text.
#[derive(Default)]
pub(super) struct Decor {
    brackets: Option<TextDecorationCollection>,
    occurrences: Option<TextDecorationCollection>,
    ruler: Option<OverviewRulerDecorationCollection>,
    /// The occurrences found last, for the scrollbar.
    places: Vec<Range<usize>>,
    /// The bookmarked lines, lit, and their places for the scrollbar.
    bookmarks: Option<RangeDecorationCollection>,
    bookmarked: Vec<Range<usize>>,
    task: Option<Task<()>>,
    /// Expand Selection's steps, to shrink back through them.
    pub(super) grown: Vec<Range<usize>>,
    /// Renaming a tag's partner: the next change is Keel's own.
    mirroring: bool,
    /// The text before the last change, to see what it changed.
    before: Option<String>,
}

pub(super) type DecorMap = HashMap<String, Decor>;

fn on(settings: &Settings, key: &str, default: bool) -> bool {
    settings
        .get(key)
        .and_then(Value::as_bool)
        .unwrap_or(default)
}

impl Workbench {
    pub(super) fn writing_actions(&self, root: Div, cx: &mut Context<Self>) -> Div {
        let lines = |root: Div, cx: &mut Context<Self>| {
            root.on_action(cx.listener(|this, _: &SortLinesAscending, window, cx| {
                this.edit_lines(
                    |text, selection, _| text_edits::sort_lines(text, selection, false),
                    window,
                    cx,
                )
            }))
            .on_action(cx.listener(|this, _: &SortLinesDescending, window, cx| {
                this.edit_lines(
                    |text, selection, _| text_edits::sort_lines(text, selection, true),
                    window,
                    cx,
                )
            }))
            .on_action(cx.listener(|this, _: &DeleteDuplicateLines, window, cx| {
                this.edit_lines(
                    |text, selection, _| text_edits::unique_lines(text, selection),
                    window,
                    cx,
                )
            }))
            .on_action(cx.listener(|this, _: &JoinLines, window, cx| {
                this.edit_lines(
                    |text, selection, _| text_edits::join_lines(text, selection),
                    window,
                    cx,
                )
            }))
        };
        let case = |root: Div, cx: &mut Context<Self>| {
            let transform = |case: Case| {
                move |this: &mut Workbench, window: &mut Window, cx: &mut Context<Workbench>| {
                    this.edit_lines(
                        move |text, selection, _| text_edits::transform_case(text, selection, case),
                        window,
                        cx,
                    )
                }
            };
            root.on_action(
                cx.listener(move |this, _: &TransformToUppercase, window, cx| {
                    transform(Case::Upper)(this, window, cx)
                }),
            )
            .on_action(
                cx.listener(move |this, _: &TransformToLowercase, window, cx| {
                    transform(Case::Lower)(this, window, cx)
                }),
            )
            .on_action(
                cx.listener(move |this, _: &TransformToTitlecase, window, cx| {
                    transform(Case::Title)(this, window, cx)
                }),
            )
            .on_action(
                cx.listener(move |this, _: &TransformToSnakecase, window, cx| {
                    transform(Case::Snake)(this, window, cx)
                }),
            )
            .on_action(
                cx.listener(move |this, _: &TransformToCamelcase, window, cx| {
                    transform(Case::Camel)(this, window, cx)
                }),
            )
            .on_action(cx.listener(
                move |this, _: &TransformToKebabcase, window, cx| {
                    transform(Case::Kebab)(this, window, cx)
                },
            ))
        };
        let root = lines(root, cx);
        let root = case(root, cx);
        root.on_action(
            cx.listener(|this, _: &JumpToBracket, window, cx| this.jump_to_bracket(window, cx)),
        )
        .on_action(cx.listener(|this, _: &ExpandSelection, _, cx| this.grow_selection(true, cx)))
        .on_action(cx.listener(|this, _: &ShrinkSelection, _, cx| this.grow_selection(false, cx)))
    }

    fn active_text(&self, cx: &App) -> Option<(Entity<EditorState>, String, Range<usize>, String)> {
        let pane = self.panes.get(self.active)?;
        let editor = pane.editor.clone();
        let language = self
            .documents
            .get(&pane.uri)
            .map(|document| document.language_id.clone())
            .unwrap_or_default();
        let (text, selection) = editor.read_with(cx, |editor, _| {
            (editor.value().to_string(), editor.selected_range())
        });
        Some((editor, text, selection, language))
    }

    /// Cmd+Shift+\: to the bracket matching the one at the cursor.
    fn jump_to_bracket(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some((editor, text, selection, language)) = self.active_text(cx) else {
            return;
        };
        if let Some(target) = text_edits::matching_bracket(&text, selection.end, &language) {
            editor.update(cx, |editor, cx| {
                editor.set_selected_range(target..target, cx);
                let _ = window;
            });
        }
    }

    /// Ctrl+Shift+Cmd+→ grows the selection, ← takes it back a step.
    fn grow_selection(&mut self, grow: bool, cx: &mut Context<Self>) {
        let Some((editor, text, selection, _)) = self.active_text(cx) else {
            return;
        };
        let id = self.panes[self.active].editor_id.clone();
        let decor = self.decor.entry(id).or_default();
        let target = if grow {
            // A selection made otherwise starts the steps again.
            if decor.grown.last() != Some(&selection) {
                decor.grown.clear();
            }
            decor.grown.push(selection.clone());
            let grown = text_edits::expand_selection(&text, selection);
            decor.grown.push(grown.clone());
            grown
        } else {
            if decor.grown.last() == Some(&selection) {
                decor.grown.pop();
            }
            match decor.grown.pop() {
                Some(previous) => previous,
                None => return,
            }
        };
        editor.update(cx, |editor, cx| editor.set_selected_range(target, cx));
    }

    /// Typing a quote or a bracket over a selection surrounds it.
    pub(super) fn surround_selection(
        &mut self,
        key: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.settings.string("editor.autoSurround") == Some("never") {
            return false;
        }
        let mut chars = key.chars();
        let (Some(typed), None) = (chars.next(), chars.next()) else {
            return false;
        };
        let Some((editor, text, selection, _)) = self.active_text(cx) else {
            return false;
        };
        let several = editor.read(cx).selected_ranges().len() > 1;
        if several {
            return false;
        }
        let Some(edit) = text_edits::surround(&text, selection, typed) else {
            return false;
        };
        editor.update(cx, |editor, cx| {
            editor.set_selected_range(edit.range.clone(), cx);
            editor.replace(edit.text.clone(), window, cx);
            editor.set_selected_range(edit.selection.clone(), cx);
        });
        true
    }

    // -----------------------------------------------------------------
    // Decorations

    /// A new editor: its collections, then its first colours.
    pub(super) fn start_decor(
        &mut self,
        editor_id: &str,
        editor: &Entity<EditorState>,
        cx: &mut Context<Self>,
    ) {
        let (brackets, occurrences, ruler, bookmarks) = editor.update(cx, |editor, cx| {
            (
                editor.create_decorations_collection(Vec::new(), cx),
                editor.create_decorations_collection(Vec::new(), cx),
                editor.create_overview_ruler_decorations_collection(Vec::new(), cx),
                editor.create_range_decorations_collection(Vec::new(), cx),
            )
        });
        let text = editor.read(cx).value().to_string();
        let decor = self.decor.entry(editor_id.to_owned()).or_default();
        decor.before = Some(text);
        decor.brackets = Some(brackets);
        decor.occurrences = Some(occurrences);
        decor.ruler = Some(ruler);
        decor.bookmarks = Some(bookmarks);
        self.redecorate(editor_id, editor, false, cx);
    }

    /// The text or the cursor changed: brackets, the word's places and the
    /// scrollbar's marks follow after a short pause.
    pub(super) fn redecorate(
        &mut self,
        editor_id: &str,
        editor: &Entity<EditorState>,
        pause: bool,
        cx: &mut Context<Self>,
    ) {
        let Some(decor) = self.decor.get_mut(editor_id) else {
            return;
        };
        let editor = editor.downgrade();
        let id = editor_id.to_owned();
        decor.task = Some(cx.spawn(async move |this, cx| {
            if pause {
                cx.background_executor().timer(PAUSE).await;
            }
            this.update(cx, |this, cx| {
                if let Some(editor) = editor.upgrade() {
                    this.apply_decor(&id, &editor, cx);
                }
            })
            .ok();
        }));
    }

    fn apply_decor(
        &mut self,
        editor_id: &str,
        editor: &Entity<EditorState>,
        cx: &mut Context<Self>,
    ) {
        let (text, selection) = editor.read_with(cx, |editor, _| {
            (editor.value().to_string(), editor.selected_range())
        });
        if text.len() > LARGEST {
            return;
        }
        let language = self
            .panes
            .iter()
            .chain(
                self.groups
                    .iter()
                    .flat_map(|group| group.tabs.iter().filter_map(|tab| tab.parked.as_ref())),
            )
            .find(|pane| pane.editor_id == editor_id)
            .and_then(|pane| self.documents.get(&pane.uri))
            .map(|document| document.language_id.clone())
            .unwrap_or_default();
        let dark = cx.theme().mode.is_dark();
        let colours = if dark { DARK_BRACKETS } else { LIGHT_BRACKETS };
        let brackets: Vec<TextDecoration> =
            if on(
                &self.settings,
                "editor.bracketPairColorization.enabled",
                true,
            ) && !matches!(language.as_str(), "plaintext" | "markdown" | "")
            {
                text_edits::bracket_depths(&text, &language)
                    .into_iter()
                    .map(|(at, depth)| {
                        TextDecoration::new(
                            at..at + 1,
                            HighlightStyle {
                                color: Some(gpui::rgb(colours[depth % 3]).into()),
                                ..Default::default()
                            },
                        )
                    })
                    .collect()
            } else {
                Vec::new()
            };
        // The word under the cursor, elsewhere in the file.
        let places =
            if on(&self.settings, "editor.occurrencesHighlight", true) && selection.is_empty() {
                let word = editor::text_edits::word_at(&text, selection.start);
                let word = &text[word];
                if word
                    .chars()
                    .next()
                    .is_some_and(|c| c.is_alphabetic() || c == '_')
                {
                    text_edits::occurrences(&text, word, 400)
                } else {
                    Vec::new()
                }
            } else {
                Vec::new()
            };
        let highlight = cx.theme().accent.opacity(if dark { 0.16 } else { 0.12 });
        let occurrences: Vec<TextDecoration> = if places.len() > 1 {
            places
                .iter()
                .map(|range| {
                    TextDecoration::new(
                        range.clone(),
                        HighlightStyle {
                            background_color: Some(highlight),
                            ..Default::default()
                        },
                    )
                })
                .collect()
        } else {
            Vec::new()
        };
        let Some(decor) = self.decor.get_mut(editor_id) else {
            return;
        };
        if let Some(collection) = &decor.brackets {
            collection.set(brackets, cx);
        }
        if let Some(collection) = &decor.occurrences {
            collection.set(occurrences, cx);
        }
        decor.places = if places.len() > 1 { places } else { Vec::new() };
        // The bookmarks, following their lines.
        let path = self
            .panes
            .iter()
            .find(|pane| pane.editor_id == editor_id)
            .and_then(|pane| self.documents.get(&pane.uri))
            .and_then(|document| document.disk_path().map(Path::to_path_buf));
        let bookmarked = match path {
            Some(path) if !self.bookmarks.is_empty() => self.bookmark_ranges(&path, &text),
            _ => Vec::new(),
        };
        let lit = cx.theme().info.opacity(0.13);
        if let Some(decor) = self.decor.get_mut(editor_id) {
            if let Some(collection) = &decor.bookmarks {
                let lines = bookmarked
                    .iter()
                    .map(|range| {
                        RangeDecoration::new(range.clone())
                            .with_style(RangeDecorationStyle::Fill)
                            .with_color(lit)
                    })
                    .collect();
                collection.set(lines, cx);
            }
            decor.bookmarked = bookmarked;
        }
        self.refresh_ruler(editor_id, editor, cx);
    }

    /// The marks along the scrollbar: errors and warnings at the right,
    /// git's changes at the left, the word's places in the middle.
    pub(super) fn refresh_ruler(
        &self,
        editor_id: &str,
        editor: &Entity<EditorState>,
        cx: &mut App,
    ) {
        let Some(decor) = self.decor.get(editor_id) else {
            return;
        };
        let Some(ruler) = &decor.ruler else {
            return;
        };
        let pane = self.panes.iter().find(|pane| pane.editor_id == editor_id);
        let theme = cx.theme();
        let (danger, warning, success, info, accent) = (
            theme.danger,
            theme.warning,
            theme.success,
            theme.info,
            theme.accent,
        );
        let text = editor.read(cx).value().to_string();
        let lines = super::problems::LineIndex::new(&text);
        let line_start = |line: usize| -> usize {
            text.split_inclusive('\n')
                .take(line)
                .map(str::len)
                .sum::<usize>()
                .min(text.len())
        };
        let mut marks: Vec<OverviewRulerDecoration> = Vec::new();
        if let Some(pane) = pane {
            for diagnostic in self
                .languages
                .diagnostics
                .get(&pane.uri)
                .into_iter()
                .flat_map(|by_server| by_server.values().flatten())
            {
                let color = match diagnostic.severity {
                    Some(lsp_types::DiagnosticSeverity::ERROR) => danger,
                    Some(lsp_types::DiagnosticSeverity::WARNING) => warning,
                    _ => continue,
                };
                let start = lines.offset(diagnostic.range.start);
                let end = lines
                    .offset(diagnostic.range.end)
                    .max(start + 1)
                    .min(text.len());
                marks.push(
                    OverviewRulerDecoration::new(start..end)
                        .with_lane(OverviewRulerLane::Right)
                        .with_color(color),
                );
            }
            for hunk in &pane.hunks {
                let color = match hunk.kind() {
                    git::HunkKind::Added => success,
                    git::HunkKind::Modified => info,
                    git::HunkKind::Deleted => danger,
                };
                let start = line_start(hunk.new.start);
                let end = line_start(hunk.new.end.max(hunk.new.start + 1))
                    .max(start + 1)
                    .min(text.len());
                marks.push(
                    OverviewRulerDecoration::new(start..end)
                        .with_lane(OverviewRulerLane::Left)
                        .with_color(color),
                );
            }
        }
        marks.extend(decor.places.iter().map(|range| {
            OverviewRulerDecoration::new(range.clone())
                .with_lane(OverviewRulerLane::Center)
                .with_color(accent.opacity(0.8))
        }));
        marks.extend(decor.bookmarked.iter().map(|range| {
            OverviewRulerDecoration::new(range.start..range.end.max(range.start + 1))
                .with_lane(OverviewRulerLane::Right)
                .with_color(info)
        }));
        ruler.set(marks, cx);
    }

    /// After a change: when it renamed a tag, its partner follows, in
    /// HTML, JSX, Vue and XML files.
    pub(super) fn mirror_tag(
        &mut self,
        uri: &str,
        editor_id: &str,
        editor: &Entity<EditorState>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let language = self
            .documents
            .get(uri)
            .map(|document| document.language_id.clone())
            .unwrap_or_default();
        let tags = matches!(
            language.as_str(),
            "html" | "xml" | "vue" | "svelte" | "javascriptreact" | "typescriptreact" | "astro"
        );
        let new = editor.read(cx).value().to_string();
        let Some(decor) = self.decor.get_mut(editor_id) else {
            return;
        };
        let before = decor.before.replace(new.clone());
        if !tags
            || std::mem::take(&mut decor.mirroring)
            || !on(&self.settings, "editor.linkedEditing", true)
        {
            return;
        }
        let Some(old) = before else {
            return;
        };
        if new.len().abs_diff(old.len()) > 64 {
            return;
        }
        let Some((range, name)) = text_edits::paired_tag_edit(&old, &new) else {
            return;
        };
        // The partner's place in the new text: after the change, it moved
        // by what the change added.
        let grown = new.len() as isize - old.len() as isize;
        let changed_at = old
            .bytes()
            .zip(new.bytes())
            .take_while(|(a, b)| a == b)
            .count();
        let shift = if range.start > changed_at { grown } else { 0 };
        let start = (range.start as isize + shift) as usize;
        let end = (range.end as isize + shift) as usize;
        if new.get(start..end).is_none() {
            return;
        }
        decor.mirroring = true;
        editor.update(cx, |editor, cx| {
            let cursor = editor.selected_range();
            editor.set_selected_range(start..end, cx);
            editor.replace(name.clone(), window, cx);
            // The cursor stays where it was typing, moved when the partner
            // was before it.
            let moved = |offset: usize| {
                if start < offset {
                    (offset as isize + name.len() as isize - (end - start) as isize) as usize
                } else {
                    offset
                }
            };
            editor.set_selected_range(moved(cursor.start)..moved(cursor.end), cx);
        });
    }

    /// An editor closed: its decorations go with it.
    pub(super) fn forget_decor(&mut self, editor_id: &str) {
        self.decor.remove(editor_id);
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use gpui::{TestAppContext, VisualTestContext};

    use super::*;

    fn text(view: &Entity<Workbench>, cx: &mut VisualTestContext) -> String {
        view.read_with(cx, |view, cx| {
            view.active_editor().unwrap().read(cx).value().to_string()
        })
    }

    fn select(view: &Entity<Workbench>, cx: &mut VisualTestContext, range: Range<usize>) {
        view.update(cx, |view, cx| {
            let editor = view.active_editor().unwrap();
            editor.update(cx, |editor, cx| editor.set_selected_range(range, cx));
        });
    }

    fn selected(view: &Entity<Workbench>, cx: &mut VisualTestContext) -> Range<usize> {
        view.read_with(cx, |view, cx| {
            view.active_editor().unwrap().read(cx).selected_range()
        })
    }

    #[gpui::test]
    fn writing_as_in_vs_code(cx: &mut TestAppContext) {
        cx.update(|cx| {
            gpui_component::init(cx);
            language::register_grammars();
            crate::workbench_window::init(cx);
        });
        let folder = tempfile::tempdir().unwrap();
        let root = fs::canonicalize(folder.path()).unwrap();
        fs::write(
            root.join("a.ts"),
            "call(\"hello world\", other);\nlet total = total + 1;\n",
        )
        .unwrap();
        fs::write(root.join("card.tsx"), "const a = <div><b>x</b></div>;\n").unwrap();
        let settings_file = root.join(".settings.json");
        fs::write(&settings_file, r#"{ "keel.language": "en" }"#).unwrap();
        let settings = Settings::load(&settings_file, None);
        let opening = Opening::new(Some(root.clone()), None);
        let (view, cx) =
            cx.add_window_view(|window, cx| Workbench::new(opening, settings, None, window, cx));
        view.update_in(cx, |view, window, cx| {
            view.open_file(&root.join("a.ts"), window, cx)
        });
        cx.update(|window, _| window.activate_window());
        cx.run_until_parked();

        // Expand Selection: the word, inside the quotes, with them.
        let at = text(&view, cx).find("world").unwrap() + 1;
        select(&view, cx, at..at);
        cx.simulate_keystrokes("ctrl-shift-cmd-right");
        assert_eq!(&text(&view, cx)[selected(&view, cx)], "world");
        cx.simulate_keystrokes("ctrl-shift-cmd-right");
        assert_eq!(&text(&view, cx)[selected(&view, cx)], "hello world");
        cx.simulate_keystrokes("ctrl-shift-cmd-left");
        assert_eq!(&text(&view, cx)[selected(&view, cx)], "world");

        // A quote typed over a selection surrounds it.
        cx.simulate_keystrokes("'");
        assert!(
            text(&view, cx).starts_with("call(\"hello 'world'\","),
            "{}",
            text(&view, cx)
        );
        cx.simulate_keystrokes("cmd-z");

        // The matching bracket.
        select(&view, cx, 4..4);
        cx.simulate_keystrokes("cmd-shift-\\");
        let close = text(&view, cx).find(')').unwrap();
        assert_eq!(selected(&view, cx), close..close);

        // The word's other places are found.
        let total = text(&view, cx).find("total").unwrap();
        select(&view, cx, total..total);
        cx.executor().advance_clock(Duration::from_millis(400));
        cx.run_until_parked();
        let places = view.read_with(cx, |view, _| {
            let id = &view.panes[view.active].editor_id;
            view.decor.get(id).map(|decor| decor.places.len())
        });
        assert_eq!(places, Some(2));

        // Cmd+C with nothing selected takes the line; Cmd+V puts it above.
        let line_two = text(&view, cx).find("let").unwrap();
        select(&view, cx, line_two + 2..line_two + 2);
        cx.simulate_keystrokes("cmd-c");
        cx.simulate_keystrokes("cmd-v");
        assert_eq!(
            text(&view, cx),
            "call(\"hello world\", other);\nlet total = total + 1;\nlet total = total + 1;\n"
        );

        // Join Lines.
        select(&view, cx, 0..0);
        cx.simulate_keystrokes("ctrl-j");
        assert!(text(&view, cx).starts_with("call(\"hello world\", other); let total"));

        // Renaming a tag renames its closing partner.
        view.update_in(cx, |view, window, cx| {
            view.open_file(&root.join("card.tsx"), window, cx)
        });
        cx.run_until_parked();
        let at = text(&view, cx).find("div").unwrap() + 3;
        select(&view, cx, at..at);
        cx.simulate_input("x");
        cx.run_until_parked();
        assert_eq!(text(&view, cx), "const a = <divx><b>x</b></divx>;\n");
    }
}
