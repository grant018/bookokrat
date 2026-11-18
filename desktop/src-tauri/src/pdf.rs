//! PDF and DjVu documents. Pages are drawn by the core's render service,
//! the one the terminal app uses, so theming, DjVu and reloading a file
//! that changed on disk behave the same in both. The service thinks in
//! terminal cells; it is given cells one pixel square and a view one pixel
//! high, which makes it fit every page to the width asked for.

use crate::search::{self, Hit};
use bookokrat::pdf::synctex::{SyncTexCommand, SyncTexListener, SyncTexScanner};
use bookokrat::pdf::{
    CellSize, Command, LinkTarget, PageData, RenderResponse, RenderService, RequestId, TocEntry,
    TocTarget,
};
use bookokrat::search_engine::SearchLine;
use image::ImageEncoder;
use image::codecs::png::{CompressionType, FilterType, PngEncoder};
use ratatui::layout::Rect;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::time::Duration;

const WORKERS: usize = 3;
const CACHED_PAGES: usize = 24;
/// Pages waiting to be drawn beyond this many are dropped, oldest first:
/// they were asked for on the way past and are no longer on screen.
const QUEUE_LIMIT: usize = 12;

/// How pages are coloured. `paper` and `ink` are the theme's page and text
/// colours as 0xRRGGBB; with `themed` off the document keeps its own.
#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
pub struct Look {
    pub themed: bool,
    pub paper: i32,
    pub ink: i32,
    /// Whether pictures take the theme's colours too.
    pub tint_images: bool,
}

impl Look {
    fn colors(&self) -> (i32, i32) {
        if self.themed {
            (self.paper, self.ink)
        } else {
            (-1, -1)
        }
    }
}

type PageReply = mpsc::Sender<Result<Arc<PageData>, String>>;

enum Cmd {
    Page {
        page: usize,
        width: u16,
        reply: PageReply,
    },
    Look(Look),
    Watch(bool),
}

/// A watched file as it is after being read again.
pub struct Reloaded {
    pub pages: Vec<(f32, f32)>,
    pub toc: Vec<TocEntry>,
    pub number_offset: Option<i32>,
}

/// A way to ask the open document for pages from any thread.
#[derive(Clone)]
pub struct Handle {
    tx: mpsc::Sender<Cmd>,
}

impl Handle {
    /// A page drawn `width` pixels wide. Waits for it to be drawn.
    pub fn page(&self, page: usize, width: u16) -> Result<Arc<PageData>, String> {
        let (reply, answer) = mpsc::channel();
        self.tx
            .send(Cmd::Page { page, width, reply })
            .map_err(|_| "The document was closed".to_string())?;
        answer
            .recv_timeout(Duration::from_secs(30))
            .map_err(|_| format!("Page {} took too long to draw", page + 1))?
    }

    pub fn set_look(&self, look: Look) {
        let _ = self.tx.send(Cmd::Look(look));
    }

    pub fn set_watching(&self, on: bool) {
        let _ = self.tx.send(Cmd::Watch(on));
    }
}

pub struct OpenPdf {
    pub path: String,
    /// Leads the URLs of this opening's pages.
    pub opening: String,
    pub title: String,
    pub author: Option<String>,
    /// Each page's width and height in the document's own units (points).
    pub pages: Vec<(f32, f32)>,
    pub toc: Vec<TocEntry>,
    /// What the page numbers printed in the document differ from its page
    /// positions by, when that could be told.
    pub number_offset: Option<i32>,
    pub handle: Handle,
    pub synctex: Option<Arc<SyncTexScanner>>,
    _synctex_listener: Option<SyncTexListener>,
    engine: Option<search::Index>,
    /// Each page's text in lower case with its spacing evened out, read
    /// the first time `/` looks for something.
    folded: Option<Vec<String>>,
    /// Counts reloads, so that text read before one is not kept after it.
    text_version: u64,
}

/// What reading a document's text takes, so that it can be read without
/// holding the open document, which pages are drawn from meanwhile.
pub struct TextSource {
    path: String,
    pages: usize,
    heights: Vec<f32>,
    opening: String,
    version: u64,
}

