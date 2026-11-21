//! The rest of the Explorer, as VS Code's: several files selected with Cmd
//! and Shift; Copy, Cut, Paste and Duplicate, with Finder's free names;
//! Cmd+Z for the last change to the files; a filter over the whole
//! project (Cmd+Alt+F) and letters typed to jump to a name; the folders of
//! the top row kept in sight while scrolling; files dropped from Finder
//! copied in, a folder opening under a file held over it; and the imports
//! a language server updates when a file moves.

use std::cell::Cell;
use std::path::{Path, PathBuf};
use std::time::Instant;

use gpui::{
    AnyElement, ExternalPaths, ScrollStrategy, StatefulInteractiveElement as _, uniform_list,
};
use gpui_component::input::{Input, InputEvent, InputState};
use gpui_component::{Icon, Sizable as _};
use gpui_kit_assets::IconName;

use gpui::Div;

use super::notices::Notice;
use super::shell::Colors;
use super::*;

use super::explorer::{INDENT, ROW_HEIGHT};
/// How long a dragged file must stay on a closed folder to open it.
const HOVER_OPEN: Duration = Duration::from_millis(650);
/// Letters typed within this time make one name to jump to.
const TYPED_PAUSE: Duration = Duration::from_millis(900);

actions!(
    explorer,
    [
        CopyFiles,
        CutFiles,
        PasteFiles,
        UndoFileChange,
        FilterTree,
        DuplicateFiles,
        QuickLookSelected
    ]
);

/// What Quick Look shows better than Keel, as in the Finder: pictures
/// Keel does not open, video, sound, documents of other apps, archives.
const QUICK_LOOK: &[&str] = &[
    "png", "jpg", "jpeg", "gif", "webp", "heic", "heif", "tif", "tiff", "bmp", "ico", "icns",
    "svg", "psd", "pdf", "mp4", "mov", "m4v", "avi", "mkv", "webm", "mp3", "wav", "m4a", "aac",
    "flac", "aiff", "ogg", "key", "pages", "numbers", "doc", "docx", "xls", "xlsx", "ppt", "pptx",
    "rtf", "zip", "dmg", "ttf", "otf", "woff", "woff2", "usdz",
];

/// The Explorer's menu actions on one file or folder.
#[derive(Clone, Debug, PartialEq, gpui::Action)]
#[action(namespace = explorer, no_json)]
pub struct OpenToSide {
    pub path: PathBuf,
}

#[derive(Clone, Debug, PartialEq, gpui::Action)]
#[action(namespace = explorer, no_json)]
pub struct SearchInFolder {
    pub path: PathBuf,
}

#[derive(Clone, Debug, PartialEq, gpui::Action)]
#[action(namespace = explorer, no_json)]
pub struct OpenInTerminal {
    pub path: PathBuf,
}

#[derive(Clone, Debug, PartialEq, gpui::Action)]
#[action(namespace = explorer, no_json)]
pub struct SelectForCompare {
    pub path: PathBuf,
}

#[derive(Clone, Debug, PartialEq, gpui::Action)]
#[action(namespace = explorer, no_json)]
pub struct CompareWithSelected {
    pub path: PathBuf,
}

/// A change to the files Cmd+Z can take back.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum FileChange {
    Moved {
        from: PathBuf,
        to: PathBuf,
    },
    /// Made new, by New File or Paste: Undo puts it in the Trash.
    Made(PathBuf),
    Trashed {
        original: PathBuf,
        trashed: PathBuf,
    },
}

#[derive(Default)]
pub(super) struct TreeExtras {
    /// More rows selected with Cmd or Shift, besides `tree_selection`.
    pub(super) selected: Vec<PathBuf>,
    /// Files copied or cut, for Paste, and whether they were cut.
    clipboard: Option<(Vec<PathBuf>, bool)>,
    /// What Cmd+Z takes back, the last first.
    pub(super) changes: Vec<FileChange>,
    /// The filter over the project, while open, and what it found.
    pub(super) filter: Option<Entity<InputState>>,
    filter_matches: Vec<String>,
    _filter_subscription: Option<Subscription>,
    /// Letters typed to jump, and when the last came.
    typed: String,
    typed_at: Option<Instant>,
    /// The first row in sight, for the folders kept on top.
    pub(super) top_row: Cell<usize>,
    /// The closed folder a dragged file is held over.
    hover: Option<PathBuf>,
    hover_task: Option<Task<()>>,
    /// A file chosen to compare with another one.
    pub(super) compare_with: Option<PathBuf>,
    /// The Quick Look window open, to close it before the next.
    quick_look: Option<std::process::Child>,
}

