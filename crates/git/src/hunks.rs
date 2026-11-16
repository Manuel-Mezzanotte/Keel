//! The blocks of lines that changed between two versions of a text, for
//! the editor's margin and for staging or undoing one block at a time.

use std::ops::Range;

use imara_diff::{Algorithm, Diff, InternedInput};

/// A changed block: lines `old` of the older text became lines `new` of
/// the newer one. Lines count from zero.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hunk {
    pub old: Range<usize>,
    pub new: Range<usize>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HunkKind {
    /// Lines only added: green in the margin.
    Added,
    /// Lines changed: blue.
    Modified,
    /// Lines only removed: a red mark where they were.
    Deleted,
}

impl Hunk {
    pub fn kind(&self) -> HunkKind {
        match (self.old.is_empty(), self.new.is_empty()) {
            (true, _) => HunkKind::Added,
            (_, true) => HunkKind::Deleted,
            _ => HunkKind::Modified,
        }
    }

    /// Whether a line of the newer text belongs to the block. A deletion
    /// owns the line where its lines were.
    pub fn contains_line(&self, line: usize) -> bool {
        if self.new.is_empty() {
            line == self.new.start || (self.new.start > 0 && line + 1 == self.new.start)
        } else {
            self.new.contains(&line)
        }
    }
}

/// The changed blocks between two texts, as git's histogram diff finds them.
pub fn hunks(old: &str, new: &str) -> Vec<Hunk> {
    let input = InternedInput::new(old, new);
    let mut diff = Diff::compute(Algorithm::Histogram, &input);
    diff.postprocess_lines(&input);
    diff.hunks()
        .map(|hunk| Hunk {
            old: hunk.before.start as usize..hunk.before.end as usize,
            new: hunk.after.start as usize..hunk.after.end as usize,
        })
        .collect()
}

/// The lines of a text with their endings, as the diff counts them.
fn lines(text: &str) -> Vec<&str> {
    text.split_inclusive('\n').collect()
}

/// The text of a block's lines in a version.
pub fn block_text(text: &str, lines_range: Range<usize>) -> String {
    lines(text)
        .get(lines_range)
        .map(|lines| lines.concat())
        .unwrap_or_default()
}

/// `old` with one block replaced by its lines in `new`: what staging that
/// block makes of the staged version.
pub fn apply_hunk(old: &str, new: &str, hunk: &Hunk) -> String {
    let old_lines = lines(old);
    let new_lines = lines(new);
    let mut result = String::with_capacity(old.len() + new.len() / 4);
    for line in &old_lines[..hunk.old.start.min(old_lines.len())] {
        result.push_str(line);
    }
    let added = &new_lines[hunk.new.start.min(new_lines.len())..hunk.new.end.min(new_lines.len())];
    // A block added after a last line without its newline needs one.
    if !added.is_empty() && !result.is_empty() && !result.ends_with('\n') {
        result.push('\n');
    }
    for line in added {
        result.push_str(line);
    }
    for line in &old_lines[hunk.old.end.min(old_lines.len())..] {
        result.push_str(line);
    }
    result
}

/// `new` with one block put back as it was in `old`: undoing that block.
pub fn revert_hunk(old: &str, new: &str, hunk: &Hunk) -> String {
    let reversed = Hunk {
        old: hunk.new.clone(),
        new: hunk.old.clone(),
    };
    apply_hunk(new, old, &reversed)
}

#[cfg(test)]
mod tests {
    use super::*;

    const OLD: &str = "uno\ndue\ntre\nquattro\ncinque\n";

    #[test]
    fn finds_added_modified_and_deleted_blocks() {
        let new = "zero\nuno\nDUE\ntre\ncinque\n";
        let found = hunks(OLD, new);
        assert_eq!(
            found,
            [
                Hunk {
                    old: 0..0,
                    new: 0..1
                },
                Hunk {
                    old: 1..2,
                    new: 2..3
                },
                Hunk {
                    old: 3..4,
                    new: 4..4
                },
            ]
        );
        assert_eq!(
            found.iter().map(Hunk::kind).collect::<Vec<_>>(),
            [HunkKind::Added, HunkKind::Modified, HunkKind::Deleted]
        );
        assert!(found[2].contains_line(3) && found[2].contains_line(4));
        assert_eq!(block_text(OLD, found[1].old.clone()), "due\n");
    }

    #[test]
    fn staging_and_undoing_one_block() {
        let new = "zero\nuno\nDUE\ntre\ncinque\n";
        let found = hunks(OLD, new);
        // Staging only the change of "due".
        assert_eq!(
            apply_hunk(OLD, new, &found[1]),
            "uno\nDUE\ntre\nquattro\ncinque\n"
        );
        // Undoing only the added first line.
        assert_eq!(revert_hunk(OLD, new, &found[0]), "uno\nDUE\ntre\ncinque\n");
        // Undoing the deletion brings "quattro" back.
        assert_eq!(
            revert_hunk(OLD, new, &found[2]),
            "zero\nuno\nDUE\ntre\nquattro\ncinque\n"
        );
    }

    #[test]
    fn a_last_line_without_newline() {
        let old = "a\nb";
        let new = "a\nb\nc\n";
        let found = hunks(old, new);
        let staged = found
            .iter()
            .fold(old.to_owned(), |text, hunk| apply_hunk(&text, new, hunk));
        assert_eq!(staged, new);
    }
}
