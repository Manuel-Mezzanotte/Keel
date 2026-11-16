//! Open VSX, the open registry of VS Code extensions that Cursor and VSCodium
//! use: search, the latest version of an extension for this Mac, and
//! downloads checked against their published SHA-256. Requests go through
//! curl, as the language servers' downloads do, so they run on a background
//! thread.

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context as _, Result, bail};
use serde::Deserialize;
use sha2::{Digest, Sha256};

/// The public registry.
pub const OPEN_VSX: &str = "https://open-vsx.org";

/// An extension as the registry lists it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Listing {
    pub namespace: String,
    pub name: String,
    pub version: String,
    #[serde(default)]
    pub display_name: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub download_count: u64,
    #[serde(default)]
    pub deprecated: bool,
    #[serde(default)]
    pub verified: bool,
    #[serde(default)]
    pub files: ListingFiles,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
pub struct ListingFiles {
    #[serde(default)]
    pub download: Option<String>,
    #[serde(default)]
    pub icon: Option<String>,
    #[serde(default)]
    pub sha256: Option<String>,
}

impl Listing {
    /// `publisher.name`, as in `package.json`.
    pub fn id(&self) -> String {
        format!("{}.{}", self.namespace, self.name)
    }

    /// The display name, or the bare name.
    pub fn title(&self) -> &str {
        self.display_name
            .as_deref()
            .filter(|name| !name.trim().is_empty())
            .unwrap_or(&self.name)
    }
}

#[derive(Deserialize)]
struct SearchResult {
    #[serde(default)]
    extensions: Vec<Listing>,
}

/// Reads the answer of `/api/-/search`.
pub fn parse_search(body: &[u8]) -> Result<Vec<Listing>> {
    let result: SearchResult =
        serde_json::from_slice(body).context("Open VSX sent an answer Keel cannot read")?;
    Ok(result.extensions)
}

/// The registry's name for this Mac's builds.
pub fn platform() -> &'static str {
    if cfg!(target_arch = "aarch64") {
        "darwin-arm64"
    } else {
        "darwin-x64"
    }
}

/// Whether version `candidate` comes after `installed`, comparing the
/// numbers of `major.minor.patch`; a pre-release tag counts as older.
pub fn is_newer(candidate: &str, installed: &str) -> bool {
    fn parts(version: &str) -> (Vec<u64>, bool) {
        let (numbers, pre) = match version.split_once('-') {
            Some((numbers, _)) => (numbers, true),
            None => (version, false),
        };
        let numbers = numbers
            .split('.')
            .map(|part| part.trim().parse().unwrap_or(0))
            .collect();
        (numbers, pre)
    }
    let (new, new_pre) = parts(candidate);
    let (old, old_pre) = parts(installed);
    let length = new.len().max(old.len());
    for index in 0..length {
        let a = new.get(index).copied().unwrap_or(0);
        let b = old.get(index).copied().unwrap_or(0);
        if a != b {
            return a > b;
        }
    }
    old_pre && !new_pre
}

/// A registry to talk to: Open VSX, or another server with its API.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Marketplace {
    base: String,
}

impl Default for Marketplace {
    fn default() -> Self {
        Self::open_vsx()
    }
}

impl Marketplace {
    pub fn open_vsx() -> Self {
        Self::with_base(OPEN_VSX)
    }

    /// Another registry, as `https://host` or, in tests, `file:///folder`.
    pub fn with_base(base: impl Into<String>) -> Self {
        Self {
            base: base.into().trim_end_matches('/').to_owned(),
        }
    }

    /// The extensions matching `query`, most relevant first.
    pub fn search(&self, query: &str, size: usize) -> Result<Vec<Listing>> {
        let url = format!(
            "{}/api/-/search?query={}&size={size}&sortBy=relevance&sortOrder=desc",
            self.base,
            encode(query.trim())
        );
        parse_search(&fetch(&url)?)
    }

