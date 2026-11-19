//! What a differences tab shows, worked out from the two whole versions of
//! a file: the changed blocks, as git's histogram diff finds them; the
//! unchanged lines around them, folded beyond three, to open twenty at a
//! time or all at once; and inside a changed line, the words that changed.
//! The rows are the same for one file and for the page with every change.

use std::collections::HashMap;
use std::ops::Range;

/// Unchanged lines kept around each changed block.
pub(super) const CONTEXT: usize = 3;
/// How many hidden lines an arrow of a fold shows.
pub(super) const EXPAND_STEP: usize = 20;
/// The fewest lines taken away in one place and written in another that
/// count as moved, as VS Code's `diffEditor.experimental.showMoves`.
const MOVED_LINES: usize = 3;

/// One file's two versions and its changed blocks.
pub(super) struct FileDiff {
    /// The file, from the repository's top folder.
    pub(super) path: String,
    /// The two versions as they are, for staging or undoing a block.
    pub(super) old_text: String,
    pub(super) new_text: String,
    pub(super) old: Vec<String>,
    pub(super) new: Vec<String>,
    pub(super) hunks: Vec<git::Hunk>,
    /// For each unchanged stretch, by its first line in the new version:
    /// how many hidden lines were opened from its top and from its bottom.
    pub(super) revealed: HashMap<usize, (usize, usize)>,
    /// Closed to its header, in the page with every change.
    pub(super) collapsed: bool,
    /// Lines taken away that were written elsewhere: old line to new line,
    /// and the other way round.
    pub(super) moved_old: HashMap<usize, usize>,
    pub(super) moved_new: HashMap<usize, usize>,
}

fn lines(text: &str) -> Vec<String> {
    text.lines().map(str::to_owned).collect()
}

/// A line with its spaces squeezed, for comparing without them.
fn squeezed(line: &str) -> String {
    line.split_whitespace().collect::<Vec<_>>().join(" ")
}

impl FileDiff {
    pub(super) fn new(path: impl Into<String>, old: &str, new: &str, ignore_spaces: bool) -> Self {
        let (old_text, new_text) = (old.to_owned(), new.to_owned());
        let (old, new) = (lines(old), lines(new));
        let hunks = if ignore_spaces {
            let join = |lines: &[String]| {
                lines
                    .iter()
                    .map(|line| squeezed(line) + "\n")
                    .collect::<String>()
            };
            git::hunks(&join(&old), &join(&new))
        } else {
            let join = |lines: &[String]| {
                lines
                    .iter()
                    .map(|line| line.clone() + "\n")
                    .collect::<String>()
            };
            git::hunks(&join(&old), &join(&new))
        };
        let (moved_old, moved_new) = find_moves(&old, &new, &hunks);
        Self {
            path: path.into(),
            old_text,
            new_text,
            old,
            new,
            hunks,
            revealed: HashMap::new(),
            collapsed: false,
            moved_old,
            moved_new,
        }
    }

    /// The line a moved stretch went to, on the first of its old lines.
    pub(super) fn moved_to_at(&self, old: usize) -> Option<usize> {
        let to = *self.moved_old.get(&old)?;
        let continues = old > 0 && self.moved_old.get(&(old - 1)) == Some(&(to.wrapping_sub(1)));
        (!continues).then_some(to)
    }

    /// The line a moved stretch came from, on the first of its new lines.
    pub(super) fn moved_from_at(&self, new: usize) -> Option<usize> {
        let from = *self.moved_new.get(&new)?;
        let continues = new > 0 && self.moved_new.get(&(new - 1)) == Some(&(from.wrapping_sub(1)));
        (!continues).then_some(from)
    }

    pub(super) fn additions(&self) -> usize {
        self.hunks.iter().map(|hunk| hunk.new.len()).sum()
    }

    pub(super) fn deletions(&self) -> usize {
        self.hunks.iter().map(|hunk| hunk.old.len()).sum()
    }

    /// Opens hidden lines of the stretch starting at new line `at`.
    pub(super) fn expand(&mut self, at: usize, how: Expand) {
        let revealed = self.revealed.entry(at).or_default();
        match how {
            Expand::Down => revealed.0 += EXPAND_STEP,
            Expand::Up => revealed.1 += EXPAND_STEP,
            Expand::All => *revealed = (usize::MAX / 4, usize::MAX / 4),
        }
    }

