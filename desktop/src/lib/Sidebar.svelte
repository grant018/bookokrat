<script lang="ts" module>
  /** "Chapter 1: The Discovery" or "1. Loomings": the number goes in the margin. */
  const NUMBERED =
    /^(?:chapter\s+(\d{1,3})\s*[:.\u2014\u2013-]?|(\d{1,3})\s*[:.\u2014\u2013-])\s+(\S.*)$/iu;
  const ROW_HEIGHT = 32;
</script>

<script lang="ts">
  import { bookCover, type BookMeta, type TocEntry } from './api';
  import { coverFor } from './covers';

  type Row = {
    key: string;
    depth: number;
    label: string;
    number: string | null;
    hasChildren: boolean;
    collapsed: boolean;
    entry: TocEntry;
  };

  let {
    book,
    toc,
    currentChapter,
    currentNode = null,
    paged,
    focused,
    width,
    onopenentry,
    onlibrary,
    onfocus,
  }: {
    book: BookMeta | null;
    toc: TocEntry[];
    currentChapter: number | null;
    /** The chapter block at the top of the view, when known. */
    currentNode?: number | null;
    /** For a PDF: entries lead to pages, whose numbers are shown. */
    paged: boolean;
    focused: boolean;
    width: number;
    onopenentry: (entry: TocEntry) => void;
    onlibrary: () => void;
    onfocus: () => void;
  } = $props();

  /** The book whose cover image would not load, so its drawn cover shows instead. */
  let failedCover = $state<string | null>(null);
  let collapsed = $state(new Set<string>());
  let cursor = $state(0);
  let listEl: HTMLElement;

  function flatten(entries: TocEntry[], depth: number, prefix: string, out: Row[]) {
    entries.forEach((entry, i) => {
      const key = `${prefix}${i}`;
      const isCollapsed = collapsed.has(key);
      const hasChildren = entry.children.length > 0;
      // A section's margin holds its chevron, so its number stays in the title.
      const numbered = hasChildren || paged ? null : NUMBERED.exec(entry.title);
      out.push({
        key,
        depth,
        label: numbered ? numbered[3]! : entry.title,
        number: numbered ? (numbered[1] ?? numbered[2])! : null,
        hasChildren,
        collapsed: isCollapsed,
        entry,
      });
      if (!isCollapsed) flatten(entry.children, depth + 1, `${key}.`, out);
    });
  }

  let rows = $derived.by((): Row[] => {
    const out: Row[] = [];
    flatten(toc, 0, '', out);
    return out;
  });

  /** Whether any row needs the margin, for a number or a chevron. */
  let margin = $derived(rows.some((r) => r.number !== null || r.hasChildren));

  // A contents list need not run in reading order (this one may end with
  // "Illustrations", which sits near the front of the book), so the row is
  // the one for the latest place at or before this, wherever it is listed.
  // Entries made from headings also say where in their chapter they are.
  let active = $derived.by(() => {
    let found = -1;
    let latest: [number, number] = [-1, -1];
    if (currentChapter === null) return found;
    const here: [number, number] = [currentChapter, currentNode ?? 0];
    const after = (a: [number, number], b: [number, number]) => a[0] > b[0] || (a[0] === b[0] && a[1] > b[1]);
    rows.forEach((row, index) => {
      const { chapter, node } = row.entry;
      if (chapter === null) return;
      const at: [number, number] = [chapter, node ?? 0];
      if (!after(at, here) && after(at, latest)) {
        found = index;
        latest = at;
      }
    });
    return found;
  });

  // While reading, the cursor trails the current chapter so that switching
  // focus to the panel starts from where you are.
  $effect(() => {
    if (!focused && active >= 0) cursor = active;
  });

  $effect(() => {
    if (cursor >= rows.length) cursor = Math.max(0, rows.length - 1);
    listEl?.children[cursor]?.scrollIntoView({ block: 'nearest' });
  });

  function setCollapsed(keys: string[], value: boolean) {
    const next = new Set(collapsed);
    for (const key of keys) {
      if (value) next.add(key);
      else next.delete(key);
    }
    collapsed = next;
  }

  function sectionKeys(entries: TocEntry[], prefix: string, out: string[]) {
    entries.forEach((entry, i) => {
      if (entry.children.length === 0) return;
      out.push(`${prefix}${i}`);
      sectionKeys(entry.children, `${prefix}${i}.`, out);
    });
  }

  export function move(delta: number) {
    cursor = Math.max(0, Math.min(rows.length - 1, cursor + delta));
  }

  export function page(pages: number) {
    const visible = Math.max(1, Math.floor(listEl.clientHeight / ROW_HEIGHT));
    move(Math.round(pages * visible));
  }

  export function toEdge(edge: 'top' | 'bottom') {
    cursor = edge === 'top' ? 0 : Math.max(0, rows.length - 1);
  }

  export function collapse() {
    const row = rows[cursor];
    if (!row) return;
    if (row.hasChildren && !row.collapsed) {
      setCollapsed([row.key], true);
      return;
    }
    const parent = row.key.includes('.') ? row.key.slice(0, row.key.lastIndexOf('.')) : null;
    if (parent !== null) cursor = rows.findIndex((r) => r.key === parent);
  }

  export function expand() {
    const row = rows[cursor];
    if (row?.hasChildren) setCollapsed([row.key], false);
  }

  export function setAllCollapsed(value: boolean) {
    const keys: string[] = [];
    sectionKeys(toc, '', keys);
    setCollapsed(keys, value);
  }

  export function select() {
    const row = rows[cursor];
    if (row) open(row);
  }

  /** The title under the cursor. */
  export function selectedTitle(): string | null {
    return rows[cursor]?.entry.title ?? null;
  }

  export function place(): number {
    return cursor;
  }

  export function restore(index: number) {
    cursor = Math.max(0, Math.min(rows.length - 1, index));
  }

  /** Moves the cursor to an entry whose title has the words: the first
      from where the cursor is (`step` 0), or the next or the one before,
      round the ends. Returns whether there is one. */
  export function seek(words: string, step: 0 | 1 | -1): boolean {
    const needle = words.trim().toLowerCase();
    if (!needle || rows.length === 0) return false;
    const direction = step === 0 ? 1 : step;
    for (let n = step === 0 ? 0 : 1; n <= rows.length; n++) {
      const index = (cursor + direction * n + rows.length * n) % rows.length;
      if (rows[index]!.entry.title.toLowerCase().includes(needle)) {
        cursor = index;
        return true;
      }
    }
    return false;
  }

  function open(row: Row) {
    if (row.entry.chapter !== null) onopenentry(row.entry);
    else if (row.hasChildren) setCollapsed([row.key], !row.collapsed);
  }

  function click(index: number) {
    cursor = index;
    onfocus();
    select();
  }
