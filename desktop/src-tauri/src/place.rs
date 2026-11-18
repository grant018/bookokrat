//! Where the reader left off. The place in each book goes into the terminal
//! app's bookmarks, so either app picks up where the other stopped. Beside
//! that, this module keeps what only the desktop reader needs: how far into
//! a block the view started, and whether a book was open when the app closed.

use bookokrat::library;
use bookokrat::marks::{GlobalMarks, MarkLocation};
use chrono::{SecondsFormat, Utc};
use epub::doc::EpubDoc;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use std::collections::HashMap;
use std::fs;
use std::io::{Read, Seek};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

const SESSION_FILE: &str = "reader_session.json";

/// Held across each read-change-write of the session file: places and the
/// open book are recorded by separate commands that can run at once.
static SESSION: Mutex<()> = Mutex::new(());

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Place {
    pub chapter: usize,
    /// The chapter block at the top of the view, as the terminal app counts them.
    pub node: usize,
    /// How far down that block the view starts, as a fraction of its height.
    #[serde(default)]
    pub offset: f32,
}

/// What a book says about itself, written into its bookmark the way the
/// terminal app does, so its reading history can name the book.
pub struct BookFacts<'a> {
    pub path: &'a str,
    pub title: &'a str,
    pub author: Option<&'a str>,
    pub chapter_count: usize,
}

#[derive(Default, Serialize, Deserialize)]
#[serde(default)]
struct Session {
    /// The book that was open when the app was last used, if one was.
    #[serde(skip_serializing_if = "Option::is_none")]
    reading: Option<String>,
    /// The library folder last shown, for a start that names none.
    #[serde(skip_serializing_if = "Option::is_none")]
    library: Option<String>,
    places: HashMap<String, Place>,
}

fn session_path() -> Result<PathBuf, String> {
    let libraries = library::libraries_data_dir().map_err(|e| e.to_string())?;
    let dir = libraries
        .parent()
        .ok_or("The data directory has no parent")?;
    Ok(dir.join(SESSION_FILE))
}

fn load_session() -> Session {
    let Ok(path) = session_path() else {
        return Session::default();
    };
    if !path.exists() {
        return Session::default();
    }
    fs::read_to_string(&path)
        .map_err(|e| e.to_string())
        .and_then(|text| serde_json::from_str(&text).map_err(|e| e.to_string()))
        .unwrap_or_else(|e| {
            log::error!("Failed to load {path:?}: {e}");
            Session::default()
        })
}

fn save_session(session: &Session) -> Result<(), String> {
    let path = session_path()?;
    let text = serde_json::to_string_pretty(session).map_err(|e| e.to_string())?;
    write_whole(&path, &text)
}

/// Writes beside the file and moves the result into place, so a crash or a
/// second writer never leaves half a file.
fn write_whole(path: &Path, text: &str) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(|e| format!("Failed to create {dir:?}: {e}"))?;
    }
    let partial = path.with_extension(format!("{}.part", std::process::id()));
    fs::write(&partial, text).map_err(|e| format!("Failed to write {partial:?}: {e}"))?;
    fs::rename(&partial, path).map_err(|e| format!("Failed to replace {path:?}: {e}"))
}

/// The book to reopen when the app starts with no file named, if the last
/// session ended inside one that still exists.
pub fn resume_target() -> Option<String> {
    load_session()
        .reading
        .filter(|path| Path::new(path).is_file())
}

/// The library folder of the last session, if it is still there.
pub fn last_library() -> Option<PathBuf> {
    load_session()
        .library
        .map(PathBuf::from)
        .filter(|dir| dir.is_dir())
}

pub fn set_library(dir: &Path) {
    let _held = SESSION.lock().unwrap_or_else(|e| e.into_inner());
    let mut session = load_session();
    let dir = dir.to_string_lossy();
    if session.library.as_deref() == Some(&*dir) {
        return;
    }
    session.library = Some(dir.into_owned());
    if let Err(e) = save_session(&session) {
        log::error!("Failed to record the library folder: {e}");
    }
}

