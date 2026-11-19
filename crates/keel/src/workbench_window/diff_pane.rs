//! Keel's differences view, drawn from the two whole versions of a file.
//! Side by side, each column takes half of the tab and wraps its long
//! lines, so neither is ever cut off; in one column, what went comes
//! before what came. Code keeps its syntax colours, changed words stand
//! out inside changed lines, unchanged lines fold with arrows to open
//! them, and each block has its own Stage and Undo.

use std::cell::RefCell;
use std::collections::HashMap;
use std::ops::Range;
use std::rc::Rc;

use gpui::{
    AnyElement, HighlightStyle, ListAlignment, ListOffset, ListState, Pixels, SharedString,
    StatefulInteractiveElement as _, StyledText, WeakEntity, list, relative,
};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::highlighter::SyntaxHighlighter;
use gpui_component::{Icon, Sizable as _};
use gpui_kit_assets::IconName;

use super::diff_model::{self, Expand, FileDiff, Row};
use super::shell::Colors;
use super::*;

/// Files above this size are shown without syntax colours.
const HIGHLIGHT_LIMIT: usize = 1_000_000;

/// The column of line numbers, in characters.
const NUMBER_CHARS: f32 = 5.;

/// What a block's buttons do, by where the differences come from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum BlockActions {
    /// Changes not staged: stage the block, or undo it in the file.
    StageOrRevert,
    /// Staged changes: take the block out of the stage.
    Unstage,
    /// A commit, or someone's proposal: nothing to do on a block.
    None,
}

/// A line's syntax colours, from the start of the line.
type LineStyles = Rc<Vec<(Range<usize>, HighlightStyle)>>;
/// What a file kept of its view: the lines opened, and whether it was
/// closed to its header.
type Kept = (HashMap<usize, (usize, usize)>, bool);

/// One version of a file with its syntax colours, worked out once.
pub(super) struct Side {
    text: String,
    /// Where each line starts in `text`.
    starts: Vec<usize>,
    highlighter: Option<SyntaxHighlighter>,
    styles: RefCell<HashMap<usize, LineStyles>>,
}

impl Side {
    pub(super) fn new(lines: &[String], language: &str) -> Self {
        let mut text = String::new();
        let mut starts = Vec::with_capacity(lines.len());
        for line in lines {
            starts.push(text.len());
            text.push_str(line);
            text.push('\n');
        }
        let highlighter = (text.len() <= HIGHLIGHT_LIMIT && !language.is_empty()).then(|| {
            let mut highlighter = SyntaxHighlighter::new(language);
            highlighter.update(None, &gpui_component::Rope::from(text.as_str()), None);
            highlighter
        });
        Self {
            text,
            starts,
            highlighter,
            styles: RefCell::default(),
        }
    }

    /// A line's colours, from the start of the line.
    pub(super) fn styles(&self, line: usize, cx: &App) -> LineStyles {
        if let Some(styles) = self.styles.borrow().get(&line) {
            return styles.clone();
        }
        let styles = match (&self.highlighter, self.starts.get(line)) {
            (Some(highlighter), Some(&start)) => {
                let end = self
                    .starts
                    .get(line + 1)
                    .map_or(self.text.len(), |next| next - 1);
                let theme = cx.theme().highlight_theme.clone();
                highlighter
                    .styles(&(start..end), &*theme)
                    .into_iter()
                    .filter(|(range, _)| range.start < range.end)
                    .map(|(range, style)| (range.start - start..range.end - start, style))
                    .collect()
            }
            _ => Vec::new(),
        };
        let styles = Rc::new(styles);
        self.styles.borrow_mut().insert(line, styles.clone());
        styles
    }
}

struct DocState {
    files: Vec<FileDiff>,
    rows: Vec<Row>,
    split: bool,
    headers: bool,
    /// Each file's two sides, made when first drawn: old, new.
    sides: HashMap<usize, Rc<[Side; 2]>>,
    /// The theme the colours were worked out for.
    theme: SharedString,
}

impl DocState {
    fn rebuild(&mut self) {
        self.rows = diff_model::rows(&self.files, self.split, self.headers);
    }

