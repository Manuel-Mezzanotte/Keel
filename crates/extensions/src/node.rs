//! The native half of the small Node.js layer extensions get (`fs`, `os`,
//! `http`...): file details as Node gives them, the real path of a file,
//! facts about this Mac and web requests through curl. Errors carry Node's
//! codes (`ENOENT: ...`) so the JavaScript side can rebuild Node's errors.

use std::fs;
use std::io;
use std::path::Path;
use std::process::{Command, Stdio};

use deno_core::{ToJsBuffer, op2};
use deno_error::JsErrorBox;
use serde::Serialize;

/// Node's code for an I/O error.
fn code(error: &io::Error) -> &'static str {
    match error.kind() {
        io::ErrorKind::NotFound => "ENOENT",
        io::ErrorKind::AlreadyExists => "EEXIST",
        io::ErrorKind::PermissionDenied => "EACCES",
        io::ErrorKind::NotADirectory => "ENOTDIR",
        io::ErrorKind::IsADirectory => "EISDIR",
        io::ErrorKind::DirectoryNotEmpty => "ENOTEMPTY",
        io::ErrorKind::InvalidInput => "EINVAL",
        _ => "EIO",
    }
}

pub(crate) fn node_error(error: io::Error, syscall: &str, path: &str) -> JsErrorBox {
    JsErrorBox::generic(format!("{}: {error}, {syscall} '{path}'", code(&error)))
}

fn milliseconds(time: io::Result<std::time::SystemTime>) -> f64 {
    time.ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0.0, |duration| duration.as_secs_f64() * 1000.0)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct NodeStat {
    /// 1 file, 2 directory, 64 symbolic link, 0 anything else.
    kind: u32,
    size: u64,
    mode: u32,
    ino: u64,
    dev: u64,
    nlink: u64,
    uid: u32,
    gid: u32,
    atime_ms: f64,
    mtime_ms: f64,
    ctime_ms: f64,
    birthtime_ms: f64,
}

/// `fs.stat` (`follow`) or `fs.lstat`.
#[op2]
#[serde]
pub(crate) fn op_keel_node_stat(
    #[string] path: String,
    follow: bool,
) -> Result<NodeStat, JsErrorBox> {
    use std::os::unix::fs::MetadataExt as _;
    let metadata = if follow {
        fs::metadata(&path)
    } else {
        fs::symlink_metadata(&path)
    }
    .map_err(|error| node_error(error, if follow { "stat" } else { "lstat" }, &path))?;
    let kind = metadata.file_type();
    Ok(NodeStat {
        kind: if kind.is_symlink() {
            64
        } else if kind.is_dir() {
            2
        } else if kind.is_file() {
            1
        } else {
            0
        },
        size: metadata.len(),
        mode: metadata.mode(),
        ino: metadata.ino(),
        dev: metadata.dev(),
        nlink: metadata.nlink(),
        uid: metadata.uid(),
        gid: metadata.gid(),
        atime_ms: milliseconds(metadata.accessed()),
        mtime_ms: milliseconds(metadata.modified()),
        ctime_ms: metadata.ctime() as f64 * 1000.0,
        birthtime_ms: milliseconds(metadata.created()),
    })
}

/// `fs.realpath`.
#[op2]
#[string]
pub(crate) fn op_keel_node_realpath(#[string] path: String) -> Result<String, JsErrorBox> {
    fs::canonicalize(&path)
        .map(|real| real.to_string_lossy().into_owned())
        .map_err(|error| node_error(error, "realpath", &path))
}

/// `fs.mkdir`, with or without `recursive`.
#[op2(fast)]
pub(crate) fn op_keel_node_mkdir(
    #[string] path: String,
    recursive: bool,
) -> Result<(), JsErrorBox> {
    let made = if recursive {
        fs::create_dir_all(&path)
    } else {
        fs::create_dir(&path)
    };
    made.map_err(|error| node_error(error, "mkdir", &path))
}

