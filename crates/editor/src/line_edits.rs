//! Editing commands on whole lines, as in VS Code: comment, move, copy,
//! delete and insert lines, and closing JSX and HTML tags. Each command
//! turns the text and the selection into one edit and the new selection.
//! Offsets are byte offsets into the text.

use std::ops::Range;

/// One replacement and where the selection goes after it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LineEdit {
    pub range: Range<usize>,
    pub text: String,
    pub selection: Range<usize>,
}

/// How a language writes comments.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CommentSyntax<'a> {
    pub line: Option<&'a str>,
    pub block: Option<(&'a str, &'a str)>,
}

/// The comment syntax of a VS Code language identifier.
pub fn comment_syntax(language_id: &str) -> CommentSyntax<'static> {
    const C_STYLE: CommentSyntax<'static> = CommentSyntax {
        line: Some("//"),
        block: Some(("/*", "*/")),
    };
    const HASH: CommentSyntax<'static> = CommentSyntax {
        line: Some("#"),
        block: None,
    };
    match language_id {
        "typescript" | "typescriptreact" | "javascript" | "javascriptreact" | "rust" | "c"
        | "cpp" | "csharp" | "java" | "swift" | "go" | "kotlin" | "dart" | "scss" | "less"
        | "jsonc" | "php" | "scala" | "prisma" | "groovy" => C_STYLE,
        "python"
        | "shellscript"
        | "yaml"
        | "toml"
        | "ruby"
        | "perl"
        | "r"
        | "dockerfile"
        | "makefile"
        | "powershell"
        | "properties"
        | "elixir"
        | "gitignore"
        | "ignore"
        | "spring-boot-properties"
        | "spring-boot-properties-yaml" => HASH,
        "sql" | "lua" | "haskell" => CommentSyntax {
            line: Some("--"),
            block: None,
        },
        "css" => CommentSyntax {
            line: None,
            block: Some(("/*", "*/")),
        },
        "html" | "xml" | "markdown" | "vue" | "svelte" => CommentSyntax {
            line: None,
            block: Some(("<!--", "-->")),
        },
        _ => CommentSyntax {
            line: None,
            block: None,
        },
    }
}

/// The lines a selection covers, as a byte range from the start of the
/// first line to the end of the last one, without its newline. A selection
/// that ends at the very start of a line leaves that line out, as VS Code
/// does.
fn covered_lines(text: &str, selection: &Range<usize>) -> Range<usize> {
    let start = text[..selection.start].rfind('\n').map_or(0, |at| at + 1);
    let mut end = selection.end;
    if end > selection.start && end > 0 && text.as_bytes().get(end - 1) == Some(&b'\n') {
        end -= 1;
    }
    let end = text[end..].find('\n').map_or(text.len(), |at| end + at);
    start..end.max(start)
}

fn indentation(line: &str) -> &str {
    &line[..line.len() - line.trim_start().len()]
}

/// Moves `offset` across an edit that replaced `range` with `inserted`.
fn shift(offset: usize, range: &Range<usize>, inserted: usize) -> usize {
    if offset <= range.start {
        offset
    } else if offset >= range.end {
        offset + inserted - (range.end - range.start)
    } else {
        range.start + inserted
    }
}

