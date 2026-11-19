//! More of VS Code's editing commands, on plain text: sort and unique
//! lines, join lines, change case, the matching bracket, expand the
//! selection, surround a selection; the depth of each bracket, for their
//! colours; the closing tag that follows its opening one when renamed; and
//! the other places a word appears.

use std::ops::Range;

use crate::line_edits::LineEdit;

/// The whole lines a selection touches, without the last line break.
fn lines_of(text: &str, selection: &Range<usize>) -> Range<usize> {
    let start = text[..selection.start.min(text.len())]
        .rfind('\n')
        .map_or(0, |at| at + 1);
    let mut end = selection.end.min(text.len());
    // A selection ending at a line's start does not take that line.
    if end > start && end > selection.start && text[..end].ends_with('\n') {
        end -= 1;
    }
    let end = text[end..].find('\n').map_or(text.len(), |at| end + at);
    start..end
}

/// Sorts the selected lines, or every line without a selection, as VS
/// Code's Sort Lines Ascending and Descending.
pub fn sort_lines(text: &str, selection: Range<usize>, descending: bool) -> Option<LineEdit> {
    let range = if selection.is_empty() {
        0..text.trim_end_matches('\n').len()
    } else {
        lines_of(text, &selection)
    };
    let mut lines: Vec<&str> = text[range.clone()].split('\n').collect();
    lines.sort_by(|a, b| natural(a).cmp(&natural(b)));
    if descending {
        lines.reverse();
    }
    let sorted = lines.join("\n");
    (sorted != text[range.clone()]).then(|| LineEdit {
        selection: range.start..range.start + sorted.len(),
        range,
        text: sorted,
    })
}

/// A line's sort key: ignoring case, so `b` sits near `B`.
fn natural(line: &str) -> (String, &str) {
    (line.to_lowercase(), line)
}

/// Deletes repeated lines among the selected ones, keeping the first.
pub fn unique_lines(text: &str, selection: Range<usize>) -> Option<LineEdit> {
    let range = if selection.is_empty() {
        0..text.trim_end_matches('\n').len()
    } else {
        lines_of(text, &selection)
    };
    let mut seen = std::collections::HashSet::new();
    let kept: Vec<&str> = text[range.clone()]
        .split('\n')
        .filter(|line| seen.insert(*line))
        .collect();
    let joined = kept.join("\n");
    (joined != text[range.clone()]).then(|| LineEdit {
        selection: range.start..range.start + joined.len(),
        range,
        text: joined,
    })
}

/// Joins the selected lines, or the cursor's line with the next, with one
/// space, as VS Code's Join Lines (Ctrl+J).
pub fn join_lines(text: &str, selection: Range<usize>) -> Option<LineEdit> {
    let mut range = lines_of(text, &selection);
    if !text[range.clone()].contains('\n') {
        // One line: it takes the next one up.
        let next_end = text[range.end..]
            .strip_prefix('\n')
            .map(|rest| range.end + 1 + rest.find('\n').unwrap_or(rest.len()))?;
        range.end = next_end;
    }
    let lines: Vec<&str> = text[range.clone()].split('\n').collect();
    let mut joined = lines[0].trim_end().to_owned();
    let mut cursor = joined.len();
    for line in &lines[1..] {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        cursor = joined.len();
        joined.push(' ');
        joined.push_str(line);
    }
    Some(LineEdit {
        selection: range.start + cursor..range.start + cursor,
        range,
        text: joined,
    })
}

/// How Transform changes a word.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Case {
    Upper,
    Lower,
    Title,
    Snake,
    Camel,
    Kebab,
}

/// The word around `offset`: letters, digits and `_`.
pub fn word_at(text: &str, offset: usize) -> Range<usize> {
    let word = |c: char| c.is_alphanumeric() || c == '_';
    let offset = offset.min(text.len());
    let start = text[..offset]
        .char_indices()
        .rev()
        .take_while(|(_, c)| word(*c))
        .last()
        .map_or(offset, |(at, _)| at);
    let end = text[offset..]
        .char_indices()
        .find(|(_, c)| !word(*c))
        .map_or(text.len(), |(at, _)| offset + at);
    start..end
}

