//! A text file on disk and the last version of it that was saved.

use std::fmt;
use std::fs;
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};

/// A text document backed by a file.
///
/// The document remembers the text as it was last read from or written to
/// disk, so the editor can tell whether the buffer has unsaved changes.
#[derive(Debug)]
pub struct Document {
    path: PathBuf,
    saved_text: String,
    exists_on_disk: bool,
}

/// Why a file could not be opened as a text document.
#[derive(Debug)]
pub enum OpenError {
    /// The file could not be read.
    Io(io::Error),
    /// The file is not valid UTF-8 text.
    NotUtf8,
    /// The path points to a directory.
    IsDirectory,
}

impl fmt::Display for OpenError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            OpenError::Io(error) => write!(f, "{error}"),
            OpenError::NotUtf8 => f.write_str("the file is not UTF-8 text"),
            OpenError::IsDirectory => f.write_str("the path is a directory"),
        }
    }
}

impl std::error::Error for OpenError {}

impl From<io::Error> for OpenError {
    fn from(error: io::Error) -> Self {
        OpenError::Io(error)
    }
}

impl Document {
    /// Opens the file at `path`.
    ///
    /// A missing file opens as an empty document; it is created on the first
    /// save, like `code missing.ts` does in VS Code.
    pub fn open(path: impl Into<PathBuf>) -> Result<Self, OpenError> {
        let path = path.into();
        match fs::metadata(&path) {
            Ok(metadata) if metadata.is_dir() => Err(OpenError::IsDirectory),
            Ok(_) => {
                let bytes = fs::read(&path)?;
                let saved_text = String::from_utf8(bytes).map_err(|_| OpenError::NotUtf8)?;
                Ok(Self {
                    path,
                    saved_text,
                    exists_on_disk: true,
                })
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Self {
                path,
                saved_text: String::new(),
                exists_on_disk: false,
            }),
            Err(error) => Err(error.into()),
        }
    }

    /// The path the document was opened from.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The file's text if it changed on disk since Keel last read or saved
    /// it, for example because Claude Code wrote it. A missing or unreadable
    /// file counts as unchanged.
    pub fn changed_on_disk(&self) -> Option<String> {
        let text = fs::read_to_string(&self.path).ok()?;
        (text != self.saved_text).then_some(text)
    }

    /// Takes the text now on disk as the saved one, after a reload.
    pub fn accept_disk_text(&mut self, text: String) {
        self.saved_text = text;
        self.exists_on_disk = true;
    }

    /// Follows the file after it was renamed or moved on disk.
    pub fn set_path(&mut self, path: impl Into<PathBuf>) {
        self.path = path.into();
    }

    /// The file name, for window titles and tabs.
    pub fn file_name(&self) -> String {
        self.path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| self.path.display().to_string())
    }

    /// The text as it was last read from or written to disk.
    pub fn saved_text(&self) -> &str {
        &self.saved_text
    }

    /// Whether the file existed when it was opened or has been saved since.
    pub fn exists_on_disk(&self) -> bool {
        self.exists_on_disk
    }

    /// Whether `current` differs from what is on disk. A file that does not
    /// exist yet counts as empty.
    pub fn is_modified(&self, current: &str) -> bool {
        current != self.saved_text
    }

    /// Writes `text` to disk without ever leaving a half-written file.
    ///
    /// The text goes to a temporary file in the same folder, which then
    /// replaces the original in one step. Symbolic links are followed, so the
    /// file they point to is updated and the link itself stays in place. The
    /// permissions of an existing file are kept.
    pub fn save(&mut self, text: &str) -> io::Result<()> {
        let target = resolve_symlink(&self.path)?;
        write_atomically(&target, text.as_bytes())?;
        self.saved_text = text.to_owned();
        self.exists_on_disk = true;
        Ok(())
    }
}

/// Follows a symbolic link to the file it points to, if the path is one.
fn resolve_symlink(path: &Path) -> io::Result<PathBuf> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => fs::canonicalize(path),
        _ => Ok(path.to_path_buf()),
    }
}

