//! Documents and editing for Keel.

mod document;
mod encoding;
pub mod line_edits;
pub mod text_edits;

pub use document::{Document, OpenError};
pub use encoding::Encoding;
