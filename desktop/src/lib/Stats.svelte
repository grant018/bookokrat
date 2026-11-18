<script lang="ts">
  import { tick, type Snippet } from 'svelte';
  import { api, type BookStats } from './api';

  let {
    title,
    current,
    status,
    onjump,
    onnotice,
    onclose,
  }: {
    title: string;
    /** The chapter on screen, or the page of a PDF. */
    current: number;
    /** The window's status line, drawn under the list. */
    status: Snippet;
    onjump: (chapter: number) => void;
    onnotice: (text: string, error?: boolean) => void;
    onclose: () => void;
  } = $props();

  const WORDS_PER_MINUTE = 250;

  let stats = $state<BookStats | null>(null);
  let cursor = $state(0);
  let listEl = $state<HTMLElement>();

  let rows = $derived(stats?.rows ?? []);
  let total = $derived(rows.reduce((sum, row) => sum + row.count, 0));
  let longest = $derived(Math.max(1, ...rows.map((row) => row.count)));
  /** The row for the part being read: the last one starting at or before it. */
  let here = $derived(rows.findLastIndex((row) => row.chapter <= current));

  $effect(() => {
    api
      .bookStats()
      .then((found) => {
        stats = found;
        cursor = Math.max(0, found.rows.findLastIndex((row) => row.chapter <= current));
      })
      .catch((e) => {
        onnotice(String(e), true);
        onclose();
      });
  });

  $effect(() => {
    if (cursor >= rows.length) cursor = Math.max(0, rows.length - 1);
    void tick().then(() => listEl?.querySelector('.cursor')?.scrollIntoView({ block: 'nearest' }));
  });

  const number = new Intl.NumberFormat();

  function duration(words: number): string {
    const minutes = Math.round(words / WORDS_PER_MINUTE);
    if (minutes < 1) return 'under a minute';
    if (minutes < 60) return `${minutes} min`;
    return `${Math.floor(minutes / 60)} h ${minutes % 60} min`;
  }

  function move(delta: number) {
    cursor = Math.max(0, Math.min(rows.length - 1, cursor + delta));
  }

  /** Handles a keymap action; returns false if it means nothing here. */
  export function handle(action: string): boolean {
    switch (action) {
      case 'move_down':
        move(1);
        return true;
      case 'move_up':
        move(-1);
        return true;
      case 'scroll_half_down':
      case 'scroll_page_down':
        move(10);
        return true;
      case 'scroll_half_up':
      case 'scroll_page_up':
        move(-10);
        return true;
      case 'go_top':
        move(-rows.length);
        return true;
      case 'go_bottom':
        move(rows.length);
        return true;
      case 'select': {
        const row = rows[cursor];
        if (row) onjump(row.chapter);
        return true;
      }
      case 'cancel':
        onclose();
        return true;
      default:
        return false;
    }
  }
</script>

<section class="stats" aria-label="Book statistics">
  <header>
    <button class="back" aria-label="Close statistics" onclick={onclose}>
      <svg aria-hidden="true" width="16" height="16" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round"><path d="M9.5 4L5.5 8l4 4"></path></svg>
    </button>
    <div>
      <h1>{title}</h1>
      {#if stats}
        <p>
          {#if stats.unit === 'words'}
            {number.format(total)} words in {rows.length} {rows.length === 1 ? 'chapter' : 'chapters'},
            about {duration(total)} of reading
          {:else}
            {number.format(total)} pages in {rows.length} {rows.length === 1 ? 'part' : 'parts'}
          {/if}
        </p>
      {/if}
    </div>
  </header>

  <div class="list" bind:this={listEl}>
    {#each rows as row, index}
      <button
        class="row"
        class:cursor={index === cursor}
        class:here={index === here}
        onclick={() => {
          cursor = index;
          onjump(row.chapter);
        }}
      >
        <span class="title">{row.title}</span>
        <span class="bar" aria-hidden="true"><span style:width="{(row.count / longest) * 100}%"></span></span>
        <span class="count">
          {number.format(row.count)}
          {stats?.unit === 'words' ? 'words' : row.count === 1 ? 'page' : 'pages'}
        </span>
        <span class="time">{stats?.unit === 'words' ? duration(row.count) : `from p. ${row.chapter + 1}`}</span>
      </button>
    {/each}
  </div>

  {@render status()}
</section>

<style>
  .stats {
    flex: 1;
    min-width: 0;
    min-height: 0;
    display: flex;
    flex-direction: column;
  }

  header {
    flex: none;
    display: flex;
    align-items: flex-start;
    gap: 10px;
    padding: 40px 64px 0 24px;
  }

  .back {
    flex: none;
    width: 32px;
    height: 32px;
    margin-top: 4px;
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
    margin: 0;
    font-family: var(--font-reading);
    font-size-adjust: var(--letter-height, none);
    font-size: 32px;
    font-weight: 500;
    line-height: 1.2;
    letter-spacing: -0.01em;
    color: var(--heading);
  }

  p {
    margin: 6px 0 0;
    color: var(--muted);
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
    grid-template-columns: minmax(0, 1fr) minmax(80px, 240px) 110px 110px;
    align-items: center;
    gap: 24px;
    width: 100%;
    max-width: 980px;
    min-height: 36px;
    padding: 4px 10px;
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

  .title {
    font-family: var(--font-reading);
    font-size-adjust: var(--letter-height, none);
    font-size: 15.5px;
    line-height: 1.35;
    color: var(--quiet);
    overflow-wrap: break-word;
  }

  .row.here .title {
    color: var(--accent);
  }

  .bar {
    height: 2px;
    border-radius: 1px;
    background: var(--hairline);
    overflow: hidden;
  }

  .bar span {
    display: block;
    height: 100%;
    background: var(--muted);
  }

  .row.here .bar span {
    background: var(--accent);
  }

  .count,
  .time {
    font-family: var(--font-mono);
    font-size: 11px;
    text-align: right;
    color: var(--muted);
    white-space: nowrap;
  }
</style>
