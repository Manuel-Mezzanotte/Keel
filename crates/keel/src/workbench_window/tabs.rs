//! Tabs and editor groups, as in VS Code. The editor area is a row of
//! columns, each a stack of groups with their tabs. The tab in view keeps
//! its editor in `panes`; the others keep theirs in the tab, with cursor,
//! scroll and undo history.

use super::*;

/// An editor group: a cell of the editor area, with its tabs. Groups of
/// the same `column` are stacked top to bottom and come one after another
/// in `Workbench::groups`, which reads the area left to right and each
/// column top to bottom, so a group's index is the same as before.
pub(super) struct Group {
    pub(super) tabs: Vec<Tab>,
    /// The tab in view.
    pub(super) active: usize,
    /// The column it is in: no order, only the same for its neighbors.
    pub(super) column: u64,
    /// The strip's scroll, and the tab it was last brought to, so a tab
    /// opened or chosen out of sight scrolls into view once.
    pub(super) scroll: gpui::ScrollHandle,
    pub(super) shown: std::cell::Cell<Option<usize>>,
}

impl Group {
    pub(super) fn with(tabs: Vec<Tab>, column: u64) -> Self {
        Self {
            tabs,
            active: 0,
            column,
            scroll: gpui::ScrollHandle::new(),
            shown: std::cell::Cell::new(None),
        }
    }
}

pub(super) struct Tab {
    pub(super) uri: String,
    /// A preview tab, in italics, gives way to the next file opened in
    /// preview. Editing the file or double-clicking the tab keeps it.
    pub(super) preview: bool,
    /// Pinned tabs come first and are left open by Close Others and Close All.
    pub(super) pinned: bool,
    /// The tab's editor while another tab is in view.
    pub(super) parked: Option<Pane>,
}

/// Where a new group goes beside another, as Split Editor Left, Right, Up
/// and Down: left and right open a column beside the group's column,
/// above and below stack it in the same column.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Side {
    Left,
    Right,
    Above,
    Below,
}

/// Where on an editor a dragged tab or file is dropped, as VS Code's drop
/// zones: near an edge it opens a group on that side, in the middle it
/// goes into that group.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum DropZone {
    Left,
    Right,
    Top,
    Bottom,
    Center,
}

impl DropZone {
    /// The zone under `position` in an editor's `bounds`: within the outer
    /// 30% of a side, the nearest side; elsewhere the middle.
    pub(super) fn at(
        bounds: gpui::Bounds<gpui::Pixels>,
        position: gpui::Point<gpui::Pixels>,
    ) -> Option<Self> {
        if !bounds.contains(&position)
            || bounds.size.width <= px(0.)
            || bounds.size.height <= px(0.)
        {
            return None;
        }
        let x = (position.x - bounds.left()) / bounds.size.width;
        let y = (position.y - bounds.top()) / bounds.size.height;
        let (distance, zone) = [
            (x, DropZone::Left),
            (1. - x, DropZone::Right),
            (y, DropZone::Top),
            (1. - y, DropZone::Bottom),
        ]
        .into_iter()
        .min_by(|a, b| a.0.total_cmp(&b.0))?;
        Some(if distance < 0.3 {
            zone
        } else {
            DropZone::Center
        })
    }

    /// The side a group opens on, or `None` in the middle.
    pub(super) fn side(self) -> Option<Side> {
        match self {
            DropZone::Left => Some(Side::Left),
            DropZone::Right => Some(Side::Right),
            DropZone::Top => Some(Side::Above),
            DropZone::Bottom => Some(Side::Below),
            DropZone::Center => None,
        }
    }
}

/// A tab being dragged to another place in its group or to another group.
#[derive(Clone)]
pub(super) struct DraggedTab {
    pub(super) group: usize,
    pub(super) index: usize,
    pub(super) name: String,
}

impl gpui::Render for DraggedTab {
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

/// How a file opens.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum OpenAs {
    Normal,
    Preview,
}

/// How many closed files Reopen Closed Editor remembers.
const CLOSED_LIMIT: usize = 20;

/// Ctrl+1…9: the tab at a position of the group in front.
#[derive(Clone, Debug, PartialEq, gpui::Action)]
#[action(namespace = keel, no_json)]
pub struct OpenEditorAtIndex {
    pub index: usize,
}

actions!(
    keel,
    [
        CloseSavedEditors,
        CloseEditor,
        CloseOtherEditors,
        CloseEditorsInGroup,
        CloseAllEditors,
        ReopenClosedEditor,
        TogglePinEditor,
        KeepEditor,
        NextEditor,
        PreviousEditor,
        SplitEditor,
        SplitEditorLeft,
        SplitEditorUp,
        SplitEditorDown,
        SplitEditorOrthogonal,
        FocusGroup1,
        FocusGroup2,
        FocusGroup3,
        FocusLeftGroup,
        FocusRightGroup,
        FocusAboveGroup,
        FocusBelowGroup
    ]
);

/// What a tab's context menu does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TabCommand {
    Close,
    CloseOthers,
    CloseToTheRight,
    CloseAll,
    TogglePin,
    Split,
    SplitDown,
    CopyPath,
    Reveal,
}

/// A tab's context menu entry.
#[derive(Clone, Debug, PartialEq, gpui::Action)]
#[action(namespace = keel, no_json)]
pub struct TabAction {
    pub group: usize,
    pub tab: usize,
    pub command: TabCommand,
}

/// Replaces `current` with `wanted` in an editor with as small an edit as
/// possible, so its cursor and undo history mostly survive.
pub(super) fn replace_text(
    editor: &Entity<EditorState>,
    wanted: &str,
    window: &mut Window,
    cx: &mut App,
) {
    editor.update(cx, |editor, cx| {
        let current = editor.value().to_string();
        if current == wanted {
            return;
        }
        let mut prefix = current
            .bytes()
            .zip(wanted.bytes())
            .take_while(|(a, b)| a == b)
            .count();
        while !current.is_char_boundary(prefix) || !wanted.is_char_boundary(prefix) {
            prefix -= 1;
        }
        let longest = current.len().min(wanted.len()) - prefix;
        let mut suffix = current
            .bytes()
            .rev()
            .zip(wanted.bytes().rev())
            .take(longest)
            .take_while(|(a, b)| a == b)
            .count();
        while !current.is_char_boundary(current.len() - suffix)
            || !wanted.is_char_boundary(wanted.len() - suffix)
        {
            suffix -= 1;
        }
        let range = prefix..current.len() - suffix;
        let inserted = wanted[prefix..wanted.len() - suffix].to_owned();
        let selection = editor.selected_range();
        let reversed = editor.is_selection_reversed();
        editor.set_selected_range(range.clone(), cx);
        editor.replace(inserted.clone(), window, cx);
        let edits = [(range, inserted)];
        let start = map_offset(selection.start, &edits);
        let end = map_offset(selection.end, &edits);
        editor.set_selected_range_with_direction(start..end, reversed, cx);
    });
}

impl Workbench {
    /// Whether the caret glides, as VS Code's
    /// `editor.cursorSmoothCaretAnimation`: on in Keel unless turned off,
    /// and never with Reduce Motion.
    pub(super) fn smooth_caret(&self) -> bool {
        self.look.motion && self.settings.string("editor.cursorSmoothCaretAnimation") != Some("off")
    }

