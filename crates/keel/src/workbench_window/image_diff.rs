//! A changed picture in the differences, as GitHub Desktop shows it: the
//! two versions side by side with their sizes, or one over the other with
//! a slider that fades from the old to the new.

use std::io::Cursor;
use std::path::Path;
use std::sync::Arc;

use gpui::{AnyElement, ImageFormat, ObjectFit, StyledImage as _, img};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::slider::{Slider, SliderState};
use gpui_component::{Selectable as _, Sizable as _};

use super::diff_view::{DiffSource, DiffTab};
use super::shell::Colors;
use super::*;

/// One version of the picture.
pub(super) struct ImageSide {
    image: Arc<gpui::Image>,
    size: Option<(u32, u32)>,
    bytes: usize,
}

/// The two versions, and how they are shown.
pub(super) struct ImageDiff {
    pub(super) old: Option<ImageSide>,
    pub(super) new: Option<ImageSide>,
    pub(super) overlay: std::cell::Cell<bool>,
    /// How much of the new version shows over the old one.
    pub(super) fade: Entity<SliderState>,
    _fade: Subscription,
}

fn format_of(path: &str) -> Option<ImageFormat> {
    let extension = Path::new(path).extension()?.to_str()?.to_ascii_lowercase();
    Some(match extension.as_str() {
        "png" => ImageFormat::Png,
        "jpg" | "jpeg" => ImageFormat::Jpeg,
        "gif" => ImageFormat::Gif,
        "webp" => ImageFormat::Webp,
        "bmp" => ImageFormat::Bmp,
        "svg" => ImageFormat::Svg,
        "tif" | "tiff" => ImageFormat::Tiff,
        "ico" => ImageFormat::Ico,
        _ => return None,
    })
}

fn side(format: ImageFormat, bytes: Option<Vec<u8>>) -> Option<ImageSide> {
    let bytes = bytes.filter(|bytes| !bytes.is_empty())?;
    let size = image::ImageReader::new(Cursor::new(&bytes))
        .with_guessed_format()
        .ok()
        .and_then(|reader| reader.into_dimensions().ok());
    Some(ImageSide {
        bytes: bytes.len(),
        size,
        image: Arc::new(gpui::Image::from_bytes(format, bytes)),
    })
}

impl Workbench {
    /// The two versions of a changed picture, for a git difference.
    pub(super) fn image_diff(
        &mut self,
        path: &str,
        source: &DiffSource,
        cx: &mut Context<Self>,
    ) -> Option<ImageDiff> {
        let format = format_of(path)?;
        let repo = self.scm.repo.as_ref()?;
        let (old, new) = match source {
            DiffSource::Changes => (
                repo.bytes_at(&format!(":{path}")),
                std::fs::read(repo.absolute(path)).ok(),
            ),
            DiffSource::Staged => (
                repo.bytes_at(&format!("HEAD:{path}")),
                repo.bytes_at(&format!(":{path}")),
            ),
            DiffSource::Commit(hash, original) => (
                repo.bytes_at(&format!("{hash}^:{}", original.as_deref().unwrap_or(path))),
                repo.bytes_at(&format!("{hash}:{path}")),
            ),
            _ => return None,
        };
        let fade = cx.new(|_| {
            SliderState::new()
                .min(0.)
                .max(100.)
                .step(1.)
                .default_value(50.)
        });
        let watch = cx.observe(&fade, |_, _, cx| cx.notify());
        Some(ImageDiff {
            old: side(format, old),
            new: side(format, new),
            overlay: std::cell::Cell::new(false),
            fade,
            _fade: watch,
        })
    }

