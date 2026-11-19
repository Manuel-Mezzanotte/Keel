//! How a file's text is written as bytes: UTF-8, with or without the mark
//! at the start, UTF-16, or Windows 1252 for the old files of Windows and
//! of many Italian projects. A file says which by its first bytes; one
//! that is not valid UTF-8 and has no zero bytes is taken as Windows 1252,
//! as VS Code guesses.

/// The encodings Keel reads and writes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Encoding {
    #[default]
    Utf8,
    /// UTF-8 with the byte order mark at the start.
    Utf8Bom,
    Utf16Le,
    Utf16Be,
    Windows1252,
}

/// Windows 1252 from 0x80 to 0x9F; the five unused bytes stand for
/// themselves, as the WHATWG decoder has them.
const WINDOWS_1252_HIGH: [char; 32] = [
    '\u{20AC}', '\u{81}', '\u{201A}', '\u{192}', '\u{201E}', '\u{2026}', '\u{2020}', '\u{2021}',
    '\u{2C6}', '\u{2030}', '\u{160}', '\u{2039}', '\u{152}', '\u{8D}', '\u{17D}', '\u{8F}',
    '\u{90}', '\u{2018}', '\u{2019}', '\u{201C}', '\u{201D}', '\u{2022}', '\u{2013}', '\u{2014}',
    '\u{2DC}', '\u{2122}', '\u{161}', '\u{203A}', '\u{153}', '\u{9D}', '\u{17E}', '\u{178}',
];

/// How far into a file to look for zero bytes, which only binary files
/// have outside UTF-16.
const SNIFFED: usize = 8000;

impl Encoding {
    pub const ALL: [Encoding; 5] = [
        Encoding::Utf8,
        Encoding::Utf8Bom,
        Encoding::Utf16Le,
        Encoding::Utf16Be,
        Encoding::Windows1252,
    ];

    /// The name the status bar shows, as VS Code's.
    pub fn name(self) -> &'static str {
        match self {
            Encoding::Utf8 => "UTF-8",
            Encoding::Utf8Bom => "UTF-8 with BOM",
            Encoding::Utf16Le => "UTF-16 LE",
            Encoding::Utf16Be => "UTF-16 BE",
            Encoding::Windows1252 => "Windows 1252",
        }
    }

    /// The text of a file in this encoding, or `None` when its bytes are
    /// not valid for it.
    pub fn decode(self, bytes: &[u8]) -> Option<String> {
        match self {
            Encoding::Utf8 => String::from_utf8(bytes.to_vec()).ok(),
            Encoding::Utf8Bom => String::from_utf8(
                bytes
                    .strip_prefix(b"\xEF\xBB\xBF")
                    .unwrap_or(bytes)
                    .to_vec(),
            )
            .ok(),
            Encoding::Utf16Le | Encoding::Utf16Be => {
                let bom: &[u8] = if self == Encoding::Utf16Le {
                    b"\xFF\xFE"
                } else {
                    b"\xFE\xFF"
                };
                let bytes = bytes.strip_prefix(bom).unwrap_or(bytes);
                if !bytes.len().is_multiple_of(2) {
                    return None;
                }
                let units = bytes.as_chunks::<2>().0.iter().map(|pair| {
                    if self == Encoding::Utf16Le {
                        u16::from_le_bytes([pair[0], pair[1]])
                    } else {
                        u16::from_be_bytes([pair[0], pair[1]])
                    }
                });
                char::decode_utf16(units)
                    .collect::<Result<String, _>>()
                    .ok()
            }
            Encoding::Windows1252 => Some(
                bytes
                    .iter()
                    .map(|&byte| match byte {
                        0x80..=0x9F => WINDOWS_1252_HIGH[usize::from(byte - 0x80)],
                        _ => char::from(byte),
                    })
                    .collect(),
            ),
        }
    }

    /// The bytes of `text` in this encoding, or the first character it has
    /// no byte for.
    pub fn encode(self, text: &str) -> Result<Vec<u8>, char> {
        Ok(match self {
            Encoding::Utf8 => text.as_bytes().to_vec(),
            Encoding::Utf8Bom => [b"\xEF\xBB\xBF".as_slice(), text.as_bytes()].concat(),
            Encoding::Utf16Le => [0xFEFFu16]
                .into_iter()
                .chain(text.encode_utf16())
                .flat_map(u16::to_le_bytes)
                .collect(),
            Encoding::Utf16Be => [0xFEFFu16]
                .into_iter()
                .chain(text.encode_utf16())
                .flat_map(u16::to_be_bytes)
                .collect(),
            Encoding::Windows1252 => {
                let mut bytes = Vec::with_capacity(text.len());
                for c in text.chars() {
                    let byte = match u32::from(c) {
                        code @ (0..=0x7F | 0xA0..=0xFF) => code as u8,
                        _ => match WINDOWS_1252_HIGH.iter().position(|&high| high == c) {
                            Some(index) => 0x80 + index as u8,
                            None => return Err(c),
                        },
                    };
                    bytes.push(byte);
                }
                bytes
            }
        })
    }

    /// The encoding a file is in and its text: by the mark at its start,
    /// else UTF-8 when its bytes are, else Windows 1252, unless zero bytes
    /// say it is not text at all.
    pub fn detect(bytes: &[u8]) -> Option<(Encoding, String)> {
        let by_mark = if bytes.starts_with(b"\xEF\xBB\xBF") {
            Some(Encoding::Utf8Bom)
        } else if bytes.starts_with(b"\xFF\xFE") {
            Some(Encoding::Utf16Le)
        } else if bytes.starts_with(b"\xFE\xFF") {
            Some(Encoding::Utf16Be)
        } else {
            None
        };
        if let Some(encoding) = by_mark
            && let Some(text) = encoding.decode(bytes)
            && (encoding == Encoding::Utf8Bom || !text.contains('\0'))
        {
            return Some((encoding, text));
        }
        if bytes[..bytes.len().min(SNIFFED)].contains(&0) {
            return None;
        }
        if let Some(text) = Encoding::Utf8.decode(bytes) {
            return Some((Encoding::Utf8, text));
        }
        Encoding::Windows1252
            .decode(bytes)
            .map(|text| (Encoding::Windows1252, text))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_encoding_round_trips() {
        let text = "città € “virgolette” — ñ\r\n";
        for encoding in Encoding::ALL {
            let bytes = encoding.encode(text).unwrap();
            assert_eq!(
                encoding.decode(&bytes).as_deref(),
                Some(text),
                "{encoding:?}"
            );
            assert_eq!(Encoding::detect(&bytes), Some((encoding, text.to_owned())));
        }
    }

    #[test]
    fn detection_by_mark_validity_and_zero_bytes() {
        assert_eq!(
            Encoding::detect(b"plain"),
            Some((Encoding::Utf8, "plain".to_owned()))
        );
        // «città» as an old Windows editor wrote it.
        assert_eq!(
            Encoding::detect(b"citt\xE0"),
            Some((Encoding::Windows1252, "città".to_owned()))
        );
        assert_eq!(Encoding::detect(b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR"), None);
        assert_eq!(Encoding::detect(&[0x00, 0xFF, 0x00, 0x80]), None);
    }

    #[test]
    fn what_windows_1252_cannot_write_is_said() {
        assert_eq!(Encoding::Windows1252.encode("ok 😀"), Err('😀'));
        assert_eq!(Encoding::Windows1252.encode("€"), Ok(vec![0x80]));
    }
}
