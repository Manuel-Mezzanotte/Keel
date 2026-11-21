//! Claude in the editor, as Cursor's Cmd+K and VS Code's inline chat: Cmd+I
//! (Cmd+K starts Keel's chords) opens a field over the selected lines;
//! Claude rewrites them, the new lines show lit with Keep and Undo, and a
//! further request changes them again. Also the commit message written by
//! Claude, from the changes and the style of the last messages.
//!
//! Both ask Claude Code once, in print mode, with the Mac's own login and
//! no tools: Claude only writes text, Keel puts it in place.

use std::collections::HashMap;
use std::io::Write as _;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};

use gpui::{AnyElement, Div};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::input::{
    Input, InputEvent, InputState, RangeDecoration, RangeDecorationCollection, RangeDecorationStyle,
};
use gpui_component::{Icon, Sizable as _};
use gpui_kit_assets::IconName;

use super::notices::fill;
use super::shell::Colors;
use super::*;

actions!(keel, [InlineEditWithClaude, KeepInlineEdit]);

/// The key context of Cmd+I's card.
const INLINE_CONTEXT: &str = "InlineEdit";

/// Cmd+Enter in the card keeps Claude's lines, as in VS Code.
pub(crate) fn bindings() -> Vec<gpui::KeyBinding> {
    vec![gpui::KeyBinding::new(
        "cmd-enter",
        KeepInlineEdit,
        Some(INLINE_CONTEXT),
    )]
}

/// The instructions for an edit in place.
const EDIT_SYSTEM: &str = "You are the inline editing assistant of a code editor. The user's file \
comes on standard input, with the part to change between the lines <<<KEEL_SELECTION and \
KEEL_SELECTION>>>. Answer ONLY with the text that replaces that part, ready to be put in the \
file: no explanation, no Markdown fences, the file's own indentation. If the part is empty, \
answer with the code to write there.";

/// The instructions for a commit message.
const COMMIT_SYSTEM: &str = "You write git commit messages. The changes come on standard input \
as a patch. Follow the style of the recent messages given (language, Conventional Commits or \
not, length). The first line at most 72 characters; then, only if it helps, a blank line and a \
short body wrapped at 72. Answer ONLY with the message.";

/// The most of a file sent around the lines to change.
const CONTEXT_LINES: usize = 400;
/// The most of a patch sent for a commit message.
const PATCH_LIMIT: usize = 60_000;

/// A run of Claude Code that can be stopped.
#[derive(Clone, Default)]
pub(super) struct ClaudeRun(Arc<Mutex<Option<u32>>>);

impl ClaudeRun {
    pub(super) fn stop(&self) {
        if let Some(pid) = self.0.lock().ok().and_then(|pid| *pid) {
            terminal::end_process(pid);
        }
    }
}

#[cfg(test)]
pub(super) static FAKE_CLAUDE: Mutex<Option<PathBuf>> = Mutex::new(None);

/// Claude Code's program and the login shell's variables it needs.
fn claude_program() -> Option<(PathBuf, HashMap<String, String>)> {
    let env = lsp::servers::login_environment().clone();
    #[cfg(test)]
    if let Some(fake) = FAKE_CLAUDE.lock().ok().and_then(|fake| fake.clone()) {
        return Some((fake, env));
    }
    let program = lsp::servers::which_in("claude", &env)?;
    Some((program, env))
}

