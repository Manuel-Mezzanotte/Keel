//! Positions as VS Code counts them: a line and a column in UTF-16 code
//! units. Keel's editor works with UTF-8 byte offsets, so every position that
//! crosses the extension bridge goes through these conversions.

use serde::{Deserialize, Serialize};

/// A zero-based line and UTF-16 column, like `vscode.Position`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct Position {
    pub line: u32,
    pub character: u32,
}

impl Position {
    pub fn new(line: u32, character: u32) -> Self {
        Self { line, character }
    }
}

/// Byte offsets where each line starts. VS Code treats `\r\n`, `\n` and a
/// lone `\r` as line breaks.
fn line_starts(text: &str) -> Vec<usize> {
    let bytes = text.as_bytes();
    let mut starts = vec![0];
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'\r' if bytes.get(index + 1) == Some(&b'\n') => {
                index += 2;
                starts.push(index);
            }
            b'\r' | b'\n' => {
                index += 1;
                starts.push(index);
            }
            _ => index += 1,
        }
    }
    starts
}

/// The byte range of a line's text, without its line break.
fn line_content(text: &str, starts: &[usize], line: usize) -> (usize, usize) {
    let start = starts[line];
    let mut end = starts.get(line + 1).copied().unwrap_or(text.len());
    let bytes = text.as_bytes();
    if end > start && bytes[end - 1] == b'\n' {
        end -= 1;
    }
    if end > start && bytes[end - 1] == b'\r' {
        end -= 1;
    }
    (start, end)
}

/// The byte offset of `position`, clamped to the text like VS Code's
/// `validatePosition`: a line past the end means the end of the text, a
/// column past the end of a line means the end of that line.
pub fn offset_of(text: &str, position: Position) -> usize {
    let starts = line_starts(text);
    let line = position.line as usize;
    if line >= starts.len() {
        return text.len();
    }
    let (start, end) = line_content(text, &starts, line);
    let mut remaining = position.character as usize;
    for (index, character) in text[start..end].char_indices() {
        let width = character.len_utf16();
        if remaining < width {
            // Inside a surrogate pair: stay before the character.
            return start + index;
        }
        remaining -= width;
        if remaining == 0 {
            return start + index + character.len_utf8();
        }
    }
    if position.character == 0 { start } else { end }
}

/// The position of the byte `offset`. Offsets inside a character or past
/// the end are clamped to the nearest valid place before them.
pub fn position_of(text: &str, offset: usize) -> Position {
    let mut offset = offset.min(text.len());
    while !text.is_char_boundary(offset) {
        offset -= 1;
    }
    let starts = line_starts(text);
    let line = match starts.binary_search(&offset) {
        Ok(line) => line,
        Err(next) => next - 1,
    };
    let (start, end) = line_content(text, &starts, line);
    let column_end = offset.min(end);
    let character = text[start..column_end].encode_utf16().count();
    Position::new(line as u32, character as u32)
}

/// The UTF-16 length of `text`, which is what JavaScript strings count.
pub fn utf16_len(text: &str) -> usize {
    text.encode_utf16().count()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_ascii_round_trips() {
        let text = "const a = 1;\nlet b = 2;\n";
        for (offset, line, character) in [(0, 0, 0), (6, 0, 6), (13, 1, 0), (16, 1, 3), (24, 2, 0)]
        {
            assert_eq!(
                position_of(text, offset),
                Position::new(line, character),
                "{offset}"
            );
            assert_eq!(
                offset_of(text, Position::new(line, character)),
                offset,
                "{line}:{character}"
            );
        }
    }

    #[test]
    fn accented_letters_are_one_column_but_two_bytes() {
        let text = "è più\ncosì";
        assert_eq!(offset_of(text, Position::new(0, 1)), 2);
        assert_eq!(
            offset_of(text, Position::new(0, 5)),
            text.find('\n').unwrap()
        );
        assert_eq!(position_of(text, 2), Position::new(0, 1));
        assert_eq!(position_of(text, text.len()), Position::new(1, 4));
    }

    #[test]
    fn emoji_take_two_columns_like_in_javascript() {
        let text = "a😀b";
        assert_eq!(offset_of(text, Position::new(0, 1)), 1);
        assert_eq!(offset_of(text, Position::new(0, 3)), 5);
        // Halfway through the surrogate pair stays before the emoji.
        assert_eq!(offset_of(text, Position::new(0, 2)), 1);
        assert_eq!(position_of(text, 5), Position::new(0, 3));
        assert_eq!(utf16_len(text), 4);
    }

    #[test]
    fn windows_and_old_mac_line_breaks_count_as_one() {
        let text = "one\r\ntwo\rthree";
        assert_eq!(offset_of(text, Position::new(1, 0)), 5);
        assert_eq!(offset_of(text, Position::new(2, 0)), 9);
        assert_eq!(position_of(text, 9), Position::new(2, 0));
        // The end of line 0 is before the \r\n.
        assert_eq!(offset_of(text, Position::new(0, 99)), 3);
        assert_eq!(position_of(text, 4), Position::new(0, 3));
    }

    #[test]
    fn positions_past_the_end_are_clamped() {
        let text = "ab\ncd";
        assert_eq!(offset_of(text, Position::new(9, 0)), text.len());
        assert_eq!(offset_of(text, Position::new(0, 9)), 2);
        assert_eq!(position_of(text, 99), Position::new(1, 2));
        assert_eq!(offset_of("", Position::new(0, 0)), 0);
    }
}
