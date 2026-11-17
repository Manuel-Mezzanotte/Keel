//! Errors and warnings from the language servers: the underlines in the
//! editors, the Problems view, the counts in the status bar and F8.

use std::collections::{HashMap, HashSet};
use std::ops::Range;

use gpui::{AnyElement, StatefulInteractiveElement as _, uniform_list};
use gpui_component::highlighter::Diagnostic as EditorDiagnostic;
use gpui_component::input::Position as EditorPosition;
use gpui_component::{ActiveTheme as _, Icon, Sizable as _};
use gpui_kit_assets::IconName;
use lsp::lsp_types::{self, Diagnostic, DiagnosticSeverity};

use super::shell::{Colors, note};
use super::*;

actions!(keel, [NextProblem, PreviousProblem]);

const ROW_HEIGHT: f32 = 24.;

/// A row of the Problems view.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum ProblemRow {
    File(String),
    Problem { uri: String, index: usize },
}

/// One problem, flattened from the servers' lists.
#[derive(Clone, Debug)]
pub(super) struct Problem {
    pub(super) diagnostic: Diagnostic,
    pub(super) server: &'static str,
}

/// The Problems view: problems by file, and the files folded away.
#[derive(Default)]
pub(super) struct ProblemsView {
    /// Problems of each file, worst first, then by position.
    pub(super) files: Vec<(String, Vec<Problem>)>,
    pub(super) rows: Vec<ProblemRow>,
    collapsed: HashSet<String>,
    pub(super) errors: usize,
    pub(super) warnings: usize,
    /// For the Explorer: files and their folders with errors (`true`) or
    /// only warnings (`false`), and each file's count.
    pub(super) marks: HashMap<PathBuf, (bool, usize)>,
}

impl ProblemsView {
    fn rebuild_rows(&mut self) {
        self.rows.clear();
        for (uri, problems) in &self.files {
            self.rows.push(ProblemRow::File(uri.clone()));
            if !self.collapsed.contains(uri) {
                self.rows
                    .extend((0..problems.len()).map(|index| ProblemRow::Problem {
                        uri: uri.clone(),
                        index,
                    }));
            }
        }
    }

    fn problem(&self, uri: &str, index: usize) -> Option<&Problem> {
        self.files
            .iter()
            .find(|(file, _)| file == uri)
            .and_then(|(_, problems)| problems.get(index))
    }
}

/// VS Code leaves hints out of the Problems view and the counts.
fn is_shown(diagnostic: &Diagnostic) -> bool {
    diagnostic.severity != Some(DiagnosticSeverity::HINT)
}

fn severity_rank(diagnostic: &Diagnostic) -> u8 {
    match diagnostic.severity {
        Some(DiagnosticSeverity::ERROR) | None => 0,
        Some(DiagnosticSeverity::WARNING) => 1,
        Some(DiagnosticSeverity::INFORMATION) => 2,
        _ => 3,
    }
}

/// Line starts of a text, to turn LSP columns (UTF-16 units) into the
/// editor's (characters) and byte offsets quickly.
pub(super) struct LineIndex<'a> {
    text: &'a str,
    starts: Vec<usize>,
}

impl<'a> LineIndex<'a> {
    pub(super) fn new(text: &'a str) -> Self {
        let mut starts = vec![0];
        starts.extend(text.match_indices('\n').map(|(at, _)| at + 1));
        Self { text, starts }
    }

