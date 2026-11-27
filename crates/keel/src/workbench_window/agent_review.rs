//! The agent's changes, file by file: Keel keeps each file's text from
//! before the agent first touched it in the conversation, lists the files
//! that differ now with their added and removed lines, and lets you look
//! at the differences, keep a change (Accept) or put the file back
//! (Reject), one by one or all together.

use gpui::{AnyElement, StatefulInteractiveElement as _};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::diff::{Diff, DiffFile, DiffMode, DiffState};
use gpui_component::input::{Textarea, TextareaState};
use gpui_component::{ActiveTheme as _, Icon, Sizable as _};
use gpui_kit_assets::IconName;

use super::diff_view::{DiffSource, DiffTab, diff_uri};
use super::shell::Colors;
use super::*;

/// A comment on lines of Claude's changes, as Warp's and Xcode's review:
/// they wait together, beside their lines, and go to Claude as one
/// message.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct ReviewComment {
    pub(super) path: PathBuf,
    /// The lines of the file as it is now, from 0, the end left out.
    pub(super) lines: std::ops::Range<usize>,
    pub(super) text: String,
}

/// How many lines of code a comment quotes to Claude, at most.
const QUOTED_LINES: usize = 8;

/// A file the agent changed, as it stands now.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct AgentChange {
    pub(super) path: PathBuf,
    /// None: the agent created it.
    pub(super) before: Option<String>,
    pub(super) added: usize,
    pub(super) removed: usize,
}

impl Workbench {
    /// Reads again which kept files differ from what is on disk now.
    pub(super) fn refresh_agent_changes(&mut self) {
        let changes = self
            .agent_panel
            .snapshots
            .iter()
            .filter_map(|(path, before)| {
                let now = std::fs::read_to_string(path).ok();
                if now == *before {
                    return None;
                }
                let old = before.as_deref().unwrap_or_default();
                let new = now.as_deref().unwrap_or_default();
                let hunks = git::hunks(old, new);
                Some(AgentChange {
                    path: path.clone(),
                    before: before.clone(),
                    added: hunks.iter().map(|hunk| hunk.new.len()).sum(),
                    removed: hunks.iter().map(|hunk| hunk.old.len()).sum(),
                })
            })
            .collect();
        self.agent_panel.changes = changes;
    }

    /// Keeps the agent's change to `path`.
    pub(super) fn accept_agent_change(
        &mut self,
        path: &Path,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.agent_panel
            .snapshots
            .retain(|(known, _)| known != path);
        self.close_agent_change_tab(path, window, cx);
        self.refresh_agent_changes();
        cx.notify();
    }

    /// Puts `path` back as it was; a file the agent created goes to the
    /// Trash.
    pub(super) fn reject_agent_change(
        &mut self,
        path: &Path,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(index) = self
            .agent_panel
            .snapshots
            .iter()
            .position(|(known, _)| known == path)
        else {
            return;
        };
        let (_, before) = self.agent_panel.snapshots.remove(index);
        let restored = match &before {
            Some(text) => std::fs::write(path, text).map_err(|error| error.to_string()),
            None if path.exists() => files::trash(path).map_err(|error| error.to_string()),
            None => Ok(()),
        };
        if let Err(error) = restored {
            self.report(format!("{}: {error}", path.display()));
        }
        // An open file without unsaved changes shows the old text at once.
        self.reload_document(&Uri::file(path).to_string(), window, cx);
        // Its lines are gone, and what was said of them.
        self.agent_panel
            .review
            .retain(|comment| comment.path != path);
        self.close_agent_change_tab(path, window, cx);
        self.refresh_agent_changes();
        cx.notify();
    }

    pub(super) fn accept_all_agent_changes(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let paths: Vec<PathBuf> = self
            .agent_panel
            .changes
            .iter()
            .map(|change| change.path.clone())
            .collect();
        for path in paths {
            self.accept_agent_change(&path, window, cx);
        }
    }

    pub(super) fn reject_all_agent_changes(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let paths: Vec<PathBuf> = self
            .agent_panel
            .changes
            .iter()
            .map(|change| change.path.clone())
            .collect();
        for path in paths {
            self.reject_agent_change(&path, window, cx);
        }
    }

