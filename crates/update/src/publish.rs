//! The other side of updates: what `scripts/release.sh` publishes. From a
//! built `.zip` and disk image it makes the folder that goes on Keel's site:
//! the two files, the list of releases with the new one signed, and the
//! install script with the disk image's address and SHA-256.

use std::path::Path;

use anyhow::{Context as _, Result, ensure};
use ed25519_dalek::{Signer as _, SigningKey};
use sha2::{Digest as _, Sha256};

use crate::{DiskImage, Feed, Notes, Release, decode_hex, encode_hex};

/// What a release is made of.
pub struct Stage<'a> {
    /// The folder to fill, as it will be on the site.
    pub out: &'a Path,
    pub version: &'a str,
    pub build: u64,
    pub date: &'a str,
    pub zip: &'a Path,
    pub dmg: &'a Path,
    pub notes: Notes,
    /// The site's address, such as `https://keel.mezzalab.eu`.
    pub base: &'a str,
    /// `scripts/install.sh`, with `@VERSION@`, `@URL@` and `@SHA256@`.
    pub installer: &'a str,
    /// The list published until now, if any: its newest release is kept.
    pub previous: Option<Feed>,
    pub key: &'a SigningKey,
}

/// Fills `stage.out`: `releases/Keel-VERSION.zip` and `.dmg`,
/// `updates.json` and `install.sh`. Returns the names of the files the
/// site keeps in `releases/`: the new ones and the previous version's.
pub fn stage(stage: &Stage) -> Result<Vec<String>> {
    let releases = stage.out.join("releases");
    std::fs::create_dir_all(&releases)?;
    let zip_name = format!("Keel-{}.zip", stage.version);
    let dmg_name = format!("Keel-{}.dmg", stage.version);
    std::fs::copy(stage.zip, releases.join(&zip_name)).context("the .zip is missing")?;
    std::fs::copy(stage.dmg, releases.join(&dmg_name)).context("the disk image is missing")?;

    let bytes = std::fs::read(stage.zip)?;
    let image = std::fs::read(stage.dmg)?;
    let base = stage.base.trim_end_matches('/');
    let release = Release {
        version: stage.version.to_owned(),
        build: stage.build,
        date: stage.date.to_owned(),
        minimum_system: "13.0".to_owned(),
        notes: stage.notes.clone(),
        url: format!("{base}/releases/{zip_name}"),
        size: bytes.len() as u64,
        sha256: encode_hex(&Sha256::digest(&bytes)),
        signature: encode_hex(&stage.key.sign(&bytes).to_bytes()),
        disk_image: Some(DiskImage {
            url: format!("{base}/releases/{dmg_name}"),
            size: image.len() as u64,
            sha256: encode_hex(&Sha256::digest(&image)),
        }),
    };
    // The previous version stays, for whoever needs to go back to it.
    let previous = stage
        .previous
        .iter()
        .flat_map(|feed| feed.releases.iter())
        .filter(|old| old.build < stage.build)
        .max_by_key(|old| old.build)
        .cloned();
    ensure!(
        stage
            .previous
            .iter()
            .flat_map(|feed| feed.releases.iter())
            .all(|old| old.build < stage.build),
        "build {} is not newer than the ones published",
        stage.build
    );
    let mut keep = vec![zip_name, dmg_name.clone()];
    if let Some(old) = &previous {
        let name = old.url.rsplit('/').next().unwrap_or_default().to_owned();
        keep.push(name.replace(".zip", ".dmg"));
        keep.push(name);
    }
    let feed = Feed {
        releases: std::iter::once(release).chain(previous).collect(),
    };
    std::fs::write(
        stage.out.join("updates.json"),
        serde_json::to_string_pretty(&feed)? + "\n",
    )?;

    let installer = stage
        .installer
        .replace("@VERSION@", stage.version)
        .replace("@URL@", &format!("{base}/releases/{dmg_name}"))
        .replace("@SHA256@", &encode_hex(&Sha256::digest(&image)));
    ensure!(
        ["@VERSION@", "@URL@", "@SHA256@"]
            .iter()
            .all(|placeholder| !installer.contains(placeholder)),
        "the install script kept a placeholder"
    );
    std::fs::write(stage.out.join("install.sh"), installer)?;
    Ok(keep)
}

