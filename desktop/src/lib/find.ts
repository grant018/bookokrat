/** Finding text on the page (`/`, then `n` and `N`). Matches are painted
    by the web view as highlights, so the text itself is left alone. */

const LEAF = 'p,h1,h2,h3,h4,h5,h6,pre,td,th,dt,dd,li,blockquote';

/** Typographic quotes and dashes are found by the plain keys for them. */
const PLAIN: Record<string, string> = {
  '‘': "'",
  '’': "'",
  '“': '"',
  '”': '"',
  '„': '"',
  '‐': '-',
  '‑': '-',
  '–': '-',
  '—': '-',
  ' ': ' ',
};

function fold(text: string): string {
  let out = '';
  for (const ch of text) {
    const plain = PLAIN[ch] ?? ch;
    const lower = plain.toLowerCase();
    // A character whose lower case is longer would shift every position after it.
    out += lower.length === plain.length ? lower : plain;
  }
  return out;
}

/** A block's text as it reads, and where each of its characters came from. */
type Stretch = { text: string; nodes: Text[]; offsets: number[] };

function stretches(root: HTMLElement): Stretch[] {
  const out: Stretch[] = [];
  const walker = document.createTreeWalker(root, NodeFilter.SHOW_TEXT | NodeFilter.SHOW_ELEMENT);
  let current: Stretch | null = null;
  let block: Element | null = null;
  let spaced = true;
  const space = (node: Text, offset: number) => {
    if (spaced || !current) return;
    current.text += ' ';
    current.nodes.push(node);
    current.offsets.push(offset);
    spaced = true;
  };
  let last: Text | null = null;
  for (let node = walker.nextNode(); node; node = walker.nextNode()) {
    if (node instanceof Text) {
      const el = node.parentElement?.closest(LEAF) ?? node.parentElement;
      if (el !== block) {
        block = el;
        current = { text: '', nodes: [], offsets: [] };
        out.push(current);
        spaced = true;
      }
      const data = node.data;
      for (let i = 0; i < data.length; i++) {
        const ch = data[i]!;
        if (/\s/.test(ch)) {
          space(node, i);
        } else {
          current!.text += ch;
          current!.nodes.push(node);
          current!.offsets.push(i);
          spaced = false;
        }
      }
      last = node;
    } else if ((node as Element).tagName === 'BR' && last) {
      space(last, last.length);
    }
  }
  return out;
}

export class Finder {
  private ranges: Range[] = [];
  private at = -1;

  /** `painted` is false where the caller draws the matches itself. */
  constructor(
    private root: HTMLElement,
    private painted = true,
  ) {}

  /** Every match, in reading order. */
  get all(): Range[] {
    return this.ranges;
  }

  /** Makes the match at a position in `all` the current one. */
  select(index: number): Range | null {
    this.at = index >= 0 && index < this.ranges.length ? index : -1;
    this.paint();
    return this.current;
  }

  get count(): number {
    return this.ranges.length;
  }

  /** The match the reader is on, counted from 1; 0 when on none. */
  get index(): number {
    return this.at + 1;
  }

  get current(): Range | null {
    return this.ranges[this.at] ?? null;
  }

  /** Finds every place the words occur, in reading order. When `within`
      is given, only inside that element. Returns how many there are. */
  search(query: string, within?: Element | null): number {
    this.ranges = [];
    this.at = -1;
    const needle = fold(query.trim().replace(/\s+/g, ' '));
    if (needle) {
      for (const stretch of stretches((within as HTMLElement | null) ?? this.root)) {
        const hay = fold(stretch.text);
        for (let from = hay.indexOf(needle); from >= 0; from = hay.indexOf(needle, from + needle.length)) {
          const to = from + needle.length - 1;
          const range = document.createRange();
          range.setStart(stretch.nodes[from]!, stretch.offsets[from]!);
          range.setEnd(stretch.nodes[to]!, stretch.offsets[to]! + 1);
          this.ranges.push(range);
        }
      }
    }
    this.paint();
    return this.ranges.length;
  }

  /** Goes to the first match at or below a height in the window, or
      failing that the first of all. */
  startAt(top: number): Range | null {
    if (this.ranges.length === 0) return null;
    const found = this.ranges.findIndex((range) => range.getBoundingClientRect().bottom >= top);
    this.at = found < 0 ? 0 : found;
    this.paint();
    return this.current;
  }

  /** Goes to the match after or before the current one. Null when that
      would run off the end, unless `wrap` is set. */
  step(direction: 1 | -1, wrap: boolean): Range | null {
    if (this.ranges.length === 0) return null;
    const next = this.at + direction;
    if (next < 0 || next >= this.ranges.length) {
      if (!wrap) return null;
      this.at = (next + this.ranges.length) % this.ranges.length;
    } else {
      this.at = next;
    }
    this.paint();
    return this.current;
  }

  /** Goes to the first or last match. */
  edge(which: 'first' | 'last'): Range | null {
    if (this.ranges.length === 0) return null;
    this.at = which === 'first' ? 0 : this.ranges.length - 1;
    this.paint();
    return this.current;
  }

  clear() {
    this.ranges = [];
    this.at = -1;
    this.paint();
  }

  private paint() {
    if (!this.painted || !('highlights' in CSS)) return;
    const others = this.ranges.filter((_, index) => index !== this.at);
    if (others.length > 0) CSS.highlights.set('find', new Highlight(...others));
    else CSS.highlights.delete('find');
    const current = this.current;
    if (current) {
      const highlight = new Highlight(current);
      highlight.priority = 1;
      CSS.highlights.set('find-current', highlight);
    } else {
      CSS.highlights.delete('find-current');
    }
  }
}
