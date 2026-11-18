<script lang="ts">
  import { tick, untrack } from 'svelte';
  import { api, type SearchHit } from './api';

  let {
    initial,
    paged,
    focused,
    onjump,
    onkeep,
    onclose,
    onerror,
  }: {
    /** The last search, to pick up again; null starts afresh. */
    initial: { query: string; hits: SearchHit[] } | null;
    /** Whether results are pages of a PDF rather than chapters of a book. */
    paged: boolean;
    focused: boolean;
    onjump: (hit: SearchHit, query: string) => void;
    /** Hands back the search as it stands, so it can be reopened. */
    onkeep: (query: string, hits: SearchHit[]) => void;
    onclose: () => void;
    onerror: (message: string) => void;
  } = $props();

  const TYPING_PAUSE_MS = 220;

  // The panel is made afresh each time it opens, so where it starts
  // from is read once, here.
  const start = untrack(() => initial);
  let query = $state(start?.query ?? '');
  let hits = $state<SearchHit[]>(start?.hits ?? []);
  let searched = $state(start !== null);
  let cursor = $state(0);
  let fieldEl = $state<HTMLInputElement>();
  let listEl = $state<HTMLElement>();
  let timer: ReturnType<typeof setTimeout> | undefined;
  let latest = 0;

  /** Results under the title of the chapter or page they are in. */
  let rows = $derived(
    hits.map((hit, index) => ({
      hit,
      heading: index === 0 || hits[index - 1]!.section !== hit.section ? hit.title : undefined,
    })),
  );

  $effect(() => {
    if (!start || start.hits.length === 0) void tick().then(() => fieldEl?.focus());
  });

  $effect(() => {
    if (cursor >= hits.length) cursor = Math.max(0, hits.length - 1);
    void tick().then(() => listEl?.querySelector('.cursor')?.scrollIntoView({ block: 'nearest' }));
  });

  async function run() {
    clearTimeout(timer);
    const asked = query.trim();
    const id = ++latest;
    if (!asked) {
      hits = [];
      searched = false;
      onkeep('', []);
      return;
    }
    try {
      const found = await api.searchBook(asked);
      if (id !== latest) return;
      hits = found;
      searched = true;
      cursor = 0;
      onkeep(asked, found);
    } catch (e) {
      onerror(String(e));
    }
  }

  function typed() {
    clearTimeout(timer);
    timer = setTimeout(() => void run(), TYPING_PAUSE_MS);
  }

  function pieces(hit: SearchHit): { text: string; marked: boolean }[] {
    const marks = new Set(hit.marks);
    const out: { text: string; marked: boolean }[] = [];
    [...hit.snippet].forEach((ch, index) => {
      const marked = marks.has(index);
      const last = out.at(-1);
      if (last && last.marked === marked) last.text += ch;
      else out.push({ text: ch, marked });
    });
    return out;
  }

  function move(delta: number) {
    cursor = Math.max(0, Math.min(hits.length - 1, cursor + delta));
  }

  function go(index: number) {
    const hit = hits[index];
    if (!hit) return;
    cursor = index;
    onjump(hit, query.trim());
  }

  export function handleKey(token: string) {
    if (token === 'Esc' || token === 'q') onclose();
    else if (token === 'j' || token === 'Down') move(1);
    else if (token === 'k' || token === 'Up') move(-1);
    else if (token === 'C-d' || token === 'PageDown') move(8);
    else if (token === 'C-u' || token === 'PageUp') move(-8);
    else if (token === 'G' || token === 'End') move(hits.length);
    else if (token === 'g' || token === 'Home') move(-hits.length);
    else if (token === 'Enter') go(cursor);
    else if (token === '/' || token === 'i') fieldEl?.focus();
  }

  async function onFieldKey(event: KeyboardEvent) {
    if (event.key === 'Enter') {
      event.preventDefault();
      await run();
      if (hits.length > 0) fieldEl?.blur();
    } else if (event.key === 'Escape') {
      event.preventDefault();
      event.stopPropagation();
      if (hits.length > 0) fieldEl?.blur();
      else onclose();
    } else if (event.key === 'ArrowDown' && hits.length > 0) {
      event.preventDefault();
      fieldEl?.blur();
    }
  }
</script>