    fn sides(&mut self, file: usize, cx: &App) -> Rc<[Side; 2]> {
        let theme = cx.theme().theme_name().clone();
        if theme != self.theme {
            self.theme = theme;
            self.sides.clear();
        }
        self.sides
            .entry(file)
            .or_insert_with(|| {
                let diff = &self.files[file];
                let language = Language::for_path(Path::new(&diff.path)).highlighter_name();
                Rc::new([
                    Side::new(&diff.old, language),
                    Side::new(&diff.new, language),
                ])
            })
            .clone()
    }
}

/// A differences view of one file or of several, with its scroll.
#[derive(Clone)]
pub(super) struct DiffDoc {
    state: Rc<RefCell<DocState>>,
    pub(super) list: ListState,
}

impl DiffDoc {
    pub(super) fn new(files: Vec<FileDiff>, split: bool, headers: bool) -> Self {
        let mut state = DocState {
            files,
            rows: Vec::new(),
            split,
            headers,
            sides: HashMap::new(),
            theme: SharedString::default(),
        };
        state.rebuild();
        let list = ListState::new(state.rows.len(), ListAlignment::Top, px(600.));
        Self {
            state: Rc::new(RefCell::new(state)),
            list,
        }
    }

    pub(super) fn files_len(&self) -> usize {
        self.state.borrow().files.len()
    }

    /// The counts of the whole view: lines added and taken away.
    pub(super) fn counts(&self) -> (usize, usize) {
        let state = self.state.borrow();
        state.files.iter().fold((0, 0), |(added, removed), file| {
            (added + file.additions(), removed + file.deletions())
        })
    }

    /// Takes new versions, keeping what was opened and where it was.
    pub(super) fn set_files(&self, files: Vec<FileDiff>) {
        let mut state = self.state.borrow_mut();
        let old: HashMap<String, Kept> = state
            .files
            .drain(..)
            .map(|file| (file.path, (file.revealed, file.collapsed)))
            .collect();
        state.files = files;
        for file in &mut state.files {
            if let Some((revealed, collapsed)) = old.get(&file.path) {
                file.revealed = revealed.clone();
                file.collapsed = *collapsed;
            }
        }
        state.sides.clear();
        self.rebuild_keeping_place(&mut state);
    }

    /// Side by side or in one column.
    pub(super) fn set_split(&self, split: bool) {
        let mut state = self.state.borrow_mut();
        if state.split == split {
            return;
        }
        state.split = split;
        self.rebuild_keeping_place(&mut state);
    }

    fn rebuild_keeping_place(&self, state: &mut DocState) {
        let top = self.list.logical_scroll_top();
        // The line at the top of the view, to find it again.
        let anchor = state.rows.get(top.item_ix).cloned();
        state.rebuild();
        self.list.reset(state.rows.len());
        let found = anchor.and_then(|anchor| {
            let new_line = |row: &Row| match row {
                Row::Same { file, new, .. } | Row::Added { file, new, .. } => Some((*file, *new)),
                Row::Pair {
                    file,
                    new: Some(new),
                    ..
                } => Some((*file, *new)),
                Row::Fold { file, new, .. } => Some((*file, *new)),
                _ => None,
            };
            let wanted = new_line(&anchor)?;
            state
                .rows
                .iter()
                .position(|row| new_line(row).is_some_and(|line| line >= wanted))
        });
        if let Some(item_ix) = found {
            self.list.scroll_to(ListOffset {
                item_ix,
                offset_in_item: top.offset_in_item,
            });
        }
    }

    fn expand(&self, file: usize, at: usize, how: Expand) {
        let mut state = self.state.borrow_mut();
        if let Some(diff) = state.files.get_mut(file) {
            diff.expand(at, how);
        }
        let top = self.list.logical_scroll_top();
        state.rebuild();
        self.list.reset(state.rows.len());
        self.list.scroll_to(top);
    }

    fn toggle_file(&self, file: usize) {
        let mut state = self.state.borrow_mut();
        if let Some(diff) = state.files.get_mut(file) {
            diff.collapsed = !diff.collapsed;
        }
        let top = self.list.logical_scroll_top();
        state.rebuild();
        self.list.reset(state.rows.len());
        self.list.scroll_to(top);
    }