/// Runs a line command at every cursor of `selections` (in text order):
/// cursors whose lines touch or neighbor act as one, as VS Code's line
/// commands do. Returns the span of `text` to replace, its new text and the
/// new selections, or `None` when nothing changes.
pub fn at_every_cursor(
    text: &str,
    selections: &[Range<usize>],
    command: impl Fn(&str, Range<usize>) -> Option<LineEdit>,
) -> Option<(Range<usize>, String, Vec<Range<usize>>)> {
    // Blocks of selections over the same or neighboring lines.
    let line_of = |offset: usize| text[..offset.min(text.len())].matches('\n').count();
    let mut blocks: Vec<(Range<usize>, usize, usize)> = Vec::new();
    for selection in selections {
        let lines = covered_lines(text, selection);
        let (first, last) = (line_of(lines.start), line_of(lines.end));
        match blocks.last_mut() {
            Some((range, _, block_last)) if first <= *block_last + 1 => {
                // To the end of this cursor's line, so the block covers it.
                let end = if selection.is_empty() {
                    text[selection.end..]
                        .find('\n')
                        .map_or(text.len(), |at| selection.end + at)
                } else {
                    selection.end
                };
                range.end = range.end.max(end);
                *block_last = (*block_last).max(last);
            }
            _ => blocks.push((selection.clone(), first, last)),
        }
    }
    // Bottom up, so each edit leaves the offsets above it alone.
    let mut result = text.to_owned();
    let mut edits: Vec<LineEdit> = Vec::new();
    for (range, _, _) in blocks.iter().rev() {
        if let Some(edit) = command(&result, range.clone()) {
            result.replace_range(edit.range.clone(), &edit.text);
            edits.push(edit);
        }
    }
    if edits.is_empty() {
        return None;
    }
    edits.reverse();
    let start = edits.iter().map(|edit| edit.range.start).min()?;
    let end = edits.iter().map(|edit| edit.range.end).max()?;
    let grown: isize = edits
        .iter()
        .map(|edit| edit.text.len() as isize - (edit.range.end - edit.range.start) as isize)
        .sum();
    // An edit's selection moves with what the edits above it changed.
    let mut shift: isize = 0;
    let mut cursors = Vec::new();
    for edit in &edits {
        let moved = |offset: usize| (offset as isize + shift) as usize;
        cursors.push(moved(edit.selection.start)..moved(edit.selection.end));
        shift += edit.text.len() as isize - (edit.range.end - edit.range.start) as isize;
    }
    let new_end = (end as isize + grown) as usize;
    Some((start..end, result[start..new_end].to_owned(), cursors))
}

/// Comments or uncomments the selected lines with the language's line
/// comment, or wraps each line in a block comment when it has none.
pub fn toggle_line_comment(
    text: &str,
    selection: Range<usize>,
    syntax: CommentSyntax<'_>,
) -> Option<LineEdit> {
    let lines_range = covered_lines(text, &selection);
    let block = &text[lines_range.clone()];
    let lines: Vec<&str> = block.split('\n').collect();
    let mut output = Vec::with_capacity(lines.len());
    // Where each line's change happens, to move the selection along.
    let mut changes: Vec<(usize, isize)> = Vec::new();
    let mut line_start = lines_range.start;
    match syntax.line {
        Some(prefix) => {
            let filled: Vec<&&str> = lines
                .iter()
                .filter(|line| !line.trim().is_empty())
                .collect();
            let commented = !filled.is_empty()
                && filled
                    .iter()
                    .all(|line| line.trim_start().starts_with(prefix));
            let indent = filled
                .iter()
                .map(|line| indentation(line).len())
                .min()
                .unwrap_or(0);
            for line in &lines {
                if line.trim().is_empty() {
                    output.push((*line).to_owned());
                } else if commented {
                    let at = indentation(line).len();
                    let mut cut = prefix.len();
                    if line[at + cut..].starts_with(' ') {
                        cut += 1;
                    }
                    changes.push((line_start + at, -(cut as isize)));
                    output.push(format!("{}{}", &line[..at], &line[at + cut..]));
                } else {
                    changes.push((line_start + indent, prefix.len() as isize + 1));
                    output.push(format!("{}{prefix} {}", &line[..indent], &line[indent..]));
                }
                line_start += line.len() + 1;
            }
        }
        None => {
            let (open, close) = syntax.block?;
            for line in &lines {
                let trimmed = line.trim();
                let at = indentation(line).len();
                if trimmed.is_empty() {
                    output.push((*line).to_owned());
                } else if trimmed.starts_with(open) && trimmed.ends_with(close) {
                    let inner = trimmed[open.len()..trimmed.len() - close.len()].trim();
                    changes.push((
                        line_start + at,
                        inner.len() as isize - trimmed.len() as isize,
                    ));
                    output.push(format!("{}{inner}", &line[..at]));
                } else {
                    changes.push((line_start + at, open.len() as isize + 1));
                    output.push(format!("{}{open} {trimmed} {close}", &line[..at]));
                }
                line_start += line.len() + 1;
            }
        }
    }
    let replacement = output.join("\n");
    let moved = |offset: usize| {
        let mut moved = offset as isize;
        for (at, delta) in &changes {
            // A comment added right at the offset pushes it along.
            if offset > *at || (offset == *at && *delta > 0) {
                moved += delta;
            }
        }
        moved.max(lines_range.start as isize) as usize
    };
    Some(LineEdit {
        selection: moved(selection.start)..moved(selection.end),
        range: lines_range,
        text: replacement,
    })
}

