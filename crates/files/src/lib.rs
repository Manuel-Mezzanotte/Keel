//! A project's files: the tree the Explorer shows, the files git ignores,
//! and the operations on files and folders.

mod operations;
mod tree;

pub use operations::{create_file, create_folder, move_into, rename, trash};
pub use tree::{DEFAULT_EXCLUDES, FileTree, Row};
