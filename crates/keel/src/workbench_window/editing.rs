//! Editing commands as in VS Code: comments, moving, copying, deleting and
//! inserting lines, tags that close themselves, word wrap and zoom.

use std::ops::Range;

use editor::line_edits::{self, LineEdit};

use super::*;

/// The key context of the editors, for commands that only make sense
/// while typing, like VS Code's `editorTextFocus`.
pub const EDITOR_CONTEXT: &str = "KeelEditor";

actions!(
    keel,
    [
        ToggleLineComment,
        ToggleBlockComment,
        MoveLinesUp,
        MoveLinesDown,
        CopyLinesUp,
        CopyLinesDown,
        DeleteLines,
        InsertLineAfter,
        InsertLineBefore,
        ToggleWordWrap,
        ZoomIn,
        ZoomOut,
        ZoomReset,
        Fold,
        Unfold,
        FoldAll,
        UnfoldAll
    ]
);

/// The editor's font size from `editor.fontSize`, 14 when unset.
pub(super) fn font_size_setting(settings: &Settings) -> f32 {
    settings
        .number("editor.fontSize")
        .map_or(14., |size| size.clamp(6., 72.) as f32)
}

impl Workbench {
    /// Runs a line command on the active editor as one undoable edit, at
    /// every cursor: cursors on the same or neighboring lines act as one, as
    /// in VS Code.
    fn edit_lines(
        &mut self,
        command: impl Fn(&str, Range<usize>, &str) -> Option<LineEdit>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(pane) = self.panes.get(self.active) else {
            return;
        };
        let editor = pane.editor.clone();
        let language = self
            .documents
            .get(&pane.uri)
            .map(|document| document.language_id.clone())
            .unwrap_or_default();
        let (text, selection, all) = editor.read_with(cx, |editor, _| {
            (
                editor.value().to_string(),
                editor.selected_range(),
                editor.selected_ranges(),
            )
        });
        if all.len() <= 1 {
            let Some(edit) = command(&text, selection, &language) else {
                return;
            };
            editor.update(cx, |editor, cx| {
                editor.set_selected_range(edit.range.clone(), cx);
                editor.replace(edit.text.clone(), window, cx);
                editor.set_selected_range(edit.selection.clone(), cx);
            });
        } else {
            let ranges: Vec<Range<usize>> = all.into_iter().map(|(range, _)| range).collect();
            let Some((span, replacement, selections)) =
                line_edits::at_every_cursor(&text, &ranges, |text, selection| {
                    command(text, selection, &language)
                })
            else {
                return;
            };
            editor.update(cx, |editor, cx| {
                editor.set_selected_range(span, cx);
                editor.replace(replacement, window, cx);
                editor.set_selected_ranges(selections, cx);
            });
        }
        self.publish_selection_if_changed(self.active, cx);
    }

    /// Folds or unfolds at the cursor, or everywhere, as VS Code's
    /// Cmd+Alt+[ and ], Cmd+K Cmd+0 and Cmd+K Cmd+J.
    fn fold(&mut self, fold: Option<bool>, all: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(editor) = self.active_editor() else {
            return;
        };
        editor.update(cx, |editor, cx| {
            let line = editor.cursor_position().line as usize;
            let candidates = editor.display_map().fold_candidates().to_vec();
            let folded: Vec<usize> = editor
                .display_map()
                .folded_ranges()
                .iter()
                .map(|range| range.start_line)
                .collect();
            let mut moved_to = None;
            match (fold, all) {
                (Some(true), true) => {
                    for candidate in &candidates {
                        editor.set_folded_at_line(candidate.start_line, true, cx);
                    }
                    // The cursor goes to the outermost fold hiding it.
                    moved_to = candidates
                        .iter()
                        .filter(|range| range.start_line < line && line <= range.end_line)
                        .map(|range| range.start_line)
                        .min();
                }
                (_, true) => editor.unfold_all(cx),
                (Some(true), false) => {
                    // The innermost block around the cursor that is open.
                    if let Some(range) = candidates
                        .iter()
                        .filter(|range| {
                            range.start_line <= line
                                && line <= range.end_line
                                && !folded.contains(&range.start_line)
                        })
                        .min_by_key(|range| range.end_line - range.start_line)
                    {
                        editor.set_folded_at_line(range.start_line, true, cx);
                        moved_to = (range.start_line != line).then_some(range.start_line);
                    }
                }
                _ => {
                    if let Some(range) = editor
                        .display_map()
                        .folded_ranges()
                        .iter()
                        .filter(|range| range.start_line <= line && line <= range.end_line)
                        .min_by_key(|range| range.end_line - range.start_line)
                        .cloned()
                    {
                        editor.set_folded_at_line(range.start_line, false, cx);
                    }
                }
            }
            if let Some(start) = moved_to {
                let position = gpui_component::input::Position::new(start as u32, 0);
                editor.set_cursor_position(position, window, cx);
            }
        });
    }

