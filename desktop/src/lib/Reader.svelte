<script lang="ts">
  import { flushSync, tick, untrack } from 'svelte';
  import {
    annotationOver,
    markPlace,
    paint,
    sign,
    sliceAt,
    slicesIn,
    textOf,
    topBlock,
  } from './annotate';
  import type { Annotation, Chapter, Inline, Node as Block, Piece, Slice } from './api';
  import type { JumpLocation } from './jumps';
  import Blocks from './Blocks.svelte';
  import { Finder } from './find';
  import Preview, { excerpt, holds, type PreviewContent } from './Preview.svelte';
  import { SmoothScroller } from './scroller';
  import type { LinkTarget } from './types';
  import { Vim, type VimMode } from './vim';

  let {
    chapter,
    language,
    justify,
    dropCaps,
    annotations,
    marks,
    raw,
    onprogress,
    onlink,
    chapterFor,
    onimage,
    onvim,
    oncopy,
    onnotice,
    onfound,
  }: {
    chapter: Chapter;
    language: string | null;
    justify: boolean;
    dropCaps: boolean;
    /** The chapter's highlights and notes, and its bookmarks. */
    annotations: Annotation[];
    marks: { letter: string; node: number }[];
    /** The chapter's HTML as the book has it, to show instead of the text. */
    raw: string | null;
    onprogress: (fraction: number, scrollTop: number) => void;
    onlink: (target: LinkTarget) => void;
    /** Any chapter of the book, to preview where a link leads. */
    chapterFor: (index: number) => Promise<Chapter>;
    onimage: (src: string, alt: string) => void;
    onvim: (mode: VimMode, pending: string) => void;
    oncopy: (text: string) => void;
    onnotice: (text: string) => void;
    /** What `find` was looking for has been looked for in all of the
        chapter: how many places there are, and which the reader is on. */
    onfound: (count: number, index: number) => void;
  } = $props();

  let scrollEl: HTMLElement;
  let proseEl: HTMLElement;
  let aboveEl: HTMLElement;
  let belowEl: HTMLElement;
  let scroller: SmoothScroller;
  let vim: Vim | undefined;
  let finder: Finder | undefined;
  let progressFrame = 0;

  $effect(() => {
    scroller = new SmoothScroller(scrollEl);
    vim = new Vim(scrollEl, proseEl, scroller, {
      onchange: () => onvim(vim?.mode ?? 'reading', vim?.pending ?? ''),
      oncopy: (text) => oncopy(text),
      onfollow: followLink,
      onnotice: (text) => onnotice(text),
    });
    finder = new Finder(proseEl);
    return () => {
      scroller.stop();
      vim?.destroy();
      vim = undefined;
      finder?.clear();
      finder = undefined;
    };
  });

  // A long chapter (an index, a book kept in one file) takes the web view
  // the better part of a second to build and lay out, and nothing else can
  // happen meanwhile. So only what the view needs goes on the page at once,
  // from the start of the chapter or from the place being gone to; the rest
  // follows a few milliseconds' worth each frame, below and above, and room
  // is kept at both ends for what has yet to come. Each lot goes in a box
  // of its own (a run): the web view goes over every child of the box it
  // adds to, which for a few thousand blocks costs more than the blocks added.
  const FIRST_BLOCKS = 24;
  /** Blocks added at a time while filling the view, and to begin with after it. */
  const STEP_BLOCKS = 8;
  /** A place this many blocks past the text on the page is started from
      afresh rather than reached by putting everything before it there. */
  const FAR_BLOCKS = 32;
  /** The chapter's own blocks on the page. */
  const TOP_BLOCKS = ':scope > .run > [data-node]';
  /** How much of a frame goes to putting text on the page while the view
      is moving, its limits in ms (a view at rest gets the most: nothing
      shows a frame arriving late), the most blocks put there in one go,
      and the shortest a frame is taken to be, in ms. */
  const FILL = { share: 0.5, least: 3, most: 12, blocks: 256, frame: 4 };
  const MOVING_MS = 200;
  /** How much more room is kept above than the text there is judged to need. */
  const ROOMY = 1.25;
  /** Blocks of the chapter that were put on the page together. */
  type Run = { start: number; nodes: Block[] };
  let grown = $state.raw<{ chapter: Chapter; runs: Run[] } | null>(null);
  let runs = $derived(
    grown?.chapter === chapter
      ? grown.runs
      : [{ start: 0, nodes: chapter.blocks.slice(0, FIRST_BLOCKS) }],
  );
  /** The chapter's blocks from `start` up to `end` are on the page. */
  let start = $derived(runs[0]!.start);
  let end = $derived(runs.at(-1)!.start + runs.at(-1)!.nodes.length);
  let whole = $derived(start === 0 && end >= chapter.blocks.length);
  /** Counts the times text reached the page that the reader may be looking at. */
  let landed = $state(0);
  let fillFrame = 0;
  let filledAt = 0;
  /** Whether any of this chapter went on the page after it first showed. */
  let filled = false;
  let movedAt = 0;
  let frameMs = 1000 / 60;
  /** Blocks to put on the page next frame, and what one has been costing in ms. */
  let stride = STEP_BLOCKS;
  let pace = 0.15;
  /** The room kept above the text, in px. */
  let above = 0;
  let carried = 0;
  /** Whether the view was last moved here, to go along with its text. */
  let carrying = false;
  let waiting: (() => void)[] = [];

  /** How much text the chapter has before each of its blocks, to judge
      how tall the ones not on the page yet will be. */
  let sizes: { chapter: Chapter; before: Float64Array } | null = null;
  const BLOCK_CHARS = 60;

  function inlineChars(items: Inline[]): number {
    let n = 0;
    for (const item of items) {
      if (item.t === 'text') n += item.s.length;
      else if (item.t === 'link') n += inlineChars(item.children);
    }
    return n;
  }

  function blockChars(nodes: Block[]): number {
    let n = 0;
    for (const node of nodes) {
      n += BLOCK_CHARS;
      if (node.t === 'h' || node.t === 'p') n += inlineChars(node.text);
      else if (node.t === 'code') n += node.code.length;
      else if (node.t === 'quote' || node.t === 'epub') n += blockChars(node.blocks);
      else if (node.t === 'list') for (const item of node.items) n += blockChars(item);
      else if (node.t === 'dl') {
        for (const item of node.items) {
          n += inlineChars(item.term);
          for (const def of item.defs) n += blockChars(def);
        }
      } else if (node.t === 'table') {
        for (const row of [...(node.header ? [node.header] : []), ...node.rows]) {
          for (const cell of row) n += cell.text ? inlineChars(cell.text) : blockChars(cell.blocks ?? []);
        }
      }
    }
    return n;
  }

  /** The text in the chapter's blocks from `from` up to `to`. */
  function textIn(from: number, to: number): number {
    if (sizes?.chapter !== chapter) {
      const before = new Float64Array(chapter.blocks.length + 1);
      chapter.blocks.forEach((node, i) => (before[i + 1] = before[i]! + blockChars([node])));
      sizes = { chapter, before };
    }
    return sizes.before[to]! - sizes.before[from]!;
  }

  /** How tall the blocks from `from` up to `to` will be, going by the
      ones on the page. */
  function guess(from: number, to: number): number {
    return ((bottom() - topEdge()) / Math.max(1, textIn(start, end))) * textIn(from, to);
  }

  /** Puts the chapter's blocks up to `to` on the page, below what is there. */
  function growDown(to: number): boolean {
    const upto = Math.min(chapter.blocks.length, to);
    if (raw !== null || upto <= end) return false;
    const last = proseEl.lastElementChild;
    grown = { chapter, runs: [...runs, { start: end, nodes: chapter.blocks.slice(end, upto) }] };
    flushSync();
    vim?.grew(last?.nextElementSibling ?? null);
    if (whole) settle();
    return true;
  }

  /** Puts the chapter's blocks from `from` on the page, above what is
      there, without moving the text in view: the room kept above gives way
      to them, by exactly their height. What is left of it once the start
      of the chapter is there is given back when the view next moves (see
      `trim`). */
  function growUp(from: number): boolean {
    const upfrom = Math.max(0, from);
    const below = proseEl.firstElementChild;
    if (raw !== null || upfrom >= start || !below) return false;
    const was = topOf(below as HTMLElement);
    grown = { chapter, runs: [{ start: upfrom, nodes: chapter.blocks.slice(upfrom, start) }, ...runs] };
    flushSync();
    vim?.grew(proseEl.firstElementChild, true);
    const room = above - (topOf(below as HTMLElement) - was);
    // More came than room was kept for: more is made, by whole pixels as
    // the view only goes by those, and the view goes down with its text.
    const more = room < 0 ? Math.ceil(-room) + (start > 0 ? roomFor(start) : 0) : 0;
    keep(room + more, more);
    if (whole) settle();
    return true;
  }

  /** Sets the room kept above the text, and takes the view along by as
      much as that moves the text. Whichever of the two makes the page
      longer comes first: a view past the end of a page is put back on it. */
  function keep(room: number, moved: number) {
    if (moved < 0) carry(moved);
    above = room;
    aboveEl.style.height = `${room}px`;
    if (moved > 0) carry(moved);
  }

  function carry(delta: number) {
    const at = scrollEl.scrollTop;
    scroller.shift(delta);
    carrying = scrollEl.scrollTop !== at;
    carried += scrollEl.scrollTop - at;
    if (hold) hold.expected += scrollEl.scrollTop - at;
  }

  /** Room to keep above for the chapter's blocks before `to`: a whole
      number of pixels, and more than they are likely to need, since
      running out means moving the view while the reader looks at it. */
  function roomFor(to: number): number {
    return Math.ceil(guess(0, to) * ROOMY);
  }

  /** Gives back the room left above a chapter whose start is on the page.
      Only as the view moves, when it does not show: lines are drawn on
      the screen's own pixels, which are not a whole number to one of ours,
      and text moved under a view at rest, even with the view following,
      lands a pixel off here and there and is seen to twitch. */
  function trim() {
    if (raw === null && start === 0 && above > 0) keep(0, -Math.round(above));
  }

  /** Starts the page afresh at a block of the chapter, with room above
      for what comes before it. Only for a chapter nobody has seen yet. */
  function startAt(node: number) {
    grown = { chapter, runs: [{ start: node, nodes: chapter.blocks.slice(node, node + FIRST_BLOCKS) }] };
    flushSync();
    if (cursorChapter === chapter && cursorRaw === raw) {
      vim?.reset();
      finder?.clear();
    }
    above = roomFor(start);
    aboveEl.style.height = `${above}px`;
  }

  function settle() {
    cancelAnimationFrame(fillFrame);
    fillFrame = 0;
    filledAt = 0;
    belowEl.style.height = '';
    // Laid out as it now is before the web view goes back to keeping the
    // view in place itself, or it would make up for the last change too.
    void scrollEl.scrollHeight;
    scrollEl.style.overflowAnchor = '';
    landed += 1;
    refind();
    reportProgress();
    for (const done of waiting.splice(0)) done();
  }

  /** Where the text on the page starts and ends, measured down the whole chapter. */
  function topEdge(): number {
    const first = proseEl.firstElementChild as HTMLElement | null;
    return first ? topOf(first) : 0;
  }

  function bottom(): number {
    const last = proseEl.lastElementChild as HTMLElement | null;
    return last ? topOf(last) + last.offsetHeight : 0;
  }

  /** Keeps room below for the blocks still to come, so the view can
      already move about as far as it will be able to. */
  function pad() {
    belowEl.style.height = `${Math.round(guess(end, chapter.blocks.length))}px`;
  }

  function fill(now: number) {
    fillFrame = 0;
    if (raw !== null || whole) return;
    filled = true;
    if (filledAt) frameMs = Math.max(FILL.frame, Math.min(frameMs, now - filledAt));
    filledAt = now;
    const began = performance.now();
    const moving = began - movedAt < MOVING_MS;
    const budget = moving
      ? Math.max(FILL.least, Math.min(FILL.most, frameMs * FILL.share))
      : FILL.most;
    const had = end - start;
    // The end nearer the view goes first.
    const view = scrollEl.scrollTop;
    const up =
      start > 0 &&
      (end >= chapter.blocks.length || view - topEdge() < bottom() - view - scrollEl.clientHeight);
    if (up) growUp(start - stride);
    else growDown(end + stride);
    // Laying the new text out is most of the work; done here, it is timed.
    void proseEl.offsetHeight;
    const spent = Math.max(0.5, performance.now() - began);
    pace = (pace + spent / Math.max(1, end - start - had)) / 2;
    stride = Math.max(1, Math.min(FILL.blocks, 2 * stride, Math.round(budget / pace)));
    if (whole) return;
    pad();
    reportProgress();
    fillFrame = requestAnimationFrame(fill);
  }

  /** Puts on the page what a view starting at `top` shows, with a screen
      more below and half a screen above; the rest of the chapter follows
      by itself. */
  function cover(top: number) {
    if (raw !== null || whole) return;
    const view = scrollEl.clientHeight;
    let grew = false;
    while (end < chapter.blocks.length && bottom() < top + 2 * view) {
      grew = growDown(end + STEP_BLOCKS) || grew;
    }
    while (start > 0 && topEdge() > top - view / 2) {
      const first = proseEl.firstElementChild as HTMLElement;
      const was = topOf(first);
      grew = growUp(start - STEP_BLOCKS) || grew;
      top += topOf(first) - was;
    }
    if (grew) landed += 1;
    if (whole) return;
    pad();
    if (!fillFrame) fillFrame = requestAnimationFrame(fill);
  }

  /** Makes sure a block of the chapter is on the page. */
  function need(node: number) {
    const at = Math.min(node, chapter.blocks.length - 1);
    if (raw !== null || at < 0 || (at >= start && at < end)) return;
    if (!filled && at >= end + FAR_BLOCKS) startAt(at);
    else if (at >= end) growDown(at + 1);
    else growUp(at);
    landed += 1;
  }

  /** Puts the rest of the chapter on the page at once, for what takes all of it. */
  function complete() {
    growUp(0);
    growDown(chapter.blocks.length);
  }

  /** How far the view has gone down along with its text since the chapter
      came up, as text was put on the page above it: the scroll position
      changed by this much without the reader having moved. */
  export function carriedDown(): number {
    return carried;
  }

  /** Resolves once all of the chapter is on the page. */
  export function settled(): Promise<void> {
    return whole || raw !== null ? Promise.resolve() : new Promise((done) => waiting.push(done));
  }

  $effect(() => {
    void chapter;
    if (raw !== null) grown = null;
    aboveEl.style.height = '';
    belowEl.style.height = '';
    // While text is still being put on the page the view is kept in place
    // here (see `growUp`), which the web view's own anchoring would undo.
    scrollEl.style.overflowAnchor = raw !== null || untrack(() => whole) ? '' : 'none';
    above = 0;
    carried = 0;
    carrying = false;
    filled = false;
    frameMs = 1000 / 60;
    finding = null;
    stride = STEP_BLOCKS;
    let current = true;
    void tick().then(() => {
      if (current) cover(scrollEl.scrollTop);
    });
    return () => {
      current = false;
      cancelAnimationFrame(fillFrame);
      fillFrame = 0;
      filledAt = 0;
    };
  });

  // A new chapter replaces the text the cursor was in. Restoring a place in
  // it must come after the cursor has been reset, so the reset is tracked.
  let cursorChapter: Chapter | undefined;
  let cursorRaw: string | null = null;

  function resetCursor() {
    if (cursorChapter === chapter && cursorRaw === raw) return;
    cursorChapter = chapter;
    cursorRaw = raw;
    vim?.reset();
    finder?.clear();
  }

  $effect(() => {
    void chapter;
    void raw;
    void tick().then(resetCursor);
  });

  // Highlights and margin signs are put on the page once it holds the
  // chapter, and again whenever either changes.
  $effect(() => {
    void chapter;
    void landed;
    const drawn = annotations.filter((item) => item.drawable);
    const notes = annotations.filter((item) => item.color === null);
    const letters = marks;
    void tick().then(() => {
      if (!proseEl) return;
      paint(proseEl, drawn);
      sign(proseEl, notes, letters);
    });
  });

  $effect(() => () => paint(proseEl, []));

  function mouseRange(): Range | null {
    const selection = getSelection();
    if (!selection || selection.isCollapsed || !proseEl.contains(selection.anchorNode)) return null;
    return selection.getRangeAt(0);
  }

  /** The character at a point in the window, as a range one character long. */
  function rangeAt(x: number, y: number): Range | null {
    const caret = document.caretRangeFromPoint(x, y);
    const node = caret?.startContainer;
    if (!caret || !(node instanceof Text) || !proseEl.contains(node)) return null;
    const at = Math.min(caret.startOffset, Math.max(0, node.length - 1));
    caret.setStart(node, at);
    caret.setEnd(node, Math.min(node.length, at + 1));
    return caret;
  }

  /** The selection to highlight or annotate: the visual selection, or text
      selected with the mouse. Null when there is none, or none of it lies
      in a paragraph or heading. */
  export function picked(): { pieces: Piece[]; box: DOMRect } | null {
    const range = vim?.selectionRange() ?? mouseRange();
    if (!range) return null;
    const all = slicesIn(proseEl, range);
    if (all.length === 0) return null;
    // A chapter may be several of the book's files and a highlight or note
    // only in one, so a selection running on into the next file comes as
    // one piece for each.
    const file = (node: number) => chapter.parts.findLastIndex((start) => start <= node);
    const groups: Slice[][] = [];
    for (const slice of all) {
      const last = groups.at(-1);
      if (last && file(last[0]!.node) === file(slice.node)) last.push(slice);
      else groups.push([slice]);
    }
    const pieces = groups.map((slices) => ({ slices, quote: textOf(proseEl, slices) }));
    return { pieces, box: range.getBoundingClientRect() };
  }

  /** The highlight or note under the selection, the cursor, or a point. */
  export function annotationAt(point?: { x: number; y: number }): Annotation | undefined {
    const range = point
      ? rangeAt(point.x, point.y)
      : (vim?.selectionRange() ?? mouseRange() ?? vim?.cursorRange());
    return range ? annotationOver(annotations, slicesIn(proseEl, range)) : undefined;
  }

  /** Where the selection or the cursor is in the window, to open a menu by. */
  export function selectionBox(): DOMRect | null {
    const range = vim?.selectionRange() ?? mouseRange() ?? vim?.cursorRange();
    return range?.getBoundingClientRect() ?? null;
  }

  /** The text an annotation covers, for one saved without it. */
  export function quoteOf(slices: Slice[]): string {
    // Read so that a template asking for a quote asks again once the rest
    // of the chapter is on the page.
    void whole;
    return textOf(proseEl, slices);
  }

  /** Drops the selection once it has been acted on. */
  export function clearSelection() {
    getSelection()?.removeAllRanges();
    if (vim?.mode === 'visual' || vim?.mode === 'visual-line') vim.action('cancel');
  }

  /** Where a bookmark set now would go: at a point in the window if one is
      given, else at the cursor, else at the top of the view. */
  export function markSpot(point?: { x: number; y: number }) {
    const range = point ? rangeAt(point.x, point.y) : vim?.cursorRange();
    if (range) return markPlace(proseEl, range.startContainer, range.startOffset);
    const top = topBlock(proseEl, position().node);
    return top ? markPlace(proseEl, top, 0) : null;
  }

  function lineHeight(): number {
    return parseFloat(getComputedStyle(proseEl).lineHeight) || 30;
  }

  /** Brings a stretch of text a third of the way down the view. */
  function show(range: Range, glide: boolean) {
    const box = range.getBoundingClientRect();
    const frame = scrollEl.getBoundingClientRect();
    const inView = box.top >= frame.top + lineHeight() && box.bottom <= frame.bottom - 2 * lineHeight();
    if (inView) return;
    const top = scrollEl.scrollTop + box.top - frame.top - scrollEl.clientHeight / 3;
    if (glide) scroller.to(top);
    else scroller.jump(top);
    reportProgress();
  }

  /** What was looked for while only part of the chapter was on the page. */
  let finding: string | null = null;

  /** Finds text in the chapter and goes to the first place at or after
      the top of the view; inside one chapter block when `node` is given.
      Returns how many places there are. While the chapter is still coming
      onto the page that is how many are there so far; `onfound` gives
      the count for all of it once it is. */
  export function find(query: string, node?: number): number {
    if (!finder) return 0;
    if (node !== undefined) need(node);
    const within = node === undefined ? null : topBlock(proseEl, node);
    let count = finder.search(query, within);
    const partial = () => node === undefined && raw === null && !whole;
    // Not being in the part on the page says nothing of the rest.
    if (count === 0 && partial()) {
      complete();
      count = finder.search(query);
    }
    finding = partial() ? query : null;
    const first = finder.startAt(scrollEl.getBoundingClientRect().top + lineHeight());
    if (first) show(first, false);
    return count;
  }

  /** Looks again now that all of the chapter is on the page, staying on
      the place the reader was on. */
  function refind() {
    if (finding === null || !finder) return;
    const on = finder.current;
    const count = finder.search(finding);
    finding = null;
    if (on) {
      finder.select(
        finder.all.findIndex((range) => range.compareBoundaryPoints(Range.START_TO_START, on) === 0),
      );
    }
    onfound(count, finder.index);
  }

  /** Goes to the next or previous place found, round the ends of the
      chapter. Returns which one that is, counted from 1. */
  export function findStep(direction: 1 | -1): number {
    const range = finder?.step(direction, true);
    if (range) show(range, true);
    return finder?.index ?? 0;
  }

  /** Which of the places found the reader is on, counted from 1. */
  export function findAt(): number {
    return finder?.index ?? 0;
  }

  export function findClear() {
    finding = null;
    finder?.clear();
  }

  /** Marks a chapter block for a moment, to show where a jump landed. */
  export function arrive(node: number) {
    need(node);
    const el = topBlock(proseEl, node);
    if (!el) return;
    el.classList.remove('arrived');
    void el.offsetWidth;
    el.classList.add('arrived');
    el.addEventListener('animationend', () => el.classList.remove('arrived'), { once: true });
  }

  /** The chapter's text as it reads on the page. */
  export function chapterText(): string {
    complete();
    return proseEl.innerText;
  }

  function reportProgress() {
    if (progressFrame) return;
    progressFrame = requestAnimationFrame(() => {
      progressFrame = 0;
      // Room still left above a chapter that starts on the page is not part of it.
      const lead = start === 0 ? above : 0;
      const top = Math.max(0, scrollEl.scrollTop - lead);
      const max = scrollEl.scrollHeight - scrollEl.clientHeight - lead;
      onprogress(max > 0 ? Math.min(1, top / max) : 1, top);
    });
  }

  function blockTops(): number[] {
    return [...proseEl.querySelectorAll<HTMLElement>(TOP_BLOCKS)].map(topOf);
  }

  export function holdScroll(direction: 1 | -1) {
    scroller.hold(direction, lineHeight());
  }

  export function releaseScroll() {
    scroller.release();
  }

  export function scrollPages(pages: number) {
    scroller.by(pages * (scrollEl.clientHeight - lineHeight()));
  }

  export function toTop() {
    growUp(0);
    trim();
    scroller.to(0);
  }

  export function toBottom() {
    growDown(chapter.blocks.length);
    scroller.to(scrollEl.scrollHeight);
  }

  export function toParagraph(direction: 1 | -1) {
    const here = scrollEl.scrollTop;
    const tops = blockTops();
    const next =
      direction === 1 ? tops.find((t) => t > here + 1) : tops.findLast((t) => t < here - 1);
    if (next !== undefined) scroller.to(next);
  }

  export function resetScroll() {
    growUp(0);
    trim();
    scroller.jump(0);
    reportProgress();
  }

  export function toAnchor(anchor: string): boolean {
    if (!whole) {
      const node = chapter.blocks.findIndex((block) => holds(block, anchor));
      if (node < 0) return false;
      need(node);
    }
    const el = proseEl.querySelector<HTMLElement>(`[data-anchor="${CSS.escape(anchor)}"]`);
    if (!el) return false;
    toElement(el);
    return true;
  }

  /** Shows a block of the chapter at the top, with the cursor on it. */
  export function toNode(node: number) {
    need(node);
    const el = topBlock(proseEl, node);
    if (el) toElement(el);
  }

  /** Where a link leads: with the cursor on, the cursor goes there too. */
  function toElement(el: HTMLElement) {
    cover(topOf(el));
    resetCursor();
    if (!vim?.jumpTo(el)) scroller.jump(topOf(el) - lineHeight());
    reportProgress();
    const block = el.closest<HTMLElement>('[data-node]');
    const gap = topOf(el) - scrollEl.scrollTop;
    if (block) holdPlace(el, Number(block.dataset.node), 0, () => scroller.jump(topOf(el) - gap));
  }

  /** Where an element starts, measured down the whole chapter. */
  function topOf(el: HTMLElement): number {
    return el.getBoundingClientRect().top - scrollEl.getBoundingClientRect().top + scrollEl.scrollTop;
  }

  export function perfTargets() {
    return { scrollEl, scroller, lineHeight: lineHeight() };
  }

  function layout(): string {
    return `${scrollEl.scrollHeight}:${scrollEl.clientWidth}`;
  }

  /** Where the cursor is, counted from the chapter block it is in: the
      text before that block may not all be on the page to count from. */
  function cursorPlace(): JumpLocation['cursor'] {
    const at = vim?.cursorPosition();
    const text = vim?.cursorRange()?.startContainer;
    const block = text?.parentElement?.closest<HTMLElement>('[data-node]');
    if (!vim || !at || !block) return null;
    return { node: Number(block.dataset.node), s: at.s - vim.textBefore(block), i: at.i };
  }

  /** Where the reader is in the chapter: the exact scroll position and
      cursor, plus the block at the top of the view and how far down it the
      view starts, which still holds if the text is laid out differently. */
  export function position(): Omit<JumpLocation, 'chapter'> {
    // Between a picture taking its size and the place being put back, the
    // view is not where the reader is.
    if (holding() && hold) {
      return {
        node: hold.node,
        offset: hold.offset,
        scrollTop: scrollEl.scrollTop,
        layout: '',
        cursor: cursorPlace(),
      };
    }
    const place = {
      node: 0,
      offset: 0,
      scrollTop: scrollEl.scrollTop,
      layout: layout(),
      cursor: cursorPlace(),
    };
    const top = scrollEl.getBoundingClientRect().top;
    for (const el of proseEl.querySelectorAll<HTMLElement>(TOP_BLOCKS)) {
      const box = el.getBoundingClientRect();
      if (box.bottom > top) {
        place.node = Number(el.dataset.node);
        // Negative when the view starts in the gap above the block.
        place.offset = box.height > 0 ? (top - box.top) / box.height : 0;
        break;
      }
    }
    return place;
  }

  export function restore(place: Omit<JumpLocation, 'chapter'>) {
    need(place.node);
    const el = topBlock(proseEl, place.node);
    if (el) cover(topOf(el) + place.offset * el.offsetHeight);
    resetCursor();
    if (place.layout === layout()) {
      scroller.jump(place.scrollTop);
    } else if (el) {
      scrollToBlock(el, place.offset);
    }
    const cursor = place.cursor;
    const block = cursor?.node === undefined ? null : topBlock(proseEl, cursor.node);
    if (cursor && block && vim && vim.mode !== 'reading') {
      vim.restoreCursor({ s: vim.textBefore(block) + cursor.s, i: cursor.i });
    }
    reportProgress();
    if (el) holdPlace(el, place.node, place.offset, () => scrollToBlock(el, place.offset));
  }

  function scrollToBlock(el: HTMLElement, offset: number) {
    scroller.jump(topOf(el) + offset * el.getBoundingClientRect().height);
  }

  type Hold = {
    el: HTMLElement;
    node: number;
    offset: number;
    expected: number;
    release: () => void;
  };
  let hold: Hold | null = null;

  /** Whether a place is still held: the reader has not moved since. */
  function holding(): boolean {
    if (!hold) return false;
    if (hold.el.isConnected && Math.abs(scrollEl.scrollTop - hold.expected) <= 1) return true;
    hold.release();
    return false;
  }

  /** A picture above the place, or in the block it is in, has no height
      until it arrives, and then moves the text out of view. Until the reader
      moves, the place is put back (by `put`) as each one takes its size.
      `node` and `offset` are the place as `position` reports it meanwhile. */
  function holdPlace(el: HTMLElement, node: number, offset: number, put: () => void) {
    hold?.release();
    const pending = [...proseEl.querySelectorAll('img')].filter(
      (img) =>
        !img.complete &&
        (el.contains(img) || img.compareDocumentPosition(el) & Node.DOCUMENT_POSITION_FOLLOWING),
    );
    if (pending.length === 0) return;
    const settle = () => {
      if (hold !== current || !holding()) return;
      put();
      current.expected = scrollEl.scrollTop;
      reportProgress();
      if (pending.every((img) => img.complete)) current.release();
    };
    const sized = new ResizeObserver(settle);
    const current: Hold = {
      el,
      node,
      offset,
      expected: scrollEl.scrollTop,
      release: () => {
        sized.disconnect();
        for (const img of pending) {
          img.removeEventListener('load', settle);
          img.removeEventListener('error', settle);
        }
        if (hold === current) hold = null;
      },
    };
    hold = current;
    for (const img of pending) {
      sized.observe(img);
      img.addEventListener('load', settle);
      img.addEventListener('error', settle);
    }
  }

  /** Offers a key to a half-typed cursor command (a count, `f`, `y`). */
  export function vimKey(token: string): boolean {
    if (token === 'G') growDown(chapter.blocks.length);
    else if (token === 'g') growUp(0);
    return vim?.key(token) ?? false;
  }

  /** Runs a cursor command; false when the action is not one. */
  export function vimAction(action: string): boolean {
    if (action === 'go_bottom') growDown(chapter.blocks.length);
    else if (action === 'go_top') growUp(0);
    return vim?.action(action) ?? false;
  }

  /** The visual selection, or failing that text selected with the mouse. */
  export function selectedText(): string | null {
    const visual = vim?.selectedText();
    if (visual) return visual;
    const selection = getSelection();
    if (!selection || selection.isCollapsed || !proseEl.contains(selection.anchorNode)) return null;
    return selection.toString();
  }

  /** Selects the word at a point in the window, as a right click on bare
      text does elsewhere, and returns it; null where there is no word. */
  export function selectWordAt(x: number, y: number): string | null {
    const caret = document.caretRangeFromPoint(x, y);
    const node = caret?.startContainer;
    if (!caret || !(node instanceof Text) || !proseEl.contains(node)) return null;
    const words = new Intl.Segmenter(undefined, { granularity: 'word' });
    for (const { segment, index, isWordLike } of words.segment(node.data)) {
      const end = index + segment.length;
      if (!isWordLike || caret.startOffset < index || caret.startOffset > end) continue;
      const word = document.createRange();
      word.setStart(node, index);
      word.setEnd(node, end);
      // The nearest caret position can be some way from the point; only a
      // word actually under it counts.
      const under = [...word.getClientRects()].some(
        (box) => x >= box.left - 2 && x <= box.right + 2 && y >= box.top && y <= box.bottom,
      );
      if (!under) return null;
      const selection = getSelection();
      selection?.removeAllRanges();
      selection?.addRange(word);
      return segment;
    }
    return null;
  }

  /** Remembers where the reader's eye is, and returns a function that puts
      that spot back at the same height after the text has been laid out
      again (a panel opened, the column changed width). */
  export function pin(): () => void {
    const selection = getSelection();
    const anchor =
      vim?.cursorRange() ??
      (selection && !selection.isCollapsed && proseEl.contains(selection.anchorNode)
        ? selection.getRangeAt(0)
        : [...proseEl.querySelectorAll(raw === null ? TOP_BLOCKS : ':scope > *')].find(
            (el) => el.getBoundingClientRect().bottom > scrollEl.getBoundingClientRect().top,
          ));
    if (!anchor) return () => {};
    const top = () => anchor.getBoundingClientRect().top - scrollEl.getBoundingClientRect().top;
    const was = top();
    return () => {
      scroller.jump(scrollEl.scrollTop + top() - was);
      reportProgress();
    };
  }

  function followLink(link: HTMLAnchorElement) {
    const { kind, chapter: target, target: anchor, part } = link.dataset;
    if (kind === 'chapter' && target !== undefined) {
      onlink({ kind, chapter: Number(target), anchor, part: part ? Number(part) : undefined });
    } else if (kind === 'anchor' && anchor) {
      onlink({ kind, anchor });
    } else if (kind === 'external') {
      onlink({ kind, url: link.getAttribute('href') ?? '' });
    }
  }

  function onclick(event: MouseEvent) {
    const target = event.target as HTMLElement;
    const link = target.closest('a');
    if (link) {
      event.preventDefault();
      followLink(link);
    } else if (target instanceof HTMLImageElement && getSelection()?.isCollapsed) {
      onimage(target.currentSrc || target.src, target.alt);
    } else if (getSelection()?.isCollapsed) {
      vim?.placeAt(event.clientX, event.clientY);
    }
  }

  function onscroll() {
    if (carrying) {
      // The view only went along with its text; nothing moved for the reader.
      carrying = false;
      reportProgress();
      return;
    }
    trim();
    movedAt = performance.now();
    reportProgress();
    vim?.viewportMoved();
    hidePreview();
  }

  // A card with where a link leads, or with a highlight's note, comes up
  // when the pointer rests on one, and stays while the pointer is on it.
  const HOVER_DELAY_MS = 350;
  const SWITCH_DELAY_MS = 150;
  const LEAVE_DELAY_MS = 250;

  type Hovered = { key: HTMLAnchorElement | Annotation; around: DOMRect };
  let preview = $state<{ content: PreviewContent; around: DOMRect; key: Hovered['key'] } | null>(
    null,
  );
  let hovered: Hovered['key'] | null = null;
  let pointer = { x: 0, y: 0, target: null as EventTarget | null };
  let hoverFrame = 0;
  let showTimer: ReturnType<typeof setTimeout> | undefined;
  let leaveTimer: ReturnType<typeof setTimeout> | undefined;

  function onhover(event: MouseEvent) {
    if (event.buttons !== 0) return;
    pointer = { x: event.clientX, y: event.clientY, target: event.target };
    if (!hoverFrame) hoverFrame = requestAnimationFrame(checkHover);
  }

  function checkHover() {
    hoverFrame = 0;
    const found = hoverAt(pointer.x, pointer.y, pointer.target);
    if ((found?.key ?? null) === hovered) return;
    hovered = found?.key ?? null;
    clearTimeout(showTimer);
    if (!found) {
      if (preview) leaveSoon();
      return;
    }
    clearTimeout(leaveTimer);
    if (preview?.key === found.key) return;
    showTimer = setTimeout(() => showPreview(found), preview ? SWITCH_DELAY_MS : HOVER_DELAY_MS);
  }

  function hoverAt(x: number, y: number, target: EventTarget | null): Hovered | null {
    if (raw !== null) return null;
    const link = target instanceof Element ? target.closest('a') : null;
    if (link && proseEl.contains(link) && link.dataset.kind) {
      const boxes = [...link.getClientRects()];
      const around =
        boxes.find((b) => x >= b.left && x <= b.right && y >= b.top && y <= b.bottom) ??
        link.getBoundingClientRect();
      return { key: link, around };
    }
    const at = sliceAt(proseEl, x, y);
    const note = at ? annotationOver(annotations, [at.slice]) : undefined;
    return at && note ? { key: note, around: at.box } : null;
  }

  function showPreview(found: Hovered) {
    if (!(found.key instanceof HTMLAnchorElement)) {
      preview = { content: { kind: 'note', annotation: found.key }, around: found.around, key: found.key };
      return;
    }
    const link = found.key;
    const { kind, chapter: target, target: anchor, part } = link.dataset;
    let content: PreviewContent | null = null;
    if (kind === 'external') {
      content = { kind: 'web', url: link.getAttribute('href') ?? '' };
    } else if (kind === 'anchor' && anchor) {
      const nodes = excerpt(chapter, anchor);
      if (nodes) content = { kind: 'link', title: null, nodes };
    } else if (kind === 'chapter' && target !== undefined) {
      const index = Number(target);
      const at = part ? Number(part) : undefined;
      content = { kind: 'link', title: null, nodes: null };
      chapterFor(index)
        .then((there) => {
          if (preview?.key !== link) return;
          const found = excerpt(there, anchor, at);
          const nodes = found ?? excerpt(there);
          // What a link points into, such as a note, is shown on its own; the
          // chapter is named above an opening that is not a heading.
          const title = (found && anchor) || nodes?.[0]?.t === 'h' ? null : there.title;
          preview = { ...preview, content: { kind: 'link', title, nodes } };
        })
        .catch(() => {
          if (preview?.key !== link) return;
          preview = { ...preview, content: { kind: 'link', title: null, nodes: null, failed: true } };
        });
    }
    preview = content ? { content, around: found.around, key: link } : null;
  }

  /** Whether a link in a card leads back to the link the card is about, as
      a note's number links back to where the note is called. */
  function leadsBackTo(link: HTMLAnchorElement, hovered: HTMLAnchorElement): boolean {
    const { kind, chapter: there, target } = link.dataset;
    if (!target || (kind === 'chapter' && Number(there) !== chapter.index)) return false;
    const spot = proseEl.querySelector(`[data-anchor="${CSS.escape(target)}"]`);
    if (!spot) return false;
    if (spot === hovered || spot.contains(hovered) || hovered.contains(spot)) return true;
    // An anchor just ahead of the link, with nothing between but space (and
    // the comments Svelte leaves where it renders).
    let next = spot.nextSibling;
    while (next instanceof Comment || (next instanceof Text && next.data.trim() === '')) {
      next = next.nextSibling;
    }
    return next === hovered || (next instanceof Element && next.contains(hovered));
  }

  function leaveSoon() {
    clearTimeout(leaveTimer);
    leaveTimer = setTimeout(hidePreview, LEAVE_DELAY_MS);
  }

  function hidePreview() {
    clearTimeout(showTimer);
    clearTimeout(leaveTimer);
    cancelAnimationFrame(hoverFrame);
    hoverFrame = 0;
    hovered = null;
    preview = null;
  }

  $effect(() => {
    void chapter;
    void raw;
    hidePreview();
  });

  $effect(() => hidePreview);

  // Selecting with the mouse past the top or bottom of the page brings
  // more text into view, faster the further past the pointer is, and the
  // selection grows to take it in.
  const DRAG_EDGE = 24;
  /** Pixels a second: at the edge, for each pixel past it, and at most. */
  const DRAG_SPEED = { base: 120, gain: 14, max: 1400 };
  let drag: { x: number; y: number; frame: number; last: number; owed: number } | null = null;

  function dragStep(now: number) {
    if (!drag) return;
    const elapsed = Math.min(now - drag.last, 50) / 1000;
    drag.last = now;
    const view = scrollEl.getBoundingClientRect();
    const past =
      drag.y > view.bottom - DRAG_EDGE
        ? drag.y - (view.bottom - DRAG_EDGE)
        : drag.y < view.top + DRAG_EDGE
          ? drag.y - (view.top + DRAG_EDGE)
          : 0;
    const selection = getSelection();
    if (past !== 0 && selection && !selection.isCollapsed) {
      const before = scrollEl.scrollTop;
      const speed = Math.min(DRAG_SPEED.max, DRAG_SPEED.base + Math.abs(past) * DRAG_SPEED.gain);
      // The view moves by whole pixels; what is left over waits for the
      // next frame, so a slow drag on a fast screen still moves.
      drag.owed += Math.sign(past) * speed * elapsed;
      const whole = Math.trunc(drag.owed);
      drag.owed -= whole;
      if (whole !== 0) scroller.jump(before + whole);
      if (scrollEl.scrollTop !== before) {
        // The text moved under a pointer that did not: the selection's
        // end follows to the text now at the edge.
        const y = past > 0 ? view.bottom - 2 : view.top + 2;
        const x = Math.max(view.left + 1, Math.min(view.right - 1, drag.x));
        const caret = document.caretRangeFromPoint(x, y);
        const end = selection.focusNode;
        if (caret && end && proseEl.contains(caret.startContainer)) {
          // Past the last line there is no text at the edge, and the
          // nearest the web view offers may lie behind the selection's
          // end; the selection only ever grows the way the view moves.
          const focus = document.createRange();
          focus.setStart(end, selection.focusOffset);
          const ahead = caret.compareBoundaryPoints(Range.START_TO_START, focus) * Math.sign(past);
          if (ahead > 0) selection.extend(caret.startContainer, caret.startOffset);
        }
      }
    }
    drag.frame = requestAnimationFrame(dragStep);
  }

  function onmousedown(event: MouseEvent) {
    hidePreview();
    if (event.button !== 0 || drag) return;
    drag = {
      x: event.clientX,
      y: event.clientY,
      frame: requestAnimationFrame(dragStep),
      last: performance.now(),
      owed: 0,
    };
  }

  function onmousemove(event: MouseEvent) {
    if (!drag) return;
    if (event.buttons & 1) {
      drag.x = event.clientX;
      drag.y = event.clientY;
    } else {
      endDrag();
    }
  }

  function endDrag() {
    if (drag) cancelAnimationFrame(drag.frame);
    drag = null;
  }

  $effect(() => endDrag);
