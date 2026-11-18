<script lang="ts">
  import { tick } from 'svelte';
  import type { Annotation, Mark } from './api';
  import { SmoothScroller } from './scroller';

  type Row = { key: string; mark?: Mark; note?: Annotation; heading?: string };

  let {
    annotations,
    marks,
    titleOf,
    quoteOf,
    editing,
    focused,
    onjump,
    onedit,
    onsave,
    ondiscard,
    ondelete,
    oncopy,
    onexport,
    onclose,
  }: {
    annotations: Annotation[];
    marks: Mark[];
    /** The title of one of the book's chapters. */
    titleOf: (chapter: number) => string;
    /** The text an annotation covers, where it can be told. */
    quoteOf: (note: Annotation) => string;
    /** Set while a note is being written: the passage and the text so far. */
    editing: { quote: string; text: string } | null;
    focused: boolean;
    onjump: (target: { mark?: Mark; note?: Annotation }) => void;
    onedit: (note: Annotation) => void;
    onsave: (text: string) => void;
    ondiscard: () => void;
    ondelete: (target: { mark?: Mark; note?: Annotation }) => void;
    oncopy: (text: string) => void;
    /** Writes every highlight and note out as Markdown. */
    onexport: () => void;
    onclose: () => void;
  } = $props();

  let cursor = $state(0);
  let draft = $state('');
  let listEl = $state<HTMLElement>();
  let fieldEl = $state<HTMLTextAreaElement>();
  let filterEl = $state<HTMLInputElement>();
  /** Words that listed notes and bookmarks must have; null when not filtering. */
  let filter = $state<string | null>(null);
  let scroller: SmoothScroller | undefined;

  /** Highlights and notes in reading order, each chapter under its title. */
  let rows = $derived.by((): Row[] => {
    const ordered = [...annotations].sort((a, b) => {
      const [x, y] = [a.slices[0], b.slices[0]];
      return a.chapter - b.chapter || (x?.node ?? 0) - (y?.node ?? 0) || (x?.start ?? 0) - (y?.start ?? 0);
    });
    const needle = filter?.trim().toLowerCase() ?? '';
    const has = (...texts: (string | null | undefined)[]) =>
      !needle || texts.some((text) => text?.toLowerCase().includes(needle));
    const out: Row[] = marks
      .filter((mark) => has(mark.snippet, mark.chapter_title, mark.letter))
      .map((mark) => ({ key: `mark ${mark.letter}`, mark }));
    let chapter = -1;
    for (const note of ordered) {
      if (!has(note.text, quoteOf(note))) continue;
      const heading = note.chapter === chapter ? undefined : titleOf(note.chapter);
      chapter = note.chapter;
      out.push({ key: note.id, note, heading });
    }
    return out;
  });

  $effect(() => {
    if (!listEl) return;
    scroller = new SmoothScroller(listEl);
    return () => scroller?.stop();
  });

  $effect(() => {
    if (cursor >= rows.length) cursor = Math.max(0, rows.length - 1);
    void tick().then(() => listEl?.querySelector('.cursor')?.scrollIntoView({ block: 'nearest' }));
  });

  // The field takes the keyboard for as long as a note is being written.
  $effect(() => {
    if (!editing) return;
    draft = editing.text;
    void tick().then(() => fieldEl?.focus());
  });

  function fileName(path: string): string {
    return path.slice(path.lastIndexOf('/') + 1);
  }

  function move(delta: number) {
    cursor = Math.max(0, Math.min(rows.length - 1, cursor + delta));
  }

  function copyRow(row: Row) {
    if (row.mark) oncopy(row.mark.snippet ?? '');
    else if (row.note) oncopy([quoteOf(row.note), row.note.text].filter(Boolean).join('\n\n'));
  }

  async function startFilter() {
    filter ??= '';
    await tick();
    filterEl?.focus();
  }

  function onFilterKey(event: KeyboardEvent) {
    if (event.key === 'Enter' || event.key === 'ArrowDown') {
      event.preventDefault();
      filterEl?.blur();
    } else if (event.key === 'Escape') {
      event.preventDefault();
      event.stopPropagation();
      filter = null;
    }
  }

  /** Runs one of the notes list's bindings; false when it has no use for it. */
  export function handle(action: string): boolean {
    const row = rows[cursor];
    switch (action) {
      case 'move_down':
        move(1);
        break;
      case 'move_up':
        move(-1);
        break;
      case 'scroll_half_down':
        move(8);
        break;
      case 'scroll_half_up':
        move(-8);
        break;
      case 'scroll_page_down':
        move(16);
        break;
      case 'scroll_page_up':
        move(-16);
        break;
      case 'go_top':
        move(-rows.length);
        break;
      case 'go_bottom':
        move(rows.length);
        break;
      case 'select':
        if (row) onjump(row);
        break;
      case 'delete_comment':
        if (row) ondelete(row);
        break;
      case 'export_comments':
        onexport();
        break;
      case 'start_search':
        void startFilter();
        break;
      case 'cancel':
        if (filter !== null) filter = null;
        else onclose();
        break;
      default:
        return false;
    }
    return true;
  }

  /** Keys the keymap has no command for here, as the terminal app has them. */
  export function handleKey(token: string) {
    const row = rows[cursor];
    if (['c', 'C', 'y', 'Y'].includes(token) && row) copyRow(row);
    else if (token === 'q') onclose();
    else if (token === 'Home') move(-rows.length);
    else if (token === 'End') move(rows.length);
    else if ((token === 'a' || token === 'e') && row?.note && row.note.color === null) onedit(row.note);
  }

  function onFieldKey(event: KeyboardEvent) {
    // Esc keeps what was written, as it does in the terminal app.
    if (event.key === 'Escape' || (event.key === 'Enter' && event.ctrlKey)) {
      event.preventDefault();
      event.stopPropagation();
      onsave(draft);
    }
  }