    fn line(&self, line: u32) -> &'a str {
        let Some(&start) = self.starts.get(line as usize) else {
            return "";
        };
        let end = self
            .starts
            .get(line as usize + 1)
            .copied()
            .unwrap_or(self.text.len());
        self.text[start..end].trim_end_matches(['\n', '\r'])
    }

    /// The byte offset of an LSP position, clamped to its line.
    pub(super) fn offset(&self, position: lsp_types::Position) -> usize {
        let Some(&start) = self.starts.get(position.line as usize) else {
            return self.text.len();
        };
        let line = self.line(position.line);
        let mut units = 0;
        for (at, character) in line.char_indices() {
            if units >= position.character as usize {
                return start + at;
            }
            units += character.len_utf16();
        }
        start + line.len()
    }

    /// The editor's position (column in characters) of a byte offset.
    pub(super) fn editor_position_of(&self, offset: usize) -> EditorPosition {
        let offset = offset.min(self.text.len());
        let line = match self.starts.binary_search(&offset) {
            Ok(line) => line,
            Err(next) => next - 1,
        };
        let start = self.starts[line];
        let mut end = offset;
        while !self.text.is_char_boundary(end) {
            end -= 1;
        }
        EditorPosition::new(line as u32, self.text[start..end].chars().count() as u32)
    }

    /// The editor's position (column in characters) of an LSP position.
    pub(super) fn editor_position(&self, position: lsp_types::Position) -> EditorPosition {
        let line = self.line(position.line);
        let mut units = 0;
        let mut characters = 0;
        for character in line.chars() {
            if units >= position.character as usize {
                break;
            }
            units += character.len_utf16();
            characters += 1;
        }
        EditorPosition::new(position.line, characters)
    }
}

/// Moves diagnostics along with an edit, so the underlines stay on their
/// words while the server works out the new ones.
pub(super) fn shift_diagnostics(diagnostics: &mut [Diagnostic], old: &str, new: &str) {
    let Some(change) = lsp::text::content_change(old, new) else {
        return;
    };
    let Some(range) = change.range else { return };
    let inserted_lines = change.text.matches('\n').count() as u32;
    let last_line = change.text.rsplit('\n').next().unwrap_or_default();
    let last_width = last_line.encode_utf16().count() as u32;
    let new_end = if inserted_lines == 0 {
        lsp_types::Position::new(range.start.line, range.start.character + last_width)
    } else {
        lsp_types::Position::new(range.start.line + inserted_lines, last_width)
    };
    let map = |position: lsp_types::Position, is_end: bool| -> lsp_types::Position {
        if position < range.start || (position == range.start && !is_end) {
            return position;
        }
        if position < range.end {
            return if is_end { new_end } else { range.start };
        }
        if position.line == range.end.line {
            lsp_types::Position::new(
                new_end.line,
                new_end.character + (position.character - range.end.character),
            )
        } else {
            lsp_types::Position::new(
                position.line + new_end.line - range.end.line,
                position.character,
            )
        }
    };
    for diagnostic in diagnostics {
        let start = map(diagnostic.range.start, false);
        let end = map(diagnostic.range.end, true);
        diagnostic.range = lsp_types::Range::new(start, end.max(start));
    }
}

impl Workbench {
    /// Diagnostics of `uri` changed: its editors and the Problems view follow.
    pub(super) fn diagnostics_changed(&mut self, uri: &str, cx: &mut Context<Self>) {
        self.claude_diagnostics_changed(uri);
        self.languages
            .state
            .mirror_diagnostics(uri, self.languages.diagnostics.get(uri));
        self.rebuild_problems();
        for index in 0..self.panes.len() {
            if self.panes[index].uri == uri {
                self.underline_problems(index, cx);
            }
        }
        cx.notify();
    }

