//! Searching and replacing across a project's files, as VS Code's Search
//! view: case, whole words and regular expressions, folders to include and
//! exclude, and the files git ignores left out.

use std::fs;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use globset::{Glob, GlobSet, GlobSetBuilder};
use ignore::{WalkBuilder, WalkState};
use regex::{Regex, RegexBuilder};

/// The most matches a search collects; the view says when there are more.
pub const MATCH_LIMIT: usize = 20_000;
/// Files larger than this are skipped, like minified bundles and dumps.
const FILE_LIMIT: u64 = 5 * 1024 * 1024;
/// The longest part of a line kept for showing.
const LINE_PREVIEW: usize = 300;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Query {
    pub text: String,
    pub case_sensitive: bool,
    pub whole_word: bool,
    pub regex: bool,
    /// Glob patterns, comma separated as in VS Code: `src/**, *.ts`.
    pub include: String,
    pub exclude: String,
}

impl Query {
    /// The pattern to look for, or the reason it is not valid.
    pub fn pattern(&self) -> Result<Regex, String> {
        let body = if self.regex {
            self.text.clone()
        } else {
            regex::escape(&self.text)
        };
        let body = if self.whole_word {
            format!(r"\b(?:{body})\b")
        } else {
            body
        };
        RegexBuilder::new(&body)
            .case_insensitive(!self.case_sensitive)
            .multi_line(true)
            .build()
            .map_err(|error| error.to_string())
    }
}

/// A line with matches.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LineMatch {
    /// Counted from 0.
    pub line: usize,
    /// The line, cut around the matches when it is very long.
    pub text: String,
    /// Where the matches are in `text`, in bytes.
    pub ranges: Vec<Range<usize>>,
    /// Where the first match starts in the whole line, in characters,
    /// for putting the cursor there.
    pub column: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileMatches {
    /// Relative to the project, with `/`.
    pub path: String,
    pub lines: Vec<LineMatch>,
}

