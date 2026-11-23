//! Keel's own updates, with nothing drawn: the list of releases on Keel's
//! site, the download and its checks, and the swap of the app once Keel
//! has quit.
//!
//! A release is a `.zip` of `Keel.app`, made with `ditto`. Before anything
//! is unpacked its size, its SHA-256 and its Ed25519 signature must match
//! the list; the app inside must be Keel, of the build the list promises,
//! with a valid code signature. Only then does it take the old app's place,
//! and the old app comes back if the swap fails halfway.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use anyhow::{Context as _, Result, bail, ensure};
use ed25519_dalek::{Signature, Verifier as _, VerifyingKey};
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

pub mod publish;

/// Where Keel reads the list of releases.
pub const FEED: &str = "https://keel.mezzalab.eu/updates.json";

/// Keel's bundle identifier: an update must carry the same.
pub const BUNDLE_ID: &str = "com.manuelmezzanotte.keel";

/// The public half of the key that signs every release, as 64 hex digits.
/// The private half stays on the author's Mac; `scripts/release.sh` makes
/// the pair the first time and writes this value. Empty means no release
/// can be installed yet.
pub const PUBLIC_KEY: &str = "7c3efb97ed19f6c5cf020c8a5eaab6dd1b6c4f90af924885b38b353a4aef6a8b";

/// The list read from [`FEED`].
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
pub struct Feed {
    pub releases: Vec<Release>,
}

/// One release of Keel.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Release {
    /// The version people read, such as `0.2.0`.
    pub version: String,
    /// The build number, which only grows: it decides what is newer.
    pub build: u64,
    /// The day it came out, as `2026-11-24`.
    #[serde(default)]
    pub date: String,
    /// The oldest macOS it runs on, such as `13.0`.
    #[serde(default = "oldest_system")]
    pub minimum_system: String,
    #[serde(default)]
    pub notes: Notes,
    /// Where the `.zip` is.
    pub url: String,
    /// Its size in bytes.
    pub size: u64,
    /// Its SHA-256, as hex.
    pub sha256: String,
    /// The Ed25519 signature of its bytes, as hex.
    pub signature: String,
    /// The disk image of the same version, for the site's download page.
    /// Keel itself does not read it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub disk_image: Option<DiskImage>,
}

/// A disk image to install Keel by hand.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct DiskImage {
    pub url: String,
    pub size: u64,
    pub sha256: String,
}

/// What is new, in the interface's languages.
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
pub struct Notes {
    #[serde(default)]
    pub it: String,
    #[serde(default)]
    pub en: String,
}

fn oldest_system() -> String {
    "13.0".to_owned()
}

/// Reads a list of releases.
pub fn parse_feed(text: &str) -> Result<Feed> {
    serde_json::from_str(text).context("the list of releases is not valid")
}

/// The newest release above `build` that runs on macOS `system`.
pub fn newest<'a>(feed: &'a Feed, build: u64, system: &str) -> Option<&'a Release> {
    let system = version_parts(system);
    feed.releases
        .iter()
        .filter(|release| release.build > build)
        .filter(|release| version_parts(&release.minimum_system) <= system)
        .max_by_key(|release| release.build)
}

/// `13.4.1` as `[13, 4, 1]`, padded so `13` and `13.0` compare equal.
fn version_parts(version: &str) -> [u64; 3] {
    let mut parts = [0; 3];
    for (slot, part) in parts.iter_mut().zip(version.trim().split('.')) {
        *slot = part.parse().unwrap_or(0);
    }
    parts
}

/// The macOS this Mac runs, such as `27.1`.
pub fn system_version() -> Option<String> {
    let output = Command::new("/usr/bin/sw_vers")
        .arg("-productVersion")
        .output()
        .ok()?;
    let version = String::from_utf8(output.stdout).ok()?.trim().to_owned();
    (!version.is_empty()).then_some(version)
}

/// Where to read releases: `KEEL_UPDATE_FEED` when set, to try a list of
/// one's own, otherwise Keel's site.
pub fn feed_url() -> String {
    std::env::var("KEEL_UPDATE_FEED")
        .ok()
        .filter(|url| !url.is_empty())
        .unwrap_or_else(|| FEED.to_owned())
}