/// Asks Claude Code once, in print mode and with no tools; for a
/// background thread. `input` goes on its standard input.
fn ask_once(
    run: &ClaudeRun,
    cwd: &Path,
    system: &str,
    request: &str,
    input: String,
) -> Result<String, String> {
    let (program, env) = claude_program().ok_or_else(String::new)?;
    let mut child = Command::new(program)
        .args([
            "-p",
            "--no-session-persistence",
            "--tools",
            "",
            "--strict-mcp-config",
            "--system-prompt",
            system,
            request,
        ])
        .envs(&env)
        .current_dir(cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| error.to_string())?;
    if let Ok(mut pid) = run.0.lock() {
        *pid = Some(child.id());
    }
    if let Some(mut stdin) = child.stdin.take() {
        // Written apart, so a long input never waits on a full output.
        std::thread::spawn(move || {
            let _ = stdin.write_all(input.as_bytes());
        });
    }
    let output = child.wait_with_output().map_err(|error| error.to_string());
    if let Ok(mut pid) = run.0.lock() {
        *pid = None;
    }
    let output = output?;
    let text = String::from_utf8_lossy(&output.stdout).into_owned();
    if output.status.success() {
        Ok(text)
    } else {
        let error = String::from_utf8_lossy(&output.stderr);
        let said = if error.trim().is_empty() {
            text
        } else {
            error.into_owned()
        };
        Err(said.trim().lines().last().unwrap_or_default().to_owned())
    }
}

/// Claude's answer without the fences it may put around code anyway.
fn without_fences(answer: &str) -> String {
    let trimmed = answer.trim_matches('\n');
    let lines: Vec<&str> = trimmed.lines().collect();
    let fenced = lines.len() >= 2
        && lines[0].trim_start().starts_with("```")
        && lines[lines.len() - 1].trim() == "```";
    if fenced {
        lines[1..lines.len() - 1].join("\n")
    } else {
        trimmed.to_owned()
    }
}

/// The lines a selection touches, whole, without the last line's newline.
pub(super) fn whole_lines(text: &str, selection: Range<usize>) -> Range<usize> {
    let start = text[..selection.start.min(text.len())]
        .rfind('\n')
        .map_or(0, |at| at + 1);
    // A selection that ends at a line's start leaves that line out.
    let mut end = selection.end.min(text.len());
    if selection.end > selection.start && end > start && text[..end].ends_with('\n') {
        end -= 1;
    }
    let end = text[end..].find('\n').map_or(text.len(), |at| end + at);
    start..end.max(start)
}

/// The file with the part to change marked, cut to the lines around it.
fn marked_file(text: &str, range: &Range<usize>) -> String {
    let before = &text[..range.start];
    let after = &text[range.end..];
    let before_lines: Vec<&str> = before.split_inclusive('\n').collect();
    let after_lines: Vec<&str> = after.split_inclusive('\n').collect();
    let keep_before = before_lines.len().saturating_sub(CONTEXT_LINES);
    let kept_after = after_lines.len().min(CONTEXT_LINES);
    let mut out = String::new();
    if keep_before > 0 {
        out.push_str("…\n");
    }
    out.extend(before_lines[keep_before..].iter().copied());
    if !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }
    out.push_str("<<<KEEL_SELECTION\n");
    out.push_str(&text[range.clone()]);
    out.push_str("\nKEEL_SELECTION>>>");
    out.extend(after_lines[..kept_after].iter().copied());
    if kept_after < after_lines.len() {
        out.push_str("\n…");
    }
    out
}

enum Phase {
    Asking,
    Working {
        run: ClaudeRun,
        _task: Task<()>,
    },
    /// Claude's lines are in the file, waiting for Keep or Undo.
    Review,
    Failed(String),
}

/// Cmd+I under way in an editor.
pub(super) struct InlineEdit {
    editor_id: String,
    editor: Entity<EditorState>,
    path: String,
    language: String,
    /// The lines Claude works on, in the text as it is now.
    range: Range<usize>,
    /// Their text before Claude, for Undo.
    original: String,
    /// What Claude wrote last, while it waits for Keep or Undo.
    written: Option<String>,
    pub(super) input: Entity<InputState>,
    phase: Phase,
    decor: RangeDecorationCollection,
    _input: Subscription,
}

impl Workbench {
    pub(super) fn claude_inline_actions(&self, root: Div, cx: &mut Context<Self>) -> Div {
        root.on_action(cx.listener(|this, _: &InlineEditWithClaude, window, cx| {
            this.start_inline_edit(window, cx)
        }))
    }