/// Wraps the selection in a block comment, or unwraps it. With nothing
/// selected, inserts an empty comment with the cursor inside.
pub fn toggle_block_comment(
    text: &str,
    selection: Range<usize>,
    syntax: CommentSyntax<'_>,
) -> Option<LineEdit> {
    let (open, close) = syntax.block?;
    if selection.is_empty() {
        let inserted = format!("{open}  {close}");
        let cursor = selection.start + open.len() + 1;
        return Some(LineEdit {
            range: selection.clone(),
            text: inserted,
            selection: cursor..cursor,
        });
    }
    let selected = &text[selection.clone()];
    if let Some(inner) = selected
        .strip_prefix(open)
        .and_then(|rest| rest.strip_suffix(close))
    {
        let inner = inner.strip_prefix(' ').unwrap_or(inner);
        let inner = inner.strip_suffix(' ').unwrap_or(inner);
        return Some(LineEdit {
            range: selection.clone(),
            text: inner.to_owned(),
            selection: selection.start..selection.start + inner.len(),
        });
    }
    let wrapped = format!("{open} {selected} {close}");
    Some(LineEdit {
        range: selection.clone(),
        selection: selection.start..selection.start + wrapped.len(),
        text: wrapped,
    })
}

/// Swaps the selected lines with the line above (`up`) or below.
pub fn move_lines(text: &str, selection: Range<usize>, up: bool) -> Option<LineEdit> {
    let lines = covered_lines(text, &selection);
    if up {
        if lines.start == 0 {
            return None;
        }
        let above_start = text[..lines.start - 1].rfind('\n').map_or(0, |at| at + 1);
        let above = &text[above_start..lines.start - 1];
        let block = &text[lines.clone()];
        let shift_by = above.len() + 1;
        Some(LineEdit {
            range: above_start..lines.end,
            text: format!("{block}\n{above}"),
            selection: selection.start - shift_by..selection.end - shift_by,
        })
    } else {
        if lines.end >= text.len() {
            return None;
        }
        let below_end = text[lines.end + 1..]
            .find('\n')
            .map_or(text.len(), |at| lines.end + 1 + at);
        let below = &text[lines.end + 1..below_end];
        let block = &text[lines.clone()];
        let shift_by = below.len() + 1;
        Some(LineEdit {
            range: lines.start..below_end,
            text: format!("{below}\n{block}"),
            selection: selection.start + shift_by..selection.end + shift_by,
        })
    }
}

/// Copies the selected lines above (`up`) or below them; the selection
/// moves to the copy below, or stays on the copy above.
pub fn copy_lines(text: &str, selection: Range<usize>, up: bool) -> LineEdit {
    let lines = covered_lines(text, &selection);
    let block = &text[lines.clone()];
    let shift_by = if up { 0 } else { block.len() + 1 };
    LineEdit {
        range: lines.end..lines.end,
        text: format!("\n{block}"),
        selection: selection.start + shift_by..selection.end + shift_by,
    }
}