/// Records whether a book is being read, for the next start.
pub fn set_reading(book: Option<&str>) {
    let _held = SESSION.lock().unwrap_or_else(|e| e.into_inner());
    let mut session = load_session();
    if session.reading.as_deref() == book {
        return;
    }
    session.reading = book.map(str::to_string);
    if let Err(e) = save_session(&session) {
        log::error!("Failed to record the open book: {e}");
    }
}

/// The terminal app keeps one bookmarks file per library, the folder it was
/// started in. A book under the reader's library folder belongs to that
/// library; any other book to the folder it sits in.
fn library_of(library_dir: &Path, book: &Path) -> Result<(library::LibraryPaths, PathBuf), String> {
    let dir = if book.starts_with(library_dir) {
        library_dir
    } else {
        book.parent().unwrap_or(library_dir)
    };
    let paths = library::resolve_library_paths(dir).map_err(|e| format!("{e:#}"))?;
    Ok((paths, dir.to_path_buf()))
}

fn bookmarks_file(library_dir: &Path, book: &Path) -> Result<(PathBuf, PathBuf), String> {
    let (paths, dir) = library_of(library_dir, book)?;
    Ok((paths.bookmarks_file, dir))
}

/// Where the terminal app keeps the notes and highlights for a book's library.
pub fn comments_dir(library_dir: &Path, book: &str) -> Result<PathBuf, String> {
    Ok(library_of(library_dir, Path::new(book))?.0.comments_dir)
}

/// The book's own marks, `a` to `z`, which live in its bookmark.
pub fn local_marks(library_dir: &Path, book: &str) -> Vec<(char, MarkLocation)> {
    let read = || -> Result<Vec<(char, MarkLocation)>, String> {
        let (file, library) = bookmarks_file(library_dir, Path::new(book))?;
        let bookmarks = load_bookmarks(&file)?;
        let Some(books) = bookmarks.get("books").and_then(Value::as_object) else {
            return Ok(Vec::new());
        };
        let marks = bookmark_key(books, &library, book)
            .and_then(|key| books.get(&key))
            .and_then(|mark| mark.get("marks"))
            .cloned()
            .unwrap_or(Value::Null);
        if marks.is_null() {
            return Ok(Vec::new());
        }
        serde_json::from_value::<HashMap<char, MarkLocation>>(marks)
            .map(|marks| marks.into_iter().collect())
            .map_err(|e| format!("Failed to read the marks of {book}: {e}"))
    };
    read().unwrap_or_else(|e| {
        log::error!("{e}");
        Vec::new()
    })
}

/// Sets one of the book's own marks, or removes it when given no place.
/// The book's bookmark must exist already; `remember` makes it.
pub fn set_local_mark(
    library_dir: &Path,
    book: &str,
    letter: char,
    place: Option<&MarkLocation>,
) -> Result<(), String> {
    let (file, library) = bookmarks_file(library_dir, Path::new(book))?;
    let mut bookmarks = load_bookmarks(&file)?;
    let books = bookmarks
        .get_mut("books")
        .and_then(Value::as_object_mut)
        .ok_or_else(|| format!("{file:?} is not a bookmarks file"))?;
    let key = bookmark_key(books, &library, book)
        .ok_or_else(|| format!("{book} has no bookmark to keep a mark in"))?;
    let fields = books
        .get_mut(&key)
        .and_then(Value::as_object_mut)
        .ok_or_else(|| format!("The bookmark for {book} is not an object"))?;
    let marks = fields.entry("marks").or_insert_with(|| json!({}));
    if !marks.is_object() {
        *marks = json!({});
    }
    let Some(marks) = marks.as_object_mut() else {
        return Err(format!("The marks of {book} could not be changed"));
    };
    match place {
        Some(place) => {
            let value = serde_json::to_value(place).map_err(|e| e.to_string())?;
            marks.insert(letter.to_string(), value);
        }
        None => {
            marks.remove(&letter.to_string());
        }
    }
    if marks.is_empty() {
        fields.remove("marks");
    }
    let text = serde_json::to_string_pretty(&bookmarks).map_err(|e| e.to_string())?;
    write_whole(&file, &text)
}

