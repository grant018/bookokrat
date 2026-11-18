import type { Annotation, HighlightColor, Slice } from './api';

/** Highlights and notes sit on paragraphs and headings. The terminal app
    finds text by counting characters from the start of the block, taking a
    line break as one and a picture as none; the same count is made here from
    the page, so a stretch marked in one app is the same stretch in the other. */
const ANNOTATABLE = 'p,h1,h2,h3,h4,h5,h6';
const COLORS: HighlightColor[] = ['red', 'green', 'blue', 'yellow', 'purple'];

/** Where a paragraph or heading is in its chapter: the chapter block it is,
    or is inside, and the way down through wrapping blocks. Null for text the
    terminal app addresses differently (list items, quotations, tables). */
function addressOf(block: Element): { node: number; path: number[] } | null {
  const path: number[] = [];
  let el = block;
  while (!el.hasAttribute('data-node')) {
    const place = el.getAttribute('data-child');
    const parent = el.parentElement;
    if (place === null || !parent?.classList.contains('epub-block')) return null;
    path.unshift(Number(place));
    el = parent;
  }
  return { node: Number(el.getAttribute('data-node')), path };
}

/** One of the chapter's own blocks on the page. They are in the runs the
    reader puts a chapter on the page in, each marked with the block it
    starts at. */
export function topBlock(prose: HTMLElement, node: number): HTMLElement | null {
  const runs = prose.children;
  let at = 0;
  let end = runs.length - 1;
  while (at < end) {
    const mid = (at + end + 1) >> 1;
    if (Number((runs[mid] as HTMLElement).dataset.start) <= node) at = mid;
    else end = mid - 1;
  }
  for (const el of runs[at]?.children ?? []) {
    if (el.getAttribute('data-node') === String(node)) return el as HTMLElement;
  }
  return null;
}

function blockOf(prose: HTMLElement, slice: Slice): Element | null {
  let el: Element | null = topBlock(prose, slice.node);
  for (const place of slice.path) {
    el = el?.querySelector(`:scope > [data-child="${place}"]`) ?? null;
  }
  return el?.matches(ANNOTATABLE) ? el : null;
}

/** Superscripts and subscripts are drawn as they are, but the terminal app
    writes them out (`^{[4]}` for a raised `[4]`), so each counts as the
    characters the terminal gives it (`data-chars`). A place inside one is
    taken as its start, or its end when past the first character. */
const SCRIPT = '[data-chars]';

function widthOf(script: Element): number {
  return Number(script.getAttribute('data-chars'));
}

function length(text: string): number {
  return [...text].length;
}

/** Characters between the start of a block and a position in it. */
function charsBefore(block: Element, container: Node, offset: number): number {
  const at = (container instanceof Element ? container : container.parentElement)?.closest(SCRIPT);
  if (at && block.contains(at) && at.parentNode) {
    const lead = document.createRange();
    lead.setStart(at, 0);
    lead.setEnd(container, offset);
    const index = Array.prototype.indexOf.call(at.parentNode.childNodes, at);
    container = at.parentNode;
    offset = lead.toString() === '' ? index : index + 1;
  }
  const range = document.createRange();
  range.setStart(block, 0);
  range.setEnd(container, offset);
  let chars = length(range.toString()) + range.cloneContents().querySelectorAll('br').length;
  for (const script of block.querySelectorAll(SCRIPT)) {
    if (range.intersectsNode(script)) chars += widthOf(script) - length(script.textContent ?? '');
  }
  return chars;
}

function charsIn(block: Element): number {
  let chars = length(block.textContent ?? '') + block.querySelectorAll('br').length;
  for (const script of block.querySelectorAll(SCRIPT)) {
    chars += widthOf(script) - length(script.textContent ?? '');
  }
  return chars;
}

/** The position in a block that a count of characters reaches. */
function positionAt(block: Element, chars: number): [Node, number] {
  const walker = document.createTreeWalker(block, NodeFilter.SHOW_TEXT | NodeFilter.SHOW_ELEMENT);
  let left = chars;
  let last: [Node, number] = [block, 0];
  let passed: Element | null = null;
  for (let node = walker.nextNode(); node; node = walker.nextNode()) {
    if (passed?.contains(node)) continue;
    if (node instanceof Element && node.matches(SCRIPT)) {
      if (left === 0) return [node, 0];
      const end: [Node, number] = [node, node.childNodes.length];
      if (left <= widthOf(node)) return end;
      left -= widthOf(node);
      last = end;
      passed = node;
    } else if (node instanceof Text) {
      let index = 0;
      for (const ch of node.data) {
        if (left === 0) return [node, index];
        left -= 1;
        index += ch.length;
      }
      last = [node, node.length];
    } else if ((node as Element).tagName === 'BR') {
      if (left === 0) return last;
      left -= 1;
    }
  }
  return last;
}

function rangeOf(block: Element, slice: Slice): Range {
  const range = document.createRange();
  range.setStart(...positionAt(block, slice.start));
  if (slice.end === null) range.setEnd(block, block.childNodes.length);
  else range.setEnd(...positionAt(block, slice.end));
  return range;
}

