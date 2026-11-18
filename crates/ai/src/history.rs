//! Claude Code's own chats in a project, read from where Claude Code keeps
//! them: one file per conversation in `~/.claude/projects/<folder>/`, the
//! folder named after the project's path. Only each chat's title and time
//! are read, from the ends of its file, so the list shows at once, before
//! the agent starts; once it runs, its `session/list` says the same.
//!
//! The files are Claude Code's, not a published format: anything not
//! understood is skipped, and a chat without a title it can read is
//! called by its first message.

use std::fs::File;
use std::io::{Read as _, Seek as _, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use serde_json::Value;

/// One conversation: its id (the agent's session), what it is called, and
/// when it last changed, in seconds since 1970.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Chat {
    pub id: String,
    pub title: String,
    pub updated: i64,
}

/// How much of the end of a file is read for its title, and of the start
/// for its first message: a chat's file can be tens of megabytes.
const TAIL: u64 = 160 * 1024;
const HEAD: u64 = 96 * 1024;
/// The longest title shown.
const TITLE_CHARS: usize = 90;

/// Where Claude Code keeps its data: `CLAUDE_CONFIG_DIR`, or `~/.claude`.
pub fn claude_dir() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("CLAUDE_CONFIG_DIR").filter(|dir| !dir.is_empty()) {
        return Some(PathBuf::from(dir));
    }
    std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".claude"))
}

/// The folder of a project's chats: its path with every character that is
/// not a letter or a digit turned into `-`, as Claude Code names it.
pub fn project_folder(claude: &Path, cwd: &Path) -> PathBuf {
    let name: String = cwd
        .to_string_lossy()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    claude.join("projects").join(name)
}

/// The project's chats, the latest first, at most `most`.
pub fn chats(claude: &Path, cwd: &Path, most: usize) -> Vec<Chat> {
    let Ok(entries) = std::fs::read_dir(project_folder(claude, cwd)) else {
        return Vec::new();
    };
    let mut files: Vec<(i64, PathBuf)> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "jsonl") && path.is_file())
        .filter_map(|path| {
            let modified = path.metadata().ok()?.modified().ok()?;
            let seconds = modified.duration_since(UNIX_EPOCH).ok()?.as_secs() as i64;
            Some((seconds, path))
        })
        .collect();
    files.sort_by_key(|(updated, _)| std::cmp::Reverse(*updated));
    files
        .into_iter()
        .filter_map(|(updated, path)| {
            let id = path.file_stem()?.to_string_lossy().into_owned();
            let title = title_of(&path)?;
            Some(Chat { id, title, updated })
        })
        .take(most)
        .collect()
}

/// The lines of a piece of a file, the first one dropped when it may have
/// been cut.
fn lines(file: &Path, from_end: bool, size: u64) -> Vec<Value> {
    let Ok(mut file) = File::open(file) else {
        return Vec::new();
    };
    let length = file.metadata().map(|meta| meta.len()).unwrap_or(0);
    let start = if from_end {
        length.saturating_sub(size)
    } else {
        0
    };
    if file.seek(SeekFrom::Start(start)).is_err() {
        return Vec::new();
    }
    let mut bytes = Vec::new();
    if file.take(size).read_to_end(&mut bytes).is_err() {
        return Vec::new();
    }
    let text = String::from_utf8_lossy(&bytes);
    let mut lines: Vec<&str> = text.lines().collect();
    // A piece from the middle starts inside a line, and one read from the
    // start may end inside one: both are left out.
    if start > 0 && !lines.is_empty() {
        lines.remove(0);
    }
    if !from_end && length > size {
        lines.pop();
    }
    lines
        .into_iter()
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect()
}

/// A chat's title: the one given to it, Claude Code's summary, or its
/// first message. None for a file with no conversation in it.
fn title_of(file: &Path) -> Option<String> {
    let tail = lines(file, true, TAIL);
    let given = tail
        .iter()
        .rev()
        .find_map(|line| match line["type"].as_str() {
            Some("custom-title") => text_of(&line["customTitle"]),
            Some("summary") => text_of(&line["summary"]),
            _ => None,
        });
    if let Some(title) = given {
        return Some(title);
    }
    let first = lines(file, false, HEAD)
        .iter()
        .find_map(|line| (line["type"] == "user").then(|| user_text(line)).flatten());
    first.or_else(|| {
        tail.iter().rev().find_map(|line| {
            (line["type"] == "last-prompt")
                .then(|| text_of(&line["lastPrompt"]))
                .flatten()
        })
    })
}

/// What a person wrote in a `user` line, leaving out tool results, notes
/// Claude Code adds itself and commands in angle brackets.
fn user_text(line: &Value) -> Option<String> {
    if line["isMeta"] == true || line["isSidechain"] == true {
        return None;
    }
    let content = &line["message"]["content"];
    let text = match content {
        Value::String(text) => Some(text.clone()),
        Value::Array(blocks) => blocks
            .iter()
            .find(|block| block["type"] == "text")
            .and_then(|block| block["text"].as_str().map(str::to_owned)),
        _ => None,
    }?;
    if text.trim_start().starts_with('<') {
        return None;
    }
    text_of(&Value::String(text))
}

/// A title from a text: its first line that says something, cut short.
fn text_of(value: &Value) -> Option<String> {
    let line = value
        .as_str()?
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())?;
    let mut title: String = line.chars().take(TITLE_CHARS).collect();
    if line.chars().count() > TITLE_CHARS {
        title.push('…');
    }
    Some(title)
}