</script>

<svelte:window {onmousemove} onmouseup={endDrag} onblur={endDrag} onkeydown={hidePreview} />


<!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
<div
  class="scroll"
  bind:this={scrollEl}
  {onscroll}
  {onclick}
  {onmousedown}
  onmousemove={onhover}
  onmouseleave={() => {
    hovered = null;
    clearTimeout(showTimer);
    if (preview) leaveSoon();
  }}
>
  <div bind:this={aboveEl}></div>
  <article
    class="prose"
    class:justify
    class:dropcaps={dropCaps}
    lang={language ?? undefined}
    bind:this={proseEl}
  >
    {#if raw !== null}
      <pre class="raw">{raw}</pre>
    {:else}
      {#key chapter}
        {#each runs as run (run.start)}
          <div class="run" data-start={run.start}>
            <Blocks nodes={run.nodes} base={run.start} top opening={run.start === 0} />
          </div>
        {/each}
      {/key}
    {/if}
  </article>
  <div bind:this={belowEl}></div>
</div>

{#if preview}
  <Preview
    content={preview.content}
    around={preview.around}
    onfollow={(link) => {
      const hovered = preview?.key;
      hidePreview();
      if (link && !(hovered instanceof HTMLAnchorElement && leadsBackTo(link, hovered))) {
        followLink(link);
      } else if (hovered instanceof HTMLAnchorElement) {
        followLink(hovered);
      }
    }}
    onenter={() => clearTimeout(leaveTimer)}
    onleave={leaveSoon}
  />
{/if}

<style>
  .scroll {
    position: relative;
    height: 100%;
    overflow-y: auto;
    background: var(--page);
    scrollbar-width: thin;
    scrollbar-color: var(--line) transparent;
  }
</style>