/** The stretches of paragraphs and headings that a range covers. */
export function slicesIn(prose: HTMLElement, range: Range): Slice[] {
  const slices: Slice[] = [];
  for (const block of prose.querySelectorAll(ANNOTATABLE)) {
    if (!range.intersectsNode(block)) continue;
    const address = addressOf(block);
    if (!address) continue;
    const start = block.contains(range.startContainer)
      ? charsBefore(block, range.startContainer, range.startOffset)
      : 0;
    const end = block.contains(range.endContainer)
      ? charsBefore(block, range.endContainer, range.endOffset)
      : charsIn(block);
    if (end > start) slices.push({ ...address, start, end });
  }
  return slices;
}

/** The character under a point in the window, as a one-character stretch,
    with its box; null over a margin, a gap or a picture. */
export function sliceAt(prose: HTMLElement, x: number, y: number): { slice: Slice; box: DOMRect } | null {
  const caret = document.caretRangeFromPoint(x, y);
  const node = caret?.startContainer;
  if (!caret || !(node instanceof Text) || !prose.contains(node)) return null;
  // The caret falls between two characters; the one under the point is
  // whichever of them has its box there.
  for (const at of [caret.startOffset, caret.startOffset - 1]) {
    if (at < 0 || at >= node.length) continue;
    const ch = document.createRange();
    ch.setStart(node, at);
    ch.setEnd(node, at + 1);
    const box = [...ch.getClientRects()].find(
      (b) => x >= b.left && x <= b.right && y >= b.top && y <= b.bottom,
    );
    if (!box) continue;
    const block = node.parentElement?.closest(ANNOTATABLE);
    const address = block ? addressOf(block) : null;
    if (!block || !address) return null;
    const start = charsBefore(block, node, at);
    return { slice: { ...address, start, end: start + 1 }, box };
  }
  return null;
}

function overlap(a: Slice, b: Slice): boolean {
  if (a.node !== b.node || a.path.join() !== b.path.join()) return false;
  return a.start < (b.end ?? Infinity) && b.start < (a.end ?? Infinity);
}

/** The first annotation that shares any text with the given stretches. */
export function annotationOver(list: Annotation[], slices: Slice[]): Annotation | undefined {
  return list.find((item) => item.slices.some((own) => slices.some((other) => overlap(own, other))));
}

/** The text an annotation covers, read off the page. */
export function textOf(prose: HTMLElement, slices: Slice[]): string {
  return slices
    .map((slice) => {
      const block = blockOf(prose, slice);
      return block ? rangeOf(block, slice).toString() : '';
    })
    .filter(Boolean)
    .join(' ');
}

/** Draws the chapter's highlights and underlines its notes. The web view
    paints them itself and keeps them in place as the text is laid out again. */
export function paint(prose: HTMLElement, list: Annotation[]) {
  if (!('highlights' in CSS)) return;
  const groups = new Map<string, Range[]>();
  for (const item of list) {
    const name = item.color ? `mark-${item.color}` : 'note';
    for (const slice of item.slices) {
      const block = blockOf(prose, slice);
      if (!block) continue;
      const ranges = groups.get(name) ?? [];
      ranges.push(rangeOf(block, slice));
      groups.set(name, ranges);
    }
  }
  for (const name of [...COLORS.map((color) => `mark-${color}`), 'note']) {
    const ranges = groups.get(name);
    if (ranges) CSS.highlights.set(name, new Highlight(...ranges));
    else CSS.highlights.delete(name);
  }
}

/** Puts the margin signs on the chapter's blocks: a dot beside each block
    with a note, a letter beside each bookmarked one. */
export function sign(prose: HTMLElement, notes: Annotation[], marks: { letter: string; node: number }[]) {
  for (const el of prose.querySelectorAll(':scope > .run > :is([data-notes], [data-mark])')) {
    el.removeAttribute('data-notes');
    el.removeAttribute('data-mark');
  }
  const top = (node: number) => topBlock(prose, node);
  for (const note of notes) {
    const first = note.slices[0];
    if (first) top(first.node)?.setAttribute('data-notes', '');
  }
  for (const mark of marks) {
    const el = top(mark.node);
    if (el) el.setAttribute('data-mark', (el.getAttribute('data-mark') ?? '') + mark.letter);
  }
}

/** A place for a bookmark from a position in the text: the chapter block,
    how many characters into it when it is a paragraph or heading, and the
    words that follow, to recognise the bookmark by. */
export function markPlace(
  prose: HTMLElement,
  container: Node,
  offset: number,
): { node: number; offset: number | null; snippet: string | null } | null {
  const start = container instanceof Element ? container : container.parentElement;
  const block = start?.closest('[data-node]');
  if (!block || !prose.contains(block)) return null;
  const within = block.matches(ANNOTATABLE) ? charsBefore(block, container, offset) : null;
  const rest = document.createRange();
  rest.selectNodeContents(block);
  if (within !== null) rest.setStart(...positionAt(block, within));
  const text = rest.toString().trim().replace(/\s+/g, ' ');
  const letters = [...text];
  const snippet = letters.length > 80 ? `${letters.slice(0, 80).join('')}…` : text;
  return { node: Number(block.getAttribute('data-node')), offset: within, snippet: snippet || null };
}
