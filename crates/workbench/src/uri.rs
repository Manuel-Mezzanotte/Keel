//! URIs written exactly as the `vscode` bridge writes them, so that both
//! sides of the extension host agree on how a document is called.

use std::path::{Path, PathBuf};

/// A URI split into its parts, with percent-encoding removed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Uri {
    pub scheme: String,
    pub authority: String,
    pub path: String,
    pub query: String,
    pub fragment: String,
}

impl Uri {
    /// The `file:` URI of an absolute path.
    pub fn file(path: &Path) -> Self {
        let path = path.to_string_lossy();
        Self {
            scheme: "file".to_owned(),
            authority: String::new(),
            path: if path.starts_with('/') {
                path.into_owned()
            } else {
                format!("/{path}")
            },
            query: String::new(),
            fragment: String::new(),
        }
    }

    /// Parses `scheme://authority/path?query#fragment` or `scheme:path`.
    pub fn parse(text: &str) -> Option<Self> {
        let (scheme, rest) = text.split_once(':')?;
        let valid_scheme = scheme.chars().next()?.is_ascii_alphabetic()
            && scheme
                .chars()
                .all(|character| character.is_ascii_alphanumeric() || "+.-".contains(character));
        if !valid_scheme {
            return None;
        }
        let (rest, fragment) = rest.split_once('#').unwrap_or((rest, ""));
        let (rest, query) = rest.split_once('?').unwrap_or((rest, ""));
        let (authority, path) = match rest.strip_prefix("//") {
            Some(after) => match after.find('/') {
                Some(slash) => (&after[..slash], &after[slash..]),
                None => (after, ""),
            },
            None => ("", rest),
        };
        Some(Self {
            scheme: scheme.to_owned(),
            authority: decode(authority),
            path: decode(path),
            query: decode(query),
            fragment: decode(fragment),
        })
    }

    /// The local path of a `file:` URI.
    pub fn file_path(&self) -> Option<PathBuf> {
        (self.scheme == "file").then(|| PathBuf::from(&self.path))
    }

    /// The last part of the path, for titles.
    pub fn file_name(&self) -> &str {
        self.path.rsplit('/').next().unwrap_or(&self.path)
    }
}

impl std::fmt::Display for Uri {
    /// Matches `vscode.Uri.toString()` in Keel's bridge.
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}:", self.scheme)?;
        if !self.authority.is_empty() || self.scheme == "file" {
            write!(formatter, "//{}", encode(&self.authority))?;
        }
        let path: Vec<String> = self.path.split('/').map(encode).collect();
        write!(formatter, "{}", path.join("/"))?;
        if !self.query.is_empty() {
            write!(formatter, "?{}", encode(&self.query))?;
        }
        if !self.fragment.is_empty() {
            write!(formatter, "#{}", encode(&self.fragment))?;
        }
        Ok(())
    }
}

/// Like JavaScript's `encodeURIComponent`.
fn encode(text: &str) -> String {
    let mut encoded = String::with_capacity(text.len());
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || b"-_.!~*'()".contains(&byte) {
            encoded.push(byte as char);
        } else {
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }
    encoded
}

/// Removes percent-encoding; malformed sequences are kept as they are.
fn decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            let hex = std::str::from_utf8(&bytes[index + 1..index + 3]).ok();
            if let Some(value) = hex.and_then(|hex| u8::from_str_radix(hex, 16).ok()) {
                decoded.push(value);
                index += 3;
                continue;
            }
        }
        decoded.push(bytes[index]);
        index += 1;
    }
    String::from_utf8(decoded).unwrap_or_else(|_| text.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_uris_are_encoded_like_javascript() {
        let uri = Uri::file(Path::new("/Users/manuel/Lavori in corso/perché.tsx"));
        assert_eq!(
            uri.to_string(),
            "file:///Users/manuel/Lavori%20in%20corso/perch%C3%A9.tsx"
        );
        assert_eq!(uri.file_name(), "perché.tsx");
    }

    #[test]
    fn parsing_reverses_encoding() {
        let uri = Uri::parse("file:///Users/manuel/Lavori%20in%20corso/perch%C3%A9.tsx").unwrap();
        assert_eq!(
            uri.file_path().unwrap(),
            PathBuf::from("/Users/manuel/Lavori in corso/perché.tsx")
        );
        assert_eq!(
            uri.to_string(),
            "file:///Users/manuel/Lavori%20in%20corso/perch%C3%A9.tsx"
        );
    }

    #[test]
    fn custom_schemes_with_query_and_fragment() {
        let text = "vsauto-ghost:/session-1/booking-card.tsx?id%3D3#top";
        let uri = Uri::parse(text).unwrap();
        assert_eq!(uri.scheme, "vsauto-ghost");
        assert_eq!(uri.path, "/session-1/booking-card.tsx");
        assert_eq!(uri.query, "id=3");
        assert_eq!(uri.fragment, "top");
        assert_eq!(uri.to_string(), text);
        assert_eq!(uri.file_path(), None);
    }

    #[test]
    fn rejects_text_without_a_scheme() {
        assert_eq!(Uri::parse("/just/a/path"), None);
        assert_eq!(Uri::parse("1abc:/x"), None);
    }
}
