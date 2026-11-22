//! Where the debug adapters are: `lldb-dap` from Xcode for Rust, C, C++
//! and Swift, `debugpy` for Python, `js-debug` for Node and TypeScript.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use lsp::servers::which_in;

use crate::client::AdapterCommand;

/// A debugger Keel knows.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Adapter {
    Lldb,
    Python,
    Node,
}

/// What an adapter needs to be there.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Missing {
    /// Keel can install it, when asked: what it is and where from.
    Installable {
        name: &'static str,
        from: &'static str,
    },
    /// The user has to: the command or the program it takes.
    ByHand {
        name: &'static str,
        how: &'static str,
    },
}

/// js-debug's release, as its DAP server alone.
pub const JS_DEBUG_VERSION: &str = "1.140.1";
pub const JS_DEBUG_URL: &str = "https://github.com/microsoft/vscode-js-debug/releases/download/v1.140.1/js-debug-dap-v1.140.1.tar.gz";
/// From the release's own page.
pub const JS_DEBUG_SHA256: &str =
    "c72fab31bcbfdce6ad1f42adfd28eff8d4681dd3b03b5e2e6f9e72e6f9927faf";
pub const DEBUGPY_VERSION: &str = "1.8.20";

impl Adapter {
    /// The adapter for a `type` of `launch.json`.
    pub fn of_type(kind: &str) -> Option<Self> {
        Some(match kind {
            "lldb" | "lldb-dap" | "cppdbg" | "codelldb" | "rust" | "swift" | "swift-lldb" => {
                Adapter::Lldb
            }
            "python" | "debugpy" => Adapter::Python,
            "node" | "pwa-node" | "node-terminal" | "chrome" | "pwa-chrome" | "msedge"
            | "pwa-msedge" | "extensionHost" => Adapter::Node,
            _ => return None,
        })
    }

    /// The adapter for files of a language.
    pub fn of_language(language: &str) -> Option<Self> {
        Some(match language {
            "rust" | "c" | "cpp" | "objective-c" | "objective-cpp" | "swift" | "zig" => {
                Adapter::Lldb
            }
            "python" => Adapter::Python,
            "javascript" | "typescript" | "javascriptreact" | "typescriptreact" => Adapter::Node,
            _ => return None,
        })
    }
}

/// `lldb-dap`, on the `PATH` or in Xcode's tools.
pub fn find_lldb(env: &HashMap<String, String>) -> Result<AdapterCommand, Missing> {
    let program = which_in("lldb-dap", env).or_else(|| {
        let output = Command::new("/usr/bin/xcrun")
            .args(["--find", "lldb-dap"])
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output()
            .ok()?;
        let path = PathBuf::from(String::from_utf8_lossy(&output.stdout).trim());
        (output.status.success() && path.exists()).then_some(path)
    });
    match program {
        Some(program) => Ok(AdapterCommand {
            program,
            ..AdapterCommand::default()
        }),
        None => Err(Missing::ByHand {
            name: "lldb-dap",
            how: "xcode-select --install",
        }),
    }
}

/// The Python of a project: its virtual environment's, the shell's active
/// one, or the first on the `PATH`.
pub fn python_of(root: &Path, env: &HashMap<String, String>) -> Option<PathBuf> {
    let in_folder = |folder: PathBuf| {
        let program = folder.join("bin/python");
        program.exists().then_some(program)
    };
    [".venv", "venv", "env"]
        .iter()
        .find_map(|name| in_folder(root.join(name)))
        .or_else(|| in_folder(PathBuf::from(env.get("VIRTUAL_ENV")?)))
        .or_else(|| which_in("python3", env))
        .or_else(|| which_in("python", env))
}

/// Where Keel installs debugpy and js-debug.
pub fn debugpy_dir(adapters: &Path) -> PathBuf {
    adapters.join("debugpy")
}

pub fn js_debug_dir(adapters: &Path) -> PathBuf {
    adapters.join(format!("js-debug-{JS_DEBUG_VERSION}"))
}

