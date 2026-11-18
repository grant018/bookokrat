//! One reader at a time. Two would truncate each other's log and overwrite
//! each other's bookmarks and session, so the first to start holds a lock in
//! the state folder, and a later start hands the file it was given to that
//! one and exits. Test runs keep their own state folder, so they never meet
//! a reader started by hand.
//!
//! macOS starts no second copy: it asks the running app to open the file
//! (a book opened from the Finder), which arrives here the same way.

use serde::Serialize;
use std::fs::{File, OpenOptions, TryLockError};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use tauri::{AppHandle, Emitter, Manager};

/// What a later start, or the system, asked the running reader to open.
#[derive(Clone, Serialize)]
pub struct OpenRequest {
    pub path: String,
    pub is_file: bool,
}

/// The latest request not yet taken by the page. It is kept rather than
/// only announced, since one can come before the page is listening: macOS
/// sends the file a book was opened with just after the app starts.
static PENDING: Mutex<Option<OpenRequest>> = Mutex::new(None);

/// The request waiting for the page, if any; it is taken only once.
pub fn take() -> Option<OpenRequest> {
    PENDING.lock().unwrap_or_else(|e| e.into_inner()).take()
}

/// Brings the window forward and, given a path, keeps it for the page and
/// tells the page to take it.
pub fn deliver(app: &AppHandle, path: Option<PathBuf>) {
    if let Some(window) = app.webview_windows().into_values().next() {
        let _ = window.unminimize();
        let _ = window.show();
        if let Err(e) = window.set_focus() {
            log::error!("Failed to bring the reader forward: {e}");
        }
    }
    let Some(path) = path else {
        return;
    };
    log::info!("Asked to open {path:?}");
    let request = OpenRequest {
        is_file: path.is_file(),
        path: path.to_string_lossy().into_owned(),
    };
    *PENDING.lock().unwrap_or_else(|e| e.into_inner()) = Some(request);
    if let Err(e) = app.emit("open-request", ()) {
        log::error!("Failed to pass an open request to the page: {e}");
    }
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
            deliver(&app, (!text.is_empty()).then(|| PathBuf::from(text)));
        }
    });
}

#[cfg(not(unix))]
pub fn listen(_socket: PathBuf, _app: AppHandle) {}