    /// The line a scope of code begins on above new line `line`, as git
    /// names a hunk: the nearest line that starts at the margin with a
    /// word, such as `fn save(` or `export function Card(`.
    pub(super) fn scope_above(&self, line: usize) -> Option<String> {
        self.new[..line.min(self.new.len())]
            .iter()
            .rev()
            .find(|text| {
                text.chars()
                    .next()
                    .is_some_and(|first| first.is_alphabetic() || matches!(first, '_' | '$' | '@'))
            })
            .map(|text| {
                let text = text.trim_end();
                let mut short: String = text.chars().take(80).collect();
                if text.chars().count() > 80 {
                    short.push('…');
                }
                short
            })
    }
}

/// Stretches of at least `MOVED_LINES` lines taken away in one place and
/// written, the same but for their indentation, in another.
#[allow(clippy::type_complexity)]
fn find_moves(
    old: &[String],
    new: &[String],
    hunks: &[git::Hunk],
) -> (HashMap<usize, usize>, HashMap<usize, usize>) {
    let mut removed = vec![false; old.len()];
    let mut added = vec![false; new.len()];
    for hunk in hunks {
        for line in hunk.old.clone().filter(|line| *line < old.len()) {
            removed[line] = true;
        }
        for line in hunk.new.clone().filter(|line| *line < new.len()) {
            added[line] = true;
        }
    }
    let key = |line: &str| line.trim().to_owned();
    let mut written: HashMap<String, Vec<usize>> = HashMap::new();
    for (line, text) in new.iter().enumerate().filter(|(line, _)| added[*line]) {
        let text = key(text);
        if !text.is_empty() {
            written.entry(text).or_default().push(line);
        }
    }
    let (mut moved_old, mut moved_new) = (HashMap::new(), HashMap::new());
    let mut line = 0;
    while line < old.len() {
        if !removed[line] {
            line += 1;
            continue;
        }
        let mut best = (0, 0);
        for &start in written.get(&key(&old[line])).into_iter().flatten() {
            let mut length = 0;
            while line + length < old.len()
                && start + length < new.len()
                && removed[line + length]
                && added[start + length]
                && !moved_new.contains_key(&(start + length))
                && key(&old[line + length]) == key(&new[start + length])
            {
                length += 1;
            }
            if length > best.0 {
                best = (length, start);
            }
        }
        let (length, start) = best;
        // Braces and blank lines alone do not make a move.
        let meaningful = (0..length).any(|offset| {
            old[line + offset]
                .chars()
                .filter(|c| c.is_alphanumeric())
                .count()
                >= 4
        });
        if length >= MOVED_LINES && meaningful {
            for offset in 0..length {
                moved_old.insert(line + offset, start + offset);
                moved_new.insert(start + offset, line + offset);
            }
            line += length;
        } else {
            line += 1;
        }
    }
    (moved_old, moved_new)
}

/// Which hidden lines of a fold to show.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Expand {
    /// Twenty more under the lines shown above the fold.
    Down,
    /// Twenty more over the lines shown below it.
    Up,
    All,
}

/// A row of a differences tab.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Row {
    /// A file's name and counts, in the page with every change.
    Header { file: usize },
    /// A file with nothing different.
    Unchanged { file: usize },
    /// An unchanged line, on both sides.
    Same { file: usize, old: usize, new: usize },
    /// Side by side: a line of a changed block on either side or both.
    Pair {
        file: usize,
        hunk: usize,
        first: bool,
        old: Option<usize>,
        new: Option<usize>,
    },
    /// In one column: a line taken away, or one added.
    Removed {
        file: usize,
        hunk: usize,
        first: bool,
        old: usize,
    },
    Added {
        file: usize,
        hunk: usize,
        first: bool,
        new: usize,
    },
    /// Hidden unchanged lines: `len` of them from new line `new`, and
    /// whether there are lines shown above and below to grow from.
    Fold {
        file: usize,
        at: usize,
        old: usize,
        new: usize,
        len: usize,
        above: bool,
        below: bool,
    },
}

