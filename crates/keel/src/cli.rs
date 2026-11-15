//! Command-line arguments: `keel [FILE]`.

use std::path::PathBuf;

/// What the user asked for on the command line.
#[derive(Debug, PartialEq, Eq)]
pub enum Command {
    /// Open the editor, optionally on a file.
    Open(Option<PathBuf>),
    /// Print the version and exit.
    Version,
    /// Print the usage and exit.
    Help,
}

/// Usage text printed by `keel --help` and after a wrong invocation.
pub const USAGE: &str = "\
Usage: keel [FILE]

Opens FILE in Keel. A file that does not exist yet is created on the first save.

Options:
  -h, --help     Print this help
  -V, --version  Print the version";

/// Parses the arguments that follow the program name.
pub fn parse(arguments: impl IntoIterator<Item = String>) -> Result<Command, String> {
    let mut file = None;
    let mut only_files = false;
    for argument in arguments {
        if !only_files && argument.starts_with('-') && argument != "-" {
            match argument.as_str() {
                "-h" | "--help" => return Ok(Command::Help),
                "-V" | "--version" => return Ok(Command::Version),
                "--" => only_files = true,
                other => return Err(format!("unknown option: {other}")),
            }
        } else if file.is_none() {
            file = Some(PathBuf::from(argument));
        } else {
            return Err("only one file can be opened for now".to_owned());
        }
    }
    Ok(Command::Open(file))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_args(arguments: &[&str]) -> Result<Command, String> {
        parse(arguments.iter().map(|argument| argument.to_string()))
    }

    #[test]
    fn no_arguments_opens_an_empty_editor() {
        assert_eq!(parse_args(&[]), Ok(Command::Open(None)));
    }

    #[test]
    fn a_path_opens_that_file() {
        assert_eq!(
            parse_args(&["src/app.tsx"]),
            Ok(Command::Open(Some(PathBuf::from("src/app.tsx"))))
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
            Ok(Command::Open(Some(PathBuf::from("-notes.md"))))
        );
    }

    #[test]
    fn unknown_options_and_extra_files_are_errors() {
        assert!(parse_args(&["--wat"]).is_err());
        assert!(parse_args(&["a.ts", "b.ts"]).is_err());
    }
}
