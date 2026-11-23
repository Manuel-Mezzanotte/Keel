//! Command-line arguments: `keel [PATH]`.

use std::path::PathBuf;

/// What the user asked for on the command line.
#[derive(Debug, PartialEq, Eq)]
pub enum Command {
    /// Open a window, on folders and files, or on nothing.
    Open(Vec<PathBuf>),
    /// Print the version and exit.
    Version,
    /// Print the usage and exit.
    Help,
    /// Install a VSIX package and exit.
    InstallExtension(PathBuf),
    /// Not for people: run by Keel as it quits to install an update,
    /// `--finish-update PID STAGED APP`.
    FinishUpdate {
        pid: i32,
        staged: PathBuf,
        app: PathBuf,
    },
}

/// Usage text printed by `keel --help` and after a wrong invocation.
pub const USAGE: &str = "\
Usage: keel [PATH...]

Opens each PATH in Keel: a folder as a project, a file in its project.
`keel .` opens the current folder; `keel a.ts b.ts` opens both files. A
file that does not exist yet is created on the first save. If Keel is
already running, the paths open there.

Options:
  --install-extension VSIX  Install a VS Code extension package
  -h, --help                Print this help
  -V, --version             Print the version";

/// Parses the arguments that follow the program name.
pub fn parse(arguments: impl IntoIterator<Item = String>) -> Result<Command, String> {
    let mut files = Vec::new();
    let mut only_files = false;
    let mut arguments = arguments.into_iter();
    while let Some(argument) = arguments.next() {
        if !only_files && argument.starts_with('-') && argument != "-" {
            match argument.as_str() {
                "-h" | "--help" => return Ok(Command::Help),
                "-V" | "--version" => return Ok(Command::Version),
                "--install-extension" => {
                    return match arguments.next() {
                        Some(package) => Ok(Command::InstallExtension(PathBuf::from(package))),
                        None => Err("--install-extension needs a .vsix file".to_owned()),
                    };
                }
                "--finish-update" => {
                    let mut next = || arguments.next();
                    return match (next(), next(), next()) {
                        (Some(pid), Some(staged), Some(app)) => pid
                            .parse()
                            .map(|pid| Command::FinishUpdate {
                                pid,
                                staged: PathBuf::from(staged),
                                app: PathBuf::from(app),
                            })
                            .map_err(|_| "--finish-update needs a process id".to_owned()),
                        _ => Err("--finish-update needs PID STAGED APP".to_owned()),
                    };
                }
                "--" => only_files = true,
                other => return Err(format!("unknown option: {other}")),
            }
        } else {
            files.push(PathBuf::from(argument));
        }
    }
    Ok(Command::Open(files))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_args(arguments: &[&str]) -> Result<Command, String> {
        parse(arguments.iter().map(|argument| argument.to_string()))
    }

    #[test]
    fn no_arguments_opens_an_empty_editor() {
        assert_eq!(parse_args(&[]), Ok(Command::Open(Vec::new())));
    }

    #[test]
    fn a_path_opens_that_file() {
        assert_eq!(
            parse_args(&["src/app.tsx"]),
            Ok(Command::Open(vec![PathBuf::from("src/app.tsx")]))
        );
    }

    #[test]
    fn a_dot_opens_the_current_folder() {
        assert_eq!(
            parse_args(&["."]),
            Ok(Command::Open(vec![PathBuf::from(".")]))
        );
    }

    #[test]
    fn flags_print_help_or_version() {
        assert_eq!(parse_args(&["--help"]), Ok(Command::Help));
        assert_eq!(parse_args(&["-h"]), Ok(Command::Help));
        assert_eq!(parse_args(&["--version"]), Ok(Command::Version));
        assert_eq!(parse_args(&["-V"]), Ok(Command::Version));
    }

    #[test]
    fn a_double_dash_lets_file_names_start_with_a_dash() {
        assert_eq!(
            parse_args(&["--", "-notes.md"]),
            Ok(Command::Open(vec![PathBuf::from("-notes.md")]))
        );
    }

    #[test]
    fn install_extension_takes_a_package() {
        assert_eq!(
            parse_args(&["--install-extension", "ghost.vsix"]),
            Ok(Command::InstallExtension(PathBuf::from("ghost.vsix")))
        );
        assert!(parse_args(&["--install-extension"]).is_err());
    }

    #[test]
    fn unknown_options_are_errors() {
        assert!(parse_args(&["--wat"]).is_err());
    }

    #[test]
    fn several_paths_open_together() {
        assert_eq!(
            parse_args(&["a.ts", "b.ts", "--", "-c.ts"]),
            Ok(Command::Open(vec![
                PathBuf::from("a.ts"),
                PathBuf::from("b.ts"),
                PathBuf::from("-c.ts"),
            ]))
        );
    }

    #[test]
    fn finish_update_takes_the_process_and_both_apps() {
        assert_eq!(
            parse_args(&[
                "--finish-update",
                "42",
                "/A/.keel-update/Keel-2/Keel.app",
                "/A/Keel.app"
            ]),
            Ok(Command::FinishUpdate {
                pid: 42,
                staged: PathBuf::from("/A/.keel-update/Keel-2/Keel.app"),
                app: PathBuf::from("/A/Keel.app"),
            })
        );
        assert!(parse_args(&["--finish-update", "x", "a", "b"]).is_err());
        assert!(parse_args(&["--finish-update", "42"]).is_err());
    }
}
