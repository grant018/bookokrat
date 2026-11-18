#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod dto;
#[cfg(target_os = "linux")]
mod harness;
mod history;
mod instance;
mod lookup;
mod notes;
mod pdf;
mod place;
mod prefs;
mod search;

use bookokrat::annotations::HighlightColor;
use bookokrat::book_manager::{BookFormat, BookManager};
use bookokrat::keybindings::context::KeyContext;
use bookokrat::keybindings::notation::KeyInput;
use bookokrat::markdown::{Block, Document};
use bookokrat::marks::MarkLocation;
use bookokrat::parsing::html_to_markdown::HtmlToMarkdownConverter;
use bookokrat::parsing::toc_parser::TocParser;
use bookokrat::settings::{RuntimeSettings, Settings};
use bookokrat::system_command::{RealSystemCommandExecutor, SystemCommandExecutor};
use bookokrat::table_of_contents::TocItem;
use crossterm::event::{KeyCode, KeyModifiers};
use epub::doc::EpubDoc;
use image::codecs::jpeg::JpegEncoder;
use image::imageops::FilterType;
use serde::{Deserialize, Serialize};
use std::collections::hash_map::DefaultHasher;
use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::hash::{Hash, Hasher};
use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use tauri::{AppHandle, Emitter, Manager, State};

type Epub = EpubDoc<BufReader<File>>;

/// The library shows covers small, so each is scaled down once and kept.
/// These bounds leave room for a high-density screen.
const COVER_MAX_WIDTH: u32 = 480;
const COVER_MAX_HEIGHT: u32 = 720;
const COVER_JPEG_QUALITY: u8 = 88;
/// How long a closing window waits for the page to save what is pending.
const CLOSE_GRACE: std::time::Duration = std::time::Duration::from_millis(1500);

/// A name for one opening of a book, never reused, not even by a later run
/// of the app. It leads the URLs of the book's images and pages, so the web
/// view's cache cannot answer for one book with another's pictures.
fn new_opening() -> String {
    static LAUNCHED: std::sync::LazyLock<u128> = std::sync::LazyLock::new(|| {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|since| since.as_nanos())
            .unwrap_or_default()
    });
    static OPENED: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let count = OPENED.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    format!("{:x}-{count}", *LAUNCHED)
}

/// A book is a run of files (its spine), and publishers often split one
/// chapter over several: the text up to a full-page map, the map, the text
/// after it. The reader shows each chapter whole, so its chapters are groups
/// of files. The terminal app and the bookmarks it shares count by file, so
/// places are translated on the way in and out.
struct OpenBook {
    path: String,
    opening: String,
    title: String,
    author: Option<String>,
    doc: Epub,
    /// The files of each chapter, as spine positions.
    chapters: Vec<Vec<usize>>,
    /// For each spine position, its chapter and where it comes in it.
    chapter_of: Vec<(usize, usize)>,
    chapter_titles: HashMap<usize, String>,
    chapter_paths: HashMap<PathBuf, (usize, usize)>,
    /// The block each file of a chapter starts at, once counted.
    part_starts: HashMap<usize, Vec<usize>>,
    /// The whole book's text, read the first time it is searched.
    engine: Option<search::Index>,
    /// Each chapter's title and number of words, counted along with `engine`.
    words: Option<Vec<(usize, String, usize)>>,
}

impl OpenBook {
    /// A copy of the book that reads from `doc`, a second opening of the
    /// same file, so that long reading can go on outside the lock.
    fn twin(&self, doc: Epub) -> OpenBook {
        OpenBook {
            path: self.path.clone(),
            opening: self.opening.clone(),
            title: self.title.clone(),
            author: self.author.clone(),
            doc,
            chapters: self.chapters.clone(),
            chapter_of: self.chapter_of.clone(),
            chapter_titles: self.chapter_titles.clone(),
            chapter_paths: self.chapter_paths.clone(),
            part_starts: self.part_starts.clone(),
            engine: None,
            words: None,
        }
    }

    fn document(&mut self, spine_index: usize) -> Result<(Option<PathBuf>, Document), String> {
        if !self.doc.set_current_chapter(spine_index) {
            return Err(format!("The book has no file {spine_index}"));
        }
        let path = self.doc.get_current_path();
        let (html, _mime) = self.doc.get_current_str().unwrap_or_default();
        Ok((
            path,
            HtmlToMarkdownConverter::keeping_scripts().convert(&html),
        ))
    }

    fn part_starts(&mut self, chapter: usize) -> Result<Vec<usize>, String> {
        if let Some(starts) = self.part_starts.get(&chapter) {
            return Ok(starts.clone());
        }
        let parts = self
            .chapters
            .get(chapter)
            .cloned()
            .ok_or_else(|| format!("Chapter {chapter} is out of range"))?;
        let mut starts = Vec::with_capacity(parts.len());
        let mut blocks = 0;
        for part in parts {
            starts.push(blocks);
            blocks += self.document(part)?.1.blocks.len();
        }
        self.part_starts.insert(chapter, starts.clone());
        Ok(starts)
    }

    /// A place counted by the reader's chapters, as the file and block the
    /// terminal app would record.
    fn place_by_file(&mut self, place: place::Place) -> Result<place::Place, String> {
        let starts = self.part_starts(place.chapter)?;
        let part = starts
            .iter()
            .rposition(|start| *start <= place.node)
            .unwrap_or(0);
        Ok(place::Place {
            chapter: self.chapters[place.chapter][part],
            node: place.node - starts[part],
            offset: place.offset,
        })
    }

    /// The spine position of one of the book's files, from its path in the book.
    fn file_of(&self, href: &str) -> Option<usize> {
        self.doc.spine.iter().position(|item| {
            self.doc.resources.get(&item.idref).is_some_and(|resource| {
                let path = resource.path.to_string_lossy();
                path == href || path.contains(href) || href.contains(&*path)
            })
        })
    }

    fn href_of(&self, spine_index: usize) -> String {
        self.doc
            .spine
            .get(spine_index)
            .and_then(|item| self.doc.resources.get(&item.idref))
            .map(|resource| resource.path.to_string_lossy().into_owned())
            .unwrap_or_else(|| format!("chapter_{spine_index}"))
    }

    fn chapter_title(&mut self, chapter: usize) -> String {
        if let Some(title) = self.chapter_titles.get(&chapter) {
            return title.clone();
        }
        self.chapters
            .get(chapter)
            .and_then(|parts| parts.first().copied())
            .and_then(|part| self.document(part).ok())
            .and_then(|(_, document)| first_heading(&document))
            .unwrap_or_else(|| format!("Chapter {}", chapter + 1))
    }

    /// Each chapter's title and its text as lines to search, with the
    /// chapter block each line is in.
    fn text_by_chapter(
        &mut self,
    ) -> Result<Vec<(usize, String, Vec<bookokrat::search_engine::SearchLine>)>, String> {
        let mut out = Vec::with_capacity(self.chapters.len());
        for chapter in 0..self.chapters.len() {
            let starts = self.part_starts(chapter)?;
            let mut lines = Vec::new();
            for (part, first) in self.chapters[chapter].clone().into_iter().zip(starts) {
                lines.extend(search::lines_of(&self.document(part)?.1, first));
            }
            out.push((chapter, self.chapter_title(chapter), lines));
        }
        Ok(out)
    }

    /// A contents list made of the book's headings: every heading at the
    /// most important level used more than once, and any above it, in
    /// reading order. Calibre's catalogue page and pages kept out of the
    /// reading order are left out.
    fn heading_contents(&mut self) -> Result<Vec<TocDto>, String> {
        let mut found: Vec<(u8, usize, usize, String)> = Vec::new();
        for chapter in 0..self.chapters.len() {
            let mut starts = Vec::new();
            let mut blocks = 0;
            for spine_index in self.chapters[chapter].clone() {
                starts.push(blocks);
                if !self.doc.set_current_chapter(spine_index) {
                    return Err(format!("The book has no file {spine_index}"));
                }
                let (html, _mime) = self.doc.get_current_str().unwrap_or_default();
                let document = HtmlToMarkdownConverter::keeping_scripts().convert(&html);
                let linear = self
                    .doc
                    .spine
                    .get(spine_index)
                    .is_none_or(|item| item.linear);
                if linear && !is_jacket(&html) {
                    for (index, node) in document.blocks.iter().enumerate() {
                        headings_in(node, &mut |level, title| {
                            found.push((level, chapter, blocks + index, title));
                        });
                    }
                }
                blocks += document.blocks.len();
            }
            self.part_starts.insert(chapter, starts);
        }
        let mut uses: HashMap<u8, usize> = HashMap::new();
        for (level, ..) in &found {
            *uses.entry(*level).or_default() += 1;
        }
        let Some(deepest) = (1..=6).find(|level| uses.get(level).is_some_and(|n| *n >= 2)) else {
            return Ok(Vec::new());
        };
        Ok(found
            .into_iter()
            .filter(|(level, ..)| *level <= deepest)
            .map(|(_, chapter, node, title)| TocDto {
                title,
                chapter: Some(chapter),
                anchor: None,
                node: Some(node),
                children: Vec::new(),
            })
            .collect())
    }

    fn place_by_chapter(&mut self, place: place::Place) -> Result<place::Place, String> {
        let (chapter, part) = *self
            .chapter_of
            .get(place.chapter)
            .ok_or_else(|| format!("The book has no file {}", place.chapter))?;
        let starts = self.part_starts(chapter)?;
        Ok(place::Place {
            chapter,
            node: starts[part] + place.node,
            offset: place.offset,
        })
    }
}

struct AppState {
    library_dir: String,
    open_path: Option<String>,
    perf_out: Option<PathBuf>,
    /// A replay script for automated visual checks: a JSON list of steps the
    /// frontend plays back (key presses, pauses, screenshots) before quitting.
    script: Option<PathBuf>,
    book: Mutex<Option<OpenBook>>,
    pdf: pdf::Shared,
    lookups: lookup::Running,
}

impl AppState {
    fn from_env() -> Self {
        let target = std::env::args().skip(1).find(|a| !a.starts_with('-'));
        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        // Without a file to open, the book that was open last time comes
        // back; without a folder either, so does the library it was in.
        let resumed = || place::resume_target().map(PathBuf::from);
        let (library_dir, open_path) = match target.map(|t| absolute(&cwd, &t)) {
            Some(path) if path.is_file() => (
                path.parent().map(Path::to_path_buf).unwrap_or(cwd),
                Some(path),
            ),
            Some(path) => (path, resumed()),
            None => match resumed() {
                Some(book) => (
                    place::last_library()
                        .filter(|dir| book.starts_with(dir))
                        .or_else(|| book.parent().map(Path::to_path_buf))
                        .unwrap_or(cwd),
                    Some(book),
                ),
                None => (place::last_library().unwrap_or(cwd), None),
            },
        };
        place::set_library(&library_dir);
        Self {
            library_dir: library_dir.to_string_lossy().into_owned(),
            open_path: open_path.map(|p| p.to_string_lossy().into_owned()),
            perf_out: std::env::var_os("BOOKOKRAT_READER_PERF").map(PathBuf::from),
            script: std::env::var_os("BOOKOKRAT_READER_SCRIPT").map(PathBuf::from),
            book: Mutex::new(None),
            pdf: Mutex::new(None),
            lookups: lookup::Running::default(),
        }
    }

