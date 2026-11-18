import { convertFileSrc, invoke } from '@tauri-apps/api/core';

export type Inline =
  | {
      t: 'text';
      s: string;
      style?: 'code' | 'em' | 'strong' | 'strike';
      script?: 'sup' | 'sub';
      /** How many characters the terminal app writes `script` text with. */
      chars?: number;
    }
  | {
      t: 'link';
      kind: 'external' | 'chapter' | 'anchor';
      url: string;
      chapter?: number;
      anchor?: string;
      /** For a link to a file that continues a chapter: which of its files. */
      part?: number;
      children: Inline[];
    }
  | { t: 'img'; src: string; alt: string }
  | { t: 'anchor'; id: string }
  | { t: 'br' }
  | { t: 'sbr' };

export type Cell = {
  header: boolean;
  rowspan: number;
  colspan: number;
  text?: Inline[];
  blocks?: Node[];
};

export type Block =
  | { t: 'h'; level: number; text: Inline[] }
  | { t: 'p'; text: Inline[] }
  | { t: 'code'; lang?: string; code: string }
  | { t: 'quote'; blocks: Node[] }
  | { t: 'list'; ordered: boolean; start: number; items: { id?: string; blocks: Node[] }[] }
  | { t: 'table'; header?: Cell[]; rows: Cell[][]; align: string[] }
  | { t: 'dl'; items: { term: Inline[]; defs: Node[][] }[] }
  | { t: 'epub'; kind: string; el: string; blocks: Node[] }
  | { t: 'hr' };

export type Node = Block & { id?: string };

export type TocEntry = {
  title: string;
  chapter: number | null;
  anchor: string | null;
  /** For an entry made from one of the book's headings: the chapter block. */
  node?: number;
  children: TocEntry[];
};

/** A place in a book: a chapter, the block at the top of the view, and how
    far down that block the view starts, as a fraction of its height. */
export type Place = { chapter: number; node: number; offset: number };

export type BookMeta = {
  path: string;
  /** Leads the URLs of this opening's images and pages. */
  opening: string;
  title: string;
  author: string | null;
  language: string | null;
  chapter_count: number;
  /** Whether the book carries a cover image. */
  cover: boolean;
  toc: TocEntry[];
  /** Where the book was left, if it has been read before. */
  place: Place | null;
  /** Set for a PDF or DjVu document, whose "chapters" are its pages. */
  pdf: PdfMeta | null;
};

export type PdfMeta = {
  /** Each page's width and height in the document's own units. */
  pages: [number, number][];
  /** Printed page number minus page position, where it could be told. */
  number_offset: number | null;
  /** Where the document was left: the page, and how far down it. */
  view: { page: number; offset: number };
  themed: boolean;
  tint_images: boolean;
  /** Whether TeX left the data to jump between the PDF and its source. */
  synctex: boolean;
  djvu: boolean;
};

/** How a PDF's pages are coloured; colours are 0xRRGGBB. */
export type PdfLook = { themed: boolean; paper: number; ink: number; tint_images: boolean };

/** One line of text on a PDF page. Positions are fractions of the page. */
export type PdfLine = {
  x0: number;
  y0: number;
  x1: number;
  y1: number;
  block: number;
  text: string;
  /** Where each character of `text` starts, left to right. */
  xs: number[];
};

export type PdfLink = { x0: number; y0: number; x1: number; y1: number; page?: number; url?: string };

export type PdfLayer = { page: number; lines: PdfLine[]; links: PdfLink[] };

/** A point on a PDF page, as fractions of its width and height. */
export type PdfSpot = { page: number; x: number; y: number };

/** A place the whole-book search found. */
export type SearchHit = {
  /** The chapter, or the page of a PDF. */
  section: number;
  title: string;
  /** The chapter block, or the line on a PDF page. */
  node: number;
  /** On a PDF page: the top and bottom of the line, as fractions of its height. */
  span?: [number, number];
  snippet: string;
  /** The characters of `snippet` that matched. */
  marks: number[];
};

export type BookStats = {
  unit: 'words' | 'pages';
  rows: { title: string; chapter: number; count: number }[];
};

export type HistoryEntry = {
  path: string;
  title: string;
  author: string | null;
  progress: number | null;
  last_read: string;
  exists: boolean;
  here: boolean;
  source: string;
  key: string;
};

/** A chapter as the reader shows it. A book may spread one over several
    files; `parts` is the block each of them starts at. */
export type Chapter = { index: number; title: string; parts: number[]; blocks: Node[] };