    /// The first row of the next changed block below the top of the view,
    /// or of the one before; the view goes to it, a little above.
    pub(super) fn go_to_change(&self, forward: bool) -> Option<usize> {
        let state = self.state.borrow();
        let top = self.list.logical_scroll_top().item_ix;
        let starts: Vec<usize> = state
            .rows
            .iter()
            .enumerate()
            .filter(|(_, row)| row.starts_hunk())
            .map(|(index, _)| index)
            .collect();
        // The view shows a block from a few lines above it.
        let shown = top + 2;
        let target = if forward {
            starts.iter().copied().find(|&start| start > shown)
        } else {
            starts.iter().rev().copied().find(|&start| start < shown)
        }
        .or_else(|| {
            // Round the end, as VS Code does.
            if forward {
                starts.first().copied()
            } else {
                starts.last().copied()
            }
        });
        if let Some(target) = target {
            self.list.scroll_to(ListOffset {
                item_ix: target.saturating_sub(2),
                offset_in_item: px(0.),
            });
        }
        target
    }

    /// Goes to a line of the new version, a little below the top.
    pub(super) fn scroll_to_new_line(&self, line: usize) {
        let state = self.state.borrow();
        let found = state.rows.iter().position(|row| match row {
            Row::Same { new, .. } | Row::Added { new, .. } | Row::Fold { new, .. } => *new >= line,
            Row::Pair { new: Some(new), .. } => *new >= line,
            _ => false,
        });
        if let Some(index) = found {
            self.list.scroll_to(ListOffset {
                item_ix: index.saturating_sub(2),
                offset_in_item: px(0.),
            });
        }
    }

    /// The file and new line at the top of the view, to open the file there.
    pub(super) fn line_in_view(&self) -> Option<(String, usize)> {
        let state = self.state.borrow();
        let top = self.list.logical_scroll_top().item_ix;
        state.rows[top.min(state.rows.len().saturating_sub(1))..]
            .iter()
            .find_map(|row| match row {
                Row::Same { file, new, .. }
                | Row::Added { file, new, .. }
                | Row::Pair {
                    file,
                    new: Some(new),
                    ..
                }
                | Row::Fold { file, new, .. } => Some((state.files[*file].path.clone(), *new)),
                _ => None,
            })
    }

    /// The texts and block, to stage or undo it.
    pub(super) fn block(
        &self,
        file: usize,
        hunk: usize,
    ) -> Option<(String, String, String, git::Hunk)> {
        let state = self.state.borrow();
        let diff = state.files.get(file)?;
        Some((
            diff.path.clone(),
            diff.old_text.clone(),
            diff.new_text.clone(),
            diff.hunks.get(hunk)?.clone(),
        ))
    }

    #[cfg(test)]
    pub(super) fn rows(&self) -> Vec<Row> {
        self.state.borrow().rows.clone()
    }
}

/// What every row needs to be drawn.
#[derive(Clone)]
struct Look {
    colors: Colors,
    font: SharedString,
    font_size: Pixels,
    line_height: Pixels,
    number_width: Pixels,
    actions: BlockActions,
    workbench: WeakEntity<Workbench>,
    uri: String,
    strings: &'static Strings,
}

impl Workbench {
    /// The differences of a tab, drawn by Keel.
    pub(super) fn render_diff_doc(
        &self,
        uri: &str,
        doc: &DiffDoc,
        actions: BlockActions,
        colors: Colors,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let split = self.diff_split && self.diff_fits_two_columns(window);
        doc.set_split(split);
        let font_size = px(self.font_size.max(9.) - 1.);
        let look = Look {
            colors,
            font: cx.theme().mono_font_family.clone(),
            font_size,
            line_height: (font_size * 1.6).round(),
            number_width: font_size * 0.62 * NUMBER_CHARS + px(16.),
            actions,
            workbench: cx.weak_entity(),
            uri: uri.to_owned(),
            strings: self.strings(),
        };
        let state = doc.state.clone();
        let this_doc = doc.clone();
        list(doc.list.clone(), move |index, _, cx| {
            render_row(&state, &this_doc, index, &look, cx)
        })
        .size_full()
        .into_any_element()
    }

    /// Whether two columns fit: under 900 points one is better, as VS
    /// Code does.
    pub(super) fn diff_fits_two_columns(&self, window: &Window) -> bool {
        self.editor_area_width(window) >= 900.
    }

    /// The width the editors have, for choosing one column or two.
    fn editor_area_width(&self, window: &Window) -> f32 {
        let mut width = window.viewport_size().width.as_f32();
        if self.layout.sidebar {
            width -= self.sidebar_width;
        }
        if self.layout.agent {
            width -= self.agent_width;
        }
        width / self.panes.len().max(1) as f32
    }