impl Workbench {
    pub(super) fn tree_extra_actions(&self, root: Div, cx: &mut Context<Self>) -> Div {
        root.on_action(cx.listener(|this, _: &CopyFiles, _, cx| this.copy_files(false, cx)))
            .on_action(cx.listener(|this, _: &QuickLookSelected, window, cx| {
                if !this.typing_a_name(cx) {
                    this.quick_look_selected(window, cx)
                }
            }))
            .on_action(cx.listener(|this, _: &CutFiles, _, cx| this.copy_files(true, cx)))
            .on_action(cx.listener(|this, _: &PasteFiles, window, cx| this.paste_files(window, cx)))
            .on_action(cx.listener(|this, _: &DuplicateFiles, window, cx| {
                this.copy_files(false, cx);
                this.paste_files(window, cx)
            }))
            .on_action(
                cx.listener(|this, _: &UndoFileChange, window, cx| {
                    this.undo_file_change(window, cx)
                }),
            )
            .on_action(
                cx.listener(|this, _: &FilterTree, window, cx| this.open_tree_filter(window, cx)),
            )
            .on_action(cx.listener(|this, action: &OpenToSide, window, cx| {
                this.open_to_side(&action.path, window, cx)
            }))
            .on_action(cx.listener(|this, action: &SearchInFolder, window, cx| {
                this.search_in_folder(&action.path, window, cx)
            }))
            .on_action(cx.listener(|this, action: &OpenInTerminal, window, cx| {
                this.open_in_terminal(action.path.clone(), window, cx)
            }))
            .on_action(cx.listener(|this, action: &SelectForCompare, _, cx| {
                this.tree_extras.compare_with = Some(action.path.clone());
                cx.notify();
            }))
            .on_action(
                cx.listener(|this, action: &CompareWithSelected, window, cx| {
                    if let Some(first) = this.tree_extras.compare_with.clone() {
                        this.open_diff(
                            &first.to_string_lossy(),
                            super::diff_view::DiffSource::Files(
                                action.path.to_string_lossy().into_owned(),
                            ),
                            window,
                            cx,
                        );
                    }
                }),
            )
    }

    // -----------------------------------------------------------------
    // Several rows

    /// The rows chosen, in the tree's order: the selected one and those
    /// added with Cmd or Shift.
    pub(super) fn chosen_paths(&self) -> Vec<PathBuf> {
        let mut chosen: Vec<PathBuf> = self.tree_extras.selected.clone();
        if let Some(selection) = &self.tree_selection
            && !chosen.contains(selection)
        {
            chosen.push(selection.clone());
        }
        let order = |path: &PathBuf| {
            self.tree_rows
                .iter()
                .position(|row| &row.path == path)
                .unwrap_or(usize::MAX)
        };
        chosen.sort_by_key(order);
        chosen
    }

    pub(super) fn is_chosen(&self, path: &Path) -> bool {
        self.tree_extras
            .selected
            .iter()
            .any(|chosen| chosen == path)
    }

    /// Cmd+click adds a row or takes it away; Shift+click takes the rows
    /// from the last one clicked.
    pub(super) fn choose_row(&mut self, index: usize, shift: bool) {
        let Some(path) = self.tree_rows.get(index).map(|row| row.path.clone()) else {
            return;
        };
        if shift {
            let anchor = self
                .tree_selection
                .as_ref()
                .and_then(|anchor| self.tree_rows.iter().position(|row| &row.path == anchor))
                .unwrap_or(index);
            let (from, to) = (anchor.min(index), anchor.max(index));
            self.tree_extras.selected = self.tree_rows[from..=to]
                .iter()
                .map(|row| row.path.clone())
                .collect();
            return;
        }
        if let Some(at) = self
            .tree_extras
            .selected
            .iter()
            .position(|chosen| *chosen == path)
        {
            self.tree_extras.selected.remove(at);
        } else {
            if let Some(previous) = self.tree_selection.clone()
                && !self.tree_extras.selected.contains(&previous)
            {
                self.tree_extras.selected.push(previous);
            }
            self.tree_extras.selected.push(path.clone());
        }
        self.tree_selection = Some(path);
    }