/// The key releases are checked with. Builds for development also take
/// `KEEL_UPDATE_KEY`, to try a release signed with a key of one's own.
pub fn public_key() -> Result<VerifyingKey> {
    #[cfg(debug_assertions)]
    if let Ok(key) = std::env::var("KEEL_UPDATE_KEY")
        && !key.is_empty()
    {
        return parse_key(&key);
    }
    ensure!(
        !PUBLIC_KEY.is_empty(),
        "this build of Keel has no key to check releases with"
    );
    parse_key(PUBLIC_KEY)
}

/// A public key from its 64 hex digits.
pub fn parse_key(hex: &str) -> Result<VerifyingKey> {
    let bytes: [u8; 32] = decode_hex(hex)?
        .try_into()
        .map_err(|_| anyhow::anyhow!("a public key has 32 bytes"))?;
    VerifyingKey::from_bytes(&bytes).context("the public key is not valid")
}

fn decode_hex(hex: &str) -> Result<Vec<u8>> {
    let hex = hex.trim();
    ensure!(hex.len().is_multiple_of(2), "odd number of hex digits");
    (0..hex.len())
        .step_by(2)
        .map(|at| {
            u8::from_str_radix(hex.get(at..at + 2).unwrap_or("zz"), 16).context("not a hex digit")
        })
        .collect()
}

fn encode_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// Reads the list of releases at `url`.
pub fn fetch(url: &str) -> Result<Feed> {
    let output = curl(url)
        .args(["--max-time", "15"])
        .output()
        .context("curl did not start")?;
    if !output.status.success() {
        bail!("{}", curl_error(&output.stderr));
    }
    parse_feed(&String::from_utf8_lossy(&output.stdout))
}

/// The Mac's own curl, as Keel uses for everything it downloads.
fn curl(url: &str) -> Command {
    let mut command = Command::new("/usr/bin/curl");
    command
        .args(["--fail", "--silent", "--show-error", "--location"])
        .args(["--proto", "=https,file"])
        .arg(url)
        .stdin(Stdio::null());
    command
}

fn curl_error(stderr: &[u8]) -> String {
    String::from_utf8_lossy(stderr)
        .trim()
        .trim_start_matches("curl: ")
        .to_owned()
}

/// Where a release is downloaded, next to the app it will replace: a swap
/// within one folder never copies, so it is done in an instant.
pub fn staging_dir(app: &Path) -> PathBuf {
    app.parent()
        .unwrap_or_else(|| Path::new("/"))
        .join(".keel-update")
}

/// The file a download grows into: its size says how far it got.
pub fn partial_file(staging: &Path, release: &Release) -> PathBuf {
    staging.join(format!("Keel-{}.zip.part", release.build))
}

/// Fails when Keel could not put a new app where `app` is, because its
/// folder cannot be written.
pub fn can_replace(app: &Path) -> Result<()> {
    let staging = staging_dir(app);
    let folder = app.parent().unwrap_or_else(|| Path::new("/"));
    std::fs::create_dir_all(&staging)
        .and_then(|()| std::fs::write(staging.join(".probe"), b""))
        .with_context(|| format!("the folder {} cannot be written", folder.display()))?;
    let _ = std::fs::remove_file(staging.join(".probe"));
    Ok(())
}

/// Downloads `release` into `staging` and checks it. Returns the `.zip`.
pub fn download(release: &Release, staging: &Path, key: &VerifyingKey) -> Result<PathBuf> {
    std::fs::create_dir_all(staging)?;
    let partial = partial_file(staging, release);
    let _ = std::fs::remove_file(&partial);
    let output = curl(&release.url)
        .args(["--max-time", "1800", "--output"])
        .arg(&partial)
        .output()
        .context("curl did not start")?;
    if !output.status.success() {
        let _ = std::fs::remove_file(&partial);
        bail!("{}", curl_error(&output.stderr));
    }
    let bytes = std::fs::read(&partial)?;
    if let Err(error) = verify(&bytes, release, key) {
        let _ = std::fs::remove_file(&partial);
        return Err(error);
    }
    let zip = staging.join(format!("Keel-{}.zip", release.build));
    std::fs::rename(&partial, &zip)?;
    Ok(zip)
}