    fn manager(&self) -> BookManager {
        let mut manager = BookManager::new_with_directory(
            &self.library_dir,
            RuntimeSettings::in_memory(Settings::default()),
        );
        // The core lists PDFs only for terminals that can draw them.
        manager.supports_graphics = true;
        manager
    }

    fn book(&self) -> MutexGuard<'_, Option<OpenBook>> {
        self.book.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn pdf(&self) -> MutexGuard<'_, Option<pdf::OpenPdf>> {
        self.pdf.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// The file on screen, whichever kind it is.
    fn open_path(&self) -> Result<String, String> {
        if let Some(book) = self.book().as_ref() {
            return Ok(book.path.clone());
        }
        self.pdf()
            .as_ref()
            .map(|open| open.path.clone())
            .ok_or_else(|| "No book is open".to_string())
    }

    fn library(&self) -> &Path {
        Path::new(&self.library_dir)
    }
}

fn absolute(cwd: &Path, target: &str) -> PathBuf {
    let path = Path::new(target);
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        cwd.join(path)
    }
}

#[derive(Serialize)]
struct LaunchInfo {
    library_dir: String,
    open_path: Option<String>,
    perf: bool,
    script: Option<serde_json::Value>,
}

#[derive(Serialize)]
struct BookDto {
    path: String,
    title: String,
    format: &'static str,
}

#[derive(Serialize)]
struct BookDetails {
    title: Option<String>,
    author: Option<String>,
    cover: bool,
}

#[derive(Clone, Serialize)]
struct TocDto {
    title: String,
    chapter: Option<usize>,
    anchor: Option<String>,
    /// For an entry made from a heading: the chapter block it is in.
    #[serde(skip_serializing_if = "Option::is_none")]
    node: Option<usize>,
    children: Vec<TocDto>,
}

#[derive(Serialize)]
struct BookMeta {
    path: String,
    /// Leads the URLs of this opening's images and pages.
    opening: String,
    title: String,
    author: Option<String>,
    language: Option<String>,
    chapter_count: usize,
    /// Whether the book carries a cover image.
    cover: bool,
    toc: Vec<TocDto>,
    /// Where the book was left, if it has been read before.
    place: Option<place::Place>,
    /// Set for a PDF or DjVu document, whose "chapters" are its pages.
    pdf: Option<PdfMeta>,
}

#[derive(Serialize)]
struct PdfMeta {
    /// Each page's width and height in the document's own units.
    pages: Vec<(f32, f32)>,
    /// Printed page number minus page position, where it could be told.
    number_offset: Option<i32>,
    /// Where the document was left.
    view: pdf::View,
    themed: bool,
    tint_images: bool,
    /// Whether TeX left the data to jump between the PDF and its source.
    synctex: bool,
    djvu: bool,
}

#[derive(Serialize)]
struct ChapterDto {
    index: usize,
    title: String,
    /// The block each of the chapter's files starts at.
    parts: Vec<usize>,
    blocks: Vec<dto::NodeDto>,
}

#[derive(Serialize)]
struct BindingDto {
    keys: Vec<String>,
    action: serde_json::Value,
}

#[tauri::command]
fn launch_info(state: State<'_, AppState>) -> LaunchInfo {
    LaunchInfo {
        library_dir: state.library_dir.clone(),
        open_path: state.open_path.clone(),
        perf: state.perf_out.is_some(),
        script: state.script.as_ref().and_then(|path| {
            let text = std::fs::read_to_string(path)
                .inspect_err(|e| log::error!("Failed to read script {path:?}: {e}"))
                .ok()?;
            serde_json::from_str(&text)
                .inspect_err(|e| log::error!("Failed to parse script {path:?}: {e}"))
                .ok()
        }),
    }
}

#[tauri::command(async)]
fn list_books(state: State<'_, AppState>) -> Vec<BookDto> {
    state
        .manager()
        .get_books()
        .into_iter()
        .map(|b| BookDto {
            path: b.path,
            title: b.display_name,
            format: match b.format {
                BookFormat::Epub => "epub",
                BookFormat::Html => "html",
                BookFormat::Pdf => "pdf",
                BookFormat::Djvu => "djvu",
            },
        })
        .collect()
}

fn is_paged(path: &str) -> bool {
    matches!(
        BookManager::detect_format(path),
        Some(BookFormat::Pdf | BookFormat::Djvu)
    )
}

/// Runs work that can take a while (reading a whole book, waiting for a
/// page to be drawn or for a lookup to finish) on the blocking pool, so it
/// never ties up the async runtime every other command runs on.
async fn blocking<T: Send + 'static>(
    app: AppHandle,
    work: impl FnOnce(&AppState) -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(move || work(&app.state::<AppState>()))
        .await
        .map_err(|e| format!("Background work failed: {e}"))?
}

/// The title and author a book declares about itself, for the library's covers.
#[tauri::command]
async fn book_details(app: AppHandle, path: String) -> Result<BookDetails, String> {
    blocking(app, move |state| book_details_now(state, path)).await
}

fn book_details_now(state: &AppState, path: String) -> Result<BookDetails, String> {
    if is_paged(&path) {
        let pdf = BookManager::detect_format(&path) == Some(BookFormat::Pdf);
        let doc = pdf
            .then(|| mupdf::Document::open(&path))
            .transpose()
            .map_err(|e| format!("Failed to open {path}: {e}"))?;
        let field = |name| {
            doc.as_ref()
                .and_then(|doc| doc.metadata(name).ok())
                .map(|text| text.trim().to_string())
                .filter(|text| !text.is_empty())
        };
        return Ok(BookDetails {
            title: field(mupdf::MetadataName::Title),
            author: field(mupdf::MetadataName::Author),
            cover: pdf,
        });
    }
    let doc = state.manager().load_epub(&path)?;
    Ok(BookDetails {
        title: doc.mdata("title").map(|m| m.value.clone()),
        author: doc.mdata("creator").map(|m| m.value.clone()),
        cover: cover_id(&doc).is_some(),
    })
}

/// Opens a book or document. `paper` and `ink` are the theme's page and
/// text colours (0xRRGGBB), which a PDF's pages are drawn in.
#[tauri::command]
async fn open_book(app: AppHandle, path: String, paper: i32, ink: i32) -> Result<BookMeta, String> {
    blocking(app.clone(), move |state| {
        open_book_now(app, state, path, paper, ink)
    })
    .await
}

fn open_book_now(
    app: AppHandle,
    state: &AppState,
    path: String,
    paper: i32,
    ink: i32,
) -> Result<BookMeta, String> {
    if is_paged(&path) {
        return open_pdf(app, state, path, paper, ink);
    }
    let meta = open_epub(state, path)?;
    *state.pdf() = None;
    Ok(meta)
}

fn pdf_toc(entries: &[bookokrat::pdf::TocEntry], open: &pdf::OpenPdf) -> Vec<TocDto> {
    fn build(
        entries: &[bookokrat::pdf::TocEntry],
        index: &mut usize,
        level: usize,
        open: &pdf::OpenPdf,
    ) -> Vec<TocDto> {
        let mut out = Vec::new();
        while let Some(entry) = entries.get(*index) {
            if entry.level < level {
                break;
            }
            *index += 1;
            let children = build(entries, index, entry.level + 1, open);
            out.push(TocDto {
                title: entry.title.clone(),
                chapter: pdf::toc_page(entry, open.pages.len(), open.number_offset),
                anchor: None,
                node: None,
                children,
            });
        }
        out
    }
    build(entries, &mut 0, 0, open)
}

fn open_pdf(
    app: AppHandle,
    state: &AppState,
    path: String,
    paper: i32,
    ink: i32,
) -> Result<BookMeta, String> {
    let left = place::restore_paged(state.library(), &path);
    let look = pdf::Look {
        themed: left.themed.unwrap_or(true),
        tint_images: left.tint_images.unwrap_or(true),
        paper,
        ink,
    };
    let reload_app = app.clone();
    let open = pdf::OpenPdf::open(
        &path,
        new_opening(),
        look,
        move |reloaded| {
            let state = reload_app.state::<AppState>();
            let mut guard = state.pdf();
            let Some(open) = guard.as_mut() else { return };
            open.reloaded(reloaded);
            let now = ReloadedDto {
                pages: open.pages.clone(),
                toc: pdf_toc(&open.toc, open),
            };
            drop(guard);
            if let Err(e) = reload_app.emit("pdf-reloaded", now) {
                log::error!("Failed to announce the reloaded document: {e}");
            }
        },
        move |file, line| {
            let target = app
                .state::<AppState>()
                .pdf()
                .as_ref()
                .and_then(|open| open.forward(&file, line));
            let found = SourceLine { file, line, target };
            if let Err(e) = app.emit("synctex-forward", found) {
                log::error!("Failed to pass on the SyncTeX request: {e}");
            }
        },
    )?;
    let page = left.page.min(open.pages.len() - 1);
    let meta = BookMeta {
        path: open.path.clone(),
        opening: open.opening.clone(),
        title: open.title.clone(),
        author: open.author.clone(),
        language: None,
        chapter_count: open.pages.len(),
        cover: !open.is_djvu(),
        toc: pdf_toc(&open.toc, &open),
        place: None,
        pdf: Some(PdfMeta {
            pages: open.pages.clone(),
            number_offset: open.number_offset,
            view: pdf::View {
                page,
                offset: if page == left.page { left.offset } else { 0.0 },
            },
            themed: look.themed,
            tint_images: look.tint_images,
            synctex: open.synctex.is_some(),
            djvu: open.is_djvu(),
        }),
    };
    place::set_reading(Some(&meta.path));
    *state.book() = None;
    *state.pdf() = Some(open);
    Ok(meta)
}

/// A watched PDF as it is after being read again.
#[derive(Clone, Serialize)]
struct ReloadedDto {
    pages: Vec<(f32, f32)>,
    toc: Vec<TocDto>,
}

/// A source line an editor asked to see, and where it is in the document.
#[derive(Clone, Serialize)]
struct SourceLine {
    file: String,
    line: u32,
    target: Option<pdf::Spot>,
}

