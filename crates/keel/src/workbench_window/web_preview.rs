//! A web page in a tab, beside the code: the page of the project that runs
//! (`localhost:3000` of `pnpm dev`), opened from Ports or the palette. It is
//! macOS's WebKit laid over the window, as Sail shows its pages (crate
//! `web`): Keel draws the bar above it and leaves the room below, where the
//! page stands while its tab is in view. A picture of it goes to Claude.

use std::collections::HashMap;
use std::rc::Rc;
use std::time::Duration;

use gpui::{AnyElement, canvas};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::input::{Input, InputEvent, InputState};
use gpui_component::{Disableable as _, Icon, Selectable as _, Sizable as _};
use gpui_kit_assets::IconName;

use super::shell::Colors;
use super::*;

actions!(keel, [OpenWebPreview]);

/// How often an open page is asked for its address and title.
const WATCH: Duration = Duration::from_millis(400);

/// How wide the page is drawn: the whole tab, or a phone's or a tablet's
/// width, to see how a page fits on them.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum PageWidth {
    #[default]
    Full,
    Tablet,
    Phone,
}

impl PageWidth {
    fn points(self) -> Option<f32> {
        match self {
            PageWidth::Full => None,
            PageWidth::Tablet => Some(820.),
            PageWidth::Phone => Some(390.),
        }
    }
}

/// What the bar shows of the page, read from WebKit now and then.
#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct PageState {
    pub(super) url: String,
    pub(super) title: Option<String>,
    pub(super) loading: bool,
    pub(super) back: bool,
    pub(super) forward: bool,
}

/// An open page: WebKit's view (none in the tests, which have no window of
/// macOS), its address field and what it shows.
pub(super) struct WebPage {
    pub(super) view: Option<Rc<web::WebView>>,
    pub(super) address: Entity<InputState>,
    pub(super) state: PageState,
    pub(super) width: PageWidth,
    _events: Subscription,
}

/// The `NSView` of the window, where the pages go.
fn host_view(window: &Window) -> Option<std::ptr::NonNull<std::ffi::c_void>> {
    use raw_window_handle::RawWindowHandle;
    let handle = raw_window_handle::HasWindowHandle::window_handle(window).ok()?;
    match handle.as_raw() {
        RawWindowHandle::AppKit(appkit) => Some(appkit.ns_view),
        _ => None,
    }
}

impl Workbench {
    /// Opens `url` in a tab of its own, in the active group.
    pub(super) fn open_web_page(&mut self, url: &str, window: &mut Window, cx: &mut Context<Self>) {
        let url = web::address(url);
        self.next_web_page += 1;
        let uri = format!(
            "{}{}",
            settings_view::PageKind::Web.uri(),
            self.next_web_page
        );
        let address = cx.new(|cx| {
            let mut input = InputState::new(window, cx).placeholder("localhost:3000");
            input.set_value(url.clone(), window, cx);
            input
        });
        let page_uri = uri.clone();
        let events = cx.subscribe_in(
            &address,
            window,
            move |this, input, event: &InputEvent, _, cx| {
                if let InputEvent::PressEnter { .. } = event {
                    let typed = input.read(cx).value().to_string();
                    this.open_typed_address(&page_uri, &typed, cx);
                }
            },
        );
        let view = host_view(window).and_then(web::WebView::new).map(Rc::new);
        if let Some(view) = &view {
            view.load(&url);
        }
        self.web_pages.insert(
            uri.clone(),
            WebPage {
                view,
                address,
                state: PageState {
                    url,
                    ..PageState::default()
                },
                width: PageWidth::Full,
                _events: events,
            },
        );
        self.add_document(
            uri.clone(),
            Backing::Page(settings_view::PageKind::Web),
            "plaintext".to_owned(),
            String::new(),
        );
        let column = self.active.min(self.panes.len().saturating_sub(1));
        self.open_in_pane(&uri, column, true, window, cx);
        self.watch_web_pages(cx);
        cx.notify();
    }

    /// The address typed in the bar, opened.
    fn open_typed_address(&mut self, uri: &str, typed: &str, cx: &mut Context<Self>) {
        let Some(page) = self.web_pages.get_mut(uri) else {
            return;
        };
        let url = web::address(typed);
        if let Some(view) = &page.view {
            view.load(&url);
            view.focus();
        }
        page.state.url = url;
        cx.notify();
    }

