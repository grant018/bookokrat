<script lang="ts">
  import { tick } from 'svelte';
  import { api, type Chapter } from './api';
  import Blocks from './Blocks.svelte';
  import { FONTS, fontStack } from './fonts';
  import { prefs } from './prefs.svelte';
  import type { Snippet } from 'svelte';
  import type { Theme } from './themes';

  type Pane = 'fonts' | 'themes' | 'text';
  const PANES: Pane[] = ['fonts', 'themes', 'text'];
  const THEME_COLUMNS = 6;
  const TEXT_ROWS = ['font_size', 'line_spacing', 'measure', 'justify', 'drop_caps'] as const;
  const INSTALLED_ROW = FONTS.length;

  let {
    chapter,
    title,
    status,
    onclose,
  }: {
    chapter: Chapter | null;
    /** The open book's title, for the preview page's running head. */
    title: string | null;
    /** The window's status line, drawn under the preview page. */
    status: Snippet;
    onclose: () => void;
  } = $props();

  let pane = $state<Pane>('fonts');
  const savedFont = FONTS.findIndex((f) => f.name === prefs.saved.font);
  let fontCursor = $state(savedFont >= 0 ? savedFont : INSTALLED_ROW);
  let themeCursor = $state(Math.max(0, prefs.themes.indexOf(prefs.theme)));
  let textCursor = $state(0);

  let installed = $state<string[] | null>(null);
  let choosingInstalled = $state(false);
  let filter = $state('');
  let installedCursor = $state(0);
  let filterEl = $state<HTMLInputElement>();
  let installedListEl = $state<HTMLElement>();

  let groups = $derived(
    [
      { label: 'Dark', themes: prefs.themes.filter((t) => !t.custom && !t.light) },
      { label: 'Light', themes: prefs.themes.filter((t) => !t.custom && t.light) },
      { label: 'Yours', themes: prefs.themes.filter((t) => t.custom) },
    ].filter((g) => g.themes.length > 0),
  );

  // Rows of theme indices as laid out on screen, so j/k move straight up and down.
  let themeRows = $derived.by(() => {
    const rows: number[][] = [];
    for (const group of groups) {
      for (let i = 0; i < group.themes.length; i += THEME_COLUMNS) {
        rows.push(
          group.themes.slice(i, i + THEME_COLUMNS).map((theme) => prefs.themes.indexOf(theme)),
        );
      }
    }
    return rows;
  });

  let matches = $derived.by(() => {
    const needle = filter.trim().toLowerCase();
    const all = installed ?? [];
    return (needle ? all.filter((name) => name.toLowerCase().includes(needle)) : all).slice(0, 300);
  });

  // Keeps the keyboard cursor on screen when a list is taller than the window.
  let rootEl = $state<HTMLElement>();
  $effect(() => {
    void [pane, fontCursor, themeCursor, textCursor];
    void tick().then(() => rootEl?.querySelector('.cursor')?.scrollIntoView({ block: 'nearest' }));
  });

  let savedFontIsInstalled = $derived(!FONTS.some((f) => f.name === prefs.saved.font));

  function previewFontAt(index: number) {
    fontCursor = Math.max(0, Math.min(INSTALLED_ROW, index));
    prefs.previewFont = FONTS[fontCursor]?.name ?? null;
  }

  function previewThemeAt(index: number) {
    themeCursor = index;
    prefs.previewTheme = prefs.themes[index]?.name ?? null;
  }

  function moveTheme(rowDelta: number, columnDelta: number) {
    let row = themeRows.findIndex((r) => r.includes(themeCursor));
    let column = themeRows[row]?.indexOf(themeCursor) ?? 0;
    if (row < 0) row = 0;
    if (columnDelta !== 0) {
      const flat = themeRows.flat();
      const next = flat[flat.indexOf(themeCursor) + columnDelta];
      if (next !== undefined) previewThemeAt(next);
      return;
    }
    const target = themeRows[Math.max(0, Math.min(themeRows.length - 1, row + rowDelta))];
    if (target) previewThemeAt(target[Math.min(column, target.length - 1)]!);
  }

  function adjust(direction: 1 | -1) {
    const row = TEXT_ROWS[textCursor];
    if (row === 'justify') prefs.update({ justify: !prefs.saved.justify });
    else if (row === 'drop_caps') prefs.update({ drop_caps: !prefs.saved.drop_caps });
    // Wider margins mean a narrower column of text.
    else if (row === 'measure') prefs.step('measure', direction === 1 ? -1 : 1);
    else if (row) prefs.step(row, direction);
  }

  async function chooseInstalled() {
    choosingInstalled = true;
    filter = '';
    installedCursor = 0;
    installed ??= await api.systemFonts();
    await tick();
    filterEl?.focus();
  }

  function leaveInstalled() {
    choosingInstalled = false;
    prefs.previewFont = null;
  }

  function moveInstalled(delta: number) {
    installedCursor = Math.max(0, Math.min(matches.length - 1, installedCursor + delta));
    prefs.previewFont = matches[installedCursor] ?? null;
    void tick().then(() =>
      installedListEl?.children[installedCursor]?.scrollIntoView({ block: 'nearest' }),
    );
  }

  function commitInstalled(name: string | undefined) {
    if (!name) return;
    prefs.previewFont = name;
    prefs.commitPreview();
    choosingInstalled = false;
  }

  function onFilterKey(event: KeyboardEvent) {
    if (event.key === 'ArrowDown') moveInstalled(1);
    else if (event.key === 'ArrowUp') moveInstalled(-1);
    else if (event.key === 'Enter') commitInstalled(matches[installedCursor]);
    else if (event.key === 'Escape') leaveInstalled();
    else return;
    event.preventDefault();
  }

  /** Handles a keymap action; returns false if it means nothing here. */
  export function handle(action: string): boolean {
    const vertical = action === 'move_down' ? 1 : action === 'move_up' ? -1 : 0;
    const horizontal = action === 'move_right' ? 1 : action === 'move_left' ? -1 : 0;
    switch (action) {
      case 'next_tab':
      case 'prev_tab': {
        const step = action === 'next_tab' ? 1 : PANES.length - 1;
        pane = PANES[(PANES.indexOf(pane) + step) % PANES.length]!;
        return true;
      }
      case 'move_down':
      case 'move_up':
        if (pane === 'fonts') previewFontAt(fontCursor + vertical);
        else if (pane === 'themes') moveTheme(vertical, 0);
        else textCursor = Math.max(0, Math.min(TEXT_ROWS.length - 1, textCursor + vertical));
        return true;
      case 'move_left':
      case 'move_right':
        if (pane === 'themes') moveTheme(0, horizontal);
        else if (pane === 'text') adjust(horizontal as 1 | -1);
        return true;
      case 'go_top':
      case 'go_bottom': {
        const top = action === 'go_top';
        if (pane === 'fonts') previewFontAt(top ? 0 : INSTALLED_ROW);
        else if (pane === 'themes') previewThemeAt(top ? themeRows.flat()[0]! : themeRows.flat().at(-1)!);
        else textCursor = top ? 0 : TEXT_ROWS.length - 1;
        return true;
      }
      case 'select':
        if (pane === 'fonts' && fontCursor === INSTALLED_ROW) void chooseInstalled();
        else if (pane === 'text') {
          const row = TEXT_ROWS[textCursor];
          if (row === 'justify' || row === 'drop_caps') adjust(1);
        } else prefs.commitPreview();
        return true;
      case 'cancel':
        prefs.discardPreview();
        onclose();
        return true;
      default:
        return false;
    }
  }

  /** The theme whose name a group shows: the one being tried, else the one in use. */
  function named(themes: Theme[]): string {
    return themes.includes(prefs.theme) ? prefs.theme.name : '';
  }
