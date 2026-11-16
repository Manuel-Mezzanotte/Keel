//! The Search view of the sidebar (Cmd+Shift+F), as in VS Code: the query
//! with its switches for case, whole words and regular expressions, the
//! folders to include and exclude, and the results grouped by file.

use std::collections::HashSet;
use std::ops::Range;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use files::search::{FileMatches, Query, Results};
use gpui::{
    AnyElement, FontWeight, HighlightStyle, StatefulInteractiveElement as _, StyledText,
    uniform_list,
};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::input::{Input, InputState};
use gpui_component::{Icon, Selectable as _, Sizable as _};
use gpui_kit_assets::IconName;

use super::shell::{Colors, heading, note};
use super::tabs::OpenAs;
use super::*;

const ROW_HEIGHT: f32 = 22.;
/// How long typing must pause before the search runs.
const TYPING_PAUSE: Duration = Duration::from_millis(200);

/// A line of the results list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ResultRow {
    File(usize),
    Line(usize, usize),
}

pub(super) struct SearchView {
    query: Entity<InputState>,
    include: Entity<InputState>,
    exclude: Entity<InputState>,
    case_sensitive: bool,
    whole_word: bool,
    regex: bool,
    show_folders: bool,
    results: Results,
    rows: Vec<ResultRow>,
    error: Option<String>,
    searching: bool,
    /// Files whose lines are hidden, by relative path.
    collapsed: HashSet<String>,
    stop: Arc<AtomicBool>,
    task: Task<()>,
    _subscriptions: Vec<Subscription>,
}

impl SearchView {
    fn query(&self, cx: &App) -> Query {
        Query {
            text: self.query.read(cx).value().to_string(),
            case_sensitive: self.case_sensitive,
            whole_word: self.whole_word,
            regex: self.regex,
            include: self.include.read(cx).value().to_string(),
            exclude: self.exclude.read(cx).value().to_string(),
        }
    }

    fn rebuild_rows(&mut self) {
        self.rows.clear();
        for (file_index, file) in self.results.files.iter().enumerate() {
            self.rows.push(ResultRow::File(file_index));
            if !self.collapsed.contains(&file.path) {
                self.rows
                    .extend((0..file.lines.len()).map(|line| ResultRow::Line(file_index, line)));
            }
        }
    }
}

/// `7.310` in Italian, `7,310` in English.
pub(super) fn grouped(number: usize, language: UiLanguage) -> String {
    let digits = number.to_string();
    let separator = if language == UiLanguage::Italian {
        '.'
    } else {
        ','
    };
    let mut text = String::new();
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            text.push(separator);
        }
        text.push(digit);
    }
    text
}

impl Workbench {
    pub(super) fn create_search_view(
        window: &mut Window,
        cx: &mut Context<Self>,
        strings: &Strings,
    ) -> SearchView {
        let query =
            cx.new(|cx| InputState::new(window, cx).placeholder(strings.search_placeholder));
        let include =
            cx.new(|cx| InputState::new(window, cx).placeholder(strings.include_placeholder));
        let exclude =
            cx.new(|cx| InputState::new(window, cx).placeholder(strings.exclude_placeholder));
        let subscriptions = [&query, &include, &exclude]
            .into_iter()
            .map(|input| {
                cx.subscribe_in(
                    input,
                    window,
                    |this, _, event: &InputEvent, _, cx| match event {
                        InputEvent::Change => this.search_soon(cx),
                        InputEvent::PressEnter { .. } => this.run_search(cx),
                        _ => {}
                    },
                )
            })
            .collect();
        SearchView {
            query,
            include,
            exclude,
            case_sensitive: false,
            whole_word: false,
            regex: false,
            show_folders: false,
            results: Results::default(),
            rows: Vec::new(),
            error: None,
            searching: false,
            collapsed: HashSet::new(),
            stop: Arc::default(),
            task: Task::ready(()),
            _subscriptions: subscriptions,
        }
    }