    // -----------------------------------------------------------------
    // Copy, Cut, Paste

    /// Space, as in the Finder: Quick Look for what Keel would not show as
    /// well; a text file opens, a folder opens or closes.
    fn quick_look_selected(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(index) = self.selected_index() else {
            return;
        };
        let row = self.tree_rows[index].clone();
        let looks = !row.is_dir
            && row
                .path
                .extension()
                .and_then(|ext| ext.to_str())
                .is_some_and(|ext| QUICK_LOOK.contains(&ext.to_lowercase().as_str()));
        if !looks {
            self.open_selected(window, cx);
            return;
        }
        if let Some(mut open) = self.tree_extras.quick_look.take() {
            let _ = open.kill();
            let _ = open.wait();
        }
        self.tree_extras.quick_look = std::process::Command::new("/usr/bin/qlmanage")
            .arg("-p")
            .arg(&row.path)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .ok();
    }

    fn copy_files(&mut self, cut: bool, cx: &mut Context<Self>) {
        let chosen = self.chosen_paths();
        if chosen.is_empty() {
            return;
        }
        // The files go to the system clipboard too: as files, so Finder
        // pastes them and Mail attaches them, and as their paths, for the
        // terminal.
        let text = chosen
            .iter()
            .map(|path| path.to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join("\n");
        if !crate::mac::copy_files(&chosen, &text) {
            cx.write_to_clipboard(gpui::ClipboardItem::new_string(text));
        }
        self.tree_extras.clipboard = Some((chosen, cut));
        cx.notify();
    }

    /// Paste: into the selected folder, or the selected file's folder.
    fn paste_files(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some((paths, cut)) = self.tree_extras.clipboard.clone() else {
            return;
        };
        let Some(folder) = self.paste_folder() else {
            return;
        };
        let word = self.strings().explorer_copy_word;
        let mut last = None;
        for path in &paths {
            let result = if cut {
                files::move_into(path, &folder).inspect(|target| {
                    self.path_moved(path, target, window, cx);
                    self.tree_extras.changes.push(FileChange::Moved {
                        from: path.clone(),
                        to: target.clone(),
                    });
                })
            } else {
                files::copy_into(path, &folder, word).inspect(|target| {
                    self.tree_extras
                        .changes
                        .push(FileChange::Made(target.clone()));
                })
            };
            match result {
                Ok(target) => last = Some(target),
                Err(error) => {
                    let name = display_name_of(path);
                    let message = self.strings().could_not_change(&name, &error.to_string());
                    self.report(message);
                }
            }
        }
        if cut {
            self.tree_extras.clipboard = None;
        }
        self.after_file_change(last, cx);
    }

    fn paste_folder(&self) -> Option<PathBuf> {
        let selected = self
            .tree_selection
            .clone()
            .or_else(|| self.project.root.clone())?;
        if selected.is_dir() {
            Some(selected)
        } else {
            selected.parent().map(Path::to_path_buf)
        }
    }

    /// After files changed: the tree reads them again, and shows `shown`.
    pub(super) fn after_file_change(&mut self, shown: Option<PathBuf>, cx: &mut Context<Self>) {
        self.refresh_tree(cx);
        if let Some(path) = shown {
            if let Some(tree) = &mut self.tree {
                tree.reveal(&path);
            }
            self.tree_changed();
            self.tree_selection = Some(path);
            self.tree_extras.selected.clear();
            self.scroll_tree_to_selection();
        }
        cx.notify();
    }

    // -----------------------------------------------------------------
    // Undo

    /// Cmd+Z in the tree: the last rename, move, new file, paste or trash
    /// goes back.
    fn undo_file_change(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(change) = self.tree_extras.changes.pop() else {
            return;
        };
        let strings = self.strings();
        let result = match &change {
            FileChange::Moved { from, to } => std::fs::rename(to, from).map(|()| {
                self.path_moved(to, from, window, cx);
                from.clone()
            }),
            FileChange::Made(path) => files::trash(path)
                .map(|()| path.parent().map(Path::to_path_buf).unwrap_or_default()),
            FileChange::Trashed { original, trashed } => {
                std::fs::rename(trashed, original).map(|()| original.clone())
            }
        };
        match result {
            Ok(shown) => {
                self.after_file_change(Some(shown), cx);
                self.notify_user(Notice::info(strings.explorer_undone), window, cx);
            }
            Err(error) => {
                self.notify_user(
                    Notice::warning(strings.explorer_undo_failed).message(error.to_string()),
                    window,
                    cx,
                );
            }
        }
    }

    /// Several files and folders to the Trash, after asking once.
    pub(super) fn trash_paths(
        &mut self,
        paths: Vec<PathBuf>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let strings = self.strings();
        let question = strings.trash_question(&strings.explorer_items(paths.len()));
        let answer = window.prompt(
            PromptLevel::Warning,
            &question,
            Some(strings.trash_detail),
            &[strings.explorer_trash, strings.cancel],
            cx,
        );
        cx.spawn_in(window, async move |this, cx| {
            if answer.await != Ok(0) {
                return;
            }
            this.update(cx, |this, cx| {
                for path in &paths {
                    match files::trash(path) {
                        Ok(()) => this.remember_trashed(path),
                        Err(error) => {
                            let message = this
                                .strings()
                                .could_not_change(&display_name_of(path), &error.to_string());
                            this.report(message);
                        }
                    }
                }
                this.tree_extras.selected.clear();
                this.refresh_tree(cx);
            })
            .ok();
        })
        .detach();
    }

    /// A file or folder went to the Trash: Cmd+Z can bring it back.
    pub(super) fn remember_trashed(&mut self, original: &Path) {
        if let Some(trashed) = files::trashed_location(original) {
            self.tree_extras.changes.push(FileChange::Trashed {
                original: original.to_path_buf(),
                trashed,
            });
        }
    }

    // -----------------------------------------------------------------
    // Filter and letters

    /// Cmd+Alt+F: a field over the tree; what it finds in the whole
    /// project shows in place of the tree, Enter opens the first.
    fn open_tree_filter(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let placeholder = self.strings().explorer_filter;
        let input = cx.new(|cx| InputState::new(window, cx).placeholder(placeholder));
        let subscription = cx.subscribe_in(
            &input,
            window,
            |this, input, event: &InputEvent, window, cx| match event {
                InputEvent::Change => {
                    let query = input.read(cx).value().to_string();
                    this.filter_tree(&query);
                    cx.notify();
                }
                InputEvent::PressEnter { .. } => {
                    if let Some(first) = this.tree_extras.filter_matches.first().cloned() {
                        this.open_filtered(&first, window, cx);
                    }
                }
                _ => {}
            },
        );
        input.update(cx, |input, cx| input.focus(window, cx));
        self.tree_extras.filter = Some(input);
        self.tree_extras.filter_matches.clear();
        self.tree_extras._filter_subscription = Some(subscription);
        self.layout
            .show_side_view(crate::layout::SideView::Explorer);
        cx.notify();
    }

    pub(super) fn close_tree_filter(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.tree_extras.filter.take().is_none() {
            return false;
        }
        self.tree_extras.filter_matches.clear();
        self.tree_extras._filter_subscription = None;
        self.tree_focus.focus(window, cx);
        cx.notify();
        true
    }

    fn filter_tree(&mut self, query: &str) {
        let query = query.trim();
        if query.is_empty() {
            self.tree_extras.filter_matches.clear();
            return;
        }
        let mut scored: Vec<(i32, &String)> = self
            .file_index
            .iter()
            .flat_map(|index| index.iter())
            .filter_map(|path| Some((files::fuzzy::score_path(query, path)?, path)))
            .collect();
        scored.sort_by_key(|(score, _)| std::cmp::Reverse(*score));
        self.tree_extras.filter_matches = scored
            .into_iter()
            .take(200)
            .map(|(_, path)| path.clone())
            .collect();
    }

    fn open_filtered(&mut self, relative: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(root) = self.project.root.clone() else {
            return;
        };
        let path = root.join(relative);
        self.close_tree_filter(window, cx);
        self.open_file(&path, window, cx);
    }

    /// Letters typed in the tree jump to the next name starting with them,
    /// as in Finder.
    pub(super) fn tree_type_ahead(&mut self, key: &str, cx: &mut Context<Self>) -> bool {
        let mut chars = key.chars();
        let (Some(letter), None) = (chars.next(), chars.next()) else {
            return false;
        };
        if !(letter.is_alphanumeric() || matches!(letter, '.' | '_' | '-')) {
            return false;
        }
        let now = Instant::now();
        if self
            .tree_extras
            .typed_at
            .is_none_or(|at| now.duration_since(at) > TYPED_PAUSE)
        {
            self.tree_extras.typed.clear();
        }
        self.tree_extras.typed_at = Some(now);
        self.tree_extras.typed.push(letter.to_ascii_lowercase());
        let typed = self.tree_extras.typed.clone();
        let start = self.selected_row().unwrap_or(0);
        // One letter again goes to the next name with it; more letters
        // stay on the name if it still fits.
        let from = if typed.chars().count() == 1 {
            start + 1
        } else {
            start
        };
        let count = self.tree_rows.len();
        let found = (0..count)
            .map(|offset| (from + offset) % count.max(1))
            .find(|index| {
                self.tree_rows[*index]
                    .name
                    .to_lowercase()
                    .starts_with(&typed)
            });
        if let Some(index) = found {
            self.tree_selection = Some(self.tree_rows[index].path.clone());
            self.tree_extras.selected.clear();
            self.scroll_tree_to_selection();
            cx.notify();
        }
        true
    }

    fn selected_row(&self) -> Option<usize> {
        let selection = self.tree_selection.as_ref()?;
        self.tree_rows.iter().position(|row| &row.path == selection)
    }

    /// The filter's field and its matches, in place of the tree.
    pub(super) fn render_tree_filter(
        &self,
        colors: Colors,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let input = self.tree_extras.filter.as_ref()?;
        let count = self.tree_extras.filter_matches.len();
        let list = uniform_list(
            "tree-filter-matches",
            count,
            cx.processor(move |this, range: std::ops::Range<usize>, _, cx| {
                range
                    .filter_map(|index| {
                        let path = this.tree_extras.filter_matches.get(index)?.clone();
                        let (folder, name) = match path.rsplit_once('/') {
                            Some((folder, name)) => (folder.to_owned(), name.to_owned()),
                            None => (String::new(), path.clone()),
                        };
                        let icon = this.file_icon_element(Path::new(&path), cx);
                        Some(
                            div()
                                .id(("tree-filter-match", index))
                                .h(px(ROW_HEIGHT))
                                .mx_1()
                                .px_2()
                                .flex()
                                .items_center()
                                .gap_1p5()
                                .rounded(px(6.))
                                .text_sm()
                                .cursor_pointer()
                                .when(index == 0, |row| row.bg(Colors::new(cx).hover))
                                .hover(|row| row.bg(Colors::new(cx).hover))
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.open_filtered(&path, window, cx)
                                }))
                                .child(icon)
                                .child(div().flex_none().child(name))
                                .child(
                                    div()
                                        .min_w_0()
                                        .truncate()
                                        .text_xs()
                                        .text_color(Colors::new(cx).faint)
                                        .child(folder),
                                )
                                .into_any_element(),
                        )
                    })
                    .collect::<Vec<_>>()
            }),
        )
        .size_full();
        Some(
            div()
                .flex_1()
                .min_h_0()
                .flex()
                .flex_col()
                .child(
                    div()
                        .px_2()
                        .py_1()
                        .flex()
                        .items_center()
                        .gap_1()
                        .child(Icon::new(IconName::Funnel).small().text_color(colors.faint))
                        .child(
                            div()
                                .flex_1()
                                .child(Input::new(input).small().cleanable(true)),
                        ),
                )
                .child(div().flex_1().min_h_0().child(list))
                .into_any_element(),
        )
    }

    // -----------------------------------------------------------------
    // Folders kept on top

    /// The open folders above the first row in sight, drawn over the top
    /// of the tree, as VS Code's sticky scroll; a click goes to the folder.
    pub(super) fn render_sticky_folders(
        &self,
        colors: Colors,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let top = {
            let state = self.tree_scroll.0.borrow();
            (-state.base_handle.offset().y.as_f32() / ROW_HEIGHT)
                .floor()
                .max(0.) as usize
        };
        self.tree_extras.top_row.set(top);
        let first = self.tree_rows.get(top)?;
        let mut chain: Vec<(usize, &files::Row)> = Vec::new();
        let mut depth = first.depth;
        for index in (0..top).rev() {
            let row = &self.tree_rows[index];
            if row.depth < depth && row.is_dir && row.expanded {
                chain.push((index, row));
                depth = row.depth;
                if depth == 0 {
                    break;
                }
            }
        }
        if chain.is_empty() {
            return None;
        }
        chain.reverse();
        chain.truncate(5);
        Some(
            div()
                .absolute()
                .top_0()
                .left_0()
                .right_0()
                .bg(colors.panel)
                .border_b_1()
                .border_color(colors.border.opacity(0.7))
                .shadow_sm()
                .children(chain.into_iter().map(|(index, row)| {
                    div()
                        .id(("tree-sticky", index))
                        .h(px(ROW_HEIGHT))
                        .pl(px(8. + row.depth as f32 * INDENT))
                        .flex()
                        .items_center()
                        .gap_1p5()
                        .text_sm()
                        .cursor_pointer()
                        .hover(|item| item.bg(colors.hover))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.tree_scroll.scroll_to_item(index, ScrollStrategy::Top);
                            cx.notify();
                        }))
                        .child(
                            Icon::new(IconName::ChevronDown)
                                .xsmall()
                                .text_color(colors.faint),
                        )
                        .child(
                            Icon::new(IconName::FolderOpen)
                                .small()
                                .text_color(colors.accent),
                        )
                        .child(div().truncate().child(row.name.clone()))
                }))
                .into_any_element(),
        )
    }

    /// Scrolling the tree: the folders on top follow when the first row
    /// in sight changes.
    pub(super) fn tree_scrolled(&mut self, cx: &mut Context<Self>) {
        let top = {
            let state = self.tree_scroll.0.borrow();
            (-state.base_handle.offset().y.as_f32() / ROW_HEIGHT)
                .floor()
                .max(0.) as usize
        };
        if top != self.tree_extras.top_row.get() {
            cx.notify();
        }
    }

    // -----------------------------------------------------------------
    // Dragging

    /// Files dropped from Finder, or another app, are copied in.
    pub(super) fn drop_external(
        &mut self,
        paths: &ExternalPaths,
        folder: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let word = self.strings().explorer_copy_word;
        let mut last = None;
        for path in paths.paths() {
            match files::copy_into(path, &folder, word) {
                Ok(target) => {
                    self.tree_extras
                        .changes
                        .push(FileChange::Made(target.clone()));
                    last = Some(target);
                }
                Err(error) => {
                    let message = self
                        .strings()
                        .could_not_change(&display_name_of(path), &error.to_string());
                    self.report(message);
                }
            }
        }
        self.after_file_change(last, cx);
        let _ = window;
    }

    /// A dragged file held over a closed folder opens it after a moment.
    pub(super) fn drag_over_folder(&mut self, folder: Option<PathBuf>, cx: &mut Context<Self>) {
        if self.tree_extras.hover == folder {
            return;
        }
        self.tree_extras.hover = folder.clone();
        let Some(folder) = folder else {
            self.tree_extras.hover_task = None;
            return;
        };
        self.tree_extras.hover_task = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(HOVER_OPEN).await;
            this.update(cx, |this, cx| {
                if this.tree_extras.hover.as_ref() == Some(&folder) {
                    if let Some(tree) = &mut this.tree {
                        tree.expand(&folder);
                    }
                    this.tree_changed();
                    cx.notify();
                }
            })
            .ok();
        }));
    }

    // -----------------------------------------------------------------
    // The menu's other commands

    /// Opens a file in the group to the right, making one if needed.
    pub(super) fn open_to_side(
        &mut self,
        path: &Path,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if path.is_dir() {
            return;
        }
        let Ok(uri) = self.load_document(path, window, cx) else {
            return;
        };
        let column = self.side_group(self.active);
        self.show_tab(&uri, column, tabs::OpenAs::Normal, true, window, cx);
        cx.notify();
    }

    /// Find in Folder: the search, limited to the folder.
    fn search_in_folder(&mut self, folder: &Path, window: &mut Window, cx: &mut Context<Self>) {
        let Some(root) = self.project.root.clone() else {
            return;
        };
        let folder = if folder.is_dir() {
            folder.to_path_buf()
        } else {
            folder
                .parent()
                .map(Path::to_path_buf)
                .unwrap_or(root.clone())
        };
        let relative = folder
            .strip_prefix(&root)
            .map(|path| path.to_string_lossy().into_owned())
            .unwrap_or_default();
        self.search_only_in(&relative, window, cx);
    }

    /// A terminal in the folder.
    fn open_in_terminal(&mut self, folder: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        let folder = if folder.is_dir() {
            folder
        } else {
            folder.parent().map(Path::to_path_buf).unwrap_or(folder)
        };
        let spawn = terminal_view::shell_spawn(folder, cx);
        self.add_terminal(spawn, None, true, window, cx);
        cx.notify();
    }

    // -----------------------------------------------------------------
    // Imports that follow a file

    /// Before a file moves: the language servers that update imports are
    /// asked, as VS Code's `workspace/willRenameFiles`, and their edits
    /// applied; then the file moves. Without such a server it moves at once.
    pub(super) fn move_with_imports(
        &mut self,
        moves: Vec<(PathBuf, PathBuf)>,
        then: impl FnOnce(&mut Self, &mut Window, &mut Context<Self>) + 'static,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let files: Vec<Value> = moves
            .iter()
            .map(|(from, to)| {
                json!({
                    "oldUri": Uri::file(from).to_string(),
                    "newUri": Uri::file(to).to_string(),
                })
            })
            .collect();
        let servers: Vec<_> = moves
            .iter()
            .flat_map(|(from, _)| self.languages.servers_for(&Uri::file(from).to_string()))
            .filter(|running| {
                running.server.capabilities().is_some_and(|capabilities| {
                    capabilities
                        .workspace
                        .as_ref()
                        .and_then(|workspace| workspace.file_operations.as_ref())
                        .and_then(|operations| operations.will_rename.as_ref())
                        .is_some()
                })
            })
            .collect();
        if servers.is_empty() || files.is_empty() {
            then(self, window, cx);
            return;
        }
        let params = json!({ "files": files });
        let executor = cx.background_executor().clone();
        cx.spawn_in(window, async move |this, cx| {
            let mut edits = Vec::new();
            for running in servers {
                // A slow server does not hold the move for long.
                let request = running
                    .server
                    .request_raw("workspace/willRenameFiles", params.clone());
                let timeout = executor.timer(Duration::from_secs(3));
                let answer = futures::future::select(Box::pin(request), Box::pin(timeout)).await;
                if let futures::future::Either::Left((Ok(value), _)) = answer
                    && let Ok(Some(edit)) =
                        serde_json::from_value::<Option<lsp_types::WorkspaceEdit>>(value)
                {
                    edits.push(edit);
                }
            }
            this.update_in(cx, |this, window, cx| {
                for edit in edits {
                    this.apply_workspace_edit(&edit, window, cx);
                }
                then(this, window, cx);
            })
            .ok();
        })
        .detach();
    }
}

