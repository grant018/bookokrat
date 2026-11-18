import type { SmoothScroller } from './scroller';

export type VimMode = 'reading' | 'normal' | 'visual' | 'visual-line';

/** One cursor stop: a grapheme, or a run of collapsed whitespace. `gap`
    marks a line break just before it that has no character of its own. */
type Unit = { node: Text; start: number; end: number; ch: string; gap: boolean };

/** The text of one leaf block (a paragraph, heading, table cell, ...), which
    plays the part of vim's "line": `f`, `t` and text objects stay inside it. */
type Segment = { el: HTMLElement; items: (Text | null)[]; pre: boolean; units: Unit[] | null };

type Pos = { s: number; i: number };
type Box = { left: number; right: number; top: number; bottom: number };
/** Both ends included. */
type Span = [Pos, Pos];
type FindKind = 'f' | 'F' | 't' | 'T';

export type VimHooks = {
  onchange: () => void;
  oncopy: (text: string) => void;
  onfollow: (link: HTMLAnchorElement) => void;
  onnotice: (text: string) => void;
};

/** How the text on screen differs from a chapter of flowing text. */
export type VimOptions = {
  /** For the pages of a PDF. Each character sits in a box of its own
      (class `ch`) that is exactly where the page draws it, so boxes are
      read off those rather than off the invisible text; a line break
      inside a paragraph is where the page wrapped it, so copied text is
      joined across it; and blocks need not run down the page in order. */
  paged?: boolean;
};

const LEAF = 'p,h1,h2,h3,h4,h5,h6,pre,td,th,dt,dd,li,blockquote';
/** Lines kept between the cursor and the edge of the view. */
const SCROLLOFF = 3;
const YANK_FLASH_MS = 250;
/** Lines a chapter's initial letter is sunk through; `initial-letter` in reader.css. */
const INITIAL_LINES = 3;
const SPACE = 0;
const WORD = 1;
const PUNCT = 2;

/** Book text uses typographic quotes and dashes that are awkward to type, so
    the plain key also finds its typographic forms. */
const LOOKALIKES: Record<string, string> = {
  '"': '"“”„',
  "'": "'‘’",
  '-': '-‐‑–—',
};
const QUOTES: Record<string, { open: string; close: string; both: string }> = {
  '"': { open: '“„', close: '”', both: '"' },
  "'": { open: '‘', close: '’', both: "'" },
  '`': { open: '', close: '', both: '`' },
};
const BRACKETS: Record<string, [string, string]> = {
  '(': ['(', ')'],
  ')': ['(', ')'],
  '[': ['[', ']'],
  ']': ['[', ']'],
  '{': ['{', '}'],
  '}': ['{', '}'],
  '<': ['<', '>'],
  '>': ['<', '>'],
};

const segmenter = new Intl.Segmenter(undefined, { granularity: 'grapheme' });

function charClass(ch: string): number {
  if (/^\s/.test(ch)) return SPACE;
  return /^[\p{L}\p{N}\p{M}_]/u.test(ch) ? WORD : PUNCT;
}

function same(a: Pos, b: Pos): boolean {
  return a.s === b.s && a.i === b.i;
}

function before(a: Pos, b: Pos): boolean {
  return a.s < b.s || (a.s === b.s && a.i < b.i);
}

function visible(box: Box): boolean {
  return box.right - box.left > 0.5;
}

/** Rects on one visual line share a vertical centre, give or take a
    superscript; neighbouring lines are a full line height apart. */
function sameLine(a: Box, b: Box): boolean {
  const gap = Math.abs(a.top + a.bottom - b.top - b.bottom) / 2;
  return gap < Math.min(a.bottom - a.top, b.bottom - b.top) / 2;
}

function distance(box: Box, x: number): number {
  return x < box.left ? box.left - x : x > box.right ? x - box.right : 0;
}

/** Vim's normal and visual modes over the rendered book text. The cursor and
    selection are drawn as boxes laid over and under the text rather than by
    restyling it: recoloring part of a ligature such as "fl" makes the web
    view split the glyph and misdraw it. */
export class Vim {
  private segs: Segment[] = [];
  private segOf = new Map<HTMLElement, number>();
  private stale = true;
  private active = false;
  private visual: 'none' | 'char' | 'line' = 'none';
  private cursor: Pos = { s: 0, i: 0 };
  private anchor: Pos = { s: 0, i: 0 };
  /** Whether the cursor has been put somewhere worth coming back to. */
  private placed = false;
  /** Where `j` and `k` aim, measured from the left edge of the text column. */
  private goalX = 0;
  private count = '';
  private pendingFind: { kind: FindKind; yank: boolean } | null = null;
  private pendingYank: 'motion' | 'g' | 'inner' | 'around' | null = null;
  private pendingInner = false;
  private lastFind: { kind: FindKind; ch: string } | null = null;
  private range = document.createRange();
  private ox = 0;
  private oy = 0;
  private followFrame = 0;
  private yankTimer: ReturnType<typeof setTimeout> | undefined;
  private settleTimer: ReturnType<typeof setTimeout> | undefined;
  private yanked: Span | null = null;
  /** Selection boxes, under the text. */
  private marks = document.createElement('div');
  /** The block cursor, over the text; it inverts what it covers. */
  private caret = document.createElement('div');
  private observer = new ResizeObserver(() => this.draw());
  private redraw = () => this.draw();

  constructor(
    private scrollEl: HTMLElement,
    private prose: HTMLElement,
    private scroller: SmoothScroller,
    private hooks: VimHooks,
    private options: VimOptions = {},
  ) {
    this.marks.className = 'vim-marks';
    this.caret.className = 'vim-cursor';
    this.caret.hidden = true;
    scrollEl.prepend(this.marks);
    scrollEl.append(this.caret);
    // The boxes are placed by measuring the text, so they are placed again
    // whenever it is laid out afresh, moves sideways with its column, or a
    // code block scrolls sideways.
    this.observer.observe(prose);
    this.observer.observe(scrollEl);
    prose.addEventListener('scroll', this.redraw, true);
  }

