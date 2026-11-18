//! Crashes, kept on this Mac only:
//!
//! - a panic writes `crashes/crash-<date>-<pid>.log` in the data folder,
//!   with Keel's version, the thread, the message, where it happened and
//!   the backtrace;
//! - each running Keel leaves `running/<pid>`, removed when it quits
//!   normally. One left behind by a process that is gone means the last run
//!   ended abruptly (a crash, `kill -9`, the Mac turned off): the next Keel
//!   says so, and names the crash log when there is one.
//!
//! Unsaved changes come back after either, from the backups (see
//! [`crate::session`]).

use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::SystemTime;

/// How many crash logs are kept; older ones go.
const KEPT_LOGS: usize = 20;

pub fn crashes_dir(data: &Path) -> PathBuf {
    data.join("crashes")
}

fn running_dir(data: &Path) -> PathBuf {
    data.join("running")
}

/// What the previous run left, found at the start of this one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UncleanExit {
    /// The crash log it wrote, if it was a panic.
    pub log: Option<PathBuf>,
}

/// Notes the end of the last run, then starts watching this one: the
/// panic hook and this process's `running` mark.
pub fn start(data: &Path) -> Option<UncleanExit> {
    let unclean = previous_unclean_exit(data, process_alive);
    install_panic_hook(crashes_dir(data));
    let mark = running_dir(data).join(std::process::id().to_string());
    if fs::create_dir_all(running_dir(data)).is_ok() {
        let _ = fs::write(&mark, "");
    }
    remove_mark_on_sigterm(&mark);
    *RUNNING_MARK.lock().unwrap() = Some(mark);
    prune_logs(&crashes_dir(data), KEPT_LOGS);
    if let Some(unclean) = &unclean {
        *NOTICE.lock().unwrap() = Some(unclean.clone());
    }
    unclean
}

/// Removes this process's `running` mark: Keel is quitting normally.
pub fn finish() {
    if let Some(mark) = RUNNING_MARK.lock().unwrap().take() {
        let _ = fs::remove_file(mark);
    }
}

/// The unclean exit to tell the first window about, once.
pub fn take_notice() -> Option<UncleanExit> {
    NOTICE.lock().unwrap().take()
}

static RUNNING_MARK: Mutex<Option<PathBuf>> = Mutex::new(None);

/// The mark as a C string, for the signal handler, which may not lock or
/// allocate.
static MARK_FOR_SIGNAL: OnceLock<std::ffi::CString> = OnceLock::new();

/// `kill` and the logout of macOS ask politely with SIGTERM: that is a
/// normal end, not one to warn about. The mark goes, then the signal does
/// what it always did.
fn remove_mark_on_sigterm(mark: &Path) {
    use std::os::unix::ffi::OsStrExt as _;
    let Ok(path) = std::ffi::CString::new(mark.as_os_str().as_bytes()) else {
        return;
    };
    if MARK_FOR_SIGNAL.set(path).is_err() {
        return;
    }
    extern "C" fn on_sigterm(signal: libc::c_int) {
        if let Some(path) = MARK_FOR_SIGNAL.get() {
            // SAFETY: `unlink`, `signal` and `raise` are safe in a signal
            // handler; the path was made before the handler was set.
            unsafe { libc::unlink(path.as_ptr()) };
        }
        unsafe {
            libc::signal(signal, libc::SIG_DFL);
            libc::raise(signal);
        }
    }
    // SAFETY: installs a handler that only does what is safe in one.
    unsafe {
        libc::signal(libc::SIGTERM, on_sigterm as *const () as libc::sighandler_t);
    }
}
static NOTICE: Mutex<Option<UncleanExit>> = Mutex::new(None);

fn process_alive(pid: i32) -> bool {
    // Signal 0 only checks that the process exists.
    // SAFETY: `kill` with signal 0 sends nothing.
    unsafe { libc::kill(pid, 0) == 0 }
}

/// Marks of processes that are gone mean an unclean exit. They are removed;
/// the newest crash log written after the oldest of them goes with it.
fn previous_unclean_exit(data: &Path, alive: impl Fn(i32) -> bool) -> Option<UncleanExit> {
    let entries = fs::read_dir(running_dir(data)).ok()?;
    let mut since: Option<SystemTime> = None;
    for entry in entries.flatten() {
        let Some(pid) = entry
            .file_name()
            .to_str()
            .and_then(|name| name.parse::<i32>().ok())
        else {
            continue;
        };
        if pid == std::process::id() as i32 || alive(pid) {
            continue;
        }
        let started = entry
            .metadata()
            .and_then(|metadata| metadata.modified())
            .ok();
        since = match (since, started) {
            (Some(since), Some(started)) => Some(since.min(started)),
            (since, started) => since.or(started),
        };
        let _ = fs::remove_file(entry.path());
    }
    let since = since?;
    let log = logs(&crashes_dir(data))
        .into_iter()
        .filter(|(modified, _)| *modified >= since)
        .max_by_key(|(modified, _)| *modified)
        .map(|(_, path)| path);
    Some(UncleanExit { log })
}

/// Crash logs with when they were written, oldest first.
fn logs(dir: &Path) -> Vec<(SystemTime, PathBuf)> {
    let mut found: Vec<(SystemTime, PathBuf)> = fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|entry| entry.file_name().to_string_lossy().starts_with("crash-"))
        .filter_map(|entry| Some((entry.metadata().ok()?.modified().ok()?, entry.path())))
        .collect();
    found.sort();
    found
}

fn prune_logs(dir: &Path, keep: usize) {
    let found = logs(dir);
    let extra = found.len().saturating_sub(keep);
    for (_, path) in found.into_iter().take(extra) {
        let _ = fs::remove_file(path);
    }
}

