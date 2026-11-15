//! Which lines of a text are new compared with an older version, for the
//! green "added lines" bar in the editor gutter.

/// Indexes of the lines of `current` that are not in `original`, found with
/// a longest common subsequence after trimming the common start and end.
///
/// Above a size limit the middle part is not compared line by line and
/// counts as entirely new: the bar stays correct at the edges and cheap.
pub fn added_lines(original: &str, current: &str) -> Vec<usize> {
    let old: Vec<&str> = original.split('\n').collect();
    let new: Vec<&str> = current.split('\n').collect();

    let mut start = 0;
    while start < old.len() && start < new.len() && old[start] == new[start] {
        start += 1;
    }
    let mut old_end = old.len();
    let mut new_end = new.len();
    while old_end > start && new_end > start && old[old_end - 1] == new[new_end - 1] {
        old_end -= 1;
        new_end -= 1;
    }

    let old_middle = &old[start..old_end];
    let new_middle = &new[start..new_end];
    const LIMIT: usize = 4_000_000;
    if old_middle.is_empty() || old_middle.len() * new_middle.len() > LIMIT {
        return (start..new_end).collect();
    }

    // lengths[i][j]: longest common subsequence of old_middle[i..], new_middle[j..].
    let columns = new_middle.len() + 1;
    let mut lengths = vec![0u32; (old_middle.len() + 1) * columns];
    for i in (0..old_middle.len()).rev() {
        for j in (0..new_middle.len()).rev() {
            lengths[i * columns + j] = if old_middle[i] == new_middle[j] {
                lengths[(i + 1) * columns + j + 1] + 1
            } else {
                lengths[(i + 1) * columns + j].max(lengths[i * columns + j + 1])
            };
        }
    }

    let mut added = Vec::new();
    let (mut i, mut j) = (0, 0);
    while j < new_middle.len() {
        if i < old_middle.len() && old_middle[i] == new_middle[j] {
            i += 1;
            j += 1;
        } else if i < old_middle.len()
            && lengths[(i + 1) * columns + j] >= lengths[i * columns + j + 1]
        {
            i += 1;
        } else {
            added.push(start + j);
            j += 1;
        }
    }
    added
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nothing_changed() {
        assert!(added_lines("a\nb\n", "a\nb\n").is_empty());
    }

    #[test]
    fn lines_typed_into_an_empty_file() {
        assert_eq!(added_lines("", "const a = 1;\nconst b = 2;"), [0, 1]);
    }

    #[test]
    fn lines_inserted_in_the_middle() {
        let original = "one\ntwo\nthree";
        let current = "one\nnew\ntwo\nanother\nthree";
        assert_eq!(added_lines(original, current), [1, 3]);
    }

    #[test]
    fn a_changed_line_counts_as_added() {
        assert_eq!(added_lines("one\ntwo\nthree", "one\nTWO\nthree"), [1]);
    }

    #[test]
    fn removed_lines_add_nothing() {
        assert!(added_lines("one\ntwo\nthree", "one\nthree").is_empty());
    }
}
