//! Fuzzy matching, as in VS Code's Quick Open and command palette: the
//! letters of the query appear in order, not necessarily together, and
//! matches at word starts and in a row count more.

/// Points for each matched letter.
const MATCH: i32 = 16;
/// Extra points for a letter right after the previous match.
const IN_A_ROW: i32 = 14;
/// Extra points for a letter at the start of a word: after `/`, `_`, `-`,
/// `.`, a space, or an uppercase letter after a lowercase one.
const WORD_START: i32 = 12;
/// Points lost for each letter skipped between two matches.
const GAP: i32 = 1;
const NONE: i32 = i32::MIN / 2;

/// How well `query` matches `text`: higher is better, `None` when a letter
/// of the query is missing. Case and spaces in the query are ignored.
pub fn score(query: &str, text: &str) -> Option<i32> {
    let query: Vec<char> = query
        .chars()
        .filter(|character| !character.is_whitespace())
        .map(lower)
        .collect();
    if query.is_empty() {
        return Some(0);
    }
    let original: Vec<char> = text.chars().collect();
    let text: Vec<char> = original.iter().copied().map(lower).collect();
    if text.len() < query.len() {
        return None;
    }
    let bonus: Vec<i32> = (0..text.len())
        .map(|index| {
            let starts_word = match index.checked_sub(1).map(|before| original[before]) {
                None => true,
                Some(before) => {
                    matches!(before, '/' | '\\' | '_' | '-' | '.' | ' ')
                        || (before.is_lowercase() && original[index].is_uppercase())
                }
            };
            MATCH + if starts_word { WORD_START } else { 0 }
        })
        .collect();

    // best[j]: the best score with the current query letter on text[j].
    let mut previous = vec![NONE; text.len()];
    for (step, &wanted) in query.iter().enumerate() {
        let mut current = vec![NONE; text.len()];
        // The best earlier match at least two letters back, less the gap.
        let mut far = NONE;
        for j in 0..text.len() {
            if j >= 2 {
                far = far.max(previous[j - 2]) - GAP;
            }
            if text[j] != wanted {
                continue;
            }
            let from = if step == 0 {
                // Matches that start later cost a little.
                -(j as i32).min(20) / 2
            } else {
                let next_to = if j >= 1 {
                    previous[j - 1].saturating_add(IN_A_ROW)
                } else {
                    NONE
                };
                next_to.max(far)
            };
            if from > NONE / 2 {
                current[j] = from + bonus[j];
            }
        }
        previous = current;
    }
    previous
        .into_iter()
        .max()
        .filter(|best| *best > NONE / 2)
        .map(|best| best - (text.len() as i32) / 8)
}

fn lower(character: char) -> char {
    character.to_lowercase().next().unwrap_or(character)
}

/// The score of a file for a query: a match in the file name counts more
/// than one spread over the folders, and a query with `/` matches the path.
pub fn score_path(query: &str, relative_path: &str) -> Option<i32> {
    let name = relative_path.rsplit('/').next().unwrap_or(relative_path);
    let in_name = if query.contains('/') {
        None
    } else {
        score(query, name).map(|score| score + 40)
    };
    let in_path = score(query, relative_path);
    in_name.max(in_path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn best<'a>(query: &str, candidates: &[&'a str]) -> Vec<&'a str> {
        let mut scored: Vec<(i32, &str)> = candidates
            .iter()
            .filter_map(|candidate| score_path(query, candidate).map(|score| (score, *candidate)))
            .collect();
        scored.sort_by_key(|entry| std::cmp::Reverse(entry.0));
        scored.into_iter().map(|(_, candidate)| candidate).collect()
    }

    #[test]
    fn letters_must_appear_in_order() {
        assert!(score("bkc", "booking-card.tsx").is_some());
        assert!(score("BKC", "booking-card.tsx").is_some());
        assert!(score("cbk", "booking-card.tsx").is_none());
        assert!(score("", "anything").is_some());
        assert!(score("toolong", "short").is_none());
    }

    #[test]
    fn word_starts_and_letters_in_a_row_win() {
        let files = [
            "src/components/booking-card.tsx",
            "src/lib/bank-checks.ts",
            "src/components/booking-panel.tsx",
            "node_modules/bkc/index.js",
        ];
        assert_eq!(best("bocard", &files)[0], "src/components/booking-card.tsx");
        assert_eq!(
            best("bpanel", &files)[0],
            "src/components/booking-panel.tsx"
        );
        // A query with a folder matches the path.
        assert_eq!(best("lib/bank", &files), ["src/lib/bank-checks.ts"]);
    }

    #[test]
    fn camel_case_counts_as_word_starts() {
        let gtd = score("gtd", "GoToDefinition").unwrap();
        let loose = score("gtd", "getting-started").unwrap();
        assert!(gtd > loose, "{gtd} <= {loose}");
    }

    #[test]
    fn commands_match_like_in_vs_code() {
        let commands = [
            "File: Save",
            "View: Toggle Sidebar",
            "Preferences: Color Theme",
        ];
        let mut scored: Vec<(i32, &str)> = commands
            .iter()
            .filter_map(|command| score("togsid", command).map(|score| (score, *command)))
            .collect();
        scored.sort_by_key(|entry| std::cmp::Reverse(entry.0));
        assert_eq!(scored[0].1, "View: Toggle Sidebar");
    }
}
