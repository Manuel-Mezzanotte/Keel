//! Files that are not text: images open in a preview tab, other binary
//! files are refused with a clear message, and very large files ask first.

use std::collections::HashSet;
use std::path::Path;

use editor::OpenError;
use gpui::{
    AnyElement, MouseButton, ObjectFit, ScrollEvent, StatefulInteractiveElement as _,
    StyledImage as _, img,
};

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

/// The zoom levels of an image, as VS Code steps through them.
const ZOOM_LEVELS: &[f32] = &[
    0.1, 0.2, 0.3, 0.5, 0.75, 1., 1.5, 2., 3., 5., 7., 10., 15., 20.,
];

/// The next zoom level after `zoom`, in or out. `None` is "fit": zooming
/// in from it shows the image at its size, zooming out at half of it.
pub(super) fn next_zoom(zoom: Option<f32>, zoom_in: bool) -> f32 {
    let Some(zoom) = zoom else {
        return if zoom_in { 1. } else { 0.5 };
    };
    if zoom_in {
        ZOOM_LEVELS
            .iter()
            .copied()
            .find(|level| *level > zoom + 0.001)
            .unwrap_or(zoom)
    } else {
        ZOOM_LEVELS
            .iter()
            .rev()
            .copied()
            .find(|level| *level < zoom - 0.001)
            .unwrap_or(zoom)
    }
}