export type BookSummary = { path: string; title: string; format: string };

export type BookDetails = { title: string | null; author: string | null; cover: boolean };

export type LaunchInfo = {
  library_dir: string;
  open_path: string | null;
  perf: boolean;
  script: unknown[] | null;
};

export type ReaderPrefs = {
  theme: string | null;
  font: string;
  font_size: number;
  line_spacing: number;
  measure: number;
  justify: boolean;
  drop_caps: boolean;
  nav_width: number;
  library_sort: 'name' | 'recent' | 'type';
  pdf_spread: boolean;
  pdf_zoom: number;
  pdf_paged: boolean;
  zen_border: boolean;
};

export type Appearance = {
  prefs: ReaderPrefs;
  terminal_theme: string | null;
  custom_themes: { name: string; colors: string[] }[];
  warnings: string[];
};

export type HighlightColor = 'red' | 'green' | 'blue' | 'yellow' | 'purple';

/** One stretch of a highlight or note: a paragraph or heading of the
    chapter, found by its place among the chapter's blocks and then down
    through any wrapping blocks, and a range of characters in it. */
export type Slice = { node: number; path: number[]; start: number; end: number | null };

/** The share of a new highlight or note that lies in one of the book's files. */
export type Piece = { slices: Slice[]; quote: string | null };

/** A highlight, when it has a colour, or else a note. */
export type Annotation = {
  id: string;
  chapter: number;
  color: HighlightColor | null;
  text: string;
  quote: string | null;
  updated: string;
  slices: Slice[];
  /** False for one the terminal app put on a list, quotation or code,
      which the reader lists but cannot draw. */
  drawable: boolean;
  /** On a PDF page (`chapter` is then the page): the boxes covered, as
      fractions of the page (left, top, right, bottom). */
  rects?: [number, number, number, number][];
};

/** A lettered bookmark. Lowercase ones belong to the open book; uppercase
    ones are shared by every library and may point into another book. */
export type Mark = {
  letter: string;
  path: string;
  here: boolean;
  chapter: number | null;
  node: number | null;
  offset: number | null;
  snippet: string | null;
  chapter_title: string | null;
};

export type Binding = { keys: string[]; action: unknown };

export type LookupDisplay = 'popup' | 'fire_and_forget';

export type LookupTarget = { name: string; display: LookupDisplay };

export type LookupTargets = { targets: LookupTarget[]; fallback: LookupDisplay | null };

export type LookupOutcome =
  | { kind: 'launched' }
  | { kind: 'output'; ok: boolean; text: string }
  | { kind: 'cancelled' };

