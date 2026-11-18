//! Changes to a JSONC file that keep everything else as it was: comments,
//! the order of the keys, blank lines and indentation. Only the top level is
//! touched: a key of `settings.json`, an entry of `keybindings.json`.

use anyhow::{Result, bail};
use serde_json::Value;

/// A member of the top-level object or array: where it starts (the start
/// of its line when it begins one), where its value starts and ends, and
/// where the member ends, after its comma if it has one.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Member {
    key: Option<String>,
    line_start: usize,
    value: std::ops::Range<usize>,
    end: usize,
}

struct Scanner<'a> {
    text: &'a [u8],
    at: usize,
}

impl Scanner<'_> {
    /// Skips blanks and comments.
    fn skip(&mut self) {
        loop {
            match (self.text.get(self.at), self.text.get(self.at + 1)) {
                (Some(byte), _) if byte.is_ascii_whitespace() => self.at += 1,
                (Some(b'/'), Some(b'/')) => {
                    while self.at < self.text.len() && self.text[self.at] != b'\n' {
                        self.at += 1;
                    }
                }
                (Some(b'/'), Some(b'*')) => {
                    self.at += 2;
                    while self.at + 1 < self.text.len()
                        && !(self.text[self.at] == b'*' && self.text[self.at + 1] == b'/')
                    {
                        self.at += 1;
                    }
                    self.at = (self.at + 2).min(self.text.len());
                }
                _ => return,
            }
        }
    }

    fn peek(&self) -> Option<u8> {
        self.text.get(self.at).copied()
    }

    /// Moves over a string, from its opening quote.
    fn string(&mut self) -> Result<()> {
        self.at += 1;
        while let Some(byte) = self.peek() {
            self.at += 1;
            match byte {
                b'\\' => self.at += 1,
                b'"' => return Ok(()),
                _ => {}
            }
        }
        bail!("a string is not closed")
    }

    /// Moves over any value: a string, an object or array (with whatever
    /// is inside), or a word such as `true` or a number.
    fn value(&mut self) -> Result<()> {
        match self.peek() {
            Some(b'"') => self.string(),
            Some(b'{' | b'[') => {
                let mut depth = 0usize;
                while let Some(byte) = self.peek() {
                    match byte {
                        b'"' => {
                            self.string()?;
                            continue;
                        }
                        b'/' if matches!(self.text.get(self.at + 1), Some(b'/' | b'*')) => {
                            self.skip();
                            continue;
                        }
                        b'{' | b'[' => depth += 1,
                        b'}' | b']' => {
                            depth -= 1;
                            if depth == 0 {
                                self.at += 1;
                                return Ok(());
                            }
                        }
                        _ => {}
                    }
                    self.at += 1;
                }
                bail!("an object or array is not closed")
            }
            Some(_) => {
                while let Some(byte) = self.peek() {
                    if matches!(byte, b',' | b'}' | b']' | b'/') || byte.is_ascii_whitespace() {
                        break;
                    }
                    self.at += 1;
                }
                Ok(())
            }
            None => bail!("a value is missing"),
        }
    }
}

/// The start of the line `at` is on, if only blanks come before it there.
fn line_start(text: &[u8], at: usize) -> usize {
    let mut start = at;
    while start > 0 && matches!(text[start - 1], b' ' | b'\t') {
        start -= 1;
    }
    if start == 0 || text[start - 1] == b'\n' {
        start
    } else {
        at
    }
}

/// The members of the top-level object or array, and where it closes.
fn members(text: &str, object: bool) -> Result<(Vec<Member>, usize)> {
    let bytes = text.as_bytes();
    let mut scanner = Scanner { text: bytes, at: 0 };
    scanner.skip();
    let open = if object { b'{' } else { b'[' };
    if scanner.peek() != Some(open) {
        bail!(
            "the file must contain {}",
            if object { "an object" } else { "an array" }
        );
    }
    scanner.at += 1;
    let mut found = Vec::new();
    loop {
        scanner.skip();
        match scanner.peek() {
            Some(b'}') if object => return Ok((found, scanner.at)),
            Some(b']') if !object => return Ok((found, scanner.at)),
            None => bail!("the file ends too soon"),
            _ => {}
        }
        let start = scanner.at;
        let key = if object {
            if scanner.peek() != Some(b'"') {
                bail!("a key must be a string");
            }
            scanner.string()?;
            let key: String = serde_json::from_str(&text[start..scanner.at])?;
            scanner.skip();
            if scanner.peek() != Some(b':') {
                bail!("a ':' is missing after {key}");
            }
            scanner.at += 1;
            scanner.skip();
            Some(key)
        } else {
            None
        };
        let value_start = scanner.at;
        scanner.value()?;
        let value_end = scanner.at;
        scanner.skip();
        let end = if scanner.peek() == Some(b',') {
            scanner.at += 1;
            scanner.at
        } else {
            value_end
        };
        found.push(Member {
            key,
            line_start: line_start(bytes, start),
            value: value_start..value_end,
            end,
        });
    }
}

