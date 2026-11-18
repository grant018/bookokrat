<script lang="ts" module>
  import type { Annotation, Chapter, Node } from './api';

  /** What the pointer rests on: where a link leads, or a highlight or note. */
  export type PreviewContent =
    | { kind: 'link'; title: string | null; nodes: Node[] | null; failed?: boolean }
    | { kind: 'web'; url: string }
    | { kind: 'note'; annotation: Annotation };

  /** Enough of what a link leads to to know it by: the block holding the
      anchor (or the start of the chapter or of one of its files), such as a
      footnote; after a heading, what follows it, up to a short paragraph's
      worth. Null when the anchor is not in the chapter. */
  export function excerpt(chapter: Chapter, anchor?: string, part?: number): Node[] | null {
    const MIN_CHARS = 280;
    const MAX_BLOCKS = 4;
    let start = part === undefined ? 0 : (chapter.parts[part] ?? 0);
    if (anchor) {
      start = chapter.blocks.findIndex((node) => holds(node, anchor));
      if (start < 0) return null;
    }
    const out: Node[] = [];
    let chars = 0;
    for (let i = start; i < chapter.blocks.length; i++) {
      if (out.length >= MAX_BLOCKS || chars >= MIN_CHARS) break;
      const node = chapter.blocks[i]!;
      const length = textLength(node);
      // An anchor often sits alone in an empty block ahead of its text.
      if (out.length === 0 && length === 0 && !holds(node, null)) continue;
      out.push(node);
      chars += length;
      if (out[0]!.t !== 'h' && chars > 0) break;
    }
    return out;
  }

  /** Whether anything in `value` has the id `id`; with null, whether it
      holds a picture. */
  export function holds(value: unknown, id: string | null): boolean {
    if (Array.isArray(value)) return value.some((item) => holds(item, id));
    if (!value || typeof value !== 'object') return false;
    const fields = value as Record<string, unknown>;
    if (id === null ? fields.t === 'img' : fields.id === id) return true;
    return Object.values(fields).some((item) => holds(item, id));
  }

  function textLength(value: unknown): number {
    if (Array.isArray(value)) return value.reduce((n: number, item) => n + textLength(item), 0);
    if (!value || typeof value !== 'object') return 0;
    const fields = value as Record<string, unknown>;
    if (fields.t === 'text' && typeof fields.s === 'string') return fields.s.trim().length;
    if (fields.t === 'code' && typeof fields.code === 'string') return fields.code.length;
    return Object.values(fields).reduce((n: number, item) => n + textLength(item), 0);
  }
</script>

<script lang="ts">
  import Blocks from './Blocks.svelte';

  let {
    content,
    around,
    onfollow,
    onenter,
    onleave,
  }: {
    content: PreviewContent;
    /** The box of what is hovered, in the window; the card keeps clear of it. */
    around: DOMRect;
    /** A link in the card was clicked, or (null) the card itself, which
        goes where the hovered link leads. */
    onfollow: (link: HTMLAnchorElement | null) => void;
    onenter: () => void;
    onleave: () => void;
  } = $props();

  const GAP = 8;
  const EDGE = 12;

  let card: HTMLElement;
  let place = $state<{ left: number; top: number } | null>(null);

  // Below what it is about, or above it when there is no room below.
  $effect(() => {
    void content;
    const { width, height } = card.getBoundingClientRect();
    const below = around.bottom + GAP;
    const top =
      below + height <= innerHeight - EDGE || around.top - GAP - height < EDGE
        ? below
        : around.top - GAP - height;
    const left = Math.max(EDGE, Math.min(around.left, innerWidth - width - EDGE));
    place = { left, top };
  });

  const DATE = new Intl.DateTimeFormat(undefined, { day: 'numeric', month: 'short', year: 'numeric' });

  function dated(stamp: string): string {
    const when = new Date(stamp);
    return Number.isNaN(when.getTime()) ? '' : DATE.format(when);
  }

  function onclick(event: MouseEvent) {
    const link = (event.target as HTMLElement).closest('a');
    if (link) event.preventDefault();
    // Selecting text in the card to copy it is not a click on it.
    const selection = getSelection();
    if (selection && !selection.isCollapsed && card.contains(selection.anchorNode)) return;
    if (link || content.kind === 'link') onfollow(link);
  }
</script>

<!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
<div
  class="card"
  class:goes={content.kind === 'link'}
  bind:this={card}
  style:left="{place?.left ?? 0}px"
  style:top="{place?.top ?? 0}px"
  style:visibility={place ? 'visible' : 'hidden'}
  onmouseenter={onenter}
  onmouseleave={onleave}
  {onclick}
>
  {#if content.kind === 'link'}
    {#if content.title}<div class="head">{content.title}</div>{/if}
    {#if content.nodes}
      <div class="body prose"><Blocks nodes={content.nodes} /></div>
    {:else}
      <div class="quiet">{content.failed ? 'This part of the book could not be read' : 'Loading…'}</div>
    {/if}
  {:else if content.kind === 'web'}
    <div class="head">Opens in your browser</div>
    <a class="url" href={content.url} data-kind="external">{content.url}</a>
  {:else}
    {@const { color, text, updated } = content.annotation}
    <div class="head">
      <span class="dot" style:background={color ? `var(--mark-${color})` : 'var(--note)'}></span>
      {text ? 'Note' : 'Highlight'}{#if dated(updated)}<span class="when">· {dated(updated)}</span>{/if}
    </div>
    {#if text}<div class="note">{text}</div>{/if}
  {/if}
</div>

<style>
  .card {
    position: fixed;
    z-index: 15;
    width: max-content;
    max-width: min(440px, calc(100vw - 24px));
    max-height: min(320px, 45vh);
    overflow-y: auto;
    overscroll-behavior: contain;
    padding: 12px 16px;
    border-radius: 10px;
    border: 1px solid var(--line);
    background: var(--raised);
    color: var(--text);
    box-shadow: 0 12px 32px var(--shadow);
  }

  .card.goes {
    cursor: pointer;
  }

  .head {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-bottom: 6px;
    font-size: 0.72rem;
    letter-spacing: 0.08em;
    text-transform: uppercase;
    color: var(--muted);
  }

  .card > .head:last-child {
    margin-bottom: 0;
  }

  .when {
    letter-spacing: 0.04em;
  }

  .dot {
    flex: none;
    width: 9px;
    height: 9px;
    border-radius: 50%;
  }

  .body.prose {
    max-width: none;
    margin: 0;
    padding: 0;
    font-size: calc(var(--reading-size) * 0.86);
  }

  .body.prose :global(p),
  .body.prose :global(h1),
  .body.prose :global(h2),
  .body.prose :global(h3),
  .body.prose :global(h4) {
    margin: 0 0 0.6em;
    text-indent: 0;
  }

  .body.prose :global(:last-child) {
    margin-bottom: 0;
  }

  .body.prose :global(h1),
  .body.prose :global(h2),
  .body.prose :global(h3) {
    font-size: 1.15em;
  }

  .body.prose :global(img) {
    max-height: 200px;
  }

  .note {
    font-family: var(--font-reading);
    line-height: 1.5;
    white-space: pre-wrap;
    overflow-wrap: break-word;
  }

  .quiet {
    color: var(--muted);
  }

  .url {
    color: var(--link);
    overflow-wrap: anywhere;
  }
</style>