/// Deletes the selected lines; the cursor goes to the start of the line
/// that takes their place.
pub fn delete_lines(text: &str, selection: Range<usize>) -> LineEdit {
    let lines = covered_lines(text, &selection);
    let range = if lines.end < text.len() {
        lines.start..lines.end + 1
    } else if lines.start > 0 {
        lines.start - 1..lines.end
    } else {
        lines.clone()
    };
    let cursor = if lines.end < text.len() {
        lines.start
    } else {
        text[..range.start].rfind('\n').map_or(0, |at| at + 1)
    };
    LineEdit {
        range,
        text: String::new(),
        selection: cursor..cursor,
    }
}

/// Inserts an empty line after (`after`) or before the current one, with
/// the same indentation, and puts the cursor on it.
pub fn insert_line(text: &str, selection: Range<usize>, after: bool) -> LineEdit {
    let lines = covered_lines(text, &(selection.end..selection.end));
    let line = &text[lines.clone()];
    let indent = indentation(line).to_owned();
    if after {
        let cursor = lines.end + 1 + indent.len();
        LineEdit {
            range: lines.end..lines.end,
            text: format!("\n{indent}"),
            selection: cursor..cursor,
        }
    } else {
        let cursor = lines.start + indent.len();
        LineEdit {
            range: lines.start..lines.start,
            text: format!("{indent}\n"),
            selection: cursor..cursor,
        }
    }
}

/// HTML elements that never have a closing tag.
const VOID_ELEMENTS: &[&str] = &[
    "area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "source", "track",
    "wbr",
];

/// The closing tag to insert after typing `>` at the end of `before`, as
/// VS Code does in JSX and HTML: `<div className="a"` gives `</div>`, `<`
/// alone gives the fragment's `</>`.
pub fn closing_tag(before: &str, language_id: &str) -> Option<String> {
    let html = matches!(language_id, "html" | "xml" | "vue" | "svelte");
    let jsx = matches!(language_id, "typescriptreact" | "javascriptreact");
    if !html && !jsx {
        return None;
    }
    let open = before.rfind('<')?;
    let tag = &before[open + 1..];
    if tag.is_empty() {
        return jsx.then(|| "</>".to_owned());
    }
    let name_end = tag
        .find(|character: char| character.is_whitespace())
        .unwrap_or(tag.len());
    let name = &tag[..name_end];
    let valid_name = name
        .chars()
        .next()
        .is_some_and(|first| first.is_ascii_alphabetic())
        && name
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || "-_.:".contains(character));
    if !valid_name || tag.ends_with('/') {
        return None;
    }
    // The attributes must be complete: no open quote or brace, and no `>`
    // already closing the tag.
    let attributes = &tag[name_end..];
    let mut quote = None;
    let mut braces = 0i32;
    for character in attributes.chars() {
        match (quote, character) {
            (Some(open_quote), _) if character == open_quote => quote = None,
            (Some(_), _) => {}
            (None, '"' | '\'' | '`') => quote = Some(character),
            (None, '{') => braces += 1,
            (None, '}') => braces -= 1,
            (None, '>') if braces == 0 => return None,
            _ => {}
        }
    }
    if quote.is_some() || braces != 0 {
        return None;
    }
    // In TypeScript, `a < b` or `Array<string` are not tags.
    if jsx {
        let preceding = before[..open].trim_end();
        let word_start = preceding
            .rfind(|character: char| !(character.is_ascii_alphanumeric() || character == '_'))
            .map_or(0, |at| at + 1);
        let word = &preceding[word_start..];
        let keyword = matches!(
            word,
            "return" | "yield" | "await" | "default" | "case" | "in" | "of" | "else"
        );
        let expression_before = preceding.chars().last().is_some_and(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '_' | ')' | ']')
        });
        if expression_before && !keyword {
            return None;
        }
    }
    if html && VOID_ELEMENTS.contains(&name.to_ascii_lowercase().as_str()) {
        return None;
    }
    Some(format!("</{name}>"))
}