fn open_epub(state: &AppState, path: String) -> Result<BookMeta, String> {
    let mut doc = state.manager().load_epub(&path)?;
    let toc_items = TocParser::parse_toc_structure(&mut doc);
    let mut entries = Vec::new();
    let mut toc = toc_dto(&toc_items, &doc, &mut entries);

    let listed: HashSet<usize> = entries.iter().map(|entry| entry.file).collect();
    let titles_by_file = chapter_names(&mut doc, &entries);
    let chapters = group_chapters(&mut doc, &listed);
    let mut chapter_of = vec![(0, 0); doc.get_num_chapters()];
    for (chapter, parts) in chapters.iter().enumerate() {
        for (part, spine_index) in parts.iter().enumerate() {
            chapter_of[*spine_index] = (chapter, part);
        }
    }
    retarget(&mut toc, &chapter_of);

    let title = doc
        .mdata("title")
        .map(|m| m.value.clone())
        .unwrap_or_else(|| file_stem(&path));
    let author = doc.mdata("creator").map(|m| m.value.clone());
    let language = doc.mdata("language").map(|m| m.value.clone());
    let left_at = place::restore(Path::new(&state.library_dir), &path, &doc).or_else(|| {
        Some(place::Place {
            chapter: reading_start(&mut doc),
            node: 0,
            offset: 0.0,
        })
    });
    let chapter_paths = doc
        .spine
        .iter()
        .enumerate()
        .filter_map(|(index, item)| {
            Some((
                doc.resources.get(&item.idref)?.path.clone(),
                chapter_of[index],
            ))
        })
        .collect();
    let mut book = OpenBook {
        path,
        opening: new_opening(),
        title,
        author,
        doc,
        chapter_titles: titles_by_file
            .into_iter()
            .map(|(spine_index, title)| (chapter_of[spine_index].0, title))
            .collect(),
        chapters,
        chapter_of,
        chapter_paths,
        part_starts: HashMap::new(),
        engine: None,
        words: None,
    };

    // Some books list only part of themselves (Calibre made one list its
    // endnotes); their headings make a truer contents list.
    let linear = book.doc.spine.iter().filter(|item| item.linear).count();
    if linear >= 4 && listed.len() * 4 < linear {
        let started = std::time::Instant::now();
        match book.heading_contents() {
            Ok(made) if made.len() >= 2 => {
                log::info!(
                    "Listed {} headings as the contents of {} in {:?}",
                    made.len(),
                    book.path,
                    started.elapsed()
                );
                toc = made;
            }
            Ok(_) => {}
            Err(e) => log::error!("Failed to list the headings of {}: {e}", book.path),
        }
    }

    let meta = BookMeta {
        path: book.path.clone(),
        opening: book.opening.clone(),
        title: book.title.clone(),
        author: book.author.clone(),
        language,
        chapter_count: book.chapters.len(),
        cover: cover_id(&book.doc).is_some(),
        toc,
        place: left_at.and_then(|place| {
            book.place_by_chapter(place)
                .inspect_err(|e| log::error!("Failed to restore the place in {}: {e}", book.path))
                .ok()
        }),
        pdf: None,
    };
    place::set_reading(Some(&meta.path));
    *state.book() = Some(book);
    Ok(meta)
}

#[tauri::command]
async fn load_chapter(app: AppHandle, index: usize) -> Result<ChapterDto, String> {
    blocking(app, move |state| load_chapter_now(state, index)).await
}

fn load_chapter_now(state: &AppState, index: usize) -> Result<ChapterDto, String> {
    let mut guard = state.book();
    let book = guard.as_mut().ok_or("No book is open")?;
    let parts = book
        .chapters
        .get(index)
        .cloned()
        .ok_or_else(|| format!("Chapter {index} is out of range"))?;

    let mut blocks = Vec::new();
    let mut starts = Vec::with_capacity(parts.len());
    let mut heading = None;
    for part in parts {
        let (path, document) = book.document(part)?;
        if heading.is_none() {
            heading = first_heading(&document);
        }
        let converter = dto::Converter {
            chapter_path: path.as_deref(),
            chapters: &book.chapter_paths,
        };
        starts.push(blocks.len());
        blocks.extend(converter.document(&document));
    }
    book.part_starts.insert(index, starts.clone());

    let title = book
        .chapter_titles
        .get(&index)
        .cloned()
        .or(heading)
        .unwrap_or_else(|| format!("Chapter {}", index + 1));
    Ok(ChapterDto {
        index,
        title,
        parts: starts,
        blocks,
    })
}

/// Records where the reader is in the open book. `progress` is how far
/// through the whole book that is, from 0 to 1.
#[tauri::command(async)]
fn save_place(
    state: State<'_, AppState>,
    place: place::Place,
    progress: f32,
) -> Result<(), String> {
    let mut guard = state.book();
    let book = guard.as_mut().ok_or("No book is open")?;
    let place = book.place_by_file(place)?;
    let facts = place::BookFacts {
        path: &book.path,
        title: &book.title,
        author: book.author.as_deref(),
        chapter_count: book.doc.get_num_chapters(),
    };
    place::remember(
        Path::new(&state.library_dir),
        &facts,
        &book.doc,
        place,
        progress,
    )
}

/// Records whether the open book is on screen, so the next start returns to
/// it, or to the library if that is where the reader went.
#[tauri::command(async)]
fn set_reading(state: State<'_, AppState>, reading: bool) {
    let path = state.open_path().ok().filter(|_| reading);
    place::set_reading(path.as_deref());
}

/// A highlight or note as the reader draws it: in one of its chapters, with
/// blocks counted from the chapter's start.
#[derive(Serialize)]
struct AnnotationDto {
    id: String,
    chapter: usize,
    color: Option<HighlightColor>,
    text: String,
    quote: Option<String>,
    updated: String,
    slices: Vec<notes::Slice>,
    drawable: bool,
    /// On a page of a PDF: the boxes covered, as fractions of the page's
    /// width and height (left, top, right, bottom).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    rects: Vec<[f32; 4]>,
}

fn paged_annotation_dtos(state: &AppState) -> Result<Vec<AnnotationDto>, String> {
    let guard = state.pdf();
    let open = guard.as_ref().ok_or("No book is open")?;
    let dir = place::comments_dir(state.library(), &open.path)?;
    let store = notes::load(&open.path, &dir)?;
    Ok(notes::list_paged(&store)
        .into_iter()
        .filter_map(|note| {
            let (width, height) = *open.pages.get(note.page)?;
            Some(AnnotationDto {
                id: note.id,
                chapter: note.page,
                color: note.color,
                text: note.text,
                quote: note.quote,
                updated: note.updated,
                slices: Vec::new(),
                drawable: true,
                rects: note
                    .rects
                    .iter()
                    .map(|r| {
                        [
                            r.topleft_x as f32 / width,
                            r.topleft_y as f32 / height,
                            r.bottomright_x as f32 / width,
                            r.bottomright_y as f32 / height,
                        ]
                    })
                    .collect(),
            })
        })
        .collect())
}

fn annotation_dtos(state: &AppState) -> Result<Vec<AnnotationDto>, String> {
    if state.pdf().is_some() {
        return paged_annotation_dtos(state);
    }
    let mut guard = state.book();
    let book = guard.as_mut().ok_or("No book is open")?;
    let dir = place::comments_dir(Path::new(&state.library_dir), &book.path)?;
    let store = notes::load(&book.path, &dir)?;
    let mut out = Vec::new();
    for annotation in notes::list(&store) {
        let Some(file) = book.file_of(&annotation.href) else {
            log::error!(
                "A note in {} names a chapter the book lacks: {}",
                book.path,
                annotation.href
            );
            continue;
        };
        let (chapter, part) = book.chapter_of[file];
        let first = book.part_starts(chapter)?[part];
        out.push(AnnotationDto {
            id: annotation.id,
            chapter,
            color: annotation.color,
            text: annotation.text,
            quote: annotation.quote,
            updated: annotation.updated,
            slices: annotation
                .slices
                .into_iter()
                .map(|slice| notes::Slice {
                    node: first + slice.node,
                    ..slice
                })
                .collect(),
            drawable: annotation.drawable,
            rects: Vec::new(),
        });
    }
    Ok(out)
}

/// Every highlight and note in the open book.
#[tauri::command(async)]
fn annotations(state: State<'_, AppState>) -> Result<Vec<AnnotationDto>, String> {
    annotation_dtos(&state)
}

/// The share of a new highlight or note that lies in one of a chapter's
/// files, with blocks counted from the chapter's start.
#[derive(Deserialize)]
struct PieceDto {
    slices: Vec<notes::Slice>,
    quote: Option<String>,
}

/// Adds a highlight (with a colour) or a note (without) over stretches of
/// text in one of the reader's chapters. A chapter can be several files and
/// an annotation only one, so a selection comes as a piece for each file it
/// runs through, and each piece becomes an annotation of its own.
#[tauri::command(async)]
fn add_annotation(
    state: State<'_, AppState>,
    chapter: usize,
    pieces: Vec<PieceDto>,
    color: Option<HighlightColor>,
    text: String,
) -> Result<Vec<AnnotationDto>, String> {
    {
        let mut guard = state.book();
        let book = guard.as_mut().ok_or("No book is open")?;
        let starts = book.part_starts(chapter)?;
        let part_of = |node: usize| starts.iter().rposition(|start| *start <= node).unwrap_or(0);
        let mut filed = Vec::with_capacity(pieces.len());
        for piece in pieces {
            let Some(part) = piece.slices.first().map(|slice| part_of(slice.node)) else {
                continue;
            };
            if piece.slices.iter().any(|slice| part_of(slice.node) != part) {
                return Err("A piece of the selection lies in more than one file".to_string());
            }
            filed.push(notes::Piece {
                href: book.href_of(book.chapters[chapter][part]),
                slices: piece
                    .slices
                    .into_iter()
                    .map(|slice| notes::Slice {
                        node: slice.node - starts[part],
                        ..slice
                    })
                    .collect(),
                quote: piece.quote,
            });
        }
        let dir = place::comments_dir(Path::new(&state.library_dir), &book.path)?;
        let mut store = notes::load(&book.path, &dir)?;
        notes::add(&mut store, filed, color, text)?;
    }
    annotation_dtos(&state)
}