/// The notes of a release from a file with a `## it` and a `## en`
/// section.
pub fn parse_notes(text: &str) -> Result<Notes> {
    let mut notes = Notes::default();
    let mut current: Option<&mut String> = None;
    for line in text.lines() {
        match line.trim() {
            "## it" => current = Some(&mut notes.it),
            "## en" => current = Some(&mut notes.en),
            _ => {
                if let Some(section) = current.as_deref_mut() {
                    section.push_str(line);
                    section.push('\n');
                }
            }
        }
    }
    notes.it = notes.it.trim().to_owned();
    notes.en = notes.en.trim().to_owned();
    ensure!(
        !notes.it.is_empty() && !notes.en.is_empty(),
        "the notes need a `## it` and a `## en` section, both written"
    );
    Ok(notes)
}

/// A signing key from its 64 hex digits.
pub fn signing_key(hex: &str) -> Result<SigningKey> {
    let bytes: [u8; 32] = decode_hex(hex)?
        .try_into()
        .map_err(|_| anyhow::anyhow!("a signing key has 32 bytes"))?;
    Ok(SigningKey::from_bytes(&bytes))
}

/// A new signing key, from the system's randomness.
pub fn new_signing_key() -> Result<SigningKey> {
    use std::io::Read as _;
    let mut bytes = [0; 32];
    std::fs::File::open("/dev/urandom")?.read_exact(&mut bytes)?;
    Ok(SigningKey::from_bytes(&bytes))
}

pub fn key_hex(key: &SigningKey) -> String {
    encode_hex(&key.to_bytes())
}