  destroy() {
    cancelAnimationFrame(this.followFrame);
    clearTimeout(this.yankTimer);
    clearTimeout(this.settleTimer);
    this.observer.disconnect();
    this.prose.removeEventListener('scroll', this.redraw, true);
    this.scrollEl.classList.remove('vim-active');
    this.marks.remove();
    this.caret.remove();
  }

  get mode(): VimMode {
    if (!this.active) return 'reading';
    return this.visual === 'char' ? 'visual' : this.visual === 'line' ? 'visual-line' : 'normal';
  }

  /** What has been typed towards a command so far, for the status line. */
  get pending(): string {
    let text = this.count;
    if (this.pendingYank) {
      text += 'y' + { motion: '', g: 'g', inner: 'i', around: 'a' }[this.pendingYank];
    }
    if (this.pendingFind) text += (this.pendingFind.yank ? 'y' : '') + this.pendingFind.kind;
    if (this.pendingInner) text += 'i';
    return text;
  }

  // ── Text model ──────────────────────────────────────────────

  private index() {
    if (!this.stale) return;
    this.stale = false;
    this.segs = this.collect(this.prose, null);
    this.segOf = new Map(this.segs.map((seg, s) => [seg.el, s]));
  }

  /** The text under `root` in reading order: all of it, or what is in
      `from` and after it. */
  private collect(root: Element, from: Element | null): Segment[] {
    const walker = document.createTreeWalker(root, NodeFilter.SHOW_TEXT | NodeFilter.SHOW_ELEMENT);
    if (from) walker.currentNode = from;
    const segs: Segment[] = [];
    let current = null as Segment | null;
    let listed = false;
    for (let node = walker.nextNode(); node; node = walker.nextNode()) {
      if (node instanceof Text) {
        const el = node.parentElement?.closest<HTMLElement>(LEAF) ?? node.parentElement;
        if (!el) continue;
        if (current?.el !== el) {
          current = { el, items: [], pre: el.tagName === 'PRE', units: null };
          listed = false;
        }
        current.items.push(node);
        if (!listed && /\S/.test(node.data)) {
          listed = true;
          segs.push(current);
        }
      } else if ((node as Element).tagName === 'BR') {
        current?.items.push(null);
      }
    }
    return segs;
  }

  /** How many stretches of text come before `el` on the page. */
  textBefore(el: Element): number {
    this.index();
    let at = 0;
    let end = this.segs.length;
    while (at < end) {
      const mid = (at + end) >> 1;
      const seg = this.segs[mid]!.el;
      const before =
        !el.contains(seg) && (el.compareDocumentPosition(seg) & Node.DOCUMENT_POSITION_PRECEDING) !== 0;
      if (before) at = mid + 1;
      else end = mid;
    }
    return at;
  }

  private units(s: number): Unit[] {
    const seg = this.segs[s]!;
    if (seg.units) return seg.units;
    const units: Unit[] = [];
    let gap = false;
    // Whitespace collapses the way the page lays it out: runs become one
    // stop, and none at the start or end of the block.
    let afterSpace = true;
    for (const item of seg.items) {
      if (!item) {
        gap = true;
        afterSpace = true;
        continue;
      }
      for (const { segment, index } of segmenter.segment(item.data)) {
        const end = index + segment.length;
        if (seg.pre) {
          if (segment === '\n' || segment === '\r\n') gap = true;
          else {
            units.push({ node: item, start: index, end, ch: segment, gap });
            gap = false;
          }
        } else if (/^[ \t\n\r\f]/.test(segment)) {
          if (afterSpace) continue;
          afterSpace = true;
          units.push({ node: item, start: index, end, ch: ' ', gap });
          gap = false;
        } else {
          afterSpace = false;
          units.push({ node: item, start: index, end, ch: segment, gap });
          gap = false;
        }
      }
    }
    if (!seg.pre) {
      while (units.length > 1 && units[units.length - 1]!.ch === ' ') units.pop();
    }
    seg.units = units;
    return units;
  }

  private unit(p: Pos): Unit {
    return this.units(p.s)[p.i]!;
  }

  private first(): Pos {
    return { s: 0, i: 0 };
  }

  private last(): Pos {
    const s = this.segs.length - 1;
    return { s, i: this.units(s).length - 1 };
  }

  private segEnd(s: number): Pos {
    return { s, i: this.units(s).length - 1 };
  }

  /** The next stop in reading order, and whether a line break lies between. */
  private after(p: Pos): { pos: Pos; crossed: boolean } | null {
    const units = this.units(p.s);
    if (p.i + 1 < units.length) {
      return { pos: { s: p.s, i: p.i + 1 }, crossed: units[p.i + 1]!.gap };
    }
    if (p.s + 1 >= this.segs.length) return null;
    return { pos: { s: p.s + 1, i: 0 }, crossed: true };
  }

  private behind(p: Pos): { pos: Pos; crossed: boolean } | null {
    if (p.i > 0) return { pos: { s: p.s, i: p.i - 1 }, crossed: this.unit(p).gap };
    if (p.s === 0) return null;
    return { pos: this.segEnd(p.s - 1), crossed: true };
  }

  private step(p: Pos, dir: 1 | -1) {
    return dir === 1 ? this.after(p) : this.behind(p);
  }

  /** An apostrophe inside a word belongs to it, so "don't" is one word. */
  private classAt(p: Pos): number {
    const units = this.units(p.s);
    const unit = units[p.i]!;
    if ((unit.ch === "'" || unit.ch === '’') && !unit.gap) {
      const prev = units[p.i - 1];
      const next = units[p.i + 1];
      if (prev && next && !next.gap && charClass(prev.ch) === WORD && charClass(next.ch) === WORD) {
        return WORD;
      }
    }
    return charClass(unit.ch);
  }

  private wordClass(p: Pos, big: boolean): number {
    const cls = this.classAt(p);
    return big && cls !== SPACE ? WORD : cls;
  }

