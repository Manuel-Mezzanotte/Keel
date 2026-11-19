//! Quick fixes for a command that failed, as VS Code's terminal offers
//! them: the port a server wanted is taken, a branch has no upstream yet,
//! git did not know a command, a push was rejected. They read only the
//! command line and what it printed.

use std::sync::LazyLock;

use regex::Regex;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum QuickFix {
    /// Ends what listens on the port, then runs the command again.
    FreePort { port: u16 },
    /// Runs a command the program itself suggested.
    Run(String),
}

static PORT_TAKEN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)EADDRINUSE|address already in use|port \d+ is (already )?in use|already running on port")
        .expect("valid")
});
/// A port on the line that said it: `:::3000`, `port 3000`, `:8080`.
static PORT_NUMBER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)(?:port\s+|:)(\d{2,5})\b").expect("valid"));
/// A port the command line asked for: `--port 8000`, `-p 8000`, `:8000`.
static PORT_ASKED: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?:--port[= ]|-p\s+|:)(\d{2,5})\b").expect("valid"));
static SET_UPSTREAM: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?m)^\s*(git push --set-upstream \S+ \S+)\s*$").expect("valid"));
static GIT_TYPO: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"git: '([^']+)' is not a git command(?s:.*?)The most similar commands? (?:is|are)\s*\n\s*(\S+)")
        .expect("valid")
});

/// The fix for a command that failed with `output`, if one is known.
pub(crate) fn quick_fix(command: &str, output: &str) -> Option<QuickFix> {
    if let Some(found) = SET_UPSTREAM.captures(output) {
        return Some(QuickFix::Run(found[1].to_owned()));
    }
    if let Some(found) = GIT_TYPO.captures(output) {
        let (wrong, right) = (&found[1], &found[2]);
        if command.contains(wrong) {
            return Some(QuickFix::Run(command.replacen(wrong, right, 1)));
        }
    }
    if command.trim_start().starts_with("git push")
        && (output.contains("Updates were rejected because the remote contains work")
            || output.contains("the tip of your current branch is behind"))
    {
        return Some(QuickFix::Run("git pull --rebase".to_owned()));
    }
    if let Some(line) = output.lines().find(|line| PORT_TAKEN.is_match(line)) {
        let port = PORT_NUMBER
            .captures_iter(line)
            .last()
            .or_else(|| PORT_ASKED.captures_iter(command).last())
            .and_then(|found| found[1].parse::<u16>().ok())
            .filter(|port| *port >= 80);
        if let Some(port) = port {
            return Some(QuickFix::FreePort { port });
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_taken_port_is_freed() {
        let next = "Error: listen EADDRINUSE: address already in use :::3000\n    at Server.setupListenHandle";
        assert_eq!(
            quick_fix("npm run dev", next),
            Some(QuickFix::FreePort { port: 3000 })
        );
        let rails =
            r#"Address already in use - bind(2) for "127.0.0.1" port 3001 (Errno::EADDRINUSE)"#;
        assert_eq!(
            quick_fix("bin/rails s", rails),
            Some(QuickFix::FreePort { port: 3001 })
        );
        // Python says no port: the command line does.
        let python = "OSError: [Errno 48] Address already in use";
        assert_eq!(
            quick_fix("python3 -m http.server --port 8000", python),
            Some(QuickFix::FreePort { port: 8000 })
        );
        assert_eq!(quick_fix("python3 server.py", python), None);
    }

    #[test]
    fn git_says_what_to_run() {
        let upstream = "fatal: The current branch feature/x has no upstream branch.\nTo push the current branch and set the remote as upstream, use\n\n    git push --set-upstream origin feature/x\n\nTo have this happen automatically...";
        assert_eq!(
            quick_fix("git push", upstream),
            Some(QuickFix::Run(
                "git push --set-upstream origin feature/x".into()
            ))
        );
        let typo = "git: 'stauts' is not a git command. See 'git --help'.\n\nThe most similar command is\n\tstatus\n";
        assert_eq!(
            quick_fix("git stauts -s", typo),
            Some(QuickFix::Run("git status -s".into()))
        );
        let rejected = " ! [rejected]        main -> main (fetch first)\nerror: failed to push some refs to 'github.com:me/app.git'\nhint: Updates were rejected because the remote contains work that you do not\n";
        assert_eq!(
            quick_fix("git push", rejected),
            Some(QuickFix::Run("git pull --rebase".into()))
        );
        assert_eq!(quick_fix("cargo test", "test result: FAILED"), None);
    }
}
