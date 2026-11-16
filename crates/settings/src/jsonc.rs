//! JSON with comments and trailing commas, as VS Code writes `settings.json`
//! and `keybindings.json`.

use serde_json::Value;

/// Parses JSON that may contain `//` and `/* */` comments and trailing commas.
pub fn parse(text: &str) -> Result<Value, serde_json::Error> {
    serde_json::from_str(&strip(text))
}

/// Whether the text has comments, which rewriting the file would lose.
pub fn has_comments(text: &str) -> bool {
    let mut characters = text.chars().peekable();
    let mut in_string = false;
    while let Some(character) = characters.next() {
        if in_string {
            match character {
                '\\' => {
                    characters.next();
                }
                '"' => in_string = false,
                _ => {}
            }
        } else if character == '"' {
            in_string = true;
        } else if character == '/' && matches!(characters.peek(), Some('/' | '*')) {
            return true;
        }
    }
    false
}

/// The first character from `index` that is not blank or in a comment.
fn next_meaningful(characters: &[char], mut index: usize) -> Option<char> {
    loop {
        match (characters.get(index)?, characters.get(index + 1)) {
            (character, _) if character.is_whitespace() => index += 1,
            ('/', Some('/')) => {
                while index < characters.len() && characters[index] != '\n' {
                    index += 1;
                }
            }
            ('/', Some('*')) => {
                index += 2;
                while index < characters.len()
                    && !(characters[index] == '*' && characters.get(index + 1) == Some(&'/'))
                {
                    index += 1;
                }
                index += 2;
            }
            (character, _) => return Some(*character),
        }
    }
}

/// Replaces comments with spaces (keeping line numbers for error messages)
/// and removes commas that come right before `}` or `]`.
fn strip(text: &str) -> String {
    let characters: Vec<char> = text.chars().collect();
    let mut output = String::with_capacity(text.len());
    let mut index = 0;
    let mut in_string = false;
    while index < characters.len() {
        let character = characters[index];
        let next = characters.get(index + 1).copied();
        if in_string {
            output.push(character);
            if character == '\\' {
                if let Some(next) = next {
                    output.push(next);
                    index += 1;
                }
            } else if character == '"' {
                in_string = false;
            }
            index += 1;
            continue;
        }
        match (character, next) {
            ('"', _) => {
                in_string = true;
                output.push(character);
                index += 1;
            }
            ('/', Some('/')) => {
                while index < characters.len() && characters[index] != '\n' {
                    index += 1;
                }
            }
            ('/', Some('*')) => {
                index += 2;
                while index < characters.len()
                    && !(characters[index] == '*' && characters.get(index + 1) == Some(&'/'))
                {
                    if characters[index] == '\n' {
                        output.push('\n');
                    }
                    index += 1;
                }
                index += 2;
            }
            (',', _) => {
                if !matches!(next_meaningful(&characters, index + 1), Some('}' | ']')) {
                    output.push(',');
                }
                index += 1;
            }
            _ => {
                output.push(character);
                index += 1;
            }
        }
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn many_commas_are_read_quickly() {
        let entries: Vec<String> = (0..20_000)
            .map(|n| format!("\"key{n}\": [{n}, {n},]"))
            .collect();
        let text = format!("{{ {}, // end\n }}", entries.join(",\n"));
        let started = std::time::Instant::now();
        let value = parse(&text).unwrap();
        assert_eq!(value.as_object().unwrap().len(), 20_000);
        assert_eq!(value["key7"], json!([7, 7]));
        assert!(
            started.elapsed().as_secs_f32() < 1.0,
            "{:?}",
            started.elapsed()
        );
    }

    #[test]
    fn plain_json() {
        assert_eq!(parse(r#"{ "a": 1 }"#).unwrap(), json!({ "a": 1 }));
    }

    #[test]
    fn comments_and_trailing_commas() {
        let text = r#"{
            // Tema di Keel
            "workbench.colorTheme": "Mezzanotte", /* il predefinito */
            "editor.fontSize": 14,
            "files.exclude": { "**/.next": true, },
        }"#;
        assert_eq!(
            parse(text).unwrap(),
            json!({
                "workbench.colorTheme": "Mezzanotte",
                "editor.fontSize": 14,
                "files.exclude": { "**/.next": true },
            })
        );
        assert!(has_comments(text));
        assert!(!has_comments(r#"{ "a": 1 }"#));
    }

    #[test]
    fn comment_markers_inside_strings_stay() {
        let text = r#"{ "url": "https://mezzalab.eu/*x*/", "path": "a//b", "quote": "say \"hi\" // not a comment" }"#;
        assert_eq!(
            parse(text).unwrap(),
            json!({ "url": "https://mezzalab.eu/*x*/", "path": "a//b", "quote": "say \"hi\" // not a comment" })
        );
    }

    #[test]
    fn broken_json_is_an_error() {
        assert!(parse("{ \"a\": }").is_err());
    }
}