    /// After typing `>` in JSX or HTML, adds the closing tag after the
    /// cursor. Returns whether it did, so the key is not typed again.
    pub(super) fn close_tag(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let Some(pane) = self.panes.get(self.active) else {
            return false;
        };
        let Some(document) = self.documents.get(&pane.uri) else {
            return false;
        };
        // Extensions' own documents, like VSAuto Ghost's, type for themselves.
        if matches!(document.backing, Backing::Extension) {
            return false;
        }
        let language = document.language_id.clone();
        let editor = pane.editor.clone();
        let (text, selection) = editor.read_with(cx, |editor, _| {
            (editor.value().to_string(), editor.selected_range())
        });
        if !selection.is_empty() {
            return false;
        }
        let mut from = selection.start.saturating_sub(2000);
        while !text.is_char_boundary(from) {
            from += 1;
        }
        let Some(closing) = line_edits::closing_tag(&text[from..selection.start], &language) else {
            return false;
        };
        editor.update(cx, |editor, cx| {
            editor.replace(format!(">{closing}"), window, cx);
            let cursor = selection.start + 1;
            editor.set_selected_range(cursor..cursor, cx);
        });
        true
    }

    fn set_word_wrap(&mut self, wrap: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.word_wrap = wrap;
        let editors: Vec<Entity<EditorState>> = self
            .panes
            .iter()
            .map(|pane| pane.editor.clone())
            .chain(
                self.groups
                    .iter()
                    .flat_map(|group| &group.tabs)
                    .filter_map(|tab| tab.parked.as_ref().map(|pane| pane.editor.clone())),
            )
            .collect();
        for editor in editors {
            editor.update(cx, |editor, cx| editor.set_soft_wrap(wrap, window, cx));
        }
        cx.notify();
    }

    pub(super) fn zoom(&mut self, step: f32, cx: &mut Context<Self>) {
        // On a PDF or an image the zoom keys zoom it.
        if let Some(uri) = self.active_pdf() {
            let step = (step != 0.).then_some(step > 0.);
            self.zoom_pdf(&uri, step, cx);
            return;
        }
        if let Some((uri, _)) = self.active_image() {
            let step = (step != 0.).then_some(step > 0.);
            self.zoom_image(&uri, step, cx);
            return;
        }
        self.font_size = if step == 0. {
            font_size_setting(&self.settings)
        } else {
            (self.font_size + step).clamp(6., 72.)
        };
        // Sticky lines follow the editor's size.
        let font_size = self.font_size;
        let sticky: Vec<_> = self
            .panes
            .iter()
            .filter_map(|pane| pane.sticky.clone())
            .chain(
                self.groups
                    .iter()
                    .flat_map(|group| &group.tabs)
                    .filter_map(|tab| tab.parked.as_ref().and_then(|pane| pane.sticky.clone())),
            )
            .collect();
        for sticky in sticky {
            sticky.update(cx, |sticky, cx| sticky.set_font_size(font_size, cx));
        }
        cx.notify();
    }

