export type JumpLocation = {
  chapter: number;
  /** The chapter block at the top of the view, as the terminal app records it. */
  node: number;
  /** How far down that block the view starts, as a fraction of its height. */
  offset: number;
  /** The exact scroll position, good for as long as `layout` still matches. */
  scrollTop: number;
  layout: string;
  /** Where the normal-mode cursor was, if it was showing: which stretch of
      text and which character of it. In a book the stretches are counted
      from the chapter block `node`, in a PDF from the first on the page. */
  cursor: { node?: number; s: number; i: number } | null;
};

function same(a: JumpLocation | undefined, b: JumpLocation): boolean {
  return a !== undefined && a.chapter === b.chapter && a.node === b.node;
}

/** Vim's jump list for one book, with the terminal app's rules: places are
    recorded on leaving them, and going back first records where you are so
    that going forward returns there. */
export class JumpList {
  private entries: JumpLocation[] = [];
  /** Index of the entry the reader is at, or null when at the newest place. */
  private at: number | null = null;

  constructor(private limit: number) {}

  private add(location: JumpLocation) {
    // Leaving the same block again keeps one entry, with the newer cursor
    // and scroll position.
    if (same(this.entries.at(-1), location)) this.entries.pop();
    this.entries.push(location);
    if (this.entries.length > this.limit) this.entries.shift();
  }

  push(location: JumpLocation) {
    if (this.at !== null) this.entries.length = this.at + 1;
    this.at = null;
    this.add(location);
  }

  back(current: JumpLocation | null): JumpLocation | null {
    if (this.at === null) {
      if (current) this.add(current);
      if (this.entries.length < 2) return null;
      this.at = this.entries.length - 2;
    } else if (this.at > 0) {
      this.at -= 1;
    } else {
      return null;
    }
    return this.entries[this.at]!;
  }

  forward(): JumpLocation | null {
    if (this.at === null) return null;
    if (this.at < this.entries.length - 1) this.at += 1;
    const location = this.entries[this.at]!;
    if (this.at === this.entries.length - 1) this.at = null;
    return location;
  }

  /** Whether `back` from `current` would go anywhere: not when the only
      place recorded is the one the reader is at. */
  canGoBack(current: JumpLocation | null): boolean {
    if (this.at !== null) return this.at > 0;
    const counted = current && !same(this.entries.at(-1), current) ? 1 : 0;
    return this.entries.length + counted >= 2;
  }

  get canGoForward(): boolean {
    return this.at !== null;
  }

  clear() {
    this.entries = [];
    this.at = null;
  }
}
