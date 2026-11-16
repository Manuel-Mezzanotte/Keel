//! Snippets in the format VS Code and language servers use: `$1`,
//! `${1:default}`, `${1|one,two|}`, `$0` and variables such as
//! `$TM_FILENAME`, read into plain text and the places Tab moves through.

use std::ops::Range;

/// A snippet ready to insert.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Snippet {
    pub text: String,
    /// Where Tab goes, in order: byte ranges of `text`. A tab stop used
    /// more than once lists every place. `$0`, the final cursor, comes last.
    pub stops: Vec<Vec<Range<usize>>>,
}

impl Snippet {
    /// Reads `source`. `variable` gives the value of a variable such as
    /// `TM_FILENAME`, or `None` when it is unknown.
    pub fn parse(source: &str, variable: &dyn Fn(&str) -> Option<String>) -> Self {
        let mut parser = Parser {
            chars: source.chars().collect(),
            at: 0,
            text: String::new(),
            stops: Vec::new(),
            variable,
        };
        parser.content(false);
        let mut numbered: Vec<(u32, Vec<Range<usize>>)> = Vec::new();
        for (index, range) in parser.stops {
            match numbered.iter_mut().find(|(number, _)| *number == index) {
                Some((_, ranges)) => ranges.push(range),
                None => numbered.push((index, vec![range])),
            }
        }
        // 1, 2, 3… then 0 at the end.
        numbered.sort_by_key(|(index, _)| if *index == 0 { u32::MAX } else { *index });
        Self {
            text: parser.text,
            stops: numbered.into_iter().map(|(_, ranges)| ranges).collect(),
        }
    }

    /// A plain text, with nothing to fill in.
    pub fn plain(text: &str) -> Self {
        Self {
            text: text.to_owned(),
            stops: Vec::new(),
        }
    }

    /// Whether Tab has somewhere to go besides the end.
    pub fn has_stops(&self) -> bool {
        !self.stops.is_empty()
    }
}

struct Parser<'a> {
    chars: Vec<char>,
    at: usize,
    text: String,
    stops: Vec<(u32, Range<usize>)>,
    variable: &'a dyn Fn(&str) -> Option<String>,
}

