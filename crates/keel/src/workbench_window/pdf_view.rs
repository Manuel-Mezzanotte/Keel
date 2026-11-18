//! PDF files in a tab, through macOS's PDFKit (crate `pdf`): pages one
//! under the other, drawn only when they come into view; thumbnails;
//! zoom; search with the matches marked; and text selected by dragging,
//! copied with Cmd+C.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::sync::Arc;

use gpui::{
    AnyElement, Entity, FocusHandle, ImageSource, ListAlignment, ListOffset, ListState,
    MouseButton, MouseDownEvent, MouseMoveEvent, Pixels, RenderImage,
    StatefulInteractiveElement as _, img, list,
};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::input::{Input, InputEvent, InputState};
use gpui_component::{ActiveTheme as _, Icon, Selectable as _, Sizable as _};
use gpui_kit_assets::IconName;
use pdf::{Found, PagePoint, PageSize, PdfFile, Picture, Rect, Selected};

use super::other_files::next_zoom;
use super::shell::Colors;
use super::*;

actions!(keel, [CopyPdfSelection, FindInPdf]);

pub(crate) const PDF_CONTEXT: &str = "PdfView";

/// Cmd+C and Cmd+F on a PDF copy its selection and search it.
pub(crate) fn bindings() -> Vec<gpui::KeyBinding> {
    vec![
        gpui::KeyBinding::new("cmd-c", CopyPdfSelection, Some(PDF_CONTEXT)),
        gpui::KeyBinding::new("cmd-f", FindInPdf, Some(PDF_CONTEXT)),
    ]
}

/// Space around and between pages, in points.
const GAP: f32 = 16.;
/// The most pages kept drawn at once; the others are drawn again when they
/// come back into view.
const KEPT_PAGES: usize = 12;
const THUMBNAIL_WIDTH: f32 = 120.;
const SIDEBAR_WIDTH: f32 = 160.;

/// A PDF tab.
pub(super) struct PdfTab {
    file: Rc<PdfFile>,
    pages: Vec<PageSize>,
    /// The scale, points to pixels; `None` fits the widest page to the tab.
    pub(super) zoom: Option<f32>,
    /// The scale and the top page of the last frame. The list cannot be
    /// asked while it draws its rows.
    drawn_scale: std::cell::Cell<f32>,
    drawn_top: std::cell::Cell<usize>,
    list: ListState,
    thumbnail_list: ListState,
    /// Pages drawn: the width in device pixels and the picture.
    pictures: RefCell<HashMap<usize, (u32, Arc<RenderImage>)>>,
    thumbnails: RefCell<HashMap<usize, Arc<RenderImage>>>,
    /// Pictures asked for and not arrived yet.
    drawing: RefCell<HashSet<(usize, u32)>>,
    /// When each page was last shown, to let go of the oldest pictures.
    shown: RefCell<HashMap<usize, u64>>,
    frame: std::cell::Cell<u64>,
    pub(super) show_thumbnails: bool,
    search: Entity<InputState>,
    found: Vec<Found>,
    current: usize,
    selection: Option<Selected>,
    drag_from: Option<PagePoint>,
    focus: FocusHandle,
    _search_events: Subscription,
}

impl PdfTab {
    /// The scale in use, for a list `width` pixels wide.
    fn scale(&self) -> f32 {
        if let Some(zoom) = self.zoom {
            return zoom;
        }
        let width = f32::from(self.list.viewport_bounds().size.width);
        let widest = self.pages.iter().map(|page| page.width).fold(0., f32::max);
        if width <= 0. || widest <= 0. {
            return 1.;
        }
        ((width - 2. * GAP) / widest).max(0.1)
    }

    /// The page at the top of the view, counted from 0.
    pub(super) fn current_page(&self) -> usize {
        self.list.logical_scroll_top().item_ix
    }

    pub(super) fn page_count(&self) -> usize {
        self.pages.len()
    }