    /// A new editor for a loaded document, known to extensions by its id.
    pub(super) fn create_pane(
        &mut self,
        uri: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Pane> {
        let document = self.documents.get(uri)?;
        let language = Language::for_vscode_id(&document.language_id);
        let text = document.synced_text.clone();
        let tab_size = self.indent_of(uri);
        let soft_wrap = self.word_wrap;
        let whitespace = self.settings.string("editor.renderWhitespace") == Some("all");
        let editor = cx.new(|cx| {
            EditorState::new(window, cx)
                .language(language.highlighter_name())
                .tab_size(tab_size)
                // Cmd+F opens Keel's own find bar.
                .searchable(false)
                .soft_wrap(soft_wrap)
                .show_whitespaces(whitespace)
                .default_value(text)
        });
        let smooth = self.smooth_caret();
        editor.update(cx, |editor, _| editor.set_smooth_caret(smooth));
        let editor_id = format!("editor-{}", self.next_editor);
        self.next_editor += 1;
        let watched_uri = uri.to_owned();
        let watched_id = editor_id.clone();
        let subscription = cx.subscribe_in(
            &editor,
            window,
            move |this, editor, event: &InputEvent, window, cx| {
                if matches!(event, InputEvent::Change) {
                    this.note_edit(&watched_uri, editor, cx);
                    this.mirror_tag(&watched_uri, &watched_id, editor, window, cx);
                    this.redecorate(&watched_id, editor, true, cx);
                    this.sync_document_from(&watched_uri, Some(&watched_id), window, cx);
                }
            },
        );
        // The block bar and the signature follow the cursor.
        let cursor_uri = uri.to_owned();
        let cursor_id = editor_id.clone();
        let cursor = cx.observe_in(&editor, window, move |this, editor, window, cx| {
            this.note_cursor(&cursor_uri, &editor, cx);
            this.redecorate(&cursor_id, &editor, true, cx);
            this.update_hunk_bar(cx);
            this.signature_after_cursor_move(window, cx);
            this.sync_preview_from_editor(&editor, cx);
        });
        // «On focus change» saves as soon as the editor loses the keyboard,
        // to the terminal, the sidebar or Claude as much as to another tab.
        let focus = editor.read(cx).focus_handle(cx);
        let left = cx.on_focus_out(&focus, window, |this, _, window, cx| {
            this.auto_save_on(false, window, cx)
        });
        self.underline_editor(uri, &editor, cx);
        self.start_decor(&editor_id, &editor, cx);
        // VS Code has both on unless the settings say otherwise.
        let on = |key: &str| {
            self.settings
                .get(key)
                .and_then(Value::as_bool)
                .unwrap_or(true)
        };
        let font_size = self.font_size;
        let sticky = on("editor.stickyScroll.enabled").then(|| {
            let editor = editor.clone();
            cx.new(|cx| editor_overlays::StickyScroll::new(editor, font_size, cx))
        });
        let minimap = on("editor.minimap.enabled").then(|| {
            let editor = editor.clone();
            cx.new(|cx| editor_overlays::Minimap::new(editor, cx))
        });
        Some(Pane {
            editor_id,
            uri: uri.to_owned(),
            editor,
            decorations: HashMap::new(),
            gutter: None,
            hunks: Vec::new(),
            completions: Default::default(),
            sticky,
            minimap,
            _subscriptions: vec![subscription, cursor, left],
        })
    }

    fn preview_enabled(&self) -> bool {
        self.settings
            .get("workbench.editor.enablePreview")
            .and_then(Value::as_bool)
            .unwrap_or(true)
    }

    /// The tab showing `uri`: in `preferred` group if it has one, otherwise
    /// in any group.
    pub(super) fn find_tab(&self, uri: &str, preferred: usize) -> Option<(usize, usize)> {
        let in_group = |group: usize| {
            self.groups
                .get(group)?
                .tabs
                .iter()
                .position(|tab| tab.uri == uri)
                .map(|tab| (group, tab))
        };
        in_group(preferred).or_else(|| (0..self.groups.len()).find_map(in_group))
    }

    /// How many tabs, in every group, show `uri`.
    fn views_of(&self, uri: &str) -> usize {
        self.groups
            .iter()
            .flat_map(|group| &group.tabs)
            .filter(|tab| tab.uri == uri)
            .count()
    }

    /// Shows a loaded document in group `column`, creating the group when
    /// `column` is one past the last. A tab already showing it comes
    /// forward. Returns the editor id extensions know it by.
    pub(super) fn show_tab(
        &mut self,
        uri: &str,
        column: usize,
        open_as: OpenAs,
        focus: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> String {
        if let Some((group, tab)) = self.find_tab(uri, column) {
            if open_as == OpenAs::Normal {
                self.groups[group].tabs[tab].preview = false;
            }
            self.activate_tab(group, tab, focus, window, cx);
            return self.panes[group].editor_id.clone();
        }
        let Some(pane) = self.create_pane(uri, window, cx) else {
            return String::new();
        };
        let editor_id = pane.editor_id.clone();
        let preview = open_as == OpenAs::Preview && self.preview_enabled();
        let tab = Tab {
            uri: uri.to_owned(),
            preview,
            pinned: false,
            parked: None,
        };
        let mut replaced = None;
        let group_index = if column < self.groups.len() {
            let group = &mut self.groups[column];
            let shown = std::mem::replace(&mut self.panes[column], pane);
            group.tabs[group.active].parked = Some(shown);
            // A new preview takes the place of the group's preview tab.
            let at = match preview
                .then(|| group.tabs.iter().position(|tab| tab.preview))
                .flatten()
            {
                Some(index) => {
                    replaced = Some(group.tabs.remove(index).uri);
                    index
                }
                None => group.active + 1,
            };
            group.tabs.insert(at, tab);
            group.active = at;
            column
        } else {
            let column = self.new_column();
            self.panes.push(pane);
            self.groups.push(Group::with(vec![tab], column));
            self.panes.len() - 1
        };
        if let Some(uri) = replaced {
            self.forget_if_unused(&uri);
        }
        self.install_completions(group_index, cx);
        if focus || self.panes.len() == 1 {
            self.focus_pane(group_index, window, cx);
        }
        self.publish_editors(window, cx);
        self.schedule_gutter_refresh(window, cx);
        self.refresh_title(window);
        cx.notify();
        editor_id
    }

    /// Brings a tab into view in its group.
    pub(super) fn activate_tab(
        &mut self,
        group: usize,
        tab: usize,
        focus: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if group >= self.groups.len() || tab >= self.groups[group].tabs.len() {
            return;
        }
        if self.groups[group].active != tab {
            let uri = self.groups[group].tabs[tab].uri.clone();
            let pane = match self.groups[group].tabs[tab].parked.take() {
                Some(pane) => Some(pane),
                None => self.create_pane(&uri, window, cx),
            };
            let Some(pane) = pane else {
                return;
            };
            let shown = std::mem::replace(&mut self.panes[group], pane);
            let current = self.groups[group].active;
            self.groups[group].tabs[current].parked = Some(shown);
            self.groups[group].active = tab;
            // The document may have changed while the tab was hidden.
            let synced = self
                .documents
                .get(&uri)
                .map(|document| document.synced_text.clone());
            if let Some(text) = synced {
                let editor = self.panes[group].editor.clone();
                replace_text(&editor, &text, window, cx);
            }
            self.install_completions(group, cx);
        }
        if focus {
            self.focus_pane(group, window, cx);
        }
        self.publish_editors(window, cx);
        self.schedule_gutter_refresh(window, cx);
        self.refresh_title(window);
        cx.notify();
    }

    /// Closes a tab, asking first when it is the last view of a file with
    /// unsaved changes.
    pub(super) fn close_tab(
        &mut self,
        group: usize,
        tab: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(uri) = self
            .groups
            .get(group)
            .and_then(|group| group.tabs.get(tab))
            .map(|tab| tab.uri.clone())
        else {
            return;
        };
        let unsaved = self.documents.get(&uri).is_some_and(|document| {
            document.modified && matches!(document.backing, Backing::File(_) | Backing::Untitled)
        });
        if !unsaved || self.views_of(&uri) > 1 {
            self.close_tab_now(group, tab, window, cx);
            return;
        }
        let strings = self.strings();
        let message = strings.save_changes(&self.display_name(&self.documents[&uri]));
        let answer = window.prompt(
            PromptLevel::Warning,
            &message,
            Some(strings.changes_lost),
            &[strings.save, strings.dont_save, strings.cancel],
            cx,
        );
        cx.spawn_in(window, async move |this, cx| {
            let Ok(choice) = answer.await else {
                return;
            };
            this.update_in(cx, |this, window, cx| {
                // The tab may have moved while the question was open.
                let Some((group, tab)) = this.find_tab(&uri, group) else {
                    return;
                };
                match choice {
                    0 => {
                        this.save_document(&uri, None, window, cx);
                        let saved = this
                            .documents
                            .get(&uri)
                            .is_some_and(|document| !document.modified);
                        if saved {
                            this.close_tab_now(group, tab, window, cx);
                        }
                    }
                    1 => this.close_tab_now(group, tab, window, cx),
                    _ => {}
                }
            })
            .ok();
        })
        .detach();
    }

    /// Closes a tab without asking.
    pub(super) fn close_tab_now(
        &mut self,
        group: usize,
        tab: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if group >= self.groups.len() || tab >= self.groups[group].tabs.len() {
            return;
        }
        let was_active = self.groups[group].active == tab;
        // The closed tab's editor takes its decorations with it.
        let closed_editor = if was_active {
            self.panes.get(group).map(|pane| pane.editor_id.clone())
        } else {
            self.groups[group].tabs[tab]
                .parked
                .as_ref()
                .map(|pane| pane.editor_id.clone())
        };
        if let Some(id) = closed_editor {
            self.forget_decor(&id);
        }
        let closed = self.groups[group].tabs.remove(tab);
        if self.groups[group].tabs.is_empty() {
            self.groups.remove(group);
            self.panes.remove(group);
            if self.panes.is_empty() {
                // With no tab left, the window shows the welcome screen.
                self.active = 0;
                self.focus_handle.focus(window, cx);
            } else {
                self.active = self.active.min(self.panes.len() - 1);
                self.focus_pane(self.active, window, cx);
            }
        } else if was_active {
            let next = tab.min(self.groups[group].tabs.len() - 1);
            let uri = self.groups[group].tabs[next].uri.clone();
            let pane = match self.groups[group].tabs[next].parked.take() {
                Some(pane) => Some(pane),
                None => self.create_pane(&uri, window, cx),
            };
            if let Some(pane) = pane {
                self.panes[group] = pane;
            }
            self.groups[group].active = next;
            self.install_completions(group, cx);
            if self.active == group {
                self.focus_pane(group, window, cx);
            }
        } else if tab < self.groups[group].active {
            self.groups[group].active -= 1;
        }
        self.remember_closed(&closed.uri);
        self.forget_if_unused(&closed.uri);
        self.publish_editors(window, cx);
        self.schedule_gutter_refresh(window, cx);
        self.refresh_title(window);
        cx.notify();
    }

    /// Closes several tabs of a group. Pinned tabs stay, and so do files
    /// with unsaved changes, so nothing is lost without asking.
    fn close_tabs_where(
        &mut self,
        group: usize,
        close: impl Fn(usize, &Tab) -> bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(tabs) = self.groups.get(group).map(|group| &group.tabs) else {
            return;
        };
        let doomed: Vec<String> = tabs
            .iter()
            .enumerate()
            .filter(|(index, tab)| {
                let unsaved = self
                    .documents
                    .get(&tab.uri)
                    .is_some_and(|document| document.modified);
                !tab.pinned && !unsaved && close(*index, tab)
            })
            .map(|(_, tab)| tab.uri.clone())
            .collect();
        for uri in doomed {
            if let Some(tab) = self
                .groups
                .get(group)
                .and_then(|group| group.tabs.iter().position(|tab| tab.uri == uri))
            {
                self.close_tab_now(group, tab, window, cx);
            }
        }
    }

    fn remember_closed(&mut self, uri: &str) {
        if !uri.starts_with("file:") {
            return;
        }
        self.closed.retain(|known| known != uri);
        self.closed.push(uri.to_owned());
        if self.closed.len() > CLOSED_LIMIT {
            self.closed.remove(0);
        }
    }

    /// Lets go of a document no tab shows any more.
    fn forget_if_unused(&mut self, uri: &str) {
        if self.views_of(uri) > 0 {
            return;
        }
        let removed = self.documents.remove(uri);
        // A proposal of Claude Code closed without an answer is rejected.
        if uri.starts_with(super::claude_diff::PROPOSED_SCHEME) {
            self.claude_diff_closed(uri);
        }
        // Extensions never knew about images.
        if removed.is_some_and(|document| {
            !matches!(
                document.backing,
                Backing::Image(_)
                    | Backing::Diff(_)
                    | Backing::Preview(_)
                    | Backing::Pdf(_)
                    | Backing::Graph(_)
                    | Backing::ExtensionDetails(_)
            )
        }) {
            self.notify_host("documents/closed", json!({ "uri": uri }));
        }
        self.forget_backup(uri);
    }

    /// Adds a tab at the end of group `column`, or a new group when
    /// `column` is one past the last, without looking for the file in other
    /// tabs. Restoring a session uses it.
    pub(super) fn push_tab(
        &mut self,
        column: usize,
        uri: &str,
        pinned: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(pane) = self.create_pane(uri, window, cx) else {
            return;
        };
        let tab = Tab {
            uri: uri.to_owned(),
            preview: false,
            pinned,
            parked: None,
        };
        if column < self.groups.len() {
            let group = &mut self.groups[column];
            let shown = std::mem::replace(&mut self.panes[column], pane);
            group.tabs[group.active].parked = Some(shown);
            group.tabs.push(tab);
            group.active = group.tabs.len() - 1;
        } else {
            let column = self.new_column();
            self.panes.push(pane);
            self.groups.push(Group::with(vec![tab], column));
        }
        let column = column.min(self.groups.len() - 1);
        self.install_completions(column, cx);
    }

    fn reopen_closed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        while let Some(uri) = self.closed.pop() {
            if self.find_tab(&uri, self.active).is_some() {
                continue;
            }
            if let Some(path) = Uri::parse(&uri).and_then(|uri| uri.file_path())
                && path.exists()
            {
                self.open_file(&path, window, cx);
                return;
            }
        }
    }

    /// Points every tab and editor of `old` at `new`, after a rename, a
    /// move or the first save of an untitled file.
    pub(super) fn retarget_uri(&mut self, old: &str, new: &str) {
        for pane in self.panes.iter_mut().filter(|pane| pane.uri == old) {
            pane.uri = new.to_owned();
        }
        for tab in self
            .groups
            .iter_mut()
            .flat_map(|group| &mut group.tabs)
            .filter(|tab| tab.uri == old)
        {
            tab.uri = new.to_owned();
            if let Some(pane) = &mut tab.parked {
                pane.uri = new.to_owned();
            }
        }
        if let Some(index) = self.closed.iter().position(|uri| uri == old) {
            self.closed[index] = new.to_owned();
        }
    }

    /// A column no group is in yet.
    pub(super) fn new_column(&self) -> u64 {
        self.groups
            .iter()
            .map(|group| group.column + 1)
            .max()
            .unwrap_or(0)
    }

    /// Where a group placed on `side` of group `group` goes: its index
    /// among the groups and its column.
    pub(super) fn beside(&self, group: usize, side: Side) -> (usize, u64) {
        let Some(column) = self.groups.get(group).map(|group| group.column) else {
            return (self.groups.len(), self.new_column());
        };
        let same = |index: &usize| self.groups[*index].column == column;
        let first = (0..group).rev().take_while(same).last().unwrap_or(group);
        let last = (group..self.groups.len())
            .take_while(same)
            .last()
            .unwrap_or(group);
        match side {
            Side::Left => (first, self.new_column()),
            Side::Right => (last + 1, self.new_column()),
            Side::Above => (group, column),
            Side::Below => (group + 1, column),
        }
    }

    /// Opens group `group`'s file again in a new group on `side` of it.
    pub(super) fn split_editor(
        &mut self,
        group: usize,
        side: Side,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(uri) = self.panes.get(group).map(|pane| pane.uri.clone()) else {
            return;
        };
        let Some(pane) = self.create_pane(&uri, window, cx) else {
            return;
        };
        let (at, column) = self.beside(group, side);
        let at = at.min(self.panes.len());
        self.panes.insert(at, pane);
        self.groups.insert(
            at,
            Group::with(
                vec![Tab {
                    uri,
                    preview: false,
                    pinned: false,
                    parked: None,
                }],
                column,
            ),
        );
        self.install_completions(at, cx);
        self.focus_pane(at, window, cx);
        self.publish_editors(window, cx);
        self.schedule_gutter_refresh(window, cx);
        self.refresh_title(window);
        cx.notify();
    }

    /// Moves a tab to position `to_index` of group `to_group`, as dragging
    /// it does: within its group the order changes; to another group the
    /// tab goes there, with its editor, cursor and undo history, and a group
    /// left empty closes. Pinned tabs stay before the others.
    pub(super) fn move_tab(
        &mut self,
        from_group: usize,
        from_index: usize,
        to_group: usize,
        to_index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(moved) = self
            .groups
            .get(from_group)
            .and_then(|group| group.tabs.get(from_index))
        else {
            return;
        };
        if to_group >= self.groups.len() {
            return;
        }
        let uri = moved.uri.clone();
        let pinned = moved.pinned;
        // Where pinned and unpinned tabs may go.
        let place = |tabs: &[Tab], at: usize| {
            let pinned_count = tabs.iter().filter(|tab| tab.pinned).count();
            if pinned {
                at.min(pinned_count)
            } else {
                at.max(pinned_count)
            }
        };
        if from_group == to_group {
            let group = &mut self.groups[from_group];
            let active_uri = group.tabs[group.active].uri.clone();
            let tab = group.tabs.remove(from_index);
            let at = place(&group.tabs, to_index.min(group.tabs.len()));
            group.tabs.insert(at, tab);
            group.active = group
                .tabs
                .iter()
                .position(|tab| tab.uri == active_uri)
                .unwrap_or(0);
        } else {
            // Already open there: show that one and close this one.
            if let Some(existing) = self.groups[to_group]
                .tabs
                .iter()
                .position(|tab| tab.uri == uri)
            {
                let source_closes = self.groups[from_group].tabs.len() == 1;
                self.close_tab_now(from_group, from_index, window, cx);
                let to_group = if source_closes && from_group < to_group {
                    to_group - 1
                } else {
                    to_group
                };
                self.activate_tab(to_group, existing, true, window, cx);
                self.session_changed(cx);
                return;
            }
            // The moved tab's editor: in view, it is the group's pane.
            let only_tab = self.groups[from_group].tabs.len() == 1;
            let in_view = self.groups[from_group].active == from_index;
            let mut tab;
            let pane;
            let mut to_group = to_group;
            if only_tab {
                tab = self.groups.remove(from_group).tabs.remove(0);
                pane = self.panes.remove(from_group);
                if to_group > from_group {
                    to_group -= 1;
                }
            } else {
                if in_view {
                    let neighbor = if from_index + 1 < self.groups[from_group].tabs.len() {
                        from_index + 1
                    } else {
                        from_index - 1
                    };
                    self.activate_tab(from_group, neighbor, false, window, cx);
                }
                let group = &mut self.groups[from_group];
                tab = group.tabs.remove(from_index);
                if from_index < group.active {
                    group.active -= 1;
                }
                let Some(parked) = tab.parked.take() else {
                    return;
                };
                pane = parked;
            }
            tab.parked = None;
            tab.preview = false;
            let group = &mut self.groups[to_group];
            let shown = std::mem::replace(&mut self.panes[to_group], pane);
            group.tabs[group.active].parked = Some(shown);
            let at = place(&group.tabs, to_index.min(group.tabs.len()));
            group.tabs.insert(at, tab);
            group.active = at;
            self.install_completions(to_group, cx);
            self.active = self.active.min(self.panes.len() - 1);
            self.focus_pane(to_group, window, cx);
        }
        self.publish_editors(window, cx);
        self.refresh_title(window);
        self.session_changed(cx);
        cx.notify();
    }

    /// A tab dropped on an editor: beside it in a new group, or into it.
    pub(super) fn drop_tab(
        &mut self,
        from_group: usize,
        from_index: usize,
        on_group: usize,
        zone: DropZone,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match zone.side() {
            None if from_group == on_group => {}
            None => {
                let end = self
                    .groups
                    .get(on_group)
                    .map_or(0, |group| group.tabs.len());
                self.move_tab(from_group, from_index, on_group, end, window, cx);
            }
            Some(side) => {
                self.move_tab_to_new_group(from_group, from_index, on_group, side, window, cx)
            }
        }
    }

    /// Takes a tab, with its editor, cursor and undo history, into a new
    /// group on `side` of group `on_group`. A group's only tab dropped on
    /// its own editor is split instead, as VS Code does.
    pub(super) fn move_tab_to_new_group(
        &mut self,
        from_group: usize,
        from_index: usize,
        on_group: usize,
        side: Side,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(count) = self.groups.get(from_group).map(|group| group.tabs.len()) else {
            return;
        };
        if from_index >= count || on_group >= self.groups.len() {
            return;
        }
        if count == 1 {
            if on_group == from_group {
                self.split_editor(from_group, side, window, cx);
                self.session_changed(cx);
                return;
            }
            // The group goes where it was dropped.
            let mut group = self.groups.remove(from_group);
            let pane = self.panes.remove(from_group);
            let on_group = if on_group > from_group {
                on_group - 1
            } else {
                on_group
            };
            let (at, column) = self.beside(on_group, side);
            group.column = column;
            self.groups.insert(at, group);
            self.panes.insert(at, pane);
            self.settle_new_group(at, window, cx);
            return;
        }
        let (at, column) = self.beside(on_group, side);
        if self.groups[from_group].active == from_index {
            let neighbor = if from_index + 1 < count {
                from_index + 1
            } else {
                from_index - 1
            };
            self.activate_tab(from_group, neighbor, false, window, cx);
        }
        let group = &mut self.groups[from_group];
        let mut tab = group.tabs.remove(from_index);
        if from_index < group.active {
            group.active -= 1;
        }
        let Some(pane) = tab.parked.take() else {
            return;
        };
        tab.preview = false;
        self.groups.insert(at, Group::with(vec![tab], column));
        self.panes.insert(at, pane);
        self.settle_new_group(at, window, cx);
    }

    /// A group just placed at `at`: its editor ready and in front.
    fn settle_new_group(&mut self, at: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.install_completions(at, cx);
        self.focus_pane(at, window, cx);
        self.publish_editors(window, cx);
        self.schedule_gutter_refresh(window, cx);
        self.refresh_title(window);
        self.session_changed(cx);
        cx.notify();
    }

    /// Files dropped on an editor: into its group, or into a new group
    /// beside it.
    pub(super) fn drop_paths(
        &mut self,
        paths: Vec<PathBuf>,
        on_group: usize,
        zone: DropZone,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // The first file opens the new group; the others go into it.
        let mut new_group = zone.side().map(|side| self.beside(on_group, side));
        let mut column = on_group;
        for path in paths.iter().filter(|path| path.is_file()) {
            let Ok(uri) = self.load_document(path, window, cx) else {
                continue;
            };
            if let Some((at, group_column)) = new_group {
                let Some(pane) = self.create_pane(&uri, window, cx) else {
                    continue;
                };
                column = at.min(self.groups.len());
                self.groups.insert(
                    column,
                    Group::with(
                        vec![Tab {
                            uri,
                            preview: false,
                            pinned: false,
                            parked: None,
                        }],
                        group_column,
                    ),
                );
                self.panes.insert(column, pane);
                self.settle_new_group(column, window, cx);
                new_group = None;
            } else {
                self.show_tab(&uri, column, OpenAs::Normal, true, window, cx);
            }
        }
        self.session_changed(cx);
    }

    fn toggle_pin(&mut self, group: usize, tab: usize, cx: &mut Context<Self>) {
        let Some(tabs) = self.groups.get_mut(group).map(|group| &mut group.tabs) else {
            return;
        };
        if tab >= tabs.len() {
            return;
        }
        let mut moved = tabs.remove(tab);
        moved.pinned = !moved.pinned;
        moved.preview = false;
        // Pinned tabs sit together at the start of the group.
        let pinned = tabs.iter().take_while(|tab| tab.pinned).count();
        tabs.insert(pinned, moved);
        let group = &mut self.groups[group];
        group.active = group
            .tabs
            .iter()
            .position(|tab| tab.parked.is_none())
            .unwrap_or(0);
        cx.notify();
    }

    fn step_tab(&mut self, step: isize, window: &mut Window, cx: &mut Context<Self>) {
        let group = self.active;
        let Some(count) = self.groups.get(group).map(|group| group.tabs.len()) else {
            return;
        };
        let current = self.groups[group].active as isize;
        let next = (current + step).rem_euclid(count as isize) as usize;
        self.activate_tab(group, next, true, window, cx);
    }

    pub(super) fn focus_group(
        &mut self,
        group: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if group < self.panes.len() {
            self.focus_pane(group, window, cx);
            self.publish_editors(window, cx);
            self.refresh_title(window);
            cx.notify();
        } else if group == self.panes.len() {
            self.split_editor(self.panes.len().saturating_sub(1), Side::Right, window, cx);
        }
    }

    /// The group on `side` of the one in front: above or below in its
    /// column, or in the next column at the same height.
    pub(super) fn group_toward(&self, group: usize, side: Side) -> Option<usize> {
        let column = self.groups.get(group)?.column;
        match side {
            Side::Above => group
                .checked_sub(1)
                .filter(|above| self.groups[*above].column == column),
            Side::Below => Some(group + 1).filter(|below| {
                self.groups
                    .get(*below)
                    .is_some_and(|below| below.column == column)
            }),
            Side::Left | Side::Right => {
                let columns = self.columns();
                let here = columns.iter().position(|cells| cells.contains(&group))?;
                let there = if side == Side::Left {
                    columns.get(here.checked_sub(1)?)?
                } else {
                    columns.get(here + 1)?
                };
                // The same share of the height: the middle of this cell.
                let cells = &columns[here];
                let row = cells.iter().position(|cell| *cell == group)?;
                let middle = (row as f32 + 0.5) / cells.len() as f32;
                let at = ((middle * there.len() as f32) as usize).min(there.len() - 1);
                Some(there[at])
            }
        }
    }

    /// Where what opens to the side goes: the group on the right of group
    /// `group` at the same height, or a new one after the last column.
    pub(super) fn side_group(&self, group: usize) -> usize {
        self.group_toward(group, Side::Right)
            .unwrap_or(self.groups.len())
    }

    /// The groups of each column, left to right.
    pub(super) fn columns(&self) -> Vec<Vec<usize>> {
        let mut columns: Vec<Vec<usize>> = Vec::new();
        let mut last = None;
        for (index, group) in self.groups.iter().enumerate() {
            match columns.last_mut() {
                Some(cells) if last == Some(group.column) => cells.push(index),
                _ => columns.push(vec![index]),
            }
            last = Some(group.column);
        }
        columns
    }

    fn focus_group_toward(&mut self, side: Side, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(group) = self.group_toward(self.active, side) {
            self.focus_group(group, window, cx);
        }
    }

    pub(super) fn run_tab_command(
        &mut self,
        action: &TabAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (group, tab) = (action.group, action.tab);
        let Some(uri) = self
            .groups
            .get(group)
            .and_then(|group| group.tabs.get(tab))
            .map(|tab| tab.uri.clone())
        else {
            return;
        };
        match action.command {
            TabCommand::Close => self.close_tab(group, tab, window, cx),
            TabCommand::CloseOthers => {
                self.close_tabs_where(group, |_, other| other.uri != uri, window, cx)
            }
            TabCommand::CloseToTheRight => {
                self.close_tabs_where(group, |index, _| index > tab, window, cx)
            }
            TabCommand::CloseAll => self.close_tabs_where(group, |_, _| true, window, cx),
            TabCommand::TogglePin => self.toggle_pin(group, tab, cx),
            TabCommand::Split | TabCommand::SplitDown => {
                self.activate_tab(group, tab, false, window, cx);
                let side = if action.command == TabCommand::Split {
                    Side::Right
                } else {
                    Side::Below
                };
                self.split_editor(group, side, window, cx);
            }
            TabCommand::CopyPath | TabCommand::Reveal => {
                let Some(path) = Uri::parse(&uri).and_then(|uri| uri.file_path()) else {
                    return;
                };
                if action.command == TabCommand::Reveal {
                    cx.reveal_path(&path);
                } else {
                    cx.write_to_clipboard(gpui::ClipboardItem::new_string(
                        path.display().to_string(),
                    ));
                }
            }
        }
    }

    /// The handlers of the tab commands, for the window's root.
    /// Close Saved (Cmd+K U): every tab whose file has no unsaved change,
    /// pinned ones aside.
    fn close_saved(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        for group in (0..self.groups.len()).rev() {
            for tab in (0..self.groups.get(group).map_or(0, |group| group.tabs.len())).rev() {
                let Some(entry) = self.groups.get(group).and_then(|group| group.tabs.get(tab))
                else {
                    continue;
                };
                let modified = self
                    .documents
                    .get(&entry.uri)
                    .is_some_and(|document| document.modified);
                if !entry.pinned && !modified {
                    self.close_tab_now(group, tab, window, cx);
                }
            }
        }
        cx.notify();
    }

    pub(super) fn tab_actions(&self, root: gpui::Div, cx: &mut Context<Self>) -> gpui::Div {
        root.on_action(
            cx.listener(|this, _: &CloseSavedEditors, window, cx| this.close_saved(window, cx)),
        )
        .on_action(cx.listener(|this, action: &OpenEditorAtIndex, window, cx| {
            let group = this.active.min(this.groups.len().saturating_sub(1));
            let count = this.groups.get(group).map_or(0, |group| group.tabs.len());
            // 9 is the last tab, as in VS Code and Safari.
            let tab = if action.index >= 8 {
                count.saturating_sub(1)
            } else {
                action.index
            };
            if tab < count {
                this.activate_tab(group, tab, true, window, cx);
            }
        }))
        .on_action(cx.listener(|this, _: &CloseEditor, window, cx| {
            if this.panes.is_empty() {
                // With no editor left, Cmd+W closes the window, as in VS Code.
                window.dispatch_action(Box::new(CloseWindow), cx);
                return;
            }
            let group = this.active;
            let tab = this.groups[group].active;
            this.close_tab(group, tab, window, cx);
        }))
        .on_action(cx.listener(|this, _: &CloseOtherEditors, window, cx| {
            let group = this.active;
            if let Some(uri) = this.panes.get(group).map(|pane| pane.uri.clone()) {
                this.close_tabs_where(group, |_, tab| tab.uri != uri, window, cx);
            }
        }))
        .on_action(cx.listener(|this, _: &CloseEditorsInGroup, window, cx| {
            let group = this.active;
            this.close_tabs_where(group, |_, _| true, window, cx);
        }))
        .on_action(cx.listener(|this, _: &CloseAllEditors, window, cx| {
            for group in (0..this.groups.len()).rev() {
                this.close_tabs_where(group, |_, _| true, window, cx);
            }
        }))
        .on_action(
            cx.listener(|this, _: &ReopenClosedEditor, window, cx| this.reopen_closed(window, cx)),
        )
        .on_action(cx.listener(|this, _: &TogglePinEditor, _, cx| {
            let group = this.active;
            if let Some(tab) = this.groups.get(group).map(|group| group.active) {
                this.toggle_pin(group, tab, cx);
            }
        }))
        .on_action(cx.listener(|this, _: &KeepEditor, _, cx| {
            let group = this.active;
            if let Some(group) = this.groups.get_mut(group) {
                let active = group.active;
                group.tabs[active].preview = false;
                cx.notify();
            }
        }))
        .on_action(cx.listener(|this, _: &NextEditor, window, cx| this.step_tab(1, window, cx)))
        .on_action(
            cx.listener(|this, _: &PreviousEditor, window, cx| this.step_tab(-1, window, cx)),
        )
        .on_action(cx.listener(|this, _: &SplitEditor, window, cx| {
            this.split_editor(this.active, Side::Right, window, cx)
        }))
        .on_action(cx.listener(|this, _: &SplitEditorLeft, window, cx| {
            this.split_editor(this.active, Side::Left, window, cx)
        }))
        .on_action(cx.listener(|this, _: &SplitEditorUp, window, cx| {
            this.split_editor(this.active, Side::Above, window, cx)
        }))
        // Orthogonal to the side-by-side direction, which is to the right.
        .on_action(cx.listener(|this, _: &SplitEditorDown, window, cx| {
            this.split_editor(this.active, Side::Below, window, cx)
        }))
        .on_action(cx.listener(|this, _: &SplitEditorOrthogonal, window, cx| {
            this.split_editor(this.active, Side::Below, window, cx)
        }))
        .on_action(cx.listener(|this, _: &FocusLeftGroup, window, cx| {
            this.focus_group_toward(Side::Left, window, cx)
        }))
        .on_action(cx.listener(|this, _: &FocusRightGroup, window, cx| {
            this.focus_group_toward(Side::Right, window, cx)
        }))
        .on_action(cx.listener(|this, _: &FocusAboveGroup, window, cx| {
            this.focus_group_toward(Side::Above, window, cx)
        }))
        .on_action(cx.listener(|this, _: &FocusBelowGroup, window, cx| {
            this.focus_group_toward(Side::Below, window, cx)
        }))
        .on_action(cx.listener(|this, _: &FocusGroup1, window, cx| this.focus_group(0, window, cx)))
        .on_action(cx.listener(|this, _: &FocusGroup2, window, cx| this.focus_group(1, window, cx)))
        .on_action(cx.listener(|this, _: &FocusGroup3, window, cx| this.focus_group(2, window, cx)))
        .on_action(cx.listener(|this, action: &TabAction, window, cx| {
            this.run_tab_command(action, window, cx)
        }))
    }
}

/// The paths of the files open in a window, for tests.
#[cfg(test)]
pub(super) fn open_paths(workbench: &Workbench) -> Vec<Vec<(String, bool, bool)>> {
    workbench
        .groups
        .iter()
        .map(|group| {
            group
                .tabs
                .iter()
                .map(|tab| {
                    let name = Uri::parse(&tab.uri)
                        .map(|uri| uri.file_name().to_owned())
                        .unwrap_or_else(|| tab.uri.clone());
                    (name, tab.preview, tab.pinned)
                })
                .collect()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;

    use gpui::{TestAppContext, VisualTestContext};

    use super::*;

    fn set_up(cx: &mut TestAppContext) {
        cx.update(|cx| {
            gpui_component::init(cx);
            language::register_grammars();
            crate::workbench_window::init(cx);
        });
    }

    /// A project with `a.ts` to `d.ts`, in English, nothing open.
    fn project(
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
        let root = base.join("Keel");
        fs::create_dir_all(&root).unwrap();
        for name in ["a", "b", "c", "d"] {
            fs::write(
                root.join(format!("{name}.ts")),
                format!("let {name} = 1;\n"),
            )
            .unwrap();
        }
        let settings_file = base.join("settings.json");
        fs::write(&settings_file, r#"{ "keel.language": "en" }"#).unwrap();
        let settings = Settings::load(&settings_file, None);
        let opening = Opening::new(Some(root.clone()), None);
        let (view, cx) =
            cx.add_window_view(|window, cx| Workbench::new(opening, settings, None, window, cx));
        (folder, root, view, cx)
    }

    fn open(
        view: &Entity<Workbench>,
        cx: &mut VisualTestContext,
        path: &std::path::Path,
        open_as: OpenAs,
    ) {
        view.update_in(cx, |view, window, cx| {
            view.open_file_as(path, open_as, window, cx)
        });
    }

    fn tabs(
        view: &Entity<Workbench>,
        cx: &mut VisualTestContext,
    ) -> Vec<Vec<(String, bool, bool)>> {
        view.read_with(cx, |view, _| open_paths(view))
    }

    fn tab(name: &str, preview: bool, pinned: bool) -> (String, bool, bool) {
        (name.to_owned(), preview, pinned)
    }

    fn active_text(view: &Entity<Workbench>, cx: &mut VisualTestContext) -> String {
        view.read_with(cx, |view, cx| {
            view.panes[view.active].editor.read(cx).value().to_string()
        })
    }

    #[test]
    fn the_edges_of_an_editor_are_its_drop_zones() {
        let bounds = gpui::Bounds::new(
            gpui::point(px(100.), px(0.)),
            gpui::size(px(300.), px(200.)),
        );
        let at = |x: f32, y: f32| DropZone::at(bounds, gpui::point(px(x), px(y)));
        assert_eq!(at(120., 100.), Some(DropZone::Left));
        assert_eq!(at(250., 100.), Some(DropZone::Center));
        assert_eq!(at(380., 100.), Some(DropZone::Right));
        assert_eq!(at(250., 20.), Some(DropZone::Top));
        assert_eq!(at(250., 190.), Some(DropZone::Bottom));
        // In a corner, the nearer edge.
        assert_eq!(at(105., 40.), Some(DropZone::Left));
        assert_eq!(at(20., 100.), None);
    }

    #[gpui::test]
    fn groups_stack_in_columns_and_the_keys_go_between_them(cx: &mut TestAppContext) {
        let (_folder, root, view, cx) = project(cx);
        let names = |cx: &mut VisualTestContext| -> Vec<Vec<String>> {
            tabs(&view, cx)
                .into_iter()
                .map(|group| group.into_iter().map(|(name, _, _)| name).collect())
                .collect()
        };
        let columns = |cx: &mut VisualTestContext| view.read_with(cx, |view, _| view.columns());
        let active = |cx: &mut VisualTestContext| view.read_with(cx, |view, _| view.active);
        let split = |cx: &mut VisualTestContext, group: usize, side: Side| {
            view.update_in(cx, |view, window, cx| {
                view.split_editor(group, side, window, cx)
            })
        };
        open(&view, cx, &root.join("a.ts"), OpenAs::Normal);
        split(cx, 0, Side::Below);
        assert_eq!(columns(cx), [vec![0, 1]]);
        open(&view, cx, &root.join("b.ts"), OpenAs::Normal);
        // Right of the lower group: a column the whole height.
        split(cx, 1, Side::Right);
        assert_eq!(columns(cx), [vec![0, 1], vec![2]]);
        split(cx, 2, Side::Above);
        assert_eq!(columns(cx), [vec![0, 1], vec![2, 3]]);
        assert_eq!(
            names(cx),
            [
                vec!["a.ts"],
                vec!["a.ts", "b.ts"],
                vec!["b.ts"],
                vec!["b.ts"]
            ]
        );
        assert_eq!(active(cx), 2);

        // Cmd+K and an arrow: the group that way, at the same height.
        cx.simulate_keystrokes("cmd-k cmd-down");
        assert_eq!(active(cx), 3);
        cx.simulate_keystrokes("cmd-k cmd-left");
        assert_eq!(active(cx), 1);
        cx.simulate_keystrokes("cmd-k cmd-up");
        assert_eq!(active(cx), 0);
        cx.simulate_keystrokes("cmd-k cmd-up");
        assert_eq!(active(cx), 0);
        cx.simulate_keystrokes("cmd-k cmd-right");
        assert_eq!(active(cx), 2);

        // A tab dropped on the top of an editor: a group above it.
        view.update_in(cx, |view, window, cx| {
            view.drop_tab(1, 1, 0, DropZone::Top, window, cx)
        });
        assert_eq!(columns(cx), [vec![0, 1, 2], vec![3, 4]]);
        assert_eq!(names(cx)[0], ["b.ts"]);
        // A group's only tab on the bottom of another: the group moves.
        view.update_in(cx, |view, window, cx| {
            view.drop_tab(0, 0, 4, DropZone::Bottom, window, cx)
        });
        assert_eq!(columns(cx), [vec![0, 1], vec![2, 3, 4]]);
        assert_eq!(names(cx)[4], ["b.ts"]);
        // Closing a stacked group leaves the others where they are.
        view.update_in(cx, |view, window, cx| view.close_tab(3, 0, window, cx));
        assert_eq!(columns(cx), [vec![0, 1], vec![2, 3]]);
        // Open to the side from the lower group on the left: the group on
        // its right, not the one below it.
        view.update_in(cx, |view, window, cx| {
            view.focus_group(1, window, cx);
            view.open_to_side(&root.join("c.ts"), window, cx);
        });
        assert_eq!(active(cx), 3);
        assert_eq!(names(cx)[3], ["b.ts", "c.ts"]);
        view.update_in(cx, |view, window, cx| view.close_tab(3, 1, window, cx));
        // Cmd+K Cmd+\ splits down.
        view.update_in(cx, |view, window, cx| view.focus_group(0, window, cx));
        cx.simulate_keystrokes("cmd-k cmd-\\");
        assert_eq!(columns(cx), [vec![0, 1, 2], vec![3, 4]]);
        assert_eq!(active(cx), 1);
    }

    #[gpui::test]
    fn a_tab_dropped_beside_an_editor_splits_it(cx: &mut TestAppContext) {
        let (_folder, root, view, cx) = project(cx);
        open(&view, cx, &root.join("a.ts"), OpenAs::Normal);
        open(&view, cx, &root.join("b.ts"), OpenAs::Normal);
        // Typed in b, so its editor must travel with it.
        cx.simulate_input("x");
        let names = |cx: &mut VisualTestContext| -> Vec<Vec<String>> {
            tabs(&view, cx)
                .into_iter()
                .map(|group| group.into_iter().map(|(name, _, _)| name).collect())
                .collect()
        };
        // b on the right of its own editor: a group of its own.
        view.update_in(cx, |view, window, cx| {
            view.drop_tab(0, 1, 0, DropZone::Right, window, cx)
        });
        assert_eq!(names(cx), [vec!["a.ts"], vec!["b.ts"]]);
        assert_eq!(active_text(&view, cx), "xlet b = 1;\n");
        // b, alone, on the left of a: the groups change places.
        view.update_in(cx, |view, window, cx| {
            view.drop_tab(1, 0, 0, DropZone::Left, window, cx)
        });
        assert_eq!(names(cx), [vec!["b.ts"], vec!["a.ts"]]);
        // A group's only tab beside itself: split, as VS Code does.
        view.update_in(cx, |view, window, cx| {
            view.drop_tab(0, 0, 0, DropZone::Right, window, cx)
        });
        assert_eq!(names(cx), [vec!["b.ts"], vec!["b.ts"], vec!["a.ts"]]);
        // In the middle of an editor: into its group.
        view.update_in(cx, |view, window, cx| {
            view.drop_tab(1, 0, 2, DropZone::Center, window, cx)
        });
        assert_eq!(names(cx), [vec!["b.ts"], vec!["a.ts", "b.ts"]]);
        // A file from the Explorer on the right: a new group with it.
        view.update_in(cx, |view, window, cx| {
            view.drop_paths(vec![root.join("c.ts")], 1, DropZone::Right, window, cx)
        });
        assert_eq!(
            names(cx),
            [vec!["b.ts"], vec!["a.ts", "b.ts"], vec!["c.ts"]]
        );
    }

    #[gpui::test]
    fn a_preview_tab_gives_way_until_it_is_kept(cx: &mut TestAppContext) {
        let (_folder, root, view, cx) = project(cx);
        open(&view, cx, &root.join("a.ts"), OpenAs::Normal);
        open(&view, cx, &root.join("b.ts"), OpenAs::Preview);
        open(&view, cx, &root.join("c.ts"), OpenAs::Preview);
        assert_eq!(
            tabs(&view, cx),
            [vec![tab("a.ts", false, false), tab("c.ts", true, false)]]
        );
        // b.ts went away with its tab.
        let b = Uri::file(&root.join("b.ts")).to_string();
        assert!(!view.read_with(cx, |view, _| view.documents.contains_key(&b)));

        // Typing keeps the preview tab.
        cx.simulate_input("x");
        open(&view, cx, &root.join("b.ts"), OpenAs::Preview);
        assert_eq!(
            tabs(&view, cx),
            [vec![
                tab("a.ts", false, false),
                tab("c.ts", false, false),
                tab("b.ts", true, false)
            ]]
        );
    }

    #[gpui::test]
    fn tabs_keep_their_text_and_undo_while_hidden(cx: &mut TestAppContext) {
        let (_folder, root, view, cx) = project(cx);
        open(&view, cx, &root.join("a.ts"), OpenAs::Normal);
        cx.simulate_input("x");
        open(&view, cx, &root.join("b.ts"), OpenAs::Normal);
        assert_eq!(active_text(&view, cx), "let b = 1;\n");

        cx.simulate_keystrokes("cmd-alt-left");
        assert_eq!(active_text(&view, cx), "xlet a = 1;\n");
        cx.simulate_keystrokes("cmd-z");
        assert_eq!(active_text(&view, cx), "let a = 1;\n");
        cx.simulate_keystrokes("cmd-alt-right");
        assert_eq!(active_text(&view, cx), "let b = 1;\n");
    }

    #[gpui::test]
    fn closing_asks_about_unsaved_changes_and_cmd_shift_t_reopens(cx: &mut TestAppContext) {
        let (_folder, root, view, cx) = project(cx);
        open(&view, cx, &root.join("a.ts"), OpenAs::Normal);
        open(&view, cx, &root.join("b.ts"), OpenAs::Normal);
        cx.simulate_input("x");

        cx.simulate_keystrokes("cmd-w");
        assert!(cx.has_pending_prompt());
        cx.simulate_prompt_answer("Don't Save");
        cx.run_until_parked();
        assert_eq!(tabs(&view, cx), [vec![tab("a.ts", false, false)]]);
        assert_eq!(
            fs::read_to_string(root.join("b.ts")).unwrap(),
            "let b = 1;\n"
        );

        cx.simulate_keystrokes("cmd-shift-t");
        assert_eq!(
            tabs(&view, cx),
            [vec![tab("a.ts", false, false), tab("b.ts", false, false)]]
        );
        assert_eq!(active_text(&view, cx), "let b = 1;\n");

        // Closing the last tab brings back the welcome screen.
        cx.simulate_keystrokes("cmd-w cmd-w");
        assert!(view.read_with(cx, |view, _| view.panes.is_empty()
            && view.groups.is_empty()));
    }

    #[gpui::test]
    fn close_others_leaves_pinned_and_unsaved_tabs(cx: &mut TestAppContext) {
        let (_folder, root, view, cx) = project(cx);
        for name in ["a", "b", "c", "d"] {
            open(&view, cx, &root.join(format!("{name}.ts")), OpenAs::Normal);
            if name == "c" {
                cx.simulate_input("x");
            }
        }
        view.update_in(cx, |view, window, cx| {
            view.run_tab_command(
                &TabAction {
                    group: 0,
                    tab: 1,
                    command: TabCommand::TogglePin,
                },
                window,
                cx,
            )
        });
        assert_eq!(tabs(&view, cx)[0][0], tab("b.ts", false, true));

        cx.simulate_keystrokes("cmd-alt-t");
        assert_eq!(
            tabs(&view, cx),
            [vec![
                tab("b.ts", false, true),
                tab("c.ts", false, false),
                tab("d.ts", false, false)
            ]]
        );
    }

    #[gpui::test]
    fn dragged_tabs_change_place_and_group(cx: &mut TestAppContext) {
        let (_folder, base, view, cx) = project(cx);
        for name in ["a.ts", "b.ts", "c.ts"] {
            open(&view, cx, &base.join(name), OpenAs::Normal);
        }
        let names = |view: &Entity<Workbench>, cx: &mut VisualTestContext| {
            tabs(view, cx)
                .into_iter()
                .map(|group| {
                    group
                        .into_iter()
                        .map(|(name, _, _)| name)
                        .collect::<Vec<_>>()
                })
                .collect::<Vec<_>>()
        };
        assert_eq!(names(&view, cx), [["a.ts", "b.ts", "c.ts"]]);
        // c.ts to the front: it stays in view.
        view.update_in(cx, |view, window, cx| view.move_tab(0, 2, 0, 0, window, cx));
        assert_eq!(names(&view, cx), [["c.ts", "a.ts", "b.ts"]]);
        assert_eq!(active_text(&view, cx), "let c = 1;\n");
        // Split, then drag a.ts into the new group, with its text.
        view.update_in(cx, |view, window, cx| {
            view.split_editor(0, Side::Right, window, cx)
        });
        assert_eq!(
            names(&view, cx),
            [vec!["c.ts", "a.ts", "b.ts"], vec!["c.ts"]]
        );
        view.update_in(cx, |view, window, cx| view.move_tab(0, 1, 1, 1, window, cx));
        assert_eq!(
            names(&view, cx),
            [vec!["c.ts", "b.ts"], vec!["c.ts", "a.ts"]]
        );
        assert_eq!(active_text(&view, cx), "let a = 1;\n");
        // The last tab of a group leaves: the group closes.
        view.update_in(cx, |view, window, cx| view.move_tab(1, 1, 0, 2, window, cx));
        view.update_in(cx, |view, window, cx| view.move_tab(1, 0, 0, 0, window, cx));
        assert_eq!(names(&view, cx), [vec!["c.ts", "b.ts", "a.ts"]]);
    }

    #[gpui::test]
    fn a_split_editor_shows_the_same_file_and_keeps_both_in_step(cx: &mut TestAppContext) {
        let (_folder, root, view, cx) = project(cx);
        open(&view, cx, &root.join("a.ts"), OpenAs::Normal);
        cx.simulate_keystrokes("cmd-\\");
        assert_eq!(tabs(&view, cx).len(), 2);
        assert_eq!(view.read_with(cx, |view, _| view.active), 1);

        cx.simulate_input("y");
        let texts = view.read_with(cx, |view, cx| {
            view.panes
                .iter()
                .map(|pane| pane.editor.read(cx).value().to_string())
                .collect::<Vec<_>>()
        });
        assert_eq!(texts, ["ylet a = 1;\n", "ylet a = 1;\n"]);

        // Closing one side leaves the file open in the other.
        view.update_in(cx, |view, window, cx| view.close_tab_now(1, 0, window, cx));
        assert_eq!(tabs(&view, cx), [vec![tab("a.ts", false, false)]]);
        assert_eq!(active_text(&view, cx), "ylet a = 1;\n");
        cmd_1_focuses_the_first_group(&view, cx);
    }

    fn cmd_1_focuses_the_first_group(view: &Entity<Workbench>, cx: &mut VisualTestContext) {
        cx.simulate_keystrokes("cmd-2");
        assert_eq!(view.read_with(cx, |view, _| view.panes.len()), 2);
        cx.simulate_keystrokes("cmd-1");
        assert_eq!(view.read_with(cx, |view, _| view.active), 0);
    }

    #[gpui::test]
    fn small_edits_replace_only_what_changed(cx: &mut TestAppContext) {
        set_up(cx);
        let (editor, cx) = cx.add_window_view(|window, cx| {
            EditorState::new(window, cx).default_value("perché caffè 1;\nfine")
        });
        editor.update_in(cx, |editor, _, cx| editor.set_selected_range(0..0, cx));
        cx.update(|window, cx| replace_text(&editor, "perché caffè 22;\nfine", window, cx));
        cx.update(|window, cx| replace_text(&editor, "perché tè 22;\nfine", window, cx));
        let (text, cursor) = editor.read_with(cx, |editor, _| {
            (editor.value().to_string(), editor.selected_range())
        });
        assert_eq!(text, "perché tè 22;\nfine");
        // The cursor before the edits stays where it was.
        assert_eq!(cursor, 0..0);
    }
}
