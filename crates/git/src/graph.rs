//! The lanes of the history graph, as Git Graph draws them: each commit in
//! a column, and lines down to its parents, colored by lane.
//!
//! The commits come newest first. Going down the list, each lane waits
//! for one commit: the next parent of the line it carries. A commit takes
//! the first lane waiting for it (the others merge into it), its first
//! parent continues in its lane, and its other parents take a lane that
//! already waits for them or a new one. Finished lanes leave, and the ones
//! to their right move over.

use crate::history::Commit;

/// Where a line starts or ends in a row: the top edge, the commit's dot,
/// or the bottom edge.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Point {
    Top(usize),
    Dot,
    Bottom(usize),
}

/// A line in one row of the graph.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Line {
    pub from: Point,
    pub to: Point,
    /// An index into the graph's palette.
    pub color: usize,
}

/// How one commit is drawn.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GraphRow {
    /// The commit's column.
    pub column: usize,
    pub color: usize,
    pub lines: Vec<Line>,
    /// How many columns the row uses.
    pub width: usize,
}

/// A lane: the commit it waits for, and its color.
#[derive(Clone, Debug)]
struct Lane {
    waiting_for: String,
    color: usize,
}

/// Lays out the graph of `commits`, given newest first.
pub fn layout(commits: &[Commit], colors: usize) -> Vec<GraphRow> {
    let colors = colors.max(1);
    let mut lanes: Vec<Lane> = Vec::new();
    let mut next_color = 0;
    let mut new_color = || {
        let color = next_color % colors;
        next_color += 1;
        color
    };
    let mut rows = Vec::with_capacity(commits.len());
    for commit in commits {
        // The lanes waiting for this commit; the first one is its own.
        let waiting: Vec<usize> = lanes
            .iter()
            .enumerate()
            .filter(|(_, lane)| lane.waiting_for == commit.hash)
            .map(|(index, _)| index)
            .collect();
        let (column, color) = match waiting.first() {
            Some(&index) => (index, lanes[index].color),
            None => {
                // A branch tip nobody waits for: a new lane at the right.
                let color = new_color();
                lanes.push(Lane {
                    waiting_for: commit.hash.clone(),
                    color,
                });
                (lanes.len() - 1, color)
            }
        };
        let before = lanes.clone();

        // After this commit: its lane waits for its first parent, the
        // lanes that merged into it end, and other parents find a lane.
        let mut after: Vec<Option<Lane>> = lanes.iter().cloned().map(Some).collect();
        for &index in waiting.iter().skip(1) {
            after[index] = None;
        }
        let mut parent_lanes: Vec<(usize, usize)> = Vec::new();
        match commit.parents.first() {
            Some(first) => {
                after[column] = Some(Lane {
                    waiting_for: first.clone(),
                    color,
                });
                parent_lanes.push((column, color));
            }
            None => after[column] = None,
        }
        for parent in commit.parents.iter().skip(1) {
            let existing = after.iter().position(|lane| {
                lane.as_ref()
                    .is_some_and(|lane| lane.waiting_for == *parent)
            });
            match existing {
                Some(index) => {
                    parent_lanes.push((index, after[index].as_ref().map_or(0, |lane| lane.color)))
                }
                None => {
                    let color = new_color();
                    after.push(Some(Lane {
                        waiting_for: parent.clone(),
                        color,
                    }));
                    parent_lanes.push((after.len() - 1, color));
                }
            }
        }
        // Lanes that end close the gap: map each old place to its new one.
        let mut moved = vec![None; after.len()];
        let mut kept = 0;
        for (index, lane) in after.iter().enumerate() {
            if lane.is_some() {
                moved[index] = Some(kept);
                kept += 1;
            }
        }

        let mut lines = Vec::new();
        for (index, lane) in before.iter().enumerate() {
            if lane.waiting_for == commit.hash {
                // Coming down into the commit, from its own lane or a merge.
                lines.push(Line {
                    from: Point::Top(index),
                    to: Point::Dot,
                    color: lane.color,
                });
            } else if let Some(Some(to)) = moved.get(index) {
                // Passing by.
                lines.push(Line {
                    from: Point::Top(index),
                    to: Point::Bottom(*to),
                    color: lane.color,
                });
            }
        }
        // A commit nobody waited for starts at its dot.
        if waiting.is_empty() {
            lines.retain(|line| line.from != Point::Top(column));
        }
        for (index, parent_color) in parent_lanes {
            if let Some(Some(to)) = moved.get(index) {
                // A merge parent draws in its own lane's color; the first
                // parent continues the commit's.
                lines.push(Line {
                    from: Point::Dot,
                    to: Point::Bottom(*to),
                    color: if index == column { color } else { parent_color },
                });
            }
        }
        lanes = after.into_iter().flatten().collect();
        let width = before.len().max(lanes.len()).max(column + 1);
        rows.push(GraphRow {
            column,
            color,
            lines,
            width,
        });
    }
    rows
}

#[cfg(test)]
mod tests {
    use super::*;

    fn commit(hash: &str, parents: &[&str]) -> Commit {
        Commit {
            hash: hash.to_owned(),
            parents: parents.iter().map(|parent| (*parent).to_owned()).collect(),
            author: String::new(),
            email: String::new(),
            time: 0,
            refs: Vec::new(),
            subject: String::new(),
        }
    }

    #[test]
    fn a_straight_line() {
        let rows = layout(
            &[commit("c", &["b"]), commit("b", &["a"]), commit("a", &[])],
            8,
        );
        assert!(rows.iter().all(|row| row.column == 0 && row.width == 1));
        // The first starts at its dot, the last ends at its dot.
        assert_eq!(
            rows[0].lines,
            [Line {
                from: Point::Dot,
                to: Point::Bottom(0),
                color: 0
            }]
        );
        assert_eq!(
            rows[2].lines,
            [Line {
                from: Point::Top(0),
                to: Point::Dot,
                color: 0
            }]
        );
    }

    #[test]
    fn a_branch_and_its_merge() {
        // m merges f into d; f and d both come from a.
        let commits = [
            commit("m", &["d", "f"]),
            commit("f", &["a"]),
            commit("d", &["a"]),
            commit("a", &[]),
        ];
        let rows = layout(&commits, 8);
        let columns: Vec<usize> = rows.iter().map(|row| row.column).collect();
        assert_eq!(columns, [0, 1, 0, 0]);
        // The merge opens a second lane, in a new color.
        assert!(rows[0].lines.contains(&Line {
            from: Point::Dot,
            to: Point::Bottom(1),
            color: 1
        }));
        assert_eq!(rows[1].color, 1);
        // At a, the branch's lane comes into the dot and ends.
        assert!(rows[3].lines.contains(&Line {
            from: Point::Top(1),
            to: Point::Dot,
            color: 1
        }));
        assert_eq!(rows[3].width, 2);
    }

    #[test]
    fn lanes_close_their_gap() {
        // Two tips, x on lane 0 ends early, y keeps going and moves left.
        let commits = [
            commit("x", &["a"]),
            commit("y", &["b"]),
            commit("a", &[]),
            commit("b", &[]),
        ];
        let rows = layout(&commits, 8);
        assert_eq!(rows[1].column, 1);
        // After a ends, b's lane slides from column 1 to 0.
        assert!(rows[2].lines.contains(&Line {
            from: Point::Top(1),
            to: Point::Bottom(0),
            color: 1
        }));
        assert_eq!(rows[3].column, 0);
    }
}