export const api = {
  launchInfo: () => invoke<LaunchInfo>('launch_info'),
  listBooks: () => invoke<BookSummary[]>('list_books'),
  bookDetails: (path: string) => invoke<BookDetails>('book_details', { path }),
  /** `paper` and `ink` are the theme's page and text colours, for a PDF. */
  openBook: (path: string, paper: number, ink: number) =>
    invoke<BookMeta>('open_book', { path, paper, ink }).then((meta) => {
      opening = meta.opening;
      return meta;
    }),
  loadChapter: (index: number) => invoke<Chapter>('load_chapter', { index }),
  /** `progress` is how far through the whole book the place is, from 0 to 1. */
  savePlace: (place: Place, progress: number) => invoke<void>('save_place', { place, progress }),
  /** Whether the open book is on screen; decides what the next start shows. */
  setReading: (reading: boolean) => invoke<void>('set_reading', { reading }),
  annotations: () => invoke<Annotation[]>('annotations'),
  /** Adds a highlight when given a colour, a note otherwise: one for each
      piece, or none at all if any of them cannot be made. */
  addAnnotation: (chapter: number, pieces: Piece[], color: HighlightColor | null, text: string) =>
    invoke<Annotation[]>('add_annotation', { chapter, pieces, color, text }),
  editAnnotation: (id: string, change: { text?: string; color?: HighlightColor }) =>
    invoke<Annotation[]>('edit_annotation', { id, text: change.text, color: change.color }),
  deleteAnnotation: (id: string) => invoke<Annotation[]>('delete_annotation', { id }),
  marks: () => invoke<Mark[]>('marks'),
  setMark: (
    letter: string,
    place: Place,
    at: { offset: number | null; snippet: string | null; chapterTitle: string },
    progress: number,
  ) =>
    invoke<Mark[]>('set_mark', {
      letter,
      place,
      offset: at.offset,
      snippet: at.snippet,
      chapterTitle: at.chapterTitle,
      progress,
    }),
  deleteMark: (letter: string) => invoke<Mark[]>('delete_mark', { letter }),
  addPageAnnotation: (
    page: number,
    rects: [number, number, number, number][],
    color: HighlightColor | null,
    text: string,
    quote: string | null,
  ) => invoke<Annotation[]>('add_page_annotation', { page, rects, color, text, quote }),
  setPageMark: (
    letter: string,
    page: number,
    at: { line: number | null; snippet: string | null; offset: number },
  ) => invoke<Mark[]>('set_page_mark', { letter, page, ...at }),
  savePagePlace: (page: number, offset: number, themed: boolean, tintImages: boolean) =>
    invoke<void>('save_page_place', { page, offset, themed, tintImages }),
  pdfLayer: (page: number, width: number) => invoke<PdfLayer>('pdf_layer', { page, width }),
  pdfLook: (look: PdfLook) => invoke<void>('pdf_look', { look }),
  pdfWatch: (on: boolean) => invoke<void>('pdf_watch', { on }),
  pdfPageText: (page: number) => invoke<string>('pdf_page_text', { page }),
  /** The pages some text occurs on. */
  pdfFindPages: (query: string) => invoke<number[]>('pdf_find_pages', { query }),
  /** Opens the TeX source for a point; resolves to a line for the status bar. */
  synctexInverse: (spot: PdfSpot) => invoke<string>('synctex_inverse', { spot }),
  searchBook: (query: string) => invoke<SearchHit[]>('search_book', { query }),
  bookStats: () => invoke<BookStats>('book_stats'),
  chapterHtml: (index: number) => invoke<string>('chapter_html', { index }),
  /** Writes the notes as Markdown beside the book; resolves to the file. */
  exportNotes: (chapter: number) => invoke<string>('export_notes', { chapter }),
  openInViewer: (chapter: number) => invoke<void>('open_in_viewer', { chapter }),
  readingHistory: () => invoke<HistoryEntry[]>('reading_history'),
  forgetHistory: (source: string, key: string) =>
    invoke<HistoryEntry[]>('forget_history', { source, key }),
  /** Reads keybindings.toml again; resolves to what is wrong with it. */
  keymapProblems: () => invoke<string[]>('keymap_problems'),
  keymap: () => invoke<Record<string, Binding[]>>('keymap'),
  openExternal: (url: string) => invoke<void>('open_external', { url }),
  perfReport: (report: unknown) => invoke<void>('perf_report', { report }),
  appearance: () => invoke<Appearance>('appearance'),
  savePrefs: (prefs: ReaderPrefs) => invoke<void>('save_prefs', { prefs }),
  systemFonts: () => invoke<string[]>('system_fonts'),
  scriptShot: (name: string) => invoke<void>('script_shot', { name }),
  /** Sends a real mouse event to a point of the page: one that can
      select text. */
  scriptMouse: (kind: 'press' | 'move' | 'hover' | 'release', x: number, y: number) =>
    invoke<void>('script_mouse', { kind, x, y, ratio: devicePixelRatio }),
  /** Writes text to a file beside the replay script, for a check to read. */
  scriptNote: (name: string, text: string) => invoke<void>('script_note', { name, text }),
  lookupTargets: () => invoke<LookupTargets>('lookup_targets'),
  /** Runs `target`, or the single `lookup_command` when it is null. */
  lookupRun: (id: number, target: string | null, text: string) =>
    invoke<LookupOutcome>('lookup_run', { id, target, text }),
  lookupCancel: (id: number) => invoke<void>('lookup_cancel', { id }),
  copyText: (text: string) => invoke<void>('copy_text', { text }),
  quit: () => invoke<void>('quit'),
};

/** The opening of the book now open, which leads its resource URLs so
    that the web view's cache never mixes up two books' pictures. */
let opening = '';

/** URL for a resource inside the open book, served by the backend. */
export function bookResource(path: string): string {
  if (path.startsWith('data:')) return path;
  return convertFileSrc(`${opening}/${path}`, 'bookimg');
}

/** URL for the cover image that comes inside a book, scaled for the library. */
export function bookCover(path: string): string {
  return convertFileSrc(path, 'bookcover');
}

/** URL for a page of the open PDF drawn `width` pixels wide. `stamp` tells
    one look of the page (theme, a reloaded file) from another. */
export function pdfPage(stamp: string, page: number, width: number): string {
  return convertFileSrc(`${opening}/${stamp}/${page}/${width}`, 'bookpage');
}