    /// A tab with the differences between the kept text and the file now.
    pub(super) fn open_agent_change(
        &mut self,
        path: &Path,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(change) = self
            .agent_panel
            .changes
            .iter()
            .find(|change| change.path == path)
            .cloned()
        else {
            return;
        };
        let now = std::fs::read_to_string(path).unwrap_or_default();
        let shown = self
            .project
            .root
            .as_ref()
            .and_then(|root| path.strip_prefix(root).ok())
            .unwrap_or(path)
            .to_string_lossy()
            .into_owned();
        let patch = git::unified_patch(
            &shown,
            change.before.as_deref().unwrap_or_default(),
            &now,
            3,
        );
        let doc = super::diff_pane::DiffDoc::new(
            vec![self.agent_file_diff(&change, &now)],
            self.diff_split,
            false,
        );
        let language = Language::for_path(path).highlighter_name();
        let files: Vec<DiffFile> = DiffFile::parse(&patch)
            .map(|files| {
                files
                    .into_iter()
                    .map(|file| file.with_language(language))
                    .collect()
            })
            .unwrap_or_default();
        let source = DiffSource::Agent(path.display().to_string());
        let uri = diff_uri("", &source);
        let mode = if self.diff_split {
            DiffMode::Split
        } else {
            DiffMode::Unified
        };
        let state = cx.new(|cx| DiffState::new(files, cx).with_mode(mode));
        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        // Read again each time it opens: the agent may have changed it since.
        self.documents.insert(
            uri.clone(),
            OpenDocument {
                uri: uri.clone(),
                backing: Backing::Diff(DiffTab {
                    path: path.display().to_string(),
                    source,
                    state,
                    title: format!("{name} (Claude)"),
                    doc: Some(doc),
                    images: None,
                }),
                language_id: "diff".to_owned(),
                version: 1,
                synced_text: String::new(),
                modified: false,
            },
        );
        let column = self.active.min(self.panes.len().saturating_sub(1));
        self.show_tab(&uri, column, OpenAs::Preview, true, window, cx);
        cx.notify();
    }

    /// A change's differences, with the comments beside their lines.
    fn agent_file_diff(&self, change: &AgentChange, now: &str) -> super::diff_model::FileDiff {
        let mut diff = super::diff_model::FileDiff::new(
            change.path.display().to_string(),
            change.before.as_deref().unwrap_or_default(),
            now,
            false,
        );
        diff.notes = self
            .agent_panel
            .review
            .iter()
            .filter(|comment| comment.path == change.path)
            .map(|comment| {
                let line = comment.lines.end.saturating_sub(1).max(comment.lines.start);
                (line, format!("💬 {}", comment.text.trim()))
            })
            .collect();
        diff
    }

    /// Shows the comments again in the open tab of a change.
    fn refresh_agent_change_notes(&mut self, path: &Path) {
        let Some(change) = self
            .agent_panel
            .changes
            .iter()
            .find(|change| change.path == path)
            .cloned()
        else {
            return;
        };
        let uri = diff_uri("", &DiffSource::Agent(path.display().to_string()));
        let Some(Backing::Diff(tab)) = self.documents.get(&uri).map(|document| &document.backing)
        else {
            return;
        };
        let Some(doc) = tab.doc.clone() else {
            return;
        };
        let now = std::fs::read_to_string(path).unwrap_or_default();
        doc.set_files(vec![self.agent_file_diff(&change, &now)]);
    }