    /// Reads the open pages' address and title while any is open, and draws
    /// the bar again when they change.
    fn watch_web_pages(&mut self, cx: &mut Context<Self>) {
        if self.web_watch.is_some() {
            return;
        }
        self.web_watch = Some(cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(WATCH).await;
                let going = this
                    .update(cx, |this, cx| {
                        let mut changed = false;
                        for page in this.web_pages.values_mut() {
                            let Some(view) = &page.view else { continue };
                            let state = PageState {
                                url: view.url().unwrap_or_else(|| page.state.url.clone()),
                                title: view.title(),
                                loading: view.is_loading(),
                                back: view.can_go_back(),
                                forward: view.can_go_forward(),
                            };
                            if state != page.state {
                                page.state = state;
                                changed = true;
                            }
                        }
                        if changed {
                            cx.notify();
                        }
                        !this.web_pages.is_empty()
                    })
                    .unwrap_or(false);
                if !going {
                    break;
                }
            }
            this.update(cx, |this, _| this.web_watch = None).ok();
        }));
    }

    /// Every page hidden: before each frame, so only those whose tab is drawn
    /// show again, where it stands now.
    pub(super) fn hide_web_pages(&self) {
        for page in self.web_pages.values() {
            if let Some(view) = &page.view {
                view.set_visible(false);
            }
        }
    }

    /// A page closed with its tab: WebKit's view goes with it.
    pub(super) fn forget_web_page(&mut self, uri: &str) {
        self.web_pages.remove(uri);
    }

    /// The tab's name: the page's title, or its address.
    pub(super) fn web_page_title(&self, uri: &str) -> String {
        self.web_pages
            .get(uri)
            .map(|page| {
                page.state.title.clone().unwrap_or_else(|| {
                    page.state
                        .url
                        .split("://")
                        .nth(1)
                        .unwrap_or(&page.state.url)
                        .trim_end_matches('/')
                        .to_owned()
                })
            })
            .unwrap_or_default()
    }

    /// Ports or the palette: the page of a port of the project, the only one
    /// open, or one to choose; `localhost:3000` when none is open.
    pub(super) fn open_web_preview(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let ports: Vec<u16> = self.ports.iter().map(|port| port.port).collect();
        match ports.as_slice() {
            [] => self.open_web_page("localhost:3000", window, cx),
            [port] => self.open_web_page(&format!("localhost:{port}"), window, cx),
            _ => {
                let items = ports
                    .iter()
                    .map(|port| crate::quick_pick::PickItem::new(format!("localhost:{port}")))
                    .collect();
                let chosen = crate::quick_pick::show(
                    Some(self.strings().web_choose_port.to_owned()),
                    None,
                    items,
                    window,
                    cx,
                );
                cx.spawn_in(window, async move |this, cx| {
                    let Ok(Some(index)) = chosen.await else {
                        return;
                    };
                    this.update_in(cx, |this, window, cx| {
                        if let Some(port) = ports.get(index) {
                            this.open_web_page(&format!("localhost:{port}"), window, cx);
                        }
                    })
                    .ok();
                })
                .detach();
            }
        }
    }

    /// "Show to Claude": a picture of the page goes with the next message,
    /// as a pasted screenshot does.
    pub(super) fn show_web_page_to_claude(
        &mut self,
        uri: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(view) = self.web_pages.get(uri).and_then(|page| page.view.clone()) else {
            return;
        };
        let Some(folder) = self
            .data_dir
            .as_ref()
            .map(|data| data.join("pasted-images"))
        else {
            return;
        };
        let (sender, picture) = futures::channel::oneshot::channel();
        view.snapshot(1280., move |png| {
            let _ = sender.send(png);
        });
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Some(png)) = picture.await else {
                return;
            };
            this.update_in(cx, |this, window, cx| {
                if std::fs::create_dir_all(&folder).is_err() {
                    return;
                }
                let path = super::clipboard_history::free_image_name(&folder, "png");
                if std::fs::write(&path, png).is_ok() {
                    this.add_paths_to_agent(vec![path], window, cx);
                }
            })
            .ok();
        })
        .detach();
    }

    pub(super) fn render_web_page(
        &self,
        uri: &str,
        colors: Colors,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(page) = self.web_pages.get(uri) else {
            return div().into_any_element();
        };
        let strings = self.strings();
        let state = &page.state;
        let button = |id: &'static str, icon: IconName, tooltip: &'static str| {
            Button::new(gpui::SharedString::from(format!("{id}-{uri}")))
                .ghost()
                .small()
                .icon(Icon::new(icon))
                .tooltip(tooltip)
        };
        let width_button =
            |id: &'static str, icon: IconName, tooltip: &'static str, width: PageWidth| {
                let page_uri = uri.to_owned();
                button(id, icon, tooltip)
                    .selected(page.width == width)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if let Some(page) = this.web_pages.get_mut(&page_uri) {
                            page.width = width;
                        }
                        cx.notify();
                    }))
            };
        let act = |action: fn(&web::WebView)| {
            let view = page.view.clone();
            move |_: &gpui::ClickEvent, _: &mut Window, _: &mut App| {
                if let Some(view) = &view {
                    action(view);
                }
            }
        };
        let url = state.url.clone();
        let shown_uri = uri.to_owned();
        let bar = div()
            .flex_none()
            .h(px(40.))
            .px_2()
            .flex()
            .items_center()
            .gap_1()
            .border_b_1()
            .border_color(colors.border)
            .child(
                button("web-back", IconName::ArrowLeft, strings.web_back)
                    .disabled(!state.back)
                    .on_click(act(web::WebView::go_back)),
            )
            .child(
                button("web-forward", IconName::ArrowRight, strings.web_forward)
                    .disabled(!state.forward)
                    .on_click(act(web::WebView::go_forward)),
            )
            .child(if state.loading {
                button("web-stop", IconName::X, strings.web_stop).on_click(act(web::WebView::stop))
            } else {
                button("web-reload", IconName::RefreshCw, strings.web_reload)
                    .on_click(act(web::WebView::reload))
            })
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .child(Input::new(&page.address).small()),
            )
            .child(width_button(
                "web-full",
                IconName::Monitor,
                strings.web_width_full,
                PageWidth::Full,
            ))
            .child(width_button(
                "web-tablet",
                IconName::Tablet,
                strings.web_width_tablet,
                PageWidth::Tablet,
            ))
            .child(width_button(
                "web-phone",
                IconName::Smartphone,
                strings.web_width_phone,
                PageWidth::Phone,
            ))
            .child(div().w(px(1.)).h(px(16.)).mx_0p5().bg(colors.border))
            .child(
                button("web-claude", IconName::Camera, strings.web_show_claude).on_click(
                    cx.listener(move |this, _, window, cx| {
                        this.show_web_page_to_claude(&shown_uri, window, cx)
                    }),
                ),
            )
            .child(
                button("web-inspect", IconName::Inspector, strings.web_inspect)
                    .on_click(act(web::WebView::show_inspector)),
            )
            .child(
                button(
                    "web-browser",
                    IconName::ExternalLink,
                    strings.open_in_browser,
                )
                .on_click(move |_, _, cx| cx.open_url(&url)),
            );
        // The room the page stands in. While a dialog of Keel's is open the
        // page stays hidden: it would be drawn over it.
        let view = page.view.clone();
        let width = page.width.points();
        let room = canvas(
            move |bounds, window, cx| {
                let Some(view) = &view else { return };
                if window.has_active_dialog(cx) {
                    view.set_visible(false);
                    return;
                }
                let full = bounds.size.width.as_f32();
                let shown = width.map_or(full, |width| width.min(full));
                let x = bounds.origin.x.as_f32() + (full - shown) / 2.;
                view.set_frame(
                    x as f64,
                    bounds.origin.y.as_f32() as f64,
                    shown as f64,
                    bounds.size.height.as_f32() as f64,
                );
                view.set_visible(true);
            },
            |_, _, _, _| {},
        )
        .size_full();
        let _ = window;
        div()
            .size_full()
            .flex()
            .flex_col()
            .child(bar)
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    // Around a phone's or a tablet's width.
                    .bg(colors.hover.opacity(0.5))
                    .debug_selector(|| "web-page-room".to_owned())
                    .child(room),
            )
            .into_any_element()
    }

    pub(super) fn web_preview_actions(&self, root: gpui::Div, cx: &mut Context<Self>) -> gpui::Div {
        root.on_action(
            cx.listener(|this, _: &OpenWebPreview, window, cx| this.open_web_preview(window, cx)),
        )
    }
}

