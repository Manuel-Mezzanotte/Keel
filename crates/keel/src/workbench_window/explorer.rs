//! The Explorer: the project's files as a tree, and creating, renaming,
//! moving and trashing them, as in VS Code.

use std::ops::Range;
use std::path::{Path, PathBuf};

use files::{FileTree, Row};
use gpui::{
    AnyElement, ClickEvent, ClipboardItem, Div, Render, StatefulInteractiveElement as _,
    uniform_list,
};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::input::{Input, InputState};
use gpui_component::menu::ContextMenuExt as _;
use gpui_component::{Icon, Sizable as _};
use gpui_kit_assets::IconName;

use super::shell::{Colors, heading, note};
use super::*;

const ROW_HEIGHT: f32 = 24.;
const INDENT: f32 = 14.;

actions!(
    explorer,
    [
        SelectPrevious,
        SelectNext,
        CollapseSelected,
        ExpandSelected,
        RenameSelected,
        OpenSelected,
        TrashSelected,
        NewFileHere,
        NewFolderHere,
        Refresh,
        CollapseAll
    ]
);

/// Actions of the Explorer's context menu, on a given file or folder.
#[derive(Clone, Debug, PartialEq, gpui::Action)]
#[action(namespace = explorer, no_json)]
pub struct NewFileIn {
    pub folder: PathBuf,
}

#[derive(Clone, Debug, PartialEq, gpui::Action)]
#[action(namespace = explorer, no_json)]
pub struct NewFolderIn {
    pub folder: PathBuf,
}

#[derive(Clone, Debug, PartialEq, gpui::Action)]
#[action(namespace = explorer, no_json)]
pub struct RenamePath {
    pub path: PathBuf,
}

#[derive(Clone, Debug, PartialEq, gpui::Action)]
#[action(namespace = explorer, no_json)]
pub struct TrashPath {
    pub path: PathBuf,
}

#[derive(Clone, Debug, PartialEq, gpui::Action)]
#[action(namespace = explorer, no_json)]
pub struct RevealInFinder {
    pub path: PathBuf,
}

#[derive(Clone, Debug, PartialEq, gpui::Action)]
#[action(namespace = explorer, no_json)]
pub struct CopyPath {
    pub path: PathBuf,
    pub relative: bool,
}

/// The keys of the tree, as in VS Code on the Mac.
pub const TREE_CONTEXT: &str = "FileTree";
pub fn tree_bindings() -> Vec<gpui::KeyBinding> {
    use gpui::KeyBinding;
    let context = Some(TREE_CONTEXT);
    vec![
        KeyBinding::new("up", SelectPrevious, context),
        KeyBinding::new("down", SelectNext, context),
        KeyBinding::new("left", CollapseSelected, context),
        KeyBinding::new("right", ExpandSelected, context),
        KeyBinding::new("enter", RenameSelected, context),
        KeyBinding::new("f2", RenameSelected, context),
        KeyBinding::new("cmd-down", OpenSelected, context),
        KeyBinding::new("space", OpenSelected, context),
        KeyBinding::new("cmd-backspace", TrashSelected, context),
    ]
}

/// What the name field in the tree is for.
#[derive(Clone, Debug, PartialEq)]
pub(super) enum TreeEditKind {
    NewFile { folder: PathBuf },
    NewFolder { folder: PathBuf },
    Rename { path: PathBuf },
}

pub(super) struct TreeEdit {
    kind: TreeEditKind,
    input: Entity<InputState>,
    _subscription: Subscription,
}

/// A file or folder being dragged in the tree.
#[derive(Clone)]
pub(super) struct DraggedPath {
    path: PathBuf,
    name: String,
}

impl Render for DraggedPath {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        div()
            .px_2()
            .py_1()
            .rounded(px(6.))
            .border_1()
            .border_color(theme.border)
            .bg(theme.popover)
            .text_sm()
            .text_color(theme.foreground)
            .child(self.name.clone())
    }
}

/// The glob patterns of hidden files: VS Code's defaults and the user's
/// `files.exclude`, where `{ "**/dist": true }` hides `dist`.
pub(super) fn excludes(settings: &Settings) -> Vec<String> {
    let mut patterns: Vec<String> = files::DEFAULT_EXCLUDES
        .iter()
        .map(|pattern| pattern.to_string())
        .collect();
    if let Some(Value::Object(user)) = settings.get("files.exclude") {
        for (pattern, hidden) in user {
            if hidden.as_bool() == Some(true) {
                patterns.push(pattern.clone());
            } else {
                patterns.retain(|known| known != pattern);
            }
        }
    }
    patterns
}