fn global_marks_store() -> Result<GlobalMarks, String> {
    library::global_marks_file()
        .map(GlobalMarks::load)
        .map_err(|e| format!("{e:#}"))
}

/// The marks shared by every library, `A` to `Z`.
pub fn global_marks() -> Vec<(char, MarkLocation)> {
    let read = || -> Result<Vec<(char, MarkLocation)>, String> {
        let store = global_marks_store()?;
        Ok(('A'..='Z')
            .filter_map(|letter| Some((letter, store.get(letter)?.clone())))
            .collect())
    };
    read().unwrap_or_else(|e| {
        log::error!("Failed to read the global marks: {e}");
        Vec::new()
    })
}

pub fn set_global_mark(letter: char, place: Option<MarkLocation>) -> Result<(), String> {
    let mut store = global_marks_store()?;
    match place {
        Some(place) => store.set(letter, place),
        None => {
            store.remove(letter);
        }
    }
    Ok(())
}

/// The bookmarks file is read and written as plain JSON rather than through
/// the core's types: a build without PDF support would otherwise drop the
/// PDF settings other books in the file carry.
fn load_bookmarks(file: &Path) -> Result<Value, String> {
    if !file.exists() {
        return Ok(json!({ "books": {} }));
    }
    let text = fs::read_to_string(file).map_err(|e| format!("Failed to read {file:?}: {e}"))?;
    let value: Value =
        serde_json::from_str(&text).map_err(|e| format!("Failed to parse {file:?}: {e}"))?;
    if !value.get("books").is_some_and(Value::is_object) {
        return Err(format!("{file:?} is not a bookmarks file"));
    }
    Ok(value)
}

/// The key a book is filed under. The terminal app names books relative to
/// its library folder, with or without a leading `./`, or by their full path.
fn bookmark_key(books: &Map<String, Value>, library: &Path, book: &str) -> Option<String> {
    let relative = Path::new(book)
        .strip_prefix(library)
        .ok()
        .map(|p| p.to_string_lossy().into_owned());
    let mut names = vec![book.to_string()];
    if let Some(relative) = relative {
        names.push(format!("./{relative}"));
        names.push(relative);
    }
    names
        .into_iter()
        .find(|name| books.contains_key(name))
        .or_else(|| {
            books
                .iter()
                .find(|(_, mark)| mark.get("absolute_path").and_then(Value::as_str) == Some(book))
                .map(|(key, _)| key.clone())
        })
}

fn chapter_href<R: Read + Seek>(doc: &EpubDoc<R>, chapter: usize) -> String {
    doc.spine
        .get(chapter)
        .and_then(|item| doc.resources.get(&item.idref))
        .map(|resource| resource.path.to_string_lossy().into_owned())
        .unwrap_or_else(|| format!("chapter_{chapter}"))
}

/// Matches a bookmark's chapter the way the terminal app does: by the
/// chapter's file, falling back to its position in the book.
fn chapter_of<R: Read + Seek>(doc: &EpubDoc<R>, mark: &Value) -> Option<usize> {
    let by_href = mark
        .get("chapter_href")
        .and_then(Value::as_str)
        .and_then(|href| {
            doc.spine.iter().position(|item| {
                doc.resources.get(&item.idref).is_some_and(|resource| {
                    let path = resource.path.to_string_lossy();
                    path == href || path.contains(href) || href.contains(&*path)
                })
            })
        });
    let by_index = || {
        mark.get("chapter_index")
            .and_then(Value::as_u64)
            .map(|index| index as usize)
    };
    by_href
        .or_else(by_index)
        .filter(|index| *index < doc.get_num_chapters())
}

