//! Shell integration for zsh, as VS Code's: zsh starts with Keel's own
//! ZDOTDIR, whose files read the user's and then add the hooks that mark
//! each prompt, command line, output and end (see `shell/keel.zsh`).

use std::ffi::CStr;
use std::fs;
use std::hash::{BuildHasher as _, Hasher as _};
use std::io;
use std::path::PathBuf;

/// The files of Keel's ZDOTDIR.
const FILES: [(&str, &str); 4] = [
    (".zshenv", include_str!("../shell/zshenv")),
    (".zprofile", include_str!("../shell/zprofile")),
    (".zshrc", include_str!("../shell/zshrc")),
    ("keel.zsh", include_str!("../shell/keel.zsh")),
];

/// The folder zsh starts from, written the first time it is needed and
/// again when Keel's files change.
pub(crate) fn zsh_folder() -> io::Result<PathBuf> {
    let folder = std::env::temp_dir().join(format!("keel-zsh-{}", env!("CARGO_PKG_VERSION")));
    fs::create_dir_all(&folder)?;
    for (name, text) in FILES {
        let path = folder.join(name);
        if fs::read_to_string(&path).ok().as_deref() == Some(text) {
            continue;
        }
        // Whole or not at all: a shell starting meanwhile must not read
        // half a file.
        let partial = folder.join(format!("{name}.{}", std::process::id()));
        fs::write(&partial, text)?;
        fs::rename(&partial, &path)?;
    }
    Ok(folder)
}

/// A number only the shell and Keel know, so a program printing the same
/// bytes cannot pass a command line off as the shell's.
pub(crate) fn nonce() -> String {
    let mut hasher = std::collections::hash_map::RandomState::new().build_hasher();
    hasher.write_u128(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|time| time.as_nanos())
            .unwrap_or_default(),
    );
    hasher.write_u32(std::process::id());
    format!("{:016x}", hasher.finish())
}

/// The user's shell, as Alacritty picks it: `$SHELL`, else the one of the
/// account.
pub(crate) fn login_shell() -> Option<String> {
    if let Ok(shell) = std::env::var("SHELL")
        && !shell.is_empty()
    {
        return Some(shell);
    }
    // SAFETY: getpwuid returns a pointer to static storage or null; the
    // strings are copied out right away.
    unsafe {
        let entry = libc::getpwuid(libc::getuid());
        if entry.is_null() || (*entry).pw_shell.is_null() {
            return None;
        }
        Some(
            CStr::from_ptr((*entry).pw_shell)
                .to_string_lossy()
                .into_owned(),
        )
    }
}