  private text([a, b]: Span): string {
    let out = '';
    for (let s = a.s; s <= b.s; s++) {
      const units = this.units(s);
      const from = s === a.s ? a.i : 0;
      const to = s === b.s ? b.i : units.length - 1;
      if (s > a.s) out += '\n';
      for (let i = from; i <= to; i++) {
        const unit = units[i]!;
        if (unit.gap && i > from) {
          if (!this.options.paged) out += '\n';
          // A word the page broke with a hyphen is put back together.
          else if (/\p{L}[-\u00ad]$/u.test(out) && /^\p{Ll}/u.test(unit.ch)) out = out.slice(0, -1);
          else if (!out.endsWith(' ')) out += ' ';
        }
        out += unit.ch;
      }
    }
    return out;
  }

  // ── Geometry ────────────────────────────────────────────────

  /** Boxes are measured from the top of the scrolled content and the left
      edge of the text column, so they stay comparable while the view moves. */
  private measure() {
    this.oy = this.scrollEl.getBoundingClientRect().top - this.scrollEl.scrollTop;
    this.ox = this.prose.getBoundingClientRect().left;
  }

  private toBox(rect: DOMRect): Box {
    return {
      left: rect.left - this.ox,
      right: rect.right - this.ox,
      top: rect.top - this.oy,
      bottom: rect.bottom - this.oy,
    };
  }

  /** Where a stop is drawn in the window. */
  private unitRect(unit: Unit): DOMRect {
    const exact = this.options.paged ? unit.node.parentElement : null;
    if (exact?.classList.contains('ch')) return exact.getBoundingClientRect();
    this.range.setStart(unit.node, unit.start);
    this.range.setEnd(unit.node, unit.end);
    return this.range.getBoundingClientRect();
  }

  private box(p: Pos): Box {
    const rect = this.unitRect(this.unit(p));
    return this.toBox(p.i === 0 ? this.initialRect(rect, p.s, 1) : rect);
  }

  /** A chapter's enlarged initial reports a box as tall as its outsized
      font, reaching well past the lines it stands beside. This gives the
      part of it beside the first `lines` lines of its paragraph: one line to
      tell which line the letter is on, all of them to draw over it. */
  private initialRect(rect: DOMRect, s: number, lines: number): DOMRect {
    const el = this.segs[s]!.el;
    if (!el.classList.contains('opening')) return rect;
    const height = this.lineHeight();
    if (rect.height <= 2 * height) return rect;
    return new DOMRect(rect.left, el.getBoundingClientRect().top, rect.width, lines * height);
  }

  private lineHeight(): number {
    return parseFloat(getComputedStyle(this.prose).lineHeight) || 30;
  }

  /** A space that fell on a line wrap takes up no room, so it cannot hold
      the cursor; this moves on to the nearest stop that can. */
  private settle(p: Pos, dir: 1 | -1): Pos {
    const units = this.units(p.s);
    for (const d of [dir, -dir]) {
      for (let i = p.i; i >= 0 && i < units.length; i += d) {
        if (units[i]!.ch !== ' ' || visible(this.box({ s: p.s, i }))) return { s: p.s, i };
      }
    }
    return p;
  }

  /** The first or last stop on the visual line that `p` is on. */
  private lineEdge(p: Pos, dir: 1 | -1): Pos {
    const units = this.units(p.s);
    const base = this.box(p);
    let edge = p;
    for (let i = p.i + dir; i >= 0 && i < units.length; i += dir) {
      const box = this.box({ s: p.s, i });
      if (!visible(box)) continue;
      if (!sameLine(box, base)) break;
      edge = { s: p.s, i };
    }
    return edge;
  }

  /** Walks the visual line from `start` and returns the stop nearest to `x`. */
  private nearestOnLine(start: Pos, dir: 1 | -1, x: number): Pos {
    const units = this.units(start.s);
    const base = this.box(start);
    let best = start;
    let bestDistance = distance(base, x);
    for (let i = start.i + dir; bestDistance > 0 && i >= 0 && i < units.length; i += dir) {
      const box = this.box({ s: start.s, i });
      if (!visible(box)) continue;
      if (!sameLine(box, base)) break;
      const d = distance(box, x);
      if (d < bestDistance) {
        best = { s: start.s, i };
        bestDistance = d;
      }
    }
    return best;
  }

  /** The stop above or below the cursor's line, as close to the goal column as it gets. */
  private lineStep(p: Pos, dir: 1 | -1): Pos | null {
    let next = this.step(this.lineEdge(p, dir), dir)?.pos;
    if (!next) return null;
    next = this.settle(next, dir);
    return this.nearestOnLine(next, dir, this.goalX);
  }

  private segBox(s: number): Box {
    return this.toBox(this.segs[s]!.el.getBoundingClientRect());
  }

  /** The stop nearest to a point in the content. */
  private near(x: number, y: number): Pos {
    let lo = 0;
    let hi = this.segs.length - 1;
    // Pages side by side do not run down the view in order, so there
    // every block is looked at.
    while (lo < hi && !this.options.paged) {
      const mid = (lo + hi) >> 1;
      if (this.segBox(mid).bottom < y) lo = mid + 1;
      else hi = mid;
    }
    const vertical = (b: Box) => (y < b.top ? b.top - y : y > b.bottom ? y - b.bottom : 0);
    let s = lo;
    let best = this.segBox(lo);
    // Table cells in one row sit side by side, and `y` may fall in the gap
    // above this block, nearer the one before it.
    const from = Math.max(0, lo - 1);
    const to = this.options.paged ? this.segs.length : Math.min(this.segs.length, lo + 24);
    for (let c = from; c < to; c++) {
      const box = this.segBox(c);
      if (c > lo && box.top > y && !this.options.paged) break;
      const closer =
        vertical(box) < vertical(best) ||
        (vertical(box) === vertical(best) && distance(box, x) < distance(best, x));
      if (closer) {
        s = c;
        best = box;
      }
    }

    const units = this.units(s);
    lo = 0;
    hi = units.length - 1;
    while (lo < hi) {
      const mid = (lo + hi) >> 1;
      if (this.box({ s, i: mid }).bottom < y) lo = mid + 1;
      else hi = mid;
    }
    const onLine = this.settle({ s, i: lo }, 1);
    return this.nearestOnLine(this.lineEdge(onLine, -1), 1, x);
  }

