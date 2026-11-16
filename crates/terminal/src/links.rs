//! Links in a terminal's text: web addresses, and paths such as
//! `src/main.rs:12:5` that open in the editor.

use std::sync::LazyLock;

use regex::Regex;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LinkTarget {
    Url(String),
    /// A path as written, which may be relative to the terminal's folder,
    /// with the line and column when given.
    File {
        path: String,
        line: Option<u32>,
        column: Option<u32>,
    },
}

/// A link in a line of text. `start` and `end` count characters, so they
/// are the terminal's columns when each cell holds one character.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Link {
    pub start: usize,
    pub end: usize,
    pub target: LinkTarget,
}

static URL: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"\b(?:https?|file)://[^\s<>"'`]+"#).expect("the address pattern is valid")
});

/// A path with a file name that has an extension, then `:line:column`,
/// `:line` or `(line,column)` as compilers and linters print them.
static FILE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?x)
        (?P<path>
            (?:~|\.{1,2})?/?
            (?:[\w.\-@+\[\]]+/)*
            [\w\-@+\[\]][\w.\-@+\[\]]*\.[A-Za-z][\w]*
        )
        (?:
            :(?P<line>\d+)(?::(?P<column>\d+))?
          | \((?P<pline>\d+),\s?(?P<pcolumn>\d+)\)
        )?",
    )
    .expect("the path pattern is valid")
});

/// The links in a line, in order. Paths are only candidates: the caller
/// checks that the file exists before showing them as links.
pub fn find_links(text: &str) -> Vec<Link> {
    let char_index = |byte: usize| text[..byte].chars().count();
    let mut links = Vec::new();
    let mut taken = Vec::new();
    for found in URL.find_iter(text) {
        let url = found
            .as_str()
            .trim_end_matches(['.', ',', ';', ':', '!', '?', ')', ']', '}', '\'']);
        let end = found.start() + url.len();
        taken.push(found.start()..end);
        links.push(Link {
            start: char_index(found.start()),
            end: char_index(end),
            target: LinkTarget::Url(url.to_owned()),
        });
    }
    for captures in FILE.captures_iter(text) {
        let whole = captures.get(0).expect("a match has a whole");
        if taken
            .iter()
            .any(|range| range.start < whole.end() && whole.start() < range.end)
        {
            continue;
        }
        let number = |name: &str| {
            captures
                .name(name)
                .and_then(|value| value.as_str().parse().ok())
        };
        links.push(Link {
            start: char_index(whole.start()),
            end: char_index(whole.end()),
            target: LinkTarget::File {
                path: captures["path"].to_owned(),
                line: number("line").or_else(|| number("pline")),
                column: number("column").or_else(|| number("pcolumn")),
            },
        });
    }
    links.sort_by_key(|link| link.start);
    links
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(path: &str, line: Option<u32>, column: Option<u32>) -> LinkTarget {
        LinkTarget::File {
            path: path.to_owned(),
            line,
            column,
        }
    }

    #[test]
    fn addresses() {
        let links = find_links("  ➜  Local:   http://localhost:3000/ (ready).");
        assert_eq!(links.len(), 1);
        assert_eq!(
            links[0].target,
            LinkTarget::Url("http://localhost:3000/".into())
        );
        assert_eq!((links[0].start, links[0].end), (14, 36));
    }

    #[test]
    fn paths_with_lines() {
        let links = find_links("error at src/app/page.tsx:12:5 and ./lib/a.ts(3,9)");
        assert_eq!(
            links
                .iter()
                .map(|link| link.target.clone())
                .collect::<Vec<_>>(),
            [
                file("src/app/page.tsx", Some(12), Some(5)),
                file("./lib/a.ts", Some(3), Some(9))
            ]
        );
        assert_eq!(links[0].start, 9);
        assert_eq!(
            find_links("--> crates/keel/src/main.rs:40")[0].target,
            file("crates/keel/src/main.rs", Some(40), None)
        );
        assert_eq!(
            find_links("see /Users/me/[id]/page.tsx")[0].target,
            file("/Users/me/[id]/page.tsx", None, None)
        );
    }

    #[test]
    fn plain_words_are_not_links() {
        assert!(find_links("version 1.2.3 ok").is_empty());
        assert!(find_links("hello world").is_empty());
    }
}