impl TextSource {
    /// Every page's text as lines to search.
    pub fn lines(&self) -> Vec<(usize, Vec<SearchLine>)> {
        let path = Path::new(&self.path);
        if is_djvu(path) {
            let Ok(doc) = rdjvu::Document::open(path)
                .inspect_err(|e| log::error!("Failed to open {path:?} for search: {e}"))
            else {
                return Vec::new();
            };
            return (0..doc.page_count())
                .map(|index| {
                    let text = doc
                        .page(index)
                        .ok()
                        .and_then(|page| page.text_layer().ok().flatten())
                        .map(|layer| layer.text)
                        .unwrap_or_default();
                    let lines = text
                        .lines()
                        .enumerate()
                        .filter(|(_, line)| !line.trim().is_empty())
                        .map(|(node_index, line)| SearchLine {
                            text: line.trim_end().to_string(),
                            node_index,
                            y_bounds: None,
                        })
                        .collect();
                    (index, lines)
                })
                .collect();
        }
        let Ok(doc) = mupdf::Document::open(&self.path)
            .inspect_err(|e| log::error!("Failed to open {path:?} for search: {e}"))
        else {
            return Vec::new();
        };
        (0..self.pages)
            .map(|index| (index, pdf_lines(&doc, index)))
            .collect()
    }

    /// Each page's text the way `/` compares it.
    pub fn folded(&self) -> Vec<String> {
        let mut pages = vec![String::new(); self.pages];
        for (index, lines) in self.lines() {
            let text: Vec<&str> = lines.iter().map(|line| line.text.as_str()).collect();
            if let Some(page) = pages.get_mut(index) {
                *page = fold(&text.join(" "));
            }
        }
        pages
    }

    /// Runs a search on an index made from this text.
    pub fn search(&self, index: &search::Index, query: &str) -> Vec<Hit> {
        index.run(query, Some(&self.heights))
    }
}

/// Text as `/` compares it: lower case, typographic quotes and dashes as
/// the plain keys, any run of spacing as one space.
fn fold(text: &str) -> String {
    let plain: String = text
        .chars()
        .map(|ch| match ch {
            '‘' | '’' => '\'',
            '“' | '”' | '„' => '"',
            '‐' | '‑' | '–' | '—' => '-',
            other => other,
        })
        .collect();
    plain
        .to_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn is_djvu(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| matches!(ext.to_ascii_lowercase().as_str(), "djvu" | "djv"))
}

/// The size of every page, without drawing any.
fn page_sizes(path: &Path) -> Result<Vec<(f32, f32)>, String> {
    if is_djvu(path) {
        let doc =
            rdjvu::Document::open(path).map_err(|e| format!("Failed to open {path:?}: {e}"))?;
        return Ok((0..doc.page_count())
            .map(|index| match doc.page(index) {
                Ok(page) => (page.display_width() as f32, page.display_height() as f32),
                Err(e) => {
                    log::error!("Failed to read page {index} of {path:?}: {e}");
                    (612.0, 792.0)
                }
            })
            .collect());
    }
    let doc = mupdf::Document::open(path.to_string_lossy().as_ref())
        .map_err(|e| format!("Failed to open {path:?}: {e}"))?;
    let count = doc
        .page_count()
        .map_err(|e| format!("Failed to count the pages of {path:?}: {e}"))?;
    Ok((0..count)
        .map(|index| {
            let bounds = doc.load_page(index).and_then(|page| page.bounds());
            match bounds {
                Ok(b) if b.x1 > b.x0 && b.y1 > b.y0 => (b.x1 - b.x0, b.y1 - b.y0),
                Ok(_) => (612.0, 792.0),
                Err(e) => {
                    log::error!("Failed to measure page {index} of {path:?}: {e}");
                    (612.0, 792.0)
                }
            }
        })
        .collect())
}

/// The most common difference between printed page numbers and positions.
fn number_offset(samples: &[(usize, i32)]) -> Option<i32> {
    let mut counts: HashMap<i32, usize> = HashMap::new();
    for (page, printed) in samples {
        *counts.entry(printed - *page as i32).or_default() += 1;
    }
    counts
        .into_iter()
        .filter(|(_, count)| *count >= 2)
        .max_by_key(|(_, count)| *count)
        .map(|(offset, _)| offset)
}