/// Where the book was left, or `None` if it has not been read before.
pub fn restore<R: Read + Seek>(library_dir: &Path, book: &str, doc: &EpubDoc<R>) -> Option<Place> {
    let (file, library) = bookmarks_file(library_dir, Path::new(book))
        .inspect_err(|e| log::error!("No bookmarks for {book}: {e}"))
        .ok()?;
    let bookmarks = load_bookmarks(&file)
        .inspect_err(|e| log::error!("{e}"))
        .ok()?;
    let books = bookmarks.get("books")?.as_object()?;
    let mark = books.get(&bookmark_key(books, &library, book)?)?;

    let chapter = chapter_of(doc, mark)?;
    let node = mark
        .get("node_index")
        .and_then(Value::as_u64)
        .map_or(0, |n| n as usize);
    // The finer position is only good while the bookmark still points at the
    // block it was measured in; the terminal app may have moved on since.
    let offset = load_session()
        .places
        .get(book)
        .filter(|place| place.chapter == chapter && place.node == node)
        .map_or(0.0, |place| place.offset);
    Some(Place {
        chapter,
        node,
        offset,
    })
}

/// Records the place in the open book, in the shared bookmarks and in the
/// reader's own session file.
pub fn remember<R: Read + Seek>(
    library_dir: &Path,
    facts: &BookFacts,
    doc: &EpubDoc<R>,
    place: Place,
    progress: f32,
) -> Result<(), String> {
    let (file, library) = bookmarks_file(library_dir, Path::new(facts.path))?;
    let mut bookmarks = load_bookmarks(&file)?;
    let books = bookmarks
        .get_mut("books")
        .and_then(Value::as_object_mut)
        .ok_or_else(|| format!("{file:?} is not a bookmarks file"))?;
    let key = bookmark_key(books, &library, facts.path).unwrap_or_else(|| facts.path.to_string());

    let mark = books.entry(key).or_insert_with(|| json!({}));
    let fields = mark
        .as_object_mut()
        .ok_or_else(|| format!("The bookmark for {} is not an object", facts.path))?;
    let now = Utc::now().to_rfc3339_opts(SecondsFormat::AutoSi, true);
    fields.insert(
        "chapter_href".into(),
        json!(chapter_href(doc, place.chapter)),
    );
    fields.insert("node_index".into(), json!(place.node));
    fields.insert("last_read".into(), json!(now));
    fields.insert("chapter_index".into(), json!(place.chapter));
    fields.insert("total_chapters".into(), json!(facts.chapter_count));
    fields.insert("book_progress".into(), json!(progress.clamp(0.0, 1.0)));
    // The terminal app counts every block in the book for this; a count
    // left over from it would no longer match the progress written here.
    fields.remove("total_nodes");
    fields.insert("book_title".into(), json!(facts.title));
    if let Some(author) = facts.author {
        fields.insert("book_author".into(), json!(author));
    }
    fields.insert("absolute_path".into(), json!(facts.path));

    let text = serde_json::to_string_pretty(&bookmarks).map_err(|e| e.to_string())?;
    write_whole(&file, &text)?;

    let _held = SESSION.lock().unwrap_or_else(|e| e.into_inner());
    let mut session = load_session();
    session.places.insert(facts.path.to_string(), place);
    save_session(&session)
}

/// Where a PDF or DjVu document was left, and how its pages were coloured.
#[derive(Debug, Default)]
pub struct PagedPlace {
    pub page: usize,
    /// How far down the page the view started, as a fraction of its height.
    pub offset: f32,
    pub themed: Option<bool>,
    pub tint_images: Option<bool>,
}

/// The name the terminal app files a book under, if it has a bookmark.
/// Its notes on a PDF are filed under the same name.
pub fn book_key(library_dir: &Path, book: &str) -> Option<String> {
    let (file, library) = bookmarks_file(library_dir, Path::new(book)).ok()?;
    let bookmarks = load_bookmarks(&file)
        .inspect_err(|e| log::error!("{e}"))
        .ok()?;
    bookmark_key(bookmarks.get("books")?.as_object()?, &library, book)
}

