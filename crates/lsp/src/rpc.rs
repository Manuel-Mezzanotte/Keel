//! JSON-RPC over a byte stream, as language servers speak it: each message
//! is a `Content-Length` header, a blank line and a JSON body.

use std::io::{self, BufRead, Write};

use serde_json::Value;

/// Writes one message.
pub fn write_message(writer: &mut impl Write, message: &Value) -> io::Result<()> {
    let body = serde_json::to_vec(message)?;
    write!(writer, "Content-Length: {}\r\n\r\n", body.len())?;
    writer.write_all(&body)?;
    writer.flush()
}

/// Reads one message. `Ok(None)` means the stream has ended.
pub fn read_message(reader: &mut impl BufRead) -> io::Result<Option<Value>> {
    let mut length = None;
    let mut line = String::new();
    loop {
        line.clear();
        if reader.read_line(&mut line)? == 0 {
            return Ok(None);
        }
        let header = line.trim_end();
        if header.is_empty() {
            // Some servers print stray blank lines before a header.
            if length.is_some() {
                break;
            }
            continue;
        }
        if let Some((name, value)) = header.split_once(':')
            && name.eq_ignore_ascii_case("content-length")
        {
            length = value.trim().parse::<usize>().ok();
        }
    }
    let length = length.unwrap_or_default();
    let mut body = vec![0; length];
    reader.read_exact(&mut body)?;
    serde_json::from_slice(&body)
        .map(Some)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn messages_round_trip_with_utf8_bodies() {
        let mut buffer = Vec::new();
        let first = json!({ "jsonrpc": "2.0", "method": "note", "params": { "text": "perché" } });
        let second = json!({ "jsonrpc": "2.0", "id": 1, "result": null });
        write_message(&mut buffer, &first).unwrap();
        write_message(&mut buffer, &second).unwrap();
        let mut reader = io::Cursor::new(buffer);
        assert_eq!(read_message(&mut reader).unwrap(), Some(first));
        assert_eq!(read_message(&mut reader).unwrap(), Some(second));
        assert_eq!(read_message(&mut reader).unwrap(), None);
    }

    #[test]
    fn extra_headers_and_blank_lines_are_skipped() {
        let body = r#"{"id":2}"#;
        let raw = format!(
            "\r\nContent-Type: application/vscode-jsonrpc; charset=utf-8\r\ncontent-length: {}\r\n\r\n{body}",
            body.len()
        );
        let mut reader = io::Cursor::new(raw.into_bytes());
        assert_eq!(read_message(&mut reader).unwrap(), Some(json!({ "id": 2 })));
    }
}