/// `debugpy` for a Python: its own, the one Keel installed, or the copy
/// in an editor's Python extension, newest first.
pub fn find_debugpy(
    python: &Path,
    adapters: &Path,
    home: Option<&Path>,
    env: &HashMap<String, String>,
) -> Result<AdapterCommand, Missing> {
    let run = |args: Vec<String>| AdapterCommand {
        program: python.to_path_buf(),
        args,
        ..AdapterCommand::default()
    };
    let own = Command::new(python)
        .args(["-c", "import debugpy.adapter"])
        .envs(env)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success());
    if own {
        return Ok(run(vec!["-m".into(), "debugpy.adapter".into()]));
    }
    let mut folders = vec![debugpy_dir(adapters)];
    for editor in [
        ".vscode",
        ".cursor",
        ".vscode-insiders",
        ".windsurf",
        ".antigravity",
    ] {
        let extensions = home.map(|home| home.join(editor).join("extensions"));
        let Some(Ok(entries)) = extensions.map(std::fs::read_dir) else {
            continue;
        };
        let mut found: Vec<PathBuf> = entries
            .flatten()
            .filter(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with("ms-python.debugpy-")
            })
            .map(|entry| entry.path().join("bundled/libs"))
            .collect();
        found.sort();
        folders.extend(found.into_iter().rev());
    }
    folders
        .into_iter()
        .map(|folder| folder.join("debugpy/adapter"))
        .find(|adapter| adapter.join("__main__.py").is_file())
        .map(|adapter| run(vec![adapter.to_string_lossy().into_owned()]))
        .ok_or(Missing::Installable {
            name: "debugpy",
            from: "PyPI",
        })
}

/// js-debug's server, run by the user's Node.
pub fn find_js_debug(
    adapters: &Path,
    env: &HashMap<String, String>,
) -> Result<AdapterCommand, Missing> {
    let Some(node) = which_in("node", env) else {
        return Err(Missing::ByHand {
            name: "Node.js",
            how: "https://nodejs.org",
        });
    };
    let server = js_debug_dir(adapters).join("js-debug/src/dapDebugServer.js");
    if !server.is_file() {
        return Err(Missing::Installable {
            name: "js-debug",
            from: "GitHub",
        });
    }
    Ok(AdapterCommand {
        program: node,
        args: vec![
            server.to_string_lossy().into_owned(),
            "0".into(),
            "127.0.0.1".into(),
        ],
        ..AdapterCommand::default()
    })
}

/// Installs debugpy or js-debug in Keel's folder. Slow: run it away from
/// the interface. Returns what the installer printed.
pub fn install(
    adapter: Adapter,
    python: Option<&Path>,
    adapters: &Path,
    env: &HashMap<String, String>,
) -> anyhow::Result<String> {
    use anyhow::{Context as _, bail};
    use sha2::{Digest as _, Sha256};

    std::fs::create_dir_all(adapters)?;
    let run = |program: &Path, args: &[String]| -> anyhow::Result<String> {
        let output = Command::new(program)
            .args(args)
            .envs(env)
            .stdin(Stdio::null())
            .output()
            .with_context(|| format!("running {}", program.display()))?;
        let text = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        if !output.status.success() {
            bail!("{}", text.trim());
        }
        Ok(text)
    };
    match adapter {
        Adapter::Python => {
            let python = python.context("no Python on this Mac")?;
            run(
                python,
                &[
                    "-m".into(),
                    "pip".into(),
                    "install".into(),
                    "--disable-pip-version-check".into(),
                    "--upgrade".into(),
                    "--target".into(),
                    debugpy_dir(adapters).to_string_lossy().into_owned(),
                    format!("debugpy=={DEBUGPY_VERSION}"),
                ],
            )
        }
        Adapter::Node => {
            let folder = js_debug_dir(adapters);
            let part = adapters.join("js-debug.part");
            run(
                Path::new("/usr/bin/curl"),
                &[
                    "--fail".into(),
                    "--silent".into(),
                    "--show-error".into(),
                    "--location".into(),
                    "--output".into(),
                    part.to_string_lossy().into_owned(),
                    JS_DEBUG_URL.into(),
                ],
            )?;
            let bytes = std::fs::read(&part)?;
            let sum: String = Sha256::digest(&bytes)
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect();
            if sum != JS_DEBUG_SHA256 {
                let _ = std::fs::remove_file(&part);
                bail!("js-debug is not the file it should be (SHA-256 {sum})");
            }
            std::fs::create_dir_all(&folder)?;
            let text = run(
                Path::new("/usr/bin/tar"),
                &[
                    "-xzf".into(),
                    part.to_string_lossy().into_owned(),
                    "-C".into(),
                    folder.to_string_lossy().into_owned(),
                ],
            )?;
            let _ = std::fs::remove_file(&part);
            Ok(text)
        }
        Adapter::Lldb => bail!("lldb-dap comes with Xcode"),
    }
}

