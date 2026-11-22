//! VS Code's problem matchers: the patterns that find errors in a task's
//! output. The built-in ones by name (`$tsc`, `$rustc`, `$eslint-stylish`…)
//! and those a `tasks.json` writes itself.

use lsp::lsp_types::DiagnosticSeverity;
use regex::Regex;
use serde_json::Value;

/// Which files a matcher's problems are for: some tools report what a
/// language server says already about the open ones.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum AppliesTo {
    All,
    Open,
    Closed,
}

/// The groups of a pattern's regular expression.
#[derive(Clone, Debug, Default)]
struct Groups {
    file: Option<usize>,
    /// `line`, `line,column` or the four of a range, in one group.
    location: Option<usize>,
    line: Option<usize>,
    column: Option<usize>,
    end_line: Option<usize>,
    end_column: Option<usize>,
    severity: Option<usize>,
    code: Option<usize>,
    message: Option<usize>,
}

#[derive(Clone, Debug)]
struct Pattern {
    regexp: Regex,
    groups: Groups,
    /// The last pattern of several, matched as long as lines go on.
    repeats: bool,
    /// The problem is the file's, not a line's.
    whole_file: bool,
}

/// What says that a task which never ends has begun and ended a round.
#[derive(Clone, Debug)]
pub(super) struct Background {
    pub(super) active_on_start: bool,
    pub(super) begins: Option<Regex>,
    pub(super) ends: Option<Regex>,
}

#[derive(Clone, Debug)]
pub(super) struct Matcher {
    pub(super) source: Option<String>,
    severity: DiagnosticSeverity,
    pub(super) applies_to: AppliesTo,
    /// The folder its relative files start from, when it names one.
    pub(super) base: Option<String>,
    patterns: Vec<Pattern>,
    pub(super) background: Option<Background>,
}

/// A problem found in the output; lines and columns count from 0.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Found {
    pub(super) file: String,
    pub(super) line: u32,
    pub(super) column: Option<u32>,
    pub(super) end: Option<(u32, u32)>,
    pub(super) severity: DiagnosticSeverity,
    pub(super) code: Option<String>,
    pub(super) message: String,
}

/// The named matchers, as VS Code and rust-analyzer's extension have them.
fn builtin(name: &str) -> Option<Value> {
    Some(match name {
        "$tsc" | "$tsc-watch" => {
            let mut matcher = serde_json::json!({
                "source": "ts",
                "applyTo": "closedDocuments",
                "pattern": {
                    "regexp": r"^([^\s].*)[\(:](\d+)[,:](\d+)(?:\):\s+|\s+-\s+)(error|warning|info)\s+TS(\d+)\s*:\s*(.*)$",
                    "file": 1, "line": 2, "column": 3, "severity": 4, "code": 5, "message": 6,
                },
            });
            if name == "$tsc-watch" {
                matcher["background"] = serde_json::json!({
                    "activeOnStart": true,
                    "beginsPattern": r"(Starting compilation in watch mode|File change detected\. Starting incremental compilation)\.\.\.",
                    "endsPattern": r"(Compilation complete\.|Found \d+ errors?\.) Watching for file changes\.",
                });
            }
            matcher
        }
        "$eslint-compact" => serde_json::json!({
            "source": "eslint",
            "applyTo": "allDocuments",
            "pattern": {
                "regexp": r"^(.+):\sline\s(\d+),\scol\s(\d+),\s(Error|Warning|Info)\s-\s(.+)\s\((.+)\)$",
                "file": 1, "line": 2, "column": 3, "severity": 4, "message": 5, "code": 6,
            },
        }),
        "$eslint-stylish" => serde_json::json!({
            "source": "eslint",
            "applyTo": "allDocuments",
            "pattern": [
                { "regexp": r"^([^\s].*)$", "file": 1 },
                {
                    "regexp": r"^\s+(\d+):(\d+)\s+(error|warning|info)\s+(.+?)(?:\s\s+(\S.*))?$",
                    "line": 1, "column": 2, "severity": 3, "message": 4, "code": 5, "loop": true,
                },
            ],
        }),
        "$gcc" => serde_json::json!({
            "source": "gcc",
            "applyTo": "allDocuments",
            "pattern": {
                "regexp": r"^(.*?):(\d+):(\d*):?\s+(?:fatal\s+)?(warning|error):\s+(.*)$",
                "file": 1, "line": 2, "column": 3, "severity": 4, "message": 5,
            },
        }),
        "$go" => serde_json::json!({
            "source": "go",
            "applyTo": "allDocuments",
            "pattern": {
                "regexp": r"^([^:]*: )?((.:)?[^:]*):(\d+)(:(\d+))?: (.*)$",
                "file": 2, "line": 4, "column": 6, "message": 7,
            },
        }),
        "$mscompile" => serde_json::json!({
            "source": "cs",
            "applyTo": "allDocuments",
            "pattern": {
                "regexp": r"^\s*(?:\d+>)?(\S.*?)\((\d+|\d+,\d+|\d+,\d+,\d+,\d+)\)\s*:\s+(?:(\S+)\s+)?((?:fatal +)?error|warning|info)\s+(\w+\d+)?\s*:\s*(.*)$",
                "file": 1, "location": 2, "severity": 4, "code": 5, "message": 6,
            },
        }),
        "$rustc" | "$rustc-watch" => {
            let mut matcher = serde_json::json!({
                "source": "rustc",
                "applyTo": "allDocuments",
                "pattern": [
                    {
                        "regexp": r"^(warning|warn|error)(?:\[(.*?)\])?: (.*)$",
                        "severity": 1, "code": 2, "message": 3,
                    },
                    {
                        "regexp": r"^[\s\->=]*(.*?):(\d+):(\d+)\s*$",
                        "file": 1, "line": 2, "column": 3,
                    },
                ],
            });
            if name == "$rustc-watch" {
                matcher["background"] = serde_json::json!({
                    "beginsPattern": r"^\[Running\b",
                    "endsPattern": r"^(\[Finished running\b|\[Process\b)",
                });
            }
            matcher
        }
        "$rust-panic" => serde_json::json!({
            "source": "rust",
            "applyTo": "allDocuments",
            "pattern": [
                {
                    "regexp": r"^thread '.*?' (?:\(\d+\) )?panicked at (.*?):(\d+):(\d+):$",
                    "file": 1, "line": 2, "column": 3,
                },
                { "regexp": r"^(.+)$", "message": 1 },
            ],
        }),
        _ => return None,
    })
}

