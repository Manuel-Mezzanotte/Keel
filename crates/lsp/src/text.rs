//! What changed in a document, as language servers want to hear it.

use lsp_types::{Position, Range, TextDocumentContentChangeEvent};
use workbench::position::position_of;

/// The smallest single edit that turns `old` into `new`, or `None` when they
/// are the same. Sending only this keeps every keystroke cheap, even in a
/// large file.
pub fn content_change(old: &str, new: &str) -> Option<TextDocumentContentChangeEvent> {
    if old == new {
        return None;
    }
    let mut prefix = old
        .bytes()
        .zip(new.bytes())
        .take_while(|(a, b)| a == b)
        .count();
    while !old.is_char_boundary(prefix) || !new.is_char_boundary(prefix) {
        prefix -= 1;
    }
    // Never split a `\r\n`: servers count it as one line break.
    if prefix > 0 && old.as_bytes()[prefix - 1] == b'\r' {
        prefix -= 1;
    }
    let longest = old.len().min(new.len()) - prefix;
    let mut suffix = old
        .bytes()
        .rev()
        .zip(new.bytes().rev())
        .take(longest)
        .take_while(|(a, b)| a == b)
        .count();
    while !old.is_char_boundary(old.len() - suffix) || !new.is_char_boundary(new.len() - suffix) {
        suffix -= 1;
    }
    if suffix > 0 && old.as_bytes()[old.len() - suffix] == b'\n' {
        let before = old.len() - suffix;
        if before > prefix && old.as_bytes()[before - 1] == b'\r' {
            suffix -= 1;
        }
    }
    let start = position_of(old, prefix);
    let end = position_of(old, old.len() - suffix);
    Some(TextDocumentContentChangeEvent {
        range: Some(Range::new(to_lsp(start), to_lsp(end))),
        range_length: None,
        text: new[prefix..new.len() - suffix].to_owned(),
    })
}

fn to_lsp(position: workbench::position::Position) -> Position {
    Position::new(position.line, position.character)
}

/// Applies a change the way a server does: used by the tests to check that
/// the changes Keel sends rebuild the text exactly.
pub fn apply_change(text: &str, change: &TextDocumentContentChangeEvent) -> String {
    let Some(range) = change.range else {
        return change.text.clone();
    };
    let offset = |position: Position| {
        workbench::position::offset_of(
            text,
            workbench::position::Position::new(position.line, position.character),
        )
    };
    let (start, end) = (offset(range.start), offset(range.end));
    format!("{}{}{}", &text[..start], change.text, &text[end..])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn round_trip(old: &str, new: &str) -> TextDocumentContentChangeEvent {
        let change = content_change(old, new).expect("the texts differ");
        assert_eq!(apply_change(old, &change), new, "{old:?} -> {new:?}");
        change
    }

    #[test]
    fn typing_sends_only_the_new_character() {
        let change = round_trip(
            "const a = 1;\nconst b = 2;\n",
            "const a = 12;\nconst b = 2;\n",
        );
        assert_eq!(change.text, "2");
        let range = change.range.unwrap();
        assert_eq!(
            (range.start, range.end),
            (Position::new(0, 11), Position::new(0, 11))
        );
    }

    #[test]
    fn deleting_and_replacing() {
        let change = round_trip("let nome = 'Keel';\n", "let nome = 'IDE';\n");
        assert_eq!(change.text, "IDE");
        round_trip("abc\ndef\nghi\n", "abc\nghi\n");
        round_trip("aaaa", "aa");
        round_trip("aa", "aaaa");
        round_trip("", "nuovo");
        round_trip("vecchio", "");
    }

    #[test]
    fn columns_count_utf16_and_line_breaks_stay_whole() {
        let change = round_trip("// caffè 🚀 x\n", "// caffè 🚀 y\n");
        let range = change.range.unwrap();
        // "// caffè " is 9 units, the rocket 2 more, then a space.
        assert_eq!(range.start, Position::new(0, 12));
        round_trip("a\r\nb\r\n", "a\r\n\r\nb\r\n");
        round_trip("a\r\nb", "a\nb");
        round_trip("a\nb", "a\r\nb");
        round_trip("è", "é");
    }

    #[test]
    fn the_same_text_is_no_change() {
        assert!(content_change("uguale", "uguale").is_none());
    }
}
