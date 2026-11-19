//! Whether to move things on screen: macOS's Reduce Motion (Accessibility
//! → Display) turns Keel's animations off, as it does those of Apple's apps.

/// Asked of macOS each time, which answers at once.
pub fn reduced() -> bool {
    crate::mac::accessibility().reduce_motion
}