/// Adds a highlight or a note over boxes on a page of a PDF, given as
/// fractions of the page's width and height.
#[tauri::command(async)]
fn add_page_annotation(
    state: State<'_, AppState>,
    page: usize,
    rects: Vec<[f32; 4]>,
    color: Option<HighlightColor>,
    text: String,
    quote: Option<String>,
) -> Result<Vec<AnnotationDto>, String> {
    {
        let guard = state.pdf();
        let open = guard.as_ref().ok_or("No document is open")?;
        let (width, height) = *open
            .pages
            .get(page)
            .ok_or_else(|| format!("The document has no page {}", page + 1))?;
        let rects = rects
            .iter()
            .map(|[x0, y0, x1, y1]| bookokrat::comments::PdfSelectionRect {
                page,
                topleft_x: (x0 * width).round().max(0.0) as u32,
                topleft_y: (y0 * height).round().max(0.0) as u32,
                bottomright_x: (x1 * width).round().max(0.0) as u32,
                bottomright_y: (y1 * height).round().max(0.0) as u32,
            })
            .filter(|r| r.bottomright_x > r.topleft_x && r.bottomright_y > r.topleft_y)
            .collect();
        let dir = place::comments_dir(state.library(), &open.path)?;
        let mut store = notes::load(&open.path, &dir)?;
        let name = notes::paged_name(&store)
            .or_else(|| place::book_key(state.library(), &open.path))
            .unwrap_or_else(|| open.path.clone());
        notes::add_paged(&mut store, &name, page, rects, color, text, quote)?;
    }
    annotation_dtos(&state)
}

/// Changes a note's text or a highlight's colour.
#[tauri::command(async)]
fn edit_annotation(
    state: State<'_, AppState>,
    id: String,
    text: Option<String>,
    color: Option<HighlightColor>,
) -> Result<Vec<AnnotationDto>, String> {
    {
        let path = state.open_path()?;
        let dir = place::comments_dir(state.library(), &path)?;
        let mut store = notes::load(&path, &dir)?;
        if let Some(text) = text {
            notes::set_text(&mut store, &id, text)?;
        }
        if let Some(color) = color {
            notes::set_color(&mut store, &id, color)?;
        }
    }
    annotation_dtos(&state)
}

#[tauri::command(async)]
fn delete_annotation(state: State<'_, AppState>, id: String) -> Result<Vec<AnnotationDto>, String> {
    {
        let path = state.open_path()?;
        let dir = place::comments_dir(state.library(), &path)?;
        let mut store = notes::load(&path, &dir)?;
        notes::delete(&mut store, &id)?;
    }
    annotation_dtos(&state)
}

/// A mark as the reader shows it. One in the open book carries its place
/// there; one in another book only says which book.
#[derive(Serialize)]
struct MarkDto {
    letter: char,
    path: String,
    here: bool,
    chapter: Option<usize>,
    node: Option<usize>,
    offset: Option<usize>,
    snippet: Option<String>,
    chapter_title: Option<String>,
}

fn mark_dtos(state: &AppState) -> Result<Vec<MarkDto>, String> {
    let open_path = state.open_path()?;
    let mut guard = state.book();
    let mut book = guard.as_mut();
    let library = Path::new(&state.library_dir);
    let mut out = Vec::new();
    let local = place::local_marks(library, &open_path)
        .into_iter()
        .map(|mark| (mark, true));
    let global = place::global_marks().into_iter().map(|mark| (mark, false));
    for ((letter, mark), local) in local.chain(global) {
        let is_here = |path: &str| {
            let named = Path::new(path.strip_prefix("./").unwrap_or(path));
            local || Path::new(&open_path).ends_with(named)
        };
        match mark {
            MarkLocation::Epub {
                path,
                chapter,
                node,
                node_offset,
                snippet,
                chapter_title,
            } => {
                let place = book.as_mut().filter(|_| is_here(&path)).and_then(|book| {
                    book.place_by_chapter(place::Place {
                        chapter,
                        node,
                        offset: 0.0,
                    })
                    .inspect_err(|e| log::error!("Mark {letter} points outside {}: {e}", book.path))
                    .ok()
                });
                out.push(MarkDto {
                    letter,
                    path,
                    here: place.is_some(),
                    chapter: place.map(|p| p.chapter),
                    node: place.map(|p| p.node),
                    offset: node_offset,
                    snippet,
                    chapter_title,
                });
            }
            // In a PDF the "chapter" is the page and the "node" the line on it.
            MarkLocation::Pdf {
                path,
                page,
                snippet,
                line_idx,
                ..
            } => {
                let here = book.is_none() && is_here(&path);
                out.push(MarkDto {
                    letter,
                    path,
                    here,
                    chapter: here.then_some(page),
                    node: line_idx.filter(|_| here),
                    offset: None,
                    snippet,
                    chapter_title: Some(format!("Page {}", page + 1)),
                });
            }
        }
    }
    out.sort_by_key(|mark| (mark.letter.is_ascii_uppercase(), mark.letter));
    Ok(out)
}

/// The open book's marks and the marks shared by all libraries.
#[tauri::command(async)]
fn marks(state: State<'_, AppState>) -> Result<Vec<MarkDto>, String> {
    mark_dtos(&state)
}

/// Sets a mark at a place in the open book: `a` to `z` belong to the book,
/// `A` to `Z` are shared by every library.
#[tauri::command(async)]
#[allow(clippy::too_many_arguments)]
fn set_mark(
    state: State<'_, AppState>,
    letter: char,
    place: place::Place,
    offset: Option<usize>,
    snippet: Option<String>,
    chapter_title: Option<String>,
    progress: f32,
) -> Result<Vec<MarkDto>, String> {
    {
        let mut guard = state.book();
        let book = guard.as_mut().ok_or("No book is open")?;
        let by_file = book.place_by_file(place)?;
        let mark = MarkLocation::Epub {
            path: book.path.clone(),
            chapter: by_file.chapter,
            node: by_file.node,
            node_offset: offset,
            snippet,
            chapter_title,
        };
        if letter.is_ascii_lowercase() {
            // A book's own marks live in its bookmark, which this makes
            // sure exists.
            let facts = place::BookFacts {
                path: &book.path,
                title: &book.title,
                author: book.author.as_deref(),
                chapter_count: book.doc.get_num_chapters(),
            };
            let library = Path::new(&state.library_dir);
            place::remember(library, &facts, &book.doc, by_file, progress)?;
            place::set_local_mark(library, &book.path, letter, Some(&mark))?;
        } else if letter.is_ascii_uppercase() {
            place::set_global_mark(letter, Some(mark))?;
        } else {
            return Err(format!("{letter} cannot name a mark; use a letter"));
        }
    }
    mark_dtos(&state)
}

/// Sets a mark on a page of the open PDF, at one of its lines if given.
#[tauri::command(async)]
fn set_page_mark(
    state: State<'_, AppState>,
    letter: char,
    page: usize,
    line: Option<usize>,
    snippet: Option<String>,
    offset: f32,
) -> Result<Vec<MarkDto>, String> {
    {
        let guard = state.pdf();
        let open = guard.as_ref().ok_or("No document is open")?;
        let mark = MarkLocation::Pdf {
            path: open.path.clone(),
            page,
            scroll_offset: 0,
            snippet,
            line_idx: line,
        };
        if letter.is_ascii_lowercase() {
            remember_page(&state, open, page, offset, None, None)?;
            place::set_local_mark(state.library(), &open.path, letter, Some(&mark))?;
        } else if letter.is_ascii_uppercase() {
            place::set_global_mark(letter, Some(mark))?;
        } else {
            return Err(format!("{letter} cannot name a mark; use a letter"));
        }
    }
    mark_dtos(&state)
}

#[tauri::command(async)]
fn delete_mark(state: State<'_, AppState>, letter: char) -> Result<Vec<MarkDto>, String> {
    {
        let path = state.open_path()?;
        if letter.is_ascii_lowercase() {
            place::set_local_mark(Path::new(&state.library_dir), &path, letter, None)?;
        } else {
            place::set_global_mark(letter, None)?;
        }
    }
    mark_dtos(&state)
}

fn remember_page(
    state: &AppState,
    open: &pdf::OpenPdf,
    page: usize,
    offset: f32,
    themed: Option<bool>,
    tint_images: Option<bool>,
) -> Result<(), String> {
    let facts = place::BookFacts {
        path: &open.path,
        title: &open.title,
        author: open.author.as_deref(),
        chapter_count: open.pages.len(),
    };
    let place = place::PagedPlace {
        page,
        offset,
        themed,
        tint_images,
    };
    place::remember_paged(state.library(), &facts, &place)
}

/// Records the page the open PDF is at and how its pages are coloured.
#[tauri::command(async)]
fn save_page_place(
    state: State<'_, AppState>,
    page: usize,
    offset: f32,
    themed: bool,
    tint_images: bool,
) -> Result<(), String> {
    let guard = state.pdf();
    let open = guard.as_ref().ok_or("No document is open")?;
    remember_page(&state, open, page, offset, Some(themed), Some(tint_images))
}

fn pdf_handle(state: &AppState) -> Result<pdf::Handle, String> {
    state
        .pdf()
        .as_ref()
        .map(|open| open.handle.clone())
        .ok_or_else(|| "No document is open".to_string())
}

/// The text and links of a page, laid out as it is drawn `width` pixels wide.
#[tauri::command]
async fn pdf_layer(app: AppHandle, page: usize, width: u16) -> Result<pdf::PageLayer, String> {
    blocking(app, move |state| pdf_layer_now(state, page, width)).await
}

fn pdf_layer_now(state: &AppState, page: usize, width: u16) -> Result<pdf::PageLayer, String> {
    let handle = pdf_handle(state)?;
    Ok(pdf::layer(&*handle.page(page, width)?))
}

#[tauri::command(async)]
fn pdf_look(state: State<'_, AppState>, look: pdf::Look) -> Result<(), String> {
    pdf_handle(&state)?.set_look(look);
    Ok(())
}

/// Starts or stops reloading the open PDF whenever its file changes.
#[tauri::command(async)]
fn pdf_watch(state: State<'_, AppState>, on: bool) -> Result<(), String> {
    let guard = state.pdf();
    let open = guard.as_ref().ok_or("No document is open")?;
    if on && open.is_djvu() {
        return Err("Watching is not supported for DjVu files".to_string());
    }
    open.handle.set_watching(on);
    Ok(())
}

/// The pages on which some text occurs, for `/` to step through.
#[tauri::command]
async fn pdf_find_pages(app: AppHandle, query: String) -> Result<Vec<usize>, String> {
    blocking(app, move |state| pdf_find_pages_now(state, query)).await
}

fn pdf_find_pages_now(state: &AppState, query: String) -> Result<Vec<usize>, String> {
    let source = {
        let guard = state.pdf();
        let open = guard.as_ref().ok_or("No document is open")?;
        if let Some(pages) = open.pages_with(&query) {
            return Ok(pages);
        }
        open.text_source()
    };
    // Read with the document free, so its pages keep being drawn meanwhile.
    let folded = source.folded();
    let mut guard = state.pdf();
    let open = guard.as_mut().ok_or("No document is open")?;
    open.keep_folded(&source, folded);
    Ok(open.pages_with(&query).unwrap_or_default())
}

#[tauri::command]
async fn pdf_page_text(app: AppHandle, page: usize) -> Result<String, String> {
    blocking(app, move |state| pdf_page_text_now(state, page)).await
}

