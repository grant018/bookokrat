import type { PdfLayer, PdfLine } from './api';

/** A character on a page: its page, its line there, and its place in the line. */
export type Caret = { page: number; l: number; c: number };

/** A stretch of text, both ends included, in reading order. */
export type Sel = { from: Caret; to: Caret };

/** A box on a page, as fractions of its width and height. */
export type Box = [x0: number, y0: number, x1: number, y1: number];

export function before(a: Caret, b: Caret): boolean {
  return a.page !== b.page ? a.page < b.page : a.l !== b.l ? a.l < b.l : a.c < b.c;
}

export function ordered(a: Caret, b: Caret): Sel {
  return before(b, a) ? { from: b, to: a } : { from: a, to: b };
}

function escape(ch: string): string {
  return ch === '&' ? '&amp;' : ch === '<' ? '&lt;' : ch === '>' ? '&gt;' : ch;
}

/** The characters of a line: the text is counted in code points, the way
    the positions that come with it are. */
function chars(line: PdfLine): string[] {
  return [...line.text];
}

/** Where a character of a line starts and ends, across the page. */
function span(line: PdfLine, c: number): [number, number] {
  return [line.xs[c] ?? line.x0, line.xs[c + 1] ?? line.x1];
}

/** The page's text as invisible elements lying exactly over the drawn
    page: a paragraph per block, a row per line, a box per character. The
    cursor and its motions work on this as they do on a chapter's text. */
export function layerHtml(layer: PdfLayer): string {
  let html = '';
  let block: number | null = null;
  layer.lines.forEach((line, l) => {
    if (line.block !== block) {
      if (block !== null) html += '</p>';
      block = line.block;
      html += `<p data-seg="${layer.page}:${l}">`;
    } else {
      html += '<br>';
    }
    const width = Math.max(line.x1 - line.x0, 1e-6);
    const percent = (n: number) => `${(n * 100).toFixed(3)}%`;
    html +=
      `<span class="line" data-l="${l}" style="left:${percent(line.x0)};top:${percent(line.y0)};` +
      `width:${percent(width)};height:${percent(line.y1 - line.y0)}">`;
    const lead = ((line.xs[0] ?? line.x0) - line.x0) / width;
    chars(line).forEach((ch, c) => {
      const [x0, x1] = span(line, c);
      const style =
        `width:${percent(Math.max(0, x1 - x0) / width)}` +
        (c === 0 && lead > 0 ? `;margin-left:${percent(lead)}` : '');
      html += `<span class="ch" style="${style}">${escape(ch)}</span>`;
    });
    html += '</span>';
  });
  return block === null ? '' : `${html}</p>`;
}

/** The character nearest to a point on a page. */
export function nearest(layer: PdfLayer, page: number, x: number, y: number): Caret | null {
  let best: { l: number; dy: number; dx: number } | null = null;
  layer.lines.forEach((line, l) => {
    const dy = y < line.y0 ? line.y0 - y : y > line.y1 ? y - line.y1 : 0;
    const dx = x < line.x0 ? line.x0 - x : x > line.x1 ? x - line.x1 : 0;
    if (!best || dy < best.dy || (dy === best.dy && dx < best.dx)) best = { l, dy, dx };
  });
  if (!best) return null;
  const { l } = best as { l: number };
  const line = layer.lines[l]!;
  let c = 0;
  while (c + 1 < line.xs.length && line.xs[c + 1]! <= x) c++;
  return { page, l, c };
}

/** The boxes a stretch of text covers on one of its pages, a box per line. */
export function boxes(sel: Sel, layer: PdfLayer): Box[] {
  const page = layer.page;
  if (page < sel.from.page || page > sel.to.page) return [];
  const first = page === sel.from.page ? sel.from.l : 0;
  const last = page === sel.to.page ? sel.to.l : layer.lines.length - 1;
  const out: Box[] = [];
  for (let l = first; l <= last; l++) {
    const line = layer.lines[l];
    if (!line) continue;
    const a = page === sel.from.page && l === sel.from.l ? sel.from.c : 0;
    const b = page === sel.to.page && l === sel.to.l ? sel.to.c : line.xs.length - 1;
    if (b < a) continue;
    out.push([span(line, a)[0], line.y0, span(line, b)[1], line.y1]);
  }
  return out;
}

/** The text of a stretch on one of its pages, as it reads: a paragraph's
    lines joined, a word the page broke with a hyphen put back together. */
export function textOf(sel: Sel, layer: PdfLayer): string {
  const page = layer.page;
  if (page < sel.from.page || page > sel.to.page) return '';
  const first = page === sel.from.page ? sel.from.l : 0;
  const last = page === sel.to.page ? sel.to.l : layer.lines.length - 1;
  let out = '';
  let block: number | null = null;
  for (let l = first; l <= last; l++) {
    const line = layer.lines[l];
    if (!line) continue;
    const all = chars(line);
    const a = page === sel.from.page && l === sel.from.l ? sel.from.c : 0;
    const b = page === sel.to.page && l === sel.to.l ? sel.to.c : all.length - 1;
    const piece = all.slice(a, b + 1).join('').trim();
    if (!piece) continue;
    if (block === null) out = piece;
    else if (line.block !== block) out += `\n${piece}`;
    else if (/\p{L}[-­]$/u.test(out) && /^\p{Ll}/u.test(piece)) out = out.slice(0, -1) + piece;
    else out += ` ${piece}`;
    block = line.block;
  }
  return out;
}

/** The word a character is in, or just the character where there is none. */
export function wordAt(layer: PdfLayer, at: Caret): Sel {
  const all = chars(layer.lines[at.l]!);
  const inWord = (c: number) => /[\p{L}\p{N}\p{M}_'’]/u.test(all[c] ?? '');
  let a = at.c;
  let b = at.c;
  if (inWord(at.c)) {
    while (a > 0 && inWord(a - 1)) a--;
    while (b + 1 < all.length && inWord(b + 1)) b++;
  }
  return { from: { ...at, c: a }, to: { ...at, c: b } };
}

/** The whole paragraph a character is in. */
export function blockAt(layer: PdfLayer, at: Caret): Sel {
  const block = layer.lines[at.l]!.block;
  let first = at.l;
  let last = at.l;
  while (first > 0 && layer.lines[first - 1]!.block === block) first--;
  while (last + 1 < layer.lines.length && layer.lines[last + 1]!.block === block) last++;
  return {
    from: { page: at.page, l: first, c: 0 },
    to: { page: at.page, l: last, c: Math.max(0, layer.lines[last]!.xs.length - 1) },
  };
}

export function overlaps(a: Box, b: Box): boolean {
  const slack = 0.002;
  return a[0] < b[2] - slack && b[0] < a[2] - slack && a[1] < b[3] - slack && b[1] < a[3] - slack;
}
