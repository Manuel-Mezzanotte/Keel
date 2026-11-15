//! Files that are not text: images open in a preview tab, other binary
//! files are refused with a clear message, and very large files ask first.

use std::collections::HashSet;
use std::path::Path;

use editor::OpenError;
use gpui::{AnyElement, ObjectFit, StyledImage as _, img};

use super::shell::Colors;
use super::*;

/// Files above this size ask before opening, as VS Code does.
const LARGE_FILE: u64 = 50 * 1024 * 1024;

/// Whether Keel shows the file as an image.
pub(super) fn is_image(path: &Path) -> bool {
    let extension = path
        .extension()
        .and_then(|extension| extension.to_str())
        .map(str::to_ascii_lowercase);
    matches!(
        extension.as_deref(),
        Some("png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" | "ico" | "svg" | "tif" | "tiff")
    )
}

/// A size for people: `820 KB`, `3,4 MB`.
fn readable_size(bytes: u64, italian: bool) -> String {
    let (value, unit) = if bytes >= 1024 * 1024 {
        (bytes as f64 / (1024. * 1024.), "MB")
    } else {
        (bytes as f64 / 1024., "KB")
    };
    let text = if value >= 10. {
        format!("{value:.0}")
    } else {
        format!("{value:.1}")
    };
    let text = if italian {
        text.replace('.', ",")
    } else {
        text
    };
    format!("{text} {unit}")
}

impl Workbench {
    /// Loads a file into a document, unless it is open already. Returns its
    /// uri, or the message to show when Keel cannot open it.
    pub(super) fn load_document(&mut self, path: &Path) -> Result<String, String> {
        let uri = Uri::file(path).to_string();
        if self.documents.contains_key(&uri) {
            return Ok(uri);
        }
        let strings = self.strings();
        if is_image(path) {
            if !path.is_file() {
                return Err(strings.could_not_open(&path.display().to_string(), "not found"));
            }
            // Extensions only ever see text documents.
            self.documents.insert(
                uri.clone(),
                OpenDocument {
                    uri: uri.clone(),
                    backing: Backing::Image(path.to_path_buf()),
                    language_id: "image".to_owned(),
                    version: 1,
                    synced_text: String::new(),
                    modified: false,
                },
            );
            return Ok(uri);
        }
        match Document::open(path) {
            Ok(document) => Ok(self.add_file_document(document)),
            Err(OpenError::NotUtf8) => {
                let name = path
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_default();
                Err(strings.not_text(&name))
            }
            Err(error) => {
                Err(strings.could_not_open(&path.display().to_string(), &error.to_string()))
            }
        }
    }

    /// Asks before opening a very large file. Returns `true` when the file
    /// can open now; otherwise it opens after a yes.
    pub(super) fn large_file_ok(
        &mut self,
        path: &Path,
        open_as: tabs::OpenAs,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let size = std::fs::metadata(path)
            .map(|metadata| metadata.len())
            .unwrap_or(0);
        if size <= LARGE_FILE || is_image(path) || self.large_files_ok.contains(path) {
            return true;
        }
        let strings = self.strings();
        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        let italian = self.language == UiLanguage::Italian;
        let answer = window.prompt(
            PromptLevel::Warning,
            &strings.large_file_question(&name, &readable_size(size, italian)),
            Some(strings.large_file_detail),
            &[strings.open_anyway, strings.cancel],
            cx,
        );
        let path = path.to_path_buf();
        cx.spawn_in(window, async move |this, cx| {
            if answer.await != Ok(0) {
                return;
            }
            this.update_in(cx, |this, window, cx| {
                this.large_files_ok.insert(path.clone());
                this.open_file_as(&path, open_as, window, cx);
            })
            .ok();
        })
        .detach();
        false
    }

    /// The preview of an image, fitted to the editor's room, with its name
    /// and size.
    pub(super) fn render_image(&self, path: &Path, colors: Colors) -> AnyElement {
        let italian = self.language == UiLanguage::Italian;
        let size = std::fs::metadata(path)
            .map(|metadata| readable_size(metadata.len(), italian))
            .unwrap_or_default();
        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        div()
            .size_full()
            .p_6()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap_3()
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(
                        img(path.to_path_buf())
                            .max_w_full()
                            .max_h_full()
                            .object_fit(ObjectFit::Contain),
                    ),
            )
            .child(
                div()
                    .flex_none()
                    .text_xs()
                    .text_color(colors.muted)
                    .child(format!("{name} · {size}")),
            )
            .into_any_element()
    }
}

/// The paths of very large files the user agreed to open.
pub(super) type LargeFilesOk = HashSet<std::path::PathBuf>;

#[cfg(test)]
mod tests {
    use std::fs;

    use gpui::TestAppContext;

    use super::*;

    #[test]
    fn sizes_read_like_finder() {
        assert_eq!(readable_size(840 * 1024, true), "840 KB");
        assert_eq!(readable_size(3_565_158, true), "3,4 MB");
        assert_eq!(readable_size(3_565_158, false), "3.4 MB");
        assert!(is_image(Path::new("logo.SVG")));
        assert!(!is_image(Path::new("logo.ts")));
    }

    #[gpui::test]
    fn images_preview_binaries_are_refused_and_big_files_ask(cx: &mut TestAppContext) {
        cx.update(|cx| {
            gpui_component::init(cx);
            language::register_grammars();
            crate::workbench_window::init(cx);
        });
        let folder = tempfile::tempdir().unwrap();
        let base = fs::canonicalize(folder.path()).unwrap();
        let image = base.join("logo.png");
        fs::write(&image, [0x89, b'P', b'N', b'G', 0, 0, 0xFF]).unwrap();
        let binary = base.join("data.bin");
        fs::write(&binary, [0, 159, 146, 150]).unwrap();
        let large = base.join("dump.log");
        let file = fs::File::create(&large).unwrap();
        file.set_len(LARGE_FILE + 1).unwrap();
        let settings_file = base.join("settings.json");
        fs::write(&settings_file, r#"{ "keel.language": "en" }"#).unwrap();
        let settings = Settings::load(&settings_file, None);
        let opening = Opening::new(Some(base.clone()), Some(image.clone()));
        let (view, cx) =
            cx.add_window_view(|window, cx| Workbench::new(opening, settings, None, window, cx));

        view.read_with(cx, |view, _| {
            assert_eq!(
                view.window_title(),
                format!("logo.png — {}", view.project.name)
            );
        });
        let (_, right) = view.read_with(cx, |view, cx| view.status_texts(cx));
        assert!(right.is_empty(), "{right:?}");

        view.update_in(cx, |view, window, cx| view.open_file(&binary, window, cx));
        let status = view.read_with(cx, |view, _| view.status_message.clone().unwrap());
        assert_eq!(
            status,
            "\"data.bin\" is not a text file, so Keel does not open it"
        );

        view.update_in(cx, |view, window, cx| view.open_file(&large, window, cx));
        assert!(cx.has_pending_prompt());
        cx.simulate_prompt_answer("Cancel");
        cx.run_until_parked();
        assert_eq!(view.read_with(cx, |view, _| view.documents.len()), 1);
    }
}