struct Waiting {
    page: usize,
    width: u16,
    reply: PageReply,
}

/// Owns the render service: takes requests, hands out finished pages.
fn pump(
    path: PathBuf,
    look: Look,
    rx: mpsc::Receiver<Cmd>,
    ready: mpsc::Sender<Result<bookokrat::pdf::DocumentInfo, String>>,
    on_reload: impl Fn(Reloaded),
) {
    let (black, white) = look.colors();
    let mut service = RenderService::with_config(
        path.clone(),
        CellSize::new(1, 1),
        black,
        white,
        WORKERS,
        CACHED_PAGES,
        0,
    );
    let mut tinted = true;
    if !look.tint_images {
        service.apply_command(Command::ToggleInvertImages);
        tinted = false;
    }
    match service.document_info() {
        Some(info) => {
            let _ = ready.send(Ok(info.clone()));
        }
        None => {
            let _ = ready.send(Err(format!("Failed to open {path:?}")));
            return;
        }
    }

    let mut width = 0u16;
    // Newest last; pages are drawn newest first, since those are the ones
    // on screen now.
    let mut queue: Vec<Waiting> = Vec::new();
    let mut drawing: HashMap<RequestId, usize> = HashMap::new();
    let mut waiting: Vec<Waiting> = Vec::new();
    let fail = |list: &mut Vec<Waiting>, why: &str| {
        for item in list.drain(..) {
            let _ = item.reply.send(Err(why.to_string()));
        }
    };

    loop {
        let idle = queue.is_empty() && waiting.is_empty();
        let pause = Duration::from_millis(if idle { 100 } else { 2 });
        match rx.recv_timeout(pause) {
            Ok(Cmd::Page {
                page,
                width: wanted,
                reply,
            }) => queue.push(Waiting {
                page,
                width: wanted,
                reply,
            }),
            Ok(Cmd::Look(next)) => {
                fail(&mut queue, "The page colours changed");
                fail(&mut waiting, "The page colours changed");
                drawing.clear();
                let (black, white) = next.colors();
                service.apply_command(Command::SetColors { black, white });
                if next.tint_images != tinted {
                    service.apply_command(Command::ToggleInvertImages);
                    tinted = next.tint_images;
                }
            }
            Ok(Cmd::Watch(on)) => {
                if on {
                    service.enable_watching();
                } else {
                    service.disable_watching();
                }
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }

        while queue.len() > QUEUE_LIMIT {
            let dropped = queue.remove(0);
            let _ = dropped
                .reply
                .send(Err("Skipped: the page is no longer on screen".to_string()));
        }
        while drawing.len() < WORKERS {
            let Some(next) = queue.last() else { break };
            if next.width != width {
                // The service draws at one width at a time; pages on
                // their way at the old one are let finish first.
                if !waiting.is_empty() {
                    break;
                }
                width = next.width;
                drawing.clear();
                service.apply_command(Command::SetArea(Rect::new(0, 0, width, 1)));
            }
            let Some(next) = queue.pop() else { break };
            if let Some(data) = service.get_cached_page(next.page) {
                let _ = next.reply.send(Ok(data));
                continue;
            }
            if !waiting.iter().any(|w| w.page == next.page) {
                let id = service.request_page(next.page);
                drawing.insert(id, next.page);
            }
            waiting.push(next);
        }

        for response in service.poll_responses() {
            match response {
                RenderResponse::Page { id, page, data, .. } => {
                    drawing.remove(&id);
                    waiting.retain(|w| {
                        if w.page == page {
                            let _ = w.reply.send(Ok(data.clone()));
                        }
                        w.page != page
                    });
                }
                RenderResponse::Error { id, error } => {
                    let Some(page) = drawing.remove(&id) else {
                        log::error!("The render service reported: {error}");
                        continue;
                    };
                    log::error!("Failed to draw page {page} of {path:?}: {error}");
                    let message = format!("Failed to draw page {}: {error}", page + 1);
                    waiting.retain(|w| {
                        if w.page == page {
                            let _ = w.reply.send(Err(message.clone()));
                        }
                        w.page != page
                    });
                }
                RenderResponse::Reloaded { .. } => {
                    fail(&mut queue, "The document was reloaded");
                    fail(&mut waiting, "The document was reloaded");
                    drawing.clear();
                    let info = service.document_info();
                    match page_sizes(&path) {
                        Ok(pages) => on_reload(Reloaded {
                            pages,
                            toc: info.map(|info| info.toc.clone()).unwrap_or_default(),
                            number_offset: info
                                .and_then(|info| number_offset(&info.page_number_samples)),
                        }),
                        Err(e) => log::error!("Failed to measure the reloaded document: {e}"),
                    }
                }
                _ => {}
            }
        }
    }
}

impl OpenPdf {
    /// Opens a document. `on_reload` runs when a watched file changes on
    /// disk; `on_forward` when an editor asks to see a source line.
    pub fn open(
        path: &str,
        opening: String,
        look: Look,
        on_reload: impl Fn(Reloaded) + Send + 'static,
        on_forward: impl Fn(String, u32) + Send + 'static,
    ) -> Result<Self, String> {
        let file = PathBuf::from(path);
        let pages = page_sizes(&file)?;
        if pages.is_empty() {
            return Err(format!("{path} has no pages"));
        }
        let (tx, rx) = mpsc::channel();
        let (ready_tx, ready) = mpsc::channel();
        let pump_path = file.clone();
        std::thread::Builder::new()
            .name("pdf-pages".into())
            .spawn(move || pump(pump_path, look, rx, ready_tx, on_reload))
            .map_err(|e| format!("Failed to start the page renderer: {e}"))?;
        let info = ready
            .recv()
            .map_err(|_| format!("Failed to open {path}"))??;

        let title = info.title.unwrap_or_else(|| {
            file.file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| path.to_string())
        });

        let (synctex, listener) = Self::synctex(&file, on_forward);
        Ok(Self {
            path: path.to_string(),
            opening,
            title,
            author: info.author,
            pages,
            toc: info.toc,
            number_offset: number_offset(&info.page_number_samples),
            handle: Handle { tx },
            synctex,
            _synctex_listener: listener,
            engine: None,
            folded: None,
            text_version: 0,
        })
    }

    /// Loads the SyncTeX data beside a PDF made by TeX, and listens for
    /// the editor asking to be shown a line (`bookokrat --synctex-forward`).
    fn synctex(
        file: &Path,
        on_forward: impl Fn(String, u32) + Send + 'static,
    ) -> (Option<Arc<SyncTexScanner>>, Option<SyncTexListener>) {
        let Some(sidecar) = SyncTexScanner::find_synctex_file(file) else {
            return (None, None);
        };
        let scanner = match SyncTexScanner::open(&sidecar) {
            Ok(scanner) => Arc::new(scanner),
            Err(e) => {
                log::error!("Failed to load SyncTeX data from {sidecar:?}: {e}");
                return (None, None);
            }
        };
        let (tx, rx) = flume::unbounded();
        let socket = bookokrat::pdf::synctex::synctex_socket_path(file);
        let listener = match SyncTexListener::start(socket.clone(), tx) {
            Ok(listener) => Some(listener),
            Err(e) => {
                log::error!("Failed to listen for SyncTeX on {socket:?}: {e}");
                None
            }
        };
        if listener.is_some() {
            let spawned = std::thread::Builder::new()
                .name("synctex".into())
                .spawn(move || {
                    for SyncTexCommand::Forward { file, line, .. } in rx {
                        on_forward(file, line);
                    }
                });
            if let Err(e) = spawned {
                log::error!("Failed to start the SyncTeX thread: {e}");
            }
        }
        (Some(scanner), listener)
    }

    pub fn is_djvu(&self) -> bool {
        is_djvu(Path::new(&self.path))
    }

    /// Where a line of the TeX source ended up in the document.
    pub fn forward(&self, file: &str, line: u32) -> Option<Spot> {
        let found = self.synctex.as_ref()?.forward_search(file, line, 0)?;
        let page = found.page.checked_sub(1)?;
        let (width, height) = *self.pages.get(page)?;
        let x = (found.h + found.width * 0.5) as f32 / width;
        let y = (found.v - found.height * 0.5).max(0.0) as f32 / height;
        Some(Spot {
            page,
            x: x.clamp(0.0, 1.0),
            y: y.clamp(0.0, 1.0),
        })
    }

    /// The source file and line that a point in the document was set from.
    pub fn inverse(&self, spot: Spot) -> Option<(String, u32)> {
        let (width, height) = *self.pages.get(spot.page)?;
        let found = self.synctex.as_ref()?.inverse_search(
            spot.page + 1,
            f64::from(spot.x * width),
            f64::from(spot.y * height),
        )?;
        Some((found.file, found.line))
    }

    /// Takes on what a reloaded file now holds.
    pub fn reloaded(&mut self, now: Reloaded) {
        self.pages = now.pages;
        self.toc = now.toc;
        self.number_offset = now.number_offset;
        self.engine = None;
        self.folded = None;
        self.text_version += 1;
    }

    pub fn text_source(&self) -> TextSource {
        TextSource {
            path: self.path.clone(),
            pages: self.pages.len(),
            heights: self.pages.iter().map(|(_, height)| *height).collect(),
            opening: self.opening.clone(),
            version: self.text_version,
        }
    }

    fn holds(&self, source: &TextSource) -> bool {
        source.opening == self.opening && source.version == self.text_version
    }

    /// The pages some text occurs on, or None while the pages' text has
    /// not been read yet.
    pub fn pages_with(&self, query: &str) -> Option<Vec<usize>> {
        let needle = fold(query);
        if needle.is_empty() {
            return Some(Vec::new());
        }
        let folded = self.folded.as_ref()?;
        Some(
            folded
                .iter()
                .enumerate()
                .filter(|(_, text)| text.contains(&needle))
                .map(|(index, _)| index)
                .collect(),
        )
    }

    /// Keeps the pages' text read from `source`, unless the document has
    /// changed since.
    pub fn keep_folded(&mut self, source: &TextSource, folded: Vec<String>) {
        if self.holds(source) {
            self.folded = Some(folded);
        }
    }

    /// The places a search finds, or None while there is no index yet.
    pub fn search(&self, query: &str) -> Option<Vec<Hit>> {
        let heights: Vec<f32> = self.pages.iter().map(|(_, height)| *height).collect();
        Some(self.engine.as_ref()?.run(query, Some(&heights)))
    }

    /// Keeps an index made from `source`, unless the document has changed since.
    pub fn keep_index(&mut self, source: &TextSource, index: search::Index) {
        if self.holds(source) {
            self.engine = Some(index);
        }
    }

    /// The text of one page, line by line.
    pub fn page_text(&self, page: usize) -> Result<String, String> {
        let lines = if self.is_djvu() {
            let doc = rdjvu::Document::open(Path::new(&self.path))
                .map_err(|e| format!("Failed to open {}: {e}", self.path))?;
            return Ok(doc
                .page(page)
                .ok()
                .and_then(|page| page.text_layer().ok().flatten())
                .map(|layer| layer.text)
                .unwrap_or_default());
        } else {
            let doc = mupdf::Document::open(&self.path)
                .map_err(|e| format!("Failed to open {}: {e}", self.path))?;
            pdf_lines(&doc, page)
        };
        Ok(lines
            .into_iter()
            .map(|line| line.text)
            .collect::<Vec<_>>()
            .join("\n"))
    }
}