/// The parts of an identifier: `parseHTTPRequest`, `parse_http_request`
/// and `parse-http-request` all give parse, http, request.
fn words(text: &str) -> Vec<String> {
    let mut words: Vec<String> = Vec::new();
    let mut current = String::new();
    let chars: Vec<char> = text.chars().collect();
    for (index, &c) in chars.iter().enumerate() {
        if !c.is_alphanumeric() {
            if !current.is_empty() {
                words.push(std::mem::take(&mut current));
            }
            continue;
        }
        let previous = index.checked_sub(1).map(|at| chars[at]);
        let next = chars.get(index + 1);
        let starts = c.is_uppercase()
            && previous.is_some_and(|previous| {
                previous.is_lowercase()
                    || previous.is_ascii_digit()
                    || (previous.is_uppercase() && next.is_some_and(|next| next.is_lowercase()))
            });
        if starts && !current.is_empty() {
            words.push(std::mem::take(&mut current));
        }
        current.extend(c.to_lowercase());
    }
    if !current.is_empty() {
        words.push(current);
    }
    words
}

/// Changes the case of the selection, or of the word at the cursor.
pub fn transform_case(text: &str, selection: Range<usize>, case: Case) -> Option<LineEdit> {
    let range = if selection.is_empty() {
        word_at(text, selection.start)
    } else {
        selection
    };
    if range.is_empty() {
        return None;
    }
    let original = &text[range.clone()];
    let capital = |word: &str| {
        let mut chars = word.chars();
        chars
            .next()
            .map(|first| first.to_uppercase().collect::<String>() + chars.as_str())
            .unwrap_or_default()
    };
    let changed = match case {
        Case::Upper => original.to_uppercase(),
        Case::Lower => original.to_lowercase(),
        Case::Title => original
            .split(' ')
            .map(|word| capital(&word.to_lowercase()))
            .collect::<Vec<_>>()
            .join(" "),
        Case::Snake => words(original).join("_"),
        Case::Kebab => words(original).join("-"),
        Case::Camel => {
            let parts = words(original);
            parts
                .iter()
                .enumerate()
                .map(|(index, word)| {
                    if index == 0 {
                        word.clone()
                    } else {
                        capital(word)
                    }
                })
                .collect()
        }
    };
    (changed != original).then(|| LineEdit {
        selection: range.start..range.start + changed.len(),
        range,
        text: changed,
    })
}

const PAIRS: [(char, char); 3] = [('(', ')'), ('[', ']'), ('{', '}')];

/// Where strings and comments are, so brackets inside them do not count.
fn code_mask(text: &str, language_id: &str) -> Vec<bool> {
    let hash_comments = matches!(
        language_id,
        "python" | "shellscript" | "yaml" | "toml" | "ruby" | "dockerfile" | "makefile"
    );
    let bytes = text.as_bytes();
    let mut code = vec![true; bytes.len()];
    let mut index = 0;
    while index < bytes.len() {
        let rest = &bytes[index..];
        let skip_to = if rest.starts_with(b"//") && !hash_comments {
            text[index..]
                .find('\n')
                .map_or(bytes.len(), |at| index + at)
        } else if rest.starts_with(b"/*") && !hash_comments {
            text[index + 2..]
                .find("*/")
                .map_or(bytes.len(), |at| index + 2 + at + 2)
        } else if rest[0] == b'#' && hash_comments {
            text[index..]
                .find('\n')
                .map_or(bytes.len(), |at| index + at)
        } else if matches!(rest[0], b'"' | b'\'' | b'`') {
            let quote = rest[0];
            let mut end = index + 1;
            while end < bytes.len() && bytes[end] != quote {
                // A quote alone in Rust is a lifetime, not a string.
                if bytes[end] == b'\n' && quote != b'`' {
                    break;
                }
                end += if bytes[end] == b'\\' { 2 } else { 1 };
            }
            (end + 1).min(bytes.len())
        } else {
            index += 1;
            continue;
        };
        for flag in &mut code[index..skip_to.min(bytes.len())] {
            *flag = false;
        }
        index = skip_to.max(index + 1);
    }
    code
}