fn pdf_page_text_now(state: &AppState, page: usize) -> Result<String, String> {
    let guard = state.pdf();
    guard.as_ref().ok_or("No document is open")?.page_text(page)
}

/// Opens the TeX source for a point in the open PDF, in the editor that
/// `synctex_editor` in config.yaml names. Returns a line for the status bar.
#[tauri::command(async)]
fn synctex_inverse(state: State<'_, AppState>, spot: pdf::Spot) -> Result<String, String> {
    let (file, line) = {
        let guard = state.pdf();
        let open = guard.as_ref().ok_or("No document is open")?;
        if open.synctex.is_none() {
            return Err("This document has no SyncTeX data beside it".to_string());
        }
        let (file, line) = open
            .inverse(spot)
            .ok_or("SyncTeX found no source for that place")?;
        // The data may name the source as it was on the machine that ran TeX.
        let beside = Path::new(&open.path)
            .parent()
            .zip(Path::new(&file).file_name())
            .map(|(dir, name)| dir.join(name));
        let file = match beside {
            Some(beside) if !Path::new(&file).exists() && beside.exists() => {
                beside.to_string_lossy().into_owned()
            }
            _ => file,
        };
        (file, line)
    };
    let name = file_name(&file);
    let Some(editor) = prefs::synctex_editor() else {
        return Ok(format!(
            "SyncTeX: {name}:{line} (set synctex_editor in config.yaml to open it)"
        ));
    };
    if state.script.is_some() {
        return Ok(format!("SyncTeX: {name}:{line}"));
    }
    let command = bookokrat::system_command::fill_placeholder(
        &editor
            .replace("{line}", &line.to_string())
            .replace("{column}", "0"),
        "{file}",
        &file,
    );
    let child = bookokrat::system_command::shell_command(&command)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|e| format!("Failed to run {command}: {e}"))?;
    bookokrat::system_command::reap_in_background(child);
    Ok(format!("SyncTeX: {name}:{line}"))
}

fn file_name(path: &str) -> String {
    Path::new(path)
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string())
}

/// Searches the whole of the open book or document.
#[tauri::command]
async fn search_book(app: AppHandle, query: String) -> Result<Vec<search::Hit>, String> {
    blocking(app, move |state| search_book_now(state, query)).await
}

fn search_book_now(state: &AppState, query: String) -> Result<Vec<search::Hit>, String> {
    let source = match state.pdf().as_ref() {
        Some(open) => match open.search(&query) {
            Some(hits) => return Ok(hits),
            None => Some(open.text_source()),
        },
        None => None,
    };
    if let Some(source) = source {
        // Read with the document free, so its pages keep being drawn meanwhile.
        let index = search::Index::of_pages(source.lines());
        let hits = source.search(&index, &query);
        if let Some(open) = state.pdf().as_mut() {
            open.keep_index(&source, index);
        }
        return Ok(hits);
    }
    read_book_text(state)?;
    let guard = state.book();
    let book = guard.as_ref().ok_or("No book is open")?;
    Ok(book
        .engine
        .as_ref()
        .map(|index| index.run(&query, None))
        .unwrap_or_default())
}

/// Reads the open book's text for search and statistics, the first time
/// either needs it. The reading goes through a second opening of the file,
/// so the book stays free for chapters and pictures to load meanwhile.
fn read_book_text(state: &AppState) -> Result<(), String> {
    let (path, opening) = {
        let guard = state.book();
        let book = guard.as_ref().ok_or("No book is open")?;
        if book.engine.is_some() && book.words.is_some() {
            return Ok(());
        }
        (book.path.clone(), book.opening.clone())
    };
    let doc = state.manager().load_epub(&path)?;
    let mut twin = {
        let guard = state.book();
        let book = guard
            .as_ref()
            .filter(|book| book.opening == opening)
            .ok_or("The book was closed")?;
        book.twin(doc)
    };
    let text = twin.text_by_chapter()?;
    let words = text
        .iter()
        .map(|(chapter, title, lines)| (*chapter, title.clone(), search::word_count(lines)))
        .collect();
    let engine = search::Index::of_chapters(text);

    let mut guard = state.book();
    if let Some(book) = guard.as_mut().filter(|book| book.opening == opening) {
        book.part_starts.extend(twin.part_starts);
        book.engine = Some(engine);
        book.words = Some(words);
    }
    Ok(())
}

#[derive(Serialize)]
struct StatRow {
    title: String,
    /// The chapter, or for a PDF the page the section starts on.
    chapter: usize,
    count: usize,
}

#[derive(Serialize)]
struct Stats {
    /// What `count` counts: `words` in a book, `pages` in a PDF.
    unit: &'static str,
    rows: Vec<StatRow>,
}

/// How long each chapter of the open book is.
#[tauri::command]
async fn book_stats(app: AppHandle) -> Result<Stats, String> {
    blocking(app, book_stats_now).await
}

fn book_stats_now(state: &AppState) -> Result<Stats, String> {
    if let Some(open) = state.pdf().as_ref() {
        let pages = open.pages.len();
        let mut starts: Vec<(usize, String)> = open
            .toc
            .iter()
            .filter(|entry| entry.level == 0)
            .filter_map(|entry| {
                Some((
                    pdf::toc_page(entry, pages, open.number_offset)?,
                    entry.title.clone(),
                ))
            })
            .collect();
        starts.sort_by_key(|(page, _)| *page);
        starts.dedup_by_key(|(page, _)| *page);
        if starts.first().is_none_or(|(page, _)| *page > 0) {
            let title = if starts.is_empty() {
                &open.title
            } else {
                "Front matter"
            };
            starts.insert(0, (0, title.to_string()));
        }
        let ends: Vec<usize> = starts
            .iter()
            .skip(1)
            .map(|(page, _)| *page)
            .chain([pages])
            .collect();
        return Ok(Stats {
            unit: "pages",
            rows: starts
                .into_iter()
                .zip(ends)
                .map(|((page, title), end)| StatRow {
                    title,
                    chapter: page,
                    count: end - page,
                })
                .collect(),
        });
    }
    read_book_text(state)?;
    let guard = state.book();
    let book = guard.as_ref().ok_or("No book is open")?;
    Ok(Stats {
        unit: "words",
        rows: book
            .words
            .iter()
            .flatten()
            .map(|(chapter, title, count)| StatRow {
                title: title.clone(),
                chapter: *chapter,
                count: *count,
            })
            .collect(),
    })
}

/// The chapter as the book has it: the HTML of each of its files.
#[tauri::command]
async fn chapter_html(app: AppHandle, index: usize) -> Result<String, String> {
    blocking(app, move |state| chapter_html_now(state, index)).await
}

fn chapter_html_now(state: &AppState, index: usize) -> Result<String, String> {
    let mut guard = state.book();
    let book = guard.as_mut().ok_or("No book is open")?;
    let parts = book
        .chapters
        .get(index)
        .cloned()
        .ok_or_else(|| format!("Chapter {index} is out of range"))?;
    let mut out = String::new();
    for part in parts {
        if !book.doc.set_current_chapter(part) {
            return Err(format!("The book has no file {part}"));
        }
        if !out.is_empty() {
            out.push_str("\n\n");
        }
        out.push_str(&book.doc.get_current_str().unwrap_or_default().0);
    }
    Ok(out)
}

/// Writes the open book's highlights and notes as Markdown beside the
/// book, the way the terminal app exports them. Returns the file written.
#[tauri::command]
async fn export_notes(app: AppHandle, chapter: usize) -> Result<String, String> {
    blocking(app, move |state| export_notes_now(state, chapter)).await
}

fn export_notes_now(state: &AppState, chapter: usize) -> Result<String, String> {
    use bookokrat::widget::comments_viewer::CommentsViewer;

    let path = state.open_path()?;
    let dir = place::comments_dir(state.library(), &path)?;
    let store = notes::load(&path, &dir)?;
    if store.get_all_comments().is_empty() {
        return Err("There are no highlights or notes to export".to_string());
    }
    let store = Arc::new(Mutex::new(store));
    let (title, markdown) = if let Some(open) = state.pdf().as_ref() {
        let name = notes::paged_name(&store.lock().unwrap_or_else(|e| e.into_inner()))
            .unwrap_or_else(|| open.path.clone());
        let viewer = CommentsViewer::new_for_pdf(
            store,
            &name,
            &open.toc,
            open.pages.len(),
            chapter,
            open.title.clone(),
        );
        (
            open.title.clone(),
            viewer.create_exporter().generate_markdown(),
        )
    } else {
        let mut guard = state.book();
        let book = guard.as_mut().ok_or("No book is open")?;
        let toc = TocParser::parse_toc_structure(&mut book.doc);
        let title = book.title.clone();
        let viewer = CommentsViewer::new(store, &mut book.doc, &toc, None, title.clone());
        (title, viewer.create_exporter().generate_markdown())
    };
    let folder = match &state.script {
        Some(script) => script.parent().map(Path::to_path_buf),
        None => Path::new(&path).parent().map(Path::to_path_buf),
    }
    .unwrap_or_else(|| state.library().to_path_buf());
    // Named as the terminal app names its exports.
    let words: Vec<String> = title
        .split(|c: char| !c.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .map(str::to_lowercase)
        .collect();
    let name = if words.is_empty() {
        "comments.md".to_string()
    } else {
        format!("{}_comments.md", words.join("-"))
    };
    let out = folder.join(name);
    std::fs::write(&out, markdown).map_err(|e| format!("Failed to write {out:?}: {e}"))?;
    Ok(out.to_string_lossy().into_owned())
}

/// Opens the book in the system's own viewer, at the chapter on screen.
#[tauri::command(async)]
fn open_in_viewer(state: State<'_, AppState>, chapter: usize) -> Result<(), String> {
    let (path, spine) = match state.book().as_ref() {
        Some(book) => (
            book.path.clone(),
            book.chapters
                .get(chapter)
                .and_then(|parts| parts.first().copied())
                .unwrap_or(0),
        ),
        None => (state.open_path()?, 0),
    };
    // A viewer opened by an automated check would land on the user's screen.
    if let Some(script) = &state.script {
        let out = script.with_file_name("opened.txt");
        return std::fs::write(&out, format!("{path}\n{spine}\n"))
            .map_err(|e| format!("Failed to write {out:?}: {e}"));
    }
    RealSystemCommandExecutor.open_file_at_chapter(&path, spine)
}

#[tauri::command(async)]
fn reading_history(state: State<'_, AppState>) -> Vec<history::Entry> {
    history::list(state.library())
}

#[tauri::command(async)]
fn forget_history(
    state: State<'_, AppState>,
    source: String,
    key: String,
) -> Result<Vec<history::Entry>, String> {
    history::forget(&source, &key)?;
    Ok(history::list(state.library()))
}

/// Reads keybindings.toml again. Returns what is wrong with it, if anything.
#[tauri::command(async)]
fn keymap_problems() -> Vec<String> {
    bookokrat::keybindings::reload_keymap()
        .into_iter()
        .map(|error| error.message)
        .collect()
}

/// The default bindings with the user's `keybindings.toml` applied on top.
#[tauri::command(async)]
fn keymap() -> HashMap<&'static str, Vec<BindingDto>> {
    for error in bookokrat::keybindings::reload_keymap() {
        log::error!("keybindings.toml: {}", error.message);
    }
    let keymap = bookokrat::keybindings::keymap();
    KeyContext::ALL
        .iter()
        .filter_map(|ctx| {
            let bindings = keymap
                .context(*ctx)?
                .all_bindings()
                .into_iter()
                .filter_map(|(binding, action)| {
                    Some(BindingDto {
                        keys: binding.keys().iter().map(key_token).collect(),
                        action: serde_json::to_value(&action).ok()?,
                    })
                })
                .collect();
            Some((ctx.config_key(), bindings))
        })
        .collect()
}