fn pdf_lines(doc: &mupdf::Document, page: usize) -> Vec<SearchLine> {
    use mupdf::TextPageFlags;
    use mupdf::text_page::TextBlockType;
    let Ok(text) = doc
        .load_page(page as i32)
        .and_then(|page| page.to_text_page(TextPageFlags::empty()))
    else {
        return Vec::new();
    };
    let mut lines = Vec::new();
    let mut index = 0;
    for block in text.blocks() {
        if block.r#type() != TextBlockType::Text {
            continue;
        }
        for line in block.lines() {
            let bounds = line.bounds();
            let text: String = line.chars().filter_map(|ch| ch.char()).collect();
            if !text.trim().is_empty() {
                lines.push(SearchLine {
                    text,
                    node_index: index,
                    y_bounds: Some((bounds.y0, bounds.y1)),
                });
            }
            index += 1;
        }
    }
    lines
}

/// A drawn page as a PNG. Fast compression: the picture travels a few
/// centimetres, and is drawn again whenever the zoom changes.
pub fn png(data: &PageData) -> Result<Vec<u8>, String> {
    let image = &data.img_data;
    let color = match image.channels {
        3 => image::ExtendedColorType::Rgb8,
        4 => image::ExtendedColorType::Rgba8,
        n => return Err(format!("A page came back with {n} colour channels")),
    };
    let mut out = Vec::with_capacity(image.pixels.len() / 8);
    PngEncoder::new_with_quality(&mut out, CompressionType::Fast, FilterType::Up)
        .write_image(&image.pixels, image.width_px, image.height_px, color)
        .map_err(|e| format!("Failed to encode page {}: {e}", data.page_num + 1))?;
    Ok(out)
}

