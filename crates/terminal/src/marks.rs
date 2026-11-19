//! The marks a shell prints around its prompts and commands, as VS Code,
//! iTerm and Ghostty read them: OSC 133 (`A` the prompt starts, `B` the
//! command line starts, `C` the output starts, `D;code` the command ended),
//! VS Code's OSC 633 (the same, and `E;line` for the command line), and
//! the progress some programs report (OSC 9;4).
//!
//! The emulator ignores these sequences; [`Scanner`] finds them in the
//! bytes on their way to it, so the output can be cut right after each one
//! and the mark gets the line the cursor is on at that moment.

/// A program's progress, as the tab shows it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Progress {
    /// The share done, from 0 to 100.
    Normal(u8),
    /// Something went wrong, with the share done.
    Error(u8),
    /// Working, without knowing how far it is.
    Indeterminate,
    /// Paused, with the share done.
    Paused(u8),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Mark {
    PromptStart,
    CommandStart,
    OutputStart,
    /// The command ended, with its exit code when the shell said it.
    CommandEnd(Option<i32>),
    /// The command line as typed, and the nonce that proves the shell sent
    /// it rather than a program printing the same bytes.
    CommandLine {
        text: String,
        nonce: Option<String>,
    },
    /// `None` takes the progress away.
    Progress(Option<Progress>),
    /// A program asks for a notification (OSC 9 as iTerm2, OSC 777 as
    /// rxvt and Ghostty), as Claude Code does when it waits for you.
    Notify(String),
}

/// The longest sequence kept: long command lines fit, a runaway one does
/// not take memory.
const MAX_PAYLOAD: usize = 16 * 1024;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum State {
    #[default]
    Ground,
    Escape,
    Osc,
    OscEscape,
}

/// Finds marks in output that arrives in pieces: a sequence cut in two is
/// found when its second half comes.
#[derive(Debug, Default)]
pub(crate) struct Scanner {
    state: State,
    payload: Vec<u8>,
    overflow: bool,
}

impl Scanner {
    /// The marks that end in `bytes`, each with the offset just past it.
    pub(crate) fn scan(&mut self, bytes: &[u8], found: &mut Vec<(usize, Mark)>) {
        let mut index = 0;
        while index < bytes.len() {
            let byte = bytes[index];
            match self.state {
                State::Ground => {
                    // Most output has no escape at all: skip to the next one.
                    match bytes[index..].iter().position(|&byte| byte == 0x1b) {
                        Some(offset) => {
                            index += offset;
                            self.state = State::Escape;
                        }
                        None => return,
                    }
                }
                State::Escape => match byte {
                    b']' => self.start_string(),
                    0x1b => {}
                    _ => self.state = State::Ground,
                },
                State::Osc => match byte {
                    0x07 => {
                        self.finish(index + 1, found);
                        self.state = State::Ground;
                    }
                    0x1b => self.state = State::OscEscape,
                    // CAN and SUB cancel the sequence.
                    0x18 | 0x1a => self.state = State::Ground,
                    _ => self.push(byte),
                },
                State::OscEscape => {
                    // `ESC \` ends the string; any other escape ends it too
                    // and starts a sequence of its own.
                    if byte == b'\\' {
                        self.finish(index + 1, found);
                        self.state = State::Ground;
                    } else {
                        self.finish(index.saturating_sub(1), found);
                        match byte {
                            b']' => self.start_string(),
                            0x1b => self.state = State::Escape,
                            _ => self.state = State::Ground,
                        }
                    }
                }
            }
            index += 1;
        }
    }

    fn start_string(&mut self) {
        self.state = State::Osc;
        self.payload.clear();
        self.overflow = false;
    }

    fn push(&mut self, byte: u8) {
        if self.payload.len() < MAX_PAYLOAD {
            self.payload.push(byte);
        } else {
            self.overflow = true;
        }
    }

    fn finish(&mut self, end: usize, found: &mut Vec<(usize, Mark)>) {
        if !self.overflow
            && let Some(mark) = parse(&self.payload)
        {
            found.push((end, mark));
        }
        self.payload.clear();
    }
}

/// The mark an OSC string holds, if it is one Keel reads.
fn parse(payload: &[u8]) -> Option<Mark> {
    let text = std::str::from_utf8(payload).ok()?;
    let mut parts = text.split(';');
    match parts.next()? {
        "133" | "633" => {}
        "9" => {
            let rest = text.get(2..).unwrap_or_default();
            if rest.starts_with("4;") || rest == "4" {
                return parse_progress(parts);
            }
            // ConEmu's other commands, such as `9;9;folder`, start with a
            // number: they are not notifications.
            if rest
                .split(';')
                .next()
                .is_some_and(|first| !first.is_empty() && first.chars().all(|c| c.is_ascii_digit()))
            {
                return None;
            }
            return (!rest.trim().is_empty()).then(|| Mark::Notify(rest.trim().to_owned()));
        }
        "777" => {
            // `777;notify;title;body`
            if parts.next()? != "notify" {
                return None;
            }
            let title = parts.next().unwrap_or_default();
            let body = parts.collect::<Vec<_>>().join(";");
            let text = [title, body.as_str()]
                .iter()
                .filter(|part| !part.trim().is_empty())
                .copied()
                .collect::<Vec<_>>()
                .join(" — ");
            return (!text.is_empty()).then_some(Mark::Notify(text));
        }
        _ => return None,
    }
    let kind = parts.next()?;
    Some(match kind {
        "A" => Mark::PromptStart,
        "B" => Mark::CommandStart,
        "C" => Mark::OutputStart,
        "D" => Mark::CommandEnd(parts.next().and_then(|code| code.trim().parse().ok())),
        "E" => Mark::CommandLine {
            text: unescape(parts.next().unwrap_or_default()),
            nonce: parts.next().map(str::to_owned),
        },
        _ => return None,
    })
}