    /// The comment on these lines of a change: a window to write it, with
    /// what was written before.
    pub(super) fn comment_agent_lines(
        &mut self,
        path: &Path,
        lines: std::ops::Range<usize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if window.has_active_dialog(cx) {
            return;
        }
        let strings = self.strings();
        let before = self
            .agent_panel
            .review
            .iter()
            .find(|comment| comment.path == path && comment.lines.start == lines.start)
            .map(|comment| comment.text.clone())
            .unwrap_or_default();
        let field = cx.new(|cx| {
            TextareaState::new(window, cx)
                .placeholder(strings.agent_comment_hint)
                .rows(4)
                .default_value(before)
        });
        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        let place = if lines.len() <= 1 {
            super::notices::fill(
                strings.agent_comment_line,
                &[("file", &name), ("line", &(lines.start + 1).to_string())],
            )
        } else {
            super::notices::fill(
                strings.agent_comment_lines,
                &[
                    ("file", &name),
                    ("from", &(lines.start + 1).to_string()),
                    ("to", &lines.end.to_string()),
                ],
            )
        };
        let this = cx.weak_entity();
        let path = path.to_path_buf();
        let shown = field.clone();
        window.open_dialog(cx, move |dialog, _, cx| {
            let muted = Colors::new(cx).muted;
            let (this, field, path, lines) =
                (this.clone(), shown.clone(), path.clone(), lines.clone());
            let saved = (this.clone(), field.clone(), path.clone(), lines.clone());
            dialog
                .w(px(520.))
                .title(format!("{} · {place}", strings.agent_comment_title))
                .on_ok(move |_, window, cx| {
                    let text = field.read(cx).value().trim().to_owned();
                    this.update(cx, |this, cx| {
                        this.set_agent_comment(&path, lines.clone(), text, window, cx)
                    })
                    .ok();
                    true
                })
                .child(
                    div()
                        .capture_key_down(move |event, window, cx| {
                            let keystroke = &event.keystroke;
                            if keystroke.key == "enter" && keystroke.modifiers.platform {
                                cx.stop_propagation();
                                let (this, field, path, lines) = saved.clone();
                                let text = field.read(cx).value().trim().to_owned();
                                window.close_dialog(cx);
                                this.update(cx, |this, cx| {
                                    this.set_agent_comment(&path, lines, text, window, cx)
                                })
                                .ok();
                            }
                        })
                        .flex()
                        .flex_col()
                        .gap_2()
                        .child(Textarea::new(&shown))
                        .child(
                            div()
                                .text_xs()
                                .text_color(muted)
                                .child(strings.agent_comment_keys),
                        ),
                )
        });
        field.update(cx, |field, cx| field.focus(window, cx));
    }

    /// Keeps the comment on these lines; an empty one takes it away.
    pub(super) fn set_agent_comment(
        &mut self,
        path: &Path,
        lines: std::ops::Range<usize>,
        text: String,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let review = &mut self.agent_panel.review;
        review.retain(|comment| !(comment.path == path && comment.lines.start == lines.start));
        if !text.is_empty() {
            review.push(ReviewComment {
                path: path.to_path_buf(),
                lines,
                text,
            });
            review.sort_by(|a, b| (&a.path, a.lines.start).cmp(&(&b.path, b.lines.start)));
        }
        self.refresh_agent_change_notes(path);
        cx.notify();
    }

    /// Every comment goes to Claude in one message, each with the lines it
    /// speaks of; what is written in the field goes with them.
    pub(super) fn send_agent_comments(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.agent_panel.review.is_empty() {
            return;
        }
        let comments = std::mem::take(&mut self.agent_panel.review);
        let message = self.agent_review_message(&comments);
        let typed = self.agent_panel.input.read(cx).value().trim().to_owned();
        let text = if typed.is_empty() {
            message
        } else {
            format!("{typed}\n\n{message}")
        };
        self.agent_panel
            .input
            .update(cx, |input, cx| input.set_value(text, window, cx));
        let paths: std::collections::BTreeSet<PathBuf> = comments
            .iter()
            .map(|comment| comment.path.clone())
            .collect();
        for path in paths {
            self.refresh_agent_change_notes(&path);
        }
        self.send_to_agent(window, cx);
        self.agent_panel.scroll.scroll_to_bottom();
    }

    /// Takes every comment away.
    pub(super) fn clear_agent_comments(&mut self, cx: &mut Context<Self>) {
        let paths: std::collections::BTreeSet<PathBuf> = self
            .agent_panel
            .review
            .drain(..)
            .map(|comment| comment.path)
            .collect();
        for path in paths {
            self.refresh_agent_change_notes(&path);
        }
        cx.notify();
    }

    /// The message of the comments: each file with its lines quoted, then
    /// what was said of them.
    fn agent_review_message(&self, comments: &[ReviewComment]) -> String {
        let strings = self.strings();
        let mut message = strings.agent_review_feedback.to_owned();
        for comment in comments {
            let shown = self
                .project
                .root
                .as_ref()
                .and_then(|root| comment.path.strip_prefix(root).ok())
                .unwrap_or(&comment.path)
                .display()
                .to_string();
            let place = if comment.lines.len() <= 1 {
                format!("{shown}:{}", comment.lines.start + 1)
            } else {
                format!("{shown}:{}-{}", comment.lines.start + 1, comment.lines.end)
            };
            let text = std::fs::read_to_string(&comment.path).unwrap_or_default();
            let code: Vec<&str> = text
                .lines()
                .skip(comment.lines.start)
                .take(comment.lines.len().clamp(1, QUOTED_LINES))
                .collect();
            message.push_str(&format!("\n\n`{place}`\n"));
            if !code.is_empty() {
                message.push_str(&format!("```\n{}\n```\n", code.join("\n")));
            }
            message.push_str(comment.text.trim());
        }
        message
    }

