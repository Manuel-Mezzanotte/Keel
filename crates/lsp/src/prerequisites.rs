//! Programs some servers need and Keel does not install itself, because
//! they belong to the whole system: Java, .NET and the Xcode tools. Keel
//! finds them, and when one is missing says which and how to install it.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::servers::which_in;

/// Something a server needs on the machine.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Prerequisite {
    /// A Java runtime of at least this version.
    Java { min: u32 },
    /// The .NET SDK of at least this version.
    Dotnet { min: u32 },
    /// The Command Line Tools of Xcode (`clangd`, `clang-format`...).
    XcodeTools,
}

impl Prerequisite {
    /// The name shown to the user, such as "Java 21".
    pub fn name(&self) -> String {
        match self {
            Prerequisite::Java { min } => format!("Java {min}"),
            Prerequisite::Dotnet { min } => format!(".NET {min}"),
            Prerequisite::XcodeTools => "Xcode Command Line Tools".to_owned(),
        }
    }

    /// What to run in a terminal to install it.
    pub fn install_command(&self) -> String {
        match self {
            // Keel finds Homebrew's Java without the link Homebrew suggests.
            Prerequisite::Java { min } => format!("brew install openjdk@{min}"),
            Prerequisite::Dotnet { .. } => "brew install --cask dotnet-sdk".to_owned(),
            Prerequisite::XcodeTools => "xcode-select --install".to_owned(),
        }
    }

    /// Looks for it; on success returns the variables a server needs to
    /// use it, such as `JAVA_HOME`.
    pub fn check(&self, env: &HashMap<String, String>) -> Result<Found, Missing> {
        let found = match *self {
            Prerequisite::Java { min } => find_java(min, env),
            Prerequisite::Dotnet { min } => find_dotnet(min, env),
            Prerequisite::XcodeTools => find_xcode_tools(),
        };
        found.ok_or(Missing {
            prerequisite: *self,
        })
    }
}

/// A prerequisite that was found: the variables to give the server.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Found {
    pub env: Vec<(String, String)>,
}

/// The error of a server that cannot start without `prerequisite`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Missing {
    pub prerequisite: Prerequisite,
}

impl std::fmt::Display for Missing {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} is not installed (install it with `{}`)",
            self.prerequisite.name(),
            self.prerequisite.install_command()
        )
    }
}

impl std::error::Error for Missing {}

/// Checks every prerequisite; returns the variables of all of them.
pub fn check_all(
    needs: &[Prerequisite],
    env: &HashMap<String, String>,
) -> Result<Vec<(String, String)>, Missing> {
    let mut variables = Vec::new();
    for prerequisite in needs {
        variables.extend(prerequisite.check(env)?.env);
    }
    Ok(variables)
}

/// Puts `dir` in front of the `PATH` of `env`.
fn path_with(dir: &Path, env: &HashMap<String, String>) -> (String, String) {
    let rest = env.get("PATH").map(String::as_str).unwrap_or_default();
    ("PATH".to_owned(), format!("{}:{rest}", dir.display()))
}

/// Finds a Java home of at least version `min`: `JAVA_HOME`, the JDKs macOS
/// knows, Homebrew's, then the `java` on the `PATH`.
fn find_java(min: u32, env: &HashMap<String, String>) -> Option<Found> {
    let mut homes: Vec<PathBuf> = Vec::new();
    if let Some(home) = env.get("JAVA_HOME") {
        homes.push(PathBuf::from(home));
    }
    if let Some(home) =
        output_of(Command::new("/usr/libexec/java_home").args(["-v", &format!("{min}+")]))
    {
        homes.push(PathBuf::from(home.trim()));
    }
    for prefix in ["/opt/homebrew/opt", "/usr/local/opt"] {
        let Ok(entries) = std::fs::read_dir(prefix) else {
            continue;
        };
        let mut found: Vec<PathBuf> = entries
            .flatten()
            .filter(|entry| entry.file_name().to_string_lossy().starts_with("openjdk"))
            .map(|entry| entry.path().join("libexec/openjdk.jdk/Contents/Home"))
            .collect();
        found.sort();
        homes.extend(found.into_iter().rev());
    }
    if let Some(java) = which_in("java", env)
        && let Some(home) = java_home_of(&java)
    {
        homes.push(home);
    }
    homes.into_iter().find_map(|home| {
        let java = home.join("bin/java");
        let version = java_version(&java)?;
        (version >= min).then(|| Found {
            env: vec![
                ("JAVA_HOME".to_owned(), home.to_string_lossy().into_owned()),
                path_with(&home.join("bin"), env),
            ],
        })
    })
}

/// The major version of a Java program, from `java -version`: "21.0.10"
/// is 21, the old "1.8.0" is 8.
fn java_version(java: &Path) -> Option<u32> {
    if !java.exists() {
        return None;
    }
    let output = Command::new(java)
        .arg("-version")
        .stdin(Stdio::null())
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    parse_java_version(&String::from_utf8_lossy(&output.stderr))
}

