//! Reading history: every book the terminal app or this reader has a
//! bookmark for, in any library, most recently read first.

use bookokrat::library;
use serde::Serialize;
use serde_json::Value;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Serialize)]
pub struct Entry {
    pub path: String,
    pub title: String,
    pub author: Option<String>,
    /// How far through the book, from 0 to 1, when it was recorded.
    pub progress: Option<f32>,
    /// When it was last read, as RFC 3339.
    pub last_read: String,
    /// False for a book that has since been moved or deleted.
    pub exists: bool,
    /// Whether the book is in the library on screen.
    pub here: bool,
    /// The bookmarks file the entry came from and its key there, to forget it by.
    pub source: String,
    pub key: String,
}

fn title_from_path(path: &str) -> String {
    Path::new(path)
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string())
}

fn bookmark_files() -> Vec<(PathBuf, PathBuf)> {
    let Ok(libraries) = library::libraries_data_dir() else {
        return Vec::new();
    };
    let Ok(entries) = fs::read_dir(&libraries) else {
        return Vec::new();
    };
    entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|dir| dir.is_dir())
        .map(|dir| (dir.join("bookmarks.json"), dir))
        .filter(|(file, _)| file.is_file())
        .collect()
}

/// The folder a library's bookmarks belong to. Its data folder is named
/// after it but cannot be turned back into it, so this tries the folders
/// its books with a full path sit in, and the folders above those.
fn library_folder(data_dir: &Path, books: &serde_json::Map<String, Value>) -> Option<PathBuf> {
    let slug = data_dir.file_name()?.to_str()?;
    let mut tried = std::collections::HashSet::new();
    books
        .values()
        .filter_map(|mark| mark.get("absolute_path").and_then(Value::as_str))
        .flat_map(|path| Path::new(path).ancestors().skip(1))
        .filter(|dir| tried.insert(dir.to_path_buf()))
        .find(|dir| library::library_slug(dir) == slug)
        .map(Path::to_path_buf)
}

pub fn list(library_dir: &Path) -> Vec<Entry> {
    let own = library::resolve_library_paths(library_dir)
        .map(|paths| paths.bookmarks_file)
        .ok();
    let mut latest: HashMap<String, Entry> = HashMap::new();
    for (file, data_dir) in bookmark_files() {
        let books = fs::read_to_string(&file)
            .map_err(|e| e.to_string())
            .and_then(|text| serde_json::from_str::<Value>(&text).map_err(|e| e.to_string()));
        let books = match books {
            Ok(value) => value,
            Err(e) => {
                log::error!("Failed to read {file:?}: {e}");
                continue;
            }
        };
        let Some(books) = books.get("books").and_then(Value::as_object) else {
            continue;
        };
        let here = own.as_deref() == Some(file.as_path());
        let folder = if here {
            Some(library_dir.to_path_buf())
        } else {
            library_folder(&data_dir, books)
        };
        for (key, mark) in books {
            let Some(last_read) = mark.get("last_read").and_then(Value::as_str) else {
                continue;
            };
            let text = |name: &str| mark.get(name).and_then(Value::as_str).map(str::to_string);
            // A book filed by a relative name belongs to its library's folder.
            let path = text("absolute_path").unwrap_or_else(|| match &folder {
                Some(folder) if !Path::new(key).is_absolute() => folder
                    .join(key.strip_prefix("./").unwrap_or(key))
                    .to_string_lossy()
                    .into_owned(),
                _ => key.clone(),
            });
            if latest
                .get(&path)
                .is_some_and(|seen| seen.last_read.as_str() >= last_read)
            {
                continue;
            }
            latest.insert(
                path.clone(),
                Entry {
                    title: text("book_title").unwrap_or_else(|| title_from_path(key)),
                    author: text("book_author"),
                    progress: mark
                        .get("book_progress")
                        .and_then(Value::as_f64)
                        .map(|p| p as f32),
                    last_read: last_read.to_string(),
                    exists: Path::new(&path).is_absolute() && Path::new(&path).is_file(),
                    here,
                    source: file.to_string_lossy().into_owned(),
                    key: key.clone(),
                    path,
                },
            );
        }
    }
    let mut entries: Vec<Entry> = latest.into_values().collect();
    entries.sort_by(|a, b| b.last_read.cmp(&a.last_read));
    entries
}

/// Removes a book's bookmark, and with it its place and its own marks.
pub fn forget(source: &str, key: &str) -> Result<(), String> {
    let file = Path::new(source);
    let known = bookmark_files().iter().any(|(known, _)| known == file);
    if !known {
        return Err(format!("{source} is not a bookmarks file"));
    }
    crate::place::change_bookmarks(file, |books| match books.remove(key) {
        Some(_) => Ok(()),
        None => Err(format!("{key} has no bookmark in {file:?}")),
    })
}
