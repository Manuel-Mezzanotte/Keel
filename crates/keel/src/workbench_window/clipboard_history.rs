//! The clipboard's history, as JetBrains' «Paste from History»: what was
//! copied or cut in Keel, and what came from other apps, the latest
//! first. Cmd+Alt+V lists it and pastes the one chosen.

use std::collections::VecDeque;
use std::path::Path;

use gpui::{Div, Global, KeystrokeEvent};

use super::*;
use crate::quick_pick::{self, PickItem};

actions!(keel, [PasteFromHistory]);

/// How many copies the history keeps.
const KEPT: usize = 30;
/// Copies longer than this are not kept: a whole file is not a snippet.
const LONGEST: usize = 100_000;

#[derive(Default)]
struct ClipboardHistory(VecDeque<String>);

impl Global for ClipboardHistory {}

/// Starts keeping what is copied in any window.
pub(crate) fn init(cx: &mut App) {
    cx.set_global(ClipboardHistory::default());
    cx.observe_keystrokes(|event: &KeystrokeEvent, _, cx| {
        let copied = event.action.as_ref().is_some_and(|action| {
            matches!(
                action.name(),
                "input::Copy" | "input::Cut" | "terminal::Copy"
            )
        });
        if copied {
            remember_clipboard(cx);
        }
    })
    .detach();
}

/// Keeps what the clipboard holds now, if it is text.
pub(crate) fn remember_clipboard(cx: &mut App) {
    if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
        remember(text, cx);
    }
}

fn remember(text: String, cx: &mut App) {
    if text.trim().is_empty() || text.len() > LONGEST || !cx.has_global::<ClipboardHistory>() {
        return;
    }
    let history = &mut cx.global_mut::<ClipboardHistory>().0;
    history.retain(|kept| *kept != text);
    history.push_front(text);
    history.truncate(KEPT);
}

/// The history, the latest first.
pub(crate) fn history(cx: &App) -> Vec<String> {
    cx.try_global::<ClipboardHistory>()
        .map(|history| history.0.iter().cloned().collect())
        .unwrap_or_default()
}

impl Workbench {
    pub(super) fn clipboard_actions(&self, root: Div, cx: &mut Context<Self>) -> Div {
        root.on_action(
            cx.listener(|this, _: &PasteFromHistory, window, cx| {
                this.paste_from_history(window, cx)
            }),
        )
    }

    /// Cmd+Alt+V: the history, and the one chosen pasted where the cursor
    /// is; it goes back on top of the clipboard too.
    fn paste_from_history(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let strings = self.strings();
        let Some(editor) = self.panes.get(self.active).map(|pane| pane.editor.clone()) else {
            return;
        };
        remember_clipboard(cx);
        let entries = history(cx);
        if entries.is_empty() {
            self.status_message = Some(strings.clipboard_history_empty.to_owned());
            cx.notify();
            return;
        }
        let items = entries
            .iter()
            .map(|text| {
                let first = text
                    .lines()
                    .find(|line| !line.trim().is_empty())
                    .unwrap_or_default();
                let shown: String = first.trim().chars().take(120).collect();
                let mut item = PickItem::new(shown);
                let lines = text.lines().count();
                if lines > 1 {
                    item.detail = Some(fill_lines(strings.clipboard_lines, lines));
                }
                item.keywords = vec![text.chars().take(2000).collect()];
                item
            })
            .collect();
        let chosen = quick_pick::show(
            Some(strings.clipboard_history_title.to_owned()),
            None,
            items,
            window,
            cx,
        );
        cx.spawn_in(window, async move |_, cx| {
            let Ok(Some(picked)) = chosen.await else {
                return;
            };
            let text = entries[picked].clone();
            cx.update(|window, cx| {
                cx.write_to_clipboard(gpui::ClipboardItem::new_string(text.clone()));
                remember(text.clone(), cx);
                editor.update(cx, |editor, cx| {
                    editor.focus(window, cx);
                    editor.replace(text, window, cx);
                });
            })
            .ok();
        })
        .detach();
    }
}

/// The image on the clipboard, when it holds one and no text.
pub(super) fn clipboard_image(cx: &App) -> Option<gpui::Image> {
    let item = cx.read_from_clipboard()?;
    if item.text().is_some_and(|text| !text.trim().is_empty()) {
        return None;
    }
    item.entries().iter().find_map(|entry| match entry {
        gpui::ClipboardEntry::Image(image) => Some(image.clone()),
        _ => None,
    })
}