/// A regular expression as JavaScript writes it. Rust's do not take a
/// class such as `\s` at the start of a range, which JavaScript reads as
/// a plain `-`.
fn regex(source: &str) -> Result<Regex, String> {
    Regex::new(source)
        .or_else(|error| {
            let fixed = ["s", "S", "w", "W", "d", "D"]
                .iter()
                .fold(source.to_owned(), |fixed, class| {
                    fixed.replace(&format!("\\{class}-"), &format!("\\{class}\\-"))
                });
            if fixed == source {
                Err(error)
            } else {
                Regex::new(&fixed)
            }
        })
        .map_err(|error| {
            let text = error.to_string();
            text.lines().last().unwrap_or(&text).trim().to_owned()
        })
}

fn severity_of(word: &str) -> Option<DiagnosticSeverity> {
    let word = word.trim().to_lowercase();
    Some(match word.as_str() {
        "e" => DiagnosticSeverity::ERROR,
        "w" | "warn" => DiagnosticSeverity::WARNING,
        "i" | "info" | "note" | "information" => DiagnosticSeverity::INFORMATION,
        "hint" => DiagnosticSeverity::HINT,
        word if word.contains("error") || word.contains("fatal") => DiagnosticSeverity::ERROR,
        word if word.contains("warning") => DiagnosticSeverity::WARNING,
        _ => return None,
    })
}

/// The matchers of a task's `problemMatcher`: a name, an object, or a
/// list of both; and what could not be read.
pub(super) fn matchers(value: Option<&Value>) -> (Vec<Matcher>, Vec<String>) {
    let mut found = Vec::new();
    let mut errors = Vec::new();
    let values: Vec<&Value> = match value {
        None | Some(Value::Null) => Vec::new(),
        Some(Value::Array(values)) => values.iter().collect(),
        Some(value) => vec![value],
    };
    for value in values {
        match matcher(value) {
            Ok(matcher) => found.push(matcher),
            Err(error) => errors.push(error),
        }
    }
    (found, errors)
}

