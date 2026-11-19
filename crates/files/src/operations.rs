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

/// A name free in `folder` for a copy of `name`, as Finder makes it:
/// `card.tsx`, then `card copy.tsx`, `card copy 2.tsx`. `word` is
/// «copy» in the interface's language.
pub fn free_name(folder: &Path, name: &str, word: &str) -> PathBuf {
    let first = folder.join(name);
    if !first.exists() {
        return first;
    }
    // A dotfile, such as `.env`, has no extension to keep apart.
    let (stem, extension) = match name.rfind('.') {
        Some(dot) if dot > 0 => (&name[..dot], &name[dot..]),
        _ => (name, ""),
    };
    (1..)
        .map(|count| {
            let suffix = if count == 1 {
                format!(" {word}")
            } else {
                format!(" {word} {count}")
            };
            folder.join(format!("{stem}{suffix}{extension}"))
        })
        .find(|candidate| !candidate.exists())
        .unwrap_or(first)
}

/// Copies a file or a whole folder into `folder`, under a free name.
pub fn copy_into(path: &Path, folder: &Path, word: &str) -> io::Result<PathBuf> {
    let name = path
        .file_name()
        .ok_or_else(|| io::Error::new(ErrorKind::InvalidInput, "no name"))?
        .to_string_lossy()
        .into_owned();
    if folder.starts_with(path) {
        return Err(io::Error::new(
            ErrorKind::InvalidInput,
            "a folder cannot be copied into itself",
        ));
    }
    let target = free_name(folder, &name, word);
    copy_all(path, &target)?;
    Ok(target)
}

fn copy_all(from: &Path, to: &Path) -> io::Result<()> {
    if from.is_dir() {
        std::fs::create_dir_all(to)?;
        for entry in std::fs::read_dir(from)? {
            let entry = entry?;
            copy_all(&entry.path(), &to.join(entry.file_name()))?;
        }
        Ok(())
    } else {
        std::fs::copy(from, to).map(drop)
    }
}

/// Where a file just moved to the Trash went, to bring it back: the
/// newest item of the Trash with its name, or with Finder's « 2» after it.
pub fn trashed_location(original: &Path) -> Option<PathBuf> {
    let trash = PathBuf::from(std::env::var_os("HOME")?).join(".Trash");
    let name = original.file_name()?.to_string_lossy().into_owned();
    let (stem, extension) = match name.rfind('.') {
        Some(dot) if dot > 0 => (name[..dot].to_owned(), name[dot..].to_owned()),
        _ => (name.clone(), String::new()),
    };
    std::fs::read_dir(&trash)
        .ok()?
        .flatten()
        .filter(|entry| {
            let found = entry.file_name().to_string_lossy().into_owned();
            found == name || (found.starts_with(&format!("{stem} ")) && found.ends_with(&extension))
        })
        .filter_map(|entry| {
            let changed = entry.metadata().ok()?.modified().ok()?;
            Some((changed, entry.path()))
        })
        .max_by_key(|(changed, _)| *changed)
        .map(|(_, path)| path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copies_take_a_free_name() {
        let folder = tempfile::tempdir().unwrap();
        let root = folder.path();
        std::fs::create_dir_all(root.join("src/lib")).unwrap();
        std::fs::write(root.join("src/card.tsx"), "card").unwrap();
        std::fs::write(root.join("src/lib/a.ts"), "a").unwrap();
        let copy = copy_into(&root.join("src/card.tsx"), &root.join("src"), "copia").unwrap();
        assert_eq!(copy, root.join("src/card copia.tsx"));
        let again = copy_into(&root.join("src/card.tsx"), &root.join("src"), "copia").unwrap();
        assert_eq!(again, root.join("src/card copia 2.tsx"));
        // A folder comes with what it holds.
        let lib = copy_into(&root.join("src/lib"), root, "copia").unwrap();
        assert_eq!(std::fs::read_to_string(lib.join("a.ts")).unwrap(), "a");
        assert!(copy_into(&root.join("src"), &root.join("src/lib"), "copia").is_err());
        assert_eq!(free_name(root, ".env", "copy"), root.join(".env"));
    }

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