pub fn restore_paged(library_dir: &Path, book: &str) -> PagedPlace {
    let read = || -> Option<PagedPlace> {
        let (file, library) = bookmarks_file(library_dir, Path::new(book))
            .inspect_err(|e| log::error!("No bookmarks for {book}: {e}"))
            .ok()?;
        let bookmarks = load_bookmarks(&file)
            .inspect_err(|e| log::error!("{e}"))
            .ok()?;
        let books = bookmarks.get("books")?.as_object()?;
        let mark = books.get(&bookmark_key(books, &library, book)?)?;
        let number = |name: &str| mark.get(name).and_then(Value::as_u64).map(|n| n as usize);
        let page = number("pdf_page")
            .or_else(|| number("chapter_index"))
            .or_else(|| {
                mark.get("chapter_href")
                    .and_then(Value::as_str)
                    .and_then(|href| href.parse().ok())
            })
            .unwrap_or(0);
        let offset = load_session()
            .places
            .get(book)
            .filter(|place| place.chapter == page)
            .map_or(0.0, |place| place.offset);
        Some(PagedPlace {
            page,
            offset,
            themed: mark.get("pdf_themed_rendering").and_then(Value::as_bool),
            tint_images: mark.get("pdf_invert_images").and_then(Value::as_bool),
        })
    };
    read().unwrap_or_default()
}

/// Records the page a PDF or DjVu document is open at, the way the
/// terminal app does. Its zoom and pan there are left as they are: they
/// are measured in terminal cells and mean nothing here.
pub fn remember_paged(
    library_dir: &Path,
    facts: &BookFacts,
    place: &PagedPlace,
) -> Result<(), String> {
    let (file, library) = bookmarks_file(library_dir, Path::new(facts.path))?;
    let mut bookmarks = load_bookmarks(&file)?;
    let books = bookmarks
        .get_mut("books")
        .and_then(Value::as_object_mut)
        .ok_or_else(|| format!("{file:?} is not a bookmarks file"))?;
    let key = bookmark_key(books, &library, facts.path).unwrap_or_else(|| facts.path.to_string());

    let mark = books.entry(key).or_insert_with(|| json!({}));
    let fields = mark
        .as_object_mut()
        .ok_or_else(|| format!("The bookmark for {} is not an object", facts.path))?;
    let now = Utc::now().to_rfc3339_opts(SecondsFormat::AutoSi, true);
    let pages = facts.chapter_count.max(1);
    fields.insert("chapter_href".into(), json!(place.page.to_string()));
    fields.insert("last_read".into(), json!(now));
    fields.insert("total_chapters".into(), json!(pages));
    fields.insert("pdf_page".into(), json!(place.page));
    fields.insert(
        "book_progress".into(),
        json!(((place.page + 1) as f32 / pages as f32).clamp(0.0, 1.0)),
    );
    if let Some(themed) = place.themed {
        fields.insert("pdf_themed_rendering".into(), json!(themed));
    }
    if let Some(tint) = place.tint_images {
        fields.insert("pdf_invert_images".into(), json!(tint));
    }
    fields.insert("book_title".into(), json!(facts.title));
    if let Some(author) = facts.author {
        fields.insert("book_author".into(), json!(author));
    }
    fields.insert("absolute_path".into(), json!(facts.path));

    let text = serde_json::to_string_pretty(&bookmarks).map_err(|e| e.to_string())?;
    write_whole(&file, &text)?;

    let _held = SESSION.lock().unwrap_or_else(|e| e.into_inner());
    let mut session = load_session();
    session.places.insert(
        facts.path.to_string(),
        Place {
            chapter: place.page,
            node: 0,
            offset: place.offset,
        },
    );
    save_session(&session)
}