#[derive(Serialize)]
pub struct LineDto {
    pub x0: f32,
    pub y0: f32,
    pub x1: f32,
    pub y1: f32,
    pub block: usize,
    pub text: String,
    /// Where each character of `text` starts, left to right.
    pub xs: Vec<f32>,
}

#[derive(Serialize)]
pub struct LinkDto {
    pub x0: f32,
    pub y0: f32,
    pub x1: f32,
    pub y1: f32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub page: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
}

/// The text and links of a drawn page. Positions are fractions of the
/// page's width and height, so they hold at any zoom.
#[derive(Serialize)]
pub struct PageLayer {
    pub page: usize,
    pub lines: Vec<LineDto>,
    pub links: Vec<LinkDto>,
}

pub fn layer(data: &PageData) -> PageLayer {
    let width = data.img_data.width_px.max(1) as f32;
    let height = data.img_data.height_px.max(1) as f32;
    PageLayer {
        page: data.page_num,
        lines: data
            .line_bounds
            .iter()
            .filter(|line| !line.chars.is_empty())
            .map(|line| LineDto {
                x0: line.x0 / width,
                y0: line.y0 / height,
                x1: line.x1 / width,
                y1: line.y1 / height,
                block: line.block_id,
                text: line.chars.iter().map(|ch| ch.c).collect(),
                xs: line.chars.iter().map(|ch| ch.x / width).collect(),
            })
            .collect(),
        links: data
            .link_rects
            .iter()
            .map(|link| {
                let (page, url) = match &link.target {
                    LinkTarget::Internal { page } => (Some(*page), None),
                    LinkTarget::External { uri } => (None, Some(uri.clone())),
                };
                LinkDto {
                    x0: link.x0 as f32 / width,
                    y0: link.y0 as f32 / height,
                    x1: link.x1 as f32 / width,
                    y1: link.y1 as f32 / height,
                    page,
                    url,
                }
            })
            .collect(),
    }
}

/// The page a contents entry leads to, if it leads into the document.
pub fn toc_page(entry: &TocEntry, pages: usize, number_offset: Option<i32>) -> Option<usize> {
    let page = match &entry.target {
        TocTarget::InternalPage(page) => *page as i64,
        TocTarget::PrintedPage(printed) => *printed as i64 - i64::from(number_offset.unwrap_or(1)),
        TocTarget::External(_) => return None,
    };
    (page >= 0 && (page as usize) < pages).then_some(page as usize)
}

/// A point on a page, as fractions of its width and height.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Spot {
    pub page: usize,
    pub x: f32,
    pub y: f32,
}

/// Where the document was left, kept beside the terminal app's own record.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct View {
    pub page: usize,
    /// How far down the page the view starts, as a fraction of its height.
    pub offset: f32,
}

pub type Shared = Mutex<Option<OpenPdf>>;