    /// Stages a block of a differences tab, or takes it out of the
    /// stage, or undoes it in the file.
    pub(super) fn diff_block_action(
        &mut self,
        uri: &str,
        file: usize,
        hunk: usize,
        undo: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(Backing::Diff(tab)) = self.documents.get(uri).map(|document| &document.backing)
        else {
            return;
        };
        let Some(doc) = tab.doc.clone() else {
            return;
        };
        let source = tab.source.clone();
        let Some((path, old, new, block)) = doc.block(file, hunk) else {
            return;
        };
        let strings = self.strings();
        match (source, undo) {
            // Stage: the staged version takes the block.
            (super::diff_view::DiffSource::Changes, false) => {
                let staged = git::apply_hunk(&old, &new, &block);
                self.run_git(
                    strings.scm_stage,
                    move |repo| repo.stage_text(&path, &staged),
                    |_, _, _| {},
                    window,
                    cx,
                );
            }
            // Undo: the file goes back to the staged version there.
            (super::diff_view::DiffSource::Changes, true) => {
                let Some(repo) = self.scm.repo.clone() else {
                    return;
                };
                let text = git::revert_hunk(&old, &new, &block);
                let file = repo.absolute(&path);
                self.write_file_text(&file, &text, window, cx);
                self.refresh_git(cx);
            }
            // Unstage: the staged version goes back to the last commit's.
            (super::diff_view::DiffSource::Staged, _) => {
                let staged = git::revert_hunk(&old, &new, &block);
                self.run_git(
                    strings.scm_unstage,
                    move |repo| repo.stage_text(&path, &staged),
                    |_, _, _| {},
                    window,
                    cx,
                );
            }
            _ => {}
        }
    }

    /// Writes a file: through its editor when it is open, so the tab
    /// follows and nothing typed is lost, else straight to disk.
    pub(super) fn write_file_text(
        &mut self,
        path: &Path,
        text: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let uri = Uri::file(path).to_string();
        if self.documents.contains_key(&uri) {
            for editor in self.editors_showing(&uri) {
                tabs::replace_text(&editor, text, window, cx);
            }
            if let Some(document) = self.documents.get_mut(&uri) {
                document.synced_text = text.to_owned();
            }
            // Asked for on purpose: written even over a change made
            // outside Keel meanwhile.
            self.save_file_now(&uri, None, window, cx);
        } else if let Err(error) = std::fs::write(path, text) {
            self.report(format!("{}: {error}", path.display()));
        }
        cx.notify();
    }
}

fn render_row(
    state: &Rc<RefCell<DocState>>,
    doc: &DiffDoc,
    index: usize,
    look: &Look,
    cx: &mut App,
) -> AnyElement {
    let Some(row) = state.borrow().rows.get(index).cloned() else {
        return div().into_any_element();
    };
    let colors = look.colors;
    match row {
        Row::Header { file } => render_header(state, doc, file, look, cx),
        Row::Unchanged { .. } => div()
            .px_4()
            .py_3()
            .text_sm()
            .text_color(colors.muted)
            .child(look.strings.diff_no_changes)
            .into_any_element(),
        Row::Fold {
            file,
            at,
            new,
            len,
            above,
            below,
            ..
        } => render_fold(state, doc, file, at, new, len, above, below, look, cx),
        row => {
            let file = row.file();
            let sides = state.borrow_mut().sides(file, cx);
            let state = state.borrow();
            render_code(
                &row,
                &state.files[file],
                &sides,
                index,
                state.split,
                look,
                cx,
            )
        }
    }
}