fn install_panic_hook(dir: PathBuf) {
    static INSTALLED: OnceLock<()> = OnceLock::new();
    if INSTALLED.set(()).is_err() {
        return;
    }
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let thread = std::thread::current();
        let report = report(
            thread.name().unwrap_or("unnamed"),
            &panic_message(info.payload()),
            info.location().map(ToString::to_string).as_deref(),
            &std::backtrace::Backtrace::force_capture().to_string(),
        );
        let _ = write_log(&dir, &report);
        previous(info);
    }));
}

fn panic_message(payload: &(dyn std::any::Any + Send)) -> String {
    payload
        .downcast_ref::<&str>()
        .map(|text| (*text).to_owned())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "(no message)".to_owned())
}

/// The text of a crash log.
fn report(thread: &str, message: &str, location: Option<&str>, backtrace: &str) -> String {
    format!(
        "Keel {} crashed\nmacOS {} ({})\nThread: {thread}\nMessage: {message}\nAt: {}\n\nBacktrace:\n{backtrace}\n",
        env!("CARGO_PKG_VERSION"),
        macos_version().unwrap_or_else(|| "?".to_owned()),
        std::env::consts::ARCH,
        location.unwrap_or("?"),
    )
}

fn macos_version() -> Option<String> {
    let output = std::process::Command::new("/usr/bin/sw_vers")
        .arg("-productVersion")
        .output()
        .ok()?;
    Some(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

fn write_log(dir: &Path, report: &str) -> std::io::Result<PathBuf> {
    fs::create_dir_all(dir)?;
    let path = dir.join(format!("crash-{}-{}.log", timestamp(), std::process::id()));
    let mut file = fs::File::create(&path)?;
    file.write_all(report.as_bytes())?;
    file.sync_all()?;
    Ok(path)
}

/// Local time as `20261118-093012`.
fn timestamp() -> String {
    let seconds = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs()) as libc::time_t;
    // SAFETY: `localtime_r` fills the zeroed `tm` we own.
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    unsafe { libc::localtime_r(&seconds, &mut tm) };
    format!(
        "{:04}{:02}{:02}-{:02}{:02}{:02}",
        tm.tm_year + 1900,
        tm.tm_mon + 1,
        tm.tm_mday,
        tm.tm_hour,
        tm.tm_min,
        tm.tm_sec
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_clean_run_leaves_nothing_to_say() {
        let data = tempfile::tempdir().unwrap();
        assert_eq!(previous_unclean_exit(data.path(), |_| false), None);
        // A Keel still running is not an unclean exit.
        fs::create_dir_all(running_dir(data.path())).unwrap();
        fs::write(running_dir(data.path()).join("4242"), "").unwrap();
        assert_eq!(previous_unclean_exit(data.path(), |_| true), None);
        assert!(running_dir(data.path()).join("4242").exists());
    }

    #[test]
    fn a_mark_left_behind_means_an_unclean_exit_with_its_log() {
        let data = tempfile::tempdir().unwrap();
        fs::create_dir_all(running_dir(data.path())).unwrap();
        fs::write(running_dir(data.path()).join("4242"), "").unwrap();
        // Killed: no log.
        assert_eq!(
            previous_unclean_exit(data.path(), |_| false),
            Some(UncleanExit { log: None })
        );
        assert!(!running_dir(data.path()).join("4242").exists(), "said once");

        fs::write(running_dir(data.path()).join("4243"), "").unwrap();
        let log = write_log(&crashes_dir(data.path()), "Keel crashed").unwrap();
        assert_eq!(
            previous_unclean_exit(data.path(), |_| false),
            Some(UncleanExit { log: Some(log) })
        );
    }

    #[test]
    fn a_report_says_what_and_where() {
        let report = report(
            "main",
            "index out of bounds",
            Some("src/a.rs:3:5"),
            "0: main",
        );
        assert!(report.starts_with(&format!("Keel {} crashed\n", env!("CARGO_PKG_VERSION"))));
        assert!(report.contains("Thread: main\nMessage: index out of bounds\nAt: src/a.rs:3:5"));
        assert!(report.contains("Backtrace:\n0: main"));
        assert_eq!(timestamp().len(), 15);
    }

    #[test]
    fn only_the_newest_logs_are_kept() {
        let dir = tempfile::tempdir().unwrap();
        for index in 0..5 {
            let path = dir.path().join(format!("crash-{index}.log"));
            fs::write(&path, "").unwrap();
            let when = SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_000 + index);
            fs::File::options()
                .write(true)
                .open(&path)
                .unwrap()
                .set_modified(when)
                .unwrap();
        }
        prune_logs(dir.path(), 2);
        let mut left: Vec<String> = fs::read_dir(dir.path())
            .unwrap()
            .flatten()
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .collect();
        left.sort();
        assert_eq!(left, ["crash-3.log", "crash-4.log"]);
    }

    /// A real panic, in a child process of the tests, leaves a log.
    #[test]
    fn a_real_panic_writes_a_log() {
        if let Ok(dir) = std::env::var("KEEL_CRASH_CHILD") {
            install_panic_hook(PathBuf::from(dir));
            let empty: Vec<u8> = Vec::new();
            let _ = empty[std::hint::black_box(3)];
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let status = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "crash::tests::a_real_panic_writes_a_log",
                "--exact",
                "--nocapture",
            ])
            .env("KEEL_CRASH_CHILD", dir.path())
            .output()
            .unwrap();
        assert!(!status.status.success());
        let found = logs(dir.path());
        assert_eq!(found.len(), 1);
        let text = fs::read_to_string(&found[0].1).unwrap();
        assert!(text.contains("index out of bounds"), "{text}");
        assert!(text.contains("crash.rs"), "{text}");
    }
}
