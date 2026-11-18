<script lang="ts" module>
  import { SvelteSet } from 'svelte/reactivity';
  import type { BookDetails } from './api';

  // What each book says about itself, kept for as long as the app runs so
  // the shelf only reads a book once.
  const known = new Map<string, BookDetails>();
  // Covers already on screen once, so they do not fade in a second time.
  const shown = new SvelteSet<string>();
</script>

<script lang="ts">
  import { api, bookCover, type BookSummary } from './api';
  import { coverFor } from './covers';

  let {
    books,
    directory,
    order,
    currentBook,
    onopen,
    onselect,
  }: {
    books: BookSummary[];
    directory: string;
    /** How the books are ordered, in words, to show beside the folder. */
    order: string;
    currentBook: string | null;
    onopen: (book: BookSummary) => void;
    /** Reports the book under the cursor, for the status line. */
    onselect: (book: BookSummary | null, details: BookDetails | null) => void;
  } = $props();

  let details = $state(new Map(known));
  let cursor = $state(0);
  let gridEl: HTMLElement;

  $effect(() => {
    const current = books.findIndex((b) => b.path === currentBook);
    if (current >= 0) cursor = current;
  });

  $effect(() => {
    if (cursor >= books.length) cursor = Math.max(0, books.length - 1);
    gridEl?.children[cursor]?.scrollIntoView({ block: 'nearest' });
  });

  $effect(() => {
    const book = books[cursor] ?? null;
    onselect(book, book ? (details.get(book.path) ?? null) : null);
  });

  // Titles and authors come from inside each book, read one at a time in
  // the background; until one arrives the cover shows the file's name.
  $effect(() => {
    let cancelled = false;
    void (async () => {
      for (const book of books) {
        if (cancelled) return;
        if (known.has(book.path) || book.format === 'html') continue;
        try {
          known.set(book.path, await api.bookDetails(book.path));
        } catch (e) {
          console.warn(`No details for ${book.path}: ${e}`);
          known.set(book.path, { title: null, author: null, cover: false });
        }
        if (!cancelled) details = new Map(known);
      }
    })();
    return () => {
      cancelled = true;
    };
  });

  function columns(): number {
    return Math.max(1, getComputedStyle(gridEl).gridTemplateColumns.split(' ').length);
  }

  function rowsInView(): number {
    const cover = gridEl.children[0]?.getBoundingClientRect().height ?? 0;
    return cover > 0 ? Math.max(1, Math.floor(gridEl.clientHeight / cover)) : 1;
  }

  function moveTo(index: number) {
    cursor = Math.max(0, Math.min(books.length - 1, index));
  }

  export function move(columnDelta: number, rowDelta: number) {
    const target = cursor + columnDelta + rowDelta * columns();
    // Moving down from the last full row lands on the last book.
    if (rowDelta > 0 && target >= books.length) {
      const lastRowStart = Math.floor((books.length - 1) / columns()) * columns();
      if (cursor < lastRowStart) moveTo(books.length - 1);
      return;
    }
    if (target >= 0) moveTo(target);
  }

  export function page(pages: number) {
    move(0, Math.round(pages * rowsInView()) || Math.sign(pages));
  }

  export function toEdge(edge: 'top' | 'bottom') {
    moveTo(edge === 'top' ? 0 : books.length - 1);
  }

  /** Moves the cursor to a book, as a right click on it does. */
  export function choose(path: string) {
    const index = books.findIndex((b) => b.path === path);
    if (index >= 0) cursor = index;
  }

  export function select() {
    const book = books[cursor];
    if (book) onopen(book);
  }

  /** Where the cursor is, to put it back if a search is given up. */
  export function place(): number {
    return cursor;
  }

  export function restore(index: number) {
    moveTo(index);
  }

  /** Moves the cursor to a book whose title or author has the words:
      the first from where the cursor is (`step` 0), or the next or the
      one before, round the ends. Returns whether there is one. */
  export function seek(words: string, step: 0 | 1 | -1): boolean {
    const needle = words.trim().toLowerCase();
    if (!needle || books.length === 0) return false;
    const matches = (index: number) => {
      const book = books[index]!;
      const about = details.get(book.path);
      return [about?.title, about?.author, book.title].some((text) => text?.toLowerCase().includes(needle));
    };
    const direction = step === 0 ? 1 : step;
    for (let n = step === 0 ? 0 : 1; n <= books.length; n++) {
      const index = (cursor + direction * n + books.length * n) % books.length;
      if (matches(index)) {
        cursor = index;
        return true;
      }
    }
    return false;
  }
