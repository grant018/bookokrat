<script lang="ts">
  import { tick, type Snippet } from 'svelte';
  import { api, type HistoryEntry } from './api';

  let {
    currentBook,
    status,
    onopen,
    onnotice,
    onclose,
  }: {
    currentBook: string | null;
    /** The window's status line, drawn under the list. */
    status: Snippet;
    onopen: (path: string) => void;
    onnotice: (text: string, error?: boolean) => void;
    onclose: () => void;
  } = $props();

  let entries = $state<HistoryEntry[]>([]);
  let loaded = $state(false);
  /** Which books are listed: those of the library on screen, or every library's. */
  let everywhere = $state(false);
  let cursor = $state(0);
  let listEl = $state<HTMLElement>();

  let shown = $derived(everywhere ? entries : entries.filter((entry) => entry.here));

  $effect(() => {
    api
      .readingHistory()
      .then((found) => {
        entries = found;
        // A library with no history of its own shows everything read anywhere.
        if (!found.some((entry) => entry.here)) everywhere = true;
      })
      .catch((e) => onnotice(String(e), true))
      .finally(() => (loaded = true));
  });

  $effect(() => {
    if (cursor >= shown.length) cursor = Math.max(0, shown.length - 1);
    void tick().then(() => listEl?.querySelector('.cursor')?.scrollIntoView({ block: 'nearest' }));
  });

  const day = new Intl.DateTimeFormat(undefined, { day: 'numeric', month: 'short', year: 'numeric' });
  const time = new Intl.DateTimeFormat(undefined, { hour: 'numeric', minute: '2-digit' });

  function when(stamp: string): string {
    const date = new Date(stamp);
    if (Number.isNaN(date.getTime())) return '';
    const days = Math.floor((Date.now() - date.getTime()) / 86_400_000);
    if (days < 1 && date.getDate() === new Date().getDate()) return `Today, ${time.format(date)}`;
    if (days < 2) return `Yesterday, ${time.format(date)}`;
    return day.format(date);
  }

  function folder(path: string): string {
    const parts = path.split('/');
    return parts.slice(Math.max(0, parts.length - 3), -1).join('/');
  }

  function move(delta: number) {
    cursor = Math.max(0, Math.min(shown.length - 1, cursor + delta));
  }

  function open(entry: HistoryEntry | undefined) {
    if (!entry) return;
    if (entry.exists) onopen(entry.path);
    else onnotice(`${entry.title} is no longer at ${entry.path}`, true);
  }

  async function forget(entry: HistoryEntry) {
    try {
      entries = await api.forgetHistory(entry.source, entry.key);
      onnotice(`Forgot ${entry.title}`);
    } catch (e) {
      onnotice(String(e), true);
    }
  }

  /** Handles a keymap action; returns false if it means nothing here. */
  export function handle(action: string): boolean {
    const entry = shown[cursor];
    switch (action) {
      case 'move_down':
        move(1);
        return true;
      case 'move_up':
        move(-1);
        return true;
      case 'scroll_half_down':
      case 'scroll_page_down':
        move(8);
        return true;
      case 'scroll_half_up':
      case 'scroll_page_up':
        move(-8);
        return true;
      case 'go_top':
        move(-shown.length);
        return true;
      case 'go_bottom':
        move(shown.length);
        return true;
      case 'next_tab':
      case 'prev_tab':
      case 'move_left':
      case 'move_right':
        everywhere = !everywhere;
        cursor = 0;
        return true;
      case 'select':
        open(entry);
        return true;
      case 'delete_entry':
        if (entry) void forget(entry);
        return true;
      case 'cancel':
        onclose();
        return true;
      default:
        return false;
    }
  }
</script>