fn matcher(value: &Value) -> Result<Matcher, String> {
    let resolved;
    let value = match value {
        Value::String(name) => {
            resolved = builtin(name).ok_or_else(|| name.clone())?;
            &resolved
        }
        Value::Object(own) => match own.get("base").and_then(Value::as_str) {
            // Over a named one, what the object says wins.
            Some(base) => {
                let mut merged = builtin(base).ok_or_else(|| base.to_owned())?;
                for (key, value) in own {
                    merged[key] = value.clone();
                }
                resolved = merged;
                &resolved
            }
            None => value,
        },
        other => return Err(other.to_string()),
    };
    // A pattern can be a named matcher's too: `"pattern": "$eslint-stylish"`.
    let pattern = value.get("pattern").ok_or("pattern")?;
    let named;
    let pattern = match pattern {
        Value::String(name) => {
            named = builtin(name).ok_or_else(|| name.clone())?;
            named.get("pattern").ok_or("pattern")?
        }
        pattern => pattern,
    };
    let patterns: Vec<&Value> = match pattern {
        Value::Array(patterns) => patterns.iter().collect(),
        pattern => vec![pattern],
    };
    let count = patterns.len();
    let patterns = patterns
        .into_iter()
        .enumerate()
        .map(|(index, value)| pattern_of(value, count, index))
        .collect::<Result<Vec<_>, String>>()?;
    if patterns.is_empty() {
        return Err("pattern".to_owned());
    }
    let background = value.get("background").or_else(|| value.get("watching"));
    let background = match background {
        Some(background) => Some(Background {
            active_on_start: background["activeOnStart"].as_bool().unwrap_or(false),
            begins: edge(&background["beginsPattern"])?,
            ends: edge(&background["endsPattern"])?,
        }),
        None => None,
    };
    Ok(Matcher {
        source: value["source"].as_str().map(str::to_owned),
        severity: value["severity"]
            .as_str()
            .and_then(severity_of)
            .unwrap_or(DiagnosticSeverity::ERROR),
        applies_to: match value["applyTo"].as_str() {
            Some("openDocuments") => AppliesTo::Open,
            Some("closedDocuments") => AppliesTo::Closed,
            _ => AppliesTo::All,
        },
        // ["relative", "${workspaceFolder}/app"] names a folder.
        base: value["fileLocation"]
            .as_array()
            .and_then(|location| location.get(1))
            .and_then(Value::as_str)
            .map(str::to_owned),
        patterns,
        background,
    })
}

/// `beginsPattern` and `endsPattern`: the expression, or an object with it.
fn edge(value: &Value) -> Result<Option<Regex>, String> {
    match value.as_str().or(value["regexp"].as_str()) {
        Some(source) => regex(source).map(Some),
        None => Ok(None),
    }
}

fn pattern_of(value: &Value, count: usize, index: usize) -> Result<Pattern, String> {
    let source = value["regexp"].as_str().ok_or("regexp")?;
    let group = |name: &str| {
        value[name]
            .as_u64()
            .and_then(|group| usize::try_from(group).ok())
    };
    let mut groups = Groups {
        file: group("file"),
        location: group("location"),
        line: group("line"),
        column: group("column").or(group("character")),
        end_line: group("endLine"),
        end_column: group("endColumn").or(group("endCharacter")),
        severity: group("severity"),
        code: group("code"),
        message: group("message"),
    };
    // VS Code's defaults, for a pattern alone that names nothing.
    let names_nothing = groups.file.is_none()
        && groups.location.is_none()
        && groups.line.is_none()
        && groups.message.is_none();
    if count == 1 && names_nothing {
        groups.file = Some(1);
        groups.line = Some(2);
        groups.column = Some(3);
        groups.message = Some(0);
    }
    Ok(Pattern {
        regexp: regex(source)?,
        groups,
        repeats: value["loop"].as_bool().unwrap_or(false) && index + 1 == count && count > 1,
        whole_file: value["kind"].as_str() == Some("file"),
    })
}

/// What the lines of a problem said so far.
#[derive(Clone, Default)]
struct Said {
    file: Option<String>,
    location: Option<String>,
    line: Option<u32>,
    column: Option<u32>,
    end_line: Option<u32>,
    end_column: Option<u32>,
    severity: Option<DiagnosticSeverity>,
    code: Option<String>,
    message: Option<String>,
    whole_file: bool,
}