impl FileMatches {
    pub fn count(&self) -> usize {
        self.lines.iter().map(|line| line.ranges.len()).sum()
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Results {
    pub files: Vec<FileMatches>,
    /// True when the search stopped at [`MATCH_LIMIT`].
    pub limited: bool,
}

impl Results {
    pub fn count(&self) -> usize {
        self.files.iter().map(FileMatches::count).sum()
    }
}

fn glob_set(patterns: &str) -> Option<GlobSet> {
    let mut builder = GlobSetBuilder::new();
    let mut any = false;
    for pattern in patterns
        .split(',')
        .map(str::trim)
        .filter(|pattern| !pattern.is_empty())
    {
        // `src` means everything in `src`, as in VS Code.
        let pattern = pattern.trim_start_matches("./").trim_end_matches('/');
        for glob in [pattern.to_owned(), format!("{pattern}/**")] {
            let glob = if glob.starts_with("**/") || glob.starts_with('/') {
                glob
            } else {
                format!("{{{glob},**/{glob}}}")
            };
            if let Ok(glob) = Glob::new(&glob) {
                builder.add(glob);
                any = true;
            }
        }
    }
    any.then(|| builder.build().ok()).flatten()
}

/// Searches the project. `excludes` are the hidden files of the Explorer;
/// `stop` ends the search early when it turns true.
pub fn search(
    root: &Path,
    query: &Query,
    excludes: &[String],
    stop: &AtomicBool,
) -> Result<Results, String> {
    if query.text.is_empty() {
        return Ok(Results::default());
    }
    let pattern = query.pattern()?;
    let mut hidden = GlobSetBuilder::new();
    for glob in excludes
        .iter()
        .filter_map(|pattern| Glob::new(pattern).ok())
    {
        hidden.add(glob);
    }
    let hidden = hidden.build().unwrap_or_else(|_| GlobSet::empty());
    let include = glob_set(&query.include);
    let exclude = glob_set(&query.exclude);

    let found: Arc<Mutex<Vec<FileMatches>>> = Arc::default();
    let total = Arc::new(AtomicUsize::new(0));
    let base = root.to_path_buf();
    let filter_base = base.clone();
    WalkBuilder::new(root)
        .hidden(false)
        .ignore(false)
        .filter_entry(move |entry| {
            let relative = entry
                .path()
                .strip_prefix(&filter_base)
                .unwrap_or(entry.path());
            relative.as_os_str().is_empty() || !hidden.is_match(relative)
        })
        .build_parallel()
        .run(|| {
            let found = found.clone();
            let total = total.clone();
            let pattern = pattern.clone();
            let include = include.clone();
            let exclude = exclude.clone();
            let base = base.clone();
            Box::new(move |entry| {
                if stop.load(Ordering::Relaxed) || total.load(Ordering::Relaxed) >= MATCH_LIMIT {
                    return WalkState::Quit;
                }
                let Ok(entry) = entry else {
                    return WalkState::Continue;
                };
                if !entry.file_type().is_some_and(|kind| kind.is_file()) {
                    return WalkState::Continue;
                }
                let path = entry.path();
                let relative = path.strip_prefix(&base).unwrap_or(path);
                if include
                    .as_ref()
                    .is_some_and(|include| !include.is_match(relative))
                    || exclude
                        .as_ref()
                        .is_some_and(|exclude| exclude.is_match(relative))
                {
                    return WalkState::Continue;
                }
                if let Some(lines) = search_file(path, &pattern) {
                    total.fetch_add(
                        lines.iter().map(|line| line.ranges.len()).sum(),
                        Ordering::Relaxed,
                    );
                    found.lock().unwrap().push(FileMatches {
                        path: relative.to_string_lossy().into_owned(),
                        lines,
                    });
                }
                WalkState::Continue
            })
        });
    let mut files = std::mem::take(&mut *found.lock().unwrap());
    files.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(Results {
        limited: total.load(Ordering::Relaxed) >= MATCH_LIMIT,
        files,
    })
}

/// The matching lines of a text file; `None` for no match, a binary file
/// or a file too large.
fn search_file(path: &Path, pattern: &Regex) -> Option<Vec<LineMatch>> {
    if fs::metadata(path).ok()?.len() > FILE_LIMIT {
        return None;
    }
    let bytes = fs::read(path).ok()?;
    if bytes[..bytes.len().min(8000)].contains(&0) {
        return None;
    }
    let text = String::from_utf8(bytes).ok()?;
    let lines = matching_lines(&text, pattern);
    (!lines.is_empty()).then_some(lines)
}

/// The lines of `text` where `pattern` matches.
pub fn matching_lines(text: &str, pattern: &Regex) -> Vec<LineMatch> {
    let mut lines = Vec::new();
    for (number, line) in text.lines().enumerate() {
        let ranges: Vec<Range<usize>> = pattern
            .find_iter(line)
            .filter(|found| !found.is_empty())
            .map(|found| found.range())
            .collect();
        let Some(first) = ranges.first() else {
            continue;
        };
        let column = line[..first.start].chars().count();
        // Very long lines show a window around the first match.
        let (text, offset) = if line.len() > LINE_PREVIEW {
            let mut start = first.start.saturating_sub(40);
            while !line.is_char_boundary(start) {
                start -= 1;
            }
            let mut end = (start + LINE_PREVIEW).min(line.len());
            while !line.is_char_boundary(end) {
                end -= 1;
            }
            (line[start..end].to_owned(), start)
        } else {
            (line.to_owned(), 0)
        };
        let ranges = ranges
            .into_iter()
            .filter(|range| range.start >= offset && range.end <= offset + text.len())
            .map(|range| range.start - offset..range.end - offset)
            .collect();
        lines.push(LineMatch {
            line: number,
            text,
            ranges,
            column,
        });
    }
    lines
}

/// The text after replacing every match. With a regular expression, `$1`
/// and `${name}` bring in the captured groups.
pub fn replace_all(
    text: &str,
    query: &Query,
    replacement: &str,
) -> Result<(String, usize), String> {
    let pattern = query.pattern()?;
    let count = pattern
        .find_iter(text)
        .filter(|found| !found.is_empty())
        .count();
    let replaced = if query.regex {
        pattern.replace_all(text, replacement).into_owned()
    } else {
        pattern
            .replace_all(text, regex::NoExpand(replacement))
            .into_owned()
    };
    Ok((replaced, count))
}

/// How a line reads after the replacement, for the preview.
pub fn preview_replacement(line: &str, query: &Query, replacement: &str) -> String {
    replace_all(line, query, replacement)
        .map(|(text, _)| text)
        .unwrap_or_else(|_| line.to_owned())
}

/// Replaces in a file on disk, written in one step. Returns how many
/// matches were replaced.
pub fn replace_in_file(path: &PathBuf, query: &Query, replacement: &str) -> Result<usize, String> {
    let text = fs::read_to_string(path).map_err(|error| error.to_string())?;
    let (replaced, count) = replace_all(&text, query, replacement)?;
    if count == 0 {
        return Ok(0);
    }
    let temporary = path.with_extension("keel-replace");
    fs::write(&temporary, replaced).map_err(|error| error.to_string())?;
    fs::rename(&temporary, path).map_err(|error| error.to_string())?;
    Ok(count)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn query(text: &str) -> Query {
        Query {
            text: text.to_owned(),
            ..Query::default()
        }
    }

    fn project() -> (tempfile::TempDir, PathBuf) {
        let folder = tempfile::tempdir().unwrap();
        let root = folder.path().to_path_buf();
        for dir in [".git", "src/components", "node_modules/react", "docs"] {
            fs::create_dir_all(root.join(dir)).unwrap();
        }
        fs::write(root.join(".gitignore"), "node_modules\n").unwrap();
        fs::write(
            root.join("src/components/booking-card.tsx"),
            "const total = guests.reduce(add, 0);\nexport function BookingCard() {}\n// total of the booking\n",
        )
        .unwrap();
        fs::write(root.join("src/app.ts"), "let totalPrice = 1;\n").unwrap();
        fs::write(root.join("docs/notes.md"), "Total: perché\n").unwrap();
        fs::write(root.join("node_modules/react/index.js"), "total\n").unwrap();
        fs::write(
            root.join("logo.png"),
            [0x89, b'P', 0, 0, b't', b'o', b't', b'a', b'l'],
        )
        .unwrap();
        (folder, root)
    }

    fn excludes() -> Vec<String> {
        crate::DEFAULT_EXCLUDES
            .iter()
            .map(|glob| glob.to_string())
            .collect()
    }

    fn summary(results: &Results) -> Vec<(String, Vec<usize>)> {
        results
            .files
            .iter()
            .map(|file| {
                (
                    file.path.clone(),
                    file.lines.iter().map(|line| line.line).collect(),
                )
            })
            .collect()
    }

    #[test]
    fn finds_words_leaving_out_ignored_and_binary_files() {
        let (_folder, root) = project();
        let stop = AtomicBool::new(false);
        let results = search(&root, &query("total"), &excludes(), &stop).unwrap();
        assert_eq!(
            summary(&results),
            [
                ("docs/notes.md".to_owned(), vec![0]),
                ("src/app.ts".to_owned(), vec![0]),
                ("src/components/booking-card.tsx".to_owned(), vec![0, 2]),
            ]
        );
        assert_eq!(results.count(), 4);
        let first = &results.files[2].lines[0];
        assert_eq!(&first.text[first.ranges[0].clone()], "total");
        assert_eq!(first.column, 6);
    }

    #[test]
    fn case_whole_words_regex_and_folders() {
        let (_folder, root) = project();
        let stop = AtomicBool::new(false);
        let search_with =
            |query: Query| summary(&search(&root, &query, &excludes(), &stop).unwrap());

        let mut case = query("Total");
        case.case_sensitive = true;
        assert_eq!(search_with(case), [("docs/notes.md".to_owned(), vec![0])]);

        let mut word = query("total");
        word.whole_word = true;
        assert_eq!(
            search_with(word).len(),
            2,
            "totalPrice is not the word total"
        );

        let mut regex = query(r"total\w+");
        regex.regex = true;
        assert_eq!(search_with(regex), [("src/app.ts".to_owned(), vec![0])]);

        let mut folders = query("total");
        folders.include = "src".to_owned();
        folders.exclude = "*.ts".to_owned();
        assert_eq!(
            search_with(folders),
            [("src/components/booking-card.tsx".to_owned(), vec![0, 2])]
        );

        let mut broken = query("(");
        broken.regex = true;
        assert!(search(&root, &broken, &excludes(), &stop).is_err());
    }

    #[test]
    fn replacing_keeps_literals_or_uses_groups() {
        let (_folder, root) = project();
        let mut literal = query("total");
        let file = root.join("src/components/booking-card.tsx");
        assert_eq!(replace_in_file(&file, &literal, "sum$1").unwrap(), 2);
        assert!(
            fs::read_to_string(&file)
                .unwrap()
                .starts_with("const sum$1 = ")
        );

        literal.text = "perché".to_owned();
        assert_eq!(
            preview_replacement("Total: perché", &literal, "why"),
            "Total: why"
        );

        let mut groups = query(r"let (\w+) = (\d)");
        groups.regex = true;
        assert_eq!(
            replace_all("let totalPrice = 1;", &groups, "const $1 = $2").unwrap(),
            ("const totalPrice = 1;".to_owned(), 1)
        );
    }

    #[test]
    fn long_lines_show_the_part_around_the_match() {
        let line = format!("{}needle{}", "a".repeat(1000), "b".repeat(1000));
        let lines = matching_lines(&line, &query("needle").pattern().unwrap());
        let found = &lines[0];
        assert!(found.text.len() <= LINE_PREVIEW);
        assert_eq!(&found.text[found.ranges[0].clone()], "needle");
        assert_eq!(found.column, 1000);
    }
}

/// Times a search on a real project:
/// `KEEL_TIMING_PROJECT=/path cargo test -p files --release -- --ignored --nocapture`.
#[cfg(test)]
#[test]
#[ignore]
fn timing_search_on_a_real_project() {
    let Some(root) = std::env::var_os("KEEL_TIMING_PROJECT") else {
        return;
    };
    let excludes: Vec<String> = crate::DEFAULT_EXCLUDES
        .iter()
        .map(|glob| glob.to_string())
        .collect();
    for word in ["booking", "useState", "z"] {
        let query = Query {
            text: word.to_owned(),
            ..Query::default()
        };
        let started = std::time::Instant::now();
        let results = search(Path::new(&root), &query, &excludes, &AtomicBool::new(false)).unwrap();
        println!(
            "\"{word}\": {} matches in {} files in {:?}{}",
            results.count(),
            results.files.len(),
            started.elapsed(),
            if results.limited { " (limit)" } else { "" }
        );
    }
}