    /// The latest version of `id` (`publisher.name`): the build for this
    /// Mac when there is one, otherwise the one for every platform.
    pub fn latest(&self, id: &str) -> Result<Listing> {
        let Some((namespace, name)) = id.split_once('.') else {
            bail!("{id} is not an extension id");
        };
        let base = format!("{}/api/{}/{}", self.base, encode(namespace), encode(name));
        let body = fetch(&format!("{base}/{}", platform())).or_else(|_| fetch(&base))?;
        let listing: Listing = serde_json::from_slice(&body)
            .with_context(|| format!("Open VSX sent an answer Keel cannot read for {id}"))?;
        Ok(listing)
    }

    /// Downloads the package of `listing` into `folder`, checking it
    /// against the SHA-256 the registry publishes, and returns its path.
    pub fn download(&self, listing: &Listing, folder: &Path) -> Result<PathBuf> {
        let Some(url) = &listing.files.download else {
            bail!("{} has no package to download", listing.id());
        };
        let bytes = fetch(url)?;
        if let Some(sha_url) = &listing.files.sha256
            && let Ok(published) = fetch(sha_url)
        {
            let published = String::from_utf8_lossy(&published);
            if let Some(expected) = published.split_whitespace().next() {
                let actual = hex(&Sha256::digest(&bytes));
                if !expected.eq_ignore_ascii_case(&actual) {
                    bail!(
                        "the package of {} does not match its checksum",
                        listing.id()
                    );
                }
            }
        }
        std::fs::create_dir_all(folder)?;
        let path = folder.join(format!("{}-{}.vsix", listing.id(), listing.version));
        std::fs::write(&path, bytes)?;
        Ok(path)
    }

    /// Saves the icon of `listing` into `folder`, once per version, and
    /// returns its path.
    pub fn icon(&self, listing: &Listing, folder: &Path) -> Result<PathBuf> {
        let Some(url) = &listing.files.icon else {
            bail!("{} has no icon", listing.id());
        };
        let extension = Path::new(url)
            .extension()
            .and_then(|extension| extension.to_str())
            .filter(|extension| extension.len() <= 4)
            .unwrap_or("png");
        let path = folder.join(format!(
            "{}-{}.{extension}",
            listing.id().to_lowercase(),
            listing.version
        ));
        if !path.is_file() {
            let bytes = fetch(url)?;
            std::fs::create_dir_all(folder)?;
            std::fs::write(&path, bytes)?;
        }
        Ok(path)
    }
}