impl Said {
    /// Takes what a pattern's groups caught; `false` when it did not match.
    fn take(&mut self, pattern: &Pattern, line: &str) -> bool {
        let Some(caught) = pattern.regexp.captures(line) else {
            return false;
        };
        let text = |group: Option<usize>| {
            caught
                .get(group?)
                .map(|found| found.as_str().trim().to_owned())
                .filter(|text| !text.is_empty())
        };
        let number = |group: Option<usize>| text(group)?.parse::<u32>().ok();
        let groups = &pattern.groups;
        self.file = text(groups.file).or(self.file.take());
        self.location = text(groups.location).or(self.location.take());
        self.line = number(groups.line).or(self.line);
        self.column = number(groups.column).or(self.column);
        self.end_line = number(groups.end_line).or(self.end_line);
        self.end_column = number(groups.end_column).or(self.end_column);
        self.severity = text(groups.severity)
            .and_then(|word| severity_of(&word))
            .or(self.severity);
        self.code = text(groups.code).or(self.code.take());
        if let Some(message) = text(groups.message) {
            self.message = Some(match self.message.take() {
                Some(before) if !pattern.repeats => format!("{before}\n{message}"),
                _ => message,
            });
        }
        self.whole_file |= pattern.whole_file;
        true
    }

    fn problem(&self, matcher: &Matcher) -> Option<Found> {
        let file = self.file.clone()?;
        let message = self.message.clone()?;
        let (mut line, mut column) = (self.line, self.column);
        let mut end = self
            .end_line
            .or(self.end_column.and(self.line))
            .zip(self.end_column);
        if let Some(location) = &self.location {
            let numbers: Vec<u32> = location
                .split([',', '.', '-'])
                .filter_map(|part| part.trim().parse().ok())
                .collect();
            line = numbers.first().copied();
            column = numbers.get(1).copied();
            end = numbers.get(2).copied().zip(numbers.get(3).copied());
        }
        let line = match line {
            Some(line) => line,
            None if self.whole_file => 1,
            None => return None,
        };
        Some(Found {
            file,
            line: line.saturating_sub(1),
            column: column.map(|column| column.saturating_sub(1)),
            end: end.map(|(line, column)| (line.saturating_sub(1), column.saturating_sub(1))),
            severity: self.severity.unwrap_or(matcher.severity),
            code: self.code.clone(),
            message,
        })
    }
}

