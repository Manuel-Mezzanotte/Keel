//! The right column of a differences tab, edited where it is read: a
//! double click on a line of the file, or the pencil of a block, opens
//! its lines in an editor with their colours; Cmd+Enter writes them into
//! the file and saves it, Esc leaves it as it was.

use std::ops::Range;

use gpui::{Anchor, AnyElement, KeyBinding, Pixels, Point, anchored, deferred, point};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::{ActiveTheme as _, Icon, Sizable as _};
use gpui_kit_assets::IconName;

use super::file_pieces::{Put, with_pieces};
use super::shell::Colors;
use super::*;

actions!(keel, [ApplyDiffEdit]);

const DIFF_EDIT_CONTEXT: &str = "DiffEdit";
/// The editor's height, in lines, at most.
const MOST_LINES: usize = 18;

pub(crate) fn bindings() -> Vec<KeyBinding> {
    vec![KeyBinding::new(
        "cmd-enter",
        ApplyDiffEdit,
        Some(DIFF_EDIT_CONTEXT),
    )]
}

pub(super) struct DiffEdit {
    /// The differences tab it opened from.
    tab: String,
    /// The file, from the repository's top folder.
    path: String,
    /// The lines of the file it takes the place of.
    pub(super) lines: Range<usize>,
    /// Those lines as the tab showed them.
    original: Vec<String>,
    /// The file had other lines there when it was to be written.
    refused: bool,
    pub(super) editor: Entity<EditorState>,
    anchor: Point<Pixels>,
}

impl Workbench {
    /// A double click on a line of the right column: in the changes not
    /// staged yet, that column is the file, so its block opens to edit;
    /// elsewhere the file opens at the line.
    pub(super) fn diff_line_double_click(
        &mut self,
        uri: &str,
        file: usize,
        line: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // In a pull request's changes, the line's comments.
        if let Some(Backing::Diff(tab)) = self.documents.get(uri).map(|document| &document.backing)
            && let super::diff_view::DiffSource::Pull { number, .. } = &tab.source
            && let Some((path, ..)) = tab.doc.as_ref().and_then(|doc| doc.file_lines(file))
        {
            let number = *number;
            return self.open_line_comments(number, path, line as u32 + 1, window, cx);
        }
        if self.diff_editable(uri) {
            self.edit_diff_lines(uri, file, Some(line), None, window, cx);
        } else {
            self.open_diff_line(uri, line, window, cx);
        }
    }

    /// Whether a differences tab's right column is the file on disk.
    pub(super) fn diff_editable(&self, uri: &str) -> bool {
        matches!(
            self.documents.get(uri).map(|document| &document.backing),
            Some(Backing::Diff(tab)) if matches!(
                tab.source,
                super::diff_view::DiffSource::Changes | super::diff_view::DiffSource::AllChanges
            )
        )
    }

    /// Opens lines of a file of a differences tab to edit: block `hunk`'s,
    /// or the block holding `line`, or that line alone.
    pub(super) fn edit_diff_lines(
        &mut self,
        uri: &str,
        file: usize,
        line: Option<usize>,
        hunk: Option<usize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(Backing::Diff(tab)) = self.documents.get(uri).map(|document| &document.backing)
        else {
            return;
        };
        let Some((path, file_lines, hunks)) = tab.doc.as_ref().and_then(|doc| doc.file_lines(file))
        else {
            return;
        };
        let lines = match (hunk, line) {
            (Some(hunk), _) => match hunks.get(hunk) {
                Some(hunk) => hunk.new.clone(),
                None => return,
            },
            (None, Some(line)) => hunks
                .iter()
                .find(|hunk| hunk.new.contains(&line))
                .map_or(line..line + 1, |hunk| hunk.new.clone()),
            (None, None) => return,
        };
        let end = lines.end.min(file_lines.len());
        let original: Vec<String> = file_lines[lines.start.min(end)..end]
            .iter()
            .map(|line| line.trim_end_matches('\r').to_owned())
            .collect();
        let text = original.join("\n");
        let absolute = self.diff_edit_file(&path);
        let language_id = self.language_id_for(&absolute);
        let language = Language::for_vscode_id(&language_id);
        let tab_size = self.tab_size_for(&language_id);
        let editor = cx.new(|cx| {
            EditorState::new(window, cx)
                .language(language.highlighter_name())
                .tab_size(tab_size)
                .searchable(false)
                // The title says which lines they are: numbered from 1
                // they would be wrong.
                .line_number(false)
                .default_value(text)
        });
        editor.update(cx, |editor, cx| editor.focus(window, cx));
        let mouse = window.mouse_position();
        self.diff_edit = Some(DiffEdit {
            tab: uri.to_owned(),
            path,
            lines,
            original,
            refused: false,
            editor,
            anchor: point(mouse.x - px(120.), mouse.y + px(14.)),
        });
        cx.notify();
    }

