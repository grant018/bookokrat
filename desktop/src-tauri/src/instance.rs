//! One reader at a time. Two would truncate each other's log and overwrite
//! each other's bookmarks and session, so the first to start holds a lock in
//! the state folder, and a later start hands the file it was given to that
//! one and exits. Test runs keep their own state folder, so they never meet
//! a reader started by hand.

use serde::Serialize;
use std::fs::{File, OpenOptions, TryLockError};
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Emitter, Manager};

/// What a later start asked the running reader to open.
#[derive(Clone, Serialize)]
pub struct OpenRequest {
    pub path: String,
    pub is_file: bool,
}

/// Held for as long as this reader runs.
pub struct Instance {
    _lock: File,
    pub socket: PathBuf,
}

pub enum Claim {
    /// This is the only reader; `None` when the lock could not be made.
    First(Option<Instance>),
    /// Another reader is running and has been handed the file.
    Second,
}

fn state_dir() -> Option<PathBuf> {
    let log = bookokrat::library::resolve_log_path().ok()?;
    log.parent().map(Path::to_path_buf)
}

/// The socket for the reader using `state_dir`. It goes in the runtime
/// folder, named after the state folder, since a socket's path must be
/// short and a state folder's need not be.
fn socket_path(state_dir: &Path) -> PathBuf {
    let slug = bookokrat::library::library_slug(state_dir);
    let hash = slug.split('_').next().unwrap_or("default");
    let dir = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .filter(|dir| dir.is_dir())
        .unwrap_or_else(std::env::temp_dir);
    dir.join(format!("bookokrat-reader-{hash}.sock"))
}

/// Becomes the running reader, or passes `target` to the one already running.
/// Runs before logging starts, since starting it empties the log.
pub fn claim(target: Option<&Path>) -> Claim {
    let Some(dir) = state_dir() else {
        return Claim::First(None);
    };
    let lock = match OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(dir.join("reader.lock"))
    {
        Ok(file) => file,
        Err(_) => return Claim::First(None),
    };
    let socket = socket_path(&dir);
    match lock.try_lock() {
        Ok(()) => Claim::First(Some(Instance {
            _lock: lock,
            socket,
        })),
        Err(TryLockError::WouldBlock) => {
            hand_over(&socket, target);
            Claim::Second
        }
        Err(TryLockError::Error(_)) => Claim::First(None),
    }
}

#[cfg(unix)]
fn hand_over(socket: &Path, target: Option<&Path>) {
    use std::io::Write;
    if let Ok(mut stream) = std::os::unix::net::UnixStream::connect(socket) {
        let text = target
            .map(|t| t.to_string_lossy().into_owned())
            .unwrap_or_default();
        let _ = stream.write_all(text.as_bytes());
    }
}

#[cfg(not(unix))]
fn hand_over(_socket: &Path, _target: Option<&Path>) {}

/// Takes what later starts hand over: brings the window forward and asks
/// the page to open the file, if one was named.
#[cfg(unix)]
pub fn listen(socket: PathBuf, app: AppHandle) {
    use std::io::Read;
    use std::os::unix::net::UnixListener;

    // Left behind by a reader that did not exit cleanly; the lock says no
    // other reader is using it.
    let _ = std::fs::remove_file(&socket);
    let listener = match UnixListener::bind(&socket) {
        Ok(listener) => listener,
        Err(e) => {
            log::error!("Failed to listen on {socket:?}: {e}");
            return;
        }
    };
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let mut stream = match stream {
                Ok(stream) => stream,
                Err(e) => {
                    log::error!("Failed to accept a later start of the reader: {e}");
                    continue;
                }
            };
            let _ = stream.set_read_timeout(Some(std::time::Duration::from_secs(2)));
            let mut text = String::new();
            if let Err(e) = stream.read_to_string(&mut text) {
                log::error!("Failed to read what a later start asked for: {e}");
                continue;
            }
            if let Some(window) = app.webview_windows().into_values().next() {
                let _ = window.unminimize();
                let _ = window.show();
                if let Err(e) = window.set_focus() {
                    log::error!("Failed to bring the reader forward: {e}");
                }
            }
            if text.is_empty() {
                continue;
            }
            log::info!("A later start asked to open {text}");
            let request = OpenRequest {
                is_file: Path::new(&text).is_file(),
                path: text,
            };
            if let Err(e) = app.emit("open-request", request) {
                log::error!("Failed to pass an open request to the page: {e}");
            }
        }
    });
}

#[cfg(not(unix))]
pub fn listen(_socket: PathBuf, _app: AppHandle) {}