impl Matcher {
    /// The problems in an output, in its order, each once.
    pub(super) fn find(&self, output: &str) -> Vec<Found> {
        let lines: Vec<&str> = output.lines().map(str::trim_end).collect();
        let mut found: Vec<Found> = Vec::new();
        let mut at = 0;
        while at < lines.len() {
            let mut said = Said::default();
            let mut next = at;
            let mut whole = true;
            for pattern in &self.patterns {
                if pattern.repeats {
                    // One problem for each line as long as they go on,
                    // each starting from what came before the loop.
                    let before = said.clone();
                    let mut any = false;
                    while let Some(line) = lines.get(next) {
                        let mut said = before.clone();
                        if !said.take(pattern, line) {
                            break;
                        }
                        found.extend(said.problem(self));
                        any = true;
                        next += 1;
                    }
                    whole = any;
                    continue;
                }
                match lines.get(next) {
                    Some(line) if said.take(pattern, line) => next += 1,
                    _ => {
                        whole = false;
                        break;
                    }
                }
            }
            if !whole {
                at += 1;
                continue;
            }
            let repeats = self.patterns.last().is_some_and(|last| last.repeats);
            if !repeats {
                found.extend(said.problem(self));
            }
            at = next.max(at + 1);
        }
        let mut once: Vec<Found> = Vec::with_capacity(found.len());
        for problem in found {
            if !once.contains(&problem) {
                once.push(problem);
            }
        }
        once
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn named(name: &str) -> Matcher {
        let (mut found, errors) = matchers(Some(&json!(name)));
        assert!(errors.is_empty(), "{name}: {errors:?}");
        found.remove(0)
    }

    #[test]
    fn every_named_matcher_reads() {
        for name in [
            "$tsc",
            "$tsc-watch",
            "$eslint-compact",
            "$eslint-stylish",
            "$gcc",
            "$go",
            "$mscompile",
            "$rustc",
            "$rustc-watch",
            "$rust-panic",
        ] {
            named(name);
        }
        let (found, errors) = matchers(Some(&json!(["$tsc", "$chissà"])));
        assert_eq!((found.len(), errors), (1, vec!["$chissà".to_owned()]));
        assert!(matchers(None).0.is_empty());
    }

    #[test]
    fn rustc_and_panics() {
        let output = "   Compiling app v0.1.0\nerror[E0308]: mismatched types\n --> src/main.rs:4:18\n  |\n4 |     let x: i32 = \"a\";\n  |                  ^^^ expected `i32`\n\nwarning: unused variable: `y`\n  --> src/lib.rs:10:9\n\nerror: could not compile `app` (bin \"app\") due to 1 previous error\n";
        let found = named("$rustc").find(output);
        assert_eq!(found.len(), 2, "{found:?}");
        assert_eq!(
            found[0],
            Found {
                file: "src/main.rs".into(),
                line: 3,
                column: Some(17),
                end: None,
                severity: DiagnosticSeverity::ERROR,
                code: Some("E0308".into()),
                message: "mismatched types".into(),
            }
        );
        assert_eq!(
            (found[1].line, found[1].severity),
            (9, DiagnosticSeverity::WARNING)
        );
        let panic = "running 1 test\n\nthread 'tests::adds' (123) panicked at src/lib.rs:12:9:\nassertion `left == right` failed\n  left: 1\n";
        let found = named("$rust-panic").find(panic);
        assert_eq!((found[0].file.as_str(), found[0].line), ("src/lib.rs", 11));
        assert_eq!(found[0].message, "assertion `left == right` failed");
    }

    #[test]
    fn typescript_eslint_gcc_and_go() {
        let tsc = named("$tsc");
        assert_eq!(tsc.applies_to, AppliesTo::Closed);
        let both = "src/a.ts(3,7): error TS2322: Type 'string' is not assignable.\nsrc/b.ts:10:2 - error TS1005: ';' expected.\n";
        let found = tsc.find(both);
        assert_eq!(found.len(), 2);
        assert_eq!((found[0].line, found[0].column), (2, Some(6)));
        assert_eq!(
            (found[1].file.as_str(), found[1].code.as_deref()),
            ("src/b.ts", Some("1005"))
        );

        let stylish = "\n/p/src/a.js\n   1:10  error    'x' is assigned a value but never used  no-unused-vars\n  12:1   warning  Unexpected console statement            no-console\n\n/p/src/b.js\n  3:5  error  Parsing error: Unexpected token\n\n✖ 3 problems (2 errors, 1 warning)\n";
        let found = named("$eslint-stylish").find(stylish);
        assert_eq!(found.len(), 3, "{found:?}");
        assert_eq!(found[0].message, "'x' is assigned a value but never used");
        assert_eq!(found[0].code.as_deref(), Some("no-unused-vars"));
        assert_eq!(found[1].severity, DiagnosticSeverity::WARNING);
        assert_eq!(
            (found[2].file.as_str(), found[2].line, found[2].code.clone()),
            ("/p/src/b.js", 2, None)
        );

        let found = named("$gcc").find("main.c:5:3: error: expected ';' before 'return'\n");
        assert_eq!((found[0].line, found[0].column), (4, Some(2)));
        let found = named("$go").find("./main.go:7:2: undefined: fmt.Printl\n");
        assert_eq!((found[0].file.as_str(), found[0].line), ("./main.go", 6));
        let found = named("$mscompile")
            .find("Program.cs(12,34): error CS1002: ; expected [/p/App.csproj]\n");
        assert_eq!(
            (found[0].line, found[0].column, found[0].code.as_deref()),
            (11, Some(33), Some("CS1002"))
        );
    }

    #[test]
    fn a_matcher_of_ones_own() {
        let value = json!({
            "owner": "mio",
            "source": "mio",
            "severity": "warning",
            "fileLocation": ["relative", "${workspaceFolder}/app"],
            "pattern": { "regexp": "^[\\s->=]*(\\S+):(\\d+): (.*)$", "file": 1, "line": 2, "message": 3 },
            "background": { "activeOnStart": true, "beginsPattern": "^parto", "endsPattern": { "regexp": "^fatto" } },
        });
        let (found, errors) = matchers(Some(&value));
        assert!(errors.is_empty(), "{errors:?}");
        let matcher = &found[0];
        assert_eq!(matcher.base.as_deref(), Some("${workspaceFolder}/app"));
        assert!(matcher.background.as_ref().unwrap().ends.is_some());
        let found = matcher.find("--> a/b.txt:9: così no\n");
        assert_eq!(found[0].severity, DiagnosticSeverity::WARNING);
        assert_eq!(
            (found[0].file.as_str(), found[0].line, found[0].column),
            ("a/b.txt", 8, None)
        );
        // On a named one, with its own severity.
        let (found, _) = matchers(Some(&json!({ "base": "$gcc", "applyTo": "openDocuments" })));
        assert_eq!(found[0].applies_to, AppliesTo::Open);
        // A look-ahead is not a Rust expression: said, not a crash.
        let (found, errors) = matchers(Some(&json!({ "pattern": { "regexp": "^(?!x)(.*)$" } })));
        assert!(found.is_empty() && errors.len() == 1);
    }
}