  /** One box per visual line of the span, in viewport coordinates. */
  private lineBoxes([a, b]: Span): Box[] {
    const first = this.unit(a);
    const last = this.unit(b);
    const boxes: Box[] = [];
    let line: Box | null = null;
    for (let s = a.s; s <= b.s; s++) {
      let inside = s > a.s;
      for (const node of this.segs[s]!.items) {
        if (!node) continue;
        const starts = s === a.s && node === first.node;
        const ends = s === b.s && node === last.node;
        if (starts) inside = true;
        if (!inside) continue;
        const exact = this.options.paged ? node.parentElement : null;
        this.range.setStart(node, starts ? first.start : 0);
        this.range.setEnd(node, ends ? last.end : node.length);
        const rects = exact?.classList.contains('ch')
          ? [exact.getBoundingClientRect()]
          : this.range.getClientRects();
        for (const full of rects) {
          if (full.width < 0.5) continue;
          const rect = this.initialRect(full, s, INITIAL_LINES);
          const box = { left: rect.left, right: rect.right, top: rect.top, bottom: rect.bottom };
          if (line && sameLine(line, box) && box.left <= line.right + 2) {
            line.right = Math.max(line.right, box.right);
            line.top = Math.min(line.top, box.top);
            line.bottom = Math.max(line.bottom, box.bottom);
          } else {
            line = box;
            boxes.push(line);
          }
        }
        if (ends) break;
      }
    }
    return boxes;
  }

  private draw() {
    const shown = this.active && !this.stale;
    this.scrollEl.classList.toggle('vim-active', shown);
    this.caret.hidden = !shown;
    if (!shown) {
      this.marks.replaceChildren();
      return;
    }
    const frame = this.scrollEl.getBoundingClientRect();
    const dx = this.scrollEl.scrollLeft - frame.left - this.scrollEl.clientLeft;
    const dy = this.scrollEl.scrollTop - frame.top - this.scrollEl.clientTop;
    const place = (el: HTMLElement, box: Box) => {
      el.style.left = `${box.left + dx}px`;
      el.style.top = `${box.top + dy}px`;
      el.style.width = `${box.right - box.left}px`;
      el.style.height = `${box.bottom - box.top}px`;
    };
    const marks: HTMLElement[] = [];
    const mark = (box: Box, kind: string) => {
      const el = document.createElement('div');
      el.className = kind;
      place(el, box);
      marks.push(el);
    };

    const selection = this.selection();
    if (selection) for (const box of this.lineBoxes(selection)) mark(box, 'selection');
    if (this.yanked) for (const box of this.lineBoxes(this.yanked)) mark(box, 'yank');
    const whole = this.unitRect(this.unit(this.cursor));
    const at = this.cursor.i === 0 ? this.initialRect(whole, this.cursor.s, INITIAL_LINES) : whole;
    // The cursor inverts whatever is under it, so it gets plain page there.
    mark(at, 'under');
    place(this.caret, at);
    this.marks.replaceChildren(...marks);
  }

  private rangeOf([a, b]: Span): Range {
    const range = document.createRange();
    const from = this.unit(a);
    const to = this.unit(b);
    range.setStart(from.node, from.start);
    range.setEnd(to.node, to.end);
    return range;
  }

  // ── Cursor and view ─────────────────────────────────────────

  private selection(): Span | null {
    if (this.visual === 'none') return null;
    const [a, b] = before(this.cursor, this.anchor)
      ? [this.cursor, this.anchor]
      : [this.anchor, this.cursor];
    return this.visual === 'line' ? [this.lineEdge(a, -1), this.lineEdge(b, 1)] : [a, b];
  }

  private paint() {
    this.draw();
    this.hooks.onchange();
  }

  private margin(): number {
    return Math.min(SCROLLOFF * this.lineHeight(), this.scrollEl.clientHeight / 4);
  }

  /** Scrolls so the cursor is comfortably in view. `base` is where the view
      is already heading, when a page motion has decided that. */
  private reveal(base = this.scroller.destination) {
    const box = this.box(this.cursor);
    const height = this.scrollEl.clientHeight;
    const margin = this.margin();
    let top = base;
    if (box.top - margin < top) top = box.top - margin;
    else if (box.bottom + margin > top + height) top = box.bottom + margin - height;
    if (Math.abs(top - this.scroller.destination) > 0.5) this.scroller.to(top);

    // Code blocks and wide tables scroll sideways on their own.
    const unit = this.unit(this.cursor);
    const scrollX = unit.node.parentElement?.closest<HTMLElement>('pre, .table-scroll');
    if (scrollX && scrollX.scrollWidth > scrollX.clientWidth) {
      this.range.setStart(unit.node, unit.start);
      this.range.setEnd(unit.node, unit.end);
      const at = this.range.getBoundingClientRect();
      const frame = scrollX.getBoundingClientRect();
      if (at.left < frame.left + 16) scrollX.scrollLeft -= frame.left + 16 - at.left;
      else if (at.right > frame.right - 16) scrollX.scrollLeft += at.right - frame.right + 16;
    }
  }

  private setGoal() {
    const box = this.box(this.cursor);
    this.goalX = (box.left + box.right) / 2;
  }

  private moveTo(p: Pos | null, keepGoal = false) {
    if (p) this.cursor = p;
    if (!keepGoal) this.setGoal();
    this.reveal();
  }

  /** Puts the cursor at the top of the view, where reading left off. */
  private placeInView() {
    const y = this.scrollEl.scrollTop + this.margin() + this.lineHeight() / 2;
    this.cursor = this.lineEdge(this.near(0, y), -1);
    this.placed = true;
    this.setGoal();
  }

  private inView(p: Pos): boolean {
    const box = this.box(p);
    const top = this.scroller.destination;
    return box.top >= top && box.bottom <= top + this.scrollEl.clientHeight;
  }