</script>

{#snippet toggle(label: string, on: boolean, flip: () => void)}
  <button class="switch" role="switch" aria-checked={on} aria-label={label} onclick={flip}>
    <span aria-hidden="true"></span>
  </button>
{/snippet}

<div class="appearance" bind:this={rootEl}>
  <section class="fonts" class:focused={pane === 'fonts'} aria-label="Typeface">
    <div class="heading">
      <button class="back" aria-label="Back to the book" onclick={() => handle('cancel')}>
        <svg aria-hidden="true" width="16" height="16" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round"><path d="M9.5 4L5.5 8l4 4"></path></svg>
      </button>
      <div>
        <div class="screen">Appearance</div>
        <div class="scope">Applies to every book</div>
      </div>
    </div>

    {#if choosingInstalled}
      <div class="label">Installed fonts</div>
      <input
        bind:this={filterEl}
        bind:value={filter}
        oninput={() => {
          installedCursor = 0;
        }}
        onkeydown={onFilterKey}
        placeholder="Type to filter, ↑ ↓ to choose"
        aria-label="Filter installed fonts"
        spellcheck="false"
      />
      <div class="installed" bind:this={installedListEl}>
        {#each matches as name, index (name)}
          <button
            class="row compact"
            class:cursor={index === installedCursor}
            class:current={name === prefs.saved.font}
            style:font-family={fontStack(name)}
            onclick={() => commitInstalled(name)}
          >
            {name}
          </button>
        {/each}
        {#if installed && matches.length === 0}<p class="hint">No font matches.</p>{/if}
        {#if !installed}<p class="hint">Looking for fonts…</p>{/if}
      </div>
    {:else}
      {#each ['serif', 'sans'] as kind}
        <div class="label">{kind === 'serif' ? 'Serif' : 'Sans'}</div>
        {#each FONTS as font, index}
          {#if font.kind === kind}
            {@const onCursor = pane === 'fonts' && index === fontCursor}
            <button
              class="row"
              class:cursor={onCursor}
              class:current={font.name === prefs.saved.font}
              onclick={() => {
                pane = 'fonts';
                previewFontAt(index);
                prefs.commitPreview();
              }}
            >
              <span class="name" style:font-family={fontStack(font.name)}>{font.name}</span>
              {#if onCursor}<span class="note">{font.note}</span>{/if}
            </button>
          {/if}
        {/each}
      {/each}
      <div class="label">Yours</div>
      <button
        class="row"
        class:cursor={pane === 'fonts' && fontCursor === INSTALLED_ROW}
        class:current={savedFontIsInstalled}
        onclick={() => {
          pane = 'fonts';
          fontCursor = INSTALLED_ROW;
          void chooseInstalled();
        }}
      >
        <span class="name plain">{savedFontIsInstalled ? prefs.saved.font : 'Any installed font…'}</span>
      </button>
    {/if}
  </section>

  <div class="stage">
    <section class="preview" aria-label="Preview">
      <div class="running">{title ?? 'Preview'}</div>
      <div class="page">
        <article
          class="prose"
          class:justify={prefs.saved.justify}
          class:dropcaps={prefs.saved.drop_caps}
        >
          {#if chapter}
            <Blocks nodes={chapter.blocks.slice(0, 12)} opening />
          {:else}
            <h1>Loomings</h1>
            <p class="opening">
              <span class="lead">Call me Ishmael.</span> Some years ago—never mind how long precisely—having
              little or no money in my purse, and nothing particular to interest me on shore, I thought
              I would sail about a little and see the watery part of the world. It is a way I have of
              driving off the spleen and regulating the circulation.
            </p>
            <p>
              Whenever I find myself growing grim about the mouth; whenever it is a damp, drizzly
              November in my soul; then, I account it high time to get to sea as soon as I can.
              <em>This is my substitute for pistol and ball.</em>
            </p>
          {/if}
        </article>
      </div>
    </section>
    {@render status()}
  </div>

  <section class="side" aria-label="Theme and text">
    <div class="themes" class:focused={pane === 'themes'}>
      {#each groups as group}
        <div class="label split">
          <span>{group.label}</span>
          <span class="named">{named(group.themes)}</span>
        </div>
        <div class="grid">
          {#each group.themes as theme}
            {@const index = prefs.themes.indexOf(theme)}
            <button
              class="theme"
              class:cursor={pane === 'themes' && index === themeCursor}
              class:current={theme === prefs.theme}
              aria-label={theme.name}
              title={theme.name}
              style:background={theme.colors[0]}
              style:color={theme.colors[5]}
              onclick={() => {
                pane = 'themes';
                previewThemeAt(index);
                prefs.commitPreview();
              }}
            >
              Aa
            </button>
          {/each}
        </div>
      {/each}
    </div>

    <div class="text" class:focused={pane === 'text'}>
      <div class="label">Text</div>
      <div class="control" class:cursor={pane === 'text' && textCursor === 0}>
        <span class="what">Size</span>
        <button class="step" aria-label="Smaller text" onclick={() => prefs.step('font_size', -1)}>−</button>
        <span class="value">{prefs.saved.font_size} px</span>
        <button class="step" aria-label="Larger text" onclick={() => prefs.step('font_size', 1)}>+</button>
      </div>
      <div class="control" class:cursor={pane === 'text' && textCursor === 1}>
        <span class="what">Line spacing</span>
        <button class="step" aria-label="Tighter lines" onclick={() => prefs.step('line_spacing', -1)}>−</button>
        <span class="value">{prefs.saved.line_spacing.toFixed(2)}</span>
        <button class="step" aria-label="Looser lines" onclick={() => prefs.step('line_spacing', 1)}>+</button>
      </div>
      <div class="control" class:cursor={pane === 'text' && textCursor === 2}>
        <span class="what">Margins</span>
        <button class="step" aria-label="Narrower margins" onclick={() => prefs.step('measure', 1)}>−</button>
        <span class="value">{prefs.saved.measure} ch</span>
        <button class="step" aria-label="Wider margins" onclick={() => prefs.step('measure', -1)}>+</button>
      </div>
      <div class="control" class:cursor={pane === 'text' && textCursor === 3}>
        <span class="what">Justify text</span>
        {@render toggle('Justify text', prefs.saved.justify, () =>
          prefs.update({ justify: !prefs.saved.justify }),
        )}
      </div>
      <div class="control" class:cursor={pane === 'text' && textCursor === 4}>
        <span class="what">Drop caps</span>
        {@render toggle('Drop caps', prefs.saved.drop_caps, () =>
          prefs.update({ drop_caps: !prefs.saved.drop_caps }),
        )}
      </div>
    </div>
  </section>
</div>

<style>
  .appearance {
    flex: 1;
    min-width: 0;
    display: flex;
  }

  section {
    min-height: 0;
    overflow-y: auto;
    scrollbar-width: none;
  }

  .fonts {
    flex: 0 0 300px;
    padding: 16px 10px 16px 16px;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .heading {
    flex: none;
    display: flex;
    align-items: center;
    gap: 6px;
    padding-bottom: 12px;
  }

  .back {
    flex: none;
    width: 32px;
    height: 32px;
    display: flex;
    align-items: center;
    justify-content: center;
    border-radius: 7px;
    color: var(--muted);
  }

  .back:hover {
    background: var(--soft);
    color: var(--text);
  }

  .screen {
    font-family: var(--font-reading);
    font-size-adjust: var(--letter-height, none);
    font-size: 19px;
    font-weight: 500;
    line-height: 1.2;
    color: var(--heading);
  }

  .scope {
    margin-top: 3px;
    font-size: 12px;
    color: var(--muted);
  }

  .stage {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
  }

  .preview {
    flex: 1;
    margin-top: 8px;
    display: flex;
    flex-direction: column;
    overflow: hidden;
    border-radius: 12px;
    background: var(--page);
    box-shadow:
      0 0 0 1px var(--hairline),
      0 16px 48px var(--shadow);
  }

  .running {
    flex: none;
    height: 56px;
    padding: 0 40px;
    display: flex;
    align-items: center;
    justify-content: center;
    font-family: var(--font-reading);
    font-size-adjust: var(--letter-height, none);
    font-size: 13.5px;
    font-variant-caps: all-small-caps;
    letter-spacing: 0.14em;
    color: var(--muted);
    white-space: nowrap;
    overflow: hidden;
  }

  .page {
    flex: 1;
    min-height: 0;
    overflow: hidden;
  }

  .page .prose {
    padding-bottom: 0;
  }

  .side {
    flex: 0 0 348px;
    padding: 8px 28px 20px;
  }

  .label {
    flex: none;
    padding: 14px 12px 6px;
    font-family: var(--font-reading);
    font-size-adjust: var(--letter-height, none);
    font-size: 13px;
    line-height: 18px;
    font-variant-caps: all-small-caps;
    letter-spacing: 0.14em;
    color: var(--muted);
  }

  .side .label {
    padding: 16px 0 10px;
  }

  .label.split {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: 12px;
  }

  .named {
    font-family: var(--font-ui);
    font-size-adjust: none;
    font-size: 12.5px;
    font-variant-caps: normal;
    letter-spacing: 0;
    color: var(--text);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .row {
    flex: none;
    display: flex;
    flex-direction: column;
    justify-content: center;
    gap: 4px;
    min-height: 40px;
    padding: 8px 12px;
    text-align: left;
    border-radius: 7px;
    outline: 1px solid transparent;
    outline-offset: -1px;
  }

  .row.compact {
    min-height: 34px;
    padding: 4px 12px;
    font-size: 16px;
    color: var(--quiet);
  }

  .row:hover .name,
  .row.compact:hover {
    color: var(--heading);
  }

  .row.current {
    background: var(--soft);
  }

  .row.cursor {
    outline-color: var(--ring);
  }

  .name {
    font-size: 18px;
    font-size-adjust: var(--letter-height, none);
    line-height: 1.2;
    color: var(--quiet);
  }

  .row.current .name,
  .row.cursor .name,
  .row.compact.current,
  .row.compact.cursor {
    color: var(--heading);
  }

  .name.plain {
    font-size: 14px;
    font-size-adjust: none;
  }

  .note {
    font-size: 11.5px;
    color: var(--muted);
  }

  input {
    flex: none;
    margin: 0 4px 8px;
    height: 36px;
    padding: 0 10px;
    border-radius: 7px;
    border: 1px solid var(--line);
    background: var(--page);
    color: var(--text);
    font: inherit;
    outline: none;
  }

  input:focus {
    border-color: var(--accent);
  }

  .installed {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .hint {
    margin: 8px 12px;
    color: var(--muted);
  }

  .grid {
    display: grid;
    grid-template-columns: repeat(6, minmax(0, 1fr));
    gap: 10px;
    padding: 2px 0 10px;
  }

  .theme {
    aspect-ratio: 1;
    display: flex;
    align-items: center;
    justify-content: center;
    border-radius: 9px;
    box-shadow: inset 0 0 0 1px color-mix(in srgb, currentColor 14%, transparent);
    font-family: var(--font-reading);
    font-size: 15px;
    font-weight: 600;
  }

  .theme.current {
    box-shadow:
      inset 0 0 0 1px color-mix(in srgb, currentColor 14%, transparent),
      0 0 0 2px var(--chrome),
      0 0 0 3.5px var(--muted);
  }

  .theme.cursor {
    box-shadow:
      inset 0 0 0 1px color-mix(in srgb, currentColor 14%, transparent),
      0 0 0 2px var(--chrome),
      0 0 0 3.5px var(--accent);
  }

  .control {
    display: flex;
    align-items: center;
    gap: 4px;
    min-height: 46px;
    margin: 0 -8px;
    padding: 0 8px;
    border-top: 1px solid var(--hairline);
    outline: 1px solid transparent;
    outline-offset: -1px;
  }

  .control.cursor {
    border-radius: 7px;
    outline-color: var(--ring);
  }

  .what {
    flex: 1;
  }

  .step {
    width: 28px;
    height: 28px;
    display: flex;
    align-items: center;
    justify-content: center;
    border-radius: 7px;
    font-size: 15px;
    color: var(--muted);
  }

  .step:hover {
    background: var(--soft);
    color: var(--heading);
  }

  .value {
    width: 58px;
    text-align: center;
    font-family: var(--font-mono);
    font-size: 11.5px;
  }

  .switch {
    position: relative;
    width: 34px;
    height: 20px;
    margin-right: 4px;
    border-radius: 10px;
    background: var(--raised);
    transition: background 120ms ease;
  }

  .switch span {
    position: absolute;
    top: 3px;
    left: 3px;
    width: 14px;
    height: 14px;
    border-radius: 50%;
    background: var(--muted);
    transition:
      transform 120ms ease,
      background 120ms ease;
  }

  .switch[aria-checked='true'] {
    background: var(--accent);
  }

  .switch[aria-checked='true'] span {
    transform: translateX(14px);
    background: var(--page);
  }
</style>
