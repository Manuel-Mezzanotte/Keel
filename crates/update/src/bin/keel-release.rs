//! What `scripts/release.sh` runs to publish Keel:
//!
//! - `keel-release key` prints the public half of the release key, making
//!   the pair the first time; the private half lives in the login keychain;
//! - `keel-release stage …` fills the folder that goes on Keel's site;
//! - `keel-release check FEED` downloads the newest published release and
//!   checks it as Keel would.

use std::path::PathBuf;
use std::process::{Command, ExitCode};

use anyhow::{Context as _, Result, bail};
use update::publish;

/// Where the private key is kept in the login keychain.
const SERVICE: &str = "Keel release signing key";
const ACCOUNT: &str = "keel";

fn main() -> ExitCode {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let result = match arguments.first().map(String::as_str) {
        Some("key") => key().map(|key| println!("{}", publish::public_hex(&key))),
        Some("stage") => stage(&arguments[1..]),
        Some("check") => check(&arguments[1..]),
        _ => {
            eprintln!(
                "usage: keel-release key\n       keel-release stage --out DIR --version V --build N --zip ZIP --dmg DMG --notes FILE --base URL --installer FILE [--previous FEED] [--date DAY]\n       keel-release check FEED_URL"
            );
            return ExitCode::from(2);
        }
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("keel-release: {error:#}");
            ExitCode::FAILURE
        }
    }
}

/// The release key from the keychain, made and kept there the first time.
fn key() -> Result<ed25519_dalek::SigningKey> {
    let found = Command::new("/usr/bin/security")
        .args(["find-generic-password", "-s", SERVICE, "-a", ACCOUNT, "-w"])
        .output()
        .context("security did not start")?;
    if found.status.success() {
        return publish::signing_key(String::from_utf8_lossy(&found.stdout).trim());
    }
    let key = publish::new_signing_key()?;
    let added = Command::new("/usr/bin/security")
        .args(["add-generic-password", "-s", SERVICE, "-a", ACCOUNT])
        .args([
            "-l",
            SERVICE,
            "-j",
            "Signs Keel's releases. Keep a copy outside this Mac.",
        ])
        .args(["-w", &publish::key_hex(&key)])
        .status()
        .context("security did not start")?;
    if !added.success() {
        bail!("the new key could not be kept in the keychain");
    }
    eprintln!(
        "keel-release: made a new release key, kept in the login keychain as «{SERVICE}». Keep a copy outside this Mac: without it no Keel can be updated."
    );
    Ok(key)
}

fn stage(arguments: &[String]) -> Result<()> {
    let value = |name: &str| -> Result<String> {
        arguments
            .iter()
            .position(|argument| argument == name)
            .and_then(|at| arguments.get(at + 1))
            .cloned()
            .with_context(|| format!("{name} is missing"))
    };
    let optional = |name: &str| value(name).ok();
    let out = PathBuf::from(value("--out")?);
    let notes = std::fs::read_to_string(value("--notes")?).context("the notes file")?;
    let installer = std::fs::read_to_string(value("--installer")?).context("the install script")?;
    let previous = match optional("--previous") {
        Some(file) => Some(update::parse_feed(&std::fs::read_to_string(file)?)?),
        None => None,
    };
    let date = optional("--date").unwrap_or_else(today);
    let key = key()?;
    let keep = publish::stage(&publish::Stage {
        out: &out,
        version: &value("--version")?,
        build: value("--build")?.parse().context("--build is a number")?,
        date: &date,
        zip: &PathBuf::from(value("--zip")?),
        dmg: &PathBuf::from(value("--dmg")?),
        notes: publish::parse_notes(&notes)?,
        base: &value("--base")?,
        installer: &installer,
        previous,
        key: &key,
    })?;
    for name in keep {
        println!("{name}");
    }
    Ok(())
}

/// Downloads the newest published release and checks it as Keel would,
/// with the key of this Mac.
fn check(arguments: &[String]) -> Result<()> {
    let feed_url = arguments.first().context("the list's address is missing")?;
    let feed = update::fetch(feed_url)?;
    let release = update::newest(&feed, 0, "99")
        .context("the list has no release")?
        .clone();
    let public = key()?.verifying_key();
    let folder = std::env::temp_dir().join(format!("keel-release-check-{}", std::process::id()));
    let staging = folder.join(".keel-update");
    let result = update::download(&release, &staging, &public)
        .and_then(|zip| update::unpack(&zip, &staging, &release));
    let _ = std::fs::remove_dir_all(&folder);
    result?;
    println!(
        "Keel {} (build {}) downloads and checks.",
        release.version, release.build
    );
    Ok(())
}

fn today() -> String {
    let output = Command::new("/bin/date").arg("+%Y-%m-%d").output();
    output
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_owned())
        .unwrap_or_default()
}