  private activate(): boolean {
    this.index();
    if (this.segs.length === 0) {
      this.hooks.onnotice('There is no text here to put the cursor on');
      return false;
    }
    this.measure();
    this.active = true;
    if (!this.placed || !this.inView(this.cursor)) this.placeInView();
    return true;
  }

  private deactivate() {
    this.active = false;
    this.visual = 'none';
    this.clearPending();
  }

  private clearPending() {
    this.count = '';
    this.pendingFind = null;
    this.pendingYank = null;
    this.pendingInner = false;
  }

  private takeCount(): number {
    const n = Math.min(parseInt(this.count, 10) || 1, 9999);
    this.count = '';
    return n;
  }

  /** The chapter on screen was replaced. */
  reset() {
    this.stale = true;
    this.placed = false;
    this.visual = 'none';
    this.clearPending();
    this.yanked = null;
    if (!this.active || !this.activate()) this.active = false;
    this.paint();
  }

  /** More of the chapter was put on the page, in the box `run`: below what
      was there, or `above` it. The cursor and the selection stay on their text. */
  grew(run: Element | null, above = false) {
    if (this.stale) return;
    if (!run) {
      this.stale = true;
      this.index();
    } else if (above) {
      const more = this.collect(run, null);
      const moved = (p: Pos): Pos => ({ s: p.s + more.length, i: p.i });
      this.segs = more.concat(this.segs);
      this.segOf = new Map(this.segs.map((seg, s) => [seg.el, s]));
      this.cursor = moved(this.cursor);
      this.anchor = moved(this.anchor);
      if (this.yanked) this.yanked = [moved(this.yanked[0]), moved(this.yanked[1])];
    } else {
      for (const seg of this.collect(this.prose, run)) {
        this.segOf.set(seg.el, this.segs.length);
        this.segs.push(seg);
      }
    }
  }

  /** Text was added or taken away without the rest changing: pages of a
      PDF coming into and going out of view. The cursor and the selection
      stay on their text if it is still there; blocks are told apart by
      their `data-seg`. */
  refresh() {
    const saved = (p: Pos) =>
      this.stale ? null : { key: this.segs[p.s]?.el.dataset.seg, i: p.i };
    const cursor = saved(this.cursor);
    const anchor = saved(this.anchor);
    this.stale = true;
    this.index();
    if (!this.active) return;
    if (this.segs.length === 0) {
      this.stale = true;
      this.draw();
      return;
    }
    const found = (was: { key: string | undefined; i: number } | null): Pos | null => {
      if (!was?.key) return null;
      const el = this.prose.querySelector<HTMLElement>(`[data-seg="${CSS.escape(was.key)}"]`);
      const s = el ? this.segOf.get(el) : undefined;
      return s === undefined ? null : { s, i: Math.min(was.i, this.units(s).length - 1) };
    };
    this.measure();
    const at = found(cursor);
    if (at) {
      this.cursor = at;
      this.anchor = found(anchor) ?? at;
    } else {
      this.visual = 'none';
      this.clearPending();
      this.placeInView();
    }
    this.paint();
  }

  /** Puts the cursor on the first or last line in view. */
  toViewEdge(edge: 'top' | 'bottom') {
    if (!this.active || this.stale) return;
    this.measure();
    const inset = this.margin() + this.lineHeight() / 2;
    const top = this.scroller.destination;
    const y = edge === 'top' ? top + inset : top + this.scrollEl.clientHeight - inset;
    this.cursor = this.lineEdge(this.near(0, y), -1);
    this.placed = true;
    this.setGoal();
    this.paint();
  }

  /** The view moved by something other than the cursor (the wheel, a jump
      from the contents). As in vim, the cursor is carried along. */
  viewportMoved() {
    // Boxes placed while the view is in motion can land a fraction of a
    // pixel off, so they are placed again once it has come to rest.
    if (this.active) {
      clearTimeout(this.settleTimer);
      this.settleTimer = setTimeout(this.redraw, 80);
    }
    if (this.mode !== 'normal' || this.followFrame) return;
    this.followFrame = requestAnimationFrame(() => {
      this.followFrame = 0;
      if (this.mode !== 'normal' || this.stale) return;
      this.measure();
      const box = this.box(this.cursor);
      const top = this.scroller.destination;
      const bottom = top + this.scrollEl.clientHeight;
      if (box.top >= top && box.bottom <= bottom) return;
      const inset = this.margin() + this.lineHeight() / 2;
      this.cursor = this.near(this.goalX, box.top < top ? top + inset : bottom - inset);
      this.paint();
    });
  }

  /** Puts the cursor on the first character at or after `target`, such as
      where a link leads, and scrolls its line to the top of the view, as
      near as the scroll margin allows. False when the cursor is off. */
  jumpTo(target: Element): boolean {
    if (!this.active) return false;
    this.index();
    if (this.segs.length === 0) return false;
    const follows = (node: Node) =>
      (target.compareDocumentPosition(node) & Node.DOCUMENT_POSITION_FOLLOWING) !== 0;
    let at: Pos | null = null;
    for (let s = 0; s < this.segs.length && !at; s++) {
      const el = this.segs[s]!.el;
      if (!el.contains(target) && !follows(el)) continue;
      const i = this.units(s).findIndex((u) => u.ch !== ' ' && follows(u.node));
      if (i >= 0) at = { s, i };
    }
    if (!at) return false;
    this.measure();
    this.visual = 'none';
    this.clearPending();
    this.cursor = at;
    this.placed = true;
    this.setGoal();
    this.scroller.jump(this.box(at).top - this.margin());
    this.paint();
    return true;
  }

  /** Moves the cursor to the character under a click. */
  placeAt(clientX: number, clientY: number) {
    if (!this.active) return;
    const doc = document as Document & {
      caretPositionFromPoint?: (x: number, y: number) => { offsetNode: Node; offset: number } | null;
    };
    let node: Node | undefined;
    let offset = 0;
    if (doc.caretPositionFromPoint) {
      const caret = doc.caretPositionFromPoint(clientX, clientY);
      node = caret?.offsetNode;
      offset = caret?.offset ?? 0;
    } else {
      const caret = document.caretRangeFromPoint(clientX, clientY);
      node = caret?.startContainer;
      offset = caret?.startOffset ?? 0;
    }
    if (node instanceof Text) this.placeOn(node, offset, clientX);
  }