/// The icon and colour of a file, from its name.
fn file_icon(name: &str) -> (IconName, Option<u32>) {
    let extension = name
        .rsplit_once('.')
        .map(|(_, extension)| extension.to_ascii_lowercase());
    match extension.as_deref() {
        Some("ts" | "mts" | "cts") => (IconName::FileCode, Some(0x3178C6)),
        Some("tsx") => (IconName::FileCode, Some(0x3B9EEB)),
        Some("js" | "mjs" | "cjs" | "jsx") => (IconName::FileCode, Some(0xE8D44D)),
        Some("rs") => (IconName::FileCode, Some(0xDEA584)),
        Some("py") => (IconName::FileCode, Some(0x4B8BBE)),
        Some("java") => (IconName::FileCode, Some(0xE76F00)),
        Some("c" | "h") => (IconName::FileCode, Some(0x7F8DB2)),
        Some("cpp" | "cc" | "hpp") => (IconName::FileCode, Some(0xF34B7D)),
        Some("cs") => (IconName::FileCode, Some(0x9B4F96)),
        Some("swift") => (IconName::FileCode, Some(0xF05138)),
        Some("html" | "htm") => (IconName::FileCode, Some(0xE44D26)),
        Some("css" | "scss") => (IconName::FileCode, Some(0x42A5F5)),
        Some("json" | "jsonc") => (IconName::FileBraces, Some(0xCBCB41)),
        Some("toml" | "yaml" | "yml" | "prisma") => (IconName::FileBraces, Some(0x8A8F98)),
        Some("md" | "mdx" | "txt") => (IconName::FileText, Some(0x519ABA)),
        Some("png" | "jpg" | "jpeg" | "gif" | "webp" | "svg" | "ico") => {
            (IconName::FileImage, Some(0xA074C4))
        }
        _ => (IconName::File, None),
    }
}

impl Workbench {
    // -----------------------------------------------------------------
    // The tree

    /// Builds the tree of the window's project.
    pub(super) fn build_tree(&mut self) {
        self.tree = self
            .project
            .root
            .as_ref()
            .map(|root| FileTree::new(root.clone(), &excludes(&self.settings)));
        self.tree_changed();
    }

    pub(super) fn tree_changed(&mut self) {
        self.tree_rows = self.tree.as_ref().map(FileTree::rows).unwrap_or_default();
    }

    /// Reads the open folders again.
    pub(super) fn refresh_tree(&mut self, cx: &mut Context<Self>) {
        if let Some(tree) = &mut self.tree {
            tree.refresh();
        }
        self.tree_changed();
        cx.notify();
    }

    /// Opens the folders down to the active file and selects it, as VS
    /// Code's `explorer.autoReveal` does.
    pub(super) fn reveal_active_in_tree(&mut self) {
        let auto_reveal = self
            .settings
            .get("explorer.autoReveal")
            .and_then(Value::as_bool)
            .unwrap_or(true);
        let Some(path) = self.active_file_path() else {
            return;
        };
        if !auto_reveal {
            return;
        }
        if let Some(tree) = &mut self.tree {
            tree.reveal(&path);
            self.tree_selection = Some(path);
            self.tree_changed();
        }
    }

    fn active_file_path(&self) -> Option<PathBuf> {
        let pane = self.panes.get(self.active)?;
        match &self.documents.get(&pane.uri)?.backing {
            Backing::File(document) => Some(document.path().to_path_buf()),
            _ => None,
        }
    }

    fn selected_index(&self) -> Option<usize> {
        let selection = self.tree_selection.as_ref()?;
        self.tree_rows.iter().position(|row| &row.path == selection)
    }

    /// The folder new files go in: the selected folder, the selected file's
    /// folder, or the project.
    fn target_folder(&self) -> Option<PathBuf> {
        let root = self.project.root.clone()?;
        let Some(selection) = &self.tree_selection else {
            return Some(root);
        };
        Some(if selection.is_dir() {
            selection.clone()
        } else {
            selection.parent().map(Path::to_path_buf).unwrap_or(root)
        })
    }

    fn click_tree_row(
        &mut self,
        index: usize,
        event: &ClickEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(row) = self.tree_rows.get(index).cloned() else {
            return;
        };
        self.tree_selection = Some(row.path.clone());
        if row.is_dir {
            if let Some(tree) = &mut self.tree {
                tree.toggle(&row.path);
            }
            self.tree_changed();
            self.tree_focus.focus(window, cx);
        } else {
            // One click shows the file and keeps the keyboard in the tree;
            // a double click moves to the editor.
            self.open_file(&row.path, window, cx);
            if event.click_count() < 2 {
                self.tree_focus.focus(window, cx);
            }
        }
        cx.notify();
    }

    /// While a name is being typed, the tree's keys (arrows, Enter, space,
    /// Backspace) belong to the name field.
    fn typing_a_name(&self, cx: &mut Context<Self>) -> bool {
        if self.tree_edit.is_some() {
            cx.propagate();
            return true;
        }
        false
    }

