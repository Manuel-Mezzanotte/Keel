//! Creating, renaming, moving and deleting files and folders. Deleting
//! moves to the Trash, so nothing is lost for good.

use std::fs;
use std::io::{self, ErrorKind};
use std::path::{Component, Path, PathBuf};

/// Checks a name typed by the user. `a/b.ts` is allowed and makes the
/// folders in between, as in VS Code.
fn checked(folder: &Path, name: &str) -> io::Result<PathBuf> {
    let name = name.trim();
    let invalid = || {
        io::Error::new(
            ErrorKind::InvalidInput,
            format!("\"{name}\" is not a valid name"),
        )
    };
    if name.is_empty() || name.starts_with('/') || name.contains('\0') {
        return Err(invalid());
    }
    let relative = Path::new(name);
    if relative
        .components()
        .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(invalid());
    }
    let target = folder.join(relative);
    if target.exists() {
        return Err(io::Error::new(
            ErrorKind::AlreadyExists,
            format!("{} already exists", target.display()),
        ));
    }
    Ok(target)
}

/// Creates an empty file in a folder.
pub fn create_file(folder: &Path, name: &str) -> io::Result<PathBuf> {
    let target = checked(folder, name)?;
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&target)?;
    Ok(target)
}

pub fn create_folder(folder: &Path, name: &str) -> io::Result<PathBuf> {
    let target = checked(folder, name)?;
    fs::create_dir_all(&target)?;
    Ok(target)
}

/// Renames a file or folder in place. Changing only the case of a name
/// works too, although macOS sees both names as the same file.
pub fn rename(path: &Path, new_name: &str) -> io::Result<PathBuf> {
    let folder = path
        .parent()
        .ok_or_else(|| io::Error::new(ErrorKind::InvalidInput, "the project cannot be renamed"))?;
    let same_but_case = path.file_name().is_some_and(|name| {
        name.to_string_lossy().to_lowercase() == new_name.trim().to_lowercase()
    });
    let target = if same_but_case {
        folder.join(new_name.trim())
    } else {
        checked(folder, new_name)?
    };
    if target == path {
        return Ok(target);
    }
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::rename(path, &target)?;
    Ok(target)
}

/// Moves a file or folder into another folder, keeping its name.
pub fn move_into(path: &Path, folder: &Path) -> io::Result<PathBuf> {
    if folder.starts_with(path) {
        return Err(io::Error::new(
            ErrorKind::InvalidInput,
            "a folder cannot go inside itself",
        ));
    }
    let name = path
        .file_name()
        .ok_or_else(|| io::Error::new(ErrorKind::InvalidInput, "nothing to move"))?;
    let target = folder.join(name);
    if target == path {
        return Ok(target);
    }
    if target.exists() {
        return Err(io::Error::new(
            ErrorKind::AlreadyExists,
            format!("{} already exists", target.display()),
        ));
    }
    fs::rename(path, &target)?;
    Ok(target)
}

/// Moves a file or folder to the Trash.
pub fn trash(path: &Path) -> io::Result<()> {
    trash::delete(path).map_err(io::Error::other)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn files_and_folders_are_created_with_valid_names() {
        let folder = tempfile::tempdir().unwrap();
        let root = folder.path();
        let file = create_file(root, "src/lib/utils.ts").unwrap();
        assert_eq!(file, root.join("src/lib/utils.ts"));
        assert!(file.is_file());
        assert!(create_folder(root, "public").unwrap().is_dir());

        for name in ["", "  ", "../escape.ts", "/abs.ts", "a/../b"] {
            let error = create_file(root, name).unwrap_err();
            assert_eq!(error.kind(), ErrorKind::InvalidInput, "{name:?}");
        }
        let error = create_file(root, "src/lib/utils.ts").unwrap_err();
        assert_eq!(error.kind(), ErrorKind::AlreadyExists);
    }

    #[test]
    fn renaming_keeps_the_folder_and_allows_case_changes() {
        let folder = tempfile::tempdir().unwrap();
        let file = create_file(folder.path(), "card.tsx").unwrap();
        create_file(folder.path(), "other.tsx").unwrap();

        let renamed = rename(&file, "Card.tsx").unwrap();
        assert_eq!(renamed.file_name().unwrap(), "Card.tsx");
        let names: Vec<String> = fs::read_dir(folder.path())
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert!(names.contains(&"Card.tsx".to_owned()), "{names:?}");

        let error = rename(&renamed, "other.tsx").unwrap_err();
        assert_eq!(error.kind(), ErrorKind::AlreadyExists);
        assert!(rename(&renamed, "BookingCard.tsx").unwrap().is_file());
    }

    #[test]
    fn moving_goes_into_folders_but_never_into_itself() {
        let folder = tempfile::tempdir().unwrap();
        let root = folder.path();
        let file = create_file(root, "page.tsx").unwrap();
        let src = create_folder(root, "src").unwrap();
        let nested = create_folder(root, "src/components").unwrap();

        let moved = move_into(&file, &src).unwrap();
        assert_eq!(moved, src.join("page.tsx"));
        assert!(moved.is_file() && !file.exists());

        let error = move_into(&src, &nested).unwrap_err();
        assert_eq!(error.kind(), ErrorKind::InvalidInput);

        create_file(root, "page.tsx").unwrap();
        let error = move_into(&root.join("page.tsx"), &src).unwrap_err();
        assert_eq!(error.kind(), ErrorKind::AlreadyExists);
    }
}
