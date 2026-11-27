//! One page of WebKit over Keel's window.

use std::cell::{Cell, RefCell};
use std::ffi::c_void;
use std::ptr::NonNull;

use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{ClassType as _, MainThreadMarker, MainThreadOnly as _, msg_send};
use objc2_app_kit::{NSBitmapImageFileType, NSBitmapImageRep, NSImage, NSView};
use objc2_foundation::{
    NSDictionary, NSError, NSPoint, NSRect, NSSize, NSString, NSURL, NSURLRequest,
};
use objc2_web_kit::{WKSnapshotConfiguration, WKWebView, WKWebViewConfiguration};

/// A page inside the window's view, hidden until Keel places it.
pub struct WebView {
    view: Retained<WKWebView>,
    host: Retained<NSView>,
    /// The last frame set, with the host's height it was computed for.
    frame: Cell<Option<[f64; 5]>>,
    visible: Cell<bool>,
}

impl WebView {
    /// A new, empty page inside `host`, the `NSView` of the window. None
    /// off the main thread, as in the tests.
    pub fn new(host: NonNull<c_void>) -> Option<Self> {
        let mtm = MainThreadMarker::new()?;
        // SAFETY: the caller passes the window's live NSView; retaining it
        // keeps it valid for this page.
        let host = unsafe { Retained::retain(host.as_ptr().cast::<NSView>()) }?;
        // SAFETY: a configuration and a view made on the main thread.
        let view = unsafe {
            let config = WKWebViewConfiguration::new(mtm);
            let view: Retained<WKWebView> = msg_send![
                WKWebView::alloc(mtm),
                initWithFrame: NSRect::ZERO,
                configuration: &*config
            ];
            view
        };
        // SAFETY: plain properties of a live view, on the main thread.
        unsafe {
            // Safari's Web Inspector can open it, as from Safari's Develop menu.
            view.setInspectable(true);
            view.setAllowsBackForwardNavigationGestures(true);
            view.setHidden(true);
            host.addSubview(&view);
        }
        Some(Self {
            view,
            host,
            frame: Cell::new(None),
            visible: Cell::new(false),
        })
    }

    pub fn load(&self, url: &str) {
        let Some(url) = NSURL::URLWithString(&NSString::from_str(url)) else {
            return;
        };
        let request = NSURLRequest::requestWithURL(&url);
        // SAFETY: a request for a live view.
        unsafe { self.view.loadRequest(&request) };
    }

    pub fn reload(&self) {
        // SAFETY: as above, for each of these.
        unsafe { self.view.reload() };
    }

    pub fn stop(&self) {
        unsafe { self.view.stopLoading() };
    }

    pub fn go_back(&self) {
        unsafe { self.view.goBack() };
    }

    pub fn go_forward(&self) {
        unsafe { self.view.goForward() };
    }

    pub fn can_go_back(&self) -> bool {
        unsafe { self.view.canGoBack() }
    }

    pub fn can_go_forward(&self) -> bool {
        unsafe { self.view.canGoForward() }
    }

    pub fn is_loading(&self) -> bool {
        unsafe { self.view.isLoading() }
    }

    /// The address of the page shown now.
    pub fn url(&self) -> Option<String> {
        let url = unsafe { self.view.URL() }?;
        Some(url.absoluteString()?.to_string())
    }

    pub fn title(&self) -> Option<String> {
        let title = unsafe { self.view.title() }?.to_string();
        (!title.trim().is_empty()).then_some(title)
    }

    /// Places the page at `x, y` from the window's top left corner, in
    /// points, as GPUI measures; nothing when it is already there.
    pub fn set_frame(&self, x: f64, y: f64, width: f64, height: f64) {
        let host_height = self.host.bounds().size.height;
        let key = [x, y, width, height, host_height];
        if self.frame.get() == Some(key) {
            return;
        }
        self.frame.set(Some(key));
        let y = if self.host.isFlipped() {
            y
        } else {
            host_height - y - height
        };
        self.view
            .setFrame(NSRect::new(NSPoint::new(x, y), NSSize::new(width, height)));
    }

    /// Shows or hides the page: hidden while its tab is not in view, or
    /// while something of Keel's is drawn where it stands.
    pub fn set_visible(&self, visible: bool) {
        if self.visible.replace(visible) != visible {
            self.view.setHidden(!visible);
        }
    }

    pub fn is_visible(&self) -> bool {
        self.visible.get()
    }

    /// The page takes the keyboard.
    pub fn focus(&self) {
        if let Some(window) = self.view.window() {
            window.makeFirstResponder(Some(&self.view));
        }
    }

    /// Whether the page has the keyboard.
    pub fn has_focus(&self) -> bool {
        let Some(window) = self.view.window() else {
            return false;
        };
        let Some(responder) = window.firstResponder() else {
            return false;
        };
        let responder: *const AnyObject = Retained::as_ptr(&responder).cast();
        // The view itself, or one inside it (WebKit's own).
        let mut view: Option<Retained<NSView>> = unsafe {
            let candidate: *mut NSView = responder as *mut NSView;
            let is_view: bool = msg_send![responder, isKindOfClass: NSView::class()];
            if is_view {
                Retained::retain(candidate)
            } else {
                None
            }
        };
        while let Some(current) = view {
            if Retained::as_ptr(&current).cast::<AnyObject>()
                == Retained::as_ptr(&self.view).cast::<AnyObject>()
            {
                return true;
            }
            // SAFETY: a live view of this window.
            view = unsafe { current.superview() };
        }
        false
    }

    /// Opens WebKit's inspector of the page (the same as Safari's).
    pub fn show_inspector(&self) {
        // SAFETY: `_inspector` is WebKit's own, answered by every WKWebView
        // of macOS since 10.11; `show` opens its window.
        let inspector: *mut AnyObject = unsafe { msg_send![&*self.view, _inspector] };
        if let Some(inspector) = unsafe { inspector.as_ref() } {
            let _: () = unsafe { msg_send![inspector, show] };
        }
    }

    /// A picture of the page as it is in view, as PNG, `width` points wide.
    pub fn snapshot(&self, width: f64, done: impl FnOnce(Option<Vec<u8>>) + 'static) {
        let Some(mtm) = MainThreadMarker::new() else {
            return done(None);
        };
        let configuration = unsafe { WKSnapshotConfiguration::new(mtm) };
        unsafe {
            configuration.setSnapshotWidth(Some(&objc2_foundation::NSNumber::new_f64(width)));
        }
        let done = RefCell::new(Some(done));
        let handler = RcBlock::new(move |image: *mut NSImage, _error: *mut NSError| {
            if let Some(done) = done.borrow_mut().take() {
                // SAFETY: WebKit passes the image alive, or null.
                done(unsafe { image.as_ref() }.and_then(png));
            }
        });
        unsafe {
            self.view
                .takeSnapshotWithConfiguration_completionHandler(Some(&configuration), &handler)
        };
    }
}

impl Drop for WebView {
    fn drop(&mut self) {
        self.view.removeFromSuperview();
    }
}

/// An image as PNG.
fn png(image: &NSImage) -> Option<Vec<u8>> {
    let tiff = image.TIFFRepresentation()?;
    let bitmap = NSBitmapImageRep::imageRepWithData(&tiff)?;
    let png = unsafe {
        bitmap.representationUsingType_properties(NSBitmapImageFileType::PNG, &NSDictionary::new())
    }?;
    Some(png.to_vec())
}
