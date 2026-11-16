//! Tabs and editor groups, as in VS Code. Each group is a column of the
//! editor area with its tabs. The tab in view keeps its editor in `panes`;
//! the others keep theirs in the tab, with cursor, scroll and undo history.

use super::*;

/// A column of the editor area.
pub(super) struct Group {
    pub(super) tabs: Vec<Tab>,
    /// The tab in view.
    pub(super) active: usize,
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

/// How a file opens.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum OpenAs {
    Normal,
    Preview,
}

/// How many closed files Reopen Closed Editor remembers.
const CLOSED_LIMIT: usize = 20;

actions!(
    keel,
    [
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
        FocusGroup1,
        FocusGroup2,
        FocusGroup3
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
        let tab_size = self.tab_size();
        let soft_wrap = self.word_wrap;
        let editor = cx.new(|cx| {
            EditorState::new(window, cx)
                .language(language.highlighter_name())
                .tab_size(tab_size)
                .soft_wrap(soft_wrap)
                .default_value(text)
        });
        let editor_id = format!("editor-{}", self.next_editor);
        self.next_editor += 1;
        let watched_uri = uri.to_owned();
        let watched_id = editor_id.clone();
        let subscription = cx.subscribe_in(
            &editor,
            window,
            move |this, _, event: &InputEvent, window, cx| {
                if matches!(event, InputEvent::Change) {
                    this.sync_document_from(&watched_uri, Some(&watched_id), window, cx);
                }
            },
        );
        // The block bar and the signature follow the cursor.
        let cursor = cx.observe_in(&editor, window, |this, editor, window, cx| {
            this.update_hunk_bar(cx);
            this.signature_after_cursor_move(window, cx);
            this.sync_preview_from_editor(&editor, cx);
        });
        self.underline_editor(uri, &editor, cx);
        Some(Pane {
            editor_id,
            uri: uri.to_owned(),
            editor,
            decorations: HashMap::new(),
            gutter: None,
            hunks: Vec::new(),
            completions: Default::default(),
            _subscriptions: vec![subscription, cursor],
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
            self.panes.push(pane);
            self.groups.push(Group {
                tabs: vec![tab],
                active: 0,
            });
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
        // Extensions never knew about images.
        if removed.is_some_and(|document| {
            !matches!(
                document.backing,
                Backing::Image(_) | Backing::Diff(_) | Backing::Preview(_) | Backing::Pdf(_)
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
            self.panes.push(pane);
            self.groups.push(Group {
                tabs: vec![tab],
                active: 0,
            });
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

    /// Opens the active file again in a new group to the right.
    fn split_editor(&mut self, group: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(uri) = self.panes.get(group).map(|pane| pane.uri.clone()) else {
            return;
        };
        let Some(pane) = self.create_pane(&uri, window, cx) else {
            return;
        };
        let at = group + 1;
        self.panes.insert(at, pane);
        self.groups.insert(
            at,
            Group {
                tabs: vec![Tab {
                    uri,
                    preview: false,
                    pinned: false,
                    parked: None,
                }],
                active: 0,
            },
        );
        self.install_completions(at, cx);
        self.focus_pane(at, window, cx);
        self.publish_editors(window, cx);
        self.schedule_gutter_refresh(window, cx);
        self.refresh_title(window);
        cx.notify();
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
            self.split_editor(self.panes.len().saturating_sub(1), window, cx);
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
            TabCommand::Split => {
                self.activate_tab(group, tab, false, window, cx);
                self.split_editor(group, window, cx);
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
    pub(super) fn tab_actions(&self, root: gpui::Div, cx: &mut Context<Self>) -> gpui::Div {
        root.on_action(cx.listener(|this, _: &CloseEditor, window, cx| {
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
            let group = this.active;
            this.split_editor(group, window, cx)
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