</script>

<aside aria-label="Notes and bookmarks" class:focused>
  <div class="top">
    <span class="label">{editing ? 'NOTE' : 'NOTES AND BOOKMARKS'}</span>
    {#if !editing && annotations.length > 0}
      <button class="export" onclick={onexport}>Export</button>
    {/if}
    <button class="close" aria-label="Close notes" onclick={editing ? () => onsave(draft) : onclose}>
      <kbd>Esc</kbd>
    </button>
  </div>

  {#if editing}
    <blockquote>{editing.quote}</blockquote>
    <textarea
      bind:this={fieldEl}
      bind:value={draft}
      onkeydown={onFieldKey}
      placeholder="Write a note on this passage"
      aria-label="Note"
      spellcheck="true"
    ></textarea>
    <div class="actions">
      <button class="plain" onclick={ondiscard}>Discard</button>
      <button class="primary" onclick={() => onsave(draft)}>Save</button>
    </div>
    <div class="hints">
      <span><kbd>Esc</kbd> save and return to the text</span>
    </div>
  {:else}
    {#if filter !== null}
      <input
        class="filter"
        bind:this={filterEl}
        bind:value={filter}
        onkeydown={onFilterKey}
        type="search"
        placeholder="Only those with these words"
        aria-label="Filter notes and bookmarks"
        spellcheck="false"
      />
    {/if}
    <div class="list" bind:this={listEl}>
      <div class="caption">Bookmarks</div>
      {#if marks.length === 0}
        <p class="empty">None yet. Press <kbd>m</kbd> and a letter to set one where you are.</p>
      {/if}
      {#each rows as row, index (row.key)}
        {#if row.heading !== undefined}
          <div class="caption chapter">{row.heading}</div>
        {/if}
        <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
        <div
          class="row"
          class:cursor={focused && index === cursor}
          onclick={() => {
            cursor = index;
            onjump(row);
          }}
        >
          {#if row.mark}
            <kbd>{row.mark.letter}</kbd>
            <div class="body">
              <div class="quote">{row.mark.snippet ?? 'A place in the book'}</div>
              <div class="where">
                {row.mark.here
                  ? (row.mark.chapter_title ?? titleOf(row.mark.chapter ?? 0))
                  : fileName(row.mark.path)}
              </div>
            </div>
          {:else if row.note}
            {@const quote = quoteOf(row.note)}
            <span
              class="dot"
              class:ring={row.note.color === null}
              style:background={row.note.color ? `var(--mark-${row.note.color})` : undefined}
            ></span>
            <div class="body">
              {#if quote}<div class="quote">{quote}</div>{/if}
              {#if row.note.text}<div class="text">{row.note.text}</div>{/if}
              {#if !row.note.drawable}
                <div class="where">On a list, quotation or code: shown in the terminal app only.</div>
              {/if}
            </div>
          {/if}
        </div>
      {/each}
      {#if annotations.length === 0}
        <div class="caption chapter">Highlights and notes</div>
        <p class="empty">
          None yet. Select some text, then press <kbd>H</kbd> to highlight it or <kbd>a</kbd> to
          write a note.
        </p>
      {/if}
    </div>
    <div class="hints">
      <span><kbd>j</kbd><kbd>k</kbd> move</span>
      <span><kbd>Enter</kbd> go there</span>
      <span><kbd>a</kbd> edit</span>
      <span><kbd>c</kbd> copy</span>
      <span><kbd>d</kbd><kbd>d</kbd> remove</span>
      <span><kbd>/</kbd> filter</span>
      <span><kbd>Space</kbd><kbd>e</kbd> export</span>
      <span><kbd>Tab</kbd> back to the text</span>
    </div>
  {/if}
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

  .export {
    margin-left: auto;
    margin-right: 8px;
    height: 26px;
    padding: 0 10px;
    border-radius: 6px;
    font-size: 12px;
    color: var(--muted);
  }

  .export:hover {
    background: var(--soft);
    color: var(--text);
  }

  .filter {
    flex: none;
    height: 34px;
    margin-top: 8px;
    padding: 0 12px;
    border-radius: 8px;
    border: 1px solid var(--line);
    background: var(--page);
    color: var(--heading);
    font: inherit;
    outline: none;
  }

  .filter:focus {
    border-color: var(--ring);
  }

  .filter::-webkit-search-cancel-button {
    display: none;
  }

  blockquote {
    flex: none;
    margin: 10px 0 18px;
    max-height: 9.6em;
    overflow: hidden;
    font-family: var(--font-reading);
    font-size-adjust: var(--letter-height, none);
    font-style: italic;
    font-size: 17px;
    line-height: 1.6;
    color: var(--heading);
    display: -webkit-box;
    -webkit-box-orient: vertical;
    -webkit-line-clamp: 6;
    line-clamp: 6;
  }

  textarea {
    flex: 1;
    min-height: 120px;
    max-height: 320px;
    padding: 12px 14px;
    border-radius: 9px;
    border: 1px solid var(--line);
    background: var(--page);
    color: var(--text);
    font: inherit;
    font-size: 14px;
    line-height: 1.55;
    resize: none;
    outline: none;
  }

  textarea:focus {
    border-color: var(--ring);
  }

  .actions {
    flex: none;
    display: flex;
    justify-content: flex-end;
    gap: 8px;
    padding: 12px 0 4px;
  }

  .actions button {
    height: 30px;
    padding: 0 14px;
    border-radius: 7px;
  }

  .plain {
    color: var(--muted);
  }

  .plain:hover {
    background: var(--soft);
    color: var(--text);
  }

  .primary {
    background: var(--raised);
    color: var(--heading);
  }

  .primary:hover {
    background: color-mix(in srgb, var(--text) 18%, var(--chrome));
  }

  .list {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    margin: 6px -10px 0;
    padding-bottom: 12px;
    scrollbar-width: none;
  }

  .caption {
    padding: 14px 10px 6px;
    font-family: var(--font-reading);
    font-size-adjust: var(--letter-height, none);
    font-size: 13px;
    line-height: 18px;
    font-variant-caps: all-small-caps;
    letter-spacing: 0.14em;
    color: var(--muted);
  }

  .caption.chapter {
    padding-top: 22px;
    color: var(--accent);
  }

  .row {
    display: flex;
    align-items: baseline;
    gap: 12px;
    padding: 8px 10px;
    border-radius: 7px;
    outline: 1px solid transparent;
    outline-offset: -1px;
    cursor: default;
  }

  .row:hover {
    background: var(--soft);
  }

  .row.cursor {
    background: var(--soft);
    outline-color: var(--ring);
  }

  .dot {
    flex: none;
    width: 8px;
    height: 8px;
    border-radius: 50%;
    transform: translateY(-1px);
  }

  .dot.ring {
    box-shadow: inset 0 0 0 2px var(--note);
  }

  .body {
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 4px;
  }

  .quote {
    font-family: var(--font-reading);
    font-size-adjust: var(--letter-height, none);
    font-size: 14.5px;
    line-height: 1.5;
    color: var(--quiet);
    display: -webkit-box;
    -webkit-box-orient: vertical;
    -webkit-line-clamp: 3;
    line-clamp: 3;
    overflow: hidden;
  }

  .text {
    line-height: 1.5;
    color: var(--heading);
    white-space: pre-wrap;
    overflow-wrap: break-word;
  }

  .where {
    font-size: 11.5px;
    color: var(--muted);
  }

  .empty {
    margin: 0;
    padding: 2px 10px 6px;
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