    /// The handlers of the editing commands, for the window's root.
    pub(super) fn editing_actions(&self, root: gpui::Div, cx: &mut Context<Self>) -> gpui::Div {
        root.on_action(cx.listener(|this, _: &ToggleLineComment, window, cx| {
            let languages = this.extension_languages.clone();
            this.edit_lines(
                move |text, selection, language| {
                    let syntax = Workbench::comment_syntax_for(&languages, language);
                    line_edits::toggle_line_comment(text, selection, syntax)
                },
                window,
                cx,
            )
        }))
        .on_action(cx.listener(|this, _: &ToggleBlockComment, window, cx| {
            let languages = this.extension_languages.clone();
            this.edit_lines(
                move |text, selection, language| {
                    let syntax = Workbench::comment_syntax_for(&languages, language);
                    line_edits::toggle_block_comment(text, selection, syntax)
                },
                window,
                cx,
            )
        }))
        .on_action(cx.listener(|this, _: &MoveLinesUp, window, cx| {
            this.edit_lines(
                |text, selection, _| line_edits::move_lines(text, selection, true),
                window,
                cx,
            )
        }))
        .on_action(cx.listener(|this, _: &MoveLinesDown, window, cx| {
            this.edit_lines(
                |text, selection, _| line_edits::move_lines(text, selection, false),
                window,
                cx,
            )
        }))
        .on_action(cx.listener(|this, _: &CopyLinesUp, window, cx| {
            this.edit_lines(
                |text, selection, _| Some(line_edits::copy_lines(text, selection, true)),
                window,
                cx,
            )
        }))
        .on_action(cx.listener(|this, _: &CopyLinesDown, window, cx| {
            this.edit_lines(
                |text, selection, _| Some(line_edits::copy_lines(text, selection, false)),
                window,
                cx,
            )
        }))
        .on_action(cx.listener(|this, _: &DeleteLines, window, cx| {
            this.edit_lines(
                |text, selection, _| Some(line_edits::delete_lines(text, selection)),
                window,
                cx,
            )
        }))
        .on_action(cx.listener(|this, _: &InsertLineAfter, window, cx| {
            this.edit_lines(
                |text, selection, _| Some(line_edits::insert_line(text, selection, true)),
                window,
                cx,
            )
        }))
        .on_action(cx.listener(|this, _: &InsertLineBefore, window, cx| {
            this.edit_lines(
                |text, selection, _| Some(line_edits::insert_line(text, selection, false)),
                window,
                cx,
            )
        }))
        .on_action(cx.listener(|this, _: &ToggleWordWrap, window, cx| {
            let wrap = !this.word_wrap;
            this.set_word_wrap(wrap, window, cx)
        }))
        .on_action(
            cx.listener(|this, _: &Fold, window, cx| this.fold(Some(true), false, window, cx)),
        )
        .on_action(
            cx.listener(|this, _: &Unfold, window, cx| this.fold(Some(false), false, window, cx)),
        )
        .on_action(
            cx.listener(|this, _: &FoldAll, window, cx| this.fold(Some(true), true, window, cx)),
        )
        .on_action(cx.listener(|this, _: &UnfoldAll, window, cx| this.fold(None, true, window, cx)))
        .on_action(cx.listener(|this, _: &ZoomIn, _, cx| this.zoom(1., cx)))
        .on_action(cx.listener(|this, _: &ZoomOut, _, cx| this.zoom(-1., cx)))
        .on_action(cx.listener(|this, _: &ZoomReset, _, cx| this.zoom(0., cx)))
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;

    use gpui::{TestAppContext, VisualTestContext};

    use super::*;

    fn window_on<'a>(
        cx: &'a mut TestAppContext,
        name: &str,
        text: &str,
    ) -> (
        tempfile::TempDir,
        PathBuf,
        Entity<Workbench>,
        &'a mut VisualTestContext,
    ) {
        cx.update(|cx| {
            gpui_component::init(cx);
            language::register_grammars();
            crate::workbench_window::init(cx);
        });
        let folder = tempfile::tempdir().unwrap();
        let path = folder.path().join(name);
        fs::write(&path, text).unwrap();
        let document = Document::open(&path).unwrap();
        let (view, cx) = cx.add_window_view(|window, cx| {
            Workbench::new(
                Some(document),
                super::super::tests::test_settings(),
                None,
                window,
                cx,
            )
        });
        (folder, path, view, cx)
    }

    fn text(view: &Entity<Workbench>, cx: &mut VisualTestContext) -> String {
        view.read_with(cx, |view, cx| {
            view.panes[view.active].editor.read(cx).value().to_string()
        })
    }

    #[gpui::test]
    fn the_line_commands_answer_their_keys(cx: &mut TestAppContext) {
        let (_folder, _path, view, cx) = window_on(cx, "app.ts", "one();\ntwo();\n");
        cx.simulate_keystrokes("cmd-/");
        assert_eq!(text(&view, cx), "// one();\ntwo();\n");
        cx.simulate_keystrokes("cmd-z");
        assert_eq!(text(&view, cx), "one();\ntwo();\n");

        cx.simulate_keystrokes("alt-down");
        assert_eq!(text(&view, cx), "two();\none();\n");
        cx.simulate_keystrokes("shift-alt-down");
        assert_eq!(text(&view, cx), "two();\none();\none();\n");
        cx.simulate_keystrokes("cmd-shift-k");
        assert_eq!(text(&view, cx), "two();\none();\n");
        cx.simulate_keystrokes("cmd-enter");
        cx.simulate_input("three();");
        assert!(
            text(&view, cx).contains("\nthree();"),
            "{:?}",
            text(&view, cx)
        );
    }

    #[gpui::test]
    fn line_commands_work_at_every_cursor(cx: &mut TestAppContext) {
        let (_folder, _path, view, cx) = window_on(cx, "app.ts", "one();\ntwo();\nthree();\n");
        // A second cursor on the line below, then comment both.
        cx.simulate_keystrokes("cmd-alt-down cmd-/");
        assert_eq!(text(&view, cx), "// one();\n// two();\nthree();\n");
        cx.simulate_keystrokes("cmd-z");
        assert_eq!(text(&view, cx), "one();\ntwo();\nthree();\n");
        cx.simulate_keystrokes("cmd-up cmd-alt-down cmd-shift-k");
        assert_eq!(text(&view, cx), "three();\n");
    }

