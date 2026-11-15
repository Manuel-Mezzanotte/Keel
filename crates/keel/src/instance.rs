//! One Keel at a time: `keel PATH` hands PATH to the Keel already running
//! and quits, so every project opens as a window of the same app, as with
//! `code .` and VS Code.
//!
//! The running Keel listens on a Unix socket in its data folder. Each
//! request is one line of JSON, `{"open": "/abs/path"}` or `{"open": null}`
//! for an empty window, answered with `ok`.

use std::io::{self, BufRead as _, BufReader, Write as _};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::time::Duration;

use futures::channel::mpsc::UnboundedSender;
use serde_json::{Value, json};

/// What another `keel` asked the running one to open; `None` is an empty
/// window.
pub type OpenRequest = Option<PathBuf>;

/// The socket in the data folder. macOS limits socket paths to about 100
/// bytes, so a data folder deep in the disk gets a short path in the user's
/// temporary folder instead, named after the data folder.
pub fn socket_path(data: &Path) -> PathBuf {
    let socket = data.join("keel.sock");
    if socket.as_os_str().len() < SOCKET_PATH_LIMIT {
        return socket;
    }
    let mut hasher = std::hash::DefaultHasher::new();
    std::hash::Hash::hash(data, &mut hasher);
    let name = format!("keel-{:016x}.sock", std::hash::Hasher::finish(&hasher));
    std::env::temp_dir().join(name)
}

/// The longest socket path macOS takes, with a little room to spare.
const SOCKET_PATH_LIMIT: usize = 100;

/// Hands a path to the Keel already running. Returns `false` when no Keel
/// answers, so this one should start.
pub fn forward(socket: &Path, path: Option<&Path>) -> bool {
    let Ok(mut stream) = UnixStream::connect(socket) else {
        return false;
    };
    let timeout = Some(Duration::from_secs(3));
    if stream.set_read_timeout(timeout).is_err() || stream.set_write_timeout(timeout).is_err() {
        return false;
    }
    let request = json!({ "open": path.map(|path| path.to_string_lossy().into_owned()) });
    if writeln!(stream, "{request}").is_err() {
        return false;
    }
    let mut reply = String::new();
    BufReader::new(stream).read_line(&mut reply).is_ok() && reply.trim() == "ok"
}

/// Starts answering other `keel` commands. Requests arrive on `requests`.
pub fn listen(socket: &Path, requests: UnboundedSender<OpenRequest>) -> io::Result<()> {
    if let Some(folder) = socket.parent() {
        std::fs::create_dir_all(folder)?;
    }
    // A socket left behind by a Keel that crashed: nobody answers it, so it
    // is replaced. A socket that answers belongs to another running Keel.
    if socket.exists() {
        if UnixStream::connect(socket).is_ok() {
            return Err(io::Error::new(
                io::ErrorKind::AddrInUse,
                "another Keel is already running",
            ));
        }
        std::fs::remove_file(socket)?;
    }
    let listener = UnixListener::bind(socket)?;
    std::thread::Builder::new()
        .name("keel-instance".to_owned())
        .spawn(move || {
            for stream in listener.incoming().flatten() {
                answer(stream, &requests);
            }
        })?;
    Ok(())
}

fn answer(stream: UnixStream, requests: &UnboundedSender<OpenRequest>) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(3)));
    let mut reader = BufReader::new(&stream);
    let mut line = String::new();
    if reader.read_line(&mut line).is_err() {
        return;
    }
    let Ok(request) = serde_json::from_str::<Value>(&line) else {
        return;
    };
    let path = request["open"].as_str().map(PathBuf::from);
    if requests.unbounded_send(path).is_ok() {
        let _ = writeln!(&stream, "ok");
    }
}

#[cfg(test)]
mod tests {
    use futures::StreamExt as _;
    use futures::channel::mpsc;

    use super::*;

    #[test]
    fn a_second_keel_hands_its_path_to_the_first() {
        let data = tempfile::tempdir().unwrap();
        let socket = socket_path(data.path());
        assert!(!forward(&socket, None));

        let (sender, mut receiver) = mpsc::unbounded();
        listen(&socket, sender).unwrap();
        assert!(forward(&socket, Some(Path::new("/projects/Keel"))));
        assert!(forward(&socket, None));

        let requests: Vec<OpenRequest> = futures::executor::block_on(async {
            vec![receiver.next().await, receiver.next().await]
        })
        .into_iter()
        .flatten()
        .collect();
        assert_eq!(requests, [Some(PathBuf::from("/projects/Keel")), None]);

        let (other, _) = mpsc::unbounded();
        assert!(
            listen(&socket, other).is_err(),
            "a running Keel keeps its socket"
        );
    }

    #[test]
    fn deep_data_folders_get_a_short_socket() {
        let deep = Path::new("/private/tmp").join("a-rather-long-folder-name".repeat(5));
        let socket = socket_path(&deep);
        assert!(
            socket.as_os_str().len() < SOCKET_PATH_LIMIT,
            "{}",
            socket.display()
        );
        assert_eq!(socket, socket_path(&deep));
        assert_ne!(socket, socket_path(&deep.join("other")));
        let home = Path::new(
            "/Users/manuelmezzanotte/Library/Application Support/com.manuelmezzanotte.keel",
        );
        assert_eq!(socket_path(home), home.join("keel.sock"));
    }

    #[test]
    fn a_socket_left_by_a_crash_is_replaced() {
        let data = tempfile::tempdir().unwrap();
        let socket = socket_path(data.path());
        drop(UnixListener::bind(&socket).unwrap());
        assert!(socket.exists());
        assert!(!forward(&socket, None));

        let (sender, _receiver) = mpsc::unbounded();
        listen(&socket, sender).unwrap();
        assert!(forward(&socket, None));
    }
}