/// A file's name, counts and buttons, in the page with every change.
fn render_header(
    state: &Rc<RefCell<DocState>>,
    doc: &DiffDoc,
    file: usize,
    look: &Look,
    cx: &mut App,
) -> AnyElement {
    let colors = look.colors;
    let (path, added, removed, collapsed) = {
        let state = state.borrow();
        let diff = &state.files[file];
        (
            diff.path.clone(),
            diff.additions(),
            diff.deletions(),
            diff.collapsed,
        )
    };
    let toggle_doc = doc.clone();
    let workbench = look.workbench.clone();
    let open_path = path.clone();
    let theme = cx.theme();
    div()
        .id(("diff-file", file))
        .w_full()
        .mt(px(if file == 0 { 0. } else { 10. }))
        .h(px(34.))
        .px_3()
        .flex()
        .items_center()
        .gap_2()
        .bg(colors.hover.opacity(0.6))
        .border_y_1()
        .border_color(colors.border)
        .cursor_pointer()
        .on_click(move |_, _, cx| {
            toggle_doc.toggle_file(file);
            if let Some(workbench) = workbench.upgrade() {
                workbench.update(cx, |_, cx| cx.notify());
            }
        })
        .child(
            Icon::new(if collapsed {
                IconName::ChevronRight
            } else {
                IconName::ChevronDown
            })
            .xsmall()
            .text_color(colors.faint),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .text_sm()
                .font_weight(FontWeight::MEDIUM)
                .child(path),
        )
        .child(
            div()
                .flex_none()
                .flex()
                .gap_1p5()
                .text_xs()
                .child(div().text_color(theme.success).child(format!("+{added}")))
                .child(div().text_color(theme.danger).child(format!("−{removed}"))),
        )
        .child({
            let workbench = look.workbench.clone();
            Button::new(("diff-file-open", file))
                .ghost()
                .xsmall()
                .icon(Icon::new(IconName::File))
                .tooltip(look.strings.scm_open_file)
                .on_click(move |_, window, cx| {
                    cx.stop_propagation();
                    if let Some(workbench) = workbench.upgrade() {
                        let path = open_path.clone();
                        workbench.update(cx, |this, cx| this.open_repo_file(&path, 0, window, cx));
                    }
                })
        })
        .into_any_element()
}

/// Hidden unchanged lines, with arrows to open them and the code they
/// are in.
#[allow(clippy::too_many_arguments)]
fn render_fold(
    state: &Rc<RefCell<DocState>>,
    doc: &DiffDoc,
    file: usize,
    at: usize,
    new: usize,
    len: usize,
    above: bool,
    below: bool,
    look: &Look,
    cx: &mut App,
) -> AnyElement {
    let colors = look.colors;
    let strings = look.strings;
    let scope = state.borrow().files[file].scope_above(new + len);
    let button = |id: &'static str, icon: IconName, how: Expand, tooltip: String| {
        let doc = doc.clone();
        let workbench = look.workbench.clone();
        Button::new((id, at))
            .ghost()
            .xsmall()
            .icon(Icon::new(icon))
            .tooltip(tooltip)
            .on_click(move |_, _, cx| {
                doc.expand(file, at, how);
                if let Some(workbench) = workbench.upgrade() {
                    workbench.update(cx, |_, cx| cx.notify());
                }
            })
    };
    let step = diff_model::EXPAND_STEP.to_string();
    let partial = len > diff_model::EXPAND_STEP;
    let all_doc = doc.clone();
    let all_workbench = look.workbench.clone();
    let _ = cx;
    div()
        .id(("diff-fold", file * 1_000_000 + at))
        .w_full()
        .h(px(30.))
        .px_2()
        .flex()
        .items_center()
        .gap_1()
        .bg(colors.hover.opacity(0.45))
        .text_xs()
        .text_color(colors.muted)
        .when(partial && above, |row| {
            row.child(button(
                "diff-fold-down",
                IconName::ArrowDown,
                Expand::Down,
                notices::fill(strings.diff_show_below, &[("count", &step)]),
            ))
        })
        .when(partial && below, |row| {
            row.child(button(
                "diff-fold-up",
                IconName::ArrowUp,
                Expand::Up,
                notices::fill(strings.diff_show_above, &[("count", &step)]),
            ))
        })
        .child(
            div()
                .id(("diff-fold-all", file * 1_000_000 + at))
                .flex()
                .items_center()
                .gap_1()
                .px_1p5()
                .h(px(22.))
                .rounded(px(5.))
                .cursor_pointer()
                .hover(|item| item.bg(colors.hover).text_color(colors.text))
                .on_click(move |_, _, cx| {
                    all_doc.expand(file, at, Expand::All);
                    if let Some(workbench) = all_workbench.upgrade() {
                        workbench.update(cx, |_, cx| cx.notify());
                    }
                })
                .child(Icon::new(IconName::ChevronsUpDown).xsmall())
                .child(notices::fill(
                    strings.diff_hidden_lines,
                    &[("count", &len.to_string())],
                )),
        )
        .children(scope.map(|scope| {
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .pl_2()
                .font_family(look.font.clone())
                .text_color(colors.faint)
                .child(scope)
        }))
        .into_any_element()
}