/// What makes LLDB show Rust's own types, from the project's toolchain.
pub fn rust_formatters(root: &Path, env: &HashMap<String, String>) -> Vec<String> {
    let Some(rustc) = which_in("rustc", env) else {
        return Vec::new();
    };
    let output = Command::new(rustc)
        .args(["--print", "sysroot"])
        .current_dir(root)
        .envs(env)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output();
    let Ok(output) = output else {
        return Vec::new();
    };
    let folder =
        PathBuf::from(String::from_utf8_lossy(&output.stdout).trim()).join("lib/rustlib/etc");
    let mut commands = Vec::new();
    let lookup = folder.join("lldb_lookup.py");
    if lookup.is_file() {
        commands.push(format!("command script import \"{}\"", lookup.display()));
    }
    let own = folder.join("lldb_commands");
    if own.is_file() {
        commands.push(format!("command source -s true \"{}\"", own.display()));
    }
    commands
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn types_and_languages() {
        assert_eq!(Adapter::of_type("pwa-node"), Some(Adapter::Node));
        assert_eq!(Adapter::of_type("debugpy"), Some(Adapter::Python));
        assert_eq!(Adapter::of_type("cppdbg"), Some(Adapter::Lldb));
        assert_eq!(Adapter::of_type("go"), None);
        assert_eq!(Adapter::of_language("typescript"), Some(Adapter::Node));
        assert_eq!(Adapter::of_language("markdown"), None);
    }

    #[test]
    fn a_projects_python_and_its_debugpy() {
        let folder = tempfile::tempdir().unwrap();
        let root = folder.path();
        let env = HashMap::from([("PATH".to_owned(), String::new())]);
        assert_eq!(python_of(root, &env), None);
        std::fs::create_dir_all(root.join(".venv/bin")).unwrap();
        std::fs::write(root.join(".venv/bin/python"), "").unwrap();
        let python = python_of(root, &env).unwrap();
        assert!(python.ends_with(".venv/bin/python"));
        // None of its own, none in Keel's folder: one to install.
        let adapters = root.join("adapters");
        let missing = find_debugpy(&python, &adapters, Some(root), &env).unwrap_err();
        assert!(matches!(
            missing,
            Missing::Installable {
                name: "debugpy",
                ..
            }
        ));
        // The newest in an editor's extensions.
        for version in ["2025.1.0", "2026.6.0"] {
            let libs = root.join(format!(
                ".vscode/extensions/ms-python.debugpy-{version}/bundled/libs/debugpy/adapter"
            ));
            std::fs::create_dir_all(&libs).unwrap();
            std::fs::write(libs.join("__main__.py"), "").unwrap();
        }
        let found = find_debugpy(&python, &adapters, Some(root), &env).unwrap();
        assert!(found.args[0].contains("2026.6.0"), "{found:?}");
        assert!(find_js_debug(&adapters, &env).is_err());
    }
}