  /** Moves the cursor to a character, given the text it is in. */
  placeOn(node: Text, offset: number, clientX?: number) {
    if (!this.active) return;
    const el = node.parentElement?.closest<HTMLElement>(LEAF) ?? node.parentElement;
    const s = el ? this.segOf.get(el) : undefined;
    if (s === undefined) return;
    const units = this.units(s);
    let i = units.findIndex((u) => u.node === node && u.end > offset);
    if (i < 0) i = units.findLastIndex((u) => u.node === node);
    if (i < 0) return;

    this.measure();
    // The caret sits between two characters; pick the one the pointer is over.
    if (clientX !== undefined && i > 0 && clientX - this.ox < this.box({ s, i }).left) i -= 1;
    this.clearPending();
    this.cursor = this.settle({ s, i }, 1);
    this.placed = true;
    this.setGoal();
    this.paint();
  }

  /** Where the cursor is, to come back to with `restoreCursor`. */
  cursorPosition(): { s: number; i: number } | null {
    return this.active ? { ...this.cursor } : null;
  }

  /** Puts the cursor back where `cursorPosition` found it, in the same
      chapter, without moving the view. */
  restoreCursor(p: { s: number; i: number }) {
    if (!this.active || this.stale || p.s >= this.segs.length) return;
    if (p.i >= this.units(p.s).length) return;
    this.measure();
    this.visual = 'none';
    this.clearPending();
    this.cursor = { ...p };
    this.placed = true;
    this.setGoal();
    this.paint();
  }

  selectedText(): string | null {
    const selection = this.selection();
    return selection ? this.text(selection) : null;
  }

  /** The visual selection as a range, or null outside visual mode. */
  selectionRange(): Range | null {
    const selection = this.selection();
    return selection ? this.rangeOf(selection) : null;
  }

  /** The cursor's position on screen, as a range that survives a reflow. */
  cursorRange(): Range | null {
    return this.active ? this.rangeOf([this.cursor, this.cursor]) : null;
  }

  // ── Motions ─────────────────────────────────────────────────

  private wordForward(p: Pos, big: boolean): { pos: Pos; hitEnd: boolean } {
    const cls = this.wordClass(p, big);
    let at = p;
    for (;;) {
      const next = this.after(at);
      if (!next) return { pos: at, hitEnd: true };
      at = next.pos;
      if (next.crossed || this.wordClass(at, big) !== cls) break;
    }
    while (this.wordClass(at, big) === SPACE) {
      const next = this.after(at);
      if (!next) return { pos: at, hitEnd: true };
      at = next.pos;
    }
    return { pos: at, hitEnd: false };
  }

  private wordEdge(p: Pos, dir: 1 | -1, big: boolean): Pos {
    let step = this.step(p, dir);
    if (!step) return p;
    let at = step.pos;
    while (this.wordClass(at, big) === SPACE) {
      step = this.step(at, dir);
      if (!step) return at;
      at = step.pos;
    }
    const cls = this.wordClass(at, big);
    for (;;) {
      step = this.step(at, dir);
      if (!step || step.crossed || this.wordClass(step.pos, big) !== cls) return at;
      at = step.pos;
    }
  }

  /** The word under `p` within its block. */
  private wordAt(p: Pos, big: boolean): Span {
    const units = this.units(p.s);
    const cls = this.wordClass(p, big);
    let a = p.i;
    let b = p.i;
    while (a > 0 && !units[a]!.gap && this.wordClass({ s: p.s, i: a - 1 }, big) === cls) a--;
    while (
      b + 1 < units.length &&
      !units[b + 1]!.gap &&
      this.wordClass({ s: p.s, i: b + 1 }, big) === cls
    ) {
      b++;
    }
    return [
      { s: p.s, i: a },
      { s: p.s, i: b },
    ];
  }

  private findChar(kind: FindKind, ch: string, count: number, repeat = false): Pos | null {
    const units = this.units(this.cursor.s);
    const dir = kind === 'f' || kind === 't' ? 1 : -1;
    const till = kind === 't' || kind === 'T';
    const wanted = LOOKALIKES[ch] ?? ch;
    // Repeating `t` from right before a match would otherwise find it again.
    let i = this.cursor.i + dir * (till && repeat ? 2 : 1);
    for (let found = 0; i >= 0 && i < units.length; i += dir) {
      if (wanted.includes(units[i]!.ch) && ++found === count) {
        return { s: this.cursor.s, i: till ? i - dir : i };
      }
    }
    return null;
  }

  private quotePair(p: Pos, key: string): Span | null {
    const family = QUOTES[key]!;
    const units = this.units(p.s);
    const pairs: [number, number][] = [];
    let open = -1;
    units.forEach((unit, i) => {
      // An apostrophe inside a word is not a quotation mark.
      if (this.classAt({ s: p.s, i }) === WORD) return;
      if (family.open.includes(unit.ch)) open = i;
      else if (family.close.includes(unit.ch) || (family.both === unit.ch && open >= 0)) {
        if (open >= 0) pairs.push([open, i]);
        open = -1;
      } else if (family.both === unit.ch) open = i;
    });
    const pair = pairs.find(([a, b]) => a <= p.i && p.i <= b) ?? pairs.find(([a]) => a > p.i);
    return pair
      ? [
          { s: p.s, i: pair[0] },
          { s: p.s, i: pair[1] },
        ]
      : null;
  }

  private bracketPair(p: Pos, key: string): Span | null {
    const [open, close] = BRACKETS[key]!;
    const units = this.units(p.s);
    let a = -1;
    for (let i = p.i, depth = 0; i >= 0; i--) {
      const ch = units[i]!.ch;
      if (ch === close && i !== p.i) depth++;
      else if (ch === open && depth-- === 0) {
        a = i;
        break;
      }
    }
    if (a < 0) return null;
    for (let i = a + 1, depth = 0; i < units.length; i++) {
      const ch = units[i]!.ch;
      if (ch === open) depth++;
      else if (ch === close && depth-- === 0) {
        return [
          { s: p.s, i: a },
          { s: p.s, i },
        ];
      }
    }
    return null;
  }