<section class="history" aria-label="Reading history">
  <header>
    <button class="back" aria-label="Close reading history" onclick={onclose}>
      <svg aria-hidden="true" width="16" height="16" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round"><path d="M9.5 4L5.5 8l4 4"></path></svg>
    </button>
    <h1>Reading history</h1>
    <div class="tabs" role="tablist">
      <button role="tab" aria-selected={!everywhere} onclick={() => (everywhere = false)}>This library</button>
      <button role="tab" aria-selected={everywhere} onclick={() => (everywhere = true)}>All libraries</button>
    </div>
  </header>

  <div class="list" bind:this={listEl}>
    {#each shown as entry, index (entry.path)}
      <button
        class="row"
        class:cursor={index === cursor}
        class:gone={!entry.exists}
        onclick={() => {
          cursor = index;
          open(entry);
        }}
      >
        <span class="when">{when(entry.last_read)}</span>
        <span class="body">
          <span class="title">{entry.title}{#if entry.path === currentBook}<span class="open">open now</span>{/if}</span>
          <span class="by">
            {entry.author ?? ''}{entry.author && (everywhere || !entry.exists) ? ' · ' : ''}{!entry.exists
              ? 'file not found'
              : everywhere
                ? folder(entry.path)
                : ''}
          </span>
        </span>
        <span class="progress" aria-label="Progress">
          {#if entry.progress !== null}
            <span class="track"><span class="fill" style:width="{Math.round(entry.progress * 100)}%"></span></span>
            <span class="percent">{Math.round(entry.progress * 100)}%</span>
          {/if}
        </span>
      </button>
    {/each}
    {#if loaded && shown.length === 0}
      <p class="empty">
        {everywhere ? 'Nothing has been read yet.' : 'Nothing in this library has been read yet.'}
      </p>
    {/if}
  </div>

  {@render status()}
</section>

<style>
  .history {
    flex: 1;
    min-width: 0;
    min-height: 0;
    display: flex;
    flex-direction: column;
  }

  header {
    flex: none;
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 40px 64px 0 24px;
  }

  .back {
    flex: none;
    width: 32px;
    height: 32px;
    display: flex;
    align-items: center;
    justify-content: center;
    border-radius: 7px;
    color: var(--muted);
  }

  .back:hover {
    background: var(--soft);
    color: var(--text);
  }

  h1 {
    flex: 1;
    margin: 0;
    font-family: var(--font-reading);
    font-size-adjust: var(--letter-height, none);
    font-size: 32px;
    font-weight: 500;
    line-height: 1.2;
    letter-spacing: -0.01em;
    color: var(--heading);
  }

  .tabs {
    display: flex;
    gap: 4px;
  }

  .tabs button {
    height: 32px;
    padding: 0 12px;
    border-radius: 7px;
    color: var(--muted);
  }

  .tabs button[aria-selected='true'] {
    background: var(--raised);
    color: var(--heading);
  }

  .list {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    padding: 24px 54px 48px 56px;
    scrollbar-width: none;
  }

  .row {
    display: grid;
    grid-template-columns: 150px minmax(0, 1fr) 150px;
    align-items: baseline;
    gap: 24px;
    width: 100%;
    max-width: 980px;
    padding: 10px;
    border-radius: 8px;
    outline: 1px solid transparent;
    outline-offset: -1px;
    text-align: left;
    cursor: default;
  }

  .row:hover {
    background: var(--soft);
  }

  .row.cursor {
    background: var(--soft);
    outline-color: var(--ring);
  }

  .when {
    font-size: 12px;
    color: var(--muted);
  }

  .body {
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 3px;
  }

  .title {
    font-family: var(--font-reading);
    font-size-adjust: var(--letter-height, none);
    font-size: 17px;
    line-height: 1.3;
    color: var(--heading);
    overflow-wrap: break-word;
  }

  .gone .title {
    color: var(--muted);
  }

  .open {
    margin-left: 10px;
    font-family: var(--font-ui);
    font-size: 11px;
    color: var(--accent);
  }

  .by {
    font-size: 12px;
    color: var(--muted);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .progress {
    display: flex;
    align-items: center;
    gap: 10px;
    font-family: var(--font-mono);
    font-size: 11px;
    color: var(--muted);
  }

  .track {
    flex: 1;
    height: 2px;
    border-radius: 1px;
    background: var(--line);
    overflow: hidden;
  }

  .fill {
    display: block;
    height: 100%;
    background: var(--accent);
  }

  .percent {
    width: 34px;
    text-align: right;
  }

  .empty {
    margin: 0;
    padding: 10px;
    color: var(--muted);
  }
</style>
