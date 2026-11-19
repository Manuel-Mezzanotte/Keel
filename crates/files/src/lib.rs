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
pub use operations::{
    copy_into, create_file, create_folder, free_name, move_into, rename, trash, trashed_location,
};
pub use tree::{DEFAULT_EXCLUDES, FileTree, NestingPatterns, Row, compare_names, default_nesting};
pub use watch::{ProjectWatcher, watch};