</script>

<nav class:focused style:width="{width}px" aria-label="Contents">
  <div class="top">
    <button class="back" onclick={onlibrary}>
      <svg aria-hidden="true" width="16" height="16" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round"><path d="M9.5 4L5.5 8l4 4"></path></svg>
      Library
    </button>
    <kbd>b</kbd>
  </div>

  {#if book}
    <div class="book">
      <span class="volume" aria-hidden="true">
        <span class="board" style:background={coverFor(book.path).cloth}></span>
        <span class="pages"></span>
        <span class="cover" style:background={coverFor(book.path).cloth}>
          {#if book.cover && failedCover !== book.path}
            <img src={bookCover(book.path)} alt="" onerror={() => (failedCover = book.path)} />
          {/if}
          <span class="binding"></span>
        </span>
      </span>
      <div class="about">
        <div class="name">{book.title}</div>
        {#if book.author}<div class="author">{book.author}</div>{/if}
      </div>
    </div>
  {/if}

  <div class="list" bind:this={listEl}>
    {#each rows as row, index (row.key)}
      <div
        class="row"
        class:active={index === active}
        class:cursor={focused && index === cursor}
        style:padding-left="{10 + row.depth * 14}px"
      >
        {#if row.hasChildren}
          <button
            class="margin chevron"
            class:open={!row.collapsed}
            aria-label={row.collapsed ? 'Expand section' : 'Collapse section'}
            onclick={() => setCollapsed([row.key], !row.collapsed)}
          >
            <svg aria-hidden="true" width="12" height="12" viewBox="0 0 12 12" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round"><path d="M4.5 2.5L8 6l-3.5 3.5"></path></svg>
          </button>
        {:else if margin}
          <span class="margin">{row.number ?? ''}</span>
        {/if}
        <button class="title" onclick={() => click(index)}>{row.label}</button>
        {#if paged && row.entry.chapter !== null}
          <span class="page">{row.entry.chapter + 1}</span>
        {/if}
      </div>
    {/each}
    {#if rows.length === 0}
      <p class="empty">This {paged ? 'document' : 'book'} has no contents list.</p>
    {/if}
  </div>
</nav>

<style>
  nav {
    flex: none;
    display: flex;
    flex-direction: column;
    min-height: 0;
    padding: 16px 10px 0 16px;
  }

  .top {
    flex: none;
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding-right: 8px;
  }

  .back {
    display: flex;
    align-items: center;
    gap: 4px;
    height: 32px;
    padding: 0 10px 0 4px;
    border-radius: 7px;
    font-size: 12.5px;
    color: var(--muted);
  }

  .back:hover {
    background: var(--soft);
    color: var(--text);
  }

  .book {
    flex: none;
    display: flex;
    align-items: center;
    gap: 14px;
    padding: 22px 8px;
  }

  /* The open book, seen square on: its cover, with the block of pages and
     the back board showing past its fore-edge. */
  .volume {
    position: relative;
    flex: none;
    width: 50px;
    height: 74px;
    margin-right: 6px;
    filter: drop-shadow(0 4px 8px var(--shadow));
  }

  .board {
    position: absolute;
    inset: 0 -6px 0 12px;
    border-radius: 0 3px 3px 0;
    filter: brightness(0.6);
  }

  .pages {
    position: absolute;
    inset: 2px -4.5px 2px 12px;
    border-radius: 0 1px 1px 0;
    background: repeating-linear-gradient(90deg, #efe8d6 0 1px, #cfc7b3 1px 2px);
  }

  .cover {
    position: absolute;
    inset: 0;
    border-radius: 1.5px 3px 3px 1.5px;
    overflow: hidden;
  }

  .cover img {
    display: block;
    width: 100%;
    height: 100%;
    object-fit: cover;
  }

  /* The spine's rounded edge and the groove of its hinge, as in the library. */
  .binding {
    position: absolute;
    inset: 0;
    border-radius: inherit;
    background:
      linear-gradient(
        90deg,
        rgb(0 0 0 / 0.3) 0,
        rgb(0 0 0 / 0.06) 3%,
        rgb(255 255 255 / 0.18) 5.5%,
        rgb(255 255 255 / 0.04) 8%,
        rgb(0 0 0 / 0.26) 10.5%,
        rgb(0 0 0 / 0.06) 14%,
        rgb(0 0 0 / 0) 20%
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

  .about {
    min-width: 0;
  }

  .name {
    font-family: var(--font-reading);
    font-size-adjust: var(--letter-height, none);
    font-size: 17px;
    font-weight: 500;
    line-height: 1.2;
    color: var(--heading);
    overflow-wrap: break-word;
    display: -webkit-box;
    -webkit-box-orient: vertical;
    -webkit-line-clamp: 3;
    line-clamp: 3;
    overflow: hidden;
  }

  .author {
    margin-top: 5px;
    font-size: 12px;
    color: var(--muted);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .list {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    padding-bottom: 48px;
    scrollbar-width: none;
    -webkit-mask-image: linear-gradient(#000 calc(100% - 48px), transparent);
    mask-image: linear-gradient(#000 calc(100% - 48px), transparent);
  }

  .row {
    display: flex;
    align-items: baseline;
    gap: 12px;
    padding-right: 10px;
    border-radius: 7px;
    font-family: var(--font-reading);
    font-size-adjust: var(--letter-height, none);
    font-size: 14.5px;
    line-height: 20px;
    color: var(--quiet);
    outline: 1px solid transparent;
    outline-offset: -1px;
  }

  .row:hover {
    color: var(--heading);
  }

  .row.active {
    background: var(--soft);
    color: var(--heading);
  }

  .row.cursor {
    outline-color: var(--ring);
    color: var(--heading);
  }

  .margin {
    flex: none;
    width: 20px;
    text-align: right;
    font-size: 12.5px;
    font-variant-numeric: var(--reading-figures, oldstyle-nums);
    color: var(--muted);
  }

  .row.active .margin {
    color: var(--accent);
  }

  .chevron {
    align-self: center;
    height: 24px;
    display: flex;
    align-items: center;
    justify-content: flex-end;
  }

  .chevron svg {
    transition: transform 120ms ease;
  }

  .chevron.open svg {
    transform: rotate(90deg);
  }

  .title {
    flex: 1;
    min-width: 0;
    padding: 6px 0;
    text-align: left;
    overflow-wrap: break-word;
  }

  .page {
    flex: none;
    font-family: var(--font-mono);
    font-size: 11px;
    color: var(--muted);
  }

  .row.active .page {
    color: var(--accent);
  }

  .empty {
    margin: 12px 10px;
    color: var(--muted);
  }
</style>
