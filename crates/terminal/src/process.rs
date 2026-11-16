//! What runs in a terminal: the program in front (zsh, node, claude…) and
//! the folder it is in, read from macOS.

use std::ffi::CStr;
use std::os::fd::RawFd;
use std::path::PathBuf;

/// The process group in front on the terminal: the shell when it waits
/// for a command, or the command it runs.
pub(crate) fn foreground(pty: RawFd) -> Option<u32> {
    // SAFETY: tcgetpgrp only reads the descriptor, which the terminal keeps
    // open while it lives.
    let group = unsafe { libc::tcgetpgrp(pty) };
    (group > 0).then_some(group as u32)
}

/// The name of a process, such as `zsh` or `node`.
#[cfg(target_os = "macos")]
pub(crate) fn name(pid: u32) -> Option<String> {
    let mut buffer = [0u8; 256];
    // SAFETY: proc_name writes at most `buffer.len()` bytes.
    let length =
        unsafe { libc::proc_name(pid as i32, buffer.as_mut_ptr().cast(), buffer.len() as u32) };
    if length <= 0 {
        return None;
    }
    Some(String::from_utf8_lossy(&buffer[..length as usize]).into_owned())
}

/// The folder a process is in.
#[cfg(target_os = "macos")]
pub(crate) fn current_dir(pid: u32) -> Option<PathBuf> {
    use std::mem::{MaybeUninit, size_of};
    use std::os::unix::ffi::OsStrExt;

    let mut info = MaybeUninit::<libc::proc_vnodepathinfo>::zeroed();
    let size = size_of::<libc::proc_vnodepathinfo>() as i32;
    // SAFETY: proc_pidinfo fills at most `size` bytes of `info`.
    let written = unsafe {
        libc::proc_pidinfo(
            pid as i32,
            libc::PROC_PIDVNODEPATHINFO,
            0,
            info.as_mut_ptr().cast(),
            size,
        )
    };
    if written != size {
        return None;
    }
    // SAFETY: the call filled the whole structure.
    let info = unsafe { info.assume_init() };
    let path = &info.pvi_cdir.vip_path;
    // SAFETY: the kernel ends the path with a zero inside the array.
    let path = unsafe { CStr::from_ptr(path.as_ptr().cast()) };
    let path = std::ffi::OsStr::from_bytes(path.to_bytes());
    (!path.is_empty()).then(|| PathBuf::from(path))
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn name(_pid: u32) -> Option<String> {
    None
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn current_dir(_pid: u32) -> Option<PathBuf> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_this_process() {
        let pid = std::process::id();
        assert!(name(pid).is_some_and(|name| !name.is_empty()));
        assert_eq!(
            current_dir(pid).map(|path| path.canonicalize().unwrap()),
            Some(std::env::current_dir().unwrap().canonicalize().unwrap())
        );
    }
}
