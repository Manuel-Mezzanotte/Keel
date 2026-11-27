//! Pages of the web inside Keel's window, as Sail shows them: the WebKit of
//! macOS (`WKWebView`), laid over the window where Keel leaves room for it.
//! GPUI cannot draw a web page itself, so the page is a view of AppKit on
//! top of GPUI's: Keel tells it where to stand, when to hide, and asks it
//! for a picture of itself.

#[cfg(target_os = "macos")]
mod mac;

#[cfg(target_os = "macos")]
pub use mac::WebView;

/// An address as typed, made one WebKit opens: `localhost:3000` and
/// `127.0.0.1:5173/x` are local pages, served over `http`; other names
/// without a scheme go over `https`.
pub fn address(typed: &str) -> String {
    let typed = typed.trim();
    if typed.contains("://") || typed.starts_with("about:") || typed.starts_with("file:") {
        return typed.to_owned();
    }
    let host = typed.split(['/', ':']).next().unwrap_or_default();
    let local = host == "localhost"
        || host.ends_with(".localhost")
        || host.ends_with(".test")
        || host.chars().all(|c| c.is_ascii_digit() || c == '.');
    if local {
        format!("http://{typed}")
    } else {
        format!("https://{typed}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typed_addresses_become_pages() {
        assert_eq!(address("localhost:3000"), "http://localhost:3000");
        assert_eq!(
            address(" 127.0.0.1:5173/prenotazioni "),
            "http://127.0.0.1:5173/prenotazioni"
        );
        assert_eq!(address("app.localhost:8080"), "http://app.localhost:8080");
        assert_eq!(address("keel.mezzalab.eu"), "https://keel.mezzalab.eu");
        assert_eq!(
            address("http://localhost:3000/a"),
            "http://localhost:3000/a"
        );
        assert_eq!(address("about:blank"), "about:blank");
    }
}