#[tauri::command]
fn open_external(state: State<'_, AppState>, url: String) -> Result<(), String> {
    let allowed = ["http://", "https://", "mailto:"];
    if !allowed.iter().any(|scheme| url.starts_with(scheme)) {
        return Err(format!("Refusing to open {url}"));
    }
    // A browser opened by an automated check would land on the user's screen.
    if let Some(script) = &state.script {
        let out = script.with_file_name("opened.txt");
        return std::fs::write(&out, format!("{url}\n"))
            .map_err(|e| format!("Failed to write {out:?}: {e}"));
    }
    open::that_detached(&url).map_err(|e| format!("Failed to open {url}: {e}"))
}

#[tauri::command(async)]
fn appearance() -> prefs::Appearance {
    prefs::load()
}

#[tauri::command(async)]
fn save_prefs(prefs: prefs::ReaderPrefs) -> Result<(), String> {
    prefs::save(&prefs).inspect_err(|e| log::error!("Failed to save preferences: {e}"))
}

#[tauri::command(async)]
fn system_fonts() -> Vec<String> {
    prefs::system_fonts()
}

/// Captures the screen for a replay script by running the command in
/// `BOOKOKRAT_READER_SHOT_CMD` with the output path, next to the script file.
#[tauri::command(async)]
fn script_shot(state: State<'_, AppState>, name: String) -> Result<(), String> {
    let script = state.script.as_ref().ok_or("No replay script is running")?;
    let command = std::env::var("BOOKOKRAT_READER_SHOT_CMD")
        .map_err(|_| "BOOKOKRAT_READER_SHOT_CMD is not set")?;
    let out = script.with_file_name(format!("{name}.png"));
    // Give the compositor time to present the frame that was just painted.
    std::thread::sleep(std::time::Duration::from_millis(150));
    let status = std::process::Command::new("sh")
        .arg("-c")
        .arg(format!("{command} \"$0\""))
        .arg(&out)
        .stdout(std::process::Stdio::null())
        .status()
        .map_err(|e| format!("Failed to run {command}: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("{command} exited with {status}"))
    }
}

/// Writes a line of text beside the replay script, for a check to read.
#[tauri::command(async)]
fn script_note(state: State<'_, AppState>, name: String, text: String) -> Result<(), String> {
    let script = state.script.as_ref().ok_or("No replay script is running")?;
    if name.contains(['/', '\\']) {
        return Err(format!("{name} is not a plain file name"));
    }
    let out = script.with_file_name(format!("{name}.txt"));
    std::fs::write(&out, text).map_err(|e| format!("Failed to write {out:?}: {e}"))
}

/// Sends a real mouse event to the page for a replay script: `press`,
/// `move` with the button held, or `release`, at a point of the page.
/// `ratio` is the page's device pixel ratio.
#[tauri::command(async)]
fn script_mouse(
    window: tauri::WebviewWindow,
    state: State<'_, AppState>,
    kind: String,
    x: f64,
    y: f64,
    ratio: f64,
) -> Result<(), String> {
    if state.script.is_none() {
        return Err("No replay script is running".to_string());
    }
    #[cfg(target_os = "linux")]
    return harness::mouse(&window, kind, x, y, ratio);
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (window, kind, x, y, ratio);
        Err("Real mouse input is only wired up on Linux".to_string())
    }
}

#[tauri::command(async)]
fn lookup_targets() -> Result<lookup::Targets, String> {
    lookup::targets()
}

#[tauri::command]
async fn lookup_run(
    app: AppHandle,
    id: u64,
    target: Option<String>,
    text: String,
) -> Result<lookup::Outcome, String> {
    blocking(app, move |state| lookup_run_now(state, id, target, text)).await
}

fn lookup_run_now(
    state: &AppState,
    id: u64,
    target: Option<String>,
    text: String,
) -> Result<lookup::Outcome, String> {
    lookup::run(&state.lookups, id, target.as_deref(), &text)
}

#[tauri::command]
fn lookup_cancel(state: State<'_, AppState>, id: u64) {
    state.lookups.cancel(id);
}

/// Under a replay script the text goes to `clipboard.txt` beside the script,
/// so automated checks can read it and never touch the real clipboard.
#[tauri::command(async)]
fn copy_text(state: State<'_, AppState>, text: String) -> Result<(), String> {
    if let Some(script) = &state.script {
        let out = script.with_file_name("clipboard.txt");
        return std::fs::write(&out, text).map_err(|e| format!("Failed to write {out:?}: {e}"));
    }
    bookokrat::clipboard::copy_to_clipboard(&text)
        .inspect_err(|e| log::error!("Failed to copy to the clipboard: {e}"))
}

#[tauri::command]
fn quit(app: AppHandle) {
    app.exit(0);
}

#[tauri::command]
fn perf_report(
    app: AppHandle,
    state: State<'_, AppState>,
    report: serde_json::Value,
) -> Result<(), String> {
    if let Some(out) = &state.perf_out {
        let json = serde_json::to_string_pretty(&report).map_err(|e| e.to_string())?;
        std::fs::write(out, json).map_err(|e| format!("Failed to write {out:?}: {e}"))?;
    }
    app.exit(0);
    Ok(())
}

/// One key as the frontend names it: modifiers first, then the key, with
/// Shift left out for characters because the character already carries it.
fn key_token(input: &KeyInput) -> String {
    let (name, is_char) = match input.code {
        KeyCode::Char(' ') => ("Space".to_string(), false),
        KeyCode::Char(c) => (c.to_string(), true),
        KeyCode::Enter => ("Enter".to_string(), false),
        KeyCode::Esc => ("Esc".to_string(), false),
        KeyCode::Tab => ("Tab".to_string(), false),
        KeyCode::BackTab => ("S-Tab".to_string(), false),
        KeyCode::F(n) => (format!("F{n}"), false),
        other => (format!("{other:?}"), false),
    };
    let mut token = String::new();
    if input.modifiers.contains(KeyModifiers::CONTROL) {
        token.push_str("C-");
    }
    if input.modifiers.contains(KeyModifiers::ALT) {
        token.push_str("A-");
    }
    if input.modifiers.contains(KeyModifiers::SHIFT) && !is_char {
        token.push_str("S-");
    }
    token + &name
}

/// An entry of the contents list, by the file it leads to.
struct TocEntry {
    file: usize,
    title: String,
    anchor: Option<String>,
}

fn toc_dto(items: &[TocItem], doc: &Epub, entries: &mut Vec<TocEntry>) -> Vec<TocDto> {
    items
        .iter()
        .map(|item| {
            let (title, href, anchor, children) = match item {
                TocItem::Chapter {
                    title,
                    href,
                    anchor,
                } => (title, Some(href.as_str()), anchor, &[][..]),
                TocItem::Section {
                    title,
                    href,
                    anchor,
                    children,
                    ..
                } => (title, href.as_deref(), anchor, children.as_slice()),
            };
            let chapter = href.and_then(|h| chapter_for_href(h, doc));
            if let Some(file) = chapter {
                entries.push(TocEntry {
                    file,
                    title: title.clone(),
                    anchor: anchor.clone(),
                });
            }
            TocDto {
                title: title.clone(),
                chapter,
                anchor: anchor.clone(),
                node: None,
                children: toc_dto(children, doc, entries),
            }
        })
        .collect()
}

/// The names the contents list gives files: that of the first entry leading
/// to the file itself, or to an anchor with nothing to read ahead of it. An
/// entry pointing further in names what is there, such as one of a book's
/// endnotes, not the file; such a file is named by its first heading.
fn chapter_names(doc: &mut Epub, entries: &[TocEntry]) -> HashMap<usize, String> {
    let mut names = HashMap::new();
    let mut pages: HashMap<usize, Option<String>> = HashMap::new();
    for entry in entries {
        if names.contains_key(&entry.file) {
            continue;
        }
        let opens = match &entry.anchor {
            None => true,
            Some(anchor) => pages
                .entry(entry.file)
                .or_insert_with(|| {
                    doc.set_current_chapter(entry.file)
                        .then(|| doc.get_current_str().map(|(html, _)| html))
                        .flatten()
                })
                .as_deref()
                .is_none_or(|html| anchor_opens_page(html, anchor)),
        };
        if opens {
            names.insert(entry.file, entry.title.clone());
        }
    }
    names
}

/// Whether an anchor sits at the start of a page: no text comes before the
/// element that carries it. An anchor the page lacks counts as its start,
/// since a link to it lands there.
fn anchor_opens_page(html: &str, anchor: &str) -> bool {
    let body = html.find("<body").map_or(html, |at| &html[at..]);
    let Some(at) = ["id", "name"]
        .iter()
        .flat_map(|attr| ['"', '\''].map(|quote| format!("{attr}={quote}{anchor}{quote}")))
        .filter_map(|needle| body.find(&needle))
        .min()
    else {
        return true;
    };
    let before = &body[..body[..at].rfind('<').unwrap_or(0)];
    let mut text = String::new();
    let mut in_tag = false;
    for ch in before.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => text.push(ch),
            _ => {}
        }
    }
    let text = text
        .to_ascii_lowercase()
        .replace("&nbsp;", " ")
        .replace("&#160;", " ")
        .replace("&#xa0;", " ");
    text.trim().is_empty()
}

/// Points the contents list at the reader's chapters instead of the book's files.
fn retarget(items: &mut [TocDto], chapter_of: &[(usize, usize)]) {
    for item in items {
        item.chapter = item
            .chapter
            .and_then(|spine_index| chapter_of.get(spine_index))
            .map(|(chapter, _)| *chapter);
        retarget(&mut item.children, chapter_of);
    }
}