    /// The files and counts of the Problems view, from the servers' lists.
    fn rebuild_problems(&mut self) {
        let mut files: Vec<(String, Vec<Problem>)> = self
            .languages
            .diagnostics
            .iter()
            .map(|(uri, by_server)| {
                let mut problems: Vec<Problem> = by_server
                    .iter()
                    .flat_map(|(server, diagnostics)| {
                        diagnostics
                            .iter()
                            .filter(|diagnostic| is_shown(diagnostic))
                            .map(|diagnostic| Problem {
                                diagnostic: diagnostic.clone(),
                                server,
                            })
                    })
                    .collect();
                problems.sort_by_key(|problem| {
                    (
                        problem.diagnostic.range.start,
                        severity_rank(&problem.diagnostic),
                    )
                });
                (uri.clone(), problems)
            })
            .filter(|(_, problems)| !problems.is_empty())
            .collect();
        files.sort_by(|(a, _), (b, _)| a.cmp(b));
        let count = |severity: DiagnosticSeverity| {
            files
                .iter()
                .flat_map(|(_, problems)| problems)
                .filter(|problem| {
                    problem
                        .diagnostic
                        .severity
                        .unwrap_or(DiagnosticSeverity::ERROR)
                        == severity
                })
                .count()
        };
        self.problems.errors = count(DiagnosticSeverity::ERROR);
        self.problems.warnings = count(DiagnosticSeverity::WARNING);
        let mut marks: HashMap<PathBuf, (bool, usize)> = HashMap::new();
        for (uri, problems) in &files {
            let Some(path) = Uri::parse(uri).and_then(|parsed| parsed.file_path()) else {
                continue;
            };
            let worst = problems
                .iter()
                .map(|problem| severity_rank(&problem.diagnostic))
                .min();
            let has_error = worst == Some(0);
            if worst.is_none_or(|rank| rank > 1) {
                continue;
            }
            let counted = problems
                .iter()
                .filter(|problem| severity_rank(&problem.diagnostic) <= 1)
                .count();
            marks.insert(path.clone(), (has_error, counted));
            // Folders take the worst of what they hold, without a count.
            for folder in path.ancestors().skip(1) {
                if self
                    .project
                    .root
                    .as_deref()
                    .is_some_and(|root| !folder.starts_with(root))
                {
                    break;
                }
                let mark = marks.entry(folder.to_path_buf()).or_insert((false, 0));
                mark.0 |= has_error;
            }
        }
        self.problems.marks = marks;
        self.problems.files = files;
        self.problems.rebuild_rows();
    }

    /// Gives the editor of pane `index` the underlines of its file.
    pub(super) fn underline_problems(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(pane) = self.panes.get(index) else {
            return;
        };
        self.underline_editor(&pane.uri, &pane.editor, cx);
    }

    /// Gives `editor` the underlines of `uri`.
    pub(super) fn underline_editor(&self, uri: &str, editor: &Entity<EditorState>, cx: &mut App) {
        let diagnostics: Vec<Diagnostic> = self
            .languages
            .diagnostics
            .get(uri)
            .into_iter()
            .flat_map(|by_server| by_server.values().flatten())
            .filter(|diagnostic| is_shown(diagnostic))
            .cloned()
            .collect();
        editor.update(cx, |editor, cx| {
            let text = editor.value().to_string();
            let lines = LineIndex::new(&text);
            let Some(set) = editor.diagnostics_mut() else {
                return;
            };
            set.clear();
            for diagnostic in diagnostics {
                let start = lines.editor_position(diagnostic.range.start);
                let end = lines.editor_position(diagnostic.range.end);
                let mut shown = EditorDiagnostic::from(diagnostic);
                shown.range = start..end;
                set.push(shown);
            }
            cx.notify();
        });
    }

    /// Keeps the underlines of `uri` in place across an edit: called as the
    /// text changes from `old` to `new`.
    pub(super) fn shift_problems(&mut self, uri: &str, old: &str, new: &str) {
        let Some(by_server) = self.languages.diagnostics.get_mut(uri) else {
            return;
        };
        for diagnostics in by_server.values_mut() {
            shift_diagnostics(diagnostics, old, new);
        }
        self.languages
            .state
            .mirror_diagnostics(uri, self.languages.diagnostics.get(uri));
        self.rebuild_problems();
    }

    /// Opens `uri` and selects an LSP range in it.
    pub(super) fn open_lsp_range(
        &mut self,
        uri: &str,
        range: lsp_types::Range,
        open_as: OpenAs,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // A class of a library, which only jdtls can show.
        if uri.starts_with("jdt:") {
            self.open_class_file(uri.to_owned(), range, window, cx);
            return;
        }
        let Some(path) = Uri::parse(uri).and_then(|parsed| parsed.file_path()) else {
            return;
        };
        if self
            .panes
            .get(self.active)
            .is_none_or(|pane| pane.uri != uri)
        {
            self.open_file_as(&path, open_as, window, cx);
        }
        self.select_lsp_range(range, window, cx);
    }