/// Checks a downloaded release against the list: size, SHA-256 and
/// signature.
pub fn verify(bytes: &[u8], release: &Release, key: &VerifyingKey) -> Result<()> {
    ensure!(
        bytes.len() as u64 == release.size,
        "the download has {} bytes instead of {}",
        bytes.len(),
        release.size
    );
    let digest = encode_hex(&Sha256::digest(bytes));
    ensure!(
        digest.eq_ignore_ascii_case(release.sha256.trim()),
        "the download is not the release that was published"
    );
    let signature: [u8; 64] = decode_hex(&release.signature)?
        .try_into()
        .map_err(|_| anyhow::anyhow!("a signature has 64 bytes"))?;
    key.verify(bytes, &Signature::from_bytes(&signature))
        .map_err(|_| anyhow::anyhow!("the release is not signed by Keel's author"))
}

/// Unpacks a checked `.zip` into `staging` and checks the app inside.
/// Returns the app, ready to take the old one's place.
pub fn unpack(zip: &Path, staging: &Path, release: &Release) -> Result<PathBuf> {
    let into = staging.join(format!("Keel-{}", release.build));
    if into.exists() {
        std::fs::remove_dir_all(&into)?;
    }
    std::fs::create_dir_all(&into)?;
    run(Command::new("/usr/bin/ditto")
        .args(["-x", "-k"])
        .arg(zip)
        .arg(&into))
    .context("the release could not be unpacked")?;
    let apps: Vec<PathBuf> = std::fs::read_dir(&into)?
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| path.extension().is_some_and(|extension| extension == "app"))
        .collect();
    let [app] = apps.as_slice() else {
        bail!("the release does not hold one app");
    };
    let plist = app.join("Contents/Info.plist");
    let id = plist_value(&plist, "CFBundleIdentifier")?;
    ensure!(id == BUNDLE_ID, "the release is another app ({id})");
    let build = plist_value(&plist, "CFBundleVersion")?;
    ensure!(
        build == release.build.to_string(),
        "the release is build {build} instead of {}",
        release.build
    );
    run(Command::new("/usr/bin/codesign")
        .args(["--verify", "--deep", "--strict"])
        .arg(app))
    .context("the app in the release has no valid code signature")?;
    Ok(app.clone())
}

fn plist_value(plist: &Path, key: &str) -> Result<String> {
    let output = Command::new("/usr/bin/plutil")
        .args(["-extract", key, "raw", "-o", "-"])
        .arg(plist)
        .output()?;
    ensure!(output.status.success(), "the app has no {key}");
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

fn run(command: &mut Command) -> Result<()> {
    let output = command.stdin(Stdio::null()).output()?;
    if !output.status.success() {
        bail!("{}", String::from_utf8_lossy(&output.stderr).trim());
    }
    Ok(())
}

/// The app bundle Keel runs from, when it runs from one:
/// `…/Keel.app/Contents/MacOS/keel` gives `…/Keel.app`.
pub fn running_app(executable: &Path) -> Option<PathBuf> {
    let macos = executable.parent()?;
    let contents = macos.parent()?;
    let app = contents.parent()?;
    (macos.file_name()? == "MacOS"
        && contents.file_name()? == "Contents"
        && app.extension()? == "app")
        .then(|| app.to_path_buf())
}

/// Starts the process that finishes an update once Keel has quit:
/// `executable --finish-update PID STAGED APP`, on its own, so it outlives
/// Keel.
pub fn start_finisher(executable: &Path, staged: &Path, app: &Path) -> Result<()> {
    use std::os::unix::process::CommandExt as _;
    Command::new(executable)
        .arg("--finish-update")
        .arg(std::process::id().to_string())
        .arg(staged)
        .arg(app)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0)
        .spawn()
        .context("the update could not start")?;
    Ok(())
}