    pub(super) fn render_image_diff(
        &self,
        uri: &str,
        tab: &DiffTab,
        images: &ImageDiff,
        colors: Colors,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let strings = self.strings();
        let italian = self.language == UiLanguage::Italian;
        let overlay = images.overlay.get();
        let describe = |side: &ImageSide| {
            let mut parts = Vec::new();
            if let Some((width, height)) = side.size {
                parts.push(format!("{width} × {height}"));
            }
            parts.push(super::other_files::readable_size(
                side.bytes as u64,
                italian,
            ));
            parts.join(" · ")
        };
        let picture = |side: &ImageSide| {
            img(side.image.clone())
                .max_w_full()
                .max_h_full()
                .object_fit(ObjectFit::Contain)
        };
        let frame = || {
            div()
                .flex_1()
                .min_h_0()
                .w_full()
                .flex()
                .items_center()
                .justify_center()
                .p_3()
                .rounded(px(8.))
                .border_1()
                .border_color(colors.border)
                .bg(colors.hover.opacity(0.4))
        };
        let column = |label: &'static str, side: Option<&ImageSide>, missing: &'static str| {
            div()
                .flex_1()
                .min_w_0()
                .h_full()
                .flex()
                .flex_col()
                .gap_2()
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .text_sm()
                        .child(div().font_weight(FontWeight::MEDIUM).child(label))
                        .child(
                            div()
                                .text_xs()
                                .text_color(colors.muted)
                                .child(side.map(describe).unwrap_or_default()),
                        ),
                )
                .child(
                    frame()
                        .children(side.map(|side| picture(side).into_any_element()))
                        .when(side.is_none(), |frame| {
                            frame.child(div().text_sm().text_color(colors.faint).child(missing))
                        }),
                )
        };
        let body = if overlay {
            let fade = images.fade.read(cx).value().start() / 100.;
            div()
                .flex_1()
                .min_h_0()
                .flex()
                .flex_col()
                .gap_2()
                .child(
                    frame().child(
                        div()
                            .relative()
                            .size_full()
                            .children(images.old.as_ref().map(|side| {
                                div()
                                    .absolute()
                                    .inset_0()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .child(picture(side))
                            }))
                            .children(images.new.as_ref().map(|side| {
                                div()
                                    .absolute()
                                    .inset_0()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .opacity(fade)
                                    .child(picture(side))
                            })),
                    ),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_3()
                        .text_xs()
                        .text_color(colors.muted)
                        .child(strings.image_diff_before)
                        .child(div().flex_1().child(Slider::new(&images.fade)))
                        .child(strings.image_diff_after),
                )
                .into_any_element()
        } else {
            div()
                .flex_1()
                .min_h_0()
                .flex()
                .gap_3()
                .child(column(
                    strings.image_diff_before,
                    images.old.as_ref(),
                    strings.image_diff_missing_before,
                ))
                .child(column(
                    strings.image_diff_after,
                    images.new.as_ref(),
                    strings.image_diff_missing_after,
                ))
                .into_any_element()
        };
        let choose = |id: &'static str, label: &'static str, on: bool| {
            let uri = uri.to_owned();
            Button::new(id)
                .ghost()
                .small()
                .label(label)
                .selected(on == overlay)
                .on_click(cx.listener(move |this, _, _, cx| {
                    if let Some(Backing::Diff(tab)) =
                        this.documents.get(&uri).map(|document| &document.backing)
                        && let Some(images) = &tab.images
                    {
                        images.overlay.set(on);
                    }
                    cx.notify();
                }))
        };
        div()
            .size_full()
            .flex()
            .flex_col()
            .child(
                div()
                    .h(px(34.))
                    .flex_none()
                    .px_3()
                    .flex()
                    .items_center()
                    .gap_1()
                    .border_b_1()
                    .border_color(colors.border)
                    .child(div().flex_1().text_sm().truncate().child(tab.title.clone()))
                    .child(choose(
                        "image-diff-side",
                        strings.image_diff_side_by_side,
                        false,
                    ))
                    .child(choose(
                        "image-diff-overlay",
                        strings.image_diff_overlay,
                        true,
                    )),
            )
            .child(div().flex_1().min_h_0().p_4().flex().child(body))
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use std::process::Command;

    use gpui::TestAppContext;

    use super::*;

    fn git(folder: &Path, args: &[&str]) {
        let output = Command::new("git")
            .arg("-C")
            .arg(folder)
            .args(args)
            .output()
            .unwrap();
        assert!(output.status.success(), "git {args:?}");
    }

    #[gpui::test]
    fn a_changed_picture_shows_both_versions(cx: &mut TestAppContext) {
        cx.update(|cx| {
            gpui_component::init(cx);
            crate::workbench_window::init(cx);
        });
        let folder = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(folder.path()).unwrap();
        git(&root, &["init", "-q", "-b", "main"]);
        git(&root, &["config", "user.name", "Test"]);
        git(&root, &["config", "user.email", "test@example.com"]);
        git(&root, &["config", "commit.gpgsign", "false"]);
        image::RgbImage::new(2, 3)
            .save(root.join("logo.png"))
            .unwrap();
        git(&root, &["add", "."]);
        git(&root, &["commit", "-q", "-m", "logo"]);
        image::RgbImage::new(4, 5)
            .save(root.join("logo.png"))
            .unwrap();
        let settings = settings::Settings::load("/nonexistent/keel-test/settings.json", None);
        let opening = Opening::new(Some(root.clone()), None);
        let (view, cx) =
            cx.add_window_view(|window, cx| Workbench::new(opening, settings, None, window, cx));
        view.update_in(cx, |view, window, cx| {
            view.open_diff("logo.png", DiffSource::Changes, window, cx)
        });
        view.read_with(cx, |view, _| {
            let tab = view
                .documents
                .values()
                .find_map(|document| match &document.backing {
                    Backing::Diff(tab) if tab.path == "logo.png" => Some(tab),
                    _ => None,
                })
                .expect("a differences tab");
            let images = tab.images.as_ref().expect("shown as pictures");
            assert_eq!(images.old.as_ref().and_then(|side| side.size), Some((2, 3)));
            assert_eq!(images.new.as_ref().and_then(|side| side.size), Some((4, 5)));
            assert!(tab.doc.is_none());
        });
    }
}