/// `fs.unlink`, `fs.rmdir` and `fs.rm`.
#[op2(fast)]
pub(crate) fn op_keel_node_remove(
    #[string] path: String,
    recursive: bool,
    directory: bool,
) -> Result<(), JsErrorBox> {
    let removed = match fs::symlink_metadata(&path) {
        Ok(metadata) if metadata.is_dir() && recursive => fs::remove_dir_all(&path),
        Ok(metadata) if metadata.is_dir() && directory => fs::remove_dir(&path),
        Ok(metadata) if metadata.is_dir() => Err(io::Error::from(io::ErrorKind::IsADirectory)),
        Ok(_) => fs::remove_file(&path),
        Err(error) => Err(error),
    };
    removed.map_err(|error| node_error(error, "rm", &path))
}

/// `fs.readFile` without the vscode codes of `op_keel_fs_read`.
#[op2]
#[buffer]
pub(crate) fn op_keel_node_read(#[string] path: String) -> Result<Vec<u8>, JsErrorBox> {
    fs::read(&path).map_err(|error| node_error(error, "open", &path))
}

/// `fs.writeFile` and `fs.appendFile`.
#[op2(fast)]
pub(crate) fn op_keel_node_write(
    #[string] path: String,
    #[buffer] data: &[u8],
    append: bool,
) -> Result<(), JsErrorBox> {
    use std::io::Write as _;
    let mut file = fs::OpenOptions::new()
        .create(true)
        .write(true)
        .append(append)
        .truncate(!append)
        .open(&path)
        .map_err(|error| node_error(error, "open", &path))?;
    file.write_all(data)
        .map_err(|error| node_error(error, "write", &path))
}

/// `fs.readdir`: names with the kinds of `op_keel_node_stat`.
#[op2]
#[serde]
pub(crate) fn op_keel_node_readdir(
    #[string] path: String,
) -> Result<Vec<(String, u32)>, JsErrorBox> {
    let mut entries = Vec::new();
    for entry in fs::read_dir(&path).map_err(|error| node_error(error, "scandir", &path))? {
        let entry = entry.map_err(|error| node_error(error, "scandir", &path))?;
        let kind = entry
            .file_type()
            .map_err(|error| node_error(error, "scandir", &path))?;
        let kind = if kind.is_symlink() {
            64
        } else if kind.is_dir() {
            2
        } else if kind.is_file() {
            1
        } else {
            0
        };
        entries.push((entry.file_name().to_string_lossy().into_owned(), kind));
    }
    Ok(entries)
}

/// `fs.rename` and `fs.copyFile`.
#[op2(fast)]
pub(crate) fn op_keel_node_move(
    #[string] from: String,
    #[string] to: String,
    copy: bool,
) -> Result<(), JsErrorBox> {
    let moved = if copy {
        fs::copy(&from, &to).map(|_| ())
    } else {
        fs::rename(&from, &to)
    };
    moved.map_err(|error| node_error(error, if copy { "copyfile" } else { "rename" }, &from))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct OsInfo {
    homedir: String,
    tmpdir: String,
    hostname: String,
    username: String,
    arch: &'static str,
    release: String,
    cpus: usize,
    env: Vec<(String, String)>,
}

fn sysctl(name: &str) -> String {
    Command::new("/usr/sbin/sysctl")
        .args(["-n", name])
        .output()
        .ok()
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_owned())
        .unwrap_or_default()
}

/// `os.homedir()`, `os.tmpdir()`... and the variables `process.env` shows:
/// only harmless ones, no secrets from the shell.
#[op2]
#[serde]
pub(crate) fn op_keel_node_os() -> OsInfo {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/".to_owned());
    let username = std::env::var("USER").unwrap_or_default();
    let env = ["HOME", "USER", "LANG", "TMPDIR", "SHELL", "PATH"]
        .iter()
        .filter_map(|key| Some(((*key).to_owned(), std::env::var(key).ok()?)))
        .collect();
    OsInfo {
        tmpdir: std::env::temp_dir()
            .to_string_lossy()
            .trim_end_matches('/')
            .to_owned(),
        hostname: sysctl("kern.hostname"),
        release: sysctl("kern.osrelease"),
        cpus: std::thread::available_parallelism().map_or(1, usize::from),
        arch: if cfg!(target_arch = "aarch64") {
            "arm64"
        } else {
            "x64"
        },
        homedir: home,
        username,
        env,
    }
}