/// Sorts the book's files into chapters. A file starts a chapter if the
/// contents list names it or it opens with a heading; any other file carries
/// on the chapter before it. Files ahead of the first chapter (a cover, a
/// dedication) stay on their own, and so does everything in a book whose
/// contents list names too little to go by.
fn group_chapters(doc: &mut Epub, listed: &HashSet<usize>) -> Vec<Vec<usize>> {
    let count = doc.get_num_chapters();
    if listed.len() < 2 {
        return (0..count).map(|index| vec![index]).collect();
    }
    let mut chapters: Vec<Vec<usize>> = Vec::new();
    let mut in_chapter = false;
    for index in 0..count {
        let starts_chapter = listed.contains(&index)
            || (doc.set_current_chapter(index)
                && doc
                    .get_current_str()
                    .is_some_and(|(html, _)| opens_with_heading(&html)));
        match chapters.last_mut() {
            Some(parts) if in_chapter && !starts_chapter => parts.push(index),
            _ => {
                chapters.push(vec![index]);
                in_chapter = starts_chapter;
            }
        }
    }
    chapters
}

/// Whether the first thing in a page's body, inside any wrapping elements,
/// is a heading.
fn opens_with_heading(html: &str) -> bool {
    const WRAPPERS: [&str; 8] = [
        "div", "section", "article", "header", "hgroup", "main", "span", "a",
    ];
    let Some(body) = html.find("<body").and_then(|at| {
        let rest = &html[at..];
        rest.find('>').map(|end| &rest[end + 1..])
    }) else {
        return false;
    };
    let mut rest = body.trim_start();
    loop {
        if let Some(after) = rest.strip_prefix("<!--") {
            match after.find("-->") {
                Some(end) => rest = after[end + 3..].trim_start(),
                None => return false,
            }
            continue;
        }
        let Some(tag) = rest.strip_prefix('<') else {
            return false;
        };
        // An empty anchor ahead of the heading closes before it.
        if tag.starts_with('/') {
            match tag.find('>') {
                Some(end) => rest = tag[end + 1..].trim_start(),
                None => return false,
            }
            continue;
        }
        let name: String = tag
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric())
            .collect::<String>()
            .to_ascii_lowercase();
        if matches!(name.as_str(), "h1" | "h2" | "h3" | "h4" | "h5" | "h6") {
            return true;
        }
        if !WRAPPERS.contains(&name.as_str()) {
            return false;
        }
        match tag.find('>') {
            Some(end) => rest = tag[end + 1..].trim_start(),
            None => return false,
        }
    }
}

/// TOC hrefs are stored relative to the book root already; the join is a
/// fallback for books whose NCX stores them relative to the OPF instead.
fn chapter_for_href(href: &str, doc: &Epub) -> Option<usize> {
    let base = doc.root_file.parent().unwrap_or_else(|| Path::new(""));
    let path = Path::new(href);
    let resolved = if path.is_absolute() || path.starts_with(base) {
        path.to_path_buf()
    } else {
        base.join(path)
    };
    doc.resource_uri_to_chapter(&dto::normalize(&resolved))
}

/// The file a book opens at the first time: where it says its text begins,
/// or else the first page past its cover.
fn reading_start(doc: &mut Epub) -> usize {
    let count = doc.get_num_chapters();
    let declared = DeclaredPages::read(doc);
    let has_text = |doc: &mut Epub, index: usize| page_look(doc, index).is_some_and(|p| p.text);
    if let Some(start) = declared
        .start
        .filter(|start| *start < count && !declared.covers.contains(start))
    {
        // Publishers often point the start at a splash, such as their logo,
        // just ahead of the title page.
        if !has_text(doc, start) && start + 1 < count && has_text(doc, start + 1) {
            return start + 1;
        }
        return start;
    }
    let mut passed_cover = false;
    for index in 0..count {
        let Some(page) = page_look(doc, index) else {
            return index;
        };
        let linear = doc.spine.get(index).is_none_or(|item| item.linear);
        // A page that is one picture and nothing else is the cover, the
        // first time; after that it is the book itself, as in a comic.
        let cover =
            declared.covers.contains(&index) || (!page.text && page.pictures == 1 && !passed_cover);
        let blank = !page.text && page.pictures == 0;
        if linear && !page.jacket && !cover && !blank {
            return index;
        }
        passed_cover |= page.pictures > 0;
    }
    0
}

struct PageLook {
    text: bool,
    pictures: usize,
    /// The page Calibre writes ahead of a book with its catalogue entry.
    jacket: bool,
}

fn page_look(doc: &mut Epub, index: usize) -> Option<PageLook> {
    if !doc.set_current_chapter(index) {
        return None;
    }
    let (html, _mime) = doc.get_current_str()?;
    let lower = html.to_ascii_lowercase();
    let body = &lower[lower.find("<body").unwrap_or(0)..];
    let pictures = ["<img", "<image", "<svg:image"]
        .iter()
        .map(|tag| body.matches(tag).count())
        .sum();
    let document = HtmlToMarkdownConverter::new().convert(&html);
    Some(PageLook {
        text: search::lines_of(&document, 0)
            .iter()
            .any(|line| !line.text.trim().is_empty()),
        pictures,
        jacket: is_jacket(&html),
    })
}

/// What a book says about its own pages: where the text begins and which
/// page is the cover, from the EPUB 3 landmarks or the older OPF guide.
#[derive(Default)]
struct DeclaredPages {
    start: Option<usize>,
    covers: HashSet<usize>,
}

impl DeclaredPages {
    fn read(doc: &mut Epub) -> Self {
        let mut pages = Self::default();
        if let Some(nav) = doc
            .get_nav_id()
            .and_then(|id| doc.resources.get(&id))
            .map(|resource| resource.path.clone())
        {
            pages.take(doc, &nav, "a", "landmarks");
        }
        let opf = doc.root_file.clone();
        pages.take(doc, &opf, "reference", "");
        pages
    }

    /// Takes the start and covers named by the `tag` elements of the file at
    /// `path` (inside a `nav` of the kind `within`, when given), unless an
    /// earlier source already named a start.
    fn take(&mut self, doc: &mut Epub, path: &Path, tag: &str, within: &str) {
        let Some(xml) = doc.get_resource_str_by_path(path) else {
            log::error!("Failed to read {path:?} from the book");
            return;
        };
        let parsed = match roxmltree::Document::parse(&xml) {
            Ok(parsed) => parsed,
            Err(e) => {
                log::warn!("Failed to parse {path:?} for the book's landmarks: {e}");
                return;
            }
        };
        let kind = |node: roxmltree::Node| {
            node.attributes()
                .find(|attr| attr.name() == "type")
                .map(|attr| attr.value().to_ascii_lowercase())
                .unwrap_or_default()
        };
        let dir = path.parent().unwrap_or_else(|| Path::new(""));
        let mut start = None;
        for node in parsed.descendants() {
            if !node.is_element() || node.tag_name().name() != tag {
                continue;
            }
            if !within.is_empty()
                && !node.ancestors().any(|up| {
                    up.tag_name().name() == "nav"
                        && kind(up).split_whitespace().any(|k| k == within)
                })
            {
                continue;
            }
            let Some(href) = node.attributes().find(|attr| attr.name() == "href") else {
                continue;
            };
            let file = href.value().split('#').next().unwrap_or_default();
            let full = dto::normalize(&dir.join(dto::percent_decode(file)));
            let Some(index) = doc.resource_uri_to_chapter(&full) else {
                continue;
            };
            let kinds = kind(node);
            for k in kinds.split_whitespace() {
                match k {
                    "cover" => {
                        self.covers.insert(index);
                    }
                    "bodymatter" | "text" | "start" => {
                        start.get_or_insert(index);
                    }
                    _ => {}
                }
            }
        }
        if self.start.is_none() {
            self.start = start;
        }
    }
}

/// Calls `found` with the level and text of each heading in a block,
/// looking inside the wrappers a book sets around its sections.
fn headings_in(node: &bookokrat::markdown::Node, found: &mut impl FnMut(u8, String)) {
    match &node.block {
        Block::Heading { level, content } => {
            let text = search::plain_text(content);
            let title = text.split_whitespace().collect::<Vec<_>>().join(" ");
            if !title.is_empty() {
                found(level.as_u8(), title);
            }
        }
        Block::EpubBlock { content, .. } => {
            for inner in content {
                headings_in(inner, found);
            }
        }
        _ => {}
    }
}

/// Whether a page is the one Calibre writes ahead of a book with its
/// catalogue entry.
fn is_jacket(html: &str) -> bool {
    let lower = html.to_ascii_lowercase();
    let head = lower
        .find("<body")
        .map_or(lower.as_str(), |at| &lower[..at]);
    head.contains("calibre-content") && head.contains("\"jacket\"")
}

fn first_heading(document: &Document) -> Option<String> {
    document.blocks.iter().find_map(|node| match &node.block {
        Block::Heading { content, .. } => {
            let text = search::plain_text(content);
            let trimmed = text.trim();
            (!trimmed.is_empty()).then(|| trimmed.to_string())
        }
        _ => None,
    })
}

fn file_stem(path: &str) -> String {
    Path::new(path)
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string())
}

/// A file inside the open book, from a URL of the form `/<opening>/<path>`.
fn book_resource(app: &AppHandle, uri_path: &str) -> Option<(Vec<u8>, String)> {
    let decoded = dto::percent_decode(uri_path.trim_start_matches('/'));
    let (opening, path) = decoded.split_once('/')?;
    let state = app.state::<AppState>();
    let mut guard = state.book();
    let book = guard.as_mut().filter(|book| book.opening == opening)?;
    let bytes = book.doc.get_resource_by_path(path)?;
    let mime = book
        .doc
        .get_resource_mime_by_path(path)
        .unwrap_or_else(|| "application/octet-stream".to_string());
    Some((bytes, mime))
}

/// Where a book's scaled-down cover is kept. The name follows the file's
/// size and date as well as its path, so a replaced book gets a fresh cover.
fn cover_cache_path(app: &AppHandle, book: &Path) -> Result<PathBuf, String> {
    let meta = std::fs::metadata(book).map_err(|e| format!("Failed to read {book:?}: {e}"))?;
    let mut hasher = DefaultHasher::new();
    book.hash(&mut hasher);
    meta.len().hash(&mut hasher);
    meta.modified().ok().hash(&mut hasher);
    (COVER_MAX_WIDTH, COVER_MAX_HEIGHT).hash(&mut hasher);
    let dir = app
        .path()
        .app_cache_dir()
        .map_err(|e| format!("Could not determine the cache directory: {e}"))?;
    Ok(dir
        .join("covers")
        .join(format!("{:016x}.jpg", hasher.finish())))
}

fn store_cover(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    // Written beside its final name and moved into place, so a cover that is
    // being read is always a whole file.
    let partial = path.with_extension(format!("{}.part", std::process::id()));
    std::fs::write(&partial, bytes)?;
    std::fs::rename(&partial, path)
}