/// Each bracket outside strings and comments, with its depth: what VS
/// Code's bracket pair colorization colours. Unmatched closings get none.
pub fn bracket_depths(text: &str, language_id: &str) -> Vec<(usize, usize)> {
    let code = code_mask(text, language_id);
    let mut stack: Vec<char> = Vec::new();
    let mut found = Vec::new();
    for (at, c) in text.char_indices() {
        if !code.get(at).copied().unwrap_or(false) {
            continue;
        }
        if PAIRS.iter().any(|(open, _)| *open == c) {
            found.push((at, stack.len()));
            stack.push(c);
        } else if let Some((open, _)) = PAIRS.iter().find(|(_, close)| *close == c)
            && stack.last() == Some(open)
        {
            stack.pop();
            found.push((at, stack.len()));
        }
    }
    found
}

/// The bracket matching the one at or just before `offset`.
pub fn matching_bracket(text: &str, offset: usize, language_id: &str) -> Option<usize> {
    let code = code_mask(text, language_id);
    let at = [offset, offset.saturating_sub(1)].into_iter().find(|at| {
        text[*at..]
            .chars()
            .next()
            .is_some_and(|c| PAIRS.iter().any(|(open, close)| c == *open || c == *close))
            && code.get(*at).copied().unwrap_or(false)
    })?;
    let c = text[at..].chars().next()?;
    if let Some((open, close)) = PAIRS.iter().find(|(open, _)| *open == c) {
        let mut depth = 0;
        for (index, next) in text[at..].char_indices() {
            if !code[at + index] {
                continue;
            }
            if next == *open {
                depth += 1;
            } else if next == *close {
                depth -= 1;
                if depth == 0 {
                    return Some(at + index);
                }
            }
        }
    } else if let Some((open, close)) = PAIRS.iter().find(|(_, close)| *close == c) {
        let mut depth = 0;
        for (index, previous) in text[..=at].char_indices().rev() {
            if !code[index] {
                continue;
            }
            if previous == *close {
                depth += 1;
            } else if previous == *open {
                depth -= 1;
                if depth == 0 {
                    return Some(index);
                }
            }
        }
    }
    None
}

/// Expand Selection: the word, then inside the quotes or brackets around
/// it, then with them, then the line, then everything.
pub fn expand_selection(text: &str, selection: Range<usize>) -> Range<usize> {
    let word = word_at(text, selection.start);
    if selection.is_empty() && !word.is_empty() {
        return word;
    }
    if word.start <= selection.start && word.end >= selection.end && word != selection {
        return word;
    }
    let mut candidates: Vec<Range<usize>> = Vec::new();
    // Quotes on the same line.
    let line = lines_of(text, &selection);
    for quote in ['"', '\'', '`'] {
        let before = text[line.start..selection.start]
            .rfind(quote)
            .map(|at| line.start + at);
        let after = text[selection.end..line.end]
            .find(quote)
            .map(|at| selection.end + at);
        if let (Some(open), Some(close)) = (before, after) {
            candidates.push(open + 1..close);
            candidates.push(open..close + 1);
        }
    }
    // The brackets around, inside then with them.
    for (open, close) in PAIRS {
        let mut depth = 0;
        let mut start = None;
        for (at, c) in text[..selection.start].char_indices().rev() {
            if c == close {
                depth += 1;
            } else if c == open {
                if depth == 0 {
                    start = Some(at);
                    break;
                }
                depth -= 1;
            }
        }
        let mut depth = 0;
        let mut end = None;
        for (at, c) in text[selection.end..].char_indices() {
            if c == open {
                depth += 1;
            } else if c == close {
                if depth == 0 {
                    end = Some(selection.end + at);
                    break;
                }
                depth -= 1;
            }
        }
        if let (Some(start), Some(end)) = (start, end) {
            candidates.push(start + 1..end);
            candidates.push(start..end + 1);
        }
    }
    candidates.push(line);
    candidates.push(0..text.len());
    candidates
        .into_iter()
        .filter(|range| {
            range.start <= selection.start && range.end >= selection.end && *range != selection
        })
        .min_by_key(|range| range.end - range.start)
        .unwrap_or(selection)
}

/// Typing a quote or a bracket over a selection puts the selection inside
/// them, as VS Code's `editor.autoSurround`.
pub fn surround(text: &str, selection: Range<usize>, typed: char) -> Option<LineEdit> {
    let close = match typed {
        '"' => '"',
        '\'' => '\'',
        '`' => '`',
        '(' => ')',
        '[' => ']',
        '{' => '}',
        _ => return None,
    };
    if selection.is_empty() {
        return None;
    }
    let inner = &text[selection.clone()];
    let wrapped = format!("{typed}{inner}{close}");
    Some(LineEdit {
        selection: selection.start + 1..selection.start + 1 + inner.len(),
        range: selection,
        text: wrapped,
    })
}

