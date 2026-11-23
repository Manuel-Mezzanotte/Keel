//! Keel's version, as people read it, and its build number, which only
//! grows: updates compare builds. `scripts/bundle-macos.sh` writes both in
//! the app's `Info.plist`; the release script raises them.

/// The version people read, from the workspace's `Cargo.toml`.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// The build number: one more for every release.
pub const BUILD: u64 = 2;

/// `0.1.0 (1)`, as the Settings and `keel --version` show it.
pub fn full() -> String {
    format!("{VERSION} ({BUILD})")
}