</script>

<section class="library" aria-label="Library">
  <header>
    <h1>Library</h1>
    <span class="path">{directory} · {order}</span>
  </header>

  <div class="shelf">
    <div class="grid" bind:this={gridEl}>
      {#each books as book, index (book.path)}
        {@const cover = coverFor(book.path)}
        {@const about = details.get(book.path)}
        <button
          class="book"
          class:cursor={index === cursor}
          data-book={book.path}
          aria-current={book.path === currentBook ? 'true' : undefined}
          onclick={() => {
            cursor = index;
            onopen(book);
          }}
        >
          <span class="volume">
            <span class="back" aria-hidden="true" style:background={cover.cloth}></span>
            <span class="pages" aria-hidden="true"></span>
            <span class="cover" style:background={cover.cloth} style:color={cover.ink}>
            <span class="head">
              <span class="name">{about?.title ?? book.title}</span>
              <span class="rule" aria-hidden="true"></span>
            </span>
            <span class="by">{about?.author ?? book.format}</span>
            {#if about?.cover}
              <img
                class:shown={shown.has(book.path)}
                src={bookCover(book.path)}
                alt=""
                loading="lazy"
                decoding="async"
                onload={() => shown.add(book.path)}
              />
            {/if}
            <span class="binding" aria-hidden="true"></span>
            {#if book.path === currentBook}
              <span class="marker" aria-hidden="true"><span class="ribbon"></span></span>
            {/if}
            </span>
          </span>
        </button>
      {/each}
    </div>
    {#if books.length === 0}
      <p class="empty">No books in this folder.</p>
    {/if}
  </div>
</section>

<style>
  .library {
    flex: 1;
    min-width: 0;
    min-height: 0;
    display: flex;
    flex-direction: column;
  }

  header {
    flex: none;
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: 24px;
    padding: 44px 64px 0;
  }

  h1 {
    margin: 0;
    font-family: var(--font-reading);
    font-size-adjust: var(--letter-height, none);
    font-size: 32px;
    font-weight: 500;
    line-height: 1.2;
    letter-spacing: -0.01em;
    color: var(--heading);
  }

  .path {
    min-width: 0;
    font-family: var(--font-mono);
    font-size: 11.5px;
    color: var(--muted);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .shelf {
    position: relative;
    flex: 1;
    min-height: 0;
    display: flex;
  }

  /* Books scroll out under the status line rather than stopping at an edge. */
  .shelf::after {
    content: '';
    position: absolute;
    inset: auto 0 0;
    height: 56px;
    background: linear-gradient(transparent, var(--chrome));
    pointer-events: none;
  }

  .grid {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(170px, 1fr));
    gap: 48px 44px;
    align-content: start;
    padding: 44px 64px 72px;
    scroll-padding: 44px 0 72px;
    scrollbar-width: none;
  }

  /* Each book is a solid volume, turned a little so that the block of
     pages shows at its fore-edge. The one under the cursor turns to face
     the reader and comes forward. */
  .book {
    display: block;
    perspective: 2400px;
  }

  .volume {
    --depth: 68px;
    --tilt: -12deg;
    --lift: 0px;
    position: relative;
    display: block;
    aspect-ratio: 2 / 3;
    transform-style: preserve-3d;
    transform: rotateY(var(--tilt)) translateZ(var(--lift));
    transition:
      --tilt 240ms cubic-bezier(0.2, 0.7, 0.2, 1),
      --lift 240ms cubic-bezier(0.2, 0.7, 0.2, 1);
  }

  .book:hover .volume {
    --tilt: -6deg;
  }

  .book.cursor .volume {
    --tilt: 0deg;
    --lift: 64px;
  }

  @media (prefers-reduced-motion: reduce) {
    .volume {
      transition: none;
    }
  }

  .back {
    position: absolute;
    inset: 0;
    border-radius: 2px 4px 4px 2px;
    transform: translateZ(calc(var(--depth) / -2));
    filter: brightness(0.6);
    box-shadow: 0 14px 28px var(--shadow);
  }

  /* The fore-edge: a strip as wide as the book is thick, swung round its
     right side to stand between the two boards, which overhang it. */
  .pages {
    position: absolute;
    top: 3px;
    bottom: 3px;
    right: 2px;
    width: var(--depth);
    transform-origin: right center;
    transform: translateZ(calc(var(--depth) / -2)) rotateY(90deg);
    background: repeating-linear-gradient(90deg, #efe8d6 0 1px, #cfc7b3 1px 2px);
  }

  .cover {
    position: absolute;
    inset: 0;
    display: flex;
    flex-direction: column;
    justify-content: space-between;
    padding: 24px 16px 18px 28px;
    border-radius: 2px 4px 4px 2px;
    transform: translateZ(calc(var(--depth) / 2));
    outline: 2px solid transparent;
    outline-offset: 5px;
    font-family: var(--font-reading);
    font-size-adjust: var(--letter-height, none);
    text-align: left;
    overflow: hidden;
  }

  .book.cursor .cover {
    outline-color: var(--accent);
  }

  /* The cover that comes with the book lies over the drawn one, which
     stays underneath for books that have none or whose image fails. */
  img {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    object-fit: cover;
    opacity: 0;
    transition: opacity 180ms ease;
  }

  img.shown {
    opacity: 1;
  }

  /* Drawn over whichever cover shows: the spine's rounded edge and the
     groove of its hinge, and light falling across the board from the left. */
  .binding {
    position: absolute;
    inset: 0;
    border-radius: inherit;
    background:
      linear-gradient(
        90deg,
        rgb(0 0 0 / 0.3) 0,
        rgb(0 0 0 / 0.06) 2%,
        rgb(255 255 255 / 0.18) 3.6%,
        rgb(255 255 255 / 0.04) 5%,
        rgb(0 0 0 / 0.26) 6.4%,
        rgb(0 0 0 / 0.06) 8.4%,
        rgb(0 0 0 / 0) 12%
      ),
      linear-gradient(
        100deg,
        rgb(255 255 255 / 0.1) 0%,
        rgb(255 255 255 / 0) 40%,
        rgb(0 0 0 / 0) 60%,
        rgb(0 0 0 / 0.14) 100%
      );
    box-shadow: inset 0 0 0 1px rgb(255 255 255 / 0.06);
  }

  .ribbon {
    position: absolute;
    top: 0;
    right: 18px;
    width: 12px;
    height: 26px;
    background: var(--accent);
    clip-path: polygon(0 0, 100% 0, 100% 100%, 50% 76%, 0 100%);
  }

  /* A shadow keeps the ribbon readable over a pale cover; it sits on a
     wrapper because a clipped element cannot cast one. */
  .marker {
    position: absolute;
    inset: 0;
    filter: drop-shadow(0 1px 2px rgb(0 0 0 / 0.45));
  }

  .head {
    display: block;
    min-height: 0;
  }

  .name {
    font-size: 19px;
    font-weight: 500;
    line-height: 1.18;
    text-wrap: balance;
    overflow-wrap: break-word;
    display: -webkit-box;
    -webkit-box-orient: vertical;
    -webkit-line-clamp: 5;
    line-clamp: 5;
    overflow: hidden;
  }

  .rule {
    display: block;
    width: 22px;
    height: 1px;
    margin-top: 14px;
    background: currentColor;
    opacity: 0.55;
  }

  .by {
    display: -webkit-box;
    -webkit-box-orient: vertical;
    -webkit-line-clamp: 2;
    line-clamp: 2;
    overflow: hidden;
    font-size: 12.5px;
    line-height: 1.3;
    letter-spacing: 0.12em;
    font-variant-caps: all-small-caps;
  }

  .empty {
    position: absolute;
    inset: 44px 64px auto;
    margin: 0;
    color: var(--muted);
  }
</style>