fn write_atomically(path: &Path, contents: &[u8]) -> io::Result<()> {
    let folder = match path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent,
        _ => Path::new("."),
    };
    let file_name = path
        .file_name()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "the path has no file name"))?;
    let mut temporary_name = std::ffi::OsString::from(".");
    temporary_name.push(file_name);
    temporary_name.push(format!(".keel-save-{}", std::process::id()));
    let temporary_path = folder.join(temporary_name);

    let result = (|| {
        let mut file = fs::File::create(&temporary_path)?;
        file.write_all(contents)?;
        file.sync_all()?;
        if let Ok(metadata) = fs::metadata(path) {
            fs::set_permissions(&temporary_path, metadata.permissions())?;
        }
        fs::rename(&temporary_path, path)
    })();

    if result.is_err() {
        let _ = fs::remove_file(&temporary_path);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn changes_made_outside_are_noticed() {
        let folder = tempfile::tempdir().unwrap();
        let path = folder.path().join("app.ts");
        fs::write(&path, "one").unwrap();
        let mut document = Document::open(&path).unwrap();
        assert_eq!(document.changed_on_disk(), None);

        fs::write(&path, "two").unwrap();
        assert_eq!(document.changed_on_disk().as_deref(), Some("two"));
        document.accept_disk_text("two".to_owned());
        assert_eq!(document.changed_on_disk(), None);

        document.save("three").unwrap();
        assert_eq!(document.changed_on_disk(), None);
    }

    #[test]
    fn opens_an_existing_file() {
        let folder = tempfile::tempdir().unwrap();
        let path = folder.path().join("app.tsx");
        fs::write(&path, "export const a = 1;\n").unwrap();

        let document = Document::open(&path).unwrap();

        assert_eq!(document.saved_text(), "export const a = 1;\n");
        assert_eq!(document.file_name(), "app.tsx");
        assert!(document.exists_on_disk());
        assert!(!document.is_modified("export const a = 1;\n"));
        assert!(document.is_modified("export const a = 2;\n"));
    }

    #[test]
    fn a_missing_file_opens_empty_and_is_created_on_save() {
        let folder = tempfile::tempdir().unwrap();
        let path = folder.path().join("new.ts");

        let mut document = Document::open(&path).unwrap();
        assert_eq!(document.saved_text(), "");
        assert!(!document.exists_on_disk());
        assert!(!document.is_modified(""));
        assert!(document.is_modified("let x = 1;"));

        document.save("let x = 1;").unwrap();

        assert_eq!(fs::read_to_string(&path).unwrap(), "let x = 1;");
        assert!(document.exists_on_disk());
        assert!(!document.is_modified("let x = 1;"));
    }

    #[test]
    fn saving_replaces_the_content_and_leaves_no_temporary_files() {
        let folder = tempfile::tempdir().unwrap();
        let path = folder.path().join("index.ts");
        fs::write(&path, "old").unwrap();

        let mut document = Document::open(&path).unwrap();
        document.save("new contents with accents: è à ù").unwrap();

        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "new contents with accents: è à ù"
        );
        let entries: Vec<_> = fs::read_dir(folder.path()).unwrap().collect();
        assert_eq!(entries.len(), 1, "only the saved file should remain");
    }

    #[cfg(unix)]
    #[test]
    fn saving_keeps_the_file_permissions() {
        use std::os::unix::fs::PermissionsExt as _;

        let folder = tempfile::tempdir().unwrap();
        let path = folder.path().join("run.sh");
        fs::write(&path, "echo hi\n").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();

        let mut document = Document::open(&path).unwrap();
        document.save("echo bye\n").unwrap();

        let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o755);
    }

    #[cfg(unix)]
    #[test]
    fn saving_through_a_symlink_updates_the_target_and_keeps_the_link() {
        let folder = tempfile::tempdir().unwrap();
        let target = folder.path().join("real.ts");
        let link = folder.path().join("link.ts");
        fs::write(&target, "before").unwrap();
        std::os::unix::fs::symlink(&target, &link).unwrap();

        let mut document = Document::open(&link).unwrap();
        document.save("after").unwrap();

        assert!(
            fs::symlink_metadata(&link)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert_eq!(fs::read_to_string(&target).unwrap(), "after");
    }

    #[test]
    fn refuses_directories_and_binary_files() {
        let folder = tempfile::tempdir().unwrap();
        assert!(matches!(
            Document::open(folder.path()),
            Err(OpenError::IsDirectory)
        ));

        let binary = folder.path().join("image.bin");
        fs::write(&binary, [0xff, 0xfe, 0x00, 0x80]).unwrap();
        assert!(matches!(Document::open(&binary), Err(OpenError::NotUtf8)));
    }
}