/// A PDF's first page, drawn as wide as a cover is shown.
fn first_page(book: &str) -> Result<image::DynamicImage, String> {
    let failed = |e: mupdf::Error| format!("Failed to draw the first page of {book}: {e}");
    let doc = mupdf::Document::open(book).map_err(failed)?;
    let page = doc.load_page(0).map_err(failed)?;
    let bounds = page.bounds().map_err(failed)?;
    let scale = COVER_MAX_WIDTH as f32 / (bounds.x1 - bounds.x0).max(1.0);
    let pixmap = page
        .to_pixmap(
            &mupdf::Matrix::new_scale(scale, scale),
            &mupdf::Colorspace::device_rgb(),
            false,
            false,
        )
        .map_err(failed)?;
    let (width, height) = (pixmap.width(), pixmap.height());
    let row = width as usize * 3;
    let stride = pixmap.stride() as usize;
    let mut pixels = Vec::with_capacity(row * height as usize);
    for line in pixmap.samples().chunks(stride).take(height as usize) {
        pixels.extend_from_slice(&line[..row]);
    }
    image::RgbImage::from_raw(width, height, pixels)
        .map(image::DynamicImage::ImageRgb8)
        .ok_or_else(|| format!("The first page of {book} came out the wrong size"))
}

/// A page of the open PDF as a PNG, from a URL of the form
/// `/<opening>/<stamp>/<page>/<width in pixels>`. The stamp only keeps one
/// look of the page apart from another in the web view's cache.
fn page_image(app: &AppHandle, uri_path: &str) -> Result<Vec<u8>, String> {
    let path = dto::percent_decode(uri_path.trim_start_matches('/'));
    let mut parts = path.split('/');
    let opening = parts.next().unwrap_or_default();
    let mut parts = parts.skip(1);
    let mut number = || parts.next().and_then(|part| part.parse::<usize>().ok());
    let (Some(page), Some(width)) = (number(), number()) else {
        return Err(format!("{uri_path} does not name a page"));
    };
    let handle = app
        .state::<AppState>()
        .pdf()
        .as_ref()
        .filter(|open| open.opening == opening)
        .map(|open| open.handle.clone())
        .ok_or_else(|| format!("{uri_path} is not a page of the open document"))?;
    let width = u16::try_from(width).map_err(|_| format!("{width} pixels is too wide"))?;
    pdf::png(&*handle.page(page, width)?)
}

/// The id of a book's cover image. The epub crate looks only for the EPUB 3
/// marking in an EPUB 3 book, but many name their cover the EPUB 2 way,
/// with `<meta name="cover">`, as readers accept.
fn cover_id<R: std::io::Read + std::io::Seek>(doc: &EpubDoc<R>) -> Option<String> {
    doc.get_cover_id().or_else(|| {
        let id = doc.mdata("cover")?.value.clone();
        let resource = doc.resources.get(&id)?;
        resource.mime.starts_with("image/").then_some(id)
    })
}

/// The cover image inside a book, as a JPEG small enough for the library.
fn book_cover(app: &AppHandle, uri_path: &str) -> Result<Vec<u8>, String> {
    let book = dto::percent_decode(uri_path.trim_start_matches('/'));
    let cached = cover_cache_path(app, Path::new(&book))?;
    if let Ok(bytes) = std::fs::read(&cached) {
        return Ok(bytes);
    }

    let image = if is_paged(&book) {
        first_page(&book)?
    } else {
        let mut doc = app.state::<AppState>().manager().load_epub(&book)?;
        let (data, _mime) = cover_id(&doc)
            .and_then(|id| doc.get_resource(&id))
            .ok_or_else(|| format!("{book} has no cover image"))?;
        image::load_from_memory(&data)
            .map_err(|e| format!("Failed to decode the cover of {book}: {e}"))?
    };
    let image = if image.width() > COVER_MAX_WIDTH || image.height() > COVER_MAX_HEIGHT {
        image.resize(COVER_MAX_WIDTH, COVER_MAX_HEIGHT, FilterType::Lanczos3)
    } else {
        image
    };
    let mut bytes = Vec::new();
    JpegEncoder::new_with_quality(&mut bytes, COVER_JPEG_QUALITY)
        .encode_image(&image.to_rgb8())
        .map_err(|e| format!("Failed to encode the cover of {book}: {e}"))?;

    if let Err(e) = store_cover(&cached, &bytes) {
        log::error!("Failed to keep the cover of {book} at {cached:?}: {e}");
    }
    Ok(bytes)
}

/// On NVIDIA's driver under Wayland, WebKitGTK's GPU renderer kills the app
/// with "Error 71 (Protocol error) dispatching to Wayland display" as soon as
/// it presents a frame. Turning off the driver's explicit sync avoids that
/// while keeping GPU rendering. Skipped when the user has set either knob.
#[cfg(target_os = "linux")]
fn work_around_nvidia_wayland_crash() {
    let user_configured = [
        "__NV_DISABLE_EXPLICIT_SYNC",
        "WEBKIT_DISABLE_DMABUF_RENDERER",
    ]
    .iter()
    .any(|name| std::env::var_os(name).is_some());
    let affected =
        std::env::var_os("WAYLAND_DISPLAY").is_some() && Path::new("/sys/module/nvidia").exists();
    if affected && !user_configured {
        // SAFETY: called first thing in main, before any other thread exists.
        unsafe { std::env::set_var("__NV_DISABLE_EXPLICIT_SYNC", "1") };
    }
}

fn init_logging() {
    let Ok(terminal_log) = bookokrat::library::resolve_log_path() else {
        return;
    };
    if let Ok(file) = File::create(terminal_log.with_file_name("reader.log")) {
        let _ = simplelog::WriteLogger::init(
            simplelog::LevelFilter::Info,
            simplelog::Config::default(),
            file,
        );
    }
}

fn main() {
    #[cfg(target_os = "linux")]
    work_around_nvidia_wayland_crash();
    // A test run is never handed to, or hands its book to, a reader in use.
    let testing = ["BOOKOKRAT_READER_PERF", "BOOKOKRAT_READER_SCRIPT"]
        .iter()
        .any(|name| std::env::var_os(name).is_some());
    let instance = if testing {
        None
    } else {
        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let target = std::env::args()
            .skip(1)
            .find(|a| !a.starts_with('-'))
            .map(|t| absolute(&cwd, &t));
        match instance::claim(target.as_deref()) {
            instance::Claim::First(instance) => instance,
            instance::Claim::Second => return,
        }
    };
    init_logging();
    let socket = instance.as_ref().map(|i| i.socket.clone());

    tauri::Builder::default()
        .manage(AppState::from_env())
        .setup(move |app| {
            if let Some(socket) = socket {
                instance::listen(socket, app.handle().clone());
            }
            Ok(())
        })
        .register_asynchronous_uri_scheme_protocol("bookimg", |ctx, request, responder| {
            let app = ctx.app_handle().clone();
            tauri::async_runtime::spawn_blocking(move || {
                let response = match book_resource(&app, request.uri().path()) {
                    Some((bytes, mime)) => tauri::http::Response::builder()
                        .header("Content-Type", mime)
                        .header("Cache-Control", "max-age=3600")
                        .body(bytes),
                    None => tauri::http::Response::builder()
                        .status(404)
                        .body(Vec::new()),
                };
                match response {
                    Ok(response) => responder.respond(response),
                    Err(e) => log::error!("Failed to build book resource response: {e}"),
                }
            });
        })
        .register_asynchronous_uri_scheme_protocol("bookcover", |ctx, request, responder| {
            let app = ctx.app_handle().clone();
            tauri::async_runtime::spawn_blocking(move || {
                let response = match book_cover(&app, request.uri().path()) {
                    Ok(bytes) => tauri::http::Response::builder()
                        .header("Content-Type", "image/jpeg")
                        .header("Cache-Control", "max-age=3600")
                        .body(bytes),
                    Err(e) => {
                        log::error!("No cover served for {}: {e}", request.uri().path());
                        tauri::http::Response::builder()
                            .status(404)
                            .body(Vec::new())
                    }
                };
                match response {
                    Ok(response) => responder.respond(response),
                    Err(e) => log::error!("Failed to build book cover response: {e}"),
                }
            });
        })
        .register_asynchronous_uri_scheme_protocol("bookpage", |ctx, request, responder| {
            let app = ctx.app_handle().clone();
            tauri::async_runtime::spawn_blocking(move || {
                let response = match page_image(&app, request.uri().path()) {
                    Ok(bytes) => tauri::http::Response::builder()
                        .header("Content-Type", "image/png")
                        .header("Cache-Control", "max-age=600")
                        .body(bytes),
                    Err(e) => {
                        log::info!("No page served for {}: {e}", request.uri().path());
                        tauri::http::Response::builder()
                            .status(503)
                            .body(Vec::new())
                    }
                };
                match response {
                    Ok(response) => responder.respond(response),
                    Err(e) => log::error!("Failed to build page response: {e}"),
                }
            });
        })
        .invoke_handler(tauri::generate_handler![
            launch_info,
            list_books,
            book_details,
            open_book,
            load_chapter,
            save_place,
            set_reading,
            annotations,
            add_annotation,
            edit_annotation,
            delete_annotation,
            marks,
            set_mark,
            delete_mark,
            add_page_annotation,
            set_page_mark,
            save_page_place,
            pdf_layer,
            pdf_look,
            pdf_watch,
            pdf_page_text,
            pdf_find_pages,
            synctex_inverse,
            search_book,
            book_stats,
            chapter_html,
            export_notes,
            open_in_viewer,
            reading_history,
            forget_history,
            keymap_problems,
            keymap,
            open_external,
            appearance,
            save_prefs,
            system_fonts,
            script_shot,
            script_note,
            script_mouse,
            lookup_targets,
            lookup_run,
            lookup_cancel,
            copy_text,
            quit,
            perf_report
        ])
        .on_window_event(|window, event| match event {
            // The page writes down the place in the book and any setting
            // still waiting to be saved, then quits. A page that does not
            // answer is not waited for long.
            tauri::WindowEvent::CloseRequested { api, .. } => {
                api.prevent_close();
                let app = window.app_handle().clone();
                if let Err(e) = window.emit("close-requested", ()) {
                    log::error!("Failed to tell the page the window is closing: {e}");
                    app.exit(0);
                    return;
                }
                std::thread::spawn(move || {
                    std::thread::sleep(CLOSE_GRACE);
                    app.exit(0);
                });
            }
            tauri::WindowEvent::Destroyed => {
                window.state::<AppState>().lookups.cancel_all();
            }
            _ => {}
        })
        .run(tauri::generate_context!())
        .expect("failed to start the reader");
    drop(instance);
}