/// When an edit changed the name of an opening or closing tag, the same
/// change for its partner: the range of `old` to replace and the new name.
/// The partner is found by counting the tags of the same name between.
pub fn paired_tag_edit(old: &str, new: &str) -> Option<(Range<usize>, String)> {
    // The single changed span of the edit.
    let prefix = old
        .bytes()
        .zip(new.bytes())
        .take_while(|(a, b)| a == b)
        .count();
    let suffix = old[prefix..]
        .bytes()
        .rev()
        .zip(new[prefix..].bytes().rev())
        .take_while(|(a, b)| a == b)
        .count();
    let old_changed = prefix..old.len() - suffix;
    let name_char = |c: char| c.is_alphanumeric() || matches!(c, '-' | '_' | '.' | ':');
    // The tag name around the change, in the old text.
    let start = old[..old_changed.start]
        .char_indices()
        .rev()
        .take_while(|(_, c)| name_char(*c))
        .last()
        .map_or(old_changed.start, |(at, _)| at);
    let before = &old[..start];
    let closing = before.ends_with("</");
    if !closing && !before.ends_with('<') {
        return None;
    }
    let end = old[old_changed.end..]
        .char_indices()
        .find(|(_, c)| !name_char(*c))
        .map_or(old.len(), |(at, _)| old_changed.end + at);
    let old_name = &old[start..end];
    // The new name, at the same place in the new text.
    let new_end = end + new.len() - old.len();
    let new_name = new.get(start..new_end)?;
    if old_name.is_empty() || new_name == old_name || !new_name.chars().all(name_char) {
        return None;
    }
    fn tag_at(text: &str, at: usize) -> Option<(bool, &str)> {
        let name_char = |c: char| c.is_alphanumeric() || matches!(c, '-' | '_' | '.' | ':');
        let rest = &text[at..];
        let (closing, rest) = match rest.strip_prefix("</") {
            Some(rest) => (true, rest),
            None => (false, rest.strip_prefix('<')?),
        };
        let name_len = rest.find(|c: char| !name_char(c)).unwrap_or(rest.len());
        let name = &rest[..name_len];
        // A self-closing tag has no partner.
        if !closing {
            let tail = &rest[name_len..];
            let end = tail.find('>')?;
            if tail[..end].trim_end().ends_with('/') {
                return None;
            }
        }
        (!name.is_empty()).then_some((closing, name))
    }
    let partner = if closing {
        let mut depth = 0;
        let mut found = None;
        for (at, _) in old[..start - 2]
            .match_indices('<')
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
        {
            match tag_at(old, at) {
                Some((true, name)) if name == old_name => depth += 1,
                Some((false, name)) if name == old_name => {
                    if depth == 0 {
                        found = Some(at + 1);
                        break;
                    }
                    depth -= 1;
                }
                _ => {}
            }
        }
        found?
    } else {
        let mut depth = 0;
        let mut found = None;
        for (at, _) in old[end..].match_indices('<').collect::<Vec<_>>() {
            let at = end + at;
            match tag_at(old, at) {
                Some((false, name)) if name == old_name => depth += 1,
                Some((true, name)) if name == old_name => {
                    if depth == 0 {
                        found = Some(at + 2);
                        break;
                    }
                    depth -= 1;
                }
                _ => {}
            }
        }
        found?
    };
    Some((partner..partner + old_name.len(), new_name.to_owned()))
}

/// The other places a word appears, as whole words, for highlighting them
/// when no language server says better; at most `most`.
pub fn occurrences(text: &str, word: &str, most: usize) -> Vec<Range<usize>> {
    if word.chars().count() < 2 {
        return Vec::new();
    }
    let boundary = |c: Option<char>| c.is_none_or(|c| !(c.is_alphanumeric() || c == '_'));
    text.match_indices(word)
        .filter(|(at, _)| {
            boundary(text[..*at].chars().next_back())
                && boundary(text[at + word.len()..].chars().next())
        })
        .map(|(at, _)| at..at + word.len())
        .take(most)
        .collect()
}

