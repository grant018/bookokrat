<script lang="ts" module>
  export type MenuItem =
    | 'separator'
    | {
        /** A row of colours to pick from, each with the key that picks it. */
        label: string;
        swatches: { color: string; name: string; key: string; run: () => void }[];
      }
    | {
        label: string;
        /** The keys that do the same thing, shown beside the item. */
        keys?: string[];
        disabled?: boolean;
        run: () => void;
      };
</script>

<script lang="ts">
  import { keyLabel } from './keys';

  let {
    x,
    y,
    items,
    onclose,
  }: { x: number; y: number; items: MenuItem[]; onclose: () => void } = $props();

  const EDGE = 8;

  let menuEl: HTMLElement;
  let left = $state(0);
  let top = $state(0);
  let cursor = $state(-1);

  // Opened near the right or bottom edge, the menu moves back inside the window.
  $effect(() => {
    const box = menuEl.getBoundingClientRect();
    left = Math.max(EDGE, Math.min(x, innerWidth - box.width - EDGE));
    top = Math.max(EDGE, Math.min(y, innerHeight - box.height - EDGE));
  });

  function usable(index: number): boolean {
    const item = items[index];
    return item !== undefined && item !== 'separator' && !('swatches' in item) && !item.disabled;
  }

  function pick(run: () => void) {
    onclose();
    run();
  }

  function move(direction: 1 | -1) {
    for (let step = 1; step <= items.length; step++) {
      const next = (cursor + direction * step + items.length * step) % items.length;
      if (usable(next)) {
        cursor = next;
        return;
      }
    }
  }

  function choose(index: number) {
    const item = items[index];
    if (!usable(index) || item === 'separator' || !item || 'swatches' in item) return;
    pick(item.run);
  }

  export function handleKey(token: string) {
    if (token === 'Esc') onclose();
    else if (token === 'Down' || token === 'j') move(1);
    else if (token === 'Up' || token === 'k') move(-1);
    else if (token === 'Enter') choose(cursor);
    else {
      for (const item of items) {
        if (item === 'separator' || !('swatches' in item)) continue;
        const swatch = item.swatches.find((s) => s.key === token);
        if (swatch) pick(swatch.run);
      }
    }
  }
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div
  class="away"
  onpointerdown={onclose}
  oncontextmenu={(event) => {
    event.preventDefault();
    event.stopPropagation();
    onclose();
  }}
></div>
<div class="menu" role="menu" bind:this={menuEl} style:left="{left}px" style:top="{top}px">
  {#each items as item, index}
    {#if item === 'separator'}
      <div class="rule" role="separator"></div>
    {:else if 'swatches' in item}
      <div class="swatches">
        <span class="label">{item.label}</span>
        {#each item.swatches as swatch}
          <button
            class="swatch"
            role="menuitem"
            aria-label="{item.label} {swatch.name}"
            title="{swatch.name} ({swatch.key})"
            onclick={() => pick(swatch.run)}
          >
            <span style:background={swatch.color}></span>
          </button>
        {/each}
      </div>
    {:else}
      <button
        role="menuitem"
        class:cursor={index === cursor}
        disabled={item.disabled}
        onpointerenter={() => (cursor = usable(index) ? index : -1)}
        onclick={() => choose(index)}
      >
        <span class="label">{item.label}</span>
        {#if item.keys}
          <span class="keys">{#each item.keys as key}<kbd>{keyLabel(key)}</kbd>{/each}</span>
        {/if}
      </button>
    {/if}
  {/each}
</div>

<style>
  .away {
    position: fixed;
    inset: 0;
    z-index: 20;
  }

  .menu {
    position: fixed;
    z-index: 21;
    min-width: 236px;
    max-width: 340px;
    padding: 6px;
    display: flex;
    flex-direction: column;
    border-radius: 10px;
    border: 1px solid var(--line);
    background: var(--raised);
    box-shadow: 0 12px 32px var(--shadow);
  }

  button {
    display: flex;
    align-items: center;
    gap: 24px;
    min-height: 30px;
    padding: 0 10px;
    border-radius: 6px;
    text-align: left;
    cursor: default;
  }

  button.cursor {
    background: color-mix(in srgb, var(--text) 12%, var(--raised));
    color: var(--heading);
  }

  button:disabled {
    color: var(--muted);
    opacity: 0.6;
  }

  .swatches {
    display: flex;
    align-items: center;
    gap: 2px;
    min-height: 30px;
    padding: 0 4px 0 10px;
  }

  .swatch {
    flex: none;
    min-height: 0;
    width: 26px;
    height: 26px;
    padding: 0;
    gap: 0;
    justify-content: center;
    border-radius: 50%;
  }

  .swatch:hover {
    background: color-mix(in srgb, var(--text) 12%, var(--raised));
  }

  .swatch span {
    width: 14px;
    height: 14px;
    border-radius: 50%;
  }

  .label {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .keys {
    flex: none;
    display: flex;
    gap: 4px;
  }

  kbd {
    background: var(--chrome);
  }

  .rule {
    height: 1px;
    margin: 5px 4px;
    background: var(--line);
  }
</style>