    fn close_agent_change_tab(&mut self, path: &Path, window: &mut Window, cx: &mut Context<Self>) {
        let uri = diff_uri("", &DiffSource::Agent(path.display().to_string()));
        while let Some((group, tab)) = self.groups.iter().enumerate().find_map(|(group, tabs)| {
            tabs.tabs
                .iter()
                .position(|tab| tab.uri == uri)
                .map(|tab| (group, tab))
        }) {
            self.close_tab_now(group, tab, window, cx);
        }
        self.documents.remove(&uri);
    }

    /// The list above the message box: one row per changed file.
    pub(super) fn render_agent_changes(
        &self,
        colors: Colors,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let changes = &self.agent_panel.changes;
        if changes.is_empty() {
            return None;
        }
        let strings = self.strings();
        let theme = cx.theme();
        let (green, red) = (theme.success, theme.danger);
        let header =
            div()
                .flex()
                .items_center()
                .gap_1()
                .text_xs()
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_color(colors.muted)
                        .child(strings.agent_changes(changes.len())),
                )
                .child(
                    Button::new("agent-reject-all")
                        .ghost()
                        .small()
                        .label(strings.agent_reject_all)
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.reject_all_agent_changes(window, cx)
                        })),
                )
                .child(
                    Button::new("agent-accept-all")
                        .primary()
                        .small()
                        .label(strings.agent_accept_all)
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.accept_all_agent_changes(window, cx)
                        })),
                );
        let comments = (!self.agent_panel.review.is_empty()).then(|| {
            div()
                .flex()
                .justify_end()
                .child(self.render_agent_comments_bar("agent-panel-comments", false, colors, cx))
        });
        let rows =
            changes.iter().enumerate().map(|(index, change)| {
                let name = change
                    .path
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_default();
                let (open, accept, reject) = (
                    change.path.clone(),
                    change.path.clone(),
                    change.path.clone(),
                );
                div()
                    .id(("agent-change", index))
                    .flex()
                    .items_center()
                    .gap_1()
                    .text_xs()
                    .px_1()
                    .rounded(px(4.))
                    .cursor_pointer()
                    .hover(|row| row.bg(colors.hover))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.open_agent_change(&open, window, cx)
                    }))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_color(colors.text)
                            .child(name),
                    )
                    .when(change.before.is_none(), |row| {
                        row.child(div().text_color(colors.faint).child(strings.agent_new_file))
                    })
                    .child(div().text_color(green).child(format!("+{}", change.added)))
                    .child(div().text_color(red).child(format!("−{}", change.removed)))
                    .child(
                        Button::new(("agent-change-reject", index))
                            .ghost()
                            .small()
                            .icon(Icon::new(IconName::Undo2).xsmall())
                            .tooltip(strings.claude_reject)
                            .on_click(cx.listener(move |this, _, window, cx| {
                                cx.stop_propagation();
                                this.reject_agent_change(&reject, window, cx)
                            })),
                    )
                    .child(
                        Button::new(("agent-change-accept", index))
                            .ghost()
                            .small()
                            .icon(Icon::new(IconName::Check).xsmall())
                            .tooltip(strings.claude_accept)
                            .on_click(cx.listener(move |this, _, window, cx| {
                                cx.stop_propagation();
                                this.accept_agent_change(&accept, window, cx)
                            })),
                    )
            });
        Some(
            div()
                .flex_none()
                .mx_3()
                .mb_1()
                .px_2()
                .py_1p5()
                .rounded(px(8.))
                .border_1()
                .border_color(colors.border)
                .flex()
                .flex_col()
                .gap_0p5()
                .child(header)
                .children(comments)
                .children(rows)
                .into_any_element(),
        )
    }

    /// How many comments wait, with «Send to Claude», and ✕ where there is
    /// room.
    fn render_agent_comments_bar(
        &self,
        id: &'static str,
        compact: bool,
        colors: Colors,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let strings = self.strings();
        let count = self.agent_panel.review.len();
        div()
            .flex()
            .items_center()
            .gap_1()
            .child(
                Icon::new(IconName::MessageSquare)
                    .xsmall()
                    .text_color(colors.accent),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(colors.muted)
                    .child(strings.agent_comments(count)),
            )
            .when(!compact, |bar| {
                bar.child(
                    Button::new((id, 0usize))
                        .ghost()
                        .xsmall()
                        .icon(Icon::new(IconName::X))
                        .tooltip(strings.agent_comments_clear)
                        .on_click(cx.listener(|this, _, _, cx| this.clear_agent_comments(cx))),
                )
            })
            .child(
                Button::new((id, 1usize))
                    .outline()
                    .small()
                    .icon(Icon::new(IconName::ArrowUp))
                    .label(strings.agent_comments_send)
                    .on_click(
                        cx.listener(|this, _, window, cx| this.send_agent_comments(window, cx)),
                    ),
            )
            .into_any_element()
    }

    /// The tab of one change: the differences, with Accept and Reject.
    pub(super) fn render_agent_change_tab(
        &self,
        path: &str,
        tab: &DiffTab,
        colors: Colors,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let strings = self.strings();
        let waiting = self
            .agent_panel
            .changes
            .iter()
            .any(|change| change.path == Path::new(path));
        let (accept, reject) = (PathBuf::from(path), PathBuf::from(path));
        // Where this file is among Claude's changes, as Cursor's review bar
        // says it, with the files before and after a click away.
        let changes: Vec<PathBuf> = self
            .agent_panel
            .changes
            .iter()
            .map(|change| change.path.clone())
            .collect();
        let position = changes.iter().position(|change| change == Path::new(path));
        let step = |forward: bool| {
            let position = position?;
            let count = changes.len();
            (count > 1).then(|| {
                changes[if forward {
                    (position + 1) % count
                } else {
                    (position + count - 1) % count
                }]
                .clone()
            })
        };
        let (previous, next) = (step(false), step(true));
        let counter = position.map(|position| {
            super::notices::fill(
                strings.agent_file_of,
                &[
                    ("index", &(position + 1).to_string()),
                    ("count", &changes.len().to_string()),
                ],
            )
        });
        let toolbar = div()
            .h(px(34.))
            .flex_none()
            .px_3()
            .flex()
            .items_center()
            .gap_1()
            .border_b_1()
            .border_color(colors.border)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_xs()
                    .text_color(colors.muted)
                    .child(format!("{} · {}", strings.agent_change_of_claude, tab.path)),
            )
            .when(!self.agent_panel.review.is_empty(), |toolbar| {
                toolbar.child(self.render_agent_comments_bar("agent-tab", true, colors, cx))
            })
            .children(previous.map(|previous| {
                Button::new("agent-tab-previous")
                    .ghost()
                    .small()
                    .icon(Icon::new(IconName::ChevronLeft))
                    .tooltip(strings.agent_previous_file)
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.open_agent_change(&previous, window, cx)
                    }))
            }))
            .children(counter.map(|counter| {
                div()
                    .px_1()
                    .text_xs()
                    .text_color(colors.muted)
                    .child(counter)
            }))
            .children(next.map(|next| {
                Button::new("agent-tab-next")
                    .ghost()
                    .small()
                    .icon(Icon::new(IconName::ChevronRight))
                    .tooltip(strings.agent_next_file)
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.open_agent_change(&next, window, cx)
                    }))
            }))
            .when(changes.len() > 1, |toolbar| {
                toolbar
                    .child(
                        Button::new("agent-tab-reject-all")
                            .ghost()
                            .small()
                            .label(strings.agent_reject_all)
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.reject_all_agent_changes(window, cx)
                            })),
                    )
                    .child(
                        Button::new("agent-tab-accept-all")
                            .ghost()
                            .small()
                            .label(strings.agent_accept_all)
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.accept_all_agent_changes(window, cx)
                            })),
                    )
            })
            .when(waiting, |toolbar| {
                toolbar
                    .child(
                        Button::new("agent-tab-reject")
                            .ghost()
                            .small()
                            .icon(Icon::new(IconName::Undo2))
                            .label(strings.claude_reject)
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.reject_agent_change(&reject, window, cx)
                            })),
                    )
                    .child(
                        Button::new("agent-tab-accept")
                            .primary()
                            .small()
                            .icon(Icon::new(IconName::Check))
                            .label(strings.claude_accept)
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.accept_agent_change(&accept, window, cx)
                            })),
                    )
            });
        div()
            .size_full()
            .flex()
            .flex_col()
            .child(toolbar)
            .child(
                div().flex_1().min_h_0().child(match &tab.doc {
                    Some(doc) => self.render_diff_doc(
                        &diff_uri("", &DiffSource::Agent(path.to_owned())),
                        doc,
                        super::diff_pane::BlockActions::Comment,
                        colors,
                        window,
                        cx,
                    ),
                    None => Diff::new(&tab.state)
                        .line_number(true)
                        .syntax_highlight(true)
                        .header_visible(false)
                        .into_any_element(),
                }),
            )
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use gpui::{TestAppContext, VisualTestContext};

    use super::super::tests::test_settings;
    use super::*;

    fn wait_until(
        cx: &mut VisualTestContext,
        what: &str,
        check: impl Fn(&mut VisualTestContext) -> bool,
    ) {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            cx.executor().advance_clock(Duration::from_millis(100));
            cx.run_until_parked();
            if check(cx) {
                return;
            }
            assert!(Instant::now() < deadline, "timed out waiting for {what}");
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    /// Asks the fake agent to change `old` into `new` and allows it.
    fn agent_edit(
        view: &Entity<Workbench>,
        cx: &mut VisualTestContext,
        path: &Path,
        old: &str,
        new: &str,
    ) {
        view.update_in(cx, |view, window, cx| {
            let message = format!("edit {} {old} {new}", path.display());
            view.agent_panel
                .input
                .update(cx, |input, cx| input.set_value(message, window, cx));
            view.send_to_agent(window, cx);
        });
        wait_until(cx, "the permission", |cx| {
            view.read_with(cx, |view, _| !view.agent_panel.permissions.is_empty())
        });
        view.update(cx, |view, cx| view.answer_permission(0, "allow", cx));
        wait_until(cx, "the change", |cx| {
            view.read_with(cx, |view, _| {
                !view.agent_panel.running && !view.agent_panel.changes.is_empty()
            })
        });
    }

    /// Comments on the lines of Claude's changes wait beside their lines
    /// and go to Claude together, with the code they speak of.
    #[gpui::test]
    fn comments_on_claudes_lines_go_together(cx: &mut TestAppContext) {
        cx.executor().allow_parking();
        cx.update(|cx| {
            gpui_component::init(cx);
            language::register_grammars();
            init(cx);
        });
        let project = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(project.path().join(".git")).unwrap();
        let path = std::path::absolute(project.path().join("booking.ts")).unwrap();
        std::fs::write(&path, "const nights = 3;\nconst price = 90;\n").unwrap();
        let document = Document::open(&path).unwrap();
        let mut workbench = None;
        let (_root, cx) = cx.add_window_view(|window, cx| {
            let view = cx.new(|cx| {
                let mut workbench =
                    Workbench::new(Some(document), test_settings(), None, window, cx);
                workbench.agent_panel.fake = true;
                workbench.open_agent(window, cx);
                workbench
            });
            workbench = Some(view.clone());
            gpui_component::Root::new(view, window, cx)
        });
        let view = workbench.unwrap();
        wait_until(cx, "the agent", |cx| {
            view.read_with(cx, |view, _| {
                view.agent_panel.status == super::super::agent_panel::AgentStatus::Ready
            })
        });
        agent_edit(&view, cx, &path, "90", "95");
        view.update_in(cx, |view, window, cx| {
            view.open_agent_change(&path, window, cx)
        });
        // A line's comment, written in its window and kept with ⌘↩.
        let uri = diff_uri("", &DiffSource::Agent(path.display().to_string()));
        view.update_in(cx, |view, window, cx| {
            view.comment_diff_line(&uri, 0, 1, window, cx)
        });
        cx.run_until_parked();
        assert!(cx.update(|window, cx| window.has_active_dialog(cx)));
        cx.simulate_input("Keep 90, it is the price of the low season");
        cx.simulate_keystrokes("cmd-enter");
        cx.run_until_parked();
        assert!(!cx.update(|window, cx| window.has_active_dialog(cx)));
        // The block's comment, and the note beside the line.
        view.update_in(cx, |view, window, cx| {
            view.set_agent_comment(&path, 0..1, "Name it nightsCount".into(), window, cx);
        });
        view.read_with(cx, |view, _| {
            assert_eq!(view.agent_panel.review.len(), 2);
            assert_eq!(view.agent_panel.review[0].lines, 0..1);
            let Some(Backing::Diff(tab)) = view.documents.get(&uri).map(|doc| &doc.backing) else {
                panic!()
            };
            let (_, text, _) = tab.doc.as_ref().unwrap().file_lines(0).unwrap();
            assert_eq!(text[1], "const price = 95;");
        });
        // An empty comment takes it away.
        view.update_in(cx, |view, window, cx| {
            view.set_agent_comment(&path, 0..1, String::new(), window, cx);
            view.set_agent_comment(&path, 0..1, "Name it nightsCount".into(), window, cx);
        });
        assert_eq!(
            view.read_with(cx, |view, _| view.agent_panel.review.len()),
            2
        );
        // Together, as one message with the code quoted.
        view.update_in(cx, |view, window, cx| view.send_agent_comments(window, cx));
        cx.run_until_parked();
        view.read_with(cx, |view, _| {
            assert!(view.agent_panel.review.is_empty());
            let said = view
                .agent_panel
                .thread
                .entries
                .iter()
                .rev()
                .find_map(|entry| match entry {
                    ai::thread::Entry::User(text) => Some(text.clone()),
                    _ => None,
                })
                .unwrap();
            assert!(
                said.starts_with("Ho dei commenti sulle tue modifiche"),
                "{said}"
            );
            assert!(
                said.contains("`booking.ts:1`\n```\nconst nights = 3;\n```\nName it nightsCount"),
                "{said}"
            );
            assert!(
                said.contains("`booking.ts:2`\n```\nconst price = 95;\n```\nKeep 90"),
                "{said}"
            );
        });
    }

    #[gpui::test]
    fn the_agents_changes_are_reviewed_file_by_file(cx: &mut TestAppContext) {
        cx.executor().allow_parking();
        cx.update(|cx| {
            gpui_component::init(cx);
            language::register_grammars();
            init(cx);
        });
        let project = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(project.path().join(".git")).unwrap();
        let path = std::path::absolute(project.path().join("booking.ts")).unwrap();
        std::fs::write(&path, "const nights = 3;\nconst price = 90;\n").unwrap();
        let document = Document::open(&path).unwrap();
        let (view, cx) = cx.add_window_view(|window, cx| {
            let mut workbench = Workbench::new(Some(document), test_settings(), None, window, cx);
            workbench.agent_panel.fake = true;
            workbench.open_agent(window, cx);
            workbench
        });
        wait_until(cx, "the agent", |cx| {
            view.read_with(cx, |view, _| {
                view.agent_panel.status == super::super::agent_panel::AgentStatus::Ready
            })
        });

        agent_edit(&view, cx, &path, "90", "95");
        view.read_with(cx, |view, _| {
            let change = &view.agent_panel.changes[0];
            assert_eq!(change.path, path);
            assert_eq!((change.added, change.removed), (1, 1));
        });

        // The differences open in a tab.
        view.update_in(cx, |view, window, cx| {
            view.open_agent_change(&path, window, cx)
        });
        view.read_with(cx, |view, cx| {
            let uri = view.panes[view.active].uri.clone();
            assert!(uri.starts_with("keel-agent-diff:"), "{uri}");
            let Some(Backing::Diff(tab)) =
                view.documents.get(&uri).map(|document| &document.backing)
            else {
                panic!()
            };
            assert_eq!(tab.state.read(cx).files()[0].additions(), 1);
        });

        // Reject puts the file back, and the editor shows it.
        view.update_in(cx, |view, window, cx| {
            view.reject_agent_change(&path, window, cx)
        });
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "const nights = 3;\nconst price = 90;\n"
        );
        view.read_with(cx, |view, cx| {
            assert!(view.agent_panel.changes.is_empty());
            assert!(view.agent_panel.snapshots.is_empty());
            let uri = Uri::file(&path).to_string();
            let pane = view.panes.iter().find(|pane| pane.uri == uri).unwrap();
            assert_eq!(
                pane.editor.read(cx).value().to_string(),
                "const nights = 3;\nconst price = 90;\n"
            );
        });

        // Accept keeps it, and the list empties.
        agent_edit(&view, cx, &path, "3", "4");
        view.update_in(cx, |view, window, cx| {
            view.accept_all_agent_changes(window, cx)
        });
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "const nights = 4;\nconst price = 90;\n"
        );
        view.read_with(cx, |view, _| {
            assert!(view.agent_panel.changes.is_empty());
            assert!(view.agent_panel.snapshots.is_empty());
        });
    }
}
