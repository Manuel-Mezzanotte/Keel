//! A project's files: the tree the Explorer shows, the files git ignores,
//! the list Quick Open searches with fuzzy matching, and the operations on
//! files and folders.

pub mod fuzzy;
mod list;
mod operations;
pub mod search;
mod tree;
mod watch;

pub use list::list_files;
pub use operations::{create_file, create_folder, move_into, rename, trash};
pub use tree::{DEFAULT_EXCLUDES, FileTree, Row, compare_names};
pub use watch::{ProjectWatcher, watch};