pub fn public_hex(key: &SigningKey) -> String {
    encode_hex(key.verifying_key().as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{fake_app, signing_key as test_key, zip_of};
    use crate::{BUNDLE_ID, download, fetch, newest, unpack};

    const INSTALLER: &str = include_str!("../../../scripts/install.sh");

    fn disk_image(app: &Path, out: &Path) {
        let folder = out.with_extension("src");
        std::fs::create_dir_all(&folder).unwrap();
        crate::run(
            std::process::Command::new("/usr/bin/ditto")
                .arg(app)
                .arg(folder.join("Keel.app")),
        )
        .unwrap();
        crate::run(
            std::process::Command::new("/usr/bin/hdiutil")
                .args(["create", "-quiet", "-volname", "Keel", "-format", "UDZO"])
                .arg("-srcfolder")
                .arg(&folder)
                .arg(out),
        )
        .unwrap();
    }

    /// Release `build` of a fake Keel, staged into `site`.
    fn release(dir: &Path, site: &Path, build: u64, previous: Option<Feed>) -> Vec<String> {
        let source = dir.join(format!("build-{build}"));
        let app = fake_app(&source, BUNDLE_ID, build);
        let zip = source.join("Keel.zip");
        zip_of(&app, &zip);
        let dmg = source.join("Keel.dmg");
        disk_image(&app, &dmg);
        let version = format!("0.{build}.0");
        stage(&Stage {
            out: site,
            version: &version,
            build,
            date: "2026-11-24",
            zip: &zip,
            dmg: &dmg,
            notes: Notes {
                it: "Novità.".to_owned(),
                en: "News.".to_owned(),
            },
            base: &format!("file://{}", site.display()),
            installer: INSTALLER,
            previous,
            key: &test_key(),
        })
        .unwrap()
    }

    #[test]
    fn a_staged_release_is_found_downloaded_and_checked_by_keel() {
        let dir = tempfile::tempdir().unwrap();
        let site = dir.path().join("site");
        let keep = release(dir.path(), &site, 2, None);
        assert_eq!(keep, ["Keel-0.2.0.zip", "Keel-0.2.0.dmg"]);

        let feed = fetch(&format!("file://{}/updates.json", site.display())).unwrap();
        let found = newest(&feed, 1, "27.0").unwrap().clone();
        assert_eq!((found.version.as_str(), found.build), ("0.2.0", 2));
        let image = found.disk_image.clone().unwrap();
        assert!(image.url.ends_with("/releases/Keel-0.2.0.dmg"));
        assert_eq!(
            image.size,
            std::fs::metadata(site.join("releases/Keel-0.2.0.dmg"))
                .unwrap()
                .len()
        );
        let staging = dir.path().join("Applications/.keel-update");
        let zip = download(&found, &staging, &test_key().verifying_key()).unwrap();
        let app = unpack(&zip, &staging, &found).unwrap();
        assert!(app.join("Contents/MacOS/keel").exists());

        // The next one keeps this one, and only this one.
        let previous = Some(feed);
        let keep = release(dir.path(), &site, 3, previous);
        assert_eq!(
            keep,
            [
                "Keel-0.3.0.zip",
                "Keel-0.3.0.dmg",
                "Keel-0.2.0.dmg",
                "Keel-0.2.0.zip"
            ]
        );
        let feed = fetch(&format!("file://{}/updates.json", site.display())).unwrap();
        let builds: Vec<u64> = feed.releases.iter().map(|release| release.build).collect();
        assert_eq!(builds, [3, 2]);
        let after_two = release(dir.path(), &site, 4, Some(feed));
        assert!(!after_two.contains(&"Keel-0.2.0.zip".to_owned()));
    }

    #[test]
    fn an_older_build_is_not_published() {
        let dir = tempfile::tempdir().unwrap();
        let site = dir.path().join("site");
        release(dir.path(), &site, 3, None);
        let feed = fetch(&format!("file://{}/updates.json", site.display())).unwrap();
        let source = dir.path().join("again");
        let app = fake_app(&source, BUNDLE_ID, 3);
        let zip = source.join("Keel.zip");
        zip_of(&app, &zip);
        let error = stage(&Stage {
            out: &site,
            version: "0.3.0",
            build: 3,
            date: "2026-11-24",
            zip: &zip,
            dmg: &zip,
            notes: Notes::default(),
            base: "https://keel.mezzalab.eu",
            installer: INSTALLER,
            previous: Some(feed),
            key: &test_key(),
        })
        .unwrap_err();
        assert!(error.to_string().contains("not newer"), "{error}");
    }

    /// The install script, run for real: it downloads the disk image,
    /// checks it, and puts Keel in the folder given.
    #[test]
    fn the_install_script_puts_keel_in_applications() {
        let dir = tempfile::tempdir().unwrap();
        let site = dir.path().join("site");
        release(dir.path(), &site, 2, None);
        let applications = dir.path().join("Applications");
        let install = |script: &Path| {
            std::process::Command::new("/bin/sh")
                .arg(script)
                .env("KEEL_INSTALL_DIR", &applications)
                .env("KEEL_INSTALL_OPEN", "0")
                .output()
                .unwrap()
        };

        let output = install(&site.join("install.sh"));
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let installed = applications.join("Keel.app/Contents/Info.plist");
        assert_eq!(
            crate::plist_value(&installed, "CFBundleVersion").unwrap(),
            "2"
        );
        // A second run replaces the app, and leaves nothing else.
        assert!(install(&site.join("install.sh")).status.success());
        let left: Vec<_> = std::fs::read_dir(&applications)
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(left, ["Keel.app"]);

        // A disk image that is not the published one installs nothing.
        let script = std::fs::read_to_string(site.join("install.sh")).unwrap();
        let expected = script
            .lines()
            .find_map(|line| line.strip_prefix("KEEL_SHA256=\""))
            .unwrap()
            .trim_end_matches('"')
            .to_owned();
        let forged = dir.path().join("forged.sh");
        std::fs::write(&forged, script.replace(&expected, &"0".repeat(64))).unwrap();
        std::fs::remove_dir_all(applications.join("Keel.app")).unwrap();
        let output = install(&forged);
        assert!(!output.status.success());
        assert!(String::from_utf8_lossy(&output.stderr).contains("SHA-256"));
        assert!(!applications.join("Keel.app").exists());
    }

    #[test]
    fn notes_have_both_languages() {
        let notes =
            parse_notes("## it\nUna novità.\n\nUn'altra.\n## en\nSomething new.\n").unwrap();
        assert_eq!(notes.it, "Una novità.\n\nUn'altra.");
        assert_eq!(notes.en, "Something new.");
        assert!(parse_notes("## it\nSolo italiano.\n").is_err());
    }

    #[test]
    fn keys_go_to_hex_and_back() {
        let key = new_signing_key().unwrap();
        let again = signing_key(&key_hex(&key)).unwrap();
        assert_eq!(public_hex(&again), public_hex(&key));
        assert_ne!(key_hex(&key), key_hex(&new_signing_key().unwrap()));
    }
}