/// The pages open in a window, by the uri of their tab.
pub(super) type WebPages = HashMap<String, WebPage>;

#[cfg(test)]
mod tests {
    use gpui::TestAppContext;

    use super::super::tests::test_settings;
    use super::*;

    #[gpui::test]
    fn a_page_opens_in_a_tab_and_goes_with_it(cx: &mut TestAppContext) {
        cx.update(|cx| {
            gpui_component::init(cx);
            init(cx);
        });
        let (view, cx) = cx
            .add_window_view(|window, cx| Workbench::new(None, test_settings(), None, window, cx));
        // No port open: the usual one of `pnpm dev`.
        view.update_in(cx, |view, window, cx| view.open_web_preview(window, cx));
        let uri = view.read_with(cx, |view, _| {
            let (uri, page) = view.web_pages.iter().next().unwrap();
            assert_eq!(page.state.url, "http://localhost:3000");
            assert!(view.panes.iter().any(|pane| &pane.uri == uri));
            let document = &view.documents[uri];
            assert_eq!(view.display_name(document), "localhost:3000");
            uri.clone()
        });
        cx.update(|window, cx| {
            let _ = window.draw(cx);
        });
        assert!(cx.debug_bounds("web-page-room").is_some());

        // An address typed in the bar is opened; a phone's width is kept.
        view.update(cx, |view, cx| {
            view.open_typed_address(&uri, "127.0.0.1:5173/prenotazioni", cx);
            view.web_pages.get_mut(&uri).unwrap().width = PageWidth::Phone;
            let page = &view.web_pages[&uri];
            assert_eq!(page.state.url, "http://127.0.0.1:5173/prenotazioni");
            assert_eq!(page.width.points(), Some(390.));
        });

        // Closing the tab lets the page go.
        view.update_in(cx, |view, window, cx| {
            view.close_document_tabs(&uri, window, cx);
            assert!(view.web_pages.is_empty());
            assert!(!view.documents.contains_key(&uri));
        });
    }
}