    /// Cmd+I: the field over the lines of the selection, or the cursor's.
    pub(super) fn start_inline_edit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(pane) = self.panes.get(self.active) else {
            return;
        };
        if matches!(
            self.documents
                .get(&pane.uri)
                .map(|document| &document.backing),
            Some(
                Backing::Image(_)
                    | Backing::Diff(_)
                    | Backing::Pdf(_)
                    | Backing::Graph(_)
                    | Backing::Preview(_)
                    | Backing::Page(_)
                    | Backing::ExtensionDetails(_)
            )
        ) {
            return;
        }
        let editor = pane.editor.clone();
        let editor_id = pane.editor_id.clone();
        let uri = pane.uri.clone();
        self.close_inline_edit(false, window, cx);
        let (text, selection) = editor.read_with(cx, |editor, _| {
            (editor.value().to_string(), editor.selected_range())
        });
        let range = whole_lines(&text, selection);
        let original = text[range.clone()].to_owned();
        let strings = self.strings();
        let placeholder = if original.trim().is_empty() {
            strings.inline_edit_placeholder_empty
        } else {
            strings.inline_edit_placeholder
        };
        let input = cx.new(|cx| InputState::new(window, cx).placeholder(placeholder));
        let subscription =
            cx.subscribe_in(&input, window, |this, _, event: &InputEvent, window, cx| {
                if let InputEvent::PressEnter { secondary, .. } = event {
                    if *secondary {
                        this.close_inline_edit(false, window, cx);
                        this.focus_pane(this.active, window, cx);
                    } else {
                        this.submit_inline_edit(window, cx);
                    }
                }
            });
        let decor = editor.update(cx, |editor, cx| {
            editor.create_range_decorations_collection(Vec::new(), cx)
        });
        let language = self
            .documents
            .get(&uri)
            .map(|document| document.language_id.clone())
            .unwrap_or_default();
        let path = self.shown_path(&uri);
        self.inline_edit = Some(InlineEdit {
            editor_id,
            editor,
            path,
            language,
            range,
            original,
            written: None,
            input: input.clone(),
            phase: Phase::Asking,
            decor,
            _input: subscription,
        });
        self.light_inline_edit(cx);
        input.update(cx, |input, cx| input.focus(window, cx));
        cx.notify();
    }

    /// The lines Claude works on: lit in blue while asking, in green once
    /// written; whole lines, to the editor's right edge.
    fn light_inline_edit(&self, cx: &mut Context<Self>) {
        let Some(edit) = &self.inline_edit else {
            return;
        };
        let theme = cx.theme();
        let color = if edit.written.is_some() {
            theme.success.opacity(0.16)
        } else {
            theme.primary.opacity(0.12)
        };
        let text = edit.editor.read(cx).value();
        let end = (edit.range.end + 1).min(text.len()).max(edit.range.end);
        let decoration = RangeDecoration::new(edit.range.start..end)
            .with_style(RangeDecorationStyle::Fill)
            .with_color(color);
        edit.decor.set(vec![decoration], cx);
    }

    /// Enter: Claude rewrites the lines as asked.
    fn submit_inline_edit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(edit) = &mut self.inline_edit else {
            return;
        };
        let request = edit.input.read(cx).value().trim().to_owned();
        if request.is_empty() {
            // Enter on an empty field keeps what Claude wrote.
            if edit.written.is_some() {
                self.close_inline_edit(false, window, cx);
                self.focus_pane(self.active, window, cx);
            }
            return;
        }
        if matches!(edit.phase, Phase::Working { .. }) {
            return;
        }
        let text = edit.editor.read(cx).value().to_string();
        let range = edit.range.clone();
        let input = format!(
            "File: {} ({})\n\n{}",
            edit.path,
            edit.language,
            marked_file(&text, &range)
        );
        let cwd = self.project.root.clone().unwrap_or_else(std::env::temp_dir);
        let run = ClaudeRun::default();
        let running = run.clone();
        let answer = cx
            .background_executor()
            .spawn(async move { ask_once(&running, &cwd, EDIT_SYSTEM, &request, input) });
        let task = cx.spawn_in(window, async move |this, cx| {
            let answer = answer.await;
            this.update_in(cx, |this, window, cx| {
                this.inline_answer(answer, window, cx)
            })
            .ok();
        });
        edit.phase = Phase::Working { run, _task: task };
        edit.input
            .update(cx, |input, cx| input.set_value("", window, cx));
        cx.notify();
    }

    /// Claude answered: its lines go in place of the old ones, lit.
    fn inline_answer(
        &mut self,
        answer: Result<String, String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let strings = self.strings();
        let Some(edit) = &mut self.inline_edit else {
            return;
        };
        let written = match answer {
            Ok(text) => without_fences(&text),
            Err(error) => {
                let message = if error.is_empty() {
                    strings.claude_not_found.to_owned()
                } else {
                    error
                };
                edit.phase = Phase::Failed(message);
                cx.notify();
                return;
            }
        };
        let range = edit.range.clone();
        let current = edit.editor.read(cx).value().to_string();
        if current.get(range.clone()).is_none() {
            edit.phase = Phase::Failed(strings.inline_edit_changed_meanwhile.to_owned());
            cx.notify();
            return;
        }
        edit.editor.update(cx, |editor, cx| {
            editor.set_selected_range(range.clone(), cx);
            editor.replace(written.clone(), window, cx);
        });
        edit.range = range.start..range.start + written.len();
        edit.written = Some(written);
        edit.phase = Phase::Review;
        let placeholder = strings.inline_edit_followup;
        edit.input.update(cx, |input, cx| {
            input.set_placeholder(placeholder, window, cx);
            input.focus(window, cx);
        });
        self.light_inline_edit(cx);
        cx.notify();
    }

    /// Ends Cmd+I: keeps Claude's lines, or puts the old ones back.
    pub(super) fn close_inline_edit(
        &mut self,
        undo: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(edit) = self.inline_edit.take() else {
            return;
        };
        if let Phase::Working { run, .. } = &edit.phase {
            run.stop();
        }
        edit.decor.dispose(cx);
        if !undo {
            cx.notify();
            return;
        }
        let Some(written) = &edit.written else {
            cx.notify();
            return;
        };
        let text = edit.editor.read(cx).value().to_string();
        if text.get(edit.range.clone()) != Some(written.as_str()) {
            self.status_message = Some(self.strings().inline_edit_changed_meanwhile.to_owned());
            cx.notify();
            return;
        }
        let mut wanted = text;
        wanted.replace_range(edit.range.clone(), &edit.original);
        super::tabs::replace_text(&edit.editor, &wanted, window, cx);
        cx.notify();
    }

    fn stop_inline_edit(&mut self, cx: &mut Context<Self>) {
        if let Some(edit) = &mut self.inline_edit
            && let Phase::Working { run, .. } = &edit.phase
        {
            run.stop();
            edit.phase = Phase::Asking;
            cx.notify();
        }
    }

    /// The card of Cmd+I, over the editor that has it.
    pub(super) fn render_inline_edit(
        &self,
        index: usize,
        colors: Colors,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let edit = self.inline_edit.as_ref()?;
        let pane = self.panes.get(index)?;
        if pane.editor_id != edit.editor_id {
            return None;
        }
        let strings = self.strings();
        let theme = cx.theme();
        let text = edit.editor.read(cx).value();
        let first = text[..edit.range.start.min(text.len())]
            .matches('\n')
            .count()
            + 1;
        let last = first + text[edit.range.clone()].matches('\n').count();
        let lines = fill(
            strings.inline_edit_lines,
            &[("from", &first.to_string()), ("to", &last.to_string())],
        );
        let working = matches!(edit.phase, Phase::Working { .. });
        let status: Option<AnyElement> = match &edit.phase {
            Phase::Working { .. } => Some(
                div()
                    .flex()
                    .items_center()
                    .gap_1()
                    .child(super::agent_panel::spinner(colors.muted, self.look.motion))
                    .child(strings.inline_edit_working)
                    .into_any_element(),
            ),
            Phase::Review => edit.written.as_ref().map(|written| {
                let added = written.lines().count();
                let removed = edit.original.lines().count();
                div()
                    .flex()
                    .gap_1()
                    .child(div().text_color(theme.success).child(format!("+{added}")))
                    .child(div().text_color(theme.danger).child(format!("−{removed}")))
                    .into_any_element()
            }),
            Phase::Failed(error) => Some(
                div()
                    .min_w_0()
                    .truncate()
                    .text_color(theme.danger)
                    .child(error.clone())
                    .into_any_element(),
            ),
            Phase::Asking => None,
        };
        let buttons: Vec<AnyElement> = if working {
            vec![
                Button::new("inline-edit-stop")
                    .ghost()
                    .xsmall()
                    .icon(Icon::new(IconName::Square))
                    .label(strings.inline_edit_stop)
                    .on_click(cx.listener(|this, _, _, cx| this.stop_inline_edit(cx)))
                    .into_any_element(),
            ]
        } else if edit.written.is_some() {
            vec![
                Button::new("inline-edit-undo")
                    .ghost()
                    .xsmall()
                    .label(strings.inline_edit_undo)
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.close_inline_edit(true, window, cx);
                        this.focus_pane(this.active, window, cx);
                    }))
                    .into_any_element(),
                Button::new("inline-edit-keep")
                    .primary()
                    .xsmall()
                    .icon(Icon::new(IconName::Check))
                    .label(format!("{} ⌘↩", strings.inline_edit_keep))
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.close_inline_edit(false, window, cx);
                        this.focus_pane(this.active, window, cx);
                    }))
                    .into_any_element(),
            ]
        } else {
            vec![
                Button::new("inline-edit-close")
                    .ghost()
                    .xsmall()
                    .icon(Icon::new(IconName::X))
                    .tooltip(strings.inline_edit_close)
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.close_inline_edit(false, window, cx);
                        this.focus_pane(this.active, window, cx);
                    }))
                    .into_any_element(),
            ]
        };
        Some(
            div()
                .id("inline-edit")
                .absolute()
                .top_2()
                .left(px(56.))
                .right(px(28.))
                .max_w(px(680.))
                .p_2()
                .flex()
                .flex_col()
                .gap_1p5()
                .rounded(px(10.))
                .border_1()
                .border_color(theme.primary.opacity(0.55))
                .bg(theme.popover)
                .shadow_lg()
                .occlude()
                .key_context(INLINE_CONTEXT)
                .on_action(cx.listener(|this, _: &KeepInlineEdit, window, cx| {
                    this.close_inline_edit(false, window, cx);
                    this.focus_pane(this.active, window, cx);
                }))
                // Esc: puts the old lines back, as VS Code's inline chat.
                .on_action(
                    cx.listener(|this, _: &gpui_component::input::Escape, window, cx| {
                        this.close_inline_edit(true, window, cx);
                        this.focus_pane(this.active, window, cx);
                    }),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .text_xs()
                        .text_color(colors.muted)
                        .child(
                            Icon::new(IconName::Sparkles)
                                .xsmall()
                                .text_color(theme.primary),
                        )
                        .child(div().flex_none().child(lines))
                        .children(status)
                        .child(div().flex_1()),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_1()
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .child(Input::new(&edit.input).small().disabled(working)),
                        )
                        .children(buttons),
                )
                .into_any_element(),
        )
    }

    /// The ✦ of the commit field: Claude writes the message from the
    /// staged changes, or from all of them when none is staged.
    pub(super) fn write_commit_message(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(repo) = self.scm.repo.clone() else {
            return;
        };
        if self.scm.writing_message {
            return;
        }
        let strings = self.strings();
        self.scm.writing_message = true;
        cx.notify();
        let cwd = repo.root().to_path_buf();
        let answer = cx.background_executor().spawn(async move {
            let mut patch = repo.changes_patch(true);
            if patch.trim().is_empty() {
                patch = repo.changes_patch(false);
                let new_files = repo.untracked_files();
                if !new_files.is_empty() {
                    patch.push_str("\nNew files:\n");
                    for file in new_files {
                        patch.push_str(&file);
                        patch.push('\n');
                    }
                }
            }
            if patch.trim().is_empty() {
                return Err(None);
            }
            if patch.len() > PATCH_LIMIT {
                let cut = (PATCH_LIMIT..patch.len())
                    .find(|&at| patch.is_char_boundary(at))
                    .unwrap_or(patch.len());
                patch.truncate(cut);
                patch.push_str("\n… (the rest of the patch is left out)");
            }
            let recent: Vec<String> = repo
                .recent_messages(12)
                .iter()
                .filter_map(|message| message.lines().next().map(str::to_owned))
                .collect();
            let request = format!(
                "Write the commit message for this patch. Recent messages of the repository, the latest first:\n{}",
                recent.join("\n")
            );
            ask_once(&ClaudeRun::default(), &cwd, COMMIT_SYSTEM, &request, patch).map_err(Some)
        });
        cx.spawn_in(window, async move |this, cx| {
            let answer = answer.await;
            this.update_in(cx, |this, window, cx| {
                this.scm.writing_message = false;
                match answer {
                    Ok(message) => {
                        let message = without_fences(&message);
                        this.scm
                            .message
                            .update(cx, |field, cx| field.set_value(message.trim(), window, cx));
                    }
                    Err(None) => {
                        this.status_message = Some(strings.commit_nothing_to_describe.to_owned())
                    }
                    Err(Some(error)) => {
                        this.status_message = Some(if error.is_empty() {
                            strings.claude_not_found.to_owned()
                        } else {
                            error
                        })
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One test at a time uses the stand-in for Claude.
    static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());

    /// A stand-in for `claude` that reads its input and prints `answer`.
    fn fake_claude(folder: &Path, answer: &str) -> PathBuf {
        use std::os::unix::fs::PermissionsExt as _;
        let fake = folder.join("claude");
        std::fs::write(
            &fake,
            format!("#!/bin/sh\ncat >/dev/null\nprintf '{answer}\\n'\n"),
        )
        .unwrap();
        std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
        fake
    }

    #[gpui::test]
    fn cmd_i_rewrites_the_lines_and_esc_puts_them_back(cx: &mut gpui::TestAppContext) {
        let _turn = ONE_AT_A_TIME
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        cx.executor().allow_parking();
        let folder = tempfile::tempdir().unwrap();
        // Claude's answer in fences, as it sometimes writes it anyway.
        let fake = fake_claude(folder.path(), "```ts\\nlet total = 42;\\n```");
        *FAKE_CLAUDE.lock().unwrap() = Some(fake);
        let (_folder, _path, view, cx) =
            super::super::tests::window_with_file(cx, "app.ts", "let a = 1;\nlet b = 2;\n");
        let text = |view: &Entity<Workbench>, cx: &mut gpui::VisualTestContext| {
            view.read_with(cx, |view, cx| {
                view.panes[0].editor.read(cx).value().to_string()
            })
        };
        let ask = |view: &Entity<Workbench>, cx: &mut gpui::VisualTestContext| {
            cx.simulate_keystrokes("cmd-i");
            assert!(view.read_with(cx, |view, _| view.inline_edit.is_some()));
            cx.simulate_input("chiamala total");
            cx.simulate_keystrokes("enter");
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
            while view.read_with(cx, |view, _| {
                view.inline_edit
                    .as_ref()
                    .is_some_and(|edit| edit.written.is_none())
            }) {
                assert!(
                    std::time::Instant::now() < deadline,
                    "Claude did not answer"
                );
                cx.run_until_parked();
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
        };
        // The cursor on the second line: Claude's line takes its place.
        cx.simulate_keystrokes("down");
        ask(&view, cx);
        assert_eq!(text(&view, cx), "let a = 1;\nlet total = 42;\n");
        // Esc puts the old line back.
        cx.simulate_keystrokes("escape");
        assert_eq!(text(&view, cx), "let a = 1;\nlet b = 2;\n");
        assert!(view.read_with(cx, |view, _| view.inline_edit.is_none()));
        // Cmd+Enter keeps it.
        ask(&view, cx);
        cx.simulate_keystrokes("cmd-enter");
        assert_eq!(text(&view, cx), "let a = 1;\nlet total = 42;\n");
        assert!(view.read_with(cx, |view, _| view.inline_edit.is_none()));
        *FAKE_CLAUDE.lock().unwrap() = None;
    }

    #[gpui::test]
    fn claude_writes_the_commit_message(cx: &mut gpui::TestAppContext) {
        let _turn = ONE_AT_A_TIME
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        cx.executor().allow_parking();
        cx.update(|cx| {
            gpui_component::init(cx);
            crate::workbench_window::init(cx);
        });
        let folder = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(folder.path()).unwrap().join("repo");
        std::fs::create_dir_all(&root).unwrap();
        let git = |args: &[&str]| {
            let status = Command::new("git")
                .arg("-C")
                .arg(&root)
                .args(args)
                .output()
                .unwrap();
            assert!(status.status.success(), "git {args:?}");
        };
        git(&["init", "-q", "-b", "main"]);
        git(&["config", "user.name", "Test"]);
        git(&["config", "user.email", "test@example.com"]);
        git(&["config", "commit.gpgsign", "false"]);
        std::fs::write(root.join("a.txt"), "uno\n").unwrap();
        git(&["add", "."]);
        git(&["commit", "-q", "-m", "feat: first"]);
        std::fs::write(root.join("a.txt"), "due\n").unwrap();
        *FAKE_CLAUDE.lock().unwrap() = Some(fake_claude(folder.path(), "fix: say due"));
        let settings = settings::Settings::load("/nonexistent/keel-test/settings.json", None);
        let opening = Opening::new(Some(root.clone()), None);
        let (view, cx) =
            cx.add_window_view(|window, cx| Workbench::new(opening, settings, None, window, cx));
        view.update_in(cx, |view, window, cx| view.write_commit_message(window, cx));
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while view.read_with(cx, |view, _| view.scm.writing_message) {
            assert!(std::time::Instant::now() < deadline, "no message");
            cx.run_until_parked();
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        let message = view.read_with(cx, |view, cx| view.scm.message.read(cx).value().to_string());
        assert_eq!(message, "fix: say due");
        *FAKE_CLAUDE.lock().unwrap() = None;
    }

    #[test]
    fn the_lines_of_a_selection_whole() {
        let text = "uno\ndue\ntre\n";
        assert_eq!(whole_lines(text, 5..6), 4..7);
        // From the middle of a line to the start of the next one: one line.
        assert_eq!(whole_lines(text, 1..4), 0..3);
        assert_eq!(whole_lines(text, 1..9), 0..11);
        assert_eq!(whole_lines(text, 12..12), 12..12);
    }

    #[test]
    fn the_part_to_change_is_marked_and_fences_go() {
        let text = "a\nb\nc";
        let marked = marked_file(text, &(2..3));
        assert_eq!(marked, "a\n<<<KEEL_SELECTION\nb\nKEEL_SELECTION>>>\nc");
        assert_eq!(without_fences("```rust\nfn x() {}\n```\n"), "fn x() {}");
        assert_eq!(without_fences("fn x() {}\n"), "fn x() {}");
    }
}
