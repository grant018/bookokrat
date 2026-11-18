<script lang="ts">
  let {
    src,
    alt,
    onclose,
  }: {
    src: string;
    alt: string;
    onclose: () => void;
  } = $props();

  const ZOOM = { min: 1, max: 8, step: 1.25 };
  const PAN_STEP = 80;

  let frameEl: HTMLElement;
  /** 1 fits the whole picture in the window. */
  let zoom = $state(1);
  let x = $state(0);
  let y = $state(0);
  let natural = $state<{ width: number; height: number } | null>(null);
  let drag: { x: number; y: number; startX: number; startY: number } | null = null;

  /** The picture's size on screen at zoom 1: as large as fits in the
      window, a small one enlarged up to three times. */
  let fitted = $derived.by(() => {
    if (!natural || !frameEl) return null;
    const scale = Math.min(
      3,
      (frameEl.clientWidth - 96) / natural.width,
      (frameEl.clientHeight - 96) / natural.height,
    );
    return { width: natural.width * scale, height: natural.height * scale };
  });

  function clampPan() {
    if (!fitted) return;
    const overX = Math.max(0, (fitted.width * zoom - frameEl.clientWidth) / 2 + 48);
    const overY = Math.max(0, (fitted.height * zoom - frameEl.clientHeight) / 2 + 48);
    x = Math.max(-overX, Math.min(overX, x));
    y = Math.max(-overY, Math.min(overY, y));
  }

  function setZoom(next: number) {
    zoom = Math.max(ZOOM.min, Math.min(ZOOM.max, next));
    if (zoom === 1) x = y = 0;
    clampPan();
  }

  function pan(dx: number, dy: number) {
    x += dx;
    y += dy;
    clampPan();
  }

  export function handleKey(token: string) {
    if (token === 'Esc' || token === 'q' || token === 'Enter') onclose();
    else if (token === '=' || token === '+') setZoom(zoom * ZOOM.step);
    else if (token === '-' || token === '_') setZoom(zoom / ZOOM.step);
    else if (token === '0' || token === 'z') setZoom(1);
    else if (token === 'h' || token === 'Left') pan(PAN_STEP, 0);
    else if (token === 'l' || token === 'Right') pan(-PAN_STEP, 0);
    else if (token === 'k' || token === 'Up') pan(0, PAN_STEP);
    else if (token === 'j' || token === 'Down') pan(0, -PAN_STEP);
  }

  function onwheel(event: WheelEvent) {
    event.preventDefault();
    setZoom(zoom * (event.deltaY < 0 ? 1.12 : 1 / 1.12));
  }

  function onpointerdown(event: PointerEvent) {
    if (event.button !== 0) return;
    if (event.target === frameEl) {
      onclose();
      return;
    }
    drag = { x, y, startX: event.clientX, startY: event.clientY };
    frameEl.setPointerCapture(event.pointerId);
  }

  function onpointermove(event: PointerEvent) {
    if (!drag) return;
    x = drag.x + event.clientX - drag.startX;
    y = drag.y + event.clientY - drag.startY;
    clampPan();
  }
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div
  class="frame"
  bind:this={frameEl}
  {onwheel}
  {onpointerdown}
  {onpointermove}
  onpointerup={() => (drag = null)}
>
  <img
    {src}
    {alt}
    draggable="false"
    class:ready={fitted !== null}
    style:width={fitted ? `${fitted.width}px` : undefined}
    style:height={fitted ? `${fitted.height}px` : undefined}
    style:transform="translate({x}px, {y}px) scale({zoom})"
    onload={(event) => {
      const image = event.currentTarget as HTMLImageElement;
      natural = { width: image.naturalWidth, height: image.naturalHeight };
    }}
  />
  <div class="hints">
    {#if alt}<span class="alt">{alt}</span>{/if}
    <span><kbd>+</kbd><kbd>-</kbd> zoom</span>
    <span><kbd>h</kbd><kbd>j</kbd><kbd>k</kbd><kbd>l</kbd> move</span>
    <span><kbd>Esc</kbd> close</span>
  </div>
</div>

<style>
  .frame {
    position: absolute;
    inset: 0;
    z-index: 15;
    display: flex;
    align-items: center;
    justify-content: center;
    overflow: hidden;
    background: color-mix(in srgb, var(--chrome) 94%, transparent);
    cursor: zoom-out;
  }

  img {
    flex: none;
    max-width: none;
    border-radius: 4px;
    box-shadow: 0 16px 48px var(--shadow);
    opacity: 0;
    cursor: grab;
    user-select: none;
  }

  img.ready {
    opacity: 1;
  }

  .hints {
    position: absolute;
    inset: auto 0 0;
    height: 36px;
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 18px;
    font-size: 11.5px;
    color: var(--muted);
    pointer-events: none;
  }

  .hints span {
    display: flex;
    align-items: center;
    gap: 6px;
  }

  .alt {
    max-width: 40%;
    font-family: var(--font-reading);
    font-style: italic;
    font-size: 13px;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
</style>