impl Parser<'_> {
    fn peek(&self) -> Option<char> {
        self.chars.get(self.at).copied()
    }

    /// Reads text until the end, or until a `}` closing a placeholder when
    /// `nested`.
    fn content(&mut self, nested: bool) {
        while let Some(character) = self.peek() {
            match character {
                '\\' => {
                    let next = self.chars.get(self.at + 1).copied();
                    if matches!(next, Some('$' | '}' | '\\')) {
                        self.text.push(next.unwrap_or_default());
                        self.at += 2;
                    } else {
                        self.text.push('\\');
                        self.at += 1;
                    }
                }
                '}' if nested => return,
                '$' => self.dollar(),
                other => {
                    self.text.push(other);
                    self.at += 1;
                }
            }
        }
    }

    fn number(&mut self) -> Option<u32> {
        let start = self.at;
        while self
            .peek()
            .is_some_and(|character| character.is_ascii_digit())
        {
            self.at += 1;
        }
        self.chars[start..self.at]
            .iter()
            .collect::<String>()
            .parse()
            .ok()
    }

    fn name(&mut self) -> String {
        let start = self.at;
        while self
            .peek()
            .is_some_and(|character| character.is_ascii_alphanumeric() || character == '_')
        {
            self.at += 1;
        }
        self.chars[start..self.at].iter().collect()
    }

    fn dollar(&mut self) {
        let start = self.at;
        self.at += 1;
        match self.peek() {
            // $1
            Some(character) if character.is_ascii_digit() => {
                let index = self.number().unwrap_or_default();
                let at = self.text.len();
                self.stops.push((index, at..at));
            }
            // $NAME
            Some(character) if character.is_ascii_alphabetic() || character == '_' => {
                let name = self.name();
                let value = (self.variable)(&name).unwrap_or_default();
                self.text.push_str(&value);
            }
            Some('{') => {
                self.at += 1;
                self.braced(start);
            }
            // A lone dollar sign.
            _ => self.text.push('$'),
        }
    }

    /// After `${`: a tab stop, placeholder, choice or variable.
    fn braced(&mut self, start: usize) {
        if self
            .peek()
            .is_some_and(|character| character.is_ascii_digit())
        {
            let index = self.number().unwrap_or_default();
            let at = self.text.len();
            match self.peek() {
                Some('}') => {
                    self.at += 1;
                    self.stops.push((index, at..at));
                }
                Some(':') => {
                    self.at += 1;
                    self.content(true);
                    self.at += 1; // the closing brace
                    self.stops.push((index, at..self.text.len()));
                }
                Some('|') => {
                    self.at += 1;
                    let mut first = String::new();
                    let mut done = false;
                    while let Some(character) = self.peek() {
                        self.at += 1;
                        match character {
                            '\\' => {
                                if let Some(escaped) = self.peek() {
                                    self.at += 1;
                                    if !done {
                                        first.push(escaped);
                                    }
                                }
                            }
                            ',' => done = true,
                            '|' if self.peek() == Some('}') => {
                                self.at += 1;
                                break;
                            }
                            other if !done => first.push(other),
                            _ => {}
                        }
                    }
                    self.text.push_str(&first);
                    self.stops.push((index, at..self.text.len()));
                }
                _ => self.literal(start),
            }
            return;
        }
        let name = self.name();
        if name.is_empty() {
            return self.literal(start);
        }
        let value = (self.variable)(&name);
        match self.peek() {
            Some('}') => {
                self.at += 1;
                self.text.push_str(&value.unwrap_or_default());
            }
            Some(':') => {
                self.at += 1;
                match value {
                    Some(value) => {
                        // Skip the default, keeping nested braces balanced.
                        let mut scratch = Parser {
                            chars: std::mem::take(&mut self.chars),
                            at: self.at,
                            text: String::new(),
                            stops: Vec::new(),
                            variable: self.variable,
                        };
                        scratch.content(true);
                        self.at = scratch.at + 1;
                        self.chars = scratch.chars;
                        self.text.push_str(&value);
                    }
                    None => {
                        self.content(true);
                        self.at += 1;
                    }
                }
            }
            // A transform such as `${TM_FILENAME/(.*)/$1/}`: the value as is.
            Some('/') => {
                let mut slashes = 0;
                while let Some(character) = self.peek() {
                    self.at += 1;
                    match character {
                        '\\' => self.at += 1,
                        '/' => slashes += 1,
                        '}' if slashes >= 3 => break,
                        _ => {}
                    }
                }
                self.text.push_str(&value.unwrap_or_default());
            }
            _ => self.literal(start),
        }
    }

    /// Not a snippet construct after all: keep the characters as written.
    fn literal(&mut self, start: usize) {
        self.at = start + 1;
        self.text.push('$');
    }
}

/// The value of the variables VS Code fills in snippets, for a file.
pub fn variables(
    file_name: &str,
    selected: &str,
    line: &str,
    clipboard: Option<String>,
) -> impl Fn(&str) -> Option<String> {
    let file_name = file_name.to_owned();
    let selected = selected.to_owned();
    let line = line.to_owned();
    move |name| {
        let base = file_name
            .rsplit_once('.')
            .map_or(file_name.as_str(), |(base, _)| base);
        Some(match name {
            "TM_FILENAME" => file_name.clone(),
            "TM_FILENAME_BASE" => base.to_owned(),
            "TM_SELECTED_TEXT" | "SELECTION" => selected.clone(),
            "TM_CURRENT_LINE" => line.clone(),
            "TM_CURRENT_WORD" => String::new(),
            "CLIPBOARD" => clipboard.clone().unwrap_or_default(),
            "UUID" => uuid_v4(),
            "RANDOM" => format!("{:06}", random() % 1_000_000),
            "RANDOM_HEX" => format!("{:06x}", random() & 0xff_ffff),
            "LINE_COMMENT" => "//".to_owned(),
            "BLOCK_COMMENT_START" => "/*".to_owned(),
            "BLOCK_COMMENT_END" => "*/".to_owned(),
            _ => return date_variable(name),
        })
    }
}

fn random() -> u64 {
    use std::hash::{BuildHasher as _, Hasher as _};
    let mut hasher = std::collections::hash_map::RandomState::new().build_hasher();
    hasher.write_u128(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|time| time.as_nanos())
            .unwrap_or_default(),
    );
    hasher.finish()
}

