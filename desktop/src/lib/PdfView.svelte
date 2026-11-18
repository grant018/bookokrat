<script lang="ts" module>
  /** What the status line says about how a PDF is being shown. */
  export type PdfInfo = {
    page: number;
    pages: number;
    /** The number printed on the page, where the document's numbering is known. */
    printed: number | null;
    themed: boolean;
    watching: boolean;
  };

  /** Space between pages, around them, and the widest a page is first shown. */
  const GAP = 20;
  const PAD = 28;
  const COMFORT = 920;
  const ZOOM = { min: 0.25, max: 6, step: 1.15 };
  /** The widest a page is drawn, in pixels; wider is stretched. */
  const MAX_PIXELS = 6000;
  /** How long a changing size must rest before pages are drawn again. */
  const REDRAW_PAUSE_MS = 90;
  const LINE = 40;
</script>

<script lang="ts">
  import { tick, untrack } from 'svelte';
  import { SvelteMap } from 'svelte/reactivity';
  import { api, pdfPage, type Annotation, type PdfLayer, type PdfMeta, type PdfSpot } from './api';
  import { Finder } from './find';
  import type { JumpLocation } from './jumps';
  import {
    blockAt,
    boxes,
    layerHtml,
    nearest,
    ordered,
    overlaps,
    textOf,
    wordAt,
    type Box,
    type Caret,
    type Sel,
  } from './pdftext';
  import { prefs } from './prefs.svelte';
  import { SmoothScroller } from './scroller';
  import { pageColors } from './themes';
  import { Vim, type VimMode } from './vim';

  let {
    meta,
    annotations,
    marks,
    onprogress,
    oninfo,
    onvim,
    oncopy,
    onnotice,
    onexternal,
    onleave,
  }: {
    meta: PdfMeta;
    annotations: Annotation[];
    /** Bookmarks in this document: the page, and the line on it if known. */
    marks: { letter: string; page: number; line: number | null }[];
    /** `fraction` is how far through the document the view is. */
    onprogress: (fraction: number, scrollTop: number) => void;
    oninfo: (info: PdfInfo) => void;
    onvim: (mode: VimMode, pending: string) => void;
    oncopy: (text: string) => void;
    onnotice: (text: string, error?: boolean) => void;
    onexternal: (url: string) => void;
    /** Called before a link is followed, so the place can be come back to. */
    onleave: () => void;
  } = $props();

  type Placed = { index: number; left: number; top: number; width: number; height: number };
  type Row = { top: number; height: number; pages: Placed[] };

  let scrollEl: HTMLElement;
  let pagesEl: HTMLElement;
  let scroller: SmoothScroller;
  let vim: Vim | undefined;
  let finder: Finder | undefined;

  let viewWidth = $state(0);
  let viewHeight = $state(0);
  let scrollTop = $state(0);
  // The view is made afresh for each document, so what the document
  // says of itself is read once, here.
  const opened = untrack(() => meta);
  let pages = $state.raw(opened.pages);
  let themed = $state(opened.themed);
  let tintImages = $state(opened.tint_images);
  let watching = $state(false);
  let linkHints = $state(false);
  /** Tells one look of the pages from another in the web view's cache. */
  let stamp = $state(1);
  /** In page mode: the page (or pair) on screen. */
  let turned = $state(0);
  /** The mouse's selection. */
  let selection = $state.raw<Sel | null>(null);
  /** What `/` found among the pages that are loaded. */
  let found = $state.raw<{ sels: Sel[]; at: number }>({ sels: [], at: -1 });
  /** Where a jump landed, marked for a moment. */
  let arrived = $state.raw<{ page: number; y0: number; y1: number } | null>(null);

  const layers = new SvelteMap<number, PdfLayer>();
  /** The picture each page is showing, once it has loaded. */
  const pictures = new SvelteMap<number, string>();
  const loading = new Map<number, string>();
  /** The page and how far down it the view starts: what is kept in view
      when the pages change size. */
  let anchor = { page: opened.view.page, offset: opened.view.offset };
  let frame = 0;
  let refreshFrame = 0;
  let redrawTimer: ReturnType<typeof setTimeout> | undefined;
  let arrivedTimer: ReturnType<typeof setTimeout> | undefined;
  let press: { at: Caret | null; x: number; y: number; moved: boolean } | null = null;
  let query = '';
  let queryPages: { query: string; pages: number[] } | null = null;

  let spread = $derived(prefs.saved.pdf_spread);
  let paged = $derived(prefs.saved.pdf_paged);

  /** The pages in rows: one to a row, or two side by side. */
  let groups = $derived.by(() => {
    const out: number[][] = [];
    for (let index = 0; index < pages.length; index += spread ? 2 : 1) {
      out.push(spread && index + 1 < pages.length ? [index, index + 1] : [index]);
    }
    return out;
  });

  function sized(members: number[], zoom: number): [number, number][] {
    const size = (index: number) => pages[index] ?? [612, 792];
    if (paged) {
      // The whole page (or pair) fits in the window.
      const across = members.reduce((sum, index) => sum + size(index)[0], 0);
      const tall = Math.max(...members.map((index) => size(index)[1]));
      const fit = Math.min(
        (viewHeight - 2 * PAD) / tall,
        (viewWidth - 2 * PAD - GAP * (members.length - 1)) / across,
      );
      return members.map((index) => [size(index)[0] * fit * zoom, size(index)[1] * fit * zoom]);
    }
    const count = spread ? 2 : 1;
    const room = (viewWidth - 2 * PAD - GAP * (count - 1)) / count;
    const width = Math.max(40, Math.min(room, COMFORT) * zoom);
    return members.map((index) => [width, (width * size(index)[1]) / size(index)[0]]);
  }

  /** Where every page lies in the scrolled content. In page mode only the
      row on screen is laid out. */
  let layout = $derived.by(() => {
    const rows: Row[] = [];
    if (viewWidth === 0 || viewHeight === 0) return { rows, width: 0, height: 0 };
    const zoom = prefs.saved.pdf_zoom;
    const shownGroups = paged ? groups.slice(turned, turned + 1) : groups;
    let top = PAD;
    let widest = 0;
    const partial: { top: number; height: number; width: number; sizes: [number, number][]; members: number[] }[] = [];
    for (const members of shownGroups) {
      const sizes = sized(members, zoom);
      const width = sizes.reduce((sum, [w]) => sum + w, 0) + GAP * (sizes.length - 1);
      const height = Math.max(...sizes.map(([, h]) => h));
      partial.push({ top, height, width, sizes, members });
      top += height + GAP;
      widest = Math.max(widest, width);
    }
    const width = Math.max(viewWidth, widest + 2 * PAD);
    let height = top - GAP + PAD;
    if (paged && partial[0]) {
      height = Math.max(viewHeight, partial[0].height + 2 * PAD);
      partial[0].top = Math.max(PAD, (height - partial[0].height) / 2);
    }
    for (const row of partial) {
      let left = (width - row.width) / 2;
      rows.push({
        top: row.top,
        height: row.height,
        pages: row.members.map((index, i) => {
          const [w, h] = row.sizes[i]!;
          const placed = { index, left, top: row.top + (row.height - h) / 2, width: w, height: h };
          left += w + GAP;
          return placed;
        }),
      });
    }
    return { rows, width, height };
  });

  /** The pages near enough to the view to be kept on the page. */
  let mounted = $derived.by(() => {
    const lo = scrollTop - viewHeight;
    const hi = scrollTop + 2 * viewHeight;
    return layout.rows
      .filter((row) => paged || (row.top + row.height >= lo && row.top <= hi))
      .flatMap((row) => row.pages);
  });

  function rowOf(page: number): number {
    return Math.min(groups.length - 1, Math.max(0, spread ? page >> 1 : page));
  }

  function rowAt(top: number): Row | undefined {
    return layout.rows.find((row) => row.top + row.height > top + PAD) ?? layout.rows.at(-1);
  }

  /** The page at the top of the view and how far down it the view starts. */
  function where(): { page: number; offset: number } {
    const row = rowAt(scrollEl.scrollTop);
    const first = row?.pages[0];
    if (!row || !first) return anchor;
    const offset = paged ? 0 : (scrollEl.scrollTop - row.top) / row.height;
    return { page: first.index, offset: Math.max(0, Math.min(0.999, offset)) };
  }

  function topOf(place: { page: number; offset: number }): number {
    if (paged) return 0;
    const row = layout.rows[rowOf(place.page)];
    if (!row) return 0;
    // A page begun at its top shows the gap above it too.
    return place.offset <= 0 ? row.top - PAD / 2 : row.top + place.offset * row.height;
  }

  function report() {
    if (frame) return;
    frame = requestAnimationFrame(() => {
      frame = 0;
      scrollTop = scrollEl.scrollTop;
      anchor = where();
      const max = Math.max(1, pages.length);
      onprogress(Math.min(1, (anchor.page + anchor.offset) / max), scrollEl.scrollTop);
      vim?.viewportMoved();
    });
  }

  $effect(() => {
    scroller = new SmoothScroller(scrollEl);
    vim = new Vim(
      scrollEl,
      pagesEl,
      scroller,
      {
        onchange: () => {
          const mode = vim?.mode ?? 'reading';
          if (mode === 'visual' || mode === 'visual-line') selection = null;
          onvim(mode, vim?.pending ?? '');
        },
        oncopy: (text) => oncopy(text),
        onfollow: () => {},
        onnotice: (text) => onnotice(text),
      },
      { paged: true },
    );
    finder = new Finder(pagesEl, false);
    const sizes = new ResizeObserver(() => {
      viewWidth = scrollEl.clientWidth;
      viewHeight = scrollEl.clientHeight;
    });
    sizes.observe(scrollEl);
    turned = rowOf(opened.view.page);
    return () => {
      sizes.disconnect();
      scroller.stop();
      vim?.destroy();
      vim = undefined;
      cancelAnimationFrame(frame);
      cancelAnimationFrame(refreshFrame);
      clearTimeout(redrawTimer);
      clearTimeout(arrivedTimer);
    };
  });

  // The pages changed size (the window, the zoom, the layout): the page
  // that was at the top of the view is put back there.
  $effect(() => {
    void layout;
    untrack(() => {
      if (layout.rows.length === 0) return;
      void tick().then(() => {
        scroller.jump(topOf(anchor));
        scrollTop = scrollEl.scrollTop;
        report();
      });
    });
  });

  $effect(() => {
    void scrollTop;
    const laid = layout.rows.length > 0;
    const at = paged ? (groups[turned]?.[0] ?? 0) : untrack(() => (laid ? where().page : anchor.page));
    oninfo({
      page: at,
      pages: pages.length,
      printed: opened.number_offset === null ? null : at + opened.number_offset,
      themed,
      watching,
    });
  });

  function pixels(width: number): number {
    return Math.max(16, Math.min(MAX_PIXELS, Math.round(width * devicePixelRatio)));
  }

  /** Loads a page's picture, and its text if it is not there yet. The
      picture on screen stays, stretched, until the new one has arrived. */
  function load(page: number, width: number, attempt = 0) {
    const url = pdfPage(String(stamp), page, width);
    if (pictures.get(page) !== url && loading.get(page) !== url) {
      loading.set(page, url);
      const image = new Image();
      image.src = url;
      image
        .decode()
        .then(() => {
          if (loading.get(page) !== url) return;
          loading.delete(page);
          pictures.set(page, url);
        })
        .catch(() => {
          if (loading.get(page) !== url) return;
          loading.delete(page);
          // The page was dropped from the queue as the view moved past,
          // or the look changed under it; it is asked for again if it
          // is still wanted.
          const still = mounted.find((placed) => placed.index === page);
          if (still && attempt < 3) {
            setTimeout(() => load(page, pixels(still.width), attempt + 1), 150 * (attempt + 1));
          }
        });
    }
    if (!layers.has(page)) {
      const asked = stamp;
      api
        .pdfLayer(page, width)
        .then((layer) => {
          if (asked === stamp) layers.set(page, layer);
        })
        .catch((e) => console.warn(`No text for page ${page + 1}: ${e}`));
    }
  }

  $effect(() => {
    const wanted = mounted.map((placed) => [placed.index, pixels(placed.width)] as const);
    void stamp;
    clearTimeout(redrawTimer);
    // The first pictures are fetched at once; a size that is still
    // changing (a zoom held down, a window being dragged) waits a moment.
    const fresh = untrack(() => wanted.every(([page]) => !pictures.has(page)));
    redrawTimer = setTimeout(
      () => {
        for (const [page, width] of wanted) load(page, width);
        // Pictures of pages long out of view are let go.
        for (const page of [...pictures.keys()]) {
          if (!wanted.some(([index]) => Math.abs(index - page) <= 12)) pictures.delete(page);
        }
      },
      fresh ? 0 : REDRAW_PAUSE_MS,
    );
  });

  // The theme changed: pages drawn in the old colours are drawn again.
  let paper = $derived(pageColors(prefs.theme));
  let drawnLook = '';
  $effect(() => {
    const look = { themed, tint_images: tintImages, ...paper };
    const key = JSON.stringify(look);
    if (drawnLook === '') {
      drawnLook = key;
      return;
    }
    if (key === drawnLook) return;
    drawnLook = key;
    api
      .pdfLook(look)
      .then(() => {
        loading.clear();
        stamp += 1;
      })
      .catch((e) => onnotice(String(e), true));
  });

  /** Text arrived or left; the cursor and what `/` found are matched to it again. */
  function refreshSoon() {
    if (refreshFrame) return;
    refreshFrame = requestAnimationFrame(() => {
      refreshFrame = 0;
      vim?.refresh();
      if (query) rescan();
    });
  }

  function textLayer(el: HTMLElement, layer: PdfLayer | undefined) {
    const set = (next: PdfLayer | undefined) => {
      el.innerHTML = next ? layerHtml(next) : '';
      refreshSoon();
    };
    set(layer);
    return { update: set, destroy: refreshSoon };
  }

  // ── Places in the text ──────────────────────────────────────

  function pageEl(page: number): HTMLElement | null {
    return pagesEl.querySelector<HTMLElement>(`:scope > [data-page="${page}"]`);
  }

  /** The character nearest a point in the window. */
  function caretAt(x: number, y: number): Caret | null {
    let best: { page: number; box: DOMRect; distance: number } | null = null;
    for (const placed of mounted) {
      const box = pageEl(placed.index)?.getBoundingClientRect();
      if (!box) continue;
      const dx = x < box.left ? box.left - x : x > box.right ? x - box.right : 0;
      const dy = y < box.top ? box.top - y : y > box.bottom ? y - box.bottom : 0;
      if (!best || dx + dy < best.distance) best = { page: placed.index, box, distance: dx + dy };
    }
    if (!best) return null;
    const { page, box } = best as { page: number; box: DOMRect };
    const layer = layers.get(page);
    return layer
      ? nearest(layer, page, (x - box.left) / box.width, (y - box.top) / box.height)
      : null;
  }

  /** The point on a page under a point in the window. */
  export function spotAt(x: number, y: number): PdfSpot | null {
    for (const placed of mounted) {
      const box = pageEl(placed.index)?.getBoundingClientRect();
      if (box && x >= box.left && x <= box.right && y >= box.top && y <= box.bottom) {
        return { page: placed.index, x: (x - box.left) / box.width, y: (y - box.top) / box.height };
      }
    }
    return null;
  }

  function charEl(at: Caret): HTMLElement | null {
    const line = pageEl(at.page)?.querySelector(`.line[data-l="${at.l}"]`);
    return (line?.children[at.c] as HTMLElement | undefined) ?? null;
  }

  function caretOf(node: Node | null): Caret | null {
    const ch = (node instanceof Text ? node.parentElement : (node as Element | null))?.closest('.ch');
    const line = ch?.parentElement;
    const page = line?.closest<HTMLElement>('[data-page]');
    if (!ch || !line || !page) return null;
    return {
      page: Number(page.dataset.page),
      l: Number((line as HTMLElement).dataset.l),
      c: Array.prototype.indexOf.call(line.children, ch),
    };
  }

  function selOf(range: Range | null | undefined): Sel | null {
    if (!range) return null;
    const from = caretOf(range.startContainer);
    const to = caretOf(range.endContainer);
    return from && to ? ordered(from, to) : null;
  }

  /** The visual selection, or failing that the mouse's. */
  function chosen(): Sel | null {
    return selOf(vim?.selectionRange()) ?? selection;
  }

  function clientBox(page: number, box: Box): DOMRect | null {
    const frameBox = pageEl(page)?.getBoundingClientRect();
    if (!frameBox) return null;
    return new DOMRect(
      frameBox.left + box[0] * frameBox.width,
      frameBox.top + box[1] * frameBox.height,
      (box[2] - box[0]) * frameBox.width,
      (box[3] - box[1]) * frameBox.height,
    );
  }

  function textAcross(sel: Sel): string {
    const parts: string[] = [];
    for (let page = sel.from.page; page <= sel.to.page; page++) {
      const layer = layers.get(page);
      if (layer) parts.push(textOf(sel, layer));
    }
    return parts.filter(Boolean).join('\n');
  }

  // ── What the app asks of a document ─────────────────────────

  export function position(): Omit<JumpLocation, 'chapter'> & { page: number } {
    const at = where();
    return {
      page: at.page,
      node: 0,
      offset: at.offset,
      scrollTop: scrollEl.scrollTop,
      layout: `${layout.height}:${layout.width}`,
      cursor: vim?.cursorPosition() ?? null,
    };
  }

  /** Goes to a page, `offset` of the way down it. */
  export function goTo(page: number, offset = 0, glide = false) {
    const target = Math.max(0, Math.min(pages.length - 1, page));
    anchor = { page: target, offset };
    if (paged) {
      turned = rowOf(target);
      return;
    }
    if (glide) scroller.to(topOf(anchor));
    else scroller.jump(topOf(anchor));
    report();
  }

  /** Goes to a line of a page, once the page's text is known, and marks
      it for a moment. */
  export async function goToLine(page: number, line: number | null) {
    goTo(page);
    if (line === null) return;
    const layer = await layerOf(page);
    const found = layer?.lines[line];
    if (!found) return;
    showSpot({ page, x: found.x0, y: (found.y0 + found.y1) / 2 }, [found.y0, found.y1]);
  }

  /** Brings a point of a page into view and marks the line there. */
  export function showSpot(spot: PdfSpot, span?: [number, number]) {
    const band = span ?? [spot.y - 0.008, spot.y + 0.008];
    goTo(spot.page, paged ? 0 : Math.max(0, band[0] - 0.12));
    arrived = { page: spot.page, y0: band[0], y1: band[1] };
    clearTimeout(arrivedTimer);
    arrivedTimer = setTimeout(() => (arrived = null), 1600);
  }

  async function layerOf(page: number): Promise<PdfLayer | undefined> {
    for (let waited = 0; waited < 80 && !layers.has(page); waited++) {
      await new Promise((resolve) => setTimeout(resolve, 50));
    }
    return layers.get(page);
  }

  export function restore(place: { page?: number; offset: number; cursor?: { s: number; i: number } | null }) {
    goTo(place.page ?? anchor.page, place.offset);
  }

  /** Keeps the place across a change in the width of the view. */
  export function pin(): () => void {
    const at = where();
    return () => {
      anchor = at;
    };
  }

  export function holdScroll(direction: 1 | -1) {
    if (paged && layout.height <= viewHeight) turn(direction);
    else scroller.hold(direction, LINE);
  }

  export function releaseScroll() {
    scroller.release();
  }

  export function scrollPages(amount: number) {
    if (paged && layout.height <= viewHeight) turn(amount > 0 ? 1 : -1);
    else scroller.by(amount * (scrollEl.clientHeight - LINE));
  }

  export function toTop() {
    goTo(0, 0, !paged);
  }

  export function toBottom() {
    if (paged) goTo(pages.length - 1);
    else scroller.to(scrollEl.scrollHeight);
  }

  /** Moves a page (or pair) on or back. */
  function turn(direction: number) {
    const row = paged ? turned : rowOf(where().page);
    const next = Math.max(0, Math.min(groups.length - 1, row + direction));
    const page = groups[next]?.[0];
    if (page === undefined || next === row) {
      // Back from the middle of the first page still goes to its top.
      if (!paged && direction < 0) goTo(0, 0, true);
      return;
    }
    goTo(page, 0, true);
  }

  function setZoom(next: number) {
    prefs.update({ pdf_zoom: Math.round(Math.max(ZOOM.min, Math.min(ZOOM.max, next)) * 1000) / 1000 });
  }

  /** The zoom at which a page is as wide as the window. */
  function widthZoom(): number {
    const count = spread ? 2 : 1;
    const room = (viewWidth - 2 * PAD - GAP * (count - 1)) / count;
    if (!paged) return room / Math.min(room, COMFORT);
    const members = groups[turned] ?? [0];
    const [width] = sized(members, 1).reduce(([w], [next]) => [w + next, 0], [0, 0]);
    return (viewWidth - 2 * PAD - GAP * (members.length - 1)) / Math.max(1, width);
  }

  async function setLook(next: { themed?: boolean; tint?: boolean }) {
    themed = next.themed ?? themed;
    tintImages = next.tint ?? tintImages;
    const at = where();
    try {
      await api.savePagePlace(at.page, at.offset, themed, tintImages);
    } catch (e) {
      onnotice(String(e), true);
    }
  }

  /** How the pages are coloured, to be kept with the place. */
  export function look(): { themed: boolean; tintImages: boolean } {
    return { themed, tintImages };
  }

  /** The file changed on disk and was read again. */
  export function reloaded(next: [number, number][]) {
    pages = next;
    layers.clear();
    loading.clear();
    queryPages = null;
    stamp += 1;
  }

  /** A PDF command. Returns false for an action that is not one. */
  export function action(name: string): boolean {
    switch (name) {
      case 'next_page':
        turn(1);
        return true;
      case 'prev_page':
        turn(-1);
        return true;
      case 'zoom_in':
        setZoom(prefs.saved.pdf_zoom * ZOOM.step);
        return true;
      case 'zoom_out':
        setZoom(prefs.saved.pdf_zoom / ZOOM.step);
        return true;
      case 'zoom_reset':
        setZoom(1);
        return true;
      case 'zoom_fit_width':
        setZoom(widthZoom());
        return true;
      case 'zoom_enhance':
        onnotice('Pages are always drawn at the full sharpness of the screen here');
        return true;
      case 'pan_left':
        scrollEl.scrollLeft -= viewWidth / 8;
        return true;
      case 'pan_right':
        scrollEl.scrollLeft += viewWidth / 8;
        return true;
      case 'toggle_pdf_theming':
        onnotice(themed ? 'Pages in the document’s own colours' : 'Pages in the theme’s colours');
        void setLook({ themed: !themed });
        return true;
      case 'toggle_invert_images':
        onnotice(tintImages ? 'Pictures in their own colours' : 'Pictures in the theme’s colours');
        void setLook({ tint: !tintImages });
        return true;
      case 'toggle_pdf_link_highlight':
        linkHints = !linkHints;
        onnotice(linkHints ? 'Links outlined' : 'Links not outlined');
        return true;
      case 'toggle_pdf_page_layout':
        onnotice(spread ? 'One page across' : 'Two pages across');
        prefs.update({ pdf_spread: !spread });
        return true;
      case 'toggle_pdf_render_mode':
        onnotice(paged ? 'Continuous scrolling' : 'One page at a time');
        turned = rowOf(where().page);
        prefs.update({ pdf_paged: !paged });
        return true;
      case 'toggle_pdf_watching':
        api
          .pdfWatch(!watching)
          .then(() => {
            watching = !watching;
            onnotice(watching ? 'Watching the file: it reloads when it changes' : 'No longer watching the file');
          })
          .catch((e) => onnotice(String(e), true));
        return true;
      case 'synctex_inverse': {
        const at = selOf(vim?.cursorRange())?.from ?? chosen()?.to;
        const layer = at ? layers.get(at.page) : undefined;
        const line = at && layer?.lines[at.l];
        if (!at || !line) {
          onnotice('Put the cursor on some text first, then jump to its source');
          return true;
        }
        source({ page: at.page, x: line.xs[at.c] ?? line.x0, y: (line.y0 + line.y1) / 2 });
        return true;
      }
      default:
        return false;
    }
  }

  function source(spot: PdfSpot) {
    if (!opened.synctex) {
      onnotice('This document has no SyncTeX data beside it');
      return;
    }
    api
      .synctexInverse(spot)
      .then((message) => onnotice(message))
      .catch((e) => onnotice(String(e), true));
  }

  /** Offers a key to a half-typed cursor command (a count, `f`, `y`). */
  export function vimKey(token: string): boolean {
    return vim?.key(token) ?? false;
  }

  /** Runs a cursor command; false when the action is not one. */
  export function vimAction(name: string): boolean {
    if (!vim) return false;
    if (vim.mode !== 'reading') {
      if (name === 'go_top' || name === 'go_bottom') {
        const top = name === 'go_top';
        goTo(top ? 0 : pages.length - 1, top ? 0 : 0.999);
        // The cursor follows once the page it lands on has its text.
        void layerOf(top ? 0 : pages.length - 1).then(() =>
          setTimeout(() => vim?.toViewEdge(top ? 'top' : 'bottom'), 60),
        );
        return true;
      }
      if (name === 'follow_link') {
        const at = selOf(vim.cursorRange())?.from;
        const layer = at ? layers.get(at.page) : undefined;
        const line = at && layer?.lines[at.l];
        if (at && layer && line) follow(layer, line.xs[at.c] ?? line.x0, (line.y0 + line.y1) / 2);
        return true;
      }
    }
    const was = JSON.stringify(vim.cursorPosition());
    const handled = vim.action(name);
    // At the last line of a page shown on its own, down turns the page.
    const stuck = handled && paged && was === JSON.stringify(vim.cursorPosition());
    if (stuck && (name === 'move_down' || name === 'scroll_half_down' || name === 'scroll_page_down')) turn(1);
    if (stuck && (name === 'move_up' || name === 'scroll_half_up' || name === 'scroll_page_up')) turn(-1);
    return handled;
  }

  export function selectedText(): string | null {
    const sel = chosen();
    return sel ? textAcross(sel) || null : null;
  }

  export function clearSelection() {
    selection = null;
    if (vim?.mode === 'visual' || vim?.mode === 'visual-line') vim.action('cancel');
  }

  /** The selection to highlight or annotate: what of it lies on its first page. */
  export function picked(): { page: number; rects: Box[]; quote: string; box: DOMRect } | null {
    const sel = chosen();
    const layer = sel ? layers.get(sel.from.page) : undefined;
    if (!sel || !layer) return null;
    const rects = boxes(sel, layer);
    const box = rects[0] ? clientBox(layer.page, rects.at(-1)!) : null;
    if (!box) return null;
    return { page: layer.page, rects, quote: textOf(sel, layer), box };
  }

  /** The highlight or note under the selection, the cursor, or a point. */
  export function annotationAt(point?: { x: number; y: number }): Annotation | undefined {
    if (point) {
      const spot = spotAt(point.x, point.y);
      if (!spot) return undefined;
      return annotations.find(
        (item) =>
          item.chapter === spot.page &&
          item.rects?.some((r) => spot.x >= r[0] && spot.x <= r[2] && spot.y >= r[1] && spot.y <= r[3]),
      );
    }
    const sel = chosen() ?? selOf(vim?.cursorRange());
    const layer = sel ? layers.get(sel.from.page) : undefined;
    if (!sel || !layer) return undefined;
    const covered = boxes(sel, layer);
    return annotations.find(
      (item) =>
        item.chapter === layer.page &&
        item.rects?.some((own) => covered.some((other) => overlaps(own, other))),
    );
  }

  /** Where the selection or the cursor is in the window, to open a menu by. */
  export function selectionBox(): DOMRect | null {
    const sel = chosen() ?? selOf(vim?.cursorRange());
    const layer = sel ? layers.get(sel.to.page) : undefined;
    const last = sel && layer ? boxes(sel, layer).at(-1) : undefined;
    return last && layer ? clientBox(layer.page, last) : null;
  }

  /** Where a bookmark set now would go: at a point in the window if one is
      given, else at the cursor, else at the first line in view. */
  export function markSpot(point?: { x: number; y: number }) {
    const at = where();
    let caret = point ? caretAt(point.x, point.y) : (selOf(vim?.cursorRange())?.from ?? null);
    if (!caret) {
      const layer = layers.get(at.page);
      const l = layer?.lines.findIndex((line) => line.y0 >= at.offset - 0.005) ?? -1;
      if (l >= 0) caret = { page: at.page, l, c: 0 };
    }
    const layer = caret ? layers.get(caret.page) : undefined;
    const words = caret && layer
      ? layer.lines.slice(caret.l, caret.l + 2).map((line) => line.text.trim()).join(' ')
      : '';
    const letters = [...words];
    return {
      page: caret?.page ?? at.page,
      line: caret?.l ?? null,
      snippet: words ? (letters.length > 80 ? `${letters.slice(0, 80).join('')}…` : words) : null,
      offset: at.offset,
    };
  }

  /** Selects the word at a point in the window, as a right click on bare
      text does elsewhere, and returns it; null where there is none. */
  export function selectWordAt(x: number, y: number): string | null {
    const spot = spotAt(x, y);
    const caret = spot ? caretAt(x, y) : null;
    const layer = caret ? layers.get(caret.page) : undefined;
    const line = caret && layer?.lines[caret.l];
    if (!spot || !caret || !layer || !line) return null;
    const under = spot.y >= line.y0 && spot.y <= line.y1 && spot.x >= line.x0 && spot.x <= line.x1;
    if (!under) return null;
    const word = wordAt(layer, caret);
    const text = textOf(word, layer);
    if (!/[\p{L}\p{N}]/u.test(text)) return null;
    selection = word;
    return text;
  }

  // ── Find ────────────────────────────────────────────────────

  /** Runs the search over the pages that are loaded, keeping the place
      among the matches where it can. */
  function rescan(keep = true) {
    if (!finder) return;
    const current = keep ? found.sels[found.at] : undefined;
    finder.search(query);
    const sels = finder.all.map((range) => selOf(range)).filter((sel): sel is Sel => sel !== null);
    const same = (a: Caret, b: Caret) => a.page === b.page && a.l === b.l && a.c === b.c;
    const at = current ? sels.findIndex((sel) => same(sel.from, current.from)) : -1;
    found = { sels, at };
  }

  function showMatch(at: number) {
    found = { sels: found.sels, at };
    const sel = found.sels[at];
    const layer = sel ? layers.get(sel.from.page) : undefined;
    const box = sel && layer ? boxes(sel, layer)[0] : undefined;
    const client = sel && box ? clientBox(sel.from.page, box) : null;
    if (!client) return;
    const view = scrollEl.getBoundingClientRect();
    if (client.top < view.top + LINE || client.bottom > view.bottom - LINE) {
      scroller.to(scrollEl.scrollTop + client.top - view.top - viewHeight / 3);
    }
  }

  /** Waits until the page at the top of the view is on the page with its
      text, which it is not for a moment after a jump. */
  async function settled() {
    await tick();
    await layerOf(where().page);
    await tick();
    await new Promise((resolve) => requestAnimationFrame(resolve));
  }

  /** Finds text and goes to the first place at or after the top of the
      view, or the place nearest a given height on a page (`near`).
      Resolves to whether there is any in the document. */
  export async function find(text: string, near?: { page: number; y: number }): Promise<boolean> {
    query = text.trim();
    if (!query) {
      findClear();
      return false;
    }
    await settled();
    rescan(false);
    const top = scrollEl.getBoundingClientRect().top;
    const placed = found.sels.map((sel, index) => {
      const layer = layers.get(sel.from.page);
      return { sel, index, box: layer ? boxes(sel, layer)[0] : undefined };
    });
    if (near) {
      const onPage = placed
        .filter(({ sel, box }) => sel.from.page === near.page && box)
        .sort((a, b) => Math.abs(a.box![1] - near.y) - Math.abs(b.box![1] - near.y));
      if (onPage[0]) {
        showMatch(onPage[0].index);
        return true;
      }
    }
    const first = placed.findIndex(({ sel, box }) => {
      const client = box ? clientBox(sel.from.page, box) : null;
      return client !== null && client.bottom >= top;
    });
    if (first >= 0) {
      showMatch(first);
      return true;
    }
    return findStep(1);
  }

  /** Goes to the next or previous place the text occurs, in the pages
      that are loaded or, past them, the next page that has it. */
  export async function findStep(direction: 1 | -1): Promise<boolean> {
    if (!query) return false;
    const next = found.at + direction;
    if (found.at >= 0 && next >= 0 && next < found.sels.length) {
      showMatch(next);
      return true;
    }
    if (found.at < 0 && found.sels.length > 0 && direction === -1) {
      showMatch(found.sels.length - 1);
      return true;
    }
    try {
      if (queryPages?.query !== query) {
        queryPages = { query, pages: await api.pdfFindPages(query) };
      }
    } catch (e) {
      onnotice(String(e), true);
      return false;
    }
    const all = queryPages.pages;
    if (all.length === 0) return false;
    const here = found.sels[found.at]?.from.page ?? where().page;
    const loaded = new Set(found.sels.map((sel) => sel.from.page));
    // Onward from the page of the current match, round the ends, to the
    // first page whose matches are not among those already stepped through.
    const onward = direction === 1
      ? [...all.filter((page) => page > here), ...all.filter((page) => page <= here)]
      : [...all.filter((page) => page < here).reverse(), ...all.filter((page) => page >= here).reverse()];
    const target = onward.find((page) => !loaded.has(page)) ?? onward[0];
    if (target === undefined) return false;
    if (loaded.has(target)) {
      // Everything is loaded: wrap to the other end.
      showMatch(direction === 1 ? 0 : found.sels.length - 1);
      return true;
    }
    goTo(target);
    await layerOf(target);
    await tick();
    await new Promise((resolve) => requestAnimationFrame(resolve));
    rescan(false);
    const onPage = found.sels
      .map((sel, index) => ({ sel, index }))
      .filter(({ sel }) => sel.from.page === target);
    const landed = direction === 1 ? onPage[0] : onPage.at(-1);
    if (!landed) return false;
    showMatch(landed.index);
    return true;
  }

  export function findClear() {
    query = '';
    finder?.clear();
    found = { sels: [], at: -1 };
  }

  // ── The mouse ───────────────────────────────────────────────

  function follow(layer: PdfLayer, x: number, y: number): boolean {
    const link = layer.links.find((l) => x >= l.x0 && x <= l.x1 && y >= l.y0 && y <= l.y1);
    if (!link) return false;
    if (link.url) {
      onexternal(link.url);
    } else if (link.page !== undefined) {
      onleave();
      goTo(link.page);
    }
    return true;
  }

  function onpointerdown(event: PointerEvent) {
    if (event.button !== 0) return;
    press = { at: caretAt(event.clientX, event.clientY), x: event.clientX, y: event.clientY, moved: false };
    // A press that did not come from a real pointer has none to capture.
    try {
      scrollEl.setPointerCapture(event.pointerId);
    } catch {
      /* the drag still works while it stays inside the view */
    }
  }

  function onpointermove(event: PointerEvent) {
    if (!press) {
      const spot = spotAt(event.clientX, event.clientY);
      const layer = spot ? layers.get(spot.page) : undefined;
      const over = spot && layer?.links.some(
        (l) => spot.x >= l.x0 && spot.x <= l.x1 && spot.y >= l.y0 && spot.y <= l.y1,
      );
      scrollEl.style.cursor = over ? 'pointer' : '';
      return;
    }
    if (!press.moved && Math.hypot(event.clientX - press.x, event.clientY - press.y) < 4) return;
    press.moved = true;
    const to = caretAt(event.clientX, event.clientY);
    if (press.at && to) {
      if (vim?.mode === 'visual' || vim?.mode === 'visual-line') vim.action('cancel');
      selection = ordered(press.at, to);
    }
    // Dragging against the top or bottom edge brings more text into view.
    const view = scrollEl.getBoundingClientRect();
    if (event.clientY > view.bottom - 28) scroller.by(LINE);
    else if (event.clientY < view.top + 28) scroller.by(-LINE);
  }

  function onpointerup(event: PointerEvent) {
    const pressed = press;
    press = null;
    if (!pressed || pressed.moved || event.button !== 0) return;
    const spot = spotAt(event.clientX, event.clientY);
    const layer = spot ? layers.get(spot.page) : undefined;
    if (spot && (event.ctrlKey || event.metaKey)) {
      source(spot);
      return;
    }
    if (spot && layer && follow(layer, spot.x, spot.y)) return;
    const at = pressed.at;
    const text = at ? layers.get(at.page) : undefined;
    if (at && text && event.detail === 2) {
      selection = wordAt(text, at);
    } else if (at && text && event.detail >= 3) {
      selection = blockAt(text, at);
    } else {
      selection = null;
      const node = at ? charEl(at)?.firstChild : null;
      if (node instanceof Text) vim?.placeOn(node, 0);
    }
  }

  /** Every box to draw over a page: what is selected, found, highlighted. */
  function overlays(page: number) {
    const layer = layers.get(page);
    const out: { box: Box; kind: string; color?: string }[] = [];
    for (const item of annotations) {
      if (item.chapter !== page) continue;
      for (const box of item.rects ?? []) {
        out.push({ box, kind: item.color ? 'mark' : 'note', color: item.color ?? undefined });
      }
    }
    if (!layer) return out;
    found.sels.forEach((sel, index) => {
      for (const box of boxes(sel, layer)) out.push({ box, kind: index === found.at ? 'match here' : 'match' });
    });
    if (selection) for (const box of boxes(selection, layer)) out.push({ box, kind: 'picked' });
    return out;
  }

  function signs(page: number) {
    const layer = layers.get(page);
    const letters = marks
      .filter((mark) => mark.page === page)
      .map((mark) => ({
        text: mark.letter,
        top: (mark.line !== null ? layer?.lines[mark.line]?.y0 : undefined) ?? 0.01,
        note: false,
      }));
    const notes = annotations
      .filter((item) => item.chapter === page && item.color === null && item.rects?.[0])
      .map((item) => ({ text: '', top: item.rects![0]![1], note: true }));
    return [...letters, ...notes];
  }
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div
  class="scroll pdf"
  class:plain={!themed}
  bind:this={scrollEl}
  onscroll={report}
  {onpointerdown}
  {onpointermove}
  {onpointerup}
  onpointercancel={() => (press = null)}
