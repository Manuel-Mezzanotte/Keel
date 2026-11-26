//! A project's own picture, as JetBrains shows one in its list of projects:
//! the icon the project keeps for itself (its favicon, its app's icon, its
//! logo), found by name in the usual places. Without one, the project shows
//! its folder's icon from the icon theme.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex};

/// Where projects keep their own picture, the most telling first: an icon
/// set for Keel or for JetBrains' IDEs, then a web app's, then a logo.
const PLACES: &[&str] = &[
    ".keel/icon.svg",
    ".keel/icon.png",
    ".idea/icon.svg",
    ".idea/icon.png",
    "app/icon.svg",
    "app/icon.png",
    "src/app/icon.svg",
    "src/app/icon.png",
    "public/favicon.svg",
    "public/icon.svg",
    "public/logo.svg",
    "public/apple-touch-icon.png",
    "public/icon-192.png",
    "static/favicon.svg",
    "assets/icon.svg",
    "assets/icon.png",
    "assets/logo.svg",
    "icon.svg",
    "logo.svg",
];

/// A picture bigger than this is not an icon.
const LARGEST: u64 = 512 * 1024;

/// Projects already looked at: their picture, or none.
static FOUND: LazyLock<Mutex<HashMap<PathBuf, Option<PathBuf>>>> = LazyLock::new(Default::default);

/// The picture `folder` keeps for itself, if it has one.
pub(super) fn own_icon(folder: &Path) -> Option<PathBuf> {
    if let Some(known) = FOUND.lock().ok()?.get(folder) {
        return known.clone();
    }
    let found = PLACES.iter().map(|place| folder.join(place)).find(|path| {
        std::fs::metadata(path)
            .is_ok_and(|meta| meta.is_file() && meta.len() > 0 && meta.len() <= LARGEST)
    });
    FOUND
        .lock()
        .ok()?
        .insert(folder.to_path_buf(), found.clone());
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_project_shows_the_icon_it_keeps() {
        let web = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(web.path().join("public")).unwrap();
        std::fs::create_dir_all(web.path().join("src/app")).unwrap();
        std::fs::write(web.path().join("public/favicon.svg"), "<svg/>").unwrap();
        std::fs::write(web.path().join("src/app/icon.png"), [1u8; 8]).unwrap();
        // The app's icon before the site's.
        assert_eq!(
            own_icon(web.path()),
            Some(web.path().join("src/app/icon.png"))
        );
        // Asked again, the answer is the one found first.
        std::fs::remove_file(web.path().join("src/app/icon.png")).unwrap();
        assert_eq!(
            own_icon(web.path()),
            Some(web.path().join("src/app/icon.png"))
        );

        let plain = tempfile::tempdir().unwrap();
        std::fs::write(plain.path().join("README.md"), "# a\n").unwrap();
        assert_eq!(own_icon(plain.path()), None);

        // An empty file, or a huge one, is no icon.
        let odd = tempfile::tempdir().unwrap();
        std::fs::write(odd.path().join("icon.svg"), "").unwrap();
        assert_eq!(own_icon(odd.path()), None);
    }
}