    #[gpui::test]
    fn blocks_fold_and_unfold_from_the_keyboard(cx: &mut TestAppContext) {
        let code = "function a() {\n  one();\n  two();\n}\n\nfunction b() {\n  three();\n}\n";
        let (_folder, _path, view, cx) = window_on(cx, "app.ts", code);
        let folded = |view: &Entity<Workbench>, cx: &mut VisualTestContext| {
            view.read_with(cx, |view, cx| {
                view.panes[view.active]
                    .editor
                    .read(cx)
                    .display_map()
                    .folded_ranges()
                    .iter()
                    .map(|range| range.start_line)
                    .collect::<Vec<_>>()
            })
        };
        cx.run_until_parked();
        // From inside the first function.
        cx.simulate_keystrokes("down cmd-alt-[");
        assert_eq!(folded(&view, cx), [0]);
        cx.simulate_keystrokes("cmd-alt-]");
        assert_eq!(folded(&view, cx), Vec::<usize>::new());
        cx.simulate_keystrokes("cmd-k cmd-0");
        assert_eq!(folded(&view, cx), [0, 5]);
        cx.simulate_keystrokes("cmd-k cmd-j");
        assert_eq!(folded(&view, cx), Vec::<usize>::new());
    }

    #[gpui::test]
    fn sticky_lines_and_the_minimap_follow_the_scroll(cx: &mut TestAppContext) {
        let mut code = String::from("function lunga() {\n  if (vero) {\n");
        for line in 0..150 {
            code.push_str(&format!("    riga({line});\n"));
        }
        code.push_str("  }\n}\n");
        let (_folder, _path, view, cx) = window_on(cx, "app.ts", &code);
        let (editor, sticky, minimap) = view.read_with(cx, |view, _| {
            let pane = &view.panes[view.active];
            (
                pane.editor.clone(),
                pane.sticky.clone().unwrap(),
                pane.minimap.clone().unwrap(),
            )
        });
        let draw = |cx: &mut VisualTestContext| {
            cx.update(|window, _| window.refresh());
            cx.run_until_parked();
        };
        draw(cx);
        assert_eq!(
            sticky.read_with(cx, |sticky, cx| sticky.lines(cx)),
            Vec::<usize>::new()
        );
        // Down in the middle of the `if`: the function and the `if` stay.
        editor.update(cx, |editor, cx| {
            let line_height = editor.line_height().unwrap();
            editor.set_scroll_offset(gpui::point(px(0.), -line_height * 60.), cx);
        });
        draw(cx);
        let top = editor.read_with(cx, |editor, _| editor.visible_buffer_lines().unwrap().start);
        assert!(top >= 50, "scrolled to {top}");
        assert_eq!(sticky.read_with(cx, |sticky, cx| sticky.lines(cx)), [0, 1]);
        // A click near the top of the minimap goes back up.
        cx.update(|_, cx| super::editor_overlays::Minimap::scroll_to(&editor, 0., 400., 154, cx));
        draw(cx);
        let top = editor.read_with(cx, |editor, _| editor.visible_buffer_lines().unwrap().start);
        assert_eq!(top, 0);
        let _ = minimap;
    }

    #[gpui::test]
    fn typing_the_end_of_a_tag_closes_it(cx: &mut TestAppContext) {
        let (_folder, _path, view, cx) = window_on(cx, "card.tsx", "");
        cx.simulate_input("<div className=\"card\"");
        cx.simulate_keystrokes("shift->");
        assert_eq!(text(&view, cx), "<div className=\"card\"></div>");
        cx.simulate_input("ok");
        assert_eq!(text(&view, cx), "<div className=\"card\">ok</div>");
    }

    #[gpui::test]
    fn wrap_and_zoom_change_every_editor(cx: &mut TestAppContext) {
        let (_folder, _path, view, cx) = window_on(cx, "notes.md", "text");
        let before = view.read_with(cx, |view, _| (view.word_wrap, view.font_size));
        assert_eq!(before, (false, 14.));
        cx.simulate_keystrokes("alt-z cmd-= cmd-=");
        let after = view.read_with(cx, |view, _| (view.word_wrap, view.font_size));
        assert_eq!(after, (true, 16.));
        cx.simulate_keystrokes("cmd--");
        assert_eq!(view.read_with(cx, |view, _| view.font_size), 15.);
    }
}