/// Which side of a row a cell shows, and how.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Cell {
    Same,
    Removed,
    Added,
}

/// One cell: its line numbers (old, new), whether it has a column for
/// each, and its text on one side.
struct CellSpec<'a> {
    numbers: (Option<usize>, Option<usize>),
    two_numbers: bool,
    kind: Cell,
    text: &'a str,
    words: &'a [Range<usize>],
    side: &'a Side,
    line: usize,
    new_side: bool,
}

fn render_code(
    row: &Row,
    diff: &FileDiff,
    sides: &Rc<[Side; 2]>,
    index: usize,
    split: bool,
    look: &Look,
    cx: &mut App,
) -> AnyElement {
    let file = row.file();
    let old_cell = |line: usize, kind: Cell, words: &[Range<usize>], two: bool, cx: &App| {
        cell(
            CellSpec {
                numbers: (Some(line), None),
                two_numbers: two,
                kind,
                text: &diff.old[line],
                words,
                side: &sides[0],
                line,
                new_side: false,
            },
            look,
            cx,
        )
    };
    let new_cell = |line: usize, kind: Cell, words: &[Range<usize>], two: bool, cx: &App| {
        cell(
            CellSpec {
                numbers: (None, Some(line)),
                two_numbers: two,
                kind,
                text: &diff.new[line],
                words,
                side: &sides[1],
                line,
                new_side: true,
            },
            look,
            cx,
        )
    };
    let columns = |left: AnyElement, right: AnyElement| {
        div()
            .flex()
            .child(half(left))
            .child(divider(look))
            .child(half(right))
    };
    let element = match *row {
        Row::Same { old, new, .. } if split => columns(
            old_cell(old, Cell::Same, &[], false, cx),
            new_cell(new, Cell::Same, &[], false, cx),
        ),
        Row::Same { old, new, .. } => div().child(cell(
            CellSpec {
                numbers: (Some(old), Some(new)),
                two_numbers: true,
                kind: Cell::Same,
                text: &diff.new[new],
                words: &[],
                side: &sides[1],
                line: new,
                new_side: true,
            },
            look,
            cx,
        )),
        Row::Pair { old, new, .. } => {
            let (taken, written) = match (old, new) {
                (Some(old), Some(new)) => diff_model::changed_words(&diff.old[old], &diff.new[new]),
                _ => (Vec::new(), Vec::new()),
            };
            let left = match old {
                Some(old) => old_cell(old, Cell::Removed, &taken, false, cx),
                None => filler(look),
            };
            let right = match new {
                Some(new) => new_cell(new, Cell::Added, &written, false, cx),
                None => filler(look),
            };
            columns(left, right)
        }
        Row::Removed { old, .. } => div().child(old_cell(old, Cell::Removed, &[], true, cx)),
        Row::Added { new, .. } => div().child(new_cell(new, Cell::Added, &[], true, cx)),
        _ => div(),
    };
    let buttons = (row.starts_hunk() && look.actions != BlockActions::None)
        .then(|| {
            row.hunk()
                .map(|hunk| block_buttons(file, hunk, index, look))
        })
        .flatten();
    div()
        .id(("diff-row", index))
        .w_full()
        .relative()
        .child(element)
        .children(buttons)
        .into_any_element()
}

fn half(child: AnyElement) -> gpui::Div {
    div()
        .flex_1()
        .min_w_0()
        .flex_basis(relative(0.5))
        .child(child)
}

fn divider(look: &Look) -> gpui::Div {
    div().flex_none().w(px(1.)).bg(look.colors.border)
}

/// The other side's lines have nothing facing them here.
fn filler(look: &Look) -> AnyElement {
    div()
        .size_full()
        .min_h(look.line_height)
        .bg(look.colors.hover.opacity(0.35))
        .into_any_element()
}

