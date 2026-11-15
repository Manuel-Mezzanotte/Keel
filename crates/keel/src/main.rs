//! Keel, a fast and lightweight code editor for macOS.
//!
//! This is the application entry point. For now it only reports its version;
//! the window and the editor arrive in phase 0.2 (see `docs/PIANO.md`).

fn main() {
    println!("Keel {}", env!("CARGO_PKG_VERSION"));
}