    /// Selects an LSP range in the active editor, bringing it into view.
    pub(super) fn select_lsp_range(
        &mut self,
        range: lsp_types::Range,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(editor) = self.active_editor() else {
            return;
        };
        editor.update(cx, |editor, cx| {
            let text = editor.value().to_string();
            let lines = LineIndex::new(&text);
            let start = lines.offset(range.start);
            let end = lines.offset(range.end);
            editor.set_cursor_position(lines.editor_position(range.start), window, cx);
            editor.set_selected_range(start..end, cx);
            editor.focus(window, cx);
        });
        self.publish_selection_if_changed(self.active, cx);
    }

    /// F8 and Shift+F8: the next or previous problem after the cursor, in
    /// this file and then in the next ones of the Problems view.
    fn go_to_problem(&mut self, forward: bool, window: &mut Window, cx: &mut Context<Self>) {
        let all: Vec<(String, lsp_types::Range)> = self
            .problems
            .files
            .iter()
            .flat_map(|(uri, problems)| {
                problems
                    .iter()
                    .map(|problem| (uri.clone(), problem.diagnostic.range))
            })
            .collect();
        if all.is_empty() {
            return;
        }
        let here = self.panes.get(self.active).map(|pane| {
            let editor = pane.editor.read(cx);
            let text = editor.value().to_string();
            let cursor = editor.selected_range().start;
            let position = workbench::position::position_of(&text, cursor);
            (
                pane.uri.clone(),
                lsp_types::Position::new(position.line, position.character),
            )
        });
        let order = |(uri, range): &(String, lsp_types::Range)| (uri.clone(), range.start);
        let target = match &here {
            Some((uri, position)) => {
                let key = (uri.clone(), *position);
                if forward {
                    all.iter()
                        .find(|problem| order(problem) > key)
                        .or_else(|| all.first())
                } else {
                    all.iter()
                        .rev()
                        .find(|problem| order(problem) < key)
                        .or_else(|| all.last())
                }
            }
            None => all.first(),
        };
        let Some((uri, range)) = target.cloned() else {
            return;
        };
        self.open_lsp_range(&uri, range, OpenAs::Normal, window, cx);
        if let Some(problem) = self
            .problems
            .files
            .iter()
            .find(|(file, _)| *file == uri)
            .and_then(|(_, problems)| {
                problems
                    .iter()
                    .find(|problem| problem.diagnostic.range == range)
            })
        {
            self.status_message = Some(
                problem
                    .diagnostic
                    .message
                    .lines()
                    .next()
                    .unwrap_or_default()
                    .to_owned(),
            );
        }
        cx.notify();
    }

    pub(super) fn problem_actions(&self, root: gpui::Div, cx: &mut Context<Self>) -> gpui::Div {
        root.on_action(
            cx.listener(|this, _: &NextProblem, window, cx| this.go_to_problem(true, window, cx)),
        )
        .on_action(cx.listener(|this, _: &PreviousProblem, window, cx| {
            this.go_to_problem(false, window, cx)
        }))
    }

    // -----------------------------------------------------------------
    // Drawing

    pub(super) fn render_problems(&self, colors: Colors, cx: &mut Context<Self>) -> AnyElement {
        if self.problems.files.is_empty() {
            return note(self.strings().problems_none.to_owned(), colors).into_any_element();
        }
        uniform_list(
            "problems",
            self.problems.rows.len(),
            cx.processor(|this, range: Range<usize>, _, cx| this.render_problem_rows(range, cx)),
        )
        .size_full()
        .into_any_element()
    }

    fn render_problem_rows(
        &mut self,
        range: Range<usize>,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let colors = Colors::new(cx);
        range
            .filter_map(|index| {
                let row = self.problems.rows.get(index)?.clone();
                Some(match row {
                    ProblemRow::File(uri) => self.render_problem_file(index, &uri, colors, cx),
                    ProblemRow::Problem { uri, index: item } => {
                        self.render_problem(index, &uri, item, colors, cx)?
                    }
                })
            })
            .collect()
    }