/// How a file indents, read from its lines as VS Code's
/// `editor.detectIndentation` does: tabs, or spaces and how many for each
/// level. `None` when too few lines are indented to tell.
pub fn detect_indentation(text: &str) -> Option<(bool, usize)> {
    let mut tabs = 0usize;
    let mut spaced = 0usize;
    // How often each step between the indents of lines that follow.
    let mut steps = [0usize; 9];
    let mut previous = 0usize;
    for line in text.lines().take(10_000) {
        if line.trim().is_empty() {
            continue;
        }
        let leading = &line[..line.len() - line.trim_start().len()];
        if leading.starts_with('\t') {
            tabs += 1;
            continue;
        }
        let spaces = leading.chars().take_while(|&c| c == ' ').count();
        if spaces > 0 {
            spaced += 1;
        }
        let step = spaces.abs_diff(previous);
        if (2..=8).contains(&step) {
            steps[step] += 1;
        }
        previous = spaces;
    }
    if tabs + spaced < 2 {
        return None;
    }
    if tabs > spaced {
        return Some((true, 0));
    }
    // A step of 4 is also two of 2: the smaller one wins when it is common.
    let best = (2..=8).max_by_key(|&step| (steps[step], std::cmp::Reverse(step)))?;
    (steps[best] > 0).then_some((false, best))
}

/// Line endings, as the status bar shows them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LineEnding {
    Lf,
    CrLf,
}

impl LineEnding {
    pub fn name(self) -> &'static str {
        match self {
            LineEnding::Lf => "LF",
            LineEnding::CrLf => "CRLF",
        }
    }

    /// The ending most lines of `text` have; LF for a single line.
    pub fn of(text: &str) -> Self {
        let crlf = text.matches("\r\n").count();
        let lf = text.matches('\n').count() - crlf;
        if crlf > lf {
            LineEnding::CrLf
        } else {
            LineEnding::Lf
        }
    }

    /// `text` with every line ending this one.
    pub fn apply(self, text: &str) -> String {
        let plain = text.replace("\r\n", "\n");
        match self {
            LineEnding::Lf => plain,
            LineEnding::CrLf => plain.replace('\n', "\r\n"),
        }
    }
}