impl Row {
    pub(super) fn file(&self) -> usize {
        match self {
            Row::Header { file }
            | Row::Unchanged { file }
            | Row::Same { file, .. }
            | Row::Pair { file, .. }
            | Row::Removed { file, .. }
            | Row::Added { file, .. }
            | Row::Fold { file, .. } => *file,
        }
    }

    /// The changed block the row belongs to.
    pub(super) fn hunk(&self) -> Option<usize> {
        match self {
            Row::Pair { hunk, .. } | Row::Removed { hunk, .. } | Row::Added { hunk, .. } => {
                Some(*hunk)
            }
            _ => None,
        }
    }

    /// Whether the row starts a changed block, where its buttons go.
    pub(super) fn starts_hunk(&self) -> bool {
        matches!(
            self,
            Row::Pair { first: true, .. }
                | Row::Removed { first: true, .. }
                | Row::Added { first: true, .. }
        )
    }
}

/// The rows of `files`, side by side or in one column, with a header for
/// each file when there are `headers`.
pub(super) fn rows(files: &[FileDiff], split: bool, headers: bool) -> Vec<Row> {
    let mut rows = Vec::new();
    for (index, file) in files.iter().enumerate() {
        if headers {
            rows.push(Row::Header { file: index });
            if file.collapsed {
                continue;
            }
        }
        if file.hunks.is_empty() {
            rows.push(Row::Unchanged { file: index });
            continue;
        }
        let (mut old, mut new) = (0, 0);
        for (hunk_index, hunk) in file.hunks.iter().enumerate() {
            let gap = hunk.new.start - new;
            unchanged(
                &mut rows,
                file,
                index,
                old,
                new,
                gap,
                hunk_index == 0,
                false,
            );
            if split {
                let longest = hunk.old.len().max(hunk.new.len());
                for offset in 0..longest {
                    rows.push(Row::Pair {
                        file: index,
                        hunk: hunk_index,
                        first: offset == 0,
                        old: (offset < hunk.old.len()).then(|| hunk.old.start + offset),
                        new: (offset < hunk.new.len()).then(|| hunk.new.start + offset),
                    });
                }
            } else {
                for (offset, line) in hunk.old.clone().enumerate() {
                    rows.push(Row::Removed {
                        file: index,
                        hunk: hunk_index,
                        first: offset == 0,
                        old: line,
                    });
                }
                for (offset, line) in hunk.new.clone().enumerate() {
                    rows.push(Row::Added {
                        file: index,
                        hunk: hunk_index,
                        first: offset == 0 && hunk.old.is_empty(),
                        new: line,
                    });
                }
            }
            old = hunk.old.end;
            new = hunk.new.end;
        }
        let rest = file.new.len().saturating_sub(new);
        unchanged(&mut rows, file, index, old, new, rest, false, true);
    }
    rows
}

/// An unchanged stretch of `len` lines: its context around the changes,
/// what was opened, and a fold for the rest.
#[allow(clippy::too_many_arguments)]
fn unchanged(
    rows: &mut Vec<Row>,
    file: &FileDiff,
    index: usize,
    old: usize,
    new: usize,
    len: usize,
    first: bool,
    last: bool,
) {
    if len == 0 {
        return;
    }
    let (open_top, open_bottom) = file.revealed.get(&new).copied().unwrap_or_default();
    let top = if first { 0 } else { CONTEXT } + open_top;
    let bottom = if last { 0 } else { CONTEXT } + open_bottom;
    let same = |offset: usize| Row::Same {
        file: index,
        old: old + offset,
        new: new + offset,
    };
    // Hiding fewer than two lines saves nothing.
    if top + bottom + 1 >= len {
        rows.extend((0..len).map(same));
        return;
    }
    rows.extend((0..top).map(same));
    rows.push(Row::Fold {
        file: index,
        at: new,
        old: old + top,
        new: new + top,
        len: len - top - bottom,
        above: !first || open_top > 0,
        below: !last || open_bottom > 0,
    });
    rows.extend((len - bottom..len).map(same));
}

