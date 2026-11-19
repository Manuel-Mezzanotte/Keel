//! The File menu's commands beyond Save: Save As, Save All, Revert File,
//! Auto Save with its check mark, and Close Folder, as in VS Code.

use gpui::Div;

use super::*;

actions!(
    keel,
    [
        SaveAs,
        SaveAll,
        RevertFile,
        ToggleAutoSave,
        CloseFolder,
        UndoLastCommit
    ]
);

/// `files.autoSave` when the settings say nothing: after a pause, as you
/// had it in Cursor. Off in the tests, which save when they mean to.
pub(super) const AUTO_SAVE_DEFAULT: &str = if cfg!(test) { "off" } else { "afterDelay" };

impl Workbench {
    pub(super) fn file_command_actions(&self, root: Div, cx: &mut Context<Self>) -> Div {
        root.on_action(cx.listener(|this, _: &SaveAs, window, cx| this.save_active_as(window, cx)))
            .on_action(cx.listener(|this, _: &SaveAll, window, cx| this.save_all(window, cx)))
            .on_action(
                cx.listener(|this, _: &RevertFile, window, cx| this.revert_active(window, cx)),
            )
            .on_action(
                cx.listener(|this, _: &ToggleAutoSave, window, cx| {
                    this.toggle_auto_save(window, cx)
                }),
            )
            .on_action(
                cx.listener(|this, _: &CloseFolder, window, cx| this.close_folder(window, cx)),
            )
            .on_action(
                cx.listener(|this, _: &UndoLastCommit, window, cx| {
                    this.undo_last_commit(window, cx)
                }),
            )
    }

    /// Saves the file in view under another name; the tab follows the new
    /// file, and the old one stays as it was on disk.
    fn save_active_as(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(uri) = self.panes.get(self.active).map(|pane| pane.uri.clone()) else {
            return;
        };
        let Some(document) = self.documents.get(&uri) else {
            return;
        };
        if !matches!(document.backing, Backing::File(_) | Backing::Untitled) {
            return;
        }
        let text = document.synced_text.clone();
        self.save_as(uri, text, None, window, cx);
    }

    /// Saves every file with changes; an untitled one asks for its name.
    pub(super) fn save_all(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let uris: Vec<String> = self
            .documents
            .values()
            .filter(|document| document.modified)
            .filter(|document| matches!(document.backing, Backing::File(_) | Backing::Untitled))
            .map(|document| document.uri.clone())
            .collect();
        for uri in uris {
            self.save_document(&uri, None, window, cx);
        }
    }

    /// Drops the changes of the file in view and reads it again from disk.
    fn revert_active(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(uri) = self.panes.get(self.active).map(|pane| pane.uri.clone()) else {
            return;
        };
        self.revert_document(&uri, window, cx);
    }

    pub(super) fn auto_save_is_on(&self) -> bool {
        self.settings
            .string("files.autoSave")
            .unwrap_or(AUTO_SAVE_DEFAULT)
            != "off"
    }

    /// File → Auto Save: on after a pause, or off.
    fn toggle_auto_save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let value = if self.auto_save_is_on() {
            "off"
        } else {
            "afterDelay"
        };
        self.change_setting("files.autoSave", Some(Value::from(value)), window, cx);
        crate::menus::set_auto_save(self.auto_save_is_on(), cx);
        if value != "off" {
            self.save_all_quietly(window, cx);
        }
    }

    /// Closes the project and leaves an empty window, after asking about
    /// unsaved changes.
    fn close_folder(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.project.root.is_none() {
            return;
        }
        if self.confirm(AfterConfirm::CloseFolder, window, cx) {
            self.finish(AfterConfirm::CloseFolder, window, cx);
        }
    }
}