fn parse_java_version(text: &str) -> Option<u32> {
    let quoted = text.split('"').nth(1)?;
    let mut parts = quoted.split(['.', '-', '+', '_']);
    let first: u32 = parts.next()?.parse().ok()?;
    if first == 1 {
        parts.next()?.parse().ok()
    } else {
        Some(first)
    }
}

/// The home of the Java that `java` runs, which may be behind links.
fn java_home_of(java: &Path) -> Option<PathBuf> {
    let output = Command::new(java)
        .args(["-XshowSettings:properties", "-version"])
        .stdin(Stdio::null())
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&output.stderr);
    text.lines()
        .find_map(|line| line.trim().strip_prefix("java.home = "))
        .map(PathBuf::from)
}

/// Finds `dotnet` with an SDK of at least version `min`.
fn find_dotnet(min: u32, env: &HashMap<String, String>) -> Option<Found> {
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Some(root) = env.get("DOTNET_ROOT") {
        candidates.push(Path::new(root).join("dotnet"));
    }
    candidates.extend(which_in("dotnet", env));
    candidates.push(PathBuf::from("/usr/local/share/dotnet/dotnet"));
    if let Some(home) = env.get("HOME") {
        candidates.push(Path::new(home).join(".dotnet/dotnet"));
    }
    candidates.into_iter().find_map(|dotnet| {
        if !dotnet.exists() {
            return None;
        }
        let sdks = output_of(Command::new(&dotnet).arg("--list-sdks").envs(env))?;
        let newest = newest_sdk(&sdks)?;
        if newest < min {
            return None;
        }
        // `dotnet` may be a link, as Homebrew's is: the root is where it points.
        let real = std::fs::canonicalize(&dotnet).unwrap_or(dotnet);
        let root = real.parent()?.to_path_buf();
        Some(Found {
            env: vec![
                (
                    "DOTNET_ROOT".to_owned(),
                    root.to_string_lossy().into_owned(),
                ),
                path_with(&root, env),
            ],
        })
    })
}

/// The newest major version in `dotnet --list-sdks`, whose lines look like
/// `8.0.404 [/usr/local/share/dotnet/sdk]`.
fn newest_sdk(list: &str) -> Option<u32> {
    list.lines()
        .filter_map(|line| line.split('.').next()?.trim().parse().ok())
        .max()
}

/// The Command Line Tools are installed when `xcode-select` names a folder
/// that exists. Without them `/usr/bin/clangd` only asks to install them.
fn find_xcode_tools() -> Option<Found> {
    let path = output_of(Command::new("/usr/bin/xcode-select").arg("-p"))?;
    Path::new(path.trim()).exists().then(Found::default)
}

/// What a command prints, if it succeeds.
fn output_of(command: &mut Command) -> Option<String> {
    let output = command
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn java_versions() {
        assert_eq!(
            parse_java_version("openjdk version \"21.0.10\" 2026-01-20\n"),
            Some(21)
        );
        assert_eq!(parse_java_version("java version \"1.8.0_402\""), Some(8));
        assert_eq!(
            parse_java_version("openjdk version \"25\" 2025-09-16"),
            Some(25)
        );
        assert_eq!(parse_java_version("openjdk version \"17-ea\""), Some(17));
        assert_eq!(
            parse_java_version("The operation couldn’t be completed."),
            None
        );
    }

    #[test]
    fn dotnet_sdks() {
        let list =
            "8.0.404 [/usr/local/share/dotnet/sdk]\n10.0.100 [/usr/local/share/dotnet/sdk]\n";
        assert_eq!(newest_sdk(list), Some(10));
        assert_eq!(newest_sdk(""), None);
    }

    #[test]
    fn a_missing_prerequisite_says_how_to_install_it() {
        let missing = Missing {
            prerequisite: Prerequisite::Java { min: 21 },
        };
        assert_eq!(
            missing.to_string(),
            "Java 21 is not installed (install it with `brew install openjdk@21`)"
        );
        let error = anyhow::Error::from(missing);
        assert_eq!(error.downcast_ref::<Missing>(), Some(&missing));
    }

    #[test]
    fn a_java_home_puts_its_java_first_on_the_path() {
        let env = HashMap::from([("PATH".to_owned(), "/usr/bin".to_owned())]);
        assert_eq!(
            path_with(Path::new("/jdk/bin"), &env),
            ("PATH".to_owned(), "/jdk/bin:/usr/bin".to_owned())
        );
    }

    /// Finds the Java and .NET of this Mac, if any.
    /// Run with `cargo test -p lsp -- --ignored --nocapture finds_this_mac`.
    #[test]
    #[ignore]
    fn finds_this_macs_prerequisites() {
        let env = crate::servers::login_environment();
        for prerequisite in [
            Prerequisite::Java { min: 21 },
            Prerequisite::Dotnet { min: 8 },
            Prerequisite::XcodeTools,
        ] {
            let start = std::time::Instant::now();
            println!(
                "{}: {:?} in {:?}",
                prerequisite.name(),
                prerequisite.check(env),
                start.elapsed()
            );
        }
    }
}