    fn render_problem_file(
        &self,
        index: usize,
        uri: &str,
        colors: Colors,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let path = Uri::parse(uri).and_then(|parsed| parsed.file_path());
        let shown = match (&path, &self.project.root) {
            (Some(path), Some(root)) => path.strip_prefix(root).unwrap_or(path).to_path_buf(),
            (Some(path), None) => path.clone(),
            (None, _) => PathBuf::from(uri),
        };
        let name = shown
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        let folder = shown
            .parent()
            .map(|folder| folder.to_string_lossy().into_owned())
            .unwrap_or_default();
        let count = self
            .problems
            .files
            .iter()
            .find(|(file, _)| file == uri)
            .map_or(0, |(_, problems)| problems.len());
        let collapsed = self.problems.collapsed.contains(uri);
        let toggled = uri.to_owned();
        div()
            .id(("problem-file", index))
            .h(px(ROW_HEIGHT))
            .px_2()
            .flex()
            .items_center()
            .gap_1()
            .rounded(px(6.))
            .text_sm()
            .cursor_pointer()
            .hover(|row| row.bg(colors.hover))
            .on_click(cx.listener(move |this, _, _, cx| {
                if !this.problems.collapsed.remove(&toggled) {
                    this.problems.collapsed.insert(toggled.clone());
                }
                this.problems.rebuild_rows();
                cx.notify();
            }))
            .child(
                Icon::new(if collapsed {
                    IconName::ChevronRight
                } else {
                    IconName::ChevronDown
                })
                .xsmall()
                .text_color(colors.faint),
            )
            .child(div().flex_none().text_color(colors.text).child(name))
            .child(
                div()
                    .min_w_0()
                    .truncate()
                    .text_xs()
                    .text_color(colors.faint)
                    .child(folder),
            )
            .child(
                div()
                    .flex_none()
                    .px_1p5()
                    .rounded(px(8.))
                    .bg(colors.hover)
                    .text_xs()
                    .text_color(colors.muted)
                    .child(count.to_string()),
            )
            .into_any_element()
    }

    fn render_problem(
        &self,
        index: usize,
        uri: &str,
        item: usize,
        colors: Colors,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let problem = self.problems.problem(uri, item)?;
        let diagnostic = &problem.diagnostic;
        let (icon, color) = severity_icon(diagnostic.severity, cx);
        let mut detail = diagnostic
            .source
            .clone()
            .unwrap_or_else(|| problem.server.to_owned());
        if let Some(code) = &diagnostic.code {
            let code = match code {
                lsp_types::NumberOrString::Number(number) => number.to_string(),
                lsp_types::NumberOrString::String(text) => text.clone(),
            };
            detail.push_str(&format!("({code})"));
        }
        let position = format!(
            "[{}, {}]",
            diagnostic.range.start.line + 1,
            diagnostic.range.start.character + 1
        );
        let message = diagnostic
            .message
            .lines()
            .next()
            .unwrap_or_default()
            .to_owned();
        let (uri, range) = (uri.to_owned(), diagnostic.range);
        Some(
            div()
                .id(("problem", index))
                .h(px(ROW_HEIGHT))
                .pl(px(26.))
                .pr_2()
                .flex()
                .items_center()
                .gap_1p5()
                .rounded(px(6.))
                .text_sm()
                .cursor_pointer()
                .hover(|row| row.bg(colors.hover))
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.open_lsp_range(&uri, range, OpenAs::Preview, window, cx)
                }))
                .child(Icon::new(icon).xsmall().text_color(color))
                .child(
                    div()
                        .min_w_0()
                        .truncate()
                        .text_color(colors.text)
                        .child(message),
                )
                .child(
                    div()
                        .flex_none()
                        .text_xs()
                        .text_color(colors.faint)
                        .child(detail),
                )
                .child(
                    div()
                        .flex_none()
                        .text_xs()
                        .text_color(colors.faint)
                        .child(position),
                )
                .into_any_element(),
        )
    }

    /// The error and warning counts of the status bar; a click opens the
    /// Problems view.
    pub(super) fn render_problem_counts(
        &self,
        colors: Colors,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = cx.theme();
        let count = |icon: IconName, value: usize, color: Hsla| {
            div()
                .flex()
                .items_center()
                .gap_1()
                .child(Icon::new(icon).xsmall().text_color(if value > 0 {
                    color
                } else {
                    colors.muted
                }))
                .child(value.to_string())
        };
        div()
            .id("status-problems")
            .flex_none()
            .flex()
            .items_center()
            .gap_2()
            .cursor_pointer()
            .hover(|item| item.text_color(colors.accent))
            .child(count(IconName::CircleX, self.problems.errors, theme.danger))
            .child(count(
                IconName::TriangleAlert,
                self.problems.warnings,
                theme.warning,
            ))
            .on_click(cx.listener(|this, _, _, cx| {
                this.toggle_panel_view(crate::layout::PanelView::Problems, cx)
            }))
            .into_any_element()
    }
}