/// Moves a byte offset across an edit, for callers keeping other cursors.
pub fn map_through(offset: usize, edit: &LineEdit) -> usize {
    shift(offset, &edit.range, edit.text.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn apply(text: &str, edit: &LineEdit) -> String {
        let mut text = text.to_owned();
        text.replace_range(edit.range.clone(), &edit.text);
        text
    }

    /// Runs a command on text where `|` marks the cursor, or `[` and `]`
    /// the selection, and returns the result marked the same way.
    fn run(marked: &str, command: impl Fn(&str, Range<usize>) -> Option<LineEdit>) -> String {
        let (text, selection) = if let Some(at) = marked.find('|') {
            (marked.replacen('|', "", 1), at..at)
        } else {
            let start = marked.find('[').unwrap();
            let end = marked.find(']').unwrap() - 1;
            (marked.replacen('[', "", 1).replacen(']', "", 1), start..end)
        };
        let Some(edit) = command(&text, selection) else {
            return "unchanged".to_owned();
        };
        let mut result = apply(&text, &edit);
        if edit.selection.is_empty() {
            result.insert(edit.selection.start, '|');
        } else {
            result.insert(edit.selection.end, ']');
            result.insert(edit.selection.start, '[');
        }
        result
    }

    fn ts() -> CommentSyntax<'static> {
        comment_syntax("typescript")
    }

    #[test]
    fn line_commands_run_at_every_cursor() {
        let text = "uno\ndue\ntre\nquattro\n";
        let at = |needle: &str| {
            let start = text.find(needle).unwrap();
            start..start
        };
        // Two cursors far apart: both lines are commented.
        let (span, new, cursors) =
            at_every_cursor(text, &[at("uno"), at("tre")], |text, selection| {
                toggle_line_comment(text, selection, comment_syntax("python"))
            })
            .unwrap();
        let mut result = text.to_owned();
        result.replace_range(span, &new);
        assert_eq!(result, "# uno\ndue\n# tre\nquattro\n");
        assert_eq!(cursors.len(), 2);
        assert_eq!(&result[cursors[1].start..cursors[1].start + 3], "tre");

        // Deleting lines at two cursors.
        let (span, new, _) =
            at_every_cursor(text, &[at("due"), at("quattro")], |text, selection| {
                Some(delete_lines(text, selection))
            })
            .unwrap();
        let mut result = text.to_owned();
        result.replace_range(span, &new);
        assert_eq!(result, "uno\ntre\n");

        // Neighboring lines move together, as one block.
        let (span, new, cursors) =
            at_every_cursor(text, &[at("uno"), at("due")], |text, selection| {
                move_lines(text, selection, false)
            })
            .unwrap();
        let mut result = text.to_owned();
        result.replace_range(span, &new);
        assert_eq!(result, "tre\nuno\ndue\nquattro\n");
        assert_eq!(cursors.len(), 1);
    }

    #[test]
    fn line_comments_follow_the_least_indented_line() {
        let commented = run("  if (a) {\n    b|();\n  }", |text, selection| {
            toggle_line_comment(text, selection, ts())
        });
        assert_eq!(commented, "  if (a) {\n    // b|();\n  }");
        let block = run("[  a();\n    b();\n]c();", |text, selection| {
            toggle_line_comment(text, selection, ts())
        });
        assert_eq!(block, "[  // a();\n  //   b();\n]c();");
        let back = run("[  // a();\n  //   b();\n]c();", |text, selection| {
            toggle_line_comment(text, selection, ts())
        });
        assert_eq!(back, "[  a();\n    b();\n]c();");
    }

    #[test]
    fn python_uses_hashes_and_css_wraps_each_line() {
        let python = run("x = |1", |text, selection| {
            toggle_line_comment(text, selection, comment_syntax("python"))
        });
        assert_eq!(python, "# x = |1");
        let css = run("a { color: |red; }", |text, selection| {
            toggle_line_comment(text, selection, comment_syntax("css"))
        });
        assert_eq!(css, "/* a { color: |red; } */");
        let plain = run("text|", |text, selection| {
            toggle_line_comment(text, selection, comment_syntax("plaintext"))
        });
        assert_eq!(plain, "unchanged");
    }

    #[test]
    fn block_comments_wrap_and_unwrap_the_selection() {
        assert_eq!(
            run("a [b] c", |text, selection| toggle_block_comment(
                text,
                selection,
                ts()
            )),
            "a [/* b */] c"
        );
        assert_eq!(
            run("a [/* b */] c", |text, selection| toggle_block_comment(
                text,
                selection,
                ts()
            )),
            "a [b] c"
        );
        assert_eq!(
            run("a |c", |text, selection| toggle_block_comment(
                text,
                selection,
                ts()
            )),
            "a /* | */c"
        );
    }

    #[test]
    fn lines_move_up_and_down() {
        assert_eq!(
            run("one\ntw|o\nthree", |text, selection| move_lines(
                text, selection, true
            )),
            "tw|o\none\nthree"
        );
        assert_eq!(
            run("one\ntw|o\nthree", |text, selection| move_lines(
                text, selection, false
            )),
            "one\nthree\ntw|o"
        );
        assert_eq!(
            run("on|e\ntwo", |text, selection| move_lines(
                text, selection, true
            )),
            "unchanged"
        );
        assert_eq!(
            run("one\ntw|o", |text, selection| move_lines(
                text, selection, false
            )),
            "unchanged"
        );
    }

    #[test]
    fn lines_are_copied_deleted_and_inserted() {
        assert_eq!(
            run("a\nb|c\nd", |text, selection| Some(copy_lines(
                text, selection, false
            ))),
            "a\nbc\nb|c\nd"
        );
        assert_eq!(
            run("a\nb|c\nd", |text, selection| Some(copy_lines(
                text, selection, true
            ))),
            "a\nb|c\nbc\nd"
        );
        assert_eq!(
            run("a\nb|c\nd", |text, selection| Some(delete_lines(
                text, selection
            ))),
            "a\n|d"
        );
        assert_eq!(
            run("a\nb|c", |text, selection| Some(delete_lines(
                text, selection
            ))),
            "|a"
        );
        assert_eq!(
            run("  if (a) {|x", |text, selection| Some(insert_line(
                text, selection, true
            ))),
            "  if (a) {x\n  |"
        );
        assert_eq!(
            run("a\n    b|", |text, selection| Some(insert_line(
                text, selection, false
            ))),
            "a\n    |\n    b"
        );
    }

    #[test]
    fn tags_close_in_jsx_and_html_only_when_complete() {
        let tsx = "typescriptreact";
        assert_eq!(
            closing_tag("return <div className=\"card\"", tsx).as_deref(),
            Some("</div>")
        );
        assert_eq!(
            closing_tag("<Card onClick={() => go(1)}", tsx).as_deref(),
            Some("</Card>")
        );
        assert_eq!(closing_tag("return <", tsx).as_deref(), Some("</>"));
        assert_eq!(
            closing_tag("<Card.Header", tsx).as_deref(),
            Some("</Card.Header>")
        );
        assert_eq!(closing_tag("<img src=\"a\" /", tsx), None);
        assert_eq!(closing_tag("<div title=\"a > b", tsx), None);
        assert_eq!(closing_tag("<div onClick={() => {", tsx), None);
        assert_eq!(closing_tag("const list: Array<string", tsx), None);
        assert_eq!(closing_tag("if (a < b", tsx), None);
        assert_eq!(closing_tag("<div>text", tsx), None);
        assert_eq!(closing_tag("<br", "html"), None);
        assert_eq!(
            closing_tag("<section", "html").as_deref(),
            Some("</section>")
        );
        assert_eq!(closing_tag("<div", "typescript"), None);
    }
}
