<script lang="ts">
  import { listen } from '@tauri-apps/api/event';
  import { tick } from 'svelte';
  import {
    api,
    type Annotation,
    type BookDetails,
    type BookMeta,
    type BookSummary,
    type Chapter,
    type HighlightColor,
    type Inline,
    type LookupTarget,
    type Mark,
    type Node,
    type PdfSpot,
    type SearchHit,
    type Slice,
    type TocEntry,
  } from './lib/api';
  import Appearance from './lib/Appearance.svelte';
  import Help from './lib/Help.svelte';
  import History from './lib/History.svelte';
  import ImageView from './lib/ImageView.svelte';
  import { matchLetterHeight } from './lib/fonts';
  import { JumpList, type JumpLocation } from './lib/jumps';
  import Leader from './lib/Leader.svelte';
  import Library from './lib/Library.svelte';
  import { Keymap, KeySequence, keyToken } from './lib/keys';
  import Lookup from './lib/Lookup.svelte';
  import Menu, { type MenuItem } from './lib/Menu.svelte';
  import Notes from './lib/Notes.svelte';
  import type { Box } from './lib/pdftext';
  import PdfView, { type PdfInfo } from './lib/PdfView.svelte';
  import { measureScrolling } from './lib/perf';
  import { NAV_WIDTH, prefs } from './lib/prefs.svelte';
  import { runScript, type ScriptStep } from './lib/script';
  import Reader from './lib/Reader.svelte';
  import Search from './lib/Search.svelte';
  import Sidebar from './lib/Sidebar.svelte';
  import Stats from './lib/Stats.svelte';
  import { pageColors } from './lib/themes';
  import type { LinkTarget } from './lib/types';
  import type { VimMode } from './lib/vim';

  const WORDS_PER_MINUTE = 250;
  /** How far into a chapter the running head starts naming it: by then its
      own heading has left the top of the page. */
  const CHAPTER_HEAD_AFTER = 120;

  /** How long the reader must rest before the place is written down. */
  const PLACE_SAVE_DELAY_MS = 500;
  /** Longest piece of text quoted in a menu item. */
  const MENU_QUOTE_LENGTH = 28;
  /** How long typing in a PDF's find field rests before it is looked for. */
  const FIND_PAUSE_MS = 160;

  type View = 'reader' | 'library' | 'appearance' | 'help' | 'history' | 'stats';
  /** A screen that opens over whatever was there and returns to it. */
  type Screen = Exclude<View, 'reader' | 'library'>;

  /** Each screen's bindings, and the commands that open it, which close it again. */
  const SCREENS: Record<Screen, { context: string; openers: string[] }> = {
    appearance: { context: 'popup.settings', openers: ['open_theme_selector', 'open_settings'] },
    help: { context: 'popup.help', openers: ['toggle_help'] },
    history: { context: 'popup.history', openers: ['toggle_reading_history'] },
    stats: { context: 'popup.stats', openers: ['toggle_book_stats'] },
  };

  /** The five colours the terminal app has, with the keys it picks them by. */
  const HIGHLIGHTS: { color: HighlightColor; key: string; name: string }[] = [
    { color: 'yellow', key: 'y', name: 'Yellow' },
    { color: 'green', key: 'g', name: 'Green' },
    { color: 'blue', key: 'b', name: 'Blue' },
    { color: 'red', key: 'r', name: 'Red' },
    { color: 'purple', key: 'p', name: 'Purple' },
  ];

  const SORTS = ['name', 'recent', 'type'] as const;
  const SORT_NAMES = { name: 'by name', recent: 'most recently read first', type: 'PDFs first' };

  /** A note being written: on a new passage, or an existing note. In a
      PDF the passage is boxes on a page (`chapter` is the page). */
  type Draft = {
    id: string | null;
    chapter: number;
    slices: Slice[];
    rects?: Box[];
    quote: string;
    text: string;
  };

  /** A line being typed into the status bar: what to find in the text, in
      the library or in the contents, or the page to go to. */
  type Prompt = { kind: 'find' | 'books' | 'contents' | 'page'; value: string };

  let keymap = $state.raw<Keymap>();
  let libraryDir = $state('');
  let books = $state<BookSummary[]>([]);
  /** When each book was last read, for a library ordered by that. */
  let recency = $state.raw(new Map<string, string>());
  let book = $state<BookMeta | null>(null);
  let chapter = $state.raw<Chapter | null>(null);
  /** The chapter's HTML as the book has it, while that is shown instead. */
  let raw = $state<string | null>(null);
  let pdfInfo = $state<PdfInfo | null>(null);
  let focus = $state<'nav' | 'content' | 'lookup' | 'notes' | 'search'>('content');
  let sidebarOpen = $state(true);
  let zen = $state(false);
  let view = $state<View>('reader');
  /** The screens under the one showing: Appearance and the like open over
      whatever was there and return to it. */
  let behind = $state<View[]>([]);
  let menu = $state<{ x: number; y: number; items: MenuItem[] } | null>(null);
  let picture = $state<{ src: string; alt: string } | null>(null);
  let progress = $state(0);
  let scrollTop = $state(0);
  let shelf = $state<{ book: BookSummary; details: BookDetails | null } | null>(null);
  let pending = $state<string[]>([]);
  let message = $state<{ text: string; error: boolean } | null>(null);
  let prompt = $state<Prompt | null>(null);
  /** What `/` is looking for in the text, and how many places have it
      (not counted in a PDF, where only nearby pages are read). */
  let found = $state<{ query: string; count: number | null; index: number } | null>(null);
  let reader = $state<ReturnType<typeof Reader>>();
  let pdfView = $state<ReturnType<typeof PdfView>>();
  let sidebar = $state<ReturnType<typeof Sidebar>>();
  let library = $state<ReturnType<typeof Library>>();
  let help = $state<ReturnType<typeof Help>>();
  let historyScreen = $state<ReturnType<typeof History>>();
  let statsScreen = $state<ReturnType<typeof Stats>>();
  let menuPanel = $state<ReturnType<typeof Menu>>();
  let pictureView = $state<ReturnType<typeof ImageView>>();
  let appearance = $state<ReturnType<typeof Appearance>>();
  let lookupPanel = $state<ReturnType<typeof Lookup>>();
  let searchPanel = $state<ReturnType<typeof Search>>();
  let vimMode = $state<VimMode>('reading');
  let vimPending = $state('');
  let lookup = $state<{ query: string; targets: LookupTarget[] } | null>(null);
  /** The whole-book search panel, when open, and the search it starts from. */
  let search = $state<{ initial: { query: string; hits: SearchHit[] } | null } | null>(null);
  let annotations = $state<Annotation[]>([]);
  let marks = $state<Mark[]>([]);
  /** The notes panel, when open. `passing` is set when it was opened only
      to write one note, and closes again once that is saved. */
  let notes = $state<{ editing: Draft | null; passing: boolean } | null>(null);
  let notesPanel = $state<ReturnType<typeof Notes>>();
  /** Set after `m` or a backtick, while the letter of the bookmark is awaited. */
  let pendingMark = $state<'set' | 'goto' | null>(null);

  let messageTimer: ReturnType<typeof setTimeout> | undefined;
  let placeTimer: ReturnType<typeof setTimeout> | undefined;
  let findTimer: ReturnType<typeof setTimeout> | undefined;
  /** False while a book is being opened: until its saved place is back on
      screen, the top of the chapter must not be written over it. */
  let placeSettled = false;
  let heldKey: string | null = null;
  /** Where things stood when a line began to be typed, to go back to. */
  let promptOrigin: JumpLocation | number | null = null;
  /** The last words looked for in the library or the contents. */
  let seekWords = '';
  let lastSearch: { query: string; hits: SearchHit[] } | null = null;
  const sequence = new KeySequence((keys) => (pending = keys));
  const jumps = new JumpList(20);

  let toc = $derived.by((): TocEntry[] => {
    if (!book) return [];
    if (book.toc.length > 0 || book.pdf) return book.toc;
    return Array.from({ length: book.chapter_count }, (_, i) => ({
      title: `Chapter ${i + 1}`,
      chapter: i,
      anchor: null,
      children: [],
    }));
  });

  let sidebarShown = $derived(sidebarOpen && !zen && !lookup && !notes && !search);

  /** The chapter on screen, or the page of a PDF. */
  let current = $derived(book?.pdf ? (pdfInfo?.page ?? null) : (chapter?.index ?? null));
  /** The chapter block at the top of the view, as of the last saved place. */
  let currentNode = $state<number | null>(null);

  let chapterNotes = $derived(annotations.filter((item) => item.chapter === chapter?.index));
  let chapterMarks = $derived(
    marks.flatMap((mark) =>
      mark.here && mark.chapter === chapter?.index && mark.node !== null
        ? [{ letter: mark.letter, node: mark.node }]
        : [],
    ),
  );
  let pageMarks = $derived(
    marks.flatMap((mark) =>
      mark.here && mark.chapter !== null
        ? [{ letter: mark.letter, page: mark.chapter, line: mark.node }]
        : [],
    ),
  );

  let chapterTitles = $derived.by(() => {
    const titles = new Map<number, string>();
    const walk = (entries: TocEntry[]) => {
      for (const entry of entries) {
        if (entry.chapter !== null && !titles.has(entry.chapter)) titles.set(entry.chapter, entry.title);
        walk(entry.children);
      }
    };
    walk(toc);
    return titles;
  });

  let shelfBooks = $derived.by(() => {
    const sort = prefs.saved.library_sort;
    if (sort === 'recent') {
      const when = (b: BookSummary) => recency.get(b.path) ?? '';
      return [...books].sort((a, b) => when(b).localeCompare(when(a)));
    }
    if (sort === 'type') {
      const rank = (b: BookSummary) => (b.format === 'pdf' || b.format === 'djvu' ? 0 : 1);
      return [...books].sort((a, b) => rank(a) - rank(b));
    }
    return books;
  });

  let mode = $derived.by(() => {
    if (view === 'help') return { kind: 'nav', label: 'SHORTCUTS' };
    if (view === 'appearance') return { kind: 'nav', label: 'APPEARANCE' };
    if (view === 'history') return { kind: 'nav', label: 'HISTORY' };
    if (view === 'stats') return { kind: 'nav', label: 'STATISTICS' };
    if (view === 'library') return { kind: 'nav', label: 'LIBRARY' };
    if (focus === 'nav') return { kind: 'nav', label: 'CONTENTS' };
    if (focus === 'lookup') return { kind: 'nav', label: 'LOOK UP' };
    if (focus === 'notes') return { kind: 'nav', label: 'NOTES' };
    if (focus === 'search') return { kind: 'nav', label: 'SEARCH' };
    const labels = { reading: 'READING', normal: 'NORMAL', visual: 'VISUAL', 'visual-line': 'V-LINE' };
    return { kind: vimMode, label: labels[vimMode] };
  });

  let chapterWords = $derived(chapter ? countWords(chapter.blocks) : 0);
  let minutesLeft = $derived(Math.ceil((chapterWords * (1 - progress)) / WORDS_PER_MINUTE));

  function inlineWords(items: Inline[]): number {
    let n = 0;
    for (const item of items) {
      if (item.t === 'text') n += item.s.split(/\s+/).filter(Boolean).length;
      else if (item.t === 'link') n += inlineWords(item.children);
    }
    return n;
  }

  function countWords(nodes: Node[]): number {
    let n = 0;
    for (const node of nodes) {
      switch (node.t) {
        case 'h':
        case 'p':
          n += inlineWords(node.text);
          break;
        case 'code':
          n += node.code.split(/\s+/).filter(Boolean).length;
          break;
        case 'quote':
        case 'epub':
          n += countWords(node.blocks);
          break;
        case 'list':
          for (const item of node.items) n += countWords(item);
          break;
        case 'dl':
          for (const item of node.items) {
            n += inlineWords(item.term);
            for (const def of item.defs) n += countWords(def);
          }
          break;
        case 'table':
          for (const row of [...(node.header ? [node.header] : []), ...node.rows]) {
            for (const cell of row) {
              n += cell.text ? inlineWords(cell.text) : countWords(cell.blocks ?? []);
            }
          }
          break;
      }
    }
    return n;
  }

  function flash(text: string, error = false) {
    message = { text, error };
    clearTimeout(messageTimer);
    messageTimer = setTimeout(() => (message = null), 4000);
  }

  /** The text on screen, whichever kind of book it is. */
  function doc() {
    return book?.pdf ? pdfView : reader;
  }

  function here(): JumpLocation | null {
    if (book?.pdf) {
      const at = pdfView?.position();
      return at ? { ...at, chapter: at.page } : null;
    }
    return chapter && reader ? { chapter: chapter.index, ...reader.position() } : null;
  }

  /** Records the current place before leaving it, so Ctrl+o can return. */
  function saveJump() {
    const location = here();
    if (location) jumps.push(location);
  }

  /** Puts another chapter on screen. What belonged to the old one (its
      raw view, what was being looked for in it) goes with it. */
  /** Chapters read to preview where a link leads, kept while the book is open. */
  let previewChapters = new Map<number, Promise<Chapter>>();
  const PREVIEW_CHAPTERS_KEPT = 8;

  function chapterFor(index: number): Promise<Chapter> {
    if (chapter?.index === index) return Promise.resolve(chapter);
    let kept = previewChapters.get(index);
    if (!kept) {
      kept = api.loadChapter(index);
      kept.catch(() => previewChapters.delete(index));
      previewChapters.set(index, kept);
      const oldest = previewChapters.keys().next().value;
      if (previewChapters.size > PREVIEW_CHAPTERS_KEPT && oldest !== undefined) {
        previewChapters.delete(oldest);
      }
    }
    return kept;
  }

  async function load(index: number) {
    chapter = await api.loadChapter(index);
    raw = null;
    found = null;
    await tick();
  }

  async function jumpTo(location: JumpLocation | null) {
    if (!location) return;
    if (book?.pdf) {
      pdfView?.goTo(location.chapter, location.offset);
      return;
    }
    try {
      if (chapter?.index !== location.chapter) await load(location.chapter);
      reader?.restore(location);
    } catch (e) {
      flash(String(e), true);
    }
  }

  /** Shows a chapter from its top, or from an anchor in it, from where one
      of the files it is made of begins (`part`), or from one of its blocks
      (`block`). In a PDF, a page. */
  async function showChapter(index: number, anchor?: string | null, part?: number, block?: number) {
    if (!book) return;
    const target = Math.max(0, Math.min(book.chapter_count - 1, index));
    if (book.pdf) {
      if (current !== target) saveJump();
      pdfView?.goTo(target);
      return;
    }
    const within = Boolean(anchor) || part !== undefined || block !== undefined;
    if (chapter?.index !== target || within) saveJump();
    try {
      if (chapter?.index !== target) {
        await load(target);
        reader?.resetScroll();
      } else if (!within) {
        reader?.toTop();
      }
      const node = part === undefined ? block : chapter?.parts[part];
      if (anchor) reader?.toAnchor(anchor);
      else if (node !== undefined) reader?.toNode(node);
    } catch (e) {
      flash(String(e), true);
    }
  }

  function savePlaceSoon() {
    if (!placeSettled) return;
    clearTimeout(placeTimer);
    placeTimer = setTimeout(() => void savePlace(), PLACE_SAVE_DELAY_MS);
  }

  async function savePlace() {
    clearTimeout(placeTimer);
    placeTimer = undefined;
    const location = here();
    if (!placeSettled || !location || !book) return;
    const { chapter: index, node, offset } = location;
    if (!book.pdf) currentNode = node;
    try {
      const look = book.pdf ? pdfView?.look() : undefined;
      if (look) await api.savePagePlace(index, offset, look.themed, look.tintImages);
      else if (!book.pdf) {
        await api.savePlace({ chapter: index, node, offset }, (index + progress) / book.chapter_count);
      }
    } catch (e) {
      flash(String(e), true);
    }
  }

  /** Writes the place down now if a change is still waiting to be saved. */
  async function flushPlace() {
    if (placeTimer) await savePlace();
  }

  async function openBook(path: string) {
    try {
      await flushPlace();
      placeSettled = false;
      const { paper, ink } = pageColors(prefs.theme);
      const opened = await api.openBook(path, paper, ink);
      book = opened;
      previewChapters = new Map();
      currentNode = null;
      pdfInfo = null;
      await loadNotes();
      jumps.clear();
      chapter = null;
      raw = null;
      found = null;
      lastSearch = null;
      search = null;
      lookup = null;
      notes = null;
      picture = null;
      view = 'reader';
      behind = [];
      focus = 'content';
      const place = opened.place;
      if (opened.pdf) {
        // The pages come up at the place the document was left at.
        await tick();
      } else if (place) {
        const location = { ...place, scrollTop: 0, layout: '', cursor: null };
        await jumpTo(location);
        // Text set before its typeface has loaded moves when the face
        // arrives; if the reader has not scrolled by then, the place is put back.
        const at = () => (reader ? reader.position().scrollTop - reader.carriedDown() : undefined);
        const settled = at();
        void document.fonts.ready.then(() => {
          const unmoved = at() === settled;
          if (book?.path === opened.path && chapter?.index === place.chapter && unmoved) {
            reader?.restore(location);
          }
        });
      } else {
        await showChapter(0);
      }
    } catch (e) {
      flash(String(e), true);
    } finally {
      placeSettled = true;
      // Opening a book counts as reading it: this also files a new book in
      // the bookmarks the terminal app shares.
      if (book) savePlaceSoon();
    }
  }

  async function loadNotes() {
    try {
      [annotations, marks] = await Promise.all([api.annotations(), api.marks()]);
    } catch (e) {
      annotations = [];
      marks = [];
      flash(String(e), true);
    }
  }

  async function loadRecency() {
    try {
      const entries = await api.readingHistory();
      recency = new Map(entries.map((entry) => [entry.path, entry.last_read]));
    } catch (e) {
      flash(String(e), true);
    }
  }

  function titleOf(index: number): string {
    if (book?.pdf) return `Page ${index + 1}`;
    return chapterTitles.get(index) ?? `Chapter ${index + 1}`;
  }

  function quoteFor(note: Annotation): string {
    if (note.quote) return note.quote;
    if (book?.pdf) return '';
    return note.chapter === chapter?.index ? (reader?.quoteOf(note.slices) ?? '') : '';
  }

  // A panel beside the text narrows the column, so the spot being read is
  // pinned across opening and closing one.
  async function openPanel(open: () => void) {
    releaseScroll();
    const restore = doc()?.pin();
    lookup = null;
    notes = null;
    search = null;
    open();
    await tick();
    restore?.();
  }

  async function closePanel() {
    if (!lookup && !notes && !search) return;
    const restore = doc()?.pin();
    lookup = null;
    notes = null;
    search = null;
    focus = 'content';
    await tick();
    restore?.();
  }

  async function openNotes(editing: Draft | null, passing: boolean) {
    await openPanel(() => {
      notes = { editing, passing };
      focus = 'notes';
    });
  }

  function endEdit() {
    if (notes?.passing) void closePanel();
    else if (notes) notes = { editing: null, passing: false };
  }

  async function highlight(color: HighlightColor) {
    if (!book) return;
    const existing = doc()?.annotationAt();
    try {
      if (existing?.color) {
        annotations = await api.editAnnotation(existing.id, { color });
      } else if (existing) {
        flash('That passage has a note. Remove the note to highlight it.');
        return;
      } else if (book.pdf) {
        const pick = pdfView?.picked();
        if (!pick) {
          flash('Select some text first, then highlight it');
          return;
        }
        annotations = await api.addPageAnnotation(pick.page, pick.rects, color, '', pick.quote);
      } else {
        const pick = reader?.picked();
        if (!pick || !chapter) {
          flash('Select some text in a paragraph first, then highlight it');
          return;
        }
        annotations = await api.addAnnotation(chapter.index, pick.pieces, color, '');
      }
      doc()?.clearSelection();
    } catch (e) {
      flash(String(e), true);
    }
  }

  function swatches(label: string): MenuItem {
    return {
      label,
      swatches: HIGHLIGHTS.map(({ color, key, name }) => ({
        color: `var(--mark-${color})`,
        name,
        key,
        run: () => void highlight(color),
      })),
    };
  }

  /** `H`: the colours to highlight the selection with, by the selection. */
  function openPalette() {
    const existing = doc()?.annotationAt();
    const box = doc()?.selectionBox();
    const pick = book?.pdf ? pdfView?.picked() : reader?.picked();
    if (!box || (!existing?.color && !pick)) {
      flash('Select some text first, then highlight it');
      return;
    }
    const items: MenuItem[] = [swatches(existing?.color ? 'Colour' : 'Highlight')];
    if (existing?.color) {
      items.push({
        label: 'Remove highlight',
        keys: keysOf('content', 'delete_comment'),
        run: () => void removeAnnotation(existing),
      });
    }
    menu = { x: box.left, y: box.bottom + 6, items };
  }

  async function addNote() {
    if (!book) return;
    const existing = doc()?.annotationAt();
    if (existing?.color) {
      flash('That passage is highlighted. Remove the highlight to write a note there.');
      return;
    }
    if (existing) {
      await editNote(existing);
      return;
    }
    let draft: Draft | null = null;
    if (book.pdf) {
      const pick = pdfView?.picked();
      if (pick) draft = { id: null, chapter: pick.page, slices: [], rects: pick.rects, quote: pick.quote, text: '' };
    } else {
      const pick = reader?.picked();
      const first = pick?.pieces[0];
      if (pick && first && chapter) {
        draft = { id: null, chapter: chapter.index, slices: first.slices, quote: first.quote ?? '', text: '' };
        // A note belongs to one of the book's files, unlike a highlight,
        // which is made once for each file the selection runs through.
        if (pick.pieces.length > 1) {
          flash('The selection runs from one part of the book into the next; the note is on the first');
        }
      }
    }
    if (!draft) {
      flash('Select some text first, then write a note on it');
      return;
    }
    await openNotes(draft, notes === null);
  }

  async function editNote(note: Annotation) {
    const draft = { ...note, quote: quoteFor(note) };
    await openNotes(draft, notes === null);
  }

  /** Keeps what was written: an emptied note is removed, an empty new one never made. */
  async function saveNote(text: string) {
    const draft = notes?.editing;
    if (!draft) return;
    const body = text.trim();
    try {
      if (draft.id) {
        annotations = body
          ? await api.editAnnotation(draft.id, { text: body })
          : await api.deleteAnnotation(draft.id);
      } else if (body && draft.rects) {
        annotations = await api.addPageAnnotation(draft.chapter, draft.rects, null, body, draft.quote);
      } else if (body) {
        const piece = { slices: draft.slices, quote: draft.quote };
        annotations = await api.addAnnotation(draft.chapter, [piece], null, body);
      }
      doc()?.clearSelection();
      endEdit();
    } catch (e) {
      flash(String(e), true);
    }
  }

  async function removeAnnotation(item: Annotation) {
    try {
      annotations = await api.deleteAnnotation(item.id);
      flash(item.color ? 'Highlight removed' : 'Note removed');
    } catch (e) {
      flash(String(e), true);
    }
  }

  function removeHere() {
    const existing = doc()?.annotationAt();
    if (existing) void removeAnnotation(existing);
    else flash('There is no highlight or note here');
  }

  async function exportNotes() {
    try {
      flash(`Exported to ${await api.exportNotes(current ?? 0)}`);
    } catch (e) {
      flash(String(e), true);
    }
  }

  /** Sets a bookmark at the cursor or the top of the view, or at a point. */
  async function setMark(letter: string, point?: { x: number; y: number }) {
    if (!book) return;
    try {
      if (book.pdf) {
        const spot = pdfView?.markSpot(point);
        if (!spot) return;
        marks = await api.setPageMark(letter, spot.page, spot);
      } else {
        const spot = reader?.markSpot(point);
        if (!spot || !chapter) return;
        marks = await api.setMark(
          letter,
          { chapter: chapter.index, node: spot.node, offset: 0 },
          { offset: spot.offset, snippet: spot.snippet, chapterTitle: chapter.title },
          (chapter.index + progress) / book.chapter_count,
        );
      }
      flash(`Bookmark ${letter} set`);
    } catch (e) {
      flash(String(e), true);
    }
  }

  async function gotoMark(mark: Mark) {
    if (mark.here && mark.chapter !== null) {
      saveJump();
      if (book?.pdf) {
        await pdfView?.goToLine(mark.chapter, mark.node);
      } else if (mark.node !== null) {
        await jumpTo({ chapter: mark.chapter, node: mark.node, offset: 0, scrollTop: 0, layout: '', cursor: null });
      }
      return;
    }
    // A shared bookmark in another book: open that book, then go to it.
    await openBook(mark.path);
    await tick();
    const there = marks.find((m) => m.letter === mark.letter && m.here);
    if (there) await gotoMark(there);
  }

  async function removeMark(mark: Mark) {
    try {
      marks = await api.deleteMark(mark.letter);
      flash(`Bookmark ${mark.letter} removed`);
    } catch (e) {
      flash(String(e), true);
    }
  }

  function gotoNote(note: Annotation) {
    saveJump();
    const box = note.rects?.[0];
    if (book?.pdf) {
      if (box) pdfView?.showSpot({ page: note.chapter, x: box[0], y: (box[1] + box[3]) / 2 }, [box[1], box[3]]);
      else pdfView?.goTo(note.chapter);
      return;
    }
    const node = note.slices[0]?.node ?? 0;
    void jumpTo({ chapter: note.chapter, node, offset: 0, scrollTop: 0, layout: '', cursor: null });
  }

  /** The key after `m` or a backtick names the bookmark. */
  function takeMarkKey(token: string) {
    const op = pendingMark;
    pendingMark = null;
    if (/^[a-zA-Z]$/.test(token)) {
      const mark = marks.find((m) => m.letter === token);
      if (op === 'set') void setMark(token);
      else if (mark) void gotoMark(mark);
      else flash(`There is no bookmark ${token}`);
    } else if (op === 'goto' && (token === '`' || token === "'")) {
      // Twice over opens the list, as in the terminal app.
      void openNotes(null, false);
    }
  }

  function openExternal(url: string) {
    api.openExternal(url).catch((e) => flash(String(e), true));
  }

  function followLink(target: LinkTarget) {
    if (target.kind !== 'external') saveJump();
    if (target.kind === 'chapter') void showChapter(target.chapter, target.anchor, target.part);
    else if (target.kind === 'anchor') reader?.toAnchor(target.anchor);
    else openExternal(target.url);
  }

  function copy(text: string, announce: boolean) {
    api
      .copyText(text)
      .then(() => {
        if (announce) flash(`Copied ${[...text].length} characters`);
      })
      .catch((e) => flash(String(e), true));
  }

  /** `Space c`: the whole chapter, or the page of a PDF, to the clipboard. */
  async function copyChapter() {
    try {
      const text = book?.pdf ? await api.pdfPageText(current ?? 0) : reader?.chapterText();
      if (text?.trim()) copy(text, true);
      else flash(book?.pdf ? 'This page has no text to copy' : 'There is no text to copy');
    } catch (e) {
      flash(String(e), true);
    }
  }

  async function openLookup(picked?: string) {
    const query = (picked ?? doc()?.selectedText())?.split(/\s+/).filter(Boolean).join(' ');
    if (!query) {
      flash('No text selected. Select text first, then look it up.');
      return;
    }
    try {
      const { targets, fallback } = await api.lookupTargets();
      if (targets.length === 0 && fallback !== 'popup') {
        if (!fallback) {
          flash('No lookup command configured. Add lookups to config.yaml.');
          return;
        }
        await api.lookupRun(Date.now(), null, query);
        flash('Launched the lookup command');
        return;
      }
      await openPanel(() => {
        lookup = { query, targets };
        focus = 'lookup';
      });
    } catch (e) {
      flash(String(e), true);
    }
  }

  // ── Finding ─────────────────────────────────────────────────

  /** Starts a line in the status bar. */
  function ask(kind: Prompt['kind']) {
    releaseScroll();
    if (kind === 'find') promptOrigin = here();
    else if (kind === 'books') promptOrigin = library?.place() ?? null;
    else if (kind === 'contents') promptOrigin = sidebar?.place() ?? null;
    prompt = { kind, value: '' };
  }

  async function findText(text: string) {
    const words = text.trim();
    if (!words) {
      clearFind();
    } else if (book?.pdf) {
      const any = (await pdfView?.find(words)) ?? false;
      found = { query: words, count: any ? null : 0, index: 0 };
    } else {
      const count = reader?.find(words) ?? 0;
      found = { query: words, count, index: reader?.findAt() ?? 0 };
    }
  }

  function clearFind() {
    clearTimeout(findTimer);
    found = null;
    reader?.findClear();
    pdfView?.findClear();
  }

  async function findNext(direction: 1 | -1) {
    if (!found) return;
    if (book?.pdf) {
      const moved = (await pdfView?.findStep(direction)) ?? false;
      if (!moved) flash(`“${found.query}” is nowhere else in the document`);
    } else if (found.count) {
      found = { ...found, index: reader?.findStep(direction) ?? 0 };
    }
  }

  /** The line being typed changed: what it is for follows along. */
  function promptTyped() {
    if (!prompt) return;
    const { kind, value } = prompt;
    if (kind === 'find') {
      clearTimeout(findTimer);
      if (book?.pdf) findTimer = setTimeout(() => void findText(value), FIND_PAUSE_MS);
      else void findText(value);
    } else if (kind === 'books') {
      library?.restore(promptOrigin as number);
      library?.seek(value, 0);
    } else if (kind === 'contents') {
      sidebar?.restore(promptOrigin as number);
      sidebar?.seek(value, 0);
    }
  }

  async function commitPrompt() {
    if (!prompt) return;
    const { kind, value } = prompt;
    prompt = null;
    const words = value.trim();
    if (kind === 'find') {
      clearTimeout(findTimer);
      await findText(words);
      if (words && found?.count === 0) {
        flash(`Nothing found for “${words}”`);
        clearFind();
      }
    } else if (kind === 'page') {
      const page = Number(words);
      if (book && Number.isInteger(page) && page >= 1 && page <= book.chapter_count) {
        saveJump();
        pdfView?.goTo(page - 1);
      } else if (words) {
        flash(`There is no page ${words}`);
      }
    } else {
      seekWords = words;
      const list = kind === 'books' ? library : sidebar;
      if (words && !list?.seek(words, 0)) flash(`Nothing found for “${words}”`);
    }
  }

  function cancelPrompt() {
    if (!prompt) return;
    const { kind } = prompt;
    prompt = null;
    if (kind === 'find') {
      clearFind();
      void jumpTo(promptOrigin as JumpLocation | null);
    } else if (kind === 'books') {
      library?.restore(promptOrigin as number);
    } else if (kind === 'contents') {
      sidebar?.restore(promptOrigin as number);
    }
  }

  function promptKey(event: KeyboardEvent) {
    if (event.key === 'Enter') {
      event.preventDefault();
      void commitPrompt();
    } else if (event.key === 'Escape') {
      event.preventDefault();
      event.stopPropagation();
      cancelPrompt();
    }
  }

  async function openSearch(fresh: boolean) {
    if (!book) return;
    await openPanel(() => {
      search = { initial: fresh ? null : lastSearch };
      focus = 'search';
    });
  }

  /** The first run of matched characters in a result: the word to go to. */
  function matched(hit: SearchHit): string {
    const letters = [...hit.snippet];
    const first = hit.marks[0];
    if (first === undefined) return '';
    let end = first;
    while (hit.marks.includes(end + 1)) end++;
    return letters.slice(first, end + 1).join('');
  }

  async function gotoHit(hit: SearchHit) {
    saveJump();
    const word = matched(hit);
    if (book?.pdf) {
      const middle = hit.span ? (hit.span[0] + hit.span[1]) / 2 : 0;
      pdfView?.showSpot({ page: hit.section, x: 0, y: middle }, hit.span);
      if (word) {
        found = { query: word, count: null, index: 0 };
        void pdfView?.find(word, hit.span ? { page: hit.section, y: hit.span[0] } : undefined);
      }
      return;
    }
    await jumpTo({ chapter: hit.section, node: hit.node, offset: 0, scrollTop: 0, layout: '', cursor: null });
    reader?.arrive(hit.node);
    if (word) {
      const count = reader?.find(word) ?? 0;
      found = count > 0 ? { query: word, count, index: reader?.findAt() ?? 0 } : null;
    }
  }

  // ── Scrolling, screens, the window ──────────────────────────

  // The scroll keeps going for as long as the key is down, so it is driven
  // by key down and key up; the keyboard's own repeats are ignored.
  function holdScroll(direction: 1 | -1, event: KeyboardEvent) {
    if (event.repeat) return;
    heldKey = event.code;
    doc()?.holdScroll(direction);
  }

  function releaseScroll() {
    heldKey = null;
    doc()?.releaseScroll();
  }

  function openOver(screen: Screen) {
    releaseScroll();
    if (view === screen) return;
    behind = [...behind, view];
    view = screen;
  }

  function closeOver() {
    view = behind.at(-1) ?? 'reader';
    behind = behind.slice(0, -1);
  }

  function showing(screen: View): boolean {
    return view === screen || behind.includes(screen);
  }

  function openLibrary() {
    releaseScroll();
    void flushPlace()
      .then(() => api.setReading(false))
      .catch((e) => flash(String(e), true));
    void loadRecency();
    view = 'library';
  }

  function backToBook() {
    if (!book) return;
    api.setReading(true).catch((e) => flash(String(e), true));
    view = 'reader';
  }

  function quit() {
    void Promise.allSettled([flushPlace(), prefs.flush()]).finally(() => api.quit());
  }

  function toggleSidebar() {
    if (sidebarShown) {
      sidebarOpen = false;
    } else {
      zen = false;
      sidebarOpen = true;
    }
  }

  function toggleZen() {
    zen = !zen;
    if (zen) focus = 'content';
  }

  async function toggleRaw() {
    if (!chapter || book?.pdf) return;
    if (raw !== null) {
      raw = null;
      return;
    }
    try {
      const html = await api.chapterHtml(chapter.index);
      clearFind();
      raw = html;
      reader?.resetScroll();
    } catch (e) {
      flash(String(e), true);
    }
  }

  async function reloadKeys() {
    try {
      const problems = await api.keymapProblems();
      keymap = new Keymap(await api.keymap());
      if (problems.length === 0) flash('Keybindings reloaded');
      else flash(`keybindings.toml: ${problems[0]}${problems.length > 1 ? ` (and ${problems.length - 1} more)` : ''}`, true);
    } catch (e) {
      flash(String(e), true);
    }
  }

  function openInViewer() {
    if (!book) return;
    api
      .openInViewer(book.pdf ? 0 : (chapter?.index ?? 0))
      .then(() => flash('Opened in the system viewer'))
      .catch((e) => flash(String(e), true));
  }

  /** The simplest key bound to an action, to show beside its menu item. */
  function keysOf(context: string, action: string): string[] | undefined {
    return keymap?.keysFor(context, action)[0];
  }

  function quoted(text: string): string {
    const letters = [...text.split(/\s+/).filter(Boolean).join(' ')];
    const shown = letters.length > MENU_QUOTE_LENGTH ? [...letters.slice(0, MENU_QUOTE_LENGTH), '…'] : letters;
    return `“${shown.join('')}”`;
  }

  /** The right-click menu: what the keyboard does, for the mouse. */
  function showMenu(event: MouseEvent) {
    // Text fields keep the system's own menu, for paste and the like.
    if (event.target instanceof HTMLInputElement || event.target instanceof HTMLTextAreaElement) return;
    event.preventDefault();
    releaseScroll();
    const target = event.target as Element;
    const items: MenuItem[] = [];
    const shortcuts: MenuItem = {
      label: 'Keyboard shortcuts',
      keys: keysOf('global', 'toggle_help'),
      run: () => openOver('help'),
    };
    const looks: MenuItem = {
      label: 'Appearance',
      keys: keysOf('global', 'open_theme_selector'),
      run: () => openOver('appearance'),
    };
    const history: MenuItem = {
      label: 'Reading history',
      keys: keysOf('global', 'toggle_reading_history'),
      run: () => openOver('history'),
    };

    if (view === 'help' || view === 'history' || view === 'stats') {
      items.push({ label: 'Close', keys: ['Esc'], run: closeOver });
    } else if (view === 'appearance') {
      items.push(
        { label: 'Close appearance', keys: ['Esc'], run: () => appearance?.handle('cancel') },
        shortcuts,
      );
    } else if (view === 'library') {
      const path = target.closest('[data-book]')?.getAttribute('data-book');
      if (path) {
        library?.choose(path);
        items.push(
          { label: 'Open this book', keys: keysOf('nav', 'select'), run: () => void openBook(path) },
          'separator',
        );
      }
      if (book) {
        items.push({
          label: 'Back to the book',
          keys: keysOf('nav', 'switch_nav_mode'),
          run: backToBook,
        });
      }
      items.push(
        { label: 'Find a book', keys: keysOf('nav', 'start_search'), run: () => ask('books') },
        {
          label: `Order: ${SORT_NAMES[prefs.saved.library_sort]}`,
          keys: keysOf('nav', 'toggle_sort_order'),
          run: nextSort,
        },
        history,
        looks,
        shortcuts,
      );
    } else {
      const inText = Boolean(book && current !== null && target.closest('main'));
      const picked = inText
        ? (doc()?.selectedText() ?? doc()?.selectWordAt(event.clientX, event.clientY))
        : null;
      const point = { x: event.clientX, y: event.clientY };
      const marked = inText ? doc()?.annotationAt(point) : undefined;
      if (picked?.trim()) {
        items.push(
          {
            label: `Look up ${quoted(picked)}`,
            keys: keysOf('global', 'lookup_selection'),
            run: () => void openLookup(picked),
          },
          { label: 'Copy', keys: keysOf('content', 'copy_selection'), run: () => copy(picked, true) },
        );
        if (!marked) {
          items.push(swatches('Highlight'), {
            label: 'Write a note',
            keys: keysOf('content', 'add_comment'),
            run: () => void addNote(),
          });
        }
        items.push('separator');
      }
      if (marked?.color) {
        items.push(
          swatches('Colour'),
          { label: 'Remove highlight', run: () => void removeAnnotation(marked) },
          'separator',
        );
      } else if (marked) {
        items.push(
          { label: 'Edit note', run: () => void editNote(marked) },
          { label: 'Remove note', run: () => void removeAnnotation(marked) },
          'separator',
        );
      }
      if (lookup || search) {
        items.push(
          { label: lookup ? 'Close look up' : 'Close search', keys: ['Esc'], run: () => void closePanel() },
          'separator',
        );
      }
      if (book && current !== null) {
        const index = current;
        const paged = Boolean(book.pdf);
        items.push(
          {
            label: 'Back',
            keys: keysOf('content', 'jump_backward'),
            disabled: !jumps.canGoBack,
            run: () => void jumpTo(jumps.back(here())),
          },
          {
            label: 'Forward',
            keys: keysOf('content', 'jump_forward'),
            disabled: !jumps.canGoForward,
            run: () => void jumpTo(jumps.forward()),
          },
          {
            label: paged ? 'Previous page' : 'Previous chapter',
            keys: paged ? keysOf('pdf', 'prev_page') : keysOf('content', 'prev_chapter'),
            disabled: index === 0,
            run: () => (paged ? pdfView?.action('prev_page') : void showChapter(index - 1)),
          },
          {
            label: paged ? 'Next page' : 'Next chapter',
            keys: paged ? keysOf('pdf', 'next_page') : keysOf('content', 'next_chapter'),
            disabled: index >= book.chapter_count - 1,
            run: () => (paged ? pdfView?.action('next_page') : void showChapter(index + 1)),
          },
          'separator',
          {
            label: paged ? 'Find on the pages' : 'Find in the chapter',
            keys: keysOf(paged ? 'pdf' : 'content', 'start_search'),
            run: () => ask('find'),
          },
          {
            label: paged ? 'Search the document' : 'Search the book',
            keys: keysOf('global', 'open_book_search_fresh'),
            run: () => void openSearch(true),
          },
        );
        if (paged) {
          items.push({
            label: 'Go to page',
            keys: keysOf('global', 'go_to_page'),
            run: () => ask('page'),
          });
          if (book.pdf?.synctex && inText) {
            const spot = pdfView?.spotAt(point.x, point.y);
            if (spot) items.push({ label: 'Show the TeX source', run: () => showSource(spot) });
          }
        }
        items.push('separator', {
          label: sidebarShown ? 'Hide contents' : 'Show contents',
          disabled: lookup !== null || notes !== null || search !== null,
          run: toggleSidebar,
        });
        const free = [...'abcdefghijklmnopqrstuvwxyz'].find(
          (letter) => !marks.some((mark) => mark.letter === letter),
        );
        if (free && inText) {
          items.push({
            label: `Bookmark this place as ${free}`,
            run: () => void setMark(free, point),
          });
        }
        items.push(
          {
            label: 'Notes and bookmarks',
            keys: keysOf('global', 'toggle_comments_viewer'),
            run: () => void openNotes(null, false),
          },
          {
            label: paged ? 'Document statistics' : 'Book statistics',
            keys: keysOf('global', 'toggle_book_stats'),
            run: () => openOver('stats'),
          },
        );
      }
      items.push(
        'separator',
        { label: 'Library', run: openLibrary },
        history,
        looks,
        {
          label: zen ? 'Leave zen mode' : 'Zen mode',
          keys: keysOf('global', 'toggle_zen_mode'),
          run: toggleZen,
        },
        shortcuts,
      );
    }
    menu = { x: event.clientX, y: event.clientY, items };
  }

  function showSource(spot: PdfSpot) {
    api
      .synctexInverse(spot)
      .then((text) => flash(text))
      .catch((e) => flash(String(e), true));
  }

  function nextSort() {
    const next = SORTS[(SORTS.indexOf(prefs.saved.library_sort) + 1) % SORTS.length]!;
    prefs.update({ library_sort: next });
    flash(`Library ordered ${SORT_NAMES[next]}`);
  }

  /** Commands that mean the same wherever they are given. Returns false
      for one that is not among them. */
  function runAnywhere(action: string): boolean {
    switch (action) {
      case 'open_theme_selector':
      case 'open_settings':
        openOver('appearance');
        return true;
      case 'toggle_help':
        openOver('help');
        return true;
      case 'toggle_reading_history':
        openOver('history');
        return true;
      case 'reload_keybindings':
        void reloadKeys();
        return true;
      case 'toggle_zen_border':
        prefs.update({ zen_border: !prefs.saved.zen_border });
        flash(prefs.saved.zen_border ? 'Zen mode keeps the page’s border' : 'Zen mode fills the window');
        return true;
      case 'quit':
        quit();
        return true;
      // The terminal app's own housekeeping: redrawing its screen,
      // suspending to the shell, its profiler. None has a meaning here.
      case 'force_redraw':
      case 'suspend':
      case 'toggle_profiling':
      case 'dump_debug_state':
      case 'nop':
        return true;
      default:
        return false;
    }
  }

  function runInLibrary(action: string) {
    if (runAnywhere(action)) return;
    switch (action) {
      case 'move_down':
        library?.move(0, 1);
        break;
      case 'move_up':
        library?.move(0, -1);
        break;
      case 'collapse':
      case 'move_left':
        library?.move(-1, 0);
        break;
      case 'expand':
      case 'move_right':
        library?.move(1, 0);
        break;
      case 'go_top':
        library?.toEdge('top');
        break;
      case 'go_bottom':
        library?.toEdge('bottom');
        break;
      case 'scroll_half_down':
        library?.page(0.5);
        break;
      case 'scroll_half_up':
        library?.page(-0.5);
        break;
      case 'scroll_page_down':
        library?.page(1);
        break;
      case 'scroll_page_up':
        library?.page(-1);
        break;
      case 'select':
        library?.select();
        break;
      case 'start_search':
        ask('books');
        break;
      case 'next_search_match':
      case 'prev_search_match':
        if (!seekWords) flash('Press / first, and type what to find');
        else if (!library?.seek(seekWords, action === 'next_search_match' ? 1 : -1)) {
          flash(`Nothing found for “${seekWords}”`);
        }
        break;
      case 'toggle_sort_order':
        nextSort();
        break;
      case 'cancel':
        message = null;
        backToBook();
        break;
      case 'switch_nav_mode':
        backToBook();
        break;
      default:
        // Commands for an open book mean nothing on the shelf.
        break;
    }
  }

  function run(action: string, event: KeyboardEvent) {
    if (view === 'library') {
      runInLibrary(action);
      return;
    }
    if (runAnywhere(action)) return;
    const nav = focus === 'nav';
    const paged = Boolean(book?.pdf);

    // While something is being looked for, `n` steps through the places
    // that have it rather than showing the cursor, as in the terminal app.
    if (found && focus === 'content') {
      if (action === 'toggle_normal_mode' || action === 'next_search_match') {
        void findNext(1);
        return;
      }
      if (action === 'prev_search_match') {
        void findNext(-1);
        return;
      }
    }
    if (focus === 'content' && doc()?.vimAction(action)) return;
    if (focus === 'content' && paged && pdfView?.action(action)) return;

    switch (action) {
      case 'scroll_down':
        holdScroll(1, event);
        break;
      case 'scroll_up':
        holdScroll(-1, event);
        break;
      case 'move_down':
        sidebar?.move(1);
        break;
      case 'move_up':
        sidebar?.move(-1);
        break;
      case 'go_top':
        if (nav) sidebar?.toEdge('top');
        else doc()?.toTop();
        break;
      case 'go_bottom':
        if (nav) sidebar?.toEdge('bottom');
        else doc()?.toBottom();
        break;
      case 'scroll_half_down':
        if (nav) sidebar?.page(0.5);
        else doc()?.scrollPages(0.5);
        break;
      case 'scroll_half_up':
        if (nav) sidebar?.page(-0.5);
        else doc()?.scrollPages(-0.5);
        break;
      case 'scroll_page_down':
        if (nav) sidebar?.page(1);
        else doc()?.scrollPages(1);
        break;
      case 'scroll_page_up':
        if (nav) sidebar?.page(-1);
        else doc()?.scrollPages(-1);
        break;
      case 'next_chapter':
        if (paged) pdfView?.action('next_page');
        else if (chapter && book && chapter.index + 1 < book.chapter_count) {
          void showChapter(chapter.index + 1);
        }
        break;
      case 'prev_chapter':
        if (paged) pdfView?.action('prev_page');
        else if (chapter && chapter.index > 0) void showChapter(chapter.index - 1);
        break;
      case 'paragraph_forward':
        reader?.toParagraph(1);
        break;
      case 'paragraph_backward':
        reader?.toParagraph(-1);
        break;
      case 'switch_focus':
        if (lookup) {
          focus = 'lookup';
        } else if (notes) {
          focus = 'notes';
        } else if (search) {
          focus = 'search';
        } else if (nav) {
          focus = 'content';
        } else {
          zen = false;
          sidebarOpen = true;
          focus = 'nav';
        }
        break;
      case 'select':
        sidebar?.select();
        break;
      case 'collapse':
        sidebar?.collapse();
        break;
      case 'expand':
        sidebar?.expand();
        break;
      case 'collapse_all':
        sidebar?.setAllCollapsed(true);
        break;
      case 'expand_all':
        sidebar?.setAllCollapsed(false);
        break;
      case 'switch_nav_mode':
        openLibrary();
        break;
      case 'toggle_zen_mode':
        toggleZen();
        break;
      case 'shrink_nav_panel':
        prefs.step('nav_width', -1);
        break;
      case 'expand_nav_panel':
        prefs.step('nav_width', 1);
        break;
      case 'reset_nav_panel_width':
        prefs.update({ nav_width: NAV_WIDTH.initial });
        break;
      // Wider margins mean a narrower column of text.
      case 'increase_margin':
        prefs.step('measure', -1);
        break;
      case 'decrease_margin':
        prefs.step('measure', 1);
        break;
      case 'toggle_justify_text':
        if (!paged) prefs.update({ justify: !prefs.saved.justify });
        break;
      case 'jump_backward':
        void jumpTo(jumps.back(here()));
        break;
      case 'jump_forward':
        void jumpTo(jumps.forward());
        break;
      case 'lookup_selection':
        void openLookup();
        break;
      case 'copy_selection': {
        const text = doc()?.selectedText();
        if (text) copy(text, true);
        else flash('No text selected');
        break;
      }
      case 'copy_chapter_text':
        void copyChapter();
        break;
      case 'copy_toc_item': {
        // In a PDF this copies the contents entry under the cursor; in a
        // book it copies the chapter, as in the terminal app.
        const title = nav ? sidebar?.selectedTitle() : null;
        if (!paged) void copyChapter();
        else if (title) copy(title, true);
        else flash('Move to the contents first (Tab), then copy the entry there');
        break;
      }
      // Yanking needs the cursor; in reading mode the key does nothing.
      case 'start_yank':
        break;
      case 'cancel':
        message = null;
        if (found) clearFind();
        else if (raw !== null) raw = null;
        void closePanel();
        break;
      case 'start_search':
        if (nav) ask('contents');
        else if (book) ask('find');
        break;
      case 'next_search_match':
      case 'prev_search_match':
        if (!nav) flash('Press / first, and type what to find');
        else if (!seekWords) flash('Press / first, and type what to find');
        else if (!sidebar?.seek(seekWords, action === 'next_search_match' ? 1 : -1)) {
          flash(`Nothing found for “${seekWords}”`);
        }
        break;
      case 'open_book_search':
        void openSearch(false);
        break;
      case 'open_book_search_fresh':
        void openSearch(true);
        break;
      case 'toggle_book_stats':
        if (book) openOver('stats');
        break;
      case 'open_external_viewer':
        openInViewer();
        break;
      case 'toggle_raw_html':
        void toggleRaw();
        break;
      case 'go_to_page':
        if (paged) ask('page');
        break;
      case 'open_highlight_palette':
        openPalette();
        break;
      case 'add_comment':
        void addNote();
        break;
      case 'delete_comment':
        removeHere();
        break;
      case 'toggle_comments_viewer':
      case 'toggle_marks_list':
        if (notes) void closePanel();
        else void openNotes(null, false);
        break;
      case 'enter_comment_nav':
        void openNotes(null, false);
        break;
      case 'export_comments':
        void exportNotes();
        break;
      case 'set_mark':
        pendingMark = 'set';
        break;
      case 'goto_mark':
        pendingMark = 'goto';
        break;
      case 'toggle_pdf_page_layout':
        flash('Two columns side by side are for PDFs; a book’s text is one column here');
        break;
      case 'toggle_pdf_watching':
        flash('Watching for changes is only for PDF files');
        break;
      default:
        // A PDF command given in a book, or the reverse.
        break;
    }
  }

  function onKey(event: KeyboardEvent) {
    // Text fields take their keys raw; the keymap only sees what is typed elsewhere.
    const typing =
      event.target instanceof HTMLInputElement || event.target instanceof HTMLTextAreaElement;
    if (!keymap || event.metaKey || typing) return;
    const token = keyToken(event);
    if (!token) return;

    if (menu) {
      event.preventDefault();
      menuPanel?.handleKey(token);
      return;
    }

    if (picture) {
      event.preventDefault();
      pictureView?.handleKey(token);
      return;
    }

    // A panel beside the text takes every key while it has the focus, like
    // the terminal app's popups; Tab hands the focus back to the text.
    const panel =
      view !== 'reader'
        ? null
        : focus === 'lookup'
          ? lookupPanel
          : focus === 'notes'
            ? notesPanel
            : focus === 'search'
              ? searchPanel
              : null;
    if (panel) {
      event.preventDefault();
      if (token === 'Tab') focus = 'content';
      else panel.handleKey(token);
      return;
    }

    if (view === 'reader' && pendingMark) {
      event.preventDefault();
      takeMarkKey(token);
      return;
    }

    // A half-typed cursor command (a count, the character for `f`, the
    // motion for `y`) takes the key before any binding sees it.
    const inVim = view === 'reader' && focus === 'content' && vimMode !== 'reading';
    if (inVim && sequence.keys.length === 0 && doc()?.vimKey(token)) {
      event.preventDefault();
      return;
    }

    const keys = [...sequence.keys, token];
    const paged = Boolean(book?.pdf);
    const global = keymap.lookup('global', keys);
    // The leader key lists what can follow it, and waits for a choice.
    const stay = keys[0] === 'Space';

    if (view !== 'reader' && view !== 'library') {
      // The screen's own keys, plus the binding that opened it to close it again.
      const { context, openers } = SCREENS[view];
      const screen = { appearance, help, history: historyScreen, stats: statsScreen }[view];
      const local = keymap.lookup(context, keys);
      const wider = global.kind === 'found' ? global.action : null;
      if (wider !== null && openers.includes(wider)) {
        event.preventDefault();
        sequence.clear();
        screen?.handle('cancel');
      } else if (local.kind === 'found') {
        event.preventDefault();
        sequence.clear();
        screen?.handle(local.action);
      } else if (wider === 'toggle_help') {
        event.preventDefault();
        sequence.clear();
        openOver('help');
      } else if (global.kind === 'prefix' || local.kind === 'prefix') {
        event.preventDefault();
        sequence.push(token, stay);
      } else {
        sequence.clear();
      }
      return;
    }

    const context =
      view === 'library' || focus === 'nav'
        ? 'nav'
        : paged
          ? inVim
            ? 'pdf_normal'
            : 'pdf'
          : inVim
            ? 'epub_normal'
            : 'content';
    let local = keymap.lookup(context, keys);
    // As in the terminal app, a key that normal mode has no use for keeps
    // its reading-mode meaning (Ctrl+o, Tab, Enter on a link, `q`), and a
    // PDF takes the keys a book has that it has no other use for.
    if (local.kind === 'none' && context === 'pdf_normal') local = keymap.lookup('pdf', keys);
    if (local.kind === 'none' && context !== 'nav') local = keymap.lookup('content', keys);

    const hit = global.kind === 'found' ? global : local.kind === 'found' ? local : null;
    if (hit) {
      event.preventDefault();
      sequence.clear();
      run(hit.action, event);
    } else if (global.kind === 'prefix' || local.kind === 'prefix') {
      event.preventDefault();
      sequence.push(token, stay);
    } else {
      sequence.clear();
      // `N` goes back through what `/` found, where it has no other meaning.
      if (token === 'N' && found && view === 'reader' && focus === 'content') {
        event.preventDefault();
        void findNext(-1);
      }
    }
  }

  async function runPerf() {
    if (!book || book.chapter_count === 0 || book.pdf) return;
    const round = (n: number) => Math.round(n * 100) / 100;
    const chapters = [];
    let longest = { index: 0, bytes: -1 };
    for (let i = 0; i < book.chapter_count; i++) {
      const start = performance.now();
      const loaded = await api.loadChapter(i);
      const bytes = JSON.stringify(loaded.blocks).length;
      chapters.push({
        index: i,
        load_ms: round(performance.now() - start),
        blocks: loaded.blocks.length,
        json_bytes: bytes,
      });
      if (bytes > longest.bytes) longest = { index: i, bytes };
    }

    const frames = () => new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r)));
    /** Times a chapter coming up: until it first shows, until all of it is
        on the page, and the longest the window went without a frame. */
    const timed = async (open: () => Promise<void>) => {
      const start = performance.now();
      let frameAt = start;
      let longestFrame = 0;
      let watching = true;
      const watch = () => {
        const now = performance.now();
        longestFrame = Math.max(longestFrame, now - frameAt);
        frameAt = now;
        if (watching) requestAnimationFrame(watch);
      };
      requestAnimationFrame(watch);
      await open();
      await frames();
      const firstPaint = round(performance.now() - start);
      await reader?.settled();
      await frames();
      watching = false;
      return {
        first_paint_ms: firstPaint,
        load_and_render_ms: round(performance.now() - start),
        longest_frame_ms: round(longestFrame),
      };
    };
    const fromTop = await timed(async () => {
      await load(longest.index);
      reader?.resetScroll();
    });
    // Landing far into the chapter from another one, as on following a
    // link or opening the book where it was left.
    const node = Math.floor((chapters[longest.index]?.blocks ?? 0) * 0.9);
    const elsewhere = longest.index === 0 ? 1 : 0;
    let deep = null;
    if (elsewhere < book.chapter_count) {
      await load(elsewhere);
      const place = { chapter: longest.index, node, offset: 0, scrollTop: 0, layout: '', cursor: null };
      deep = { node, ...(await timed(() => jumpTo(place))) };
      reader?.resetScroll();
    }
    await document.fonts.ready;

    const targets = reader?.perfTargets();
    if (!targets) return;
    const scrolling = await measureScrolling(
      targets.scrollEl,
      targets.scroller,
      targets.lineHeight,
    );
    await api.perfReport({
      user_agent: navigator.userAgent,
      viewport: { width: innerWidth, height: innerHeight, pixel_ratio: devicePixelRatio },
      measured_chapter: { index: longest.index, ...fromTop, deep_landing: deep },
      document_height_px: targets.scrollEl.scrollHeight,
      chapters,
      scrolling,
    });
  }

  /** What the backend says of its own accord: a watched PDF was read
      again, or an editor asked to see a line of TeX source. */
  async function listenToBackend() {
    await listen<{ pages: [number, number][]; toc: TocEntry[] }>('pdf-reloaded', ({ payload }) => {
      if (!book?.pdf) return;
      book = { ...book, chapter_count: payload.pages.length, toc: payload.toc };
      pdfView?.reloaded(payload.pages);
      void loadNotes();
      flash('The file changed and was read again');
    });
    await listen<{ file: string; line: number; target: PdfSpot | null }>(
      'synctex-forward',
      ({ payload }) => {
        const name = payload.file.slice(payload.file.lastIndexOf('/') + 1);
        if (!payload.target) {
          flash(`SyncTeX: nothing in the document comes from ${name}:${payload.line}`);
          return;
        }
        saveJump();
        pdfView?.showSpot(payload.target);
        flash(`SyncTeX: ${name}:${payload.line} is on page ${payload.target.page + 1}`);
      },
    );
  }

  async function start() {
    try {
      prefs.onerror = (text) => flash(text, true);
      // The window asks before it closes, so that the place in the book and
      // any setting still waiting to be saved are written down first.
      await listen('close-requested', quit);
      for (const warning of await prefs.load()) flash(warning, true);
      void matchLetterHeight();
      const launch = await api.launchInfo();
      libraryDir = launch.library_dir;
      keymap = new Keymap(await api.keymap());
      await listenToBackend();
      books = await api.listBooks();
      void loadRecency();
      if (launch.open_path) await openBook(launch.open_path);
      if (!book) view = 'library';
      if (launch.perf) await runPerf();
      if (launch.script) await runScript(launch.script as ScriptStep[]);
    } catch (e) {
      flash(String(e), true);
    }
  }

  function focusNow(el: HTMLInputElement) {
    el.focus();
  }

  $effect(() => prefs.apply());

  void start();