/// `9;4;state;percent`, as ConEmu defined it and Windows Terminal, Ghostty
/// and VS Code read it.
fn parse_progress<'a>(mut parts: impl Iterator<Item = &'a str>) -> Option<Mark> {
    if parts.next()? != "4" {
        return None;
    }
    let state = parts.next().unwrap_or("0");
    let percent = parts
        .next()
        .and_then(|percent| percent.trim().parse::<u32>().ok())
        .map(|percent| percent.min(100) as u8);
    Some(Mark::Progress(match state.trim() {
        "0" => None,
        "1" => Some(Progress::Normal(percent.unwrap_or(0))),
        "2" => Some(Progress::Error(percent.unwrap_or(100))),
        "3" => Some(Progress::Indeterminate),
        "4" => Some(Progress::Paused(percent.unwrap_or(0))),
        _ => return None,
    }))
}

/// A command line as VS Code's integration escapes it: `\\` for a
/// backslash, `\xNN` for a byte such as `;` or a newline.
fn unescape(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'\\' {
            if bytes.get(index + 1) == Some(&b'\\') {
                out.push(b'\\');
                index += 2;
                continue;
            }
            if bytes.get(index + 1) == Some(&b'x')
                && let Some(hex) = text.get(index + 2..index + 4)
                && let Ok(byte) = u8::from_str_radix(hex, 16)
            {
                out.push(byte);
                index += 4;
                continue;
            }
        }
        out.push(bytes[index]);
        index += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scan_all(pieces: &[&[u8]]) -> Vec<(usize, usize, Mark)> {
        let mut scanner = Scanner::default();
        let mut all = Vec::new();
        for (piece, bytes) in pieces.iter().enumerate() {
            let mut found = Vec::new();
            scanner.scan(bytes, &mut found);
            all.extend(found.into_iter().map(|(end, mark)| (piece, end, mark)));
        }
        all
    }

    #[test]
    fn finds_the_marks_and_where_they_end() {
        let bytes =
            b"out\x1b]133;D;1\x07\x1b]133;A\x1b\\% \x1b]633;E;ls -la;n0nce\x07\x1b]133;C\x07x";
        let found = scan_all(&[bytes]);
        let marks: Vec<_> = found.iter().map(|(_, _, mark)| mark.clone()).collect();
        assert_eq!(
            marks,
            vec![
                Mark::CommandEnd(Some(1)),
                Mark::PromptStart,
                Mark::CommandLine {
                    text: "ls -la".into(),
                    nonce: Some("n0nce".into())
                },
                Mark::OutputStart,
            ]
        );
        // Each one is cut right after its terminator.
        assert_eq!(&bytes[..found[0].1], b"out\x1b]133;D;1\x07");
        assert_eq!(found[3].1, bytes.len() - 1);
    }

    #[test]
    fn a_sequence_cut_in_two_is_found_when_it_ends() {
        let found = scan_all(&[b"ab\x1b]13", b"3;D", b";0\x1b", b"\\rest"]);
        assert_eq!(found, vec![(3, 1, Mark::CommandEnd(Some(0)))]);
    }

    #[test]
    fn other_sequences_and_text_are_left_alone() {
        let found = scan_all(&[b"\x1b[31mred\x1b]0;title\x07\x1b]8;;http://x\x07link\x1b]8;;\x07"]);
        assert!(found.is_empty());
        // An escape inside the string ends it and starts its own sequence.
        let found = scan_all(&[b"\x1b]133;A\x1b[0m"]);
        assert_eq!(found, vec![(0, 7, Mark::PromptStart)]);
    }

    #[test]
    fn command_lines_and_progress() {
        assert_eq!(
            unescape(r"echo a\x3bb \\ c\x0ad"),
            "echo a;b \\ c\nd".to_owned()
        );
        assert_eq!(
            parse(b"9;4;1;42"),
            Some(Mark::Progress(Some(Progress::Normal(42))))
        );
        assert_eq!(parse(b"9;4;0"), Some(Mark::Progress(None)));
        assert_eq!(
            parse(b"9;4;3"),
            Some(Mark::Progress(Some(Progress::Indeterminate)))
        );
        assert_eq!(
            parse(b"9;4;2"),
            Some(Mark::Progress(Some(Progress::Error(100))))
        );
        assert_eq!(
            parse(b"9;Claude is waiting for your input"),
            Some(Mark::Notify("Claude is waiting for your input".into()))
        );
        assert_eq!(parse(b"9;9;/Users/me"), None);
        assert_eq!(
            parse(b"777;notify;Claude Code;Task done"),
            Some(Mark::Notify("Claude Code — Task done".into()))
        );
        assert_eq!(parse(b"133;D"), Some(Mark::CommandEnd(None)));
    }
}