/// Waits until the Keel with `pid` has quit, then puts `staged` where `app`
/// is. If the swap fails the old app goes back in its place.
pub fn finish(pid: i32, staged: &Path, app: &Path, wait: Duration) -> Result<()> {
    let deadline = Instant::now() + wait;
    while alive(pid) {
        ensure!(Instant::now() < deadline, "Keel did not quit");
        std::thread::sleep(Duration::from_millis(100));
    }
    ensure!(staged.is_dir(), "the new version is gone");
    let name = app
        .file_name()
        .context("the app has no name")?
        .to_string_lossy()
        .into_owned();
    let backup = app.with_file_name(format!(".{name}.previous"));
    if backup.exists() {
        std::fs::remove_dir_all(&backup)?;
    }
    std::fs::rename(app, &backup).context("the old version could not be moved")?;
    if let Err(error) = std::fs::rename(staged, app) {
        let _ = std::fs::rename(&backup, app);
        return Err(error).context("the new version could not be put in place");
    }
    let _ = std::fs::remove_dir_all(&backup);
    let _ = std::fs::remove_dir_all(staging_dir(app));
    Ok(())
}

fn alive(pid: i32) -> bool {
    // SAFETY: signal 0 only asks whether the process exists.
    let result = unsafe { libc::kill(pid, 0) };
    result == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

/// How the last update went, left by the finishing process for the Keel
/// it opens.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct Outcome {
    /// The version installed, or the one that could not be.
    pub version: String,
    /// Why it failed; `None` when it went well.
    pub error: Option<String>,
}

fn outcome_file(data_dir: &Path) -> PathBuf {
    data_dir.join("update-outcome.json")
}

pub fn write_outcome(data_dir: &Path, outcome: &Outcome) -> Result<()> {
    std::fs::create_dir_all(data_dir)?;
    std::fs::write(outcome_file(data_dir), serde_json::to_vec(outcome)?)?;
    Ok(())
}

/// The outcome of the last update, once: reading it removes it.
pub fn take_outcome(data_dir: &Path) -> Option<Outcome> {
    let file = outcome_file(data_dir);
    let text = std::fs::read_to_string(&file).ok()?;
    let _ = std::fs::remove_file(&file);
    serde_json::from_str(&text).ok()
}

/// A release of a small app that passes for Keel, published in a folder
/// and signed with a key made for tests: for the tests of the window.
#[cfg(any(test, feature = "test-support"))]
pub mod test_support {
    use super::*;
    use ed25519_dalek::{Signer as _, SigningKey};

    /// The key the releases of these tests are signed with.
    pub fn signing_key() -> SigningKey {
        SigningKey::from_bytes(&[7; 32])
    }

    /// Its public half, as Keel takes it.
    pub fn public_key_hex() -> String {
        encode_hex(signing_key().verifying_key().as_bytes())
    }

    /// A small app that passes for Keel: its own Info.plist, an ad hoc
    /// signed executable.
    pub fn fake_app(dir: &Path, id: &str, build: u64) -> PathBuf {
        let app = dir.join("Keel.app");
        std::fs::create_dir_all(app.join("Contents/MacOS")).unwrap();
        std::fs::write(
            app.join("Contents/Info.plist"),
            format!(
                r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleIdentifier</key><string>{id}</string>
<key>CFBundleExecutable</key><string>keel</string>
<key>CFBundlePackageType</key><string>APPL</string>
<key>CFBundleShortVersionString</key><string>0.{build}.0</string>
<key>CFBundleVersion</key><string>{build}</string>
</dict></plist>"#
            ),
        )
        .unwrap();
        std::fs::copy("/usr/bin/true", app.join("Contents/MacOS/keel")).unwrap();
        run(Command::new("/usr/bin/codesign")
            .args(["--force", "--sign", "-"])
            .arg(&app))
        .unwrap();
        app
    }

    /// The `.zip` of `app`, as releases are made.
    pub fn zip_of(app: &Path, zip: &Path) -> Vec<u8> {
        run(Command::new("/usr/bin/ditto")
            .args(["-c", "-k", "--keepParent"])
            .arg(app)
            .arg(zip))
        .unwrap();
        std::fs::read(zip).unwrap()
    }

    /// `bytes` published at a `file://` address as release `build`,
    /// signed with `key`.
    pub fn publish(dir: &Path, bytes: &[u8], build: u64, key: &SigningKey) -> Release {
        let file = dir.join(format!("Keel-{build}.zip"));
        std::fs::write(&file, bytes).unwrap();
        Release {
            version: format!("0.{build}.0"),
            build,
            date: "2026-11-24".to_owned(),
            minimum_system: "13.0".to_owned(),
            notes: Notes {
                it: "Novità di prova.".to_owned(),
                en: "Test notes.".to_owned(),
            },
            url: format!("file://{}", file.display()),
            size: bytes.len() as u64,
            sha256: encode_hex(&Sha256::digest(bytes)),
            signature: encode_hex(&key.sign(bytes).to_bytes()),
            disk_image: None,
        }
    }

