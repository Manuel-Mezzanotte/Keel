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
use gpui_component::{Disableable as _, Icon, Selectable as _, Sizable as _};
use gpui_kit_assets::IconName;

use super::shell::{Colors, empty_state, heading, keys_for, note, tool_row, view_header};
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
    replace: Entity<InputState>,
    show_replace: bool,
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
    /// The latest searches, newest first, to run again with a click.
    pub(super) recent: Vec<String>,
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

/// How many searches the view remembers.
const RECENT_SEARCHES: usize = 8;

/// Puts `text` first among the recent searches. A search that grew from
/// one before it, as typing makes `boo` then `booking`, takes its place.
/// Returns whether the list changed.
pub(super) fn remember_search(recent: &mut Vec<String>, text: &str) -> bool {
    let text = text.trim();
    if text.is_empty() || recent.first().is_some_and(|first| first == text) {
        return false;
    }
    recent.retain(|known| known != text && !text.starts_with(known.as_str()));
    recent.insert(0, text.to_owned());
    recent.truncate(RECENT_SEARCHES);
    true
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
        let replace =
            cx.new(|cx| InputState::new(window, cx).placeholder(strings.replace_placeholder));
        let include =
            cx.new(|cx| InputState::new(window, cx).placeholder(strings.include_placeholder));
        let exclude =
            cx.new(|cx| InputState::new(window, cx).placeholder(strings.exclude_placeholder));
        let mut subscriptions: Vec<Subscription> = [&query, &include, &exclude]
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
        // The preview follows the replacement as it is typed.
        subscriptions.push(
            cx.subscribe_in(&replace, window, |_, _, event: &InputEvent, _, cx| {
                if matches!(event, InputEvent::Change) {
                    cx.notify();
                }
            }),
        );
        SearchView {
            query,
            replace,
            show_replace: false,
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
            recent: Vec::new(),
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
                        let text = this.search.query.read(cx).value().to_string();
                        if remember_search(&mut this.search.recent, &text) {
                            this.session_changed(cx);
                        }
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

    /// The replacement typed, when the replace field is shown.
    fn replacement(&self, cx: &App) -> Option<String> {
        self.search
            .show_replace
            .then(|| self.search.replace.read(cx).value().to_string())
    }

    /// Replaces the matches of every file, or of one file, after asking
    /// when it is every file. Open files change in their editor, where
    /// Cmd+Z undoes it and the save stays yours; the others are written.
    fn replace_matches(
        &mut self,
        only: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(replacement) = self.replacement(cx) else {
            return;
        };
        let files: Vec<String> = self
            .search
            .results
            .files
            .iter()
            .filter(|file| only.as_ref().is_none_or(|only| *only == file.path))
            .map(|file| file.path.clone())
            .collect();
        if files.is_empty() {
            return;
        }
        if only.is_some() {
            self.apply_replacement(&files, &replacement, window, cx);
            return;
        }
        let strings = self.strings();
        let count = grouped(self.search.results.count(), self.language);
        let file_count = grouped(files.len(), self.language);
        let answer = window.prompt(
            PromptLevel::Warning,
            &strings.replace_question(&count, &file_count, &replacement),
            Some(strings.replace_detail),
            &[strings.replace_all, strings.cancel],
            cx,
        );
        cx.spawn_in(window, async move |this, cx| {
            if answer.await != Ok(0) {
                return;
            }
            this.update_in(cx, |this, window, cx| {
                this.apply_replacement(&files, &replacement, window, cx)
            })
            .ok();
        })
        .detach();
    }

    fn apply_replacement(
        &mut self,
        files: &[String],
        replacement: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(root) = self.project.root.clone() else {
            return;
        };
        let query = self.search.query(cx);
        let (mut replaced, mut changed) = (0, 0);
        for relative in files {
            let path = root.join(relative);
            let uri = Uri::file(&path).to_string();
            let open_text = self
                .documents
                .get(&uri)
                .filter(|document| matches!(document.backing, Backing::File(_)))
                .map(|document| document.synced_text.clone());
            let result = match open_text {
                Some(text) => {
                    files::search::replace_all(&text, &query, replacement).map(|(text, count)| {
                        if count > 0 {
                            self.set_document_text(&uri, &text, window, cx);
                        }
                        count
                    })
                }
                None => files::search::replace_in_file(&path, &query, replacement),
            };
            match result {
                Ok(0) => {}
                Ok(count) => {
                    replaced += count;
                    changed += 1;
                }
                Err(error) => {
                    let message = self.strings().could_not_change(relative, &error);
                    self.report(message);
                }
            }
        }
        let strings = self.strings();
        self.status_message = Some(strings.replaced_summary(
            &grouped(replaced, self.language),
            &grouped(changed, self.language),
        ));
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

    pub(super) fn render_search(
        &self,
        colors: Colors,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let strings = self.strings();
        if self.project.root.is_none() {
            return div()
                .child(view_header(strings.view_search, colors))
                .child(note(strings.no_folder.to_owned(), colors))
                .into_any_element();
        }
        let view = &self.search;
        let switch = |id: &'static str, icon: IconName, on: bool, tooltip: &'static str| {
            Button::new(id)
                .ghost()
                .small()
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
        let typed = !view.query.read(cx).value().is_empty();
        let summary = match (&view.error, view.searching, view.results.files.is_empty()) {
            (Some(error), _, _) => error.lines().last().unwrap_or_default().to_owned(),
            (None, true, true) => strings.searching.to_owned(),
            (None, _, true) if typed => strings.no_results.to_owned(),
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

        let header = view_header(strings.view_search, colors)
            .child(
                Button::new("search-again")
                    .ghost()
                    .small()
                    .icon(Icon::new(IconName::RefreshCw))
                    .tooltip(strings.search_again)
                    .disabled(!typed)
                    .on_click(cx.listener(|this, _, _, cx| this.run_search(cx))),
            )
            .child(
                Button::new("search-clear")
                    .ghost()
                    .small()
                    .icon(Icon::new(IconName::Eraser))
                    .tooltip(strings.search_clear)
                    .disabled(!typed)
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.search
                            .query
                            .update(cx, |input, cx| input.set_value("", window, cx));
                        this.run_search(cx);
                    })),
            )
            .child(
                Button::new("search-folders")
                    .ghost()
                    .small()
                    .icon(Icon::new(IconName::Funnel))
                    .selected(view.show_folders)
                    .tooltip(strings.folders_toggle)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.search.show_folders = !this.search.show_folders;
                        cx.notify();
                    })),
            );

        div()
            .size_full()
            .flex()
            .flex_col()
            .gap_1p5()
            .child(header)
            .child(
                div()
                    .flex()
                    .items_start()
                    .gap_0p5()
                    .child(
                        Button::new("search-toggle-replace")
                            .ghost()
                            .small()
                            .mt(px(4.))
                            .icon(Icon::new(if view.show_replace {
                                IconName::ChevronDown
                            } else {
                                IconName::ChevronRight
                            }))
                            .tooltip(strings.toggle_replace)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.search.show_replace = !this.search.show_replace;
                                cx.notify();
                            })),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .child(Input::new(&view.query).suffix(switches))
                            .when(view.show_replace, |fields| {
                                fields.child(
                                    Input::new(&view.replace).suffix(
                                        Button::new("search-replace-all")
                                            .ghost()
                                            .small()
                                            .icon(Icon::new(IconName::ReplaceAll))
                                            .tooltip(strings.replace_all)
                                            .on_click(cx.listener(|this, _, window, cx| {
                                                this.replace_matches(None, window, cx)
                                            })),
                                    ),
                                )
                            })
                            .when(view.show_folders, |fields| {
                                fields
                                    .child(Input::new(&view.include).small())
                                    .child(Input::new(&view.exclude).small())
                            }),
                    ),
            )
            .when(!summary.is_empty(), |panel| {
                panel.child(
                    div()
                        .px_1()
                        .pt_0p5()
                        .text_xs()
                        .text_color(if view.error.is_some() {
                            cx.theme().danger
                        } else {
                            colors.muted
                        })
                        .child(summary),
                )
            })
            .when(!typed, |panel| {
                panel.child(self.render_search_start(colors, window, cx))
            })
            .when(typed, |panel| {
                panel.child(div().flex_1().min_h_0().child(list))
            })
            .into_any_element()
    }

    /// Before the first search: what to type, the latest searches to run
    /// again, and the other ways of finding things, with their keys.
    fn render_search_start(
        &self,
        colors: Colors,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::Stateful<gpui::Div> {
        let strings = self.strings();
        let recent = self.search.recent.iter().enumerate().map(|(index, text)| {
            let query = text.clone();
            div()
                .id(("recent-search", index))
                .h(px(28.))
                .px_2()
                .flex()
                .items_center()
                .gap_2()
                .rounded(px(7.))
                .text_sm()
                .cursor_pointer()
                .hover(|row| row.bg(colors.hover))
                .active(|row| row.bg(colors.selected))
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.search
                        .query
                        .update(cx, |input, cx| input.set_value(query.clone(), window, cx));
                    this.run_search(cx);
                }))
                .child(Icon::new(IconName::Clock).small().text_color(colors.faint))
                .child(
                    div()
                        .min_w_0()
                        .truncate()
                        .text_color(colors.text)
                        .child(text.clone()),
                )
        });
        let tool = |id: &'static str,
                    icon: IconName,
                    label: &'static str,
                    action: Box<dyn gpui::Action>| {
            let keys = keys_for(action.as_ref(), window);
            tool_row(id, icon, label, keys, colors)
                .on_click(move |_, window, cx| window.dispatch_action(action.boxed_clone(), cx))
        };
        let tools = [
            tool(
                "search-go-file",
                IconName::File,
                strings.search_go_file,
                Box::new(QuickOpen),
            ),
            tool(
                "search-go-symbol-project",
                IconName::Hash,
                strings.search_go_symbol_project,
                Box::new(GoToSymbolInWorkspace),
            ),
            tool(
                "search-go-symbol-file",
                IconName::AtSign,
                strings.search_go_symbol_file,
                Box::new(GoToSymbolInFile),
            ),
            tool(
                "search-go-line",
                IconName::CornerDownRight,
                strings.search_go_line,
                Box::new(GoToLine),
            ),
        ];
        div()
            .id("search-start")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .gap_1()
            .child(empty_state(
                IconName::TextSearch,
                strings.search_start_title,
                strings.search_hint,
                colors,
            ))
            .when(!self.search.recent.is_empty(), |start| {
                start.child(
                    div()
                        .flex()
                        .flex_col()
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .child(div().flex_1().child(heading(strings.search_recent, colors)))
                                .child(
                                    Button::new("clear-recent-searches")
                                        .ghost()
                                        .small()
                                        .label(strings.search_recent_clear)
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.search.recent.clear();
                                            this.session_changed(cx);
                                            cx.notify();
                                        })),
                                ),
                        )
                        .children(recent),
                )
            })
            .child(
                div()
                    .pt_2()
                    .flex()
                    .flex_col()
                    .child(heading(strings.search_go_to, colors))
                    .children(tools),
            )
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
            .w_full()
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
            .child(self.file_icon_element(Path::new(&found.path), cx))
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
            .when(self.search.show_replace, |row| {
                let path = found.path.clone();
                row.child(
                    Button::new(("search-replace-file", index))
                        .ghost()
                        .xsmall()
                        .icon(Icon::new(IconName::Replace))
                        .tooltip(self.strings().replace_in_file)
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.replace_matches(Some(path.clone()), window, cx)
                        })),
                )
            })
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
        let ranges: Vec<Range<usize>> = matched
            .ranges
            .iter()
            .filter(|range| range.start >= cut)
            .map(|range| range.start - cut..range.end - cut)
            .collect();
        let text = match self.replacement(cx) {
            Some(replacement) => replacement_preview(
                trimmed,
                &ranges,
                &self.search.query(cx),
                &replacement,
                cx.theme().danger,
                cx.theme().success,
            ),
            None => StyledText::new(trimmed.to_owned())
                .with_highlights(ranges.into_iter().map(|range| (range, highlight))),
        };
        div()
            .id(("search-line", index))
            .h(px(ROW_HEIGHT))
            .w_full()
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