/// The body at `url`, or why it could not be read.
fn fetch(url: &str) -> Result<Vec<u8>> {
    let output = Command::new("/usr/bin/curl")
        .args([
            "--fail",
            "--silent",
            "--show-error",
            "--location",
            "--max-time",
            "120",
        ])
        .arg(url)
        .output()
        .context("curl did not start")?;
    if !output.status.success() {
        let error = String::from_utf8_lossy(&output.stderr);
        bail!("{}", error.trim().trim_start_matches("curl: "));
    }
    Ok(output.stdout)
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// Percent-encodes a query or a path part.
fn encode(text: &str) -> String {
    let mut encoded = String::new();
    for byte in text.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                encoded.push(byte as char)
            }
            _ => encoded.push_str(&format!("%{byte:02X}")),
        }
    }
    encoded
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_answers_are_read() {
        let body = br#"{"offset":0,"totalSize":2,"extensions":[
            {"namespace":"dracula-theme","name":"theme-dracula","version":"2.25.1",
             "displayName":"Dracula Theme","description":"Dark theme","downloadCount":1200,
             "verified":true,"files":{"download":"https://x/a.vsix","icon":"https://x/icon.png"}},
            {"namespace":"someone","name":"bare","version":"0.1.0","displayName":" "}]}"#;
        let listings = parse_search(body).unwrap();
        assert_eq!(listings.len(), 2);
        assert_eq!(listings[0].id(), "dracula-theme.theme-dracula");
        assert_eq!(listings[0].title(), "Dracula Theme");
        assert_eq!(listings[0].download_count, 1200);
        assert_eq!(listings[1].title(), "bare");
        assert!(listings[1].files.download.is_none());
        assert!(parse_search(b"<html>").is_err());
    }

    #[test]
    fn versions_compare_by_number() {
        assert!(is_newer("2.25.1", "2.24.9"));
        assert!(is_newer("1.10.0", "1.9.0"));
        assert!(is_newer("1.0.0", "1.0.0-beta"));
        assert!(!is_newer("1.0.0", "1.0.0"));
        assert!(!is_newer("0.9.12", "1.0"));
        assert!(!is_newer("1.0.0-beta", "1.0.0"));
    }

    #[test]
    fn queries_are_encoded() {
        assert_eq!(encode("material icon"), "material%20icon");
        assert_eq!(encode("c++ & è"), "c%2B%2B%20%26%20%C3%A8");
    }

    /// A registry made of files, as curl reads `file://` URLs.
    fn registry(root: &Path, package: &[u8], checksum: &str) -> Marketplace {
        let api = root.join("api/someone");
        std::fs::create_dir_all(&api).unwrap();
        std::fs::write(root.join("pkg.vsix"), package).unwrap();
        std::fs::write(root.join("pkg.sha256"), format!("{checksum}  pkg.vsix\n")).unwrap();
        std::fs::write(root.join("icon.png"), b"PNG").unwrap();
        let base = format!("file://{}", root.display());
        let listing = serde_json::json!({
            "namespace": "someone", "name": "tool", "version": "1.2.0",
            "files": {
                "download": format!("{base}/pkg.vsix"),
                "sha256": format!("{base}/pkg.sha256"),
                "icon": format!("{base}/icon.png"),
            }
        });
        std::fs::write(api.join("tool"), listing.to_string()).unwrap();
        Marketplace::with_base(base)
    }

    #[test]
    fn packages_are_downloaded_and_checked() {
        let root = tempfile::tempdir().unwrap();
        let package = b"zip bytes";
        let checksum = hex(&Sha256::digest(package));
        let market = registry(root.path(), package, &checksum);

        // No build for this Mac: the one for every platform.
        let listing = market.latest("someone.tool").unwrap();
        assert_eq!(listing.version, "1.2.0");
        let downloads = root.path().join("downloads");
        let path = market.download(&listing, &downloads).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), package);
        assert!(path.ends_with("someone.tool-1.2.0.vsix"));
        let icon = market.icon(&listing, &root.path().join("icons")).unwrap();
        assert!(icon.ends_with("someone.tool-1.2.0.png"));

        assert!(market.latest("someone.missing").is_err());
        assert!(market.latest("no-dot").is_err());
    }

    #[test]
    fn a_package_that_does_not_match_its_checksum_is_refused() {
        let root = tempfile::tempdir().unwrap();
        let market = registry(root.path(), b"tampered", &"0".repeat(64));
        let listing = market.latest("someone.tool").unwrap();
        let error = market
            .download(&listing, &root.path().join("downloads"))
            .unwrap_err();
        assert!(error.to_string().contains("checksum"), "{error}");
        assert!(
            !root
                .path()
                .join("downloads/someone.tool-1.2.0.vsix")
                .exists()
        );
    }

    /// Talks to the real Open VSX: `cargo test -p extensions -- --ignored open_vsx`.
    #[test]
    #[ignore]
    fn open_vsx_search_and_latest() {
        let market = Marketplace::open_vsx();
        let found = market.search("dracula", 10).unwrap();
        assert!(
            found
                .iter()
                .any(|listing| listing.id() == "dracula-theme.theme-dracula")
        );
        let latest = market.latest("rust-lang.rust-analyzer").unwrap();
        assert!(latest.files.download.unwrap().contains(platform()));
    }
}
