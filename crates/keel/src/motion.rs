//! Whether to move things on screen: macOS's Reduce Motion (Accessibility
//! → Display) turns Keel's animations off, as it does those of Apple's apps.

use std::process::{Command, Stdio};
use std::sync::OnceLock;

/// Read once: the setting rarely changes while Keel runs.
pub fn reduced() -> bool {
    static REDUCED: OnceLock<bool> = OnceLock::new();
    *REDUCED.get_or_init(|| {
        Command::new("/usr/bin/defaults")
            .args(["read", "com.apple.universalaccess", "reduceMotion"])
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output()
            .is_ok_and(|output| String::from_utf8_lossy(&output.stdout).trim() == "1")
    })
}
