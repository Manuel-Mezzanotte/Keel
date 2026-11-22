//! Lines edited away from their file, in the search's results or in a
//! differences tab, put back into it: only where the file still has the
//! lines they came from, and never over changes made meanwhile.

use super::*;

/// A text's lines, each with its own ending.
pub(super) fn lines_of(text: &str) -> Vec<(&str, &str)> {
    text.split_inclusive('\n')
        .map(|line| {
            if let Some(content) = line.strip_suffix("\r\n") {
                (content, "\r\n")
            } else if let Some(content) = line.strip_suffix('\n') {
                (content, "\n")
            } else {
                (line, "")
            }
        })
        .collect()
}

/// A piece to put into a file: where it starts, the lines that must be
/// there, and the lines that take their place.
pub(super) struct Put<'a> {
    pub(super) start: usize,
    pub(super) original: &'a [String],
    pub(super) new: &'a [String],
}

/// The file's text with the pieces in, and for each whether it went in:
/// not when the file has other lines there now. Pieces come in the file's
/// order. Lines not touched keep their own endings.
pub(super) fn with_pieces(text: &str, pieces: &[Put]) -> (String, Vec<bool>) {
    let file = lines_of(text);
    // What most files agree on: the first line's ending.
    let ending = file
        .iter()
        .map(|(_, ending)| *ending)
        .find(|ending| !ending.is_empty())
        .unwrap_or("\n");
    let mut lines: Vec<(String, &str)> = file
        .iter()
        .map(|(line, ending)| ((*line).to_owned(), *ending))
        .collect();
    let mut put = vec![false; pieces.len()];
    // Last first, so the lines of the pieces above stay where they are.
    for (index, piece) in pieces.iter().enumerate().rev() {
        let range = piece.start..piece.start + piece.original.len();
        let same = lines.get(range.clone()).is_some_and(|there| {
            there
                .iter()
                .map(|(line, _)| line.as_str())
                .eq(piece.original.iter().map(String::as_str))
        });
        if !same {
            continue;
        }
        // A last line without its newline stays without.
        let open_end =
            range.end == lines.len() && lines.last().is_some_and(|(_, end)| end.is_empty());
        let mut new: Vec<(String, &str)> = piece
            .new
            .iter()
            .map(|line| (line.clone(), ending))
            .collect();
        if open_end && let Some(last) = new.last_mut() {
            last.1 = "";
        }
        lines.splice(range, new);
        put[index] = true;
    }
    let mut out = String::with_capacity(text.len());
    for (line, ending) in lines {
        out.push_str(&line);
        out.push_str(ending);
    }
    (out, put)
}

impl Workbench {
    /// The text a piece is put into; `None` when the disk has something
    /// else than the file's tab, so nothing made outside Keel is lost.
    pub(super) fn text_to_change(&self, path: &Path) -> Option<String> {
        let uri = Uri::file(path).to_string();
        match self
            .documents
            .get(&uri)
            .map(|document| (document, &document.backing))
        {
            Some((document, Backing::File(file))) => file
                .changed_on_disk()
                .is_none()
                .then(|| document.synced_text.clone()),
            _ => String::from_utf8(std::fs::read(path).ok()?).ok(),
        }
    }

    /// Writes a file's new text. Open with changes of its own, the text
    /// goes to its tab without saving them; else it is saved.
    pub(super) fn write_pieces(
        &mut self,
        path: &Path,
        text: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let uri = Uri::file(path).to_string();
        match self.documents.get(&uri) {
            Some(document) if document.modified => {
                self.set_document_text(&uri, text, window, cx);
                true
            }
            Some(_) => {
                self.write_file_text(path, text, window, cx);
                true
            }
            None => match std::fs::write(path, text) {
                Ok(()) => true,
                Err(error) => {
                    self.report(format!("{}: {error}", path.display()));
                    false
                }
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn put(text: &str, start: usize, original: &[&str], new: &[&str]) -> (String, Vec<bool>) {
        let own = |lines: &[&str]| {
            lines
                .iter()
                .map(|line| (*line).to_owned())
                .collect::<Vec<_>>()
        };
        let (original, new) = (own(original), own(new));
        with_pieces(
            text,
            &[Put {
                start,
                original: &original,
                new: &new,
            }],
        )
    }

    #[test]
    fn pieces_go_where_the_file_still_has_their_lines() {
        assert_eq!(
            put("a\nb\nc\n", 1, &["b"], &["B", "B2"]),
            ("a\nB\nB2\nc\n".into(), vec![true])
        );
        // Emptied: the line goes. An empty piece: lines go in there.
        assert_eq!(put("a\nb\nc\n", 1, &["b"], &[]).0, "a\nc\n");
        assert_eq!(put("a\nc\n", 1, &[], &["b"]).0, "a\nb\nc\n");
        assert_eq!(put("", 0, &[], &["a"]).0, "a\n");
        // Other lines there now: nothing changes.
        assert_eq!(
            put("a\nX\nc\n", 1, &["b"], &["B"]),
            ("a\nX\nc\n".into(), vec![false])
        );
        assert_eq!(put("a\n", 5, &["b"], &["B"]).1, [false]);
        // Endings stay: CRLF, and a last line without its newline.
        assert_eq!(
            put("a\r\nb\r\nc\r\n", 1, &["b"], &["B", "B2"]).0,
            "a\r\nB\r\nB2\r\nc\r\n"
        );
        assert_eq!(put("a\nb", 1, &["b"], &["B", "C"]).0, "a\nB\nC");
        // A file of mixed endings keeps each untouched line's own.
        assert_eq!(put("a\r\nb\nc\n", 1, &["b"], &["B"]).0, "a\r\nB\r\nc\n");
    }
}