  /** `iw`, `a"`, `ip` and friends, as a span of text. */
  private textObject(key: string, around: boolean, count: number): Span | null {
    const p = this.cursor;
    if (key === 'w' || key === 'W') {
      const [a, b] = this.wordAt(p, key === 'W');
      if (!around) return [a, b];
      const units = this.units(p.s);
      // "A word" takes the space after it, or failing that the space before.
      if (units[b.i + 1]?.ch === ' ') return [a, { s: p.s, i: b.i + 1 }];
      if (units[a.i - 1]?.ch === ' ') return [{ s: p.s, i: a.i - 1 }, b];
      return [a, b];
    }
    if (key === 'p') {
      return [{ s: p.s, i: 0 }, this.segEnd(Math.min(this.segs.length - 1, p.s + count - 1))];
    }
    const pair =
      key in QUOTES ? this.quotePair(p, key) : key in BRACKETS ? this.bracketPair(p, key) : null;
    if (!pair || around) return pair;
    const [a, b] = pair;
    return b.i - a.i > 1
      ? [
          { s: a.s, i: a.i + 1 },
          { s: b.s, i: b.i - 1 },
        ]
      : null;
  }

  private page(pages: number) {
    const height = this.scrollEl.clientHeight;
    const delta = pages * (height - this.lineHeight());
    const from = this.scroller.destination;
    const to = Math.max(0, Math.min(from + delta, this.scrollEl.scrollHeight - height));
    const box = this.box(this.cursor);
    // At either end of the chapter the view stops but the cursor keeps going.
    const travel = to === from ? delta : to - from;
    this.cursor = this.near(this.goalX, (box.top + box.bottom) / 2 + travel);
    this.reveal(to);
  }

  // ── Yanking ─────────────────────────────────────────────────

  private yank(span: Span | null) {
    if (!span || before(span[1], span[0])) return;
    this.hooks.oncopy(this.text(span));
    this.yanked = span;
    clearTimeout(this.yankTimer);
    this.yankTimer = setTimeout(() => {
      this.yanked = null;
      this.draw();
    }, YANK_FLASH_MS);
  }

  /** Everything from `from` up to, but not including, `to`. */
  private upTo(from: Pos, to: Pos): Span | null {
    const end = this.behind(to)?.pos;
    return end && !before(end, from) ? [from, end] : null;
  }

  private yankMotion(key: string) {
    const p = this.cursor;
    if (key === 'g' || key === 'i' || key === 'a') {
      this.pendingYank = key === 'g' ? 'g' : key === 'i' ? 'inner' : 'around';
      return;
    }
    this.pendingYank = null;
    if (key === 'f' || key === 'F' || key === 't' || key === 'T') {
      this.pendingFind = { kind: key, yank: true };
      return;
    }
    const count = this.takeCount();
    switch (key) {
      case 'y': {
        let end = this.lineEdge(p, 1);
        for (let n = 1; n < count; n++) {
          const next = this.after(end)?.pos;
          if (!next) break;
          end = this.lineEdge(this.settle(next, 1), 1);
        }
        this.yank([this.lineEdge(p, -1), end]);
        break;
      }
      case 'w':
      case 'W': {
        let target = { pos: p, hitEnd: false };
        for (let n = 0; n < count && !target.hitEnd; n++) {
          target = this.wordForward(target.pos, key === 'W');
        }
        this.yank(target.hitEnd ? [p, this.last()] : this.upTo(p, target.pos));
        break;
      }
      case 'e': {
        let end = p;
        for (let n = 0; n < count; n++) end = this.wordEdge(end, 1, false);
        this.yank([p, end]);
        break;
      }
      case 'b': {
        let start = p;
        for (let n = 0; n < count; n++) start = this.wordEdge(start, -1, false);
        this.yank(this.upTo(start, p));
        break;
      }
      case '$':
        this.yank([p, this.lineEdge(p, 1)]);
        break;
      case '0':
      case '^':
        this.yank(this.upTo(this.lineEdge(p, -1), p));
        break;
      case '}':
        this.yank([p, this.segEnd(Math.min(this.segs.length - 1, p.s + count - 1))]);
        break;
      case '{': {
        const back = count - (p.i > 0 ? 1 : 0);
        this.yank(this.upTo({ s: Math.max(0, p.s - back), i: 0 }, p));
        break;
      }
      case 'G':
        this.yank([this.lineEdge(p, -1), this.last()]);
        break;
    }
  }

  // ── Keys ────────────────────────────────────────────────────

  /** Offers a key to whatever is waiting for one: a count, the character
      for `f`, the motion for `y`. Returns whether it was taken. */
  key(token: string): boolean {
    if (!this.active) return false;
    const ch = token === 'Space' ? ' ' : [...token].length === 1 ? token : null;
    const waiting = this.pendingFind || this.pendingYank || this.pendingInner;
    if (!waiting && !(ch && /\d/.test(ch) && (ch !== '0' || this.count))) {
      if (this.visual !== 'none' && ch === 'i') {
        this.pendingInner = true;
        this.hooks.onchange();
        return true;
      }
      return false;
    }
    this.measure();

    if (this.pendingFind) {
      const { kind, yank } = this.pendingFind;
      this.pendingFind = null;
      const count = this.takeCount();
      const target = ch ? this.findChar(kind, ch, count) : null;
      if (ch) this.lastFind = { kind, ch };
      if (target && yank) {
        const forward = kind === 'f' || kind === 't';
        this.yank(forward ? [this.cursor, target] : this.upTo(target, this.cursor));
      } else if (target) {
        this.moveTo(target);
      }
    } else if (this.pendingYank === 'motion') {
      if (ch) this.yankMotion(ch);
      else this.clearPending();
    } else if (this.pendingYank === 'g') {
      this.clearPending();
      if (ch === 'g') this.yank([this.first(), this.lineEdge(this.cursor, 1)]);
    } else if (this.pendingYank) {
      const around = this.pendingYank === 'around';
      this.pendingYank = null;
      const count = this.takeCount();
      if (ch) this.yank(this.textObject(ch, around, count));
    } else if (this.pendingInner) {
      this.pendingInner = false;
      if (ch !== 'w' && ch !== 'W') {
        this.hooks.onchange();
        return false;
      }
      [this.anchor, this.cursor] = this.wordAt(this.cursor, ch === 'W');
      this.moveTo(null);
    } else if (ch) {
      this.count += ch;
    }
    this.paint();
    return true;
  }