    /// A list of releases in `dir` with Keel `build`, signed with the key
    /// of these tests. Returns the list's `file://` address.
    pub fn feed_with(dir: &Path, build: u64) -> String {
        let source = dir.join(format!("source-{build}"));
        let app = fake_app(&source, BUNDLE_ID, build);
        let bytes = zip_of(&app, &source.join("Keel.zip"));
        let release = publish(dir, &bytes, build, &signing_key());
        let feed = Feed {
            releases: vec![release],
        };
        let file = dir.join("updates.json");
        std::fs::write(&file, serde_json::to_vec(&feed).unwrap()).unwrap();
        format!("file://{}", file.display())
    }
}

#[cfg(test)]
mod tests {
    use super::test_support::{fake_app, signing_key, zip_of};
    use super::*;
    use ed25519_dalek::SigningKey;

    fn release(build: u64, minimum: &str) -> Release {
        Release {
            version: format!("0.{build}.0"),
            build,
            date: "2026-11-24".to_owned(),
            minimum_system: minimum.to_owned(),
            notes: Notes::default(),
            url: String::new(),
            size: 0,
            sha256: String::new(),
            signature: String::new(),
            disk_image: None,
        }
    }

    fn published(dir: &Path, bytes: &[u8], build: u64, key: &SigningKey) -> Release {
        test_support::publish(dir, bytes, build, key)
    }

    #[test]
    fn the_newest_release_this_mac_runs_is_chosen() {
        let feed = Feed {
            releases: vec![release(2, "13.0"), release(4, "13.0"), release(5, "30")],
        };
        assert_eq!(newest(&feed, 1, "27.1").map(|r| r.build), Some(4));
        assert_eq!(
            newest(&feed, 4, "27.1"),
            None,
            "the same build is not newer"
        );
        assert_eq!(
            newest(&feed, 9, "27.1"),
            None,
            "an older build is never offered"
        );
        assert_eq!(newest(&feed, 1, "30.0").map(|r| r.build), Some(5));
        assert_eq!(newest(&feed, 1, "12.7"), None);
    }

