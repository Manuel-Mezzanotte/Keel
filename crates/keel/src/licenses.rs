//! The notices and licences of the software built into Keel, which most of
//! those licences ask to be passed on with the app. They are written from
//! `Cargo.lock` by `scripts/third-party-licenses.py`; Settings › Updates
//! shows them in a read-only tab.

/// All of them, as one text.
pub const NOTICES: &str = include_str!("../THIRD-PARTY-LICENSES.txt");

/// The address of the tab that shows them.
pub const URI: &str = "keel:third-party-licenses";

#[cfg(test)]
mod tests {
    use sha2::{Digest as _, Sha256};

    /// The third-party packages of `Cargo.lock`, hashed as `fingerprint()`
    /// in `scripts/third-party-licenses.py` hashes them: their names and
    /// versions, sorted, one a line. Keel's own crates have no `source`, so
    /// a new version of Keel leaves it unchanged.
    fn fingerprint(lock: &str) -> String {
        let mut found = Vec::new();
        let (mut name, mut version, mut source) = (None, None, false);
        for line in lock.lines().chain(std::iter::once("[[package]]")) {
            if line.trim() == "[[package]]" {
                if let (Some(name), Some(version), true) = (name.take(), version.take(), source) {
                    found.push(format!("{name} {version}"));
                }
                source = false;
                continue;
            }
            let quoted = |key: &str| {
                line.strip_prefix(key)
                    .and_then(|rest| rest.strip_prefix(" = \""))
                    .and_then(|rest| rest.strip_suffix('"'))
            };
            if let Some(value) = quoted("name") {
                name = Some(value.to_owned());
            } else if let Some(value) = quoted("version") {
                version = Some(value.to_owned());
            } else if quoted("source").is_some() {
                source = true;
            }
        }
        found.sort();
        Sha256::digest(found.join("\n").as_bytes())
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect()
    }

    #[test]
    fn the_notices_follow_the_dependencies() {
        let line = format!("Dependencies: {}", fingerprint(include_str!("../../../Cargo.lock")));
        assert!(
            super::NOTICES.contains(&line),
            "The dependencies changed: run scripts/third-party-licenses.py"
        );
    }

    #[test]
    fn the_notices_cover_the_engines_keel_runs_on() {
        for name in ["gpui-pre ", "gpui-component ", "deno_core ", "alacritty_terminal ", "tree-sitter ", "V8\n", "ICU\n"] {
            assert!(super::NOTICES.contains(name), "{name:?} is missing");
        }
    }
}