/// The indentation of the members, `    ` when there are none.
fn indentation(text: &str, members: &[Member]) -> String {
    members
        .first()
        .and_then(|member| {
            let line = &text[member.line_start..member.value.start];
            let indent: String = line
                .chars()
                .take_while(|c| *c == ' ' || *c == '\t')
                .collect();
            (!indent.is_empty()).then_some(indent)
        })
        .unwrap_or_else(|| "    ".to_owned())
}

/// `value` as it is written in the file: nested levels indented by the
/// file's own step, from the members' indentation.
fn written(value: &Value, indent: &str) -> String {
    use serde::Serialize as _;
    let mut out = Vec::new();
    let formatter = serde_json::ser::PrettyFormatter::with_indent(indent.as_bytes());
    let mut serializer = serde_json::Serializer::with_formatter(&mut out, formatter);
    if value.serialize(&mut serializer).is_err() {
        return "null".to_owned();
    }
    String::from_utf8_lossy(&out).replace('\n', &format!("\n{indent}"))
}

/// Inserts a new member, written as `item`, after the last one.
fn append(text: &str, members: &[Member], close: usize, item: &str) -> String {
    let indent = indentation(text, members);
    match members.last() {
        Some(last) => {
            // After its comma, or with one added.
            let (at, comma) = if last.end > last.value.end {
                (last.end, "")
            } else {
                (last.value.end, ",")
            };
            format!("{}{comma}\n{indent}{item}{}", &text[..at], &text[at..])
        }
        None => {
            let before = text[..close].trim_end();
            format!("{before}\n{indent}{item}\n{}", &text[close..])
        }
    }
}

/// Removes a member with its line, when it has one to itself.
fn remove(text: &str, members: &[Member], index: usize) -> String {
    let member = &members[index];
    let mut end = member.end;
    // The rest of its line, when only blanks are left on it.
    let rest = &text[end..];
    let line_end = rest.find('\n').map_or(rest.len(), |at| at + 1);
    if rest[..line_end].trim().is_empty() && member.line_start < member.value.start {
        end += line_end;
    }
    // When the last member goes, the one before keeps its comma: JSONC
    // allows a trailing comma, and VS Code leaves it too.
    format!("{}{}", &text[..member.line_start], &text[end..])
}

/// Sets `key` of the top-level object to `value`, or removes it (`None`).
pub fn set_key(text: &str, key: &str, value: Option<&Value>) -> Result<String> {
    let text = if text.trim().is_empty() { "{}" } else { text };
    let (members, close) = members(text, true)?;
    let index = members
        .iter()
        .position(|member| member.key.as_deref() == Some(key));
    let indent = indentation(text, &members);
    Ok(match (index, value) {
        (Some(index), Some(value)) => {
            let range = members[index].value.clone();
            format!(
                "{}{}{}",
                &text[..range.start],
                written(value, &indent),
                &text[range.end..]
            )
        }
        (Some(index), None) => remove(text, &members, index),
        (None, Some(value)) => {
            let item = format!(
                "{}: {}",
                serde_json::to_string(key)?,
                written(value, &indent)
            );
            append(text, &members, close, &item)
        }
        (None, None) => text.to_owned(),
    })
}

/// Adds `items` at the end of the top-level array.
pub fn append_items(text: &str, items: &[Value]) -> Result<String> {
    let mut text = if text.trim().is_empty() {
        "[]".to_owned()
    } else {
        text.to_owned()
    };
    for item in items {
        let (members, close) = members(&text, false)?;
        let indent = indentation(&text, &members);
        text = append(&text, &members, close, &written(item, &indent));
    }
    Ok(text)
}

