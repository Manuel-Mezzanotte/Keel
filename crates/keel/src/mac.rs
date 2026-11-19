//! The few things of macOS that GPUI does not reach: the Window and Help
//! menus as the system knows them (with the list of windows and the search
//! field), the Dock's badge and bounce, the accessibility settings, and
//! files on the pasteboard, which Finder, Mail and Slack paste.
//!
//! Each call does nothing off the main thread, as in tests.

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

/// Things that waited for you while Keel was in the background: the number
/// on its icon in the Dock.
static WAITING: AtomicUsize = AtomicUsize::new(0);

/// Something needs you while Keel is in the background, as Claude asking
/// for a permission: the Dock's icon counts it and bounces once.
pub fn needs_attention() {
    let count = WAITING.fetch_add(1, Ordering::Relaxed) + 1;
    set_dock_badge(Some(&count.to_string()));
    request_attention();
}

/// Keel came to the front: nothing waits any more.
pub fn attention_seen() {
    if WAITING.swap(0, Ordering::Relaxed) > 0 {
        set_dock_badge(None);
    }
}

/// The accessibility settings of macOS that change how Keel draws.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Accessibility {
    pub reduce_motion: bool,
    pub increase_contrast: bool,
    pub reduce_transparency: bool,
}

#[cfg(target_os = "macos")]
mod native {
    use std::path::PathBuf;

    use objc2::MainThreadMarker;
    use objc2::rc::Retained;
    use objc2::runtime::ProtocolObject;
    use objc2_app_kit::{
        NSApplication, NSMenu, NSPasteboard, NSPasteboardItem, NSPasteboardTypeFileURL,
        NSPasteboardTypeString, NSPasteboardWriting, NSRequestUserAttentionType, NSWorkspace,
    };
    use objc2_foundation::{NSArray, NSString, NSURL};

    use super::Accessibility;

    fn app() -> Option<Retained<NSApplication>> {
        MainThreadMarker::new().map(NSApplication::sharedApplication)
    }

    fn submenu(title: &str) -> Option<Retained<NSMenu>> {
        let main = app()?.mainMenu()?;
        let item = main.itemWithTitle(&NSString::from_str(title))?;
        item.submenu()
    }

    pub fn mark_menus(window: &str, help: &str) {
        let Some(app) = app() else {
            return;
        };
        if let Some(menu) = submenu(window) {
            app.setWindowsMenu(Some(&menu));
        }
        if let Some(menu) = submenu(help) {
            app.setHelpMenu(Some(&menu));
        }
    }

    pub fn bring_all_to_front() {
        if let Some(app) = app() {
            app.arrangeInFront(None);
        }
    }

    pub fn set_dock_badge(label: Option<&str>) {
        if let Some(app) = app() {
            let label = label.map(NSString::from_str);
            app.dockTile().setBadgeLabel(label.as_deref());
        }
    }

    pub fn request_attention() {
        if let Some(app) = app() {
            app.requestUserAttention(NSRequestUserAttentionType::InformationalRequest);
        }
    }

    pub fn accessibility() -> Accessibility {
        if MainThreadMarker::new().is_none() {
            return Accessibility::default();
        }
        let workspace = NSWorkspace::sharedWorkspace();
        Accessibility {
            reduce_motion: workspace.accessibilityDisplayShouldReduceMotion(),
            increase_contrast: workspace.accessibilityDisplayShouldIncreaseContrast(),
            reduce_transparency: workspace.accessibilityDisplayShouldReduceTransparency(),
        }
    }

    pub fn copy_files(paths: &[PathBuf], text: &str) -> bool {
        if MainThreadMarker::new().is_none() || paths.is_empty() {
            return false;
        }
        // One item a file, as the Finder writes them; the first also has
        // the paths as text.
        let items: Vec<Retained<ProtocolObject<dyn NSPasteboardWriting>>> = paths
            .iter()
            .enumerate()
            .map(|(index, path)| {
                let item = NSPasteboardItem::new();
                let url = NSURL::fileURLWithPath(&NSString::from_str(&path.to_string_lossy()));
                // SAFETY: the pasteboard types are constants of AppKit.
                unsafe {
                    if let Some(url) = url.absoluteString() {
                        item.setString_forType(&url, NSPasteboardTypeFileURL);
                    }
                    if index == 0 {
                        item.setString_forType(&NSString::from_str(text), NSPasteboardTypeString);
                    }
                }
                ProtocolObject::from_retained(item)
            })
            .collect();
        let pasteboard = NSPasteboard::generalPasteboard();
        pasteboard.clearContents();
        pasteboard.writeObjects(&NSArray::from_retained_slice(&items))
    }
}

/// Tells macOS which menus are Window and Help, by their titles in the
/// interface language: the open windows are listed in the first, and the
/// second gets the field that searches every menu.
pub fn mark_menus(window: &str, help: &str) {
    #[cfg(target_os = "macos")]
    native::mark_menus(window, help);
    #[cfg(not(target_os = "macos"))]
    let _ = (window, help);
}

/// Window → Bring All to Front.
pub fn bring_all_to_front() {
    #[cfg(target_os = "macos")]
    native::bring_all_to_front();
}

/// The number on Keel's icon in the Dock, or none.
pub fn set_dock_badge(label: Option<&str>) {
    #[cfg(target_os = "macos")]
    native::set_dock_badge(label);
    #[cfg(not(target_os = "macos"))]
    let _ = label;
}

/// Bounces Keel's icon in the Dock once, as Mail does for new mail.
pub fn request_attention() {
    #[cfg(target_os = "macos")]
    native::request_attention();
}

/// Reduce Motion, Increase Contrast and Reduce Transparency, as set in
/// System Settings → Accessibility → Display.
pub fn accessibility() -> Accessibility {
    #[cfg(target_os = "macos")]
    return native::accessibility();
    #[cfg(not(target_os = "macos"))]
    Accessibility::default()
}

/// Puts files on the pasteboard as files, so Finder pastes them and Mail
/// or Slack attach them, with `text` for whoever pastes text. Returns
/// whether it did.
pub fn copy_files(paths: &[PathBuf], text: &str) -> bool {
    #[cfg(target_os = "macos")]
    return native::copy_files(paths, text);
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (paths, text);
        false
    }
}