    /// Writes the edited lines into the file and saves it, where the file
    /// still has the lines they came from; else the card stays open, and
    /// says so. A file open with changes of its own gets them in its tab.
    pub(super) fn apply_diff_edit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(edit) = &self.diff_edit else {
            return;
        };
        let edited = edit.editor.read(cx).value().to_string();
        let new: Vec<String> = if edited.is_empty() {
            Vec::new()
        } else {
            let edited = edited.strip_suffix('\n').unwrap_or(&edited);
            edited
                .split('\n')
                .map(|line| line.trim_end_matches('\r').to_owned())
                .collect()
        };
        let absolute = self.diff_edit_file(&edit.path);
        let put = self.text_to_change(&absolute).map(|current| {
            with_pieces(
                &current,
                &[Put {
                    start: edit.lines.start,
                    original: &edit.original,
                    new: &new,
                }],
            )
        });
        let text = match put {
            Some((text, put)) if put == [true] => text,
            _ => {
                if let Some(edit) = &mut self.diff_edit {
                    edit.refused = true;
                }
                cx.notify();
                return;
            }
        };
        if self.write_pieces(&absolute, &text, window, cx) {
            self.diff_edit = None;
            self.refresh_git(cx);
            self.focus_pane(self.active, window, cx);
        }
    }

    fn diff_edit_file(&self, path: &str) -> PathBuf {
        self.scm
            .repo
            .as_ref()
            .map_or_else(|| PathBuf::from(path), |repo| repo.absolute(path))
    }

    pub(super) fn cancel_diff_edit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.diff_edit.take().is_some() {
            self.focus_pane(self.active, window, cx);
            cx.notify();
        }
    }

    /// The editing card, over the tab it opened from while that is in front.
    pub(super) fn render_diff_edit(
        &self,
        colors: Colors,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let edit = self.diff_edit.as_ref()?;
        if self.panes.get(self.active).map(|pane| pane.uri.as_str()) != Some(edit.tab.as_str()) {
            return None;
        }
        let strings = self.strings();
        let theme = cx.theme();
        let name = edit
            .path
            .rsplit('/')
            .next()
            .unwrap_or(&edit.path)
            .to_owned();
        let title = if edit.lines.is_empty() {
            super::notices::fill(
                strings.diff_edit_new_lines,
                &[
                    ("line", &(edit.lines.start + 1).to_string()),
                    ("file", &name),
                ],
            )
        } else {
            super::notices::fill(
                strings.diff_edit_lines,
                &[
                    ("from", &(edit.lines.start + 1).to_string()),
                    ("to", &edit.lines.end.to_string()),
                    ("file", &name),
                ],
            )
        };
        let rows = edit
            .editor
            .read(cx)
            .value()
            .lines()
            .count()
            .clamp(3, MOST_LINES);
        let line_height = self.font_size * 1.5;
        let editor_height = px(line_height * rows as f32 + 12.);
        Some(
            deferred(
                anchored()
                    .position(edit.anchor)
                    .anchor(Anchor::TopLeft)
                    .snap_to_window_with_margin(px(10.))
                    .child(
                        div()
                            .id("diff-edit")
                            .occlude()
                            .w(px(720.))
                            .p_2()
                            .flex()
                            .flex_col()
                            .gap_2()
                            .rounded(px(10.))
                            .border_1()
                            .border_color(theme.primary.opacity(0.55))
                            .bg(theme.popover)
                            .shadow_lg()
                            .key_context(DIFF_EDIT_CONTEXT)
                            .on_action(cx.listener(|this, _: &ApplyDiffEdit, window, cx| {
                                this.apply_diff_edit(window, cx)
                            }))
                            .capture_action(cx.listener(
                                |this, _: &gpui_component::input::Escape, window, cx| {
                                    this.cancel_diff_edit(window, cx);
                                    cx.stop_propagation();
                                },
                            ))
                            .child(
                                div()
                                    .px_1()
                                    .flex()
                                    .items_center()
                                    .gap_2()
                                    .text_xs()
                                    .text_color(colors.muted)
                                    .child(
                                        Icon::new(IconName::Pencil)
                                            .xsmall()
                                            .text_color(theme.primary),
                                    )
                                    .child(div().flex_1().truncate().child(title)),
                            )
                            .child(
                                div()
                                    .h(editor_height)
                                    .rounded(px(6.))
                                    .border_1()
                                    .border_color(colors.border)
                                    .bg(colors.editor)
                                    .overflow_hidden()
                                    .child(
                                        Editor::new(&edit.editor)
                                            .h_full()
                                            .bordered(false)
                                            .text_size(px(self.font_size)),
                                    ),
                            )
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_1()
                                    .child(
                                        div()
                                            .flex_1()
                                            .text_xs()
                                            .text_color(if edit.refused {
                                                theme.warning
                                            } else {
                                                colors.faint
                                            })
                                            .child(if edit.refused {
                                                strings.diff_edit_refused
                                            } else {
                                                strings.diff_edit_hint
                                            }),
                                    )
                                    .child(
                                        Button::new("diff-edit-cancel")
                                            .ghost()
                                            .small()
                                            .label(strings.diff_edit_cancel)
                                            .on_click(cx.listener(|this, _, window, cx| {
                                                this.cancel_diff_edit(window, cx)
                                            })),
                                    )
                                    .child(
                                        Button::new("diff-edit-apply")
                                            .primary()
                                            .small()
                                            .label(strings.diff_edit_apply)
                                            .on_click(cx.listener(|this, _, window, cx| {
                                                this.apply_diff_edit(window, cx)
                                            })),
                                    ),
                            ),
                    ),
            )
            .with_priority(2)
            .into_any_element(),
        )
    }
}