<aside aria-label="Search the book" class:focused>
  <div class="top">
    <span class="label">SEARCH THE {paged ? 'DOCUMENT' : 'BOOK'}</span>
    <button class="close" aria-label="Close search" onclick={onclose}><kbd>Esc</kbd></button>
  </div>

  <input
    bind:this={fieldEl}
    bind:value={query}
    oninput={typed}
    onkeydown={onFieldKey}
    type="search"
    placeholder="Words, or &quot;an exact phrase&quot;"
    aria-label="Search for"
    spellcheck="false"
    autocomplete="off"
  />

  <div class="list" bind:this={listEl}>
    {#each rows as row, index}
      {#if row.heading !== undefined}
        <div class="caption">{row.heading}</div>
      {/if}
      <button class="row" class:cursor={focused && index === cursor} onclick={() => go(index)}>
        {#each pieces(row.hit) as piece}{#if piece.marked}<mark>{piece.text}</mark>{:else}{piece.text}{/if}{/each}
      </button>
    {/each}
    {#if searched && hits.length === 0}
      <p class="empty">Nothing found for “{query.trim()}”.</p>
    {:else if !searched}
      <p class="empty">
        Type to search every {paged ? 'page' : 'chapter'}. Several words find passages with most
        of them; quotation marks find an exact phrase.
      </p>
    {/if}
  </div>

  <div class="hints">
    {#if hits.length > 0}<span>{hits.length}{hits.length === 50 ? '+' : ''} found</span>{/if}
    <span><kbd>j</kbd><kbd>k</kbd> move</span>
    <span><kbd>Enter</kbd> go there</span>
    <span><kbd>/</kbd> edit</span>
    <span><kbd>Tab</kbd> back to the text</span>
  </div>
</aside>

<style>
  aside {
    flex: 0 0 min(432px, 46%);
    min-width: 0;
    min-height: 0;
    display: flex;
    flex-direction: column;
    padding: 26px 28px 0 30px;
  }

  .top {
    flex: none;
    display: flex;
    align-items: center;
    justify-content: space-between;
  }

  .label {
    font-family: var(--font-mono);
    font-size: 10.5px;
    letter-spacing: 0.16em;
    color: var(--muted);
  }

  .close {
    height: 32px;
    display: flex;
    align-items: center;
    padding: 0 4px;
  }

  input {
    flex: none;
    height: 40px;
    margin-top: 10px;
    padding: 0 14px;
    border-radius: 9px;
    border: 1px solid var(--line);
    background: var(--page);
    color: var(--heading);
    font-family: var(--font-reading);
    font-size-adjust: var(--letter-height, none);
    font-size: 16px;
    outline: none;
  }

  input:focus {
    border-color: var(--ring);
  }

  input::placeholder {
    color: var(--muted);
  }

  input::-webkit-search-cancel-button {
    display: none;
  }

  .list {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    margin: 8px -10px 0;
    padding-bottom: 12px;
    scrollbar-width: none;
  }

  .caption {
    padding: 18px 10px 6px;
    font-family: var(--font-reading);
    font-size-adjust: var(--letter-height, none);
    font-size: 13px;
    line-height: 18px;
    font-variant-caps: all-small-caps;
    letter-spacing: 0.14em;
    color: var(--accent);
  }

  .row {
    display: block;
    width: 100%;
    padding: 8px 10px;
    border-radius: 7px;
    outline: 1px solid transparent;
    outline-offset: -1px;
    text-align: left;
    font-family: var(--font-reading);
    font-size-adjust: var(--letter-height, none);
    font-size: 14.5px;
    line-height: 1.5;
    color: var(--quiet);
    cursor: default;
    overflow-wrap: break-word;
  }

  .row:hover {
    background: var(--soft);
  }

  .row.cursor {
    background: var(--soft);
    outline-color: var(--ring);
  }

  mark {
    background: none;
    color: var(--accent);
    font-weight: 600;
  }

  .empty {
    margin: 0;
    padding: 14px 10px 6px;
    line-height: 1.7;
    color: var(--muted);
  }

  .hints {
    flex: none;
    min-height: 36px;
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 6px 16px;
    padding: 0 0 2px;
    font-size: 11.5px;
    color: var(--muted);
  }

  .hints span {
    display: flex;
    align-items: center;
    gap: 6px;
  }
</style>