  /** Runs a keymap action. Returns false for one that is not a cursor command. */
  action(name: string): boolean {
    if (!this.active) {
      const enters = ['toggle_normal_mode', 'enter_visual_mode', 'enter_visual_line_mode'];
      if (!enters.includes(name) || !this.activate()) return enters.includes(name);
      if (name !== 'toggle_normal_mode') {
        this.visual = name === 'enter_visual_mode' ? 'char' : 'line';
        this.anchor = this.cursor;
      }
      this.reveal();
      this.paint();
      return true;
    }

    this.measure();
    const count = this.takeCount();
    const p = this.cursor;
    const repeat = (move: (at: Pos) => Pos | null | undefined, keepGoal = false) => {
      let at = p;
      for (let n = 0; n < count; n++) {
        const next = move(at);
        if (!next) break;
        at = next;
      }
      this.moveTo(at, keepGoal);
    };

    switch (name) {
      case 'move_left':
        repeat((at) => (at.i > 0 ? this.settle({ s: at.s, i: at.i - 1 }, -1) : null));
        break;
      case 'move_right':
        repeat((at) =>
          at.i + 1 < this.units(at.s).length ? this.settle({ s: at.s, i: at.i + 1 }, 1) : null,
        );
        break;
      case 'move_down':
        repeat((at) => this.lineStep(at, 1), true);
        break;
      case 'move_up':
        repeat((at) => this.lineStep(at, -1), true);
        break;
      case 'word_forward':
        repeat((at) => this.wordForward(at, false).pos);
        break;
      case 'big_word_forward':
        repeat((at) => this.wordForward(at, true).pos);
        break;
      case 'word_backward':
        repeat((at) => this.wordEdge(at, -1, false));
        break;
      case 'big_word_backward':
        repeat((at) => this.wordEdge(at, -1, true));
        break;
      case 'word_end':
        repeat((at) => this.wordEdge(at, 1, false));
        break;
      case 'big_word_end':
        repeat((at) => this.wordEdge(at, 1, true));
        break;
      case 'line_start':
        this.moveTo(this.lineEdge(p, -1));
        break;
      case 'first_non_blank': {
        let at = this.lineEdge(p, -1);
        const end = this.lineEdge(p, 1);
        while (at.i < end.i && this.unit(at).ch === ' ') at = { s: at.s, i: at.i + 1 };
        this.moveTo(at);
        break;
      }
      case 'line_end':
        this.moveTo(this.lineEdge(p, 1));
        break;
      case 'line_middle': {
        const start = this.lineEdge(p, -1);
        const end = this.lineEdge(p, 1);
        const middle = { s: p.s, i: start.i + ((end.i - start.i + 1) >> 1) };
        this.moveTo(this.settle(before(end, middle) ? end : middle, 1));
        break;
      }
      case 'paragraph_forward':
        repeat((at) => (at.s + 1 < this.segs.length ? { s: at.s + 1, i: 0 } : this.segEnd(at.s)));
        break;
      case 'paragraph_backward':
        repeat((at) => ({ s: at.i > 0 ? at.s : Math.max(0, at.s - 1), i: 0 }));
        break;
      case 'go_top':
        this.moveTo(this.first());
        break;
      case 'go_bottom':
        this.moveTo(this.lineEdge(this.last(), -1));
        break;
      case 'scroll_half_down':
        this.page(0.5);
        break;
      case 'scroll_half_up':
        this.page(-0.5);
        break;
      case 'scroll_page_down':
        this.page(1);
        break;
      case 'scroll_page_up':
        this.page(-1);
        break;
      case 'find_forward':
        this.pendingFind = { kind: 'f', yank: false };
        break;
      case 'find_backward':
        this.pendingFind = { kind: 'F', yank: false };
        break;
      case 'till_forward':
        this.pendingFind = { kind: 't', yank: false };
        break;
      case 'till_backward':
        this.pendingFind = { kind: 'T', yank: false };
        break;
      case 'repeat_find':
      case 'repeat_find_reverse': {
        if (!this.lastFind) break;
        const { ch } = this.lastFind;
        const opposite = { f: 'F', F: 'f', t: 'T', T: 't' } as const;
        const kind =
          name === 'repeat_find' ? this.lastFind.kind : opposite[this.lastFind.kind];
        repeat(() => this.findChar(kind, ch, 1, true));
        break;
      }
      case 'enter_visual_mode':
      case 'enter_visual_line_mode': {
        const wanted = name === 'enter_visual_mode' ? 'char' : 'line';
        if (this.visual === 'none') this.anchor = p;
        this.visual = this.visual === wanted ? 'none' : wanted;
        break;
      }
      case 'start_yank': {
        const selection = this.selection();
        if (selection) {
          this.yank(selection);
          this.visual = 'none';
          this.moveTo(selection[0]);
        } else {
          this.count = count > 1 ? String(count) : '';
          this.pendingYank = 'motion';
        }
        break;
      }
      case 'cancel':
        if (this.visual !== 'none') this.visual = 'none';
        else this.deactivate();
        break;
      case 'toggle_normal_mode':
        this.deactivate();
        break;
      case 'follow_link': {
        const link = this.unit(p).node.parentElement?.closest('a');
        if (link) this.hooks.onfollow(link);
        break;
      }
      default:
        return false;
    }
    this.paint();
    return true;
  }
}