/// Stage and Undo for a block, or Unstage, at the right of its first row.
fn block_buttons(file: usize, hunk: usize, index: usize, look: &Look) -> AnyElement {
    let strings = look.strings;
    let action = |id: &'static str, icon: IconName, tooltip: &'static str, undo: bool| {
        let workbench = look.workbench.clone();
        let uri = look.uri.clone();
        Button::new((id, index))
            .ghost()
            .xsmall()
            .icon(Icon::new(icon))
            .tooltip(tooltip)
            .on_click(move |_, window, cx| {
                cx.stop_propagation();
                if let Some(workbench) = workbench.upgrade() {
                    workbench.update(cx, |this, cx| {
                        this.diff_block_action(&uri, file, hunk, undo, window, cx)
                    });
                }
            })
    };
    let buttons = match look.actions {
        BlockActions::StageOrRevert => vec![
            action(
                "diff-block-stage",
                IconName::Plus,
                strings.diff_stage_block,
                false,
            ),
            action(
                "diff-block-undo",
                IconName::Undo2,
                strings.diff_undo_block,
                true,
            ),
        ],
        BlockActions::Unstage => vec![action(
            "diff-block-unstage",
            IconName::Minus,
            strings.diff_unstage_block,
            false,
        )],
        BlockActions::None => Vec::new(),
    };
    div()
        .absolute()
        .top(px(-2.))
        .right(px(10.))
        .flex()
        .gap_0p5()
        .p_0p5()
        .rounded(px(6.))
        .bg(look.colors.panel)
        .border_1()
        .border_color(look.colors.border)
        .shadow_sm()
        .opacity(0.85)
        .children(buttons)
        .into_any_element()
}

/// One side of a line: its numbers, its mark and its code with colours,
/// the changed words standing out.
fn cell(spec: CellSpec, look: &Look, cx: &App) -> AnyElement {
    let CellSpec {
        numbers: (old_number, new_number),
        two_numbers,
        kind,
        text,
        words,
        side,
        line,
        new_side,
    } = spec;
    let colors = look.colors;
    let theme = cx.theme();
    let (background, word_background, mark, mark_color) = match kind {
        Cell::Added => (
            Some(theme.success.opacity(0.12)),
            theme.success.opacity(0.32),
            "+",
            theme.success,
        ),
        Cell::Removed => (
            Some(theme.danger.opacity(0.12)),
            theme.danger.opacity(0.32),
            "−",
            theme.danger,
        ),
        Cell::Same => (None, gpui::transparent_black(), " ", colors.faint),
    };
    let mut styles: Vec<(Range<usize>, HighlightStyle)> = side
        .styles(line, cx)
        .iter()
        .filter(|(range, _)| range.end <= text.len())
        .cloned()
        .collect();
    if !words.is_empty() {
        let emphasis = words.iter().map(|range| {
            (
                range.clone(),
                HighlightStyle {
                    background_color: Some(word_background),
                    ..Default::default()
                },
            )
        });
        styles = gpui::combine_highlights(styles, emphasis).collect();
    }
    let number = |value: Option<usize>| {
        div()
            .flex_none()
            .w(look.number_width)
            .pr_2()
            .text_right()
            .text_color(colors.faint)
            .child(
                value
                    .map(|value| (value + 1).to_string())
                    .unwrap_or_default(),
            )
    };
    let open = {
        let workbench = look.workbench.clone();
        let uri = look.uri.clone();
        move |line: usize| {
            let workbench = workbench.clone();
            let uri = uri.clone();
            move |event: &gpui::ClickEvent, window: &mut Window, cx: &mut App| {
                if event.click_count() < 2 {
                    return;
                }
                if let Some(workbench) = workbench.upgrade() {
                    workbench.update(cx, |this, cx| this.open_diff_line(&uri, line, window, cx));
                }
            }
        }
    };
    let code = StyledText::new(SharedString::from(text.to_owned())).with_highlights(styles);
    div()
        .id(SharedString::from(format!(
            "diff-cell-{}-{line}",
            u8::from(new_side)
        )))
        .w_full()
        .min_h(look.line_height)
        .flex()
        .font_family(look.font.clone())
        .text_size(look.font_size)
        .line_height(look.line_height)
        .when_some(background, |cell, background| cell.bg(background))
        .when(two_numbers, |cell| {
            cell.child(number(old_number)).child(number(new_number))
        })
        .when(!two_numbers, |cell| {
            cell.child(number(old_number.or(new_number)))
        })
        .child(
            div()
                .flex_none()
                .w(px(14.))
                .text_color(mark_color)
                .child(mark),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .pr_3()
                .whitespace_normal()
                .text_color(colors.text)
                .child(code),
        )
        .when(new_side, |cell| cell.on_click(open(line)))
        .into_any_element()
}