    /// Where page `index` is drawn in the window, from its row of the list.
    fn page_origin(&self, index: usize) -> Option<gpui::Point<Pixels>> {
        let row = self.list.bounds_for_item(index)?;
        let page = self.pages.get(index)?;
        let width = page.width * self.drawn_scale.get();
        Some(gpui::point(
            row.origin.x + (row.size.width - px(width)) / 2.,
            row.origin.y + px(GAP),
        ))
    }

    /// The point of a page under a window position, in page points.
    fn point_at(&self, position: gpui::Point<Pixels>) -> Option<PagePoint> {
        let scale = self.drawn_scale.get();
        let first = self.current_page();
        (first.saturating_sub(1)..(first + 4).min(self.pages.len())).find_map(|index| {
            let origin = self.page_origin(index)?;
            let page = self.pages[index];
            let x = f32::from(position.x - origin.x) / scale;
            let y = f32::from(position.y - origin.y) / scale;
            (y >= -GAP && y <= page.height + GAP).then_some(PagePoint {
                page: index,
                x: x.clamp(0., page.width),
                y: y.clamp(0., page.height),
            })
        })
    }
}

/// A picture of a page, as GPUI draws it.
fn render_image(picture: Picture) -> Option<Arc<RenderImage>> {
    let buffer = image::RgbaImage::from_raw(picture.width, picture.height, picture.bgra)?;
    Some(Arc::new(RenderImage::new(vec![image::Frame::new(buffer)])))
}

/// Whether Keel shows the file as a PDF.
pub(super) fn is_pdf(path: &Path) -> bool {
    path.extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("pdf"))
}