/// The pieces of a line, for comparing words: runs of letters and digits,
/// runs of spaces, and each other character alone.
fn tokens(line: &str) -> Vec<Range<usize>> {
    let mut tokens = Vec::new();
    let mut chars = line.char_indices().peekable();
    while let Some((start, c)) = chars.next() {
        let kind = |c: char| {
            if c.is_alphanumeric() || c == '_' {
                0
            } else if c.is_whitespace() {
                1
            } else {
                2
            }
        };
        let mut end = start + c.len_utf8();
        if kind(c) != 2 {
            while let Some(&(at, next)) = chars.peek() {
                if kind(next) != kind(c) {
                    break;
                }
                end = at + next.len_utf8();
                chars.next();
            }
        }
        tokens.push(start..end);
    }
    tokens
}

/// The longest pieces a line is compared word by word; longer ones are
/// marked whole.
const MAX_TOKENS: usize = 400;

/// What changed inside a pair of lines: the byte ranges of `old` taken
/// away and of `new` written, word by word. Ranges only spaces apart are
/// joined, so a changed phrase reads as one.
pub(super) fn changed_words(old: &str, new: &str) -> (Vec<Range<usize>>, Vec<Range<usize>>) {
    let (a, b) = (tokens(old), tokens(new));
    if a.len() > MAX_TOKENS || b.len() > MAX_TOKENS {
        return (
            std::iter::once(0..old.len()).collect(),
            std::iter::once(0..new.len()).collect(),
        );
    }
    let text = |line: &str, range: &Range<usize>| line[range.clone()].to_owned();
    let (a_text, b_text): (Vec<String>, Vec<String>) = (
        a.iter().map(|range| text(old, range)).collect(),
        b.iter().map(|range| text(new, range)).collect(),
    );
    // The longest common subsequence of the pieces.
    let mut table = vec![vec![0u16; b.len() + 1]; a.len() + 1];
    for i in (0..a.len()).rev() {
        for j in (0..b.len()).rev() {
            table[i][j] = if a_text[i] == b_text[j] {
                table[i + 1][j + 1] + 1
            } else {
                table[i + 1][j].max(table[i][j + 1])
            };
        }
    }
    let (mut kept_a, mut kept_b) = (vec![false; a.len()], vec![false; b.len()]);
    let (mut i, mut j) = (0, 0);
    while i < a.len() && j < b.len() {
        if a_text[i] == b_text[j] {
            kept_a[i] = true;
            kept_b[j] = true;
            i += 1;
            j += 1;
        } else if table[i + 1][j] >= table[i][j + 1] {
            i += 1;
        } else {
            j += 1;
        }
    }
    let changed = |line: &str, pieces: &[Range<usize>], kept: &[bool]| {
        let mut ranges: Vec<Range<usize>> = Vec::new();
        for (piece, kept) in pieces.iter().zip(kept) {
            if *kept {
                continue;
            }
            // Two changed pieces with only spaces between become one.
            match ranges.last_mut() {
                Some(last) if line[last.end..piece.start].trim().is_empty() => last.end = piece.end,
                _ => ranges.push(piece.clone()),
            }
        }
        // Spaces at the ends of a range are not part of the change.
        ranges
            .into_iter()
            .filter_map(|range| {
                let text = &line[range.clone()];
                let start = range.start + (text.len() - text.trim_start().len());
                let end = range.end - (text.len() - text.trim_end().len());
                (start < end).then_some(start..end)
            })
            .collect::<Vec<_>>()
    };
    (changed(old, &a, &kept_a), changed(new, &b, &kept_b))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn numbered(count: usize) -> String {
        (1..=count).map(|line| format!("line {line}\n")).collect()
    }

    #[test]
    fn unchanged_lines_fold_beyond_the_context() {
        let old = numbered(40);
        let new = old.replace("line 20\n", "line 20 changed\n");
        let mut file = FileDiff::new("a.txt", &old, &new, false);
        assert_eq!((file.additions(), file.deletions()), (1, 1));
        let shown = |file: &FileDiff| rows(std::slice::from_ref(file), true, false);
        let first = shown(&file);
        // 19 lines before: 16 hidden, then 3 of context; the change; 3 of
        // context, then the last 17 hidden.
        assert_eq!(
            first[0],
            Row::Fold {
                file: 0,
                at: 0,
                old: 0,
                new: 0,
                len: 16,
                above: false,
                below: true
            }
        );
        assert_eq!(
            first[1],
            Row::Same {
                file: 0,
                old: 16,
                new: 16
            }
        );
        assert_eq!(
            first[4],
            Row::Pair {
                file: 0,
                hunk: 0,
                first: true,
                old: Some(19),
                new: Some(19)
            }
        );
        assert!(matches!(
            first[8],
            Row::Fold {
                len: 17,
                at: 20,
                ..
            }
        ));
        assert_eq!(first.len(), 9);

        // Twenty more from the top of the last fold, then everything.
        file.expand(20, Expand::Down);
        let more = shown(&file);
        assert!(
            more.iter()
                .all(|row| !matches!(row, Row::Fold { at: 20, .. }))
        );
        file.expand(0, Expand::All);
        assert_eq!(shown(&file).len(), 40);
    }

    #[test]
    fn one_column_puts_what_went_before_what_came() {
        let file = FileDiff::new("a.txt", "a\nb\nc\n", "a\nB\nnew\nc\n", false);
        let rows = rows(std::slice::from_ref(&file), false, true);
        assert_eq!(
            rows,
            [
                Row::Header { file: 0 },
                Row::Same {
                    file: 0,
                    old: 0,
                    new: 0
                },
                Row::Removed {
                    file: 0,
                    hunk: 0,
                    first: true,
                    old: 1
                },
                Row::Added {
                    file: 0,
                    hunk: 0,
                    first: false,
                    new: 1
                },
                Row::Added {
                    file: 0,
                    hunk: 0,
                    first: false,
                    new: 2
                },
                Row::Same {
                    file: 0,
                    old: 2,
                    new: 3
                },
            ]
        );
    }

    #[test]
    fn spaces_can_be_ignored() {
        let old = "fn a() {\n    one();\n}\n";
        let new = "fn a() {\n        one();\n}\n";
        assert_eq!(FileDiff::new("a.rs", old, new, false).hunks.len(), 1);
        let quiet = FileDiff::new("a.rs", old, new, true);
        assert!(quiet.hunks.is_empty());
        assert_eq!(
            rows(std::slice::from_ref(&quiet), true, false),
            [Row::Unchanged { file: 0 }]
        );
    }

    #[test]
    fn changed_words_are_found_inside_a_line() {
        let old = "let total = price * 2;";
        let new = "let total = price * quantity + tax;";
        let (taken, written) = changed_words(old, new);
        assert_eq!(&old[taken[0].clone()], "2");
        assert_eq!(written.len(), 1);
        assert_eq!(&new[written[0].clone()], "quantity + tax");
        // Nothing in common: the whole line.
        let (taken, written) = changed_words("abc", "xyz");
        assert_eq!((taken.len(), written.len()), (1, 1));
        assert_eq!((taken[0].clone(), written[0].clone()), (0..3, 0..3));
    }

    #[test]
    fn a_fold_names_the_code_it_is_in() {
        let new = "use x;\n\nfn save(file: &str) {\n    let a = 1;\n    let b = 2;\n}\n";
        let file = FileDiff::new("a.rs", new, new, false);
        assert_eq!(
            file.scope_above(4).as_deref(),
            Some("fn save(file: &str) {")
        );
        assert_eq!(file.scope_above(0), None);
    }

    #[test]
    fn code_moved_elsewhere_is_seen_as_moved() {
        let helper = "fn helper() {\n    let total = 1;\n    total\n}\n";
        let body = "fn one() {}\nfn two() {}\nfn three() {}\nfn four() {}\nfn five() {}\n";
        let old = format!("{helper}{body}");
        let new = format!("{body}{helper}");
        let diff = FileDiff::new("x.rs", &old, &new, false);
        // The helper went from the top to the bottom.
        assert_eq!(diff.moved_old.get(&0), Some(&5));
        assert_eq!(diff.moved_to_at(0), Some(5));
        assert_eq!(diff.moved_to_at(1), None);
        assert_eq!(diff.moved_from_at(5), Some(0));
        // A real change is not a move.
        let changed = FileDiff::new("x.rs", "a\nb\nc\n", "a\nx\nc\n", false);
        assert!(changed.moved_old.is_empty());
    }
}