</script>

<svelte:window
  onkeydown={onKey}
  onkeyup={(event) => {
    if (event.code === heldKey) releaseScroll();
  }}
  onblur={() => {
    releaseScroll();
    menu = null;
    sequence.clear();
    void flushPlace();
  }}
  oncontextmenu={showMenu}
/>

{#snippet statusLine()}
  <footer class:wide={view !== 'reader' && view !== 'appearance'}>
    <span class="mode {mode.kind}"><i aria-hidden="true"></i>{mode.label}</span>
    {#if prompt}
      <label class="prompt">
        <span>{prompt.kind === 'page' ? 'Go to page' : '/'}</span>
        <input
          bind:value={prompt.value}
          use:focusNow
          oninput={promptTyped}
          onkeydown={promptKey}
          onblur={() => void commitPrompt()}
          inputmode={prompt.kind === 'page' ? 'numeric' : 'text'}
          aria-label={prompt.kind === 'page' ? 'Page number' : 'Find'}
          spellcheck="false"
          autocomplete="off"
        />
      </label>
      {#if prompt.kind === 'find' && found?.count}
        <span>{found.index} of {found.count}</span>
      {/if}
      <span class="hint"><kbd>Enter</kbd>{prompt.kind === 'page' ? 'go' : 'keep'}</span>
      <span class="hint"><kbd>Esc</kbd>cancel</span>
    {:else}
      <span class="track" aria-hidden="true">
        {#if view === 'reader' && current !== null}
          <span class="fill" style:transform="scaleX({progress})"></span>
        {/if}
      </span>
      {#if view === 'reader' && (vimPending || pending.length > 0 || pendingMark)}
        <span class="pending">
          {vimPending}{pending.join(' ')}{pendingMark === 'set' ? 'm' : pendingMark === 'goto' ? '`' : ''}
        </span>
      {/if}
      {#if message}<span class:error={message.error} class="message">{message.text}</span>{/if}
      {#if view === 'help'}
        <span class="hint"><kbd>j</kbd><kbd>k</kbd>scroll</span>
        <span class="hint"><kbd>Esc</kbd>close</span>
      {:else if view === 'appearance'}
        <span class="hint"><kbd>j</kbd><kbd>k</kbd>move</span>
        <span class="hint"><kbd>Tab</kbd>next list</span>
        <span class="hint"><kbd>Enter</kbd>keep</span>
        <span class="hint"><kbd>Esc</kbd>close</span>
      {:else if view === 'history'}
        <span class="hint"><kbd>Enter</kbd>open</span>
        <span class="hint"><kbd>Tab</kbd>this library or all</span>
        <span class="hint"><kbd>d</kbd><kbd>d</kbd>forget</span>
        <span class="hint"><kbd>Esc</kbd>close</span>
      {:else if view === 'stats'}
        <span class="hint"><kbd>Enter</kbd>go there</span>
        <span class="hint"><kbd>Esc</kbd>close</span>
      {:else if view === 'library'}
        {#if shelf}
          <span class="here">{shelf.details?.title ?? shelf.book.title}</span>
          {#if shelf.details?.author}<span>{shelf.details.author}</span>{/if}
          <span>{shelf.book.format.toUpperCase()}</span>
          <span class="hint"><kbd>Enter</kbd>open</span>
        {/if}
      {:else if book?.pdf && pdfInfo}
        {#if found}<span class="finding">/{found.query}</span>{/if}
        {#if prefs.saved.pdf_spread}<span>two pages</span>{/if}
        {#if !pdfInfo.themed}<span>original colours</span>{/if}
        {#if pdfInfo.watching}<span>watching file</span>{/if}
        <span>page {pdfInfo.page + 1} of {pdfInfo.pages}</span>
      {:else if chapter}
        {#if raw !== null}<span>HTML source</span>{/if}
        {#if found}<span class="finding">/{found.query} · {found.index} of {found.count}</span>{/if}
        <span>{Math.round(progress * 100)}%</span>
        <span>{minutesLeft > 0 ? `${minutesLeft} min left in chapter` : 'end of chapter'}</span>
      {/if}
    {/if}
  </footer>
{/snippet}

<div class="app">
  {#if sidebarShown}
    <Sidebar
      bind:this={sidebar}
      {book}
      {toc}
      currentChapter={current}
      {currentNode}
      paged={Boolean(book?.pdf)}
      focused={focus === 'nav'}
      width={prefs.saved.nav_width}
      onopenentry={(entry) => {
        if (entry.chapter !== null) void showChapter(entry.chapter, entry.anchor, undefined, entry.node);
      }}
      onlibrary={openLibrary}
      onfocus={() => (focus = 'nav')}
    />
  {/if}

  <div
    class="stage"
    class:zen
    class:bordered={zen && prefs.saved.zen_border}
    class:pages={Boolean(book?.pdf)}
    class:left-edge={!sidebarShown}
    class:right-edge={!lookup && !notes && !search}
  >
    <main onpointerdown={() => (focus = 'content')}>
      {#if !zen}
        <header>
          <button
            class="icon"
            aria-label="Toggle sidebar"
            aria-pressed={sidebarOpen}
            onclick={() => (sidebarOpen = !sidebarOpen)}
          >
            <svg aria-hidden="true" width="18" height="18" viewBox="0 0 18 18" fill="none" stroke="currentColor" stroke-width="1.3"><rect x="2" y="3" width="14" height="12" rx="3"></rect><path d="M7 3v12"></path></svg>
          </button>
          <div class="running">
            {#if book}
              <span class="book">{book.title}</span>
              {#if chapter && scrollTop > CHAPTER_HEAD_AFTER}
                <span aria-hidden="true">·</span>
                <span class="chapter">{chapter.title}</span>
              {/if}
            {/if}
          </div>
          {#if book}
            <button
              class="icon"
              aria-label="Notes and bookmarks"
              title="Notes and bookmarks"
              aria-pressed={notes !== null}
              onclick={() => (notes ? void closePanel() : void openNotes(null, false))}
            >
              <svg aria-hidden="true" width="18" height="18" viewBox="0 0 18 18" fill="none" stroke="currentColor" stroke-width="1.3" stroke-linejoin="round"><path d="M5 2.5h8v13l-4-3.2-4 3.2z"></path></svg>
            </button>
          {/if}
        </header>
      {/if}
      <div class="page">
        {#if book?.pdf}
          {#key book.path}
            <PdfView
              bind:this={pdfView}
              meta={book.pdf}
              {annotations}
              marks={pageMarks}
              onprogress={(fraction, top) => {
                progress = fraction;
                scrollTop = top;
                savePlaceSoon();
              }}
              oninfo={(info) => (pdfInfo = info)}
              onvim={(mode, pending) => {
                if (mode !== 'reading') releaseScroll();
                vimMode = mode;
                vimPending = pending;
              }}
              oncopy={(text) => copy(text, false)}
              onnotice={flash}
              onexternal={openExternal}
              onleave={saveJump}
            />
          {/key}
        {:else if chapter && book}
          <Reader
            bind:this={reader}
            {chapter}
            language={book.language}
            justify={prefs.saved.justify}
            dropCaps={prefs.saved.drop_caps}
            annotations={chapterNotes}
            marks={chapterMarks}
            {raw}
            onprogress={(fraction, top) => {
              progress = fraction;
              scrollTop = top;
              savePlaceSoon();
            }}
            onlink={followLink}
            {chapterFor}
            onimage={(src, alt) => (picture = { src, alt })}
            onvim={(mode, pending) => {
              if (mode !== 'reading') releaseScroll();
              vimMode = mode;
              vimPending = pending;
            }}
            oncopy={(text) => copy(text, false)}
            onnotice={(text) => flash(text)}
            onfound={(count, index) => {
              if (found) found = { ...found, count, index };
            }}
          />
        {:else if !book}
          <div class="empty">
            <p class="lead">Choose a book from the library</p>
            <p>{libraryDir}</p>
          </div>
        {/if}
        {#if !zen && !book?.pdf}
          <div class="fade top" aria-hidden="true"></div>
          <div class="fade bottom" aria-hidden="true"></div>
        {/if}
      </div>
    </main>
    {#if !zen}{@render statusLine()}{/if}
  </div>

  {#if lookup}
    {#key lookup}
      <Lookup
        bind:this={lookupPanel}
        query={lookup.query}
        targets={lookup.targets}
        focused={focus === 'lookup'}
        onclose={() => void closePanel()}
        onflash={flash}
      />
    {/key}
  {:else if notes}
    <Notes
      bind:this={notesPanel}
      {annotations}
      {marks}
      {titleOf}
      quoteOf={quoteFor}
      editing={notes.editing}
      focused={focus === 'notes'}
      onjump={(row) => {
        if (row.mark) void gotoMark(row.mark);
        else if (row.note) gotoNote(row.note);
      }}
      onedit={(note) => void editNote(note)}
      onsave={(text) => void saveNote(text)}
      ondiscard={endEdit}
      ondelete={(row) => {
        if (row.mark) void removeMark(row.mark);
        else if (row.note) void removeAnnotation(row.note);
      }}
      oncopy={(text) => {
        if (text) copy(text, true);
      }}
      onexport={() => void exportNotes()}
      onclose={() => void closePanel()}
    />
  {:else if search}
    {#key search}
      <Search
        bind:this={searchPanel}
        initial={search.initial}
        paged={Boolean(book?.pdf)}
        focused={focus === 'search'}
        onjump={(hit) => void gotoHit(hit)}
        onkeep={(query, hits) => (lastSearch = query ? { query, hits } : null)}
        onclose={() => void closePanel()}
        onerror={(text) => flash(text, true)}
      />
    {/key}
  {/if}

  {#if showing('library')}
    <div class="overlay shelf">
      <Library
        bind:this={library}
        books={shelfBooks}
        directory={libraryDir}
        order={SORT_NAMES[prefs.saved.library_sort]}
        currentBook={book?.path ?? null}
        onopen={(b) => void openBook(b.path)}
        onselect={(b, details) => (shelf = b ? { book: b, details } : null)}
      />
      {@render statusLine()}
    </div>
  {/if}
  {#if showing('history')}
    <div class="overlay">
      <History
        bind:this={historyScreen}
        currentBook={book?.path ?? null}
        status={statusLine}
        onopen={(path) => void openBook(path)}
        onnotice={flash}
        onclose={closeOver}
      />
    </div>
  {/if}
  {#if showing('stats') && book}
    <div class="overlay">
      <Stats
        bind:this={statsScreen}
        title={book.title}
        current={current ?? 0}
        status={statusLine}
        onjump={(index) => {
          closeOver();
          void showChapter(index);
        }}
        onnotice={flash}
        onclose={closeOver}
      />
    </div>
  {/if}
  {#if showing('appearance')}
    <div class="overlay">
      <Appearance
        bind:this={appearance}
        {chapter}
        title={book?.title ?? null}
        status={statusLine}
        onclose={closeOver}
      />
    </div>
  {/if}
  {#if view === 'help' && keymap}
    <div class="overlay">
      <Help bind:this={help} {keymap} status={statusLine} onclose={closeOver} />
    </div>
  {/if}

  {#if picture}
    <ImageView bind:this={pictureView} src={picture.src} alt={picture.alt} onclose={() => (picture = null)} />
  {/if}

  {#if keymap && pending[0] === 'Space'}
    <div class="sheet">
      <Leader {keymap} prefix={pending} pdf={Boolean(book?.pdf) && view === 'reader'} />
    </div>
  {/if}

  {#if menu}
    <Menu bind:this={menuPanel} x={menu.x} y={menu.y} items={menu.items} onclose={() => (menu = null)} />
  {/if}
</div>

<style>
  .app {
    position: relative;
    height: 100%;
    display: flex;
  }

  .stage {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
  }

  /* The page is a sheet lying on the window; in zen mode it is the window. */
  main {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
    margin-top: 8px;
    overflow: hidden;
    border-radius: 12px;
    background: var(--page);
    box-shadow:
      0 0 0 1px var(--hairline),
      0 16px 48px var(--shadow);
  }

  .left-edge main {
    margin-left: 8px;
  }

  .right-edge main {
    margin-right: 8px;
  }

  .zen main {
    margin: 0;
    border-radius: 0;
    box-shadow: none;
  }

  /* Zen mode with its border kept: the sheet stays a sheet, with nothing round it. */
  .zen.bordered main {
    margin: 8px;
    border-radius: 12px;
    box-shadow:
      0 0 0 1px var(--hairline),
      0 16px 48px var(--shadow);
  }

  /* A PDF's pages are sheets of their own, lying on the window. */
  .pages main {
    background: var(--chrome);
    box-shadow: none;
  }

  header {
    flex: none;
    height: 56px;
    display: grid;
    grid-template-columns: 36px minmax(0, 1fr) 36px;
    align-items: center;
    gap: 12px;
    padding: 0 10px;
  }

  .icon {
    width: 36px;
    height: 36px;
    display: flex;
    align-items: center;
    justify-content: center;
    border-radius: 8px;
    color: var(--muted);
  }

  .icon:hover {
    background: var(--raised);
    color: var(--text);
  }

  .icon[aria-label='Notes and bookmarks'][aria-pressed='true'] {
    color: var(--accent);
  }

  .running {
    min-width: 0;
    display: flex;
    align-items: baseline;
    justify-content: center;
    gap: 10px;
    font-family: var(--font-reading);
    font-size-adjust: var(--letter-height, none);
    font-size: 13.5px;
    color: var(--muted);
    white-space: nowrap;
  }

  .running .book {
    font-variant-caps: all-small-caps;
    letter-spacing: 0.14em;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .running .chapter {
    font-style: italic;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .page {
    position: relative;
    flex: 1;
    min-height: 0;
  }

  /* Lines of text dissolve into the sheet's edges instead of being cut by them. */
  .fade {
    position: absolute;
    left: 0;
    right: 0;
    pointer-events: none;
  }

  .fade.top {
    top: 0;
    height: 16px;
    background: linear-gradient(var(--page), transparent);
  }

  .fade.bottom {
    bottom: 0;
    height: 32px;
    background: linear-gradient(transparent, var(--page));
  }

  .overlay {
    position: absolute;
    inset: 0;
    display: flex;
    background: var(--chrome);
  }

  .overlay.shelf {
    flex-direction: column;
  }

  /* The leader key's list rises from the foot of the window, over whatever is there. */
  .sheet {
    position: absolute;
    inset: auto 0 0;
    z-index: 12;
    padding-bottom: 8px;
    filter: drop-shadow(0 -8px 24px var(--shadow));
  }

  .empty {
    height: 100%;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 6px;
    color: var(--muted);
  }

  .empty p {
    margin: 0;
  }

  .empty .lead {
    font-family: var(--font-reading);
    font-size: 22px;
    color: var(--heading);
  }

  footer {
    flex: none;
    height: 36px;
    display: flex;
    align-items: center;
    gap: 14px;
    padding: 0 22px 0 14px;
    font-family: var(--font-mono);
    font-size: 11px;
    color: var(--muted);
    white-space: nowrap;
    overflow: hidden;
  }

  footer.wide {
    padding: 0 64px;
  }

  .mode {
    display: flex;
    align-items: center;
    gap: 8px;
    letter-spacing: 0.14em;
    color: var(--link);
  }

  .mode i {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: currentColor;
  }

  .mode.nav {
    color: var(--accent);
  }

  .mode.reading {
    color: var(--muted);
  }

  .mode.visual,
  .mode.visual-line {
    color: var(--note);
  }

  .track {
    position: relative;
    flex: 1;
    min-width: 24px;
    height: 1px;
    background: var(--hairline);
  }

  .fill {
    position: absolute;
    inset: 0;
    background: var(--accent);
    transform-origin: left;
  }

  .here {
    color: var(--text);
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .hint {
    display: flex;
    align-items: center;
    gap: 6px;
  }

  .pending,
  .finding {
    color: var(--accent);
  }

  .prompt {
    flex: 1;
    min-width: 0;
    display: flex;
    align-items: center;
    gap: 6px;
    color: var(--accent);
  }

  .prompt input {
    flex: 1;
    min-width: 0;
    height: 24px;
    padding: 0;
    border: 0;
    background: none;
    color: var(--heading);
    font: inherit;
    font-size: 12px;
    outline: none;
  }

  .message {
    color: var(--text);
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .message.error {
    color: var(--error);
  }
</style>