/// A name for a new file in `folder` that no file has: `image.png`, then
/// `image-1.png`, `image-2.png`…
pub(super) fn free_image_name(folder: &Path, extension: &str) -> PathBuf {
    let first = folder.join(format!("image.{extension}"));
    if !first.exists() {
        return first;
    }
    (1..)
        .map(|count| folder.join(format!("image-{count}.{extension}")))
        .find(|path| !path.exists())
        .expect("a free name")
}

impl Workbench {
    /// Cmd+V in Markdown with an image on the clipboard: the image is saved
    /// beside the file and its link written at the cursor. Returns whether
    /// it did, so the editor does not paste too.
    pub(super) fn paste_image_into_markdown(
        &mut self,
        pane: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(pane) = self.panes.get(pane) else {
            return false;
        };
        let Some(document) = self.documents.get(&pane.uri) else {
            return false;
        };
        if document.language_id != "markdown" {
            return false;
        }
        let Some(folder) = document
            .disk_path()
            .and_then(Path::parent)
            .map(Path::to_path_buf)
        else {
            return false;
        };
        let Some(image) = clipboard_image(cx) else {
            return false;
        };
        let path = free_image_name(&folder, image.format.extension());
        if let Err(error) = std::fs::write(&path, &image.bytes) {
            self.status_message = Some(error.to_string());
            cx.notify();
            return true;
        }
        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().replace(' ', "%20"))
            .unwrap_or_default();
        let editor = pane.editor.clone();
        editor.update(cx, |editor, cx| {
            editor.replace(format!("![]({name})"), window, cx)
        });
        self.refresh_tree(cx);
        true
    }

    /// Cmd+V in Claude's box with an image on the clipboard: it is saved in
    /// Keel's folder and attached to the next message.
    pub(super) fn paste_image_into_chat(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(image) = clipboard_image(cx) else {
            return false;
        };
        let Some(folder) = self
            .data_dir
            .as_ref()
            .map(|data| data.join("pasted-images"))
        else {
            return false;
        };
        if std::fs::create_dir_all(&folder).is_err() {
            return false;
        }
        let path = free_image_name(&folder, image.format.extension());
        if std::fs::write(&path, &image.bytes).is_err() {
            return false;
        }
        self.attach_to_agent(vec![path], window, cx);
        true
    }
}

fn fill_lines(text: &str, count: usize) -> String {
    text.replace("{count}", &count.to_string())
}

#[cfg(test)]
mod tests {
    use gpui::TestAppContext;

    use super::*;

    #[test]
    fn pasted_images_get_names_no_file_has() {
        let folder = tempfile::tempdir().unwrap();
        assert_eq!(
            free_image_name(folder.path(), "png"),
            folder.path().join("image.png")
        );
        std::fs::write(folder.path().join("image.png"), b"x").unwrap();
        std::fs::write(folder.path().join("image-1.png"), b"x").unwrap();
        assert_eq!(
            free_image_name(folder.path(), "png"),
            folder.path().join("image-2.png")
        );
    }

    #[gpui::test]
    fn an_image_pasted_into_markdown_is_saved_beside_it(cx: &mut TestAppContext) {
        let (folder, _path, view, cx) =
            super::super::tests::window_with_file(cx, "note.md", "# Nota\n");
        let png = b"\x89PNG\r\n\x1a\nimage".to_vec();
        cx.update(|_, cx| {
            cx.write_to_clipboard(gpui::ClipboardItem::new_image(&gpui::Image::from_bytes(
                gpui::ImageFormat::Png,
                png.clone(),
            )))
        });
        cx.simulate_keystrokes("cmd-down cmd-v");
        let text = view.read_with(cx, |view, cx| {
            view.panes[0].editor.read(cx).value().to_string()
        });
        assert_eq!(text, "# Nota\n![](image.png)");
        assert_eq!(std::fs::read(folder.path().join("image.png")).unwrap(), png);
    }

    #[gpui::test]
    fn copies_are_kept_the_latest_first_and_once(cx: &mut TestAppContext) {
        cx.update(|cx| {
            cx.set_global(ClipboardHistory::default());
            remember("uno".into(), cx);
            remember("due".into(), cx);
            remember("uno".into(), cx);
            remember("   ".into(), cx);
            assert_eq!(history(cx), ["uno", "due"]);
        });
    }
}