/// A line as it reads before and after the replacement: each match struck
/// through in red, followed by its replacement in green.
fn replacement_preview(
    line: &str,
    ranges: &[Range<usize>],
    query: &Query,
    replacement: &str,
    removed: Hsla,
    added: Hsla,
) -> StyledText {
    let mut text = String::new();
    let mut highlights = Vec::new();
    let mut from = 0;
    for range in ranges {
        text.push_str(&line[from..range.start]);
        let old = &line[range.clone()];
        let start = text.len();
        text.push_str(old);
        highlights.push((
            start..text.len(),
            HighlightStyle {
                background_color: Some(removed.opacity(0.22)),
                strikethrough: Some(gpui::StrikethroughStyle {
                    thickness: px(1.),
                    color: Some(removed),
                }),
                ..Default::default()
            },
        ));
        let new = files::search::preview_replacement(old, query, replacement);
        let start = text.len();
        text.push_str(&new);
        highlights.push((
            start..text.len(),
            HighlightStyle {
                background_color: Some(added.opacity(0.22)),
                ..Default::default()
            },
        ));
        from = range.end;
    }
    text.push_str(&line[from..]);
    StyledText::new(text).with_highlights(highlights)
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;

    use gpui::{TestAppContext, VisualTestContext};

    use super::*;

    #[test]
    fn recent_searches_keep_the_finished_words() {
        let mut recent = Vec::new();
        for typed in ["b", "boo", "booking", "useState", "booking"] {
            remember_search(&mut recent, typed);
        }
        assert_eq!(recent, ["booking", "useState"]);
        for index in 0..20 {
            remember_search(&mut recent, &format!("x{index}y"));
        }
        assert_eq!(recent.len(), RECENT_SEARCHES);
        assert!(!remember_search(&mut recent, "  "));
    }

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

    #[gpui::test]
    fn cmd_f_searches_inside_the_file_without_touching_it(cx: &mut TestAppContext) {
        let (_folder, _root, view, cx) = project(cx);
        let before = view.read_with(cx, |view, cx| {
            view.panes[view.active].editor.read(cx).value().to_string()
        });
        // The editor's own search field takes the typing.
        cx.simulate_keystrokes("cmd-f");
        cx.simulate_input("Total");
        cx.simulate_keystrokes("escape");
        let after = view.read_with(cx, |view, cx| {
            view.panes[view.active].editor.read(cx).value().to_string()
        });
        assert_eq!(after, before);
    }

    #[gpui::test]
    fn replace_all_asks_then_changes_open_and_closed_files(cx: &mut TestAppContext) {
        let (_folder, root, view, cx) = project(cx);
        cx.executor().allow_parking();
        // app.ts is open in the editor; card.tsx is not.
        view.update_in(cx, |view, window, cx| {
            view.focus_search(window, cx);
            view.search.show_replace = true;
            let query = view.search.query.clone();
            query.update(cx, |query, cx| query.set_value("total", window, cx));
            let replace = view.search.replace.clone();
            replace.update(cx, |replace, cx| replace.set_value("sum", window, cx));
            view.run_search(cx);
        });
        cx.executor().advance_clock(TYPING_PAUSE * 2);
        cx.run_until_parked();

        view.update_in(cx, |view, window, cx| {
            view.replace_matches(None, window, cx)
        });
        assert!(cx.has_pending_prompt());
        cx.simulate_prompt_answer("Replace All");
        cx.executor().advance_clock(TYPING_PAUSE * 2);
        cx.run_until_parked();

        assert_eq!(
            fs::read_to_string(root.join("src/card.tsx")).unwrap(),
            "const sum = 1;\nconst x = sum + 1;\n"
        );
        // The open file changed in its editor and waits to be saved.
        assert_eq!(
            fs::read_to_string(root.join("src/app.ts")).unwrap(),
            "// no match here\nlet Total = 2;\n"
        );
        let (text, unsaved, status) = view.read_with(cx, |view, cx| {
            (
                view.panes[view.active].editor.read(cx).value().to_string(),
                view.has_unsaved_changes(),
                view.status_message.clone().unwrap_or_default(),
            )
        });
        assert_eq!(text, "// no match here\nlet sum = 2;\n");
        assert!(unsaved);
        assert_eq!(status, "Replaced 3 matches in 2 files");
    }
}