fn display_name_of(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

/// The lines a row draws for the folders above it, one per level.
pub(super) fn indent_guides(depth: usize, colors: Colors) -> impl Iterator<Item = gpui::Div> {
    (0..depth).map(move |level| {
        div()
            .absolute()
            .top_0()
            .bottom_0()
            .left(px(8. + level as f32 * INDENT + 5.5))
            .w(px(1.))
            .bg(colors.border.opacity(0.0))
            .group_hover("file-tree-area", move |guide| guide.bg(colors.border))
    })
}

#[cfg(test)]
mod tests {
    use std::fs;

    use gpui::{TestAppContext, VisualTestContext};

    use super::*;

    fn names(view: &Entity<Workbench>, cx: &mut VisualTestContext) -> Vec<String> {
        view.read_with(cx, |view, _| {
            view.tree_rows
                .iter()
                .map(|row| format!("{}{}", "  ".repeat(row.depth), row.name))
                .collect()
        })
    }

    #[gpui::test]
    fn the_explorer_copies_moves_undoes_and_finds(cx: &mut TestAppContext) {
        cx.update(|cx| {
            gpui_component::init(cx);
            language::register_grammars();
            crate::workbench_window::init(cx);
        });
        let folder = tempfile::tempdir().unwrap();
        let base = fs::canonicalize(folder.path()).unwrap();
        let root = base.join("app");
        fs::create_dir_all(root.join("src/lib")).unwrap();
        fs::create_dir_all(root.join("docs")).unwrap();
        for name in [
            "src/card.tsx",
            "src/card.test.tsx",
            "src/page.tsx",
            "package.json",
            "package-lock.json",
        ] {
            fs::write(root.join(name), name).unwrap();
        }
        let settings_file = base.join("settings.json");
        fs::write(&settings_file, r#"{ "keel.language": "en" }"#).unwrap();
        let settings = Settings::load(&settings_file, None);
        let opening = Opening::new(Some(root.clone()), None);
        let (view, cx) =
            cx.add_window_view(|window, cx| Workbench::new(opening, settings, None, window, cx));
        cx.run_until_parked();
        view.update(cx, |view, cx| {
            if let Some(tree) = &mut view.tree {
                tree.expand(&root.join("src"));
            }
            view.tree_changed();
            cx.notify();
        });
        // The lock nests under package.json, the test under its file.
        assert_eq!(
            names(&view, cx),
            [
                "docs",
                "src",
                "  lib",
                "  card.tsx",
                "  page.tsx",
                "package.json"
            ]
        );

        // Cmd adds a row, Shift takes a run of them.
        let index = |view: &Entity<Workbench>, cx: &mut VisualTestContext, name: &str| {
            view.read_with(cx, |view, _| {
                view.tree_rows
                    .iter()
                    .position(|row| row.name == name)
                    .unwrap()
            })
        };
        let card = index(&view, cx, "card.tsx");
        let page = index(&view, cx, "page.tsx");
        view.update(cx, |view, _| {
            view.tree_selection = Some(root.join("src/card.tsx"));
            view.choose_row(page, true);
            assert_eq!(
                view.chosen_paths(),
                [root.join("src/card.tsx"), root.join("src/page.tsx")]
            );
            view.tree_extras.selected.clear();
            view.tree_selection = Some(root.join("src/card.tsx"));
            let _ = card;
        });

        // Copy and Paste in the same folder: a free name.
        cx.dispatch_action(CopyFiles);
        cx.dispatch_action(PasteFiles);
        cx.run_until_parked();
        assert!(root.join("src/card copy.tsx").is_file());

        // Cut page.tsx, paste it into docs, then Cmd+Z puts it back.
        view.update(cx, |view, _| {
            view.tree_selection = Some(root.join("src/page.tsx"));
        });
        cx.dispatch_action(CutFiles);
        view.update(cx, |view, _| view.tree_selection = Some(root.join("docs")));
        cx.dispatch_action(PasteFiles);
        cx.run_until_parked();
        assert!(root.join("docs/page.tsx").is_file());
        assert!(!root.join("src/page.tsx").exists());
        cx.dispatch_action(UndoFileChange);
        cx.run_until_parked();
        assert!(root.join("src/page.tsx").is_file());
        assert!(!root.join("docs/page.tsx").exists());

        // Letters jump to a name, as in Finder.
        view.update(cx, |view, cx| {
            view.tree_selection = None;
            assert!(view.tree_type_ahead("p", cx));
            assert_eq!(
                view.tree_selection.as_deref(),
                Some(root.join("src/page.tsx").as_path())
            );
        });

        // The filter finds a file anywhere in the project.
        view.update(cx, |view, _| view.filter_tree("lock"));
        let found = view.read_with(cx, |view, _| view.tree_extras.filter_matches.clone());
        assert_eq!(found.first().map(String::as_str), Some("package-lock.json"));
    }
}