impl Workbench {
    /// Opens a PDF into a document.
    pub(super) fn load_pdf(
        &mut self,
        path: &Path,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<String, String> {
        let uri = Uri::file(path).to_string();
        if self.documents.contains_key(&uri) {
            return Ok(uri);
        }
        let file = PdfFile::open(path).map_err(|error| {
            self.strings()
                .could_not_open(&path.display().to_string(), &format!("{error:#}"))
        })?;
        let pages = file.pages().to_vec();
        let search = cx.new(|cx| {
            InputState::new(window, cx).placeholder(self.strings().pdf_search_placeholder)
        });
        let watched = uri.clone();
        let search_events = cx.subscribe_in(
            &search,
            window,
            move |this, _, event: &InputEvent, window, cx| match event {
                InputEvent::Change => this.search_pdf(&watched, cx),
                InputEvent::PressEnter { shift, .. } => {
                    this.next_pdf_match(&watched, !shift, window, cx)
                }
                _ => {}
            },
        );
        let tab = PdfTab {
            file: Rc::new(file),
            list: ListState::new(pages.len(), ListAlignment::Top, px(600.)),
            thumbnail_list: ListState::new(pages.len(), ListAlignment::Top, px(300.)),
            pages,
            zoom: None,
            drawn_scale: std::cell::Cell::new(1.),
            drawn_top: std::cell::Cell::new(0),
            pictures: RefCell::default(),
            thumbnails: RefCell::default(),
            drawing: RefCell::default(),
            shown: RefCell::default(),
            frame: std::cell::Cell::new(0),
            show_thumbnails: false,
            search,
            found: Vec::new(),
            current: 0,
            selection: None,
            drag_from: None,
            focus: cx.focus_handle(),
            _search_events: search_events,
        };
        self.documents.insert(
            uri.clone(),
            OpenDocument {
                uri: uri.clone(),
                backing: Backing::Pdf(Box::new(tab)),
                language_id: "pdf".to_owned(),
                version: 1,
                synced_text: String::new(),
                modified: false,
            },
        );
        Ok(uri)
    }

    fn pdf_tab(&self, uri: &str) -> Option<&PdfTab> {
        match &self.documents.get(uri)?.backing {
            Backing::Pdf(tab) => Some(tab),
            _ => None,
        }
    }

    fn pdf_tab_mut(&mut self, uri: &str) -> Option<&mut PdfTab> {
        match &mut self.documents.get_mut(uri)?.backing {
            Backing::Pdf(tab) => Some(tab),
            _ => None,
        }
    }

    /// The PDF of the active tab.
    pub(super) fn active_pdf(&self) -> Option<String> {
        let pane = self.panes.get(self.active)?;
        self.pdf_tab(&pane.uri).map(|_| pane.uri.clone())
    }

    /// Asks the PDF thread for a page `width` device pixels wide, unless it
    /// is already drawn at that width or on its way.
    fn draw_page(
        &self,
        uri: &str,
        index: usize,
        width: u32,
        thumbnail: bool,
        cx: &mut Context<Self>,
    ) {
        let Some(tab) = self.pdf_tab(uri) else { return };
        if !tab
            .drawing
            .borrow_mut()
            .insert((index, if thumbnail { 0 } else { width }))
        {
            return;
        }
        let picture = tab.file.draw(index, width);
        let uri = uri.to_owned();
        cx.spawn(async move |this, cx| {
            let picture = picture.await.ok().flatten().and_then(render_image);
            this.update(cx, |this, cx| {
                let Some(tab) = this.pdf_tab(&uri) else {
                    return;
                };
                tab.drawing
                    .borrow_mut()
                    .remove(&(index, if thumbnail { 0 } else { width }));
                let Some(picture) = picture else { return };
                let mut dropped: Vec<Arc<RenderImage>> = Vec::new();
                if thumbnail {
                    dropped.extend(tab.thumbnails.borrow_mut().insert(index, picture));
                } else {
                    let old = tab.pictures.borrow_mut().insert(index, (width, picture));
                    dropped.extend(old.map(|(_, image)| image));
                    dropped.extend(this.forget_old_pages(&uri));
                }
                for image in dropped {
                    cx.drop_image(image, None);
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Lets go of the pictures of pages not seen for longest, beyond the
    /// ones kept.
    fn forget_old_pages(&self, uri: &str) -> Vec<Arc<RenderImage>> {
        let Some(tab) = self.pdf_tab(uri) else {
            return Vec::new();
        };
        let mut pictures = tab.pictures.borrow_mut();
        if pictures.len() <= KEPT_PAGES {
            return Vec::new();
        }
        let shown = tab.shown.borrow();
        let mut pages: Vec<usize> = pictures.keys().copied().collect();
        pages.sort_by_key(|page| shown.get(page).copied().unwrap_or(0));
        let extra = pictures.len() - KEPT_PAGES;
        pages
            .into_iter()
            .take(extra)
            .filter_map(|page| pictures.remove(&page).map(|(_, image)| image))
            .collect()
    }

    pub(super) fn zoom_pdf(&mut self, uri: &str, step: Option<bool>, cx: &mut Context<Self>) {
        let Some(tab) = self.pdf_tab_mut(uri) else {
            return;
        };
        tab.zoom = match step {
            None => None,
            Some(zoom_in) => Some(next_zoom(
                Some(tab.zoom.unwrap_or_else(|| tab.scale())),
                zoom_in,
            )),
        };
        // Keep the same page at the top.
        let top = tab.list.logical_scroll_top();
        tab.list.remeasure();
        tab.list.scroll_to(ListOffset {
            item_ix: top.item_ix,
            offset_in_item: px(0.),
        });
        cx.notify();
    }

    fn toggle_thumbnails(&mut self, uri: &str, cx: &mut Context<Self>) {
        if let Some(tab) = self.pdf_tab_mut(uri) {
            tab.show_thumbnails = !tab.show_thumbnails;
            tab.list.remeasure();
            cx.notify();
        }
    }

    fn go_to_pdf_page(&mut self, uri: &str, page: usize, cx: &mut Context<Self>) {
        if let Some(tab) = self.pdf_tab(uri) {
            tab.list.scroll_to(ListOffset {
                item_ix: page,
                offset_in_item: px(0.),
            });
            cx.notify();
        }
    }

    /// The search field changed: look for it in the whole document.
    fn search_pdf(&mut self, uri: &str, cx: &mut Context<Self>) {
        let Some(tab) = self.pdf_tab(uri) else { return };
        let query = tab.search.read(cx).value().trim().to_owned();
        let found = tab.file.find(&query);
        let uri = uri.to_owned();
        cx.spawn(async move |this, cx| {
            let found = found.await.unwrap_or_default();
            this.update(cx, |this, cx| {
                let Some(tab) = this.pdf_tab_mut(&uri) else {
                    return;
                };
                // Only the answer for the text still in the field counts.
                if tab.search.read(cx).value().trim() != query {
                    return;
                }
                tab.found = found;
                tab.current = 0;
                if let Some(first) = tab.found.first() {
                    let page = first.page;
                    this.go_to_pdf_page(&uri, page, cx);
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn next_pdf_match(
        &mut self,
        uri: &str,
        forward: bool,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(tab) = self.pdf_tab_mut(uri) else {
            return;
        };
        let count = tab.found.len();
        if count == 0 {
            return;
        }
        tab.current = if forward {
            (tab.current + 1) % count
        } else {
            (tab.current + count - 1) % count
        };
        let page = tab.found[tab.current].page;
        self.go_to_pdf_page(uri, page, cx);
    }

    /// Dragging over a page selects its text, between where the drag
    /// started and the pointer.
    fn drag_pdf(&mut self, uri: &str, to: PagePoint, cx: &mut Context<Self>) {
        let Some(tab) = self.pdf_tab(uri) else { return };
        let Some(from) = tab.drag_from else { return };
        let selected = tab.file.select(from, to);
        let uri = uri.to_owned();
        cx.spawn(async move |this, cx| {
            let selected = selected.await.ok().flatten();
            this.update(cx, |this, cx| {
                if let Some(tab) = this.pdf_tab_mut(&uri)
                    && tab.drag_from.is_some()
                {
                    tab.selection = selected;
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
    }

    fn copy_pdf_selection(&mut self, cx: &mut Context<Self>) {
        let Some(uri) = self.active_pdf() else { return };
        let Some(text) = self
            .pdf_tab(&uri)
            .and_then(|tab| tab.selection.as_ref())
            .map(|selection| selection.text.clone())
        else {
            return;
        };
        cx.write_to_clipboard(gpui::ClipboardItem::new_string(text));
    }

    pub(super) fn pdf_actions(&self, root: gpui::Div, cx: &mut Context<Self>) -> gpui::Div {
        root.on_action(cx.listener(|this, _: &CopyPdfSelection, _, cx| this.copy_pdf_selection(cx)))
            .on_action(cx.listener(|this, _: &FindInPdf, window, cx| {
                if let Some(uri) = this.active_pdf()
                    && let Some(tab) = this.pdf_tab(&uri)
                {
                    let search = tab.search.clone();
                    search.update(cx, |search, cx| search.focus(window, cx));
                }
            }))
    }

    // -----------------------------------------------------------------
    // Drawing

    pub(super) fn render_pdf(
        &self,
        uri: &str,
        tab: &PdfTab,
        colors: Colors,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let strings = self.strings();
        let scale = tab.scale();
        tab.drawn_scale.set(scale);
        tab.drawn_top.set(tab.current_page());
        let zoom_text = match tab.zoom {
            Some(zoom) => format!("{}%", (zoom * 100.).round()),
            None => strings.image_fit.to_owned(),
        };
        let counter = match tab.found.len() {
            0 if tab.search.read(cx).value().trim().is_empty() => String::new(),
            0 => strings.no_results.to_owned(),
            count => format!("{}/{count}", tab.current + 1),
        };
        let button = |id: &'static str, icon: IconName, tooltip: &'static str| {
            Button::new(id)
                .ghost()
                .small()
                .icon(Icon::new(icon))
                .tooltip(tooltip)
        };
        let toolbar = div()
            .h(px(34.))
            .flex_none()
            .px_2()
            .flex()
            .items_center()
            .gap_1()
            .border_b_1()
            .border_color(colors.border)
            .child({
                let uri = uri.to_owned();
                button(
                    "pdf-thumbnails",
                    IconName::PanelLeft,
                    strings.pdf_thumbnails,
                )
                .selected(tab.show_thumbnails)
                .on_click(cx.listener(move |this, _, _, cx| this.toggle_thumbnails(&uri, cx)))
            })
            .child(
                div()
                    .px_2()
                    .text_xs()
                    .text_color(colors.muted)
                    .child(strings.pdf_page_of(tab.current_page() + 1, tab.page_count())),
            )
            .child({
                let uri = uri.to_owned();
                button("pdf-zoom-out", IconName::Minus, strings.command_zoom_out).on_click(
                    cx.listener(move |this, _, _, cx| this.zoom_pdf(&uri, Some(false), cx)),
                )
            })
            .child({
                let uri = uri.to_owned();
                Button::new("pdf-zoom-fit")
                    .ghost()
                    .small()
                    .label(zoom_text)
                    .tooltip(strings.image_fit)
                    .on_click(cx.listener(move |this, _, _, cx| this.zoom_pdf(&uri, None, cx)))
            })
            .child({
                let uri = uri.to_owned();
                button("pdf-zoom-in", IconName::Plus, strings.command_zoom_in).on_click(
                    cx.listener(move |this, _, _, cx| this.zoom_pdf(&uri, Some(true), cx)),
                )
            })
            .child(div().flex_1())
            .child(div().w(px(220.)).child(Input::new(&tab.search).xsmall()))
            .child(
                div()
                    .min_w(px(40.))
                    .text_xs()
                    .text_color(colors.muted)
                    .child(counter),
            )
            .child({
                let uri = uri.to_owned();
                button("pdf-previous", IconName::ChevronUp, strings.find_previous).on_click(
                    cx.listener(move |this, _, window, cx| {
                        this.next_pdf_match(&uri, false, window, cx)
                    }),
                )
            })
            .child({
                let uri = uri.to_owned();
                button("pdf-next", IconName::ChevronDown, strings.find_next).on_click(cx.listener(
                    move |this, _, window, cx| this.next_pdf_match(&uri, true, window, cx),
                ))
            });

        let device = window.scale_factor();
        let pages = {
            let uri = uri.to_owned();
            list(
                tab.list.clone(),
                cx.processor(move |this, index: usize, window, cx| {
                    this.render_pdf_page(&uri, index, window, cx)
                }),
            )
            .size_full()
        };
        let sidebar = tab.show_thumbnails.then(|| {
            let uri = uri.to_owned();
            div()
                .w(px(SIDEBAR_WIDTH))
                .h_full()
                .flex_none()
                .border_r_1()
                .border_color(colors.border)
                .child(
                    list(
                        tab.thumbnail_list.clone(),
                        cx.processor(move |this, index: usize, _, cx| {
                            this.render_pdf_thumbnail(&uri, index, device, colors, cx)
                        }),
                    )
                    .size_full(),
                )
        });
        let focus = tab.focus.clone();
        let uri_down = uri.to_owned();
        let uri_move = uri.to_owned();
        let uri_up = uri.to_owned();
        let _ = scale;
        div()
            .size_full()
            .flex()
            .flex_col()
            .child(toolbar)
            .child(
                div().flex_1().min_h_0().flex().children(sidebar).child(
                    div()
                        .id("pdf-pages")
                        .flex_1()
                        .min_w_0()
                        .h_full()
                        .bg(cx.theme().muted)
                        .key_context(PDF_CONTEXT)
                        .track_focus(&focus)
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                                let Some(tab) = this.pdf_tab_mut(&uri_down) else {
                                    return;
                                };
                                tab.focus.focus(window, cx);
                                tab.drag_from = tab.point_at(event.position);
                                tab.selection = None;
                                cx.notify();
                            }),
                        )
                        .on_mouse_move(cx.listener(move |this, event: &MouseMoveEvent, _, cx| {
                            if event.pressed_button != Some(MouseButton::Left) {
                                return;
                            }
                            let Some(point) = this
                                .pdf_tab(&uri_move)
                                .and_then(|tab| tab.point_at(event.position))
                            else {
                                return;
                            };
                            this.drag_pdf(&uri_move, point, cx);
                        }))
                        .on_mouse_up(
                            MouseButton::Left,
                            cx.listener(move |this, _, _, _| {
                                if let Some(tab) = this.pdf_tab_mut(&uri_up) {
                                    tab.drag_from = None;
                                }
                            }),
                        )
                        .child(pages),
                ),
            )
            .into_any_element()
    }

    fn render_pdf_page(
        &mut self,
        uri: &str,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(tab) = self.pdf_tab(uri) else {
            return div().into_any_element();
        };
        let Some(page) = tab.pages.get(index).copied() else {
            return div().into_any_element();
        };
        let scale = tab.drawn_scale.get();
        let (width, height) = (page.width * scale, page.height * scale);
        let device_width = (width * window.scale_factor()).round().max(1.) as u32;
        let frame = tab.frame.get() + 1;
        tab.frame.set(frame);
        tab.shown.borrow_mut().insert(index, frame);
        let picture = tab.pictures.borrow().get(&index).cloned();
        // A picture at another width stands in until the right one comes.
        if picture
            .as_ref()
            .is_none_or(|(drawn, _)| *drawn != device_width)
        {
            self.draw_page(uri, index, device_width, false, cx);
        }
        let Some(tab) = self.pdf_tab(uri) else {
            return div().into_any_element();
        };
        let theme = cx.theme();
        let highlight = |rect: &Rect, color: Hsla| {
            div()
                .absolute()
                .left(px(rect.x * scale))
                .top(px(rect.y * scale))
                .w(px(rect.width * scale))
                .h(px(rect.height * scale))
                .bg(color)
        };
        let mut marks: Vec<AnyElement> = Vec::new();
        for (number, found) in tab
            .found
            .iter()
            .enumerate()
            .filter(|(_, found)| found.page == index)
        {
            let color = if number == tab.current {
                theme.warning.opacity(0.55)
            } else {
                theme.warning.opacity(0.25)
            };
            marks.extend(
                found
                    .rects
                    .iter()
                    .map(|rect| highlight(rect, color).into_any_element()),
            );
        }
        if let Some(selection) = &tab.selection {
            marks.extend(
                selection
                    .rects
                    .iter()
                    .filter(|(page, _)| *page == index)
                    .map(|(_, rect)| highlight(rect, theme.selection).into_any_element()),
            );
        }
        div()
            .w_full()
            .pt(px(GAP))
            .flex()
            .justify_center()
            .when(index + 1 == tab.pages.len(), |row| row.pb(px(GAP)))
            .child(
                div()
                    .relative()
                    .w(px(width))
                    .h(px(height))
                    .bg(gpui::white())
                    .shadow_md()
                    .children(picture.map(|(_, image)| {
                        img(ImageSource::Render(image))
                            .size_full()
                            .into_any_element()
                    }))
                    .children(marks),
            )
            .into_any_element()
    }

    fn render_pdf_thumbnail(
        &mut self,
        uri: &str,
        index: usize,
        device: f32,
        colors: Colors,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(tab) = self.pdf_tab(uri) else {
            return div().into_any_element();
        };
        let Some(page) = tab.pages.get(index).copied() else {
            return div().into_any_element();
        };
        let height = THUMBNAIL_WIDTH * page.height / page.width.max(1.);
        let current = tab.drawn_top.get() == index;
        let picture = tab.thumbnails.borrow().get(&index).cloned();
        if picture.is_none() {
            self.draw_page(
                uri,
                index,
                (THUMBNAIL_WIDTH * device).round() as u32,
                true,
                cx,
            );
        }
        let target = uri.to_owned();
        div()
            .id(("pdf-thumbnail", index))
            .w_full()
            .py_2()
            .flex()
            .flex_col()
            .items_center()
            .gap_1()
            .cursor_pointer()
            .on_click(cx.listener(move |this, _, _, cx| this.go_to_pdf_page(&target, index, cx)))
            .child(
                div()
                    .w(px(THUMBNAIL_WIDTH))
                    .h(px(height))
                    .bg(gpui::white())
                    .border_2()
                    .border_color(if current {
                        colors.accent
                    } else {
                        gpui::transparent_black()
                    })
                    .children(picture.map(|image| img(ImageSource::Render(image)).size_full())),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(colors.muted)
                    .child((index + 1).to_string()),
            )
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use gpui::{TestAppContext, VisualTestContext};

    use super::*;

    fn wait_until(
        cx: &mut VisualTestContext,
        what: &str,
        check: impl Fn(&mut VisualTestContext) -> bool,
    ) {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            cx.run_until_parked();
            if check(cx) {
                return;
            }
            assert!(Instant::now() < deadline, "gave up waiting for {what}");
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    #[gpui::test]
    fn a_pdf_opens_draws_finds_and_copies(cx: &mut TestAppContext) {
        cx.executor().allow_parking();
        cx.update(|cx| {
            gpui_component::init(cx);
            language::register_grammars();
            crate::workbench_window::init(cx);
        });
        let folder = tempfile::tempdir().unwrap();
        let base = std::fs::canonicalize(folder.path()).unwrap();
        let path = base.join("manuale.pdf");
        pdf::sample::write(&path);
        let settings = Settings::load(base.join("settings.json"), None);
        let opening = Opening::new(Some(base.clone()), None);
        let (view, cx) =
            cx.add_window_view(|window, cx| Workbench::new(opening, settings, None, window, cx));
        view.update_in(cx, |view, window, cx| view.open_file(&path, window, cx));
        let uri = Uri::file(&path).to_string();
        view.read_with(cx, |view, _| {
            let tab = view.pdf_tab(&uri).expect("a PDF tab");
            assert_eq!(tab.page_count(), 2);
            assert_eq!(view.panes[view.active].uri, uri);
        });

        // Drawn into the window, the pages get their pictures.
        cx.update(|window, cx| {
            let _ = window.draw(cx);
        });
        wait_until(cx, "the first page", |cx| {
            view.read_with(cx, |view, _| {
                view.pdf_tab(&uri)
                    .unwrap()
                    .pictures
                    .borrow()
                    .contains_key(&0)
            })
        });

        // Searching marks the matches and goes to the first.
        view.update_in(cx, |view, window, cx| {
            let search = view.pdf_tab(&uri).unwrap().search.clone();
            search.update(cx, |search, cx| search.set_value("pagina", window, cx));
            view.search_pdf(&uri, cx);
        });
        wait_until(cx, "two matches", |cx| {
            view.read_with(cx, |view, _| view.pdf_tab(&uri).unwrap().found.len() == 2)
        });
        view.update_in(cx, |view, window, cx| {
            view.next_pdf_match(&uri, true, window, cx)
        });
        view.read_with(cx, |view, _| {
            assert_eq!(view.pdf_tab(&uri).unwrap().current, 1)
        });

        // A selection is copied with Cmd+C.
        view.update(cx, |view, cx| {
            let tab = view.pdf_tab_mut(&uri).unwrap();
            tab.drag_from = Some(PagePoint {
                page: 0,
                x: 35.,
                y: 45.,
            });
            view.drag_pdf(
                &uri,
                PagePoint {
                    page: 0,
                    x: 250.,
                    y: 65.,
                },
                cx,
            );
        });
        wait_until(cx, "a selection", |cx| {
            view.read_with(cx, |view, _| {
                view.pdf_tab(&uri).unwrap().selection.is_some()
            })
        });
        view.update(cx, |view, cx| view.copy_pdf_selection(cx));
        let copied = cx
            .read_from_clipboard()
            .and_then(|item| item.text())
            .unwrap_or_default();
        assert!(copied.contains("Ciao Keel"), "{copied:?}");

        // Zoom steps from the fitted scale and back.
        view.update(cx, |view, cx| view.zoom_pdf(&uri, Some(true), cx));
        view.read_with(cx, |view, _| {
            assert!(view.pdf_tab(&uri).unwrap().zoom.is_some())
        });
        view.update(cx, |view, cx| view.zoom_pdf(&uri, None, cx));
        view.read_with(cx, |view, _| {
            assert!(view.pdf_tab(&uri).unwrap().zoom.is_none())
        });
    }

    #[test]
    fn pdfs_are_known_by_their_extension() {
        assert!(is_pdf(Path::new("Manuale.PDF")));
        assert!(!is_pdf(Path::new("note.md")));
    }
}