fn uuid_v4() -> String {
    let (a, b) = (random(), random());
    format!(
        "{:08x}-{:04x}-4{:03x}-{:04x}-{:012x}",
        a >> 32,
        (a >> 16) & 0xffff,
        a & 0x0fff,
        ((b >> 48) & 0x3fff) | 0x8000,
        b & 0xffff_ffff_ffff
    )
}

/// `CURRENT_YEAR`, `CURRENT_MONTH`... in local time.
fn date_variable(name: &str) -> Option<String> {
    // SAFETY: `localtime_r` fills the struct it is given.
    let now = unsafe {
        let mut seconds: libc::time_t = 0;
        libc::time(&mut seconds);
        let mut parts: libc::tm = std::mem::zeroed();
        libc::localtime_r(&seconds, &mut parts);
        parts
    };
    let (year, month, day) = (now.tm_year + 1900, now.tm_mon + 1, now.tm_mday);
    Some(match name {
        "CURRENT_YEAR" => year.to_string(),
        "CURRENT_YEAR_SHORT" => format!("{:02}", year % 100),
        "CURRENT_MONTH" => format!("{month:02}"),
        "CURRENT_DATE" => format!("{day:02}"),
        "CURRENT_HOUR" => format!("{:02}", now.tm_hour),
        "CURRENT_MINUTE" => format!("{:02}", now.tm_min),
        "CURRENT_SECOND" => format!("{:02}", now.tm_sec),
        "CURRENT_SECONDS_UNIX" => {
            let seconds = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|time| time.as_secs())
                .unwrap_or_default();
            seconds.to_string()
        }
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(source: &str) -> Snippet {
        Snippet::parse(source, &|name| match name {
            "TM_FILENAME" => Some("page.tsx".to_owned()),
            _ => None,
        })
    }

    fn places(snippet: &Snippet) -> Vec<Vec<&str>> {
        snippet
            .stops
            .iter()
            .map(|ranges| {
                ranges
                    .iter()
                    .map(|range| &snippet.text[range.clone()])
                    .collect()
            })
            .collect()
    }

    #[test]
    fn tab_stops_and_placeholders() {
        let snippet = parse("console.log($1);$0");
        assert_eq!(snippet.text, "console.log();");
        assert_eq!(snippet.stops, vec![vec![12..12], vec![14..14]]);

        let snippet =
            parse("for (let ${1:i} = 0; ${1:i} < ${2:items}.length; ${1:i}++) {\n\t$0\n}");
        assert_eq!(
            snippet.text,
            "for (let i = 0; i < items.length; i++) {\n\t\n}"
        );
        assert_eq!(
            places(&snippet),
            vec![vec!["i", "i", "i"], vec!["items"], vec![""]]
        );
    }

    #[test]
    fn nested_placeholders_choices_and_escapes() {
        let snippet = parse("${1:const ${2:name}} = ${3|useState,useRef|}(\\$0 \\} \\\\);");
        assert_eq!(snippet.text, "const name = useState($0 } \\);");
        assert_eq!(
            places(&snippet),
            vec![vec!["const name"], vec!["name"], vec!["useState"]]
        );
    }

    #[test]
    fn variables_fill_in() {
        let snippet = parse("export default function ${1:${TM_FILENAME/(.*)/$1/}}() {}");
        assert_eq!(snippet.text, "export default function page.tsx() {}");
        let snippet = parse("// $TM_FILENAME ${UNKNOWN:fallback} ${TM_FILENAME:ignored}");
        assert_eq!(snippet.text, "// page.tsx fallback page.tsx");
    }

    #[test]
    fn plain_dollars_stay() {
        assert_eq!(parse("costa 5$ e ${").text, "costa 5$ e ${");
        assert_eq!(parse("$").text, "$");
        assert!(!parse("niente").has_stops());
    }

    #[test]
    fn the_file_and_date_variables() {
        let values = variables("booking-card.tsx", "sel", "  riga", None);
        assert_eq!(values("TM_FILENAME_BASE").as_deref(), Some("booking-card"));
        assert_eq!(values("TM_SELECTED_TEXT").as_deref(), Some("sel"));
        assert_eq!(values("CURRENT_YEAR").map(|year| year.len()), Some(4));
        assert_eq!(values("UUID").map(|uuid| uuid.len()), Some(36));
        assert_eq!(values("NOPE"), None);
    }
}