fn severity_icon(severity: Option<DiagnosticSeverity>, cx: &App) -> (IconName, Hsla) {
    let theme = cx.theme();
    match severity {
        Some(DiagnosticSeverity::WARNING) => (IconName::TriangleAlert, theme.warning),
        Some(DiagnosticSeverity::INFORMATION) | Some(DiagnosticSeverity::HINT) => {
            (IconName::Info, theme.info)
        }
        _ => (IconName::CircleX, theme.danger),
    }
}

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use gpui::{TestAppContext, VisualTestContext};
    use lsp_types::{Position, Range as LspRange};

    use super::*;

    fn diagnostic(start: (u32, u32), end: (u32, u32)) -> Diagnostic {
        Diagnostic {
            range: LspRange::new(Position::new(start.0, start.1), Position::new(end.0, end.1)),
            message: "problema".into(),
            ..Default::default()
        }
    }

    fn shifted(
        old: &str,
        new: &str,
        start: (u32, u32),
        end: (u32, u32),
    ) -> ((u32, u32), (u32, u32)) {
        let mut diagnostics = [diagnostic(start, end)];
        shift_diagnostics(&mut diagnostics, old, new);
        let range = diagnostics[0].range;
        (
            (range.start.line, range.start.character),
            (range.end.line, range.end.character),
        )
    }

    #[test]
    fn underlines_move_with_the_text() {
        let old = "let a = 1;\nlet sbagliato = 2;\n";
        // Typing before it on the same line pushes it right.
        assert_eq!(
            shifted(
                old,
                "let a = 1;\nconst let sbagliato = 2;\n",
                (1, 4),
                (1, 13)
            ),
            ((1, 10), (1, 19))
        );
        // A new line above pushes it down.
        assert_eq!(
            shifted(
                old,
                "// nota\nlet a = 1;\nlet sbagliato = 2;\n",
                (1, 4),
                (1, 13)
            ),
            ((2, 4), (2, 13))
        );
        // Typing after it leaves it alone.
        assert_eq!(
            shifted(
                old,
                "let a = 1;\nlet sbagliato = 2; // ok\n",
                (1, 4),
                (1, 13)
            ),
            ((1, 4), (1, 13))
        );
        // Typing inside it stretches it.
        assert_eq!(
            shifted(
                old,
                "let a = 1;\nlet sbagliatissimo = 2;\n",
                (1, 4),
                (1, 13)
            ),
            ((1, 4), (1, 18))
        );
        // Deleting the line above pulls it up.
        assert_eq!(
            shifted(old, "let sbagliato = 2;\n", (1, 4), (1, 13)),
            ((0, 4), (0, 13))
        );
    }

    #[test]
    fn columns_turn_from_utf16_into_characters_and_bytes() {
        let text = "a\n🚀 caffè x\n";
        let lines = LineIndex::new(text);
        // The rocket is two UTF-16 units but one character, four bytes.
        let x = Position::new(1, 9);
        assert_eq!(lines.editor_position(x), EditorPosition::new(1, 8));
        assert_eq!(&text[lines.offset(x)..lines.offset(x) + 1], "x");
        assert_eq!(lines.offset(Position::new(1, 99)), text.len() - 1);
        assert_eq!(lines.offset(Position::new(9, 0)), text.len());
        // And back from bytes: "x" is the 9th character of line 1.
        assert_eq!(
            lines.editor_position_of(text.find('x').unwrap()),
            EditorPosition::new(1, 8)
        );
    }

    fn window(
        cx: &mut TestAppContext,
    ) -> (
        tempfile::TempDir,
        PathBuf,
        Entity<Workbench>,
        &mut VisualTestContext,
    ) {
        cx.executor().allow_parking();
        cx.update(|cx| {
            gpui_component::init(cx);
            language::register_grammars();
            crate::workbench_window::init(cx);
        });
        let folder = tempfile::tempdir().unwrap();
        let base = std::fs::canonicalize(folder.path()).unwrap();
        std::fs::write(base.join("a.ts"), "// TODO uno\nconst a = 1;\n").unwrap();
        std::fs::write(base.join("b.ts"), "const b = 2; // TODO due\n").unwrap();
        let settings = Settings::load(base.join("settings.json"), None);
        let opening = Opening::new(Some(base.clone()), None);
        let (view, cx) = cx.add_window_view(|window, cx| {
            let mut workbench = Workbench::new(opening, settings, None, window, cx);
            workbench.languages.fake = true;
            workbench
        });
        (folder, base, view, cx)
    }

    fn wait_until(
        cx: &mut VisualTestContext,
        what: &str,
        check: impl Fn(&mut VisualTestContext) -> bool,
    ) {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            cx.run_until_parked();
            if check(cx) {
                return;
            }
            assert!(Instant::now() < deadline, "gave up waiting for {what}");
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    fn underlines(view: &Entity<Workbench>, cx: &mut VisualTestContext) -> Vec<Range<usize>> {
        view.read_with(cx, |view, cx| {
            let editor = view.active_editor().unwrap();
            editor
                .read(cx)
                .diagnostics()
                .map(|set| set.iter().map(|entry| entry.range.clone()).collect())
                .unwrap_or_default()
        })
    }

    #[gpui::test]
    fn problems_reach_the_editor_the_view_and_the_status_bar(cx: &mut TestAppContext) {
        let (_folder, base, view, cx) = window(cx);
        for name in ["a.ts", "b.ts"] {
            let path = base.join(name);
            view.update_in(cx, |view, window, cx| view.open_file(&path, window, cx));
        }
        wait_until(cx, "two warnings", |cx| {
            view.read_with(cx, |view, _| view.problems.warnings == 2)
        });
        view.read_with(cx, |view, _| {
            assert_eq!(view.problems.errors, 0);
            assert_eq!(view.problems.files.len(), 2);
            // Each file, then its problem.
            assert_eq!(view.problems.rows.len(), 4);
            // The Explorer marks both files in yellow, with their count.
            assert_eq!(
                view.problems.marks.get(&base.join("a.ts")),
                Some(&(false, 1))
            );
            assert_eq!(view.problems.marks.get(&base), Some(&(false, 0)));
        });
        // b.ts is in front: "TODO" from byte 16.
        assert_eq!(underlines(&view, cx), vec![16..20]);

        // Typing before the word moves the underline at once.
        view.update_in(cx, |view, window, cx| {
            let editor = view.active_editor().unwrap();
            editor.update(cx, |editor, cx| {
                editor.set_value("let x;\nconst b = 2; // TODO due\n", window, cx)
            });
            let uri = view.panes[view.active].uri.clone();
            view.sync_document_from(&uri, None, window, cx);
            assert_eq!(
                view.problems
                    .files
                    .iter()
                    .find(|(file, _)| *file == uri)
                    .unwrap()
                    .1[0]
                    .diagnostic
                    .range
                    .start,
                Position::new(1, 16)
            );
        });
        assert_eq!(underlines(&view, cx), vec![23..27]);

        // F8 goes to the next problem, in the other file.
        view.update_in(cx, |view, window, cx| {
            let editor = view.active_editor().unwrap();
            editor.update(cx, |editor, cx| editor.set_selected_range(30..30, cx));
            view.go_to_problem(true, window, cx);
        });
        view.read_with(cx, |view, cx| {
            assert!(view.panes[view.active].uri.ends_with("a.ts"));
            let editor = view.active_editor().unwrap();
            assert_eq!(editor.read(cx).selected_range(), 3..7);
            assert_eq!(view.status_message.as_deref(), Some("todo found"));
        });
    }
}