    /// Cmd+Shift+F: the Search view, with the selected text as the query.
    pub(super) fn focus_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.layout.show_side_view(crate::layout::SideView::Search);
        let selected = self.active_editor().and_then(|editor| {
            let editor = editor.read(cx);
            let range = editor.selected_range();
            let text = editor.value().to_string();
            let selected = text.get(range)?.to_owned();
            (!selected.is_empty() && !selected.contains('\n')).then_some(selected)
        });
        let query = self.search.query.clone();
        query.update(cx, |query, cx| {
            if let Some(selected) = selected {
                query.set_value(selected, window, cx);
            }
            query.focus(window, cx);
            query.select_all(window, cx);
        });
        self.run_search(cx);
        cx.notify();
    }

    /// Runs the search after a short pause in typing.
    fn search_soon(&mut self, cx: &mut Context<Self>) {
        self.search.task = cx.spawn(async move |this, cx| {
            cx.background_executor().timer(TYPING_PAUSE).await;
            this.update(cx, |this, cx| this.run_search(cx)).ok();
        });
    }

    /// Searches the project in the background; a newer search stops the
    /// one still running.
    pub(super) fn run_search(&mut self, cx: &mut Context<Self>) {
        let Some(root) = self.project.root.clone() else {
            return;
        };
        self.search.stop.store(true, Ordering::Relaxed);
        let stop = Arc::new(AtomicBool::new(false));
        self.search.stop = stop.clone();
        let query = self.search.query(cx);
        if query.text.is_empty() {
            self.search.results = Results::default();
            self.search.error = None;
            self.search.searching = false;
            self.search.rebuild_rows();
            cx.notify();
            return;
        }
        let excludes = super::explorer::excludes(&self.settings);
        self.search.searching = true;
        let found = cx
            .background_executor()
            .spawn(async move { files::search::search(&root, &query, &excludes, &stop) });
        self.search.task = cx.spawn(async move |this, cx| {
            let found = found.await;
            this.update(cx, |this, cx| {
                this.search.searching = false;
                match found {
                    Ok(results) => {
                        this.search.results = results;
                        this.search.error = None;
                    }
                    Err(error) => {
                        this.search.results = Results::default();
                        this.search.error = Some(error);
                    }
                }
                this.search.rebuild_rows();
                cx.notify();
            })
            .ok();
        });
        cx.notify();
    }

    fn toggle_search_option(
        &mut self,
        option: fn(&mut SearchView) -> &mut bool,
        cx: &mut Context<Self>,
    ) {
        let value = option(&mut self.search);
        *value = !*value;
        self.run_search(cx);
    }

    /// Opens a result's file and selects the match.
    fn open_result(
        &mut self,
        file: usize,
        line: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(root) = self.project.root.clone() else {
            return;
        };
        let Some(found) = self.search.results.files.get(file) else {
            return;
        };
        let Some(matched) = found.lines.get(line).cloned() else {
            return;
        };
        let path = root.join(&found.path);
        self.open_file_as(&path, OpenAs::Preview, window, cx);
        let length = matched
            .ranges
            .first()
            .map(|range| matched.text[range.clone()].chars().count())
            .unwrap_or(0);
        self.select_in_active(matched.line, matched.column, length, window, cx);
    }

    /// Selects `length` characters from a line and column, counted from 0,
    /// in the active editor.
    pub(super) fn select_in_active(
        &mut self,
        line: usize,
        column: usize,
        length: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(editor) = self.active_editor() else {
            return;
        };
        editor.update(cx, |editor, cx| {
            let text = editor.value().to_string();
            let line_start = text
                .split_inclusive('\n')
                .take(line)
                .map(str::len)
                .sum::<usize>()
                .min(text.len());
            let line_text = text[line_start..].lines().next().unwrap_or_default();
            let byte = |characters: usize| {
                line_text
                    .char_indices()
                    .nth(characters)
                    .map_or(line_text.len(), |(at, _)| at)
            };
            let start = line_start + byte(column);
            let end = line_start + byte(column + length);
            editor.set_cursor_position(
                gpui_component::input::Position::new(line as u32, column as u32),
                window,
                cx,
            );
            editor.set_selected_range(start..end, cx);
        });
        self.publish_selection_if_changed(self.active, cx);
    }

    // -----------------------------------------------------------------
    // Drawing

    pub(super) fn render_search(&self, colors: Colors, cx: &mut Context<Self>) -> AnyElement {
        let strings = self.strings();
        if self.project.root.is_none() {
            return div()
                .child(heading(strings.view_search, colors))
                .child(note(strings.no_folder.to_owned(), colors))
                .into_any_element();
        }
        let view = &self.search;
        let switch = |id: &'static str, icon: IconName, on: bool, tooltip: &'static str| {
            Button::new(id)
                .ghost()
                .xsmall()
                .icon(Icon::new(icon))
                .selected(on)
                .tooltip(tooltip)
        };
        let switches = div()
            .flex()
            .items_center()
            .child(
                switch(
                    "search-case",
                    IconName::CaseSensitive,
                    view.case_sensitive,
                    strings.match_case,
                )
                .on_click(cx.listener(|this, _, _, cx| {
                    this.toggle_search_option(|view| &mut view.case_sensitive, cx)
                })),
            )
            .child(
                switch(
                    "search-word",
                    IconName::WholeWord,
                    view.whole_word,
                    strings.whole_word,
                )
                .on_click(cx.listener(|this, _, _, cx| {
                    this.toggle_search_option(|view| &mut view.whole_word, cx)
                })),
            )
            .child(
                switch(
                    "search-regex",
                    IconName::Regex,
                    view.regex,
                    strings.use_regex,
                )
                .on_click(cx.listener(|this, _, _, cx| {
                    this.toggle_search_option(|view| &mut view.regex, cx)
                })),
            );
        let summary = match (&view.error, view.searching, view.results.files.is_empty()) {
            (Some(error), _, _) => error.lines().last().unwrap_or_default().to_owned(),
            (None, true, true) => strings.searching.to_owned(),
            (None, _, true) if !view.query.read(cx).value().is_empty() => {
                strings.no_results.to_owned()
            }
            (None, _, true) => String::new(),
            (None, _, false) => {
                let count = grouped(view.results.count(), self.language);
                let files = grouped(view.results.files.len(), self.language);
                let mut text = strings.results_summary(
                    &count,
                    &files,
                    view.results.count() == 1,
                    view.results.files.len() == 1,
                );
                if view.results.limited {
                    text.push_str(strings.results_limited);
                }
                text
            }
        };
        let list = uniform_list(
            "search-results",
            view.rows.len(),
            cx.processor(|this, range: Range<usize>, _, cx| this.render_result_rows(range, cx)),
        )
        .size_full();

        div()
            .size_full()
            .flex()
            .flex_col()
            .gap_1p5()
            .child(heading(strings.view_search, colors))
            .child(Input::new(&view.query).small().suffix(switches))
            .child(
                div().flex().justify_end().child(
                    Button::new("search-folders")
                        .ghost()
                        .xsmall()
                        .icon(Icon::new(IconName::Ellipsis))
                        .selected(view.show_folders)
                        .tooltip(strings.folders_toggle)
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.search.show_folders = !this.search.show_folders;
                            cx.notify();
                        })),
                ),
            )
            .when(view.show_folders, |panel| {
                panel
                    .child(Input::new(&view.include).small())
                    .child(Input::new(&view.exclude).small())
            })
            .child(
                div()
                    .px_1()
                    .text_xs()
                    .text_color(if view.error.is_some() {
                        cx.theme().danger
                    } else {
                        colors.muted
                    })
                    .child(summary),
            )
            .child(div().flex_1().min_h_0().child(list))
            .into_any_element()
    }

    fn render_result_rows(
        &mut self,
        range: Range<usize>,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let colors = Colors::new(cx);
        range
            .filter_map(|index| {
                let row = *self.search.rows.get(index)?;
                Some(match row {
                    ResultRow::File(file) => self.render_result_file(index, file, colors, cx),
                    ResultRow::Line(file, line) => {
                        self.render_result_line(index, file, line, colors, cx)
                    }
                })
            })
            .collect()
    }

    fn render_result_file(
        &self,
        index: usize,
        file: usize,
        colors: Colors,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let found: &FileMatches = &self.search.results.files[file];
        let (folder, name) = match found.path.rsplit_once('/') {
            Some((folder, name)) => (folder.to_owned(), name.to_owned()),
            None => (String::new(), found.path.clone()),
        };
        let collapsed = self.search.collapsed.contains(&found.path);
        let path = found.path.clone();
        let count = grouped(found.count(), self.language);
        div()
            .id(("search-file", index))
            .h(px(ROW_HEIGHT))
            .px_1()
            .flex()
            .items_center()
            .gap_1()
            .rounded(px(6.))
            .text_sm()
            .cursor_pointer()
            .hover(|row| row.bg(colors.hover))
            .on_click(cx.listener(move |this, _, _, cx| {
                if !this.search.collapsed.remove(&path) {
                    this.search.collapsed.insert(path.clone());
                }
                this.search.rebuild_rows();
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
                    .flex_1()
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
                    .child(count),
            )
            .into_any_element()
    }

    fn render_result_line(
        &self,
        index: usize,
        file: usize,
        line: usize,
        colors: Colors,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let matched = &self.search.results.files[file].lines[line];
        // Leading spaces only push the match out of view.
        let trimmed = matched.text.trim_start();
        let cut = matched.text.len() - trimmed.len();
        let highlight = HighlightStyle {
            background_color: Some(colors.accent.opacity(0.28)),
            font_weight: Some(FontWeight::SEMIBOLD),
            ..Default::default()
        };
        let highlights: Vec<(Range<usize>, HighlightStyle)> = matched
            .ranges
            .iter()
            .filter(|range| range.start >= cut)
            .map(|range| (range.start - cut..range.end - cut, highlight))
            .collect();
        let text = StyledText::new(trimmed.to_owned()).with_highlights(highlights);
        div()
            .id(("search-line", index))
            .h(px(ROW_HEIGHT))
            .pl(px(24.))
            .pr_1()
            .flex()
            .items_center()
            .rounded(px(6.))
            .text_sm()
            .text_color(colors.muted)
            .cursor_pointer()
            .hover(|row| row.bg(colors.hover))
            .on_click(
                cx.listener(move |this, _, window, cx| this.open_result(file, line, window, cx)),
            )
            .child(div().min_w_0().truncate().whitespace_nowrap().child(text))
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;

    use gpui::{TestAppContext, VisualTestContext};

    use super::*;

    #[test]
    fn numbers_group_their_thousands() {
        assert_eq!(grouped(7310, UiLanguage::Italian), "7.310");
        assert_eq!(grouped(1_234_567, UiLanguage::English), "1,234,567");
        assert_eq!(grouped(42, UiLanguage::Italian), "42");
    }

    fn project(
        cx: &mut TestAppContext,
    ) -> (
        tempfile::TempDir,
        PathBuf,
        Entity<Workbench>,
        &mut VisualTestContext,
    ) {
        cx.update(|cx| {
            gpui_component::init(cx);
            language::register_grammars();
            crate::workbench_window::init(cx);
        });
        let folder = tempfile::tempdir().unwrap();
        let base = fs::canonicalize(folder.path()).unwrap();
        let root = base.join("Hospitality");
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(
            root.join("src/card.tsx"),
            "const total = 1;\nconst x = total + 1;\n",
        )
        .unwrap();
        fs::write(
            root.join("src/app.ts"),
            "// no match here\nlet Total = 2;\n",
        )
        .unwrap();
        let settings_file = base.join("settings.json");
        fs::write(&settings_file, r#"{ "keel.language": "en" }"#).unwrap();
        let settings = Settings::load(&settings_file, None);
        let opening = Opening::new(Some(root.clone()), Some(root.join("src/app.ts")));
        let (view, cx) =
            cx.add_window_view(|window, cx| Workbench::new(opening, settings, None, window, cx));
        (folder, root, view, cx)
    }

    #[gpui::test]
    fn cmd_shift_f_searches_and_a_click_selects_the_match(cx: &mut TestAppContext) {
        let (_folder, _root, view, cx) = project(cx);
        // The search walks the project on real threads and waits for them.
        cx.executor().allow_parking();
        cx.simulate_keystrokes("cmd-shift-f");
        cx.simulate_input("total");
        // The search runs after a short pause in typing.
        cx.executor().advance_clock(TYPING_PAUSE * 2);
        cx.run_until_parked();
        let (files, count) = view.read_with(cx, |view, _| {
            (view.search.results.files.len(), view.search.results.count())
        });
        assert_eq!((files, count), (2, 3));

        view.update(cx, |view, cx| {
            view.toggle_search_option(|view| &mut view.case_sensitive, cx)
        });
        cx.executor().advance_clock(TYPING_PAUSE * 2);
        cx.run_until_parked();
        let paths: Vec<String> = view.read_with(cx, |view, _| {
            view.search
                .results
                .files
                .iter()
                .map(|file| file.path.clone())
                .collect()
        });
        assert_eq!(paths, ["src/card.tsx"]);

        view.update_in(cx, |view, window, cx| view.open_result(0, 1, window, cx));
        let (title, selected) = view.read_with(cx, |view, cx| {
            let editor = view.panes[view.active].editor.read(cx);
            let text = editor.value().to_string();
            (
                view.window_title(),
                text[editor.selected_range()].to_owned(),
            )
        });
        assert_eq!(title, "card.tsx — Hospitality");
        assert_eq!(selected, "total");
    }
}