    fn move_selection(&mut self, step: isize, cx: &mut Context<Self>) {
        if self.tree_rows.is_empty() {
            return;
        }
        let last = self.tree_rows.len() as isize - 1;
        let index = match self.selected_index() {
            Some(index) => (index as isize + step).clamp(0, last),
            None if step > 0 => 0,
            None => last,
        };
        self.tree_selection = Some(self.tree_rows[index as usize].path.clone());
        cx.notify();
    }

    fn collapse_selected(&mut self, cx: &mut Context<Self>) {
        let Some(index) = self.selected_index() else {
            return;
        };
        let row = self.tree_rows[index].clone();
        if row.is_dir && row.expanded {
            if let Some(tree) = &mut self.tree {
                tree.collapse(&row.path);
            }
            self.tree_changed();
        } else if row.depth > 0 {
            self.tree_selection = row.path.parent().map(Path::to_path_buf);
        }
        cx.notify();
    }

    fn expand_selected(&mut self, cx: &mut Context<Self>) {
        let Some(index) = self.selected_index() else {
            return;
        };
        let row = self.tree_rows[index].clone();
        if !row.is_dir {
            return;
        }
        if row.expanded {
            if let Some(child) = self.tree_rows.get(index + 1)
                && child.depth > row.depth
            {
                self.tree_selection = Some(child.path.clone());
            }
        } else if let Some(tree) = &mut self.tree {
            tree.expand(&row.path);
            self.tree_changed();
        }
        cx.notify();
    }

    fn open_selected(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(index) = self.selected_index() else {
            return;
        };
        let row = self.tree_rows[index].clone();
        if row.is_dir {
            if let Some(tree) = &mut self.tree {
                tree.toggle(&row.path);
            }
            self.tree_changed();
        } else {
            self.open_file(&row.path, window, cx);
        }
        cx.notify();
    }

    // -----------------------------------------------------------------
    // Creating and renaming

    pub(super) fn start_tree_edit(
        &mut self,
        kind: TreeEditKind,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(tree) = &mut self.tree else {
            return;
        };
        self.layout
            .show_side_view(crate::layout::SideView::Explorer);
        let (value, selected) = match &kind {
            TreeEditKind::NewFile { folder } | TreeEditKind::NewFolder { folder } => {
                tree.expand(folder);
                (String::new(), 0..0)
            }
            TreeEditKind::Rename { path } => {
                let name = path
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_default();
                // Select the name without its extension, as Finder does.
                let stem = match name.rfind('.') {
                    Some(dot) if dot > 0 && !path.is_dir() => dot,
                    _ => name.len(),
                };
                (name, 0..stem)
            }
        };
        self.tree_changed();
        let input = cx.new(|cx| InputState::new(window, cx).default_value(value));
        input.update(cx, |input, cx| {
            input.set_selected_range(selected, cx);
            input.focus(window, cx);
        });
        let subscription = cx.subscribe_in(
            &input,
            window,
            |this, _, event: &InputEvent, window, cx| match event {
                InputEvent::PressEnter { .. } => this.finish_tree_edit(true, window, cx),
                InputEvent::Blur => this.finish_tree_edit(true, window, cx),
                _ => {}
            },
        );
        self.tree_edit = Some(TreeEdit {
            kind,
            input,
            _subscription: subscription,
        });
        cx.notify();
    }

    /// Ends the name field: creates or renames with the typed name, or
    /// leaves everything as it was.
    pub(super) fn finish_tree_edit(
        &mut self,
        commit: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(edit) = self.tree_edit.take() else {
            return;
        };
        let name = edit.input.read(cx).value().trim().to_owned();
        self.tree_focus.focus(window, cx);
        cx.notify();
        if !commit || name.is_empty() {
            return;
        }
        let result = match &edit.kind {
            TreeEditKind::NewFile { folder } => files::create_file(folder, &name),
            TreeEditKind::NewFolder { folder } => files::create_folder(folder, &name),
            TreeEditKind::Rename { path } => {
                let renamed = files::rename(path, &name);
                if let Ok(target) = &renamed {
                    self.path_moved(path, target, window, cx);
                }
                renamed
            }
        };
        match result {
            Ok(path) => {
                self.refresh_tree(cx);
                if let Some(tree) = &mut self.tree {
                    tree.reveal(&path);
                }
                self.tree_changed();
                self.tree_selection = Some(path.clone());
                if matches!(edit.kind, TreeEditKind::NewFile { .. }) {
                    self.open_file(&path, window, cx);
                }
            }
            Err(error) => {
                let message = self.strings().could_not_change(&name, &error.to_string());
                self.report(message);
            }
        }
    }