/// Removes the entries of the top-level array that `remove_if` picks.
pub fn remove_items(text: &str, remove_if: impl Fn(&Value) -> bool) -> Result<String> {
    let mut text = text.to_owned();
    loop {
        let (members, _) = members(&text, false)?;
        let found = members.iter().position(|member| {
            crate::jsonc::parse(&text[member.value.clone()]).is_ok_and(|value| remove_if(&value))
        });
        match found {
            Some(index) => text = remove(&text, &members, index),
            None => return Ok(text),
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    const SETTINGS: &str = "{\n    // The theme I like\n    \"workbench.colorTheme\": \"Mezzanotte\",\n\n    /* size */\n    \"editor.fontSize\": 14\n}\n";

    #[test]
    fn a_key_changes_and_the_comments_stay() {
        let changed = set_key(SETTINGS, "editor.fontSize", Some(&json!(16))).unwrap();
        assert_eq!(changed, SETTINGS.replace("14", "16"));
        let changed = set_key(SETTINGS, "workbench.colorTheme", Some(&json!("Nero"))).unwrap();
        assert!(changed.contains("// The theme I like\n    \"workbench.colorTheme\": \"Nero\","));
    }

    #[test]
    fn a_new_key_goes_last_with_the_same_indentation() {
        let added = set_key(SETTINGS, "editor.wordWrap", Some(&json!("on"))).unwrap();
        assert!(
            added.ends_with("\"editor.fontSize\": 14,\n    \"editor.wordWrap\": \"on\"\n}\n"),
            "{added}"
        );
        assert_eq!(
            crate::jsonc::parse(&added).unwrap()["editor.wordWrap"],
            json!("on")
        );
        // An object value keeps the indentation inside.
        let added = set_key("{}", "files.exclude", Some(&json!({ "**/dist": true }))).unwrap();
        assert_eq!(
            added,
            "{\n    \"files.exclude\": {\n        \"**/dist\": true\n    }\n}"
        );
        assert_eq!(
            set_key("", "a", Some(&json!(1))).unwrap(),
            "{\n    \"a\": 1\n}"
        );
    }

    #[test]
    fn a_removed_key_takes_its_line() {
        let removed = set_key(SETTINGS, "editor.fontSize", None).unwrap();
        assert_eq!(
            removed,
            "{\n    // The theme I like\n    \"workbench.colorTheme\": \"Mezzanotte\",\n\n    /* size */\n}\n"
        );
        assert!(crate::jsonc::parse(&removed).is_ok());
        let removed = set_key(SETTINGS, "workbench.colorTheme", None).unwrap();
        assert!(!removed.contains("colorTheme"));
        assert!(removed.contains("\"editor.fontSize\": 14"));
        assert!(crate::jsonc::parse(&removed).is_ok());
    }

    #[test]
    fn keybindings_are_added_and_removed() {
        let file = "// Place your key bindings in this file\n[\n    { \"key\": \"cmd+k\", \"command\": \"a\" }\n]\n";
        let added = append_items(file, &[json!({ "key": "cmd+j", "command": "b" })]).unwrap();
        let parsed = crate::jsonc::parse(&added).unwrap();
        assert_eq!(parsed.as_array().unwrap().len(), 2);
        assert!(added.starts_with("// Place your key bindings"));
        let removed = remove_items(&added, |entry| entry["command"] == "a").unwrap();
        let parsed = crate::jsonc::parse(&removed).unwrap();
        assert_eq!(parsed, json!([{ "key": "cmd+j", "command": "b" }]));
        let first = append_items("", &[json!({ "key": "f5", "command": "c" })]).unwrap();
        assert_eq!(
            crate::jsonc::parse(&first).unwrap(),
            json!([{ "key": "f5", "command": "c" }])
        );
    }

    #[test]
    fn strings_with_brackets_and_slashes_are_not_confused() {
        let text = "{ \"a\": \"// not a comment }\", \"b\": [1, { \"c\": \"]\" }] }";
        let changed = set_key(text, "b", Some(&json!(2))).unwrap();
        assert_eq!(
            crate::jsonc::parse(&changed).unwrap(),
            json!({ "a": "// not a comment }", "b": 2 })
        );
    }
}
