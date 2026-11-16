//! Editing commands as in VS Code: comments, moving, copying, deleting and
//! inserting lines, tags that close themselves, word wrap and zoom.

use std::ops::Range;

use editor::line_edits::{self, LineEdit, comment_syntax};

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
        ZoomReset
    ]
);

/// The editor's font size from `editor.fontSize`, 14 when unset.
pub(super) fn font_size_setting(settings: &Settings) -> f32 {
    settings
        .number("editor.fontSize")
        .map_or(14., |size| size.clamp(6., 72.) as f32)
}

impl Workbench {
    /// Runs a line command on the active editor as one undoable edit.
    fn edit_lines(
        &mut self,
        command: impl FnOnce(&str, Range<usize>, &str) -> Option<LineEdit>,
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
        let (text, selection) = editor.read_with(cx, |editor, _| {
            (editor.value().to_string(), editor.selected_range())
        });
        let Some(edit) = command(&text, selection, &language) else {
            return;
        };
        editor.update(cx, |editor, cx| {
            editor.set_selected_range(edit.range.clone(), cx);
            editor.replace(edit.text.clone(), window, cx);
            editor.set_selected_range(edit.selection.clone(), cx);
        });
        self.publish_selection_if_changed(self.active, cx);
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
        // On an image the zoom keys zoom the image.
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
        cx.notify();
    }

    /// The handlers of the editing commands, for the window's root.
    pub(super) fn editing_actions(&self, root: gpui::Div, cx: &mut Context<Self>) -> gpui::Div {
        root.on_action(cx.listener(|this, _: &ToggleLineComment, window, cx| {
            this.edit_lines(
                |text, selection, language| {
                    line_edits::toggle_line_comment(text, selection, comment_syntax(language))
                },
                window,
                cx,
            )
        }))
        .on_action(cx.listener(|this, _: &ToggleBlockComment, window, cx| {
            this.edit_lines(
                |text, selection, language| {
                    line_edits::toggle_block_comment(text, selection, comment_syntax(language))
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