    // -----------------------------------------------------------------
    // Moving and trashing

    /// Moves a file or folder into another folder, after asking.
    fn move_path(
        &mut self,
        path: PathBuf,
        folder: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if path.parent() == Some(folder.as_path()) || folder.starts_with(&path) {
            return;
        }
        let strings = self.strings();
        let name = display_file_name(&path);
        let message = strings.move_question(&name, &display_file_name(&folder));
        let answer = window.prompt(
            PromptLevel::Info,
            &message,
            None,
            &[strings.explorer_move, strings.cancel],
            cx,
        );
        cx.spawn_in(window, async move |this, cx| {
            if answer.await != Ok(0) {
                return;
            }
            this.update_in(cx, |this, window, cx| {
                match files::move_into(&path, &folder) {
                    Ok(target) => {
                        this.path_moved(&path, &target, window, cx);
                        this.refresh_tree(cx);
                        this.tree_selection = Some(target);
                    }
                    Err(error) => {
                        let message = this.strings().could_not_change(&name, &error.to_string());
                        this.report(message);
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Moves a file or folder to the Trash, after asking.
    fn trash_path(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        if Some(&path) == self.project.root.as_ref() {
            return;
        }
        let strings = self.strings();
        let name = display_file_name(&path);
        let answer = window.prompt(
            PromptLevel::Warning,
            &strings.trash_question(&name),
            Some(strings.trash_detail),
            &[strings.explorer_trash, strings.cancel],
            cx,
        );
        cx.spawn_in(window, async move |this, cx| {
            if answer.await != Ok(0) {
                return;
            }
            this.update_in(cx, |this, _, cx| {
                if let Err(error) = files::trash(&path) {
                    let message = this.strings().could_not_change(&name, &error.to_string());
                    this.report(message);
                }
                if this.tree_selection.as_ref() == Some(&path) {
                    this.tree_selection = path.parent().map(Path::to_path_buf);
                }
                this.refresh_tree(cx);
            })
            .ok();
        })
        .detach();
    }

    /// Keeps open documents on their files after a rename or a move: a
    /// file, or every file inside a folder.
    pub(super) fn path_moved(
        &mut self,
        from: &Path,
        to: &Path,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let moved: Vec<(String, PathBuf)> = self
            .documents
            .values()
            .filter_map(|document| match &document.backing {
                Backing::File(file) => {
                    let rest = file.path().strip_prefix(from).ok()?;
                    let target = if rest.as_os_str().is_empty() {
                        to.to_path_buf()
                    } else {
                        to.join(rest)
                    };
                    Some((document.uri.clone(), target))
                }
                _ => None,
            })
            .collect();
        for (old_uri, target) in moved {
            let Some(mut document) = self.documents.remove(&old_uri) else {
                continue;
            };
            let new_uri = Uri::file(&target).to_string();
            if let Backing::File(file) = &mut document.backing {
                file.set_path(target.clone());
            }
            document.uri = new_uri.clone();
            document.language_id = language::vscode_id_for_path(&target).to_owned();
            self.notify_host("documents/closed", json!({ "uri": old_uri }));
            self.notify_host(
                "documents/opened",
                json!({
                    "uri": new_uri,
                    "languageId": document.language_id,
                    "version": document.version,
                    "text": document.synced_text,
                    "isDirty": document.modified,
                }),
            );
            self.documents.insert(new_uri.clone(), document);
            for pane in self.panes.iter_mut().filter(|pane| pane.uri == old_uri) {
                pane.uri = new_uri.clone();
            }
            if self.tree_selection.as_deref() == Some(from) {
                self.tree_selection = Some(to.to_path_buf());
            }
        }
        self.publish_editors(window, cx);
        self.refresh_title(window);
    }

    // -----------------------------------------------------------------
    // Drawing

    pub(super) fn render_explorer(&self, colors: Colors, cx: &mut Context<Self>) -> AnyElement {
        let strings = self.strings();
        let Some(root) = self.project.root.clone() else {
            return note(strings.no_folder.to_owned(), colors).into_any_element();
        };
        let toolbar_button = |id: &'static str, icon: IconName, tooltip: &'static str| {
            Button::new(id)
                .ghost()
                .xsmall()
                .icon(Icon::new(icon))
                .tooltip(tooltip)
        };
        let header = div()
            .flex_none()
            .flex()
            .items_center()
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .child(heading(&self.project.name, colors)),
            )
            .child(
                toolbar_button(
                    "explorer-new-file",
                    IconName::FilePlus,
                    strings.explorer_new_file,
                )
                .on_click(cx.listener(|this, _, window, cx| {
                    if let Some(folder) = this.target_folder() {
                        this.start_tree_edit(TreeEditKind::NewFile { folder }, window, cx);
                    }
                })),
            )
            .child(
                toolbar_button(
                    "explorer-new-folder",
                    IconName::FolderPlus,
                    strings.explorer_new_folder,
                )
                .on_click(cx.listener(|this, _, window, cx| {
                    if let Some(folder) = this.target_folder() {
                        this.start_tree_edit(TreeEditKind::NewFolder { folder }, window, cx);
                    }
                })),
            )
            .child(
                toolbar_button(
                    "explorer-refresh",
                    IconName::RefreshCw,
                    strings.explorer_refresh,
                )
                .on_click(cx.listener(|this, _, _, cx| this.refresh_tree(cx))),
            )
            .child(
                toolbar_button(
                    "explorer-collapse",
                    IconName::ChevronsDownUp,
                    strings.explorer_collapse,
                )
                .on_click(cx.listener(|this, _, _, cx| {
                    if let Some(tree) = &mut this.tree {
                        tree.collapse_all();
                    }
                    this.tree_changed();
                    cx.notify();
                })),
            );

        let count = self.tree_rows.len() + usize::from(self.new_entry_slot().is_some());
        let focus = self.tree_focus.clone();
        let list = uniform_list(
            "file-tree",
            count,
            cx.processor(|this, range: Range<usize>, window, cx| {
                this.render_tree_rows(range, window, cx)
            }),
        )
        .size_full();
        let root_for_menu = root.clone();
        div()
            .size_full()
            .flex()
            .flex_col()
            .child(header)
            .child(
                div()
                    .id("file-tree-area")
                    .flex_1()
                    .min_h_0()
                    .key_context(TREE_CONTEXT)
                    .track_focus(&self.tree_focus)
                    .on_action(cx.listener(|this, _: &SelectPrevious, _, cx| {
                        if !this.typing_a_name(cx) {
                            this.move_selection(-1, cx)
                        }
                    }))
                    .on_action(cx.listener(|this, _: &SelectNext, _, cx| {
                        if !this.typing_a_name(cx) {
                            this.move_selection(1, cx)
                        }
                    }))
                    .on_action(cx.listener(|this, _: &CollapseSelected, _, cx| {
                        if !this.typing_a_name(cx) {
                            this.collapse_selected(cx)
                        }
                    }))
                    .on_action(cx.listener(|this, _: &ExpandSelected, _, cx| {
                        if !this.typing_a_name(cx) {
                            this.expand_selected(cx)
                        }
                    }))
                    .on_action(cx.listener(|this, _: &OpenSelected, window, cx| {
                        if !this.typing_a_name(cx) {
                            this.open_selected(window, cx)
                        }
                    }))
                    .on_action(cx.listener(|this, _: &RenameSelected, window, cx| {
                        if this.typing_a_name(cx) {
                            return;
                        }
                        if let Some(path) = this.tree_selection.clone() {
                            this.start_tree_edit(TreeEditKind::Rename { path }, window, cx);
                        }
                    }))
                    .on_action(cx.listener(|this, _: &TrashSelected, window, cx| {
                        if this.typing_a_name(cx) {
                            return;
                        }
                        if let Some(path) = this.tree_selection.clone() {
                            this.trash_path(path, window, cx);
                        }
                    }))
                    .drag_over::<DraggedPath>(move |style, _, _, _| style.bg(colors.hover))
                    .on_drop(cx.listener({
                        let root = root.clone();
                        move |this, dragged: &DraggedPath, window, cx| {
                            this.move_path(dragged.path.clone(), root.clone(), window, cx)
                        }
                    }))
                    .context_menu(move |menu, _, _| {
                        menu.action_context(focus.clone())
                            .menu(
                                strings.explorer_new_file,
                                Box::new(NewFileIn {
                                    folder: root_for_menu.clone(),
                                }),
                            )
                            .menu(
                                strings.explorer_new_folder,
                                Box::new(NewFolderIn {
                                    folder: root_for_menu.clone(),
                                }),
                            )
                    })
                    .child(list),
            )
            .into_any_element()
    }

    /// Where the field for a new file or folder sits: just under its
    /// folder, as `(row index, depth)`.
    fn new_entry_slot(&self) -> Option<(usize, usize)> {
        let edit = self.tree_edit.as_ref()?;
        let folder = match &edit.kind {
            TreeEditKind::NewFile { folder } | TreeEditKind::NewFolder { folder } => folder,
            TreeEditKind::Rename { .. } => return None,
        };
        if Some(folder) == self.project.root.as_ref() {
            return Some((0, 0));
        }
        let index = self.tree_rows.iter().position(|row| &row.path == folder)?;
        Some((index + 1, self.tree_rows[index].depth + 1))
    }

    fn render_tree_rows(
        &mut self,
        range: Range<usize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let colors = Colors::new(cx);
        let tree_focused = self.tree_focus.contains_focused(window, cx);
        let slot = self.new_entry_slot();
        range
            .map(|index| match slot {
                Some((at, depth)) if index == at => self.render_new_entry(depth, colors),
                Some((at, _)) if index > at => {
                    self.render_tree_row(index - 1, colors, tree_focused, cx)
                }
                _ => self.render_tree_row(index, colors, tree_focused, cx),
            })
            .collect()
    }

    fn render_new_entry(&self, depth: usize, colors: Colors) -> AnyElement {
        let Some(edit) = &self.tree_edit else {
            return div().into_any_element();
        };
        let icon = match edit.kind {
            TreeEditKind::NewFolder { .. } => IconName::Folder,
            _ => IconName::File,
        };
        div()
            .h(px(ROW_HEIGHT))
            .pl(px(8. + depth as f32 * INDENT + 16.))
            .pr_2()
            .flex()
            .items_center()
            .gap_1p5()
            .child(Icon::new(icon).small().text_color(colors.muted))
            .child(div().flex_1().child(Input::new(&edit.input).xsmall()))
            .into_any_element()
    }

    fn render_tree_row(
        &self,
        index: usize,
        colors: Colors,
        tree_focused: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(row) = self.tree_rows.get(index).cloned() else {
            return div().into_any_element();
        };
        let strings = self.strings();
        let Row {
            path,
            name,
            depth,
            is_dir,
            expanded,
            ignored,
        } = row;
        let selected = self.tree_selection.as_ref() == Some(&path);
        let renaming = matches!(
            &self.tree_edit,
            Some(TreeEdit { kind: TreeEditKind::Rename { path: renamed }, .. }) if *renamed == path
        );
        let chevron = if is_dir {
            Icon::new(if expanded {
                IconName::ChevronDown
            } else {
                IconName::ChevronRight
            })
            .xsmall()
            .text_color(colors.faint)
            .into_any_element()
        } else {
            div().w(px(12.)).into_any_element()
        };
        let icon = if is_dir {
            Icon::new(if expanded {
                IconName::FolderOpen
            } else {
                IconName::Folder
            })
            .small()
            .text_color(colors.accent)
        } else {
            let (icon, color) = file_icon(&name);
            Icon::new(icon)
                .small()
                .text_color(color.map_or(colors.muted, |color| gpui::rgb(color).into()))
        };
        let label = if renaming {
            match &self.tree_edit {
                Some(edit) => div()
                    .flex_1()
                    .child(Input::new(&edit.input).xsmall())
                    .into_any_element(),
                None => div().into_any_element(),
            }
        } else {
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .text_color(if ignored { colors.faint } else { colors.text })
                .child(name.clone())
                .into_any_element()
        };
        let drop_folder = if is_dir {
            path.clone()
        } else {
            path.parent().map(Path::to_path_buf).unwrap_or_default()
        };
        let focus = self.tree_focus.clone();
        let menu_path = path.clone();
        let menu_folder = drop_folder.clone();
        div()
            .id(("tree-row", index))
            .h(px(ROW_HEIGHT))
            .w_full()
            .pl(px(8. + depth as f32 * INDENT))
            .pr_2()
            .flex()
            .items_center()
            .gap_1p5()
            .text_sm()
            .rounded(px(6.))
            .when(selected, |row| {
                row.bg(if tree_focused {
                    colors.selected
                } else {
                    colors.hover
                })
            })
            .when(!selected, |row| row.hover(|row| row.bg(colors.hover)))
            .on_click(cx.listener(move |this, event: &ClickEvent, window, cx| {
                this.click_tree_row(index, event, window, cx)
            }))
            .on_drag(
                DraggedPath {
                    path: path.clone(),
                    name: name.clone(),
                },
                |dragged, _, _, cx| cx.new(|_| dragged.clone()),
            )
            .drag_over::<DraggedPath>(move |style, _, _, _| style.bg(colors.selected))
            .on_drop(cx.listener(move |this, dragged: &DraggedPath, window, cx| {
                this.move_path(dragged.path.clone(), drop_folder.clone(), window, cx)
            }))
            .context_menu(move |menu, _, _| {
                let menu = menu.action_context(focus.clone());
                let menu = if is_dir {
                    menu.menu(
                        strings.explorer_new_file,
                        Box::new(NewFileIn {
                            folder: menu_folder.clone(),
                        }),
                    )
                    .menu(
                        strings.explorer_new_folder,
                        Box::new(NewFolderIn {
                            folder: menu_folder.clone(),
                        }),
                    )
                    .separator()
                } else {
                    menu
                };
                menu.menu(
                    strings.explorer_reveal,
                    Box::new(RevealInFinder {
                        path: menu_path.clone(),
                    }),
                )
                .separator()
                .menu(
                    strings.explorer_copy_path,
                    Box::new(CopyPath {
                        path: menu_path.clone(),
                        relative: false,
                    }),
                )
                .menu(
                    strings.explorer_copy_relative,
                    Box::new(CopyPath {
                        path: menu_path.clone(),
                        relative: true,
                    }),
                )
                .separator()
                .menu(
                    strings.explorer_rename,
                    Box::new(RenamePath {
                        path: menu_path.clone(),
                    }),
                )
                .menu(
                    strings.explorer_trash,
                    Box::new(TrashPath {
                        path: menu_path.clone(),
                    }),
                )
            })
            .child(chevron)
            .child(icon)
            .child(label)
            .into_any_element()
    }

    /// The handlers of the Explorer's actions, for the window's root.
    pub(super) fn explorer_actions(&self, root: Div, cx: &mut Context<Self>) -> Div {
        root.on_action(cx.listener(|this, action: &NewFileIn, window, cx| {
            this.start_tree_edit(
                TreeEditKind::NewFile {
                    folder: action.folder.clone(),
                },
                window,
                cx,
            )
        }))
        .on_action(cx.listener(|this, action: &NewFolderIn, window, cx| {
            this.start_tree_edit(
                TreeEditKind::NewFolder {
                    folder: action.folder.clone(),
                },
                window,
                cx,
            )
        }))
        .on_action(cx.listener(|this, action: &RenamePath, window, cx| {
            this.start_tree_edit(
                TreeEditKind::Rename {
                    path: action.path.clone(),
                },
                window,
                cx,
            )
        }))
        .on_action(cx.listener(|this, action: &TrashPath, window, cx| {
            this.trash_path(action.path.clone(), window, cx)
        }))
        .on_action(cx.listener(|_, action: &RevealInFinder, _, cx| cx.reveal_path(&action.path)))
        .on_action(cx.listener(|this, action: &CopyPath, _, cx| {
            let text = match (&this.project.root, action.relative) {
                (Some(root), true) => action
                    .path
                    .strip_prefix(root)
                    .unwrap_or(&action.path)
                    .display()
                    .to_string(),
                _ => action.path.display().to_string(),
            };
            cx.write_to_clipboard(ClipboardItem::new_string(text));
        }))
        .on_action(cx.listener(|this, _: &NewFileHere, window, cx| {
            if let Some(folder) = this.target_folder() {
                this.start_tree_edit(TreeEditKind::NewFile { folder }, window, cx);
            }
        }))
        .on_action(cx.listener(|this, _: &NewFolderHere, window, cx| {
            if let Some(folder) = this.target_folder() {
                this.start_tree_edit(TreeEditKind::NewFolder { folder }, window, cx);
            }
        }))
        .on_action(cx.listener(|this, _: &Refresh, _, cx| this.refresh_tree(cx)))
        .on_action(cx.listener(|this, _: &CollapseAll, _, cx| {
            if let Some(tree) = &mut this.tree {
                tree.collapse_all();
            }
            this.tree_changed();
            cx.notify();
        }))
    }
}

fn display_file_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

#[cfg(test)]
mod tests {
    use std::fs;

    use gpui::{TestAppContext, VisualTestContext};

    use super::*;

    fn set_up(cx: &mut TestAppContext) {
        cx.update(|cx| {
            gpui_component::init(cx);
            language::register_grammars();
            crate::workbench_window::init(cx);
        });
    }

    /// A project with `src/app.ts` open, in English.
    fn project_window(
        cx: &mut TestAppContext,
    ) -> (
        tempfile::TempDir,
        PathBuf,
        Entity<Workbench>,
        &mut VisualTestContext,
    ) {
        set_up(cx);
        let folder = tempfile::tempdir().unwrap();
        let base = fs::canonicalize(folder.path()).unwrap();
        let root = base.join("Hospitality");
        fs::create_dir_all(root.join("src/components")).unwrap();
        fs::write(root.join("src/app.ts"), "export {};\n").unwrap();
        fs::write(root.join("src/components/card.tsx"), "").unwrap();
        fs::write(root.join("README.md"), "").unwrap();
        let settings_file = base.join("settings.json");
        fs::write(&settings_file, r#"{ "keel.language": "en" }"#).unwrap();
        let settings = Settings::load(&settings_file, None);
        let opening = Opening::new(Some(root.clone()), Some(root.join("src/app.ts")));
        let (view, cx) =
            cx.add_window_view(|window, cx| Workbench::new(opening, settings, None, window, cx));
        (folder, root, view, cx)
    }

    fn shown(view: &Entity<Workbench>, cx: &mut VisualTestContext) -> Vec<String> {
        view.read_with(cx, |view, _| {
            view.tree_rows
                .iter()
                .map(|row| format!("{}{}", "  ".repeat(row.depth), row.name))
                .collect()
        })
    }

    fn selected(view: &Entity<Workbench>, cx: &mut VisualTestContext) -> Option<String> {
        view.read_with(cx, |view, _| {
            view.tree_selection
                .as_ref()
                .and_then(|path| path.file_name())
                .map(|name| name.to_string_lossy().into_owned())
        })
    }

    #[gpui::test]
    fn the_open_file_is_revealed_and_the_keys_move_around(cx: &mut TestAppContext) {
        let (_folder, _root, view, cx) = project_window(cx);
        assert_eq!(
            shown(&view, cx),
            ["src", "  components", "  app.ts", "README.md"]
        );
        assert_eq!(selected(&view, cx).as_deref(), Some("app.ts"));

        view.update_in(cx, |view, window, cx| view.tree_focus.focus(window, cx));
        cx.simulate_keystrokes("up");
        assert_eq!(selected(&view, cx).as_deref(), Some("components"));
        cx.simulate_keystrokes("right");
        assert!(shown(&view, cx).contains(&"    card.tsx".to_owned()));
        cx.simulate_keystrokes("right");
        assert_eq!(selected(&view, cx).as_deref(), Some("card.tsx"));
        cx.simulate_keystrokes("left");
        assert_eq!(selected(&view, cx).as_deref(), Some("components"));
        cx.simulate_keystrokes("left");
        assert!(!shown(&view, cx).contains(&"    card.tsx".to_owned()));
        cx.simulate_keystrokes("down down");
        assert_eq!(selected(&view, cx).as_deref(), Some("README.md"));
        cx.simulate_keystrokes("cmd-down");
        let title = view.read_with(cx, |view, _| view.window_title());
        assert_eq!(title, "README.md — Hospitality");
    }

    #[gpui::test]
    fn new_files_and_folders_are_named_in_the_tree(cx: &mut TestAppContext) {
        let (_folder, root, view, cx) = project_window(cx);
        let src = root.join("src");
        view.update_in(cx, |view, window, cx| {
            view.start_tree_edit(
                TreeEditKind::NewFile {
                    folder: src.clone(),
                },
                window,
                cx,
            )
        });
        cx.simulate_input("lib/utils.ts");
        cx.simulate_keystrokes("enter");
        assert!(src.join("lib/utils.ts").is_file());
        let title = view.read_with(cx, |view, _| view.window_title());
        assert_eq!(title, "utils.ts — Hospitality");
        assert!(shown(&view, cx).contains(&"    utils.ts".to_owned()));

        // Escape leaves without creating anything.
        view.update_in(cx, |view, window, cx| {
            view.start_tree_edit(
                TreeEditKind::NewFolder {
                    folder: root.clone(),
                },
                window,
                cx,
            )
        });
        cx.simulate_input("public");
        cx.simulate_keystrokes("escape");
        assert!(!root.join("public").exists());
        assert!(view.read_with(cx, |view, _| view.tree_edit.is_none()));
    }

    #[gpui::test]
    fn renaming_and_moving_keep_open_files_open(cx: &mut TestAppContext) {
        let (_folder, root, view, cx) = project_window(cx);
        let app = root.join("src/app.ts");
        view.update_in(cx, |view, window, cx| {
            view.start_tree_edit(TreeEditKind::Rename { path: app.clone() }, window, cx)
        });
        // The name is selected without its extension.
        cx.simulate_input("main");
        cx.simulate_keystrokes("enter");
        let main = root.join("src/main.ts");
        assert!(main.is_file() && !app.exists());
        view.read_with(cx, |view, _| {
            assert_eq!(view.window_title(), "main.ts — Hospitality");
            assert!(view.documents.contains_key(&Uri::file(&main).to_string()));
        });

        // Moving the folder asks first, then keeps the file open.
        view.update_in(cx, |view, window, cx| {
            view.move_path(root.join("src"), root.join("src/components"), window, cx)
        });
        assert!(!cx.has_pending_prompt(), "a folder cannot go inside itself");
        fs::create_dir_all(root.join("lib")).unwrap();
        view.update_in(cx, |view, window, cx| {
            view.move_path(root.join("src"), root.join("lib"), window, cx)
        });
        assert!(cx.has_pending_prompt());
        cx.simulate_prompt_answer("Move");
        cx.run_until_parked();
        let moved = root.join("lib/src/main.ts");
        assert!(moved.is_file());
        view.read_with(cx, |view, _| {
            assert!(view.documents.contains_key(&Uri::file(&moved).to_string()));
        });
    }
}