/// An image's size in pixels: from the header of a picture, or from the
/// `width` and `height` (or `viewBox`) of an SVG.
pub(super) fn image_size(path: &Path) -> Option<(u32, u32)> {
    let is_svg = path
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("svg"));
    if !is_svg {
        return image::image_dimensions(path).ok();
    }
    let text = std::fs::read_to_string(path).ok()?;
    let start = text.find("<svg")?;
    let tag = &text[start..start + text[start..].find('>')?];
    let attribute = |name: &str| -> Option<String> {
        let pattern = regex::Regex::new(&format!(r#"\s{name}\s*=\s*["']([^"']*)["']"#)).ok()?;
        Some(pattern.captures(tag)?.get(1)?.as_str().to_owned())
    };
    let number =
        |value: String| -> Option<f32> { value.trim().trim_end_matches("px").parse::<f32>().ok() };
    if let (Some(width), Some(height)) = (
        attribute("width").and_then(number),
        attribute("height").and_then(number),
    ) {
        return Some((width.round() as u32, height.round() as u32));
    }
    let view_box: Vec<f32> = attribute("viewBox")?
        .split([' ', ','])
        .filter_map(|part| part.parse().ok())
        .collect();
    match view_box.as_slice() {
        [_, _, width, height] => Some((width.round() as u32, height.round() as u32)),
        _ => None,
    }
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
    /// The size of the image at `path`, read once.
    pub(super) fn cached_image_size(&self, path: &Path) -> Option<(u32, u32)> {
        *self
            .image_sizes
            .borrow_mut()
            .entry(path.to_path_buf())
            .or_insert_with(|| image_size(path))
    }

    /// Zooms the image of `uri` one step in or out, or back to fit.
    pub(super) fn zoom_image(&mut self, uri: &str, step: Option<bool>, cx: &mut Context<Self>) {
        match step {
            None => {
                self.image_zoom.remove(uri);
            }
            Some(zoom_in) => {
                let zoom = next_zoom(self.image_zoom.get(uri).copied(), zoom_in);
                self.image_zoom.insert(uri.to_owned(), zoom);
            }
        }
        cx.notify();
    }

    /// The image of the active tab, if it shows one.
    pub(super) fn active_image(&self) -> Option<(String, PathBuf)> {
        let pane = self.panes.get(self.active)?;
        match &self.documents.get(&pane.uri)?.backing {
            Backing::Image(path) => Some((pane.uri.clone(), path.clone())),
            _ => None,
        }
    }

    /// What the status bar says about the image in front: its size and zoom.
    pub(super) fn image_status(&self) -> Option<(String, String)> {
        let (uri, path) = self.active_image()?;
        let size = self
            .cached_image_size(&path)
            .map(|(width, height)| format!("{width} × {height}"))
            .unwrap_or_default();
        let zoom = match self.image_zoom.get(&uri) {
            Some(zoom) => format!("{}%", (zoom * 100.).round()),
            None => self.strings().image_fit.to_owned(),
        };
        Some((size, zoom))
    }

    pub(super) fn render_image(
        &self,
        uri: &str,
        path: &Path,
        colors: Colors,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let italian = self.language == UiLanguage::Italian;
        let size = std::fs::metadata(path)
            .map(|metadata| readable_size(metadata.len(), italian))
            .unwrap_or_default();
        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        let zoom = self.image_zoom.get(uri).copied();
        let natural = self.cached_image_size(path);
        let picture = match (zoom, natural) {
            // Zoomed: the image at its size times the zoom, scrolling.
            (Some(zoom), Some((width, height))) => img(path.to_path_buf())
                .flex_none()
                .w(px(width as f32 * zoom))
                .h(px(height as f32 * zoom))
                .into_any_element(),
            _ => img(path.to_path_buf())
                .max_w_full()
                .max_h_full()
                .object_fit(ObjectFit::Contain)
                .into_any_element(),
        };
        let clicked = uri.to_owned();
        let wheeled = uri.to_owned();
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
                    .id("image-view")
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .flex()
                    .when(zoom.is_some(), |view| view.overflow_scroll())
                    .child(
                        // Centered while it fits, from the top left once larger.
                        div()
                            .min_w_full()
                            .min_h_full()
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(picture),
                    )
                    // A click zooms in, Alt+click out, as in VS Code.
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, event: &gpui::MouseDownEvent, _, cx| {
                            this.zoom_image(&clicked, Some(!event.modifiers.alt), cx);
                        }),
                    )
                    // Cmd and the wheel or the trackpad zoom too.
                    .on_scroll(cx.listener(move |this, event: &ScrollEvent, _, cx| {
                        if !event.modifiers.platform {
                            return;
                        }
                        let delta = event.delta.pixel_delta(px(16.)).y;
                        if delta != px(0.) {
                            this.zoom_image(&wheeled, Some(delta > px(0.)), cx);
                            cx.stop_propagation();
                        }
                    })),
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
    fn zoom_steps_like_vs_code() {
        assert_eq!(next_zoom(None, true), 1.);
        assert_eq!(next_zoom(None, false), 0.5);
        assert_eq!(next_zoom(Some(1.), true), 1.5);
        assert_eq!(next_zoom(Some(1.), false), 0.75);
        assert_eq!(next_zoom(Some(20.), true), 20.);
        assert_eq!(next_zoom(Some(0.1), false), 0.1);
    }

    #[test]
    fn image_sizes_from_pictures_and_svgs() {
        let folder = tempfile::tempdir().unwrap();
        let png = folder.path().join("logo.png");
        image::RgbaImage::new(3, 2).save(&png).unwrap();
        assert_eq!(image_size(&png), Some((3, 2)));
        let svg = folder.path().join("icon.svg");
        fs::write(&svg, r#"<?xml version="1.0"?><svg xmlns="http://www.w3.org/2000/svg" width="64px" height="32" viewBox="0 0 10 10"></svg>"#).unwrap();
        assert_eq!(image_size(&svg), Some((64, 32)));
        fs::write(
            &svg,
            r#"<svg viewBox="0 0 1024 512" xmlns="http://www.w3.org/2000/svg"/>"#,
        )
        .unwrap();
        assert_eq!(image_size(&svg), Some((1024, 512)));
        fs::write(&svg, "not an svg").unwrap();
        assert_eq!(image_size(&svg), None);
    }

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
        image::RgbaImage::new(3, 2).save(&image).unwrap();
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
        // The status bar shows the image's size and zoom, nothing of an editor.
        let (_, right) = view.read_with(cx, |view, cx| view.status_texts(cx));
        assert_eq!(right, ["3 × 2", "Fit"]);
        // The zoom keys zoom the image, not the editors' font.
        let font = view.read_with(cx, |view, _| view.font_size);
        view.update(cx, |view, cx| {
            view.zoom(1., cx);
            view.zoom(1., cx);
        });
        let (_, right) = view.read_with(cx, |view, cx| view.status_texts(cx));
        assert_eq!(right, ["3 × 2", "150%"]);
        view.update(cx, |view, cx| view.zoom(0., cx));
        let (_, right) = view.read_with(cx, |view, cx| view.status_texts(cx));
        assert_eq!(right[1], "Fit");
        assert_eq!(view.read_with(cx, |view, _| view.font_size), font);

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