#[derive(Serialize)]
pub(crate) struct HttpResponse {
    status: u16,
    headers: Vec<(String, String)>,
    body: ToJsBuffer,
}

/// The status and headers of the last response in curl's header dump: with
/// redirects followed, there is one block per response.
fn parse_headers(dump: &str) -> (u16, Vec<(String, String)>) {
    let last = dump
        .split("\r\n\r\n")
        .filter(|block| block.starts_with("HTTP/"))
        .last()
        .unwrap_or_default();
    let mut lines = last.lines();
    let status = lines
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|code| code.parse().ok())
        .unwrap_or(0);
    let headers = lines
        .filter_map(|line| {
            let (name, value) = line.split_once(':')?;
            Some((name.trim().to_lowercase(), value.trim().to_owned()))
        })
        .collect();
    (status, headers)
}

fn fetch(
    method: &str,
    url: &str,
    headers: &[(String, String)],
    body: Vec<u8>,
) -> io::Result<HttpResponse> {
    if !(url.starts_with("https://") || url.starts_with("http://")) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("only web addresses can be fetched, not {url}"),
        ));
    }
    let dump = std::env::temp_dir().join(format!(
        "keel-http-{}-{}.txt",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |time| time.as_nanos())
    ));
    let mut command = Command::new("/usr/bin/curl");
    command
        .args([
            "--silent",
            "--show-error",
            "--location",
            "--compressed",
            "--max-time",
            "60",
        ])
        .arg("--request")
        .arg(method)
        .arg("--dump-header")
        .arg(&dump);
    for (name, value) in headers {
        command.arg("--header").arg(format!("{name}: {value}"));
    }
    if !body.is_empty() {
        command.args(["--data-binary", "@-"]);
    }
    command
        .arg("--")
        .arg(url)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn()?;
    if let Some(mut stdin) = child.stdin.take() {
        use std::io::Write as _;
        stdin.write_all(&body)?;
    }
    let output = child.wait_with_output()?;
    let headers_text = fs::read_to_string(&dump).unwrap_or_default();
    // One small file of our own in the temporary folder.
    let _ = fs::remove_file(&dump);
    if !output.status.success() {
        let error = String::from_utf8_lossy(&output.stderr);
        return Err(io::Error::other(
            error.trim().trim_start_matches("curl: ").to_owned(),
        ));
    }
    let (status, headers) = parse_headers(&headers_text);
    Ok(HttpResponse {
        status,
        headers,
        body: output.stdout.into(),
    })
}

/// `http.request` and `https.request`: the whole answer at once.
#[op2]
#[serde]
pub(crate) async fn op_keel_node_http(
    #[string] method: String,
    #[string] url: String,
    #[serde] headers: Vec<(String, String)>,
    #[buffer(copy)] body: Vec<u8>,
) -> Result<HttpResponse, JsErrorBox> {
    let task = tokio::task::spawn_blocking(move || fetch(&method, &url, &headers, body));
    match task.await {
        Ok(Ok(response)) => Ok(response),
        Ok(Err(error)) => Err(JsErrorBox::generic(format!("ECONNREFUSED: {error}"))),
        Err(error) => Err(JsErrorBox::generic(format!("EIO: {error}"))),
    }
}

/// Whether `path` exists, for `fs.existsSync`.
#[op2(fast)]
pub(crate) fn op_keel_node_exists(#[string] path: String) -> bool {
    Path::new(&path).exists()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_last_response_wins_after_redirects() {
        let dump = "HTTP/2 301\r\nlocation: https://x/b\r\n\r\nHTTP/2 200\r\nContent-Type: application/json\r\nETag: \"abc\"\r\n\r\n";
        let (status, headers) = parse_headers(dump);
        assert_eq!(status, 200);
        assert_eq!(
            headers[0],
            ("content-type".to_owned(), "application/json".to_owned())
        );
        assert_eq!(headers[1].1, "\"abc\"");
    }

    #[test]
    fn only_web_addresses_are_fetched() {
        let error = fetch("GET", "file:///etc/passwd", &[], Vec::new())
            .err()
            .unwrap();
        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
    }
}