/// Seconds since 1970 of an ISO 8601 time such as `2026-11-18T20:41:58.123Z`,
/// as the agent's `updatedAt` gives it. Only UTC times are read.
pub fn iso_seconds(text: &str) -> Option<i64> {
    let (date, time) = text.split_once('T')?;
    let mut date = date.split('-').map(|part| part.parse::<i64>().ok());
    let (year, month, day) = (date.next()??, date.next()??, date.next()??);
    let time = time.trim_end_matches('Z');
    let time = time.split(['+', '.']).next()?;
    let mut time = time.split(':').map(|part| part.parse::<i64>().ok());
    let (hour, minute) = (time.next()??, time.next()??);
    let second = time.next().flatten().unwrap_or(0);
    // Days from 1970-01-01 to the date (Howard Hinnant's days_from_civil).
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let of_era = year - era * 400;
    let month_index = (month + 9) % 12;
    let of_year = (153 * month_index + 2) / 5 + day - 1;
    let of_era_day = of_era * 365 + of_era / 4 - of_era / 100 + of_year;
    let days = era * 146_097 + of_era_day - 719_468;
    Some(days * 86_400 + hour * 3600 + minute * 60 + second)
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, SystemTime};

    use super::*;

    fn write(folder: &Path, id: &str, lines: &[&str], age: u64) {
        let path = folder.join(format!("{id}.jsonl"));
        std::fs::write(&path, lines.join("\n") + "\n").unwrap();
        let time = SystemTime::now() - Duration::from_secs(age);
        File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_modified(time)
            .unwrap();
    }

    #[test]
    fn the_folder_is_named_as_claude_code_names_it() {
        assert_eq!(
            project_folder(
                Path::new("/Users/me/.claude"),
                Path::new("/Users/me/Desktop/MiniPCProgram/Keel")
            ),
            Path::new("/Users/me/.claude/projects/-Users-me-Desktop-MiniPCProgram-Keel")
        );
        assert_eq!(
            project_folder(Path::new("/c"), Path::new("/Users/me/.config/my app")),
            Path::new("/c/projects/-Users-me--config-my-app")
        );
    }

    #[test]
    fn chats_have_their_titles_the_latest_first() {
        let claude = tempfile::tempdir().unwrap();
        let cwd = Path::new("/Users/me/Progetto");
        let folder = project_folder(claude.path(), cwd);
        std::fs::create_dir_all(folder.join("abc/subagents")).unwrap();
        // Named by Claude Code, even with earlier messages.
        write(
            &folder,
            "named",
            &[
                r#"{"type":"user","message":{"role":"user","content":"Sistema il menu"}}"#,
                r#"{"type":"custom-title","customTitle":"Menu di Keel","sessionId":"named"}"#,
            ],
            300,
        );
        // Called by its first real message: not Claude Code's own notes,
        // not a command, not a tool result.
        write(
            &folder,
            "first",
            &[
                r#"{"type":"user","isMeta":true,"message":{"content":"Caveat: ..."}}"#,
                r#"{"type":"user","message":{"content":"<command-name>/clear</command-name>"}}"#,
                r#"{"type":"user","message":{"content":[{"type":"tool_result","content":"ok"}]}}"#,
                r#"{"type":"user","message":{"content":[{"type":"text","text":"\n  Aggiungi i test\nper il grafico"}]}}"#,
                "not json",
            ],
            100,
        );
        // Nothing said: not a chat.
        write(&folder, "empty", &[r#"{"type":"queue-operation"}"#], 10);
        std::fs::write(folder.join("notes.txt"), "x").unwrap();

        let found = chats(claude.path(), cwd, 10);
        let titles: Vec<(&str, &str)> = found
            .iter()
            .map(|chat| (chat.id.as_str(), chat.title.as_str()))
            .collect();
        assert_eq!(
            titles,
            [("first", "Aggiungi i test"), ("named", "Menu di Keel")]
        );
        assert!(found[0].updated > found[1].updated);
        assert_eq!(chats(claude.path(), cwd, 1).len(), 1);
        // Another project has its own chats.
        assert!(chats(claude.path(), Path::new("/Users/me/Altro"), 10).is_empty());
    }

    #[test]
    fn a_long_file_is_read_only_at_its_ends() {
        let claude = tempfile::tempdir().unwrap();
        let cwd = Path::new("/p");
        let folder = project_folder(claude.path(), cwd);
        std::fs::create_dir_all(&folder).unwrap();
        let filler = format!(
            r#"{{"type":"assistant","message":{{"content":"{}"}}}}"#,
            "x".repeat(1000)
        );
        let mut lines = vec![r#"{"type":"user","message":{"content":"All'inizio"}}"#.to_owned()];
        lines.extend(std::iter::repeat_n(filler, 2000));
        lines.push(r#"{"type":"custom-title","customTitle":"Alla fine"}"#.to_owned());
        let lines: Vec<&str> = lines.iter().map(String::as_str).collect();
        write(&folder, "long", &lines, 0);
        assert_eq!(chats(claude.path(), cwd, 5)[0].title, "Alla fine");
    }

    #[test]
    fn iso_times_become_seconds() {
        assert_eq!(iso_seconds("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(iso_seconds("2026-11-18T20:41:58.123Z"), Some(1_795_034_518));
        assert_eq!(iso_seconds("2000-03-01T00:00:00Z"), Some(951_868_800));
        assert_eq!(iso_seconds("ieri"), None);
    }
}