/// `text` with its indentation in tabs or in spaces, `size` columns a
/// level, as «Convert Indentation to Tabs/Spaces».
pub fn convert_indentation(text: &str, to_tabs: bool, size: usize) -> String {
    let size = size.max(1);
    let mut out = String::with_capacity(text.len());
    for line in text.split_inclusive('\n') {
        let rest = line.trim_start_matches([' ', '\t']);
        let leading = &line[..line.len() - rest.len()];
        let columns = leading.chars().fold(0, |columns, c| match c {
            '\t' => columns / size * size + size,
            _ => columns + 1,
        });
        if to_tabs {
            out.push_str(&"\t".repeat(columns / size));
            out.push_str(&" ".repeat(columns % size));
        } else {
            out.push_str(&" ".repeat(columns));
        }
        out.push_str(rest);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn apply(text: &str, edit: LineEdit) -> String {
        let mut text = text.to_owned();
        text.replace_range(edit.range, &edit.text);
        text
    }

    #[test]
    fn lines_sort_unique_and_join() {
        let text = "pera\nmela\nPera\nmela\n";
        assert_eq!(
            apply(text, sort_lines(text, 0..0, false).unwrap()),
            "mela\nmela\nPera\npera\n"
        );
        assert_eq!(
            apply(text, unique_lines(text, 0..0).unwrap()),
            "pera\nmela\nPera\n"
        );
        let code = "let a = [\n    1,\n    2\n];";
        assert_eq!(
            apply(code, join_lines(code, 0..code.len()).unwrap()),
            "let a = [ 1, 2 ];"
        );
        assert_eq!(
            apply("uno\n  due\n", join_lines("uno\n  due\n", 0..0).unwrap()),
            "uno due\n"
        );
    }

    #[test]
    fn case_changes_at_the_word_or_the_selection() {
        let text = "let parseHTTPRequest = 1;";
        let at = text.find("HTTP").unwrap();
        let snake = transform_case(text, at..at, Case::Snake).unwrap();
        assert_eq!(snake.text, "parse_http_request");
        assert_eq!(
            transform_case("my-big-name", 0..11, Case::Camel)
                .unwrap()
                .text,
            "myBigName"
        );
        assert_eq!(
            transform_case("hello world", 0..11, Case::Title)
                .unwrap()
                .text,
            "Hello World"
        );
        assert_eq!(
            transform_case("Get_User", 0..8, Case::Kebab).unwrap().text,
            "get-user"
        );
        assert_eq!(
            transform_case("abc", 0..3, Case::Upper).unwrap().text,
            "ABC"
        );
    }

    #[test]
    fn brackets_match_and_count_outside_strings() {
        let text = "f(a, \"(\") { [1] } // )";
        let depths = bracket_depths(text, "typescript");
        let shown: Vec<(char, usize)> = depths
            .iter()
            .map(|(at, depth)| (text[*at..].chars().next().unwrap(), *depth))
            .collect();
        assert_eq!(
            shown,
            [('(', 0), (')', 0), ('{', 0), ('[', 1), (']', 1), ('}', 0)]
        );
        let open = text.find('{').unwrap();
        assert_eq!(
            matching_bracket(text, open, "typescript"),
            Some(text.find('}').unwrap())
        );
        assert_eq!(
            matching_bracket(text, text.find('}').unwrap() + 1, "typescript"),
            Some(open)
        );
    }

    #[test]
    fn the_selection_grows_by_word_quotes_brackets_and_line() {
        let text = "call(\"hello world\", other)";
        let at = text.find("world").unwrap() + 1;
        let word = expand_selection(text, at..at);
        assert_eq!(&text[word.clone()], "world");
        let inside = expand_selection(text, word);
        assert_eq!(&text[inside.clone()], "hello world");
        let quoted = expand_selection(text, inside);
        assert_eq!(&text[quoted.clone()], "\"hello world\"");
        let args = expand_selection(text, quoted);
        assert_eq!(&text[args.clone()], "\"hello world\", other");
    }

    #[test]
    fn a_selection_is_surrounded() {
        let edit = surround("say hi", 4..6, '(').unwrap();
        assert_eq!(apply("say hi", edit), "say (hi)");
        assert!(surround("say hi", 4..4, '(').is_none());
    }

    #[test]
    fn renaming_a_tag_renames_its_partner() {
        let old = "<div><div>a</div><span/></div>";
        // The outer opening tag becomes «section».
        let new = old.replacen("<div>", "<section>", 1);
        let (range, name) = paired_tag_edit(old, &new).unwrap();
        assert_eq!(name, "section");
        assert_eq!(range.start, old.rfind("div>").unwrap());
        // A closing tag renames its opening one.
        let old = "<p>x</p>";
        let new = "<p>x</pre>";
        let (range, name) = paired_tag_edit(old, new).unwrap();
        assert_eq!((range, name.as_str()), (1..2, "pre"));
        // Text that is not a tag name is left alone.
        assert!(paired_tag_edit("<p>abc</p>", "<p>abXc</p>").is_none());
    }

    #[test]
    fn words_are_found_whole() {
        let text = "total = total + subtotal";
        assert_eq!(occurrences(text, "total", 10), [0..5, 8..13]);
    }

    #[test]
    fn indentation_is_read_from_the_lines() {
        let two = "fn a() {\n  if x {\n    y();\n  }\n}\n";
        assert_eq!(detect_indentation(two), Some((false, 2)));
        let four = "def a():\n    if x:\n        y()\n    return 1\n";
        assert_eq!(detect_indentation(four), Some((false, 4)));
        let tabs = "func a() {\n\tif x {\n\t\ty()\n\t}\n}\n";
        assert_eq!(detect_indentation(tabs), Some((true, 0)));
        assert_eq!(detect_indentation("one line\n"), None);
    }

    #[test]
    fn line_endings_and_indentation_convert() {
        assert_eq!(LineEnding::of("a\r\nb\r\nc\n"), LineEnding::CrLf);
        assert_eq!(LineEnding::of("a\nb"), LineEnding::Lf);
        assert_eq!(LineEnding::CrLf.apply("a\nb\r\n"), "a\r\nb\r\n");
        assert_eq!(LineEnding::Lf.apply("a\r\nb\r\n"), "a\nb\n");
        assert_eq!(
            convert_indentation("    a\n        b\n  c\n", true, 4),
            "\ta\n\t\tb\n  c\n"
        );
        assert_eq!(
            convert_indentation("\ta\n\t\tb\n", false, 2),
            "  a\n    b\n"
        );
    }
}
