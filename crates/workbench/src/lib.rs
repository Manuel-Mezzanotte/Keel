//! Editor logic shared by Keel's window and its extension bridge, kept free
//! of any UI so it can be tested on its own.

pub mod keybinding;
pub mod line_diff;
pub mod position;
pub mod uri;
pub mod when_clause;