    #[test]
    fn the_list_reads_without_optional_fields() {
        let feed = parse_feed(
            r#"{"releases":[{"version":"0.2.0","build":2,"url":"https://x/K.zip",
                "size":3,"sha256":"ab","signature":"cd","notes":{"it":"Novità"}}]}"#,
        )
        .unwrap();
        let only = &feed.releases[0];
        assert_eq!(only.minimum_system, "13.0");
        assert_eq!(only.notes.it, "Novità");
        assert_eq!(only.notes.en, "");
        assert!(parse_feed("<html>").is_err());
    }

    #[test]
    fn a_download_is_kept_only_when_it_is_the_signed_release() {
        let dir = tempfile::tempdir().unwrap();
        let staging = dir.path().join("staging");
        let key = signing_key();
        let good = published(dir.path(), b"the new keel", 2, &key);

        let zip = download(&good, &staging, &key.verifying_key()).unwrap();
        assert_eq!(std::fs::read(&zip).unwrap(), b"the new keel");
        assert!(!partial_file(&staging, &good).exists());

        let stranger = SigningKey::from_bytes(&[9; 32]);
        let forged = published(dir.path(), b"not keel", 3, &stranger);
        let error = download(&forged, &staging, &key.verifying_key()).unwrap_err();
        assert!(error.to_string().contains("not signed"), "{error}");
        assert!(!staging.join("Keel-3.zip").exists());

        let changed = Release {
            sha256: "00".repeat(32),
            ..published(dir.path(), b"changed", 4, &key)
        };
        assert!(download(&changed, &staging, &key.verifying_key()).is_err());

        let short = Release {
            size: 99,
            ..published(dir.path(), b"short", 5, &key)
        };
        assert!(download(&short, &staging, &key.verifying_key()).is_err());

        let missing = Release {
            url: format!("file://{}/nothing.zip", dir.path().display()),
            ..good.clone()
        };
        assert!(download(&missing, &staging, &key.verifying_key()).is_err());
    }

    #[test]
    fn only_keel_of_the_promised_build_is_unpacked() {
        let dir = tempfile::tempdir().unwrap();
        let staging = dir.path().join("staging");
        let make = |name: &str, id: &str, build: u64| {
            let source = dir.path().join(name);
            let app = fake_app(&source, id, build);
            zip_of(&app, &source.join("Keel.zip"));
            source.join("Keel.zip")
        };

        let zip = make("good", BUNDLE_ID, 2);
        let app = unpack(&zip, &staging, &release(2, "13.0")).unwrap();
        assert!(app.ends_with("Keel-2/Keel.app"));
        assert!(app.join("Contents/MacOS/keel").exists());

        let zip = make("other", "com.example.other", 3);
        let error = unpack(&zip, &staging, &release(3, "13.0")).unwrap_err();
        assert!(error.to_string().contains("another app"), "{error}");

        let zip = make("older", BUNDLE_ID, 3);
        let error = unpack(&zip, &staging, &release(4, "13.0")).unwrap_err();
        assert!(error.to_string().contains("build 3"), "{error}");
    }

    #[test]
    fn the_new_app_takes_the_old_ones_place_after_keel_quits() {
        let dir = tempfile::tempdir().unwrap();
        let applications = dir.path().join("Applications");
        std::fs::create_dir_all(applications.join("Keel.app")).unwrap();
        std::fs::write(applications.join("Keel.app/old"), b"").unwrap();
        let staged = staging_dir(&applications.join("Keel.app")).join("Keel-2/Keel.app");
        std::fs::create_dir_all(&staged).unwrap();
        std::fs::write(staged.join("new"), b"").unwrap();

        // A Keel that has quit already.
        let mut quit = Command::new("/usr/bin/true").spawn().unwrap();
        let pid = quit.id() as i32;
        quit.wait().unwrap();

        let app = applications.join("Keel.app");
        finish(pid, &staged, &app, Duration::from_secs(5)).unwrap();
        assert!(app.join("new").exists());
        assert!(!app.join("old").exists());
        let left: Vec<_> = std::fs::read_dir(&applications)
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(left, ["Keel.app"], "no backup or staging is left");

        // With the new version gone, the old one stays.
        let missing = staging_dir(&app).join("Keel-3/Keel.app");
        assert!(finish(pid, &missing, &app, Duration::from_secs(5)).is_err());
        assert!(app.join("new").exists());
    }

    #[test]
    fn a_running_keel_is_waited_for() {
        let dir = tempfile::tempdir().unwrap();
        let app = dir.path().join("Keel.app");
        std::fs::create_dir_all(&app).unwrap();
        let staged = dir.path().join("new/Keel.app");
        std::fs::create_dir_all(&staged).unwrap();
        let mut running = Command::new("/bin/sleep").arg("30").spawn().unwrap();
        let error = finish(
            running.id() as i32,
            &staged,
            &app,
            Duration::from_millis(300),
        )
        .unwrap_err();
        running.kill().unwrap();
        running.wait().unwrap();
        assert!(error.to_string().contains("did not quit"));
        assert!(staged.exists(), "nothing moved while Keel was open");
    }

    #[test]
    fn the_app_is_found_from_its_executable() {
        assert_eq!(
            running_app(Path::new("/Applications/Keel.app/Contents/MacOS/keel")),
            Some(PathBuf::from("/Applications/Keel.app"))
        );
        assert_eq!(
            running_app(Path::new("/Users/me/Keel/target/debug/keel")),
            None
        );
    }

    #[test]
    fn the_outcome_is_read_once() {
        let dir = tempfile::tempdir().unwrap();
        let outcome = Outcome {
            version: "0.2.0".to_owned(),
            error: None,
        };
        write_outcome(dir.path(), &outcome).unwrap();
        assert_eq!(take_outcome(dir.path()), Some(outcome));
        assert_eq!(take_outcome(dir.path()), None);
    }

    #[test]
    fn keys_and_signatures_read_from_hex() {
        let key = signing_key().verifying_key();
        assert_eq!(parse_key(&encode_hex(key.as_bytes())).unwrap(), key);
        assert!(parse_key("abc").is_err());
        assert!(parse_key(&"zz".repeat(32)).is_err());
    }
}