>
  <div
    class="pages"
    bind:this={pagesEl}
    style:width="{layout.width}px"
    style:height="{layout.height}px"
  >
    {#each mounted as placed (placed.index)}
      <div
        class="page"
        data-page={placed.index}
        style:left="{placed.left}px"
        style:top="{placed.top}px"
        style:width="{placed.width}px"
        style:height="{placed.height}px"
      >
        {#if pictures.has(placed.index)}
          <img src={pictures.get(placed.index)} alt="Page {placed.index + 1}" draggable="false" />
        {/if}
        <div class="over" aria-hidden="true">
          {#each overlays(placed.index) as item}
            <span
              class={item.kind}
              style:left="{item.box[0] * 100}%"
              style:top="{item.box[1] * 100}%"
              style:width="{(item.box[2] - item.box[0]) * 100}%"
              style:height="{(item.box[3] - item.box[1]) * 100}%"
              style:--color={item.color ? `var(--mark-${item.color})` : undefined}
            ></span>
          {/each}
          {#if linkHints}
            {#each layers.get(placed.index)?.links ?? [] as link}
              <span
                class="link"
                style:left="{link.x0 * 100}%"
                style:top="{link.y0 * 100}%"
                style:width="{(link.x1 - link.x0) * 100}%"
                style:height="{(link.y1 - link.y0) * 100}%"
              ></span>
            {/each}
          {/if}
          {#if arrived?.page === placed.index}
            <span class="arrived" style:top="{arrived.y0 * 100}%" style:height="{(arrived.y1 - arrived.y0) * 100}%"></span>
          {/if}
        </div>
        {#each signs(placed.index) as sign}
          <span class="sign" class:note={sign.note} style:top="{sign.top * 100}%">{sign.text}</span>
        {/each}
        <div class="text" use:textLayer={layers.get(placed.index)}></div>
      </div>
    {/each}
  </div>
</div>

<style>
  .scroll {
    position: relative;
    height: 100%;
    overflow: auto;
    scrollbar-width: thin;
    scrollbar-color: var(--line) transparent;
    user-select: none;
    -webkit-user-select: none;
  }

  .pages {
    position: relative;
    line-height: 24px;
  }

  .page {
    position: absolute;
    background: var(--page);
    box-shadow:
      0 0 0 1px var(--hairline),
      0 8px 28px var(--shadow);
  }

  .plain .page {
    background: #fff;
  }

  img {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    display: block;
    pointer-events: none;
  }

  .over,
  .text {
    position: absolute;
    inset: 0;
  }

  .over {
    pointer-events: none;
  }

  .over span {
    position: absolute;
  }

  .over .mark {
    background: color-mix(in srgb, var(--color) 36%, transparent);
    border-radius: 2px;
  }

  .over .note {
    background: color-mix(in srgb, var(--note) 16%, transparent);
    border-bottom: 1.5px solid var(--note);
  }

  .over .match {
    background: color-mix(in srgb, var(--accent) 34%, transparent);
    border-radius: 2px;
  }

  .over .match.here {
    background: color-mix(in srgb, var(--accent) 62%, transparent);
    outline: 1.5px solid var(--accent);
  }

  .over .picked {
    background: color-mix(in srgb, var(--link) 34%, transparent);
    border-radius: 2px;
  }

  .over .link {
    outline: 1px solid var(--accent);
    border-radius: 2px;
  }

  .over .arrived {
    left: 0;
    right: 0;
    background: color-mix(in srgb, var(--accent) 30%, transparent);
    animation: fade 1600ms ease-out forwards;
  }

  @keyframes fade {
    to {
      background: transparent;
    }
  }

  .sign {
    position: absolute;
    left: -20px;
    font: 500 11px / 1.4 var(--font-mono);
    color: var(--accent);
    pointer-events: none;
  }

  .sign.note {
    left: -15px;
    width: 6px;
    height: 6px;
    margin-top: 4px;
    border-radius: 50%;
    background: var(--note);
  }

  /* The page's text, lying unseen over its picture for the cursor to
     move on: a paragraph per block, a row per line, a box per character. */
  .text {
    color: transparent;
    font-size: 1px;
    line-height: 1;
    pointer-events: none;
  }

  .text :global(p) {
    margin: 0;
  }

  .text :global(br) {
    display: none;
  }

  .text :global(.line) {
    position: absolute;
    display: flex;
    white-space: pre;
  }

  .text :global(.ch) {
    flex: none;
    height: 100%;
    overflow: hidden;
  }
</style>
