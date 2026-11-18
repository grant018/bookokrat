<script lang="ts" module>
  // Ids must stay unique across panels: a late answer to an earlier lookup is
  // recognised by its id and dropped.
  let lastId = Date.now();
</script>

<script lang="ts">
  import { onDestroy, onMount } from 'svelte';
  import { api, type LookupTarget } from './api';
  import { prefs } from './prefs.svelte';
  import { SmoothScroller } from './scroller';

  let {
    query,
    targets,
    focused,
    onclose,
    onflash,
  }: {
    query: string;
    /** Named targets from config.yaml; empty means the single lookup_command runs. */
    targets: LookupTarget[];
    focused: boolean;
    onclose: () => void;
    onflash: (text: string, error?: boolean) => void;
  } = $props();

  type Run = {
    id: number;
    label: string;
    state: 'running' | 'done' | 'failed';
    text: string;
  };

  /** `j`, `k` and `q` drive the panel, so a target starting with one of them
      gets no hotkey, as in the terminal app's picker. */
  const RESERVED = ['j', 'k', 'q'];

  let shown = $derived(targets.filter((t) => t.display === 'popup'));
  let outside = $derived(targets.filter((t) => t.display === 'fire_and_forget'));
  let ordered = $derived([...shown, ...outside]);
  let hotkeys = $derived.by(() => {
    const taken = new Set<string>();
    const keys = new Map<string, string>();
    for (const target of targets) {
      const key = [...target.name][0]?.toLowerCase();
      if (key && !RESERVED.includes(key) && !taken.has(key)) {
        taken.add(key);
        keys.set(target.name, key);
      }
    }
    return keys;
  });

  let selected = $state(0);
  let run = $state<Run | null>(null);
  let seconds = $state(0);
  let resultEl = $state<HTMLElement>();
  let scroller: SmoothScroller | undefined;

  $effect(() => {
    if (!resultEl) return;
    scroller = new SmoothScroller(resultEl);
    return () => scroller?.stop();
  });

  $effect(() => {
    if (run?.state !== 'running') return;
    const started = performance.now();
    seconds = 0;
    const timer = setInterval(() => (seconds = Math.floor((performance.now() - started) / 1000)), 250);
    return () => clearInterval(timer);
  });

  function cancelRunning() {
    if (run?.state === 'running') void api.lookupCancel(run.id);
  }

  async function start(target: LookupTarget | null) {
    const id = ++lastId;
    const inPanel = !target || target.display === 'popup';
    if (inPanel) {
      cancelRunning();
      run = { id, label: target?.name ?? 'Lookup', state: 'running', text: '' };
      scroller?.jump(0);
    }
    try {
      const outcome = await api.lookupRun(id, target?.name ?? null, query);
      if (outcome.kind === 'launched') {
        onflash(`Launched ${target?.name ?? 'the lookup command'}`);
        if (!run) onclose();
      } else if (outcome.kind === 'output' && run?.id === id) {
        run = { ...run, state: outcome.ok ? 'done' : 'failed', text: outcome.text };
      }
    } catch (e) {
      if (!inPanel) onflash(String(e), true);
      else if (run?.id === id) run = { ...run, state: 'failed', text: String(e) };
    }
  }

  function lineStep(): number {
    return resultEl ? parseFloat(getComputedStyle(resultEl).lineHeight) || 24 : 24;
  }

  export function handleKey(token: string) {
    if (token === 'Esc' || token === 'q') {
      onclose();
      return;
    }
    if (!run) {
      // Choosing: the terminal picker's keys.
      if (token === 'j' || token === 'Down') selected = (selected + 1) % ordered.length;
      else if (token === 'k' || token === 'Up') {
        selected = (selected + ordered.length - 1) % ordered.length;
      } else if (token === 'Enter') void start(ordered[selected] ?? null);
      else runHotkey(token.toLowerCase());
      return;
    }
    // Reading an answer: scroll it, or switch target by hotkey.
    const page = (resultEl?.clientHeight ?? 0) / 2;
    if (token === 'j' || token === 'Down') scroller?.by(lineStep() * 2);
    else if (token === 'k' || token === 'Up') scroller?.by(-lineStep() * 2);
    else if (token === 'C-d' || token === 'PageDown') scroller?.by(page);
    else if (token === 'C-u' || token === 'PageUp') scroller?.by(-page);
    else if (token === 'G' || token === 'End') scroller?.to(resultEl?.scrollHeight ?? 0);
    else if (token === 'Home') scroller?.to(0);
    else runHotkey(token);
  }

  function runHotkey(key: string) {
    const target = targets.find((t) => hotkeys.get(t.name) === key);
    if (target) {
      selected = ordered.indexOf(target);
      void start(target);
    }
  }

  // ── Showing command output ──────────────────────────────────

  type Line =
    | { kind: 'blank' }
    | { kind: 'head' | 'entry' | 'body'; text: string; indent: number };

  /** Plain output is styled by indent, the convention lookup scripts already
      follow for the terminal app: flush left is a heading, a shallow indent
      an entry, anything deeper the body. */
  function byIndent(text: string): Line[] {
    return text
      .replace(/\s+$/, '')
      .split(/\r?\n/)
      .map((line): Line => {
        const body = line.trimStart();
        if (!body) return { kind: 'blank' };
        const indent = line.length - body.length;
        if (indent === 0) return { kind: 'head', text: body, indent: 0 };
        if (indent <= 5) return { kind: 'entry', text: body, indent: 0 };
        return { kind: 'body', text: body, indent: indent - 6 };
      });
  }

  type Piece = { text: string; style: string };

  const ANSI_TO_BASE16 = [3, 8, 11, 10, 13, 14, 12, 5];

  function ansiColor(codes: number[], at: number): [string | null, number] {
    const colors = prefs.theme.colors;
    const basic = (n: number, bright: boolean) =>
      colors[bright && n === 7 ? 7 : ANSI_TO_BASE16[n]!] ?? null;
    const code = codes[at]!;
    if (code >= 30 && code <= 37) return [basic(code - 30, false), at];
    if (code >= 90 && code <= 97) return [basic(code - 90, true), at];
    if (code === 38 && codes[at + 1] === 2) {
      return [`rgb(${codes[at + 2] ?? 0} ${codes[at + 3] ?? 0} ${codes[at + 4] ?? 0})`, at + 4];
    }
    if (code === 38 && codes[at + 1] === 5) {
      const n = codes[at + 2] ?? 0;
      if (n < 16) return [basic(n % 8, n >= 8), at + 2];
      if (n >= 232) {
        const level = 8 + (n - 232) * 10;
        return [`rgb(${level} ${level} ${level})`, at + 2];
      }
      const channel = (v: number) => (v === 0 ? 0 : 55 + v * 40);
      const c = n - 16;
      return [
        `rgb(${channel(Math.floor(c / 36))} ${channel(Math.floor(c / 6) % 6)} ${channel(c % 6)})`,
        at + 2,
      ];
    }
    return [null, at];
  }

  /** Output that colors itself with terminal escape codes keeps its own styling. */
  function ansiPieces(text: string): Piece[] {
    const pieces: Piece[] = [];
    let bold = false;
    let dim = false;
    let italic = false;
    let underline = false;
    let color: string | null = null;
    const style = () =>
      [
        bold ? 'font-weight:600' : '',
        dim ? 'opacity:0.7' : '',
        italic ? 'font-style:italic' : '',
        underline ? 'text-decoration:underline' : '',
        color ? `color:${color}` : '',
      ]
        .filter(Boolean)
        .join(';');
    // Color changes are kept; cursor movement and other escapes are dropped.
    const escape = /\x1b\[([0-9;]*)m|\x1b\[[0-9;?]*[A-Za-z]|\x1b\][^\x07\x1b]*(?:\x07|\x1b\\)/g;
    let last = 0;
    for (const match of text.matchAll(escape)) {
      if (match.index > last) pieces.push({ text: text.slice(last, match.index), style: style() });
      last = match.index + match[0].length;
      if (match[1] === undefined) continue;
      const codes = match[1] === '' ? [0] : match[1].split(';').map(Number);
      for (let i = 0; i < codes.length; i++) {
        const code = codes[i]!;
        if (code === 0) {
          bold = dim = italic = underline = false;
          color = null;
        } else if (code === 1) bold = true;
        else if (code === 2) dim = true;
        else if (code === 3) italic = true;
        else if (code === 4) underline = true;
        else if (code === 22) bold = dim = false;
        else if (code === 23) italic = false;
        else if (code === 24) underline = false;
        else if (code === 39) color = null;
        else {
          const [next, end] = ansiColor(codes, i);
          if (next) color = next;
          i = end;
        }
      }
    }
    if (last < text.length) pieces.push({ text: text.slice(last), style: style() });
    return pieces;
  }

  let hasEscapes = $derived(run ? /\x1b\[/.test(run.text) : false);

  onMount(() => {
    if (targets.length === 0) void start(null);
  });

  onDestroy(cancelRunning);
</script>

{#snippet tile(target: LookupTarget)}
  <button
    class="target"
    class:active={run ? run.label === target.name : ordered[selected] === target}
    title={target.name}
    onclick={() => {
      selected = ordered.indexOf(target);
      void start(target);
    }}
  >
    <span class="key">{hotkeys.get(target.name) ?? ''}</span>
    <span class="name">{target.name}</span>
  </button>
{/snippet}

<aside aria-label="Look up" class:focused>
  <div class="top">
    <span class="label">LOOK UP</span>
    <button class="close" aria-label="Close lookup" onclick={onclose}><kbd>Esc</kbd></button>
  </div>
  <div class="query" class:long={query.length > 28}>{query}</div>

  {#if ordered.length > 0}
    <div class="groups">
      {#if shown.length > 0}
        <span class="caption">Shown here</span>
        <div class="targets">
          {#each shown as target (target.name)}{@render tile(target)}{/each}
        </div>
      {/if}
      {#if outside.length > 0}
        <span class="caption">Opens outside the reader</span>
        <div class="targets">
          {#each outside as target (target.name)}{@render tile(target)}{/each}
        </div>
      {/if}
    </div>
  {/if}

  <div class="result" bind:this={resultEl}>
    {#if run}
      <span class="label accent">{run.label.toUpperCase()}</span>
      {#if run.state === 'running'}
        <p class="quiet">Thinking… {seconds}s</p>
      {:else if run.state === 'failed'}
        <p class="failed">{run.text.trim()}</p>
      {:else if !run.text.trim()}
        <p class="quiet">(no output)</p>
      {:else if hasEscapes}
        <pre class="terminal">{#each ansiPieces(run.text) as piece}<span style={piece.style}>{piece.text}</span>{/each}</pre>
      {:else}
        <div class="answer">
          {#each byIndent(run.text) as line}
            {#if line.kind === 'blank'}
              <div class="blank"></div>
            {:else}
              <div class={line.kind} style:padding-left={line.indent ? `${line.indent * 0.3}em` : undefined}>
                {line.text}
              </div>
            {/if}
          {/each}
        </div>
      {/if}
    {/if}
  </div>

  <div class="hints">
    {#if run}
      <span><kbd>j</kbd><kbd>k</kbd> scroll</span>
    {:else if ordered.length > 0}
      <span><kbd>j</kbd><kbd>k</kbd> choose</span>
      <span><kbd>Enter</kbd> run</span>
    {/if}
    <span><kbd>Tab</kbd> back to the text</span>
  </div>
</aside>

<style>
  aside {
    flex: 0 0 min(432px, 46%);
    min-width: 0;
    min-height: 0;
    display: flex;
    flex-direction: column;
    padding: 26px 28px 0 30px;
  }

  .top {
    flex: none;
    display: flex;
    align-items: center;
    justify-content: space-between;
  }

  .label {
    font-family: var(--font-mono);
    font-size: 10.5px;
    letter-spacing: 0.16em;
    color: var(--muted);
  }

  .label.accent {
    color: var(--accent);
  }

  .close {
    height: 32px;
    display: flex;
    align-items: center;
    padding: 0 4px;
  }

  .query {
    flex: none;
    margin: 6px 0 26px;
    font-family: var(--font-reading);
    font-size-adjust: var(--letter-height, none);
    font-style: italic;
    font-size: 40px;
    line-height: 1.1;
    letter-spacing: -0.01em;
    color: var(--heading);
    overflow-wrap: anywhere;
    display: -webkit-box;
    -webkit-box-orient: vertical;
    -webkit-line-clamp: 2;
    line-clamp: 2;
    overflow: hidden;
  }

  .query.long {
    font-size: 19px;
    line-height: 1.4;
    letter-spacing: 0;
    -webkit-line-clamp: 4;
    line-clamp: 4;
  }

  .groups {
    flex: none;
    display: flex;
    flex-direction: column;
    gap: 6px;
  }

  .caption {
    padding: 0 10px;
    font-size: 11.5px;
    color: var(--muted);
  }

  .targets {
    display: grid;
    grid-template-columns: repeat(2, minmax(0, 1fr));
    gap: 2px 8px;
    margin-bottom: 10px;
  }

  .target {
    display: flex;
    align-items: baseline;
    gap: 12px;
    min-width: 0;
    padding: 7px 10px;
    border-radius: 7px;
    line-height: 20px;
    color: var(--quiet);
    outline: 1px solid transparent;
    outline-offset: -1px;
  }

  .key {
    flex: none;
    width: 10px;
    text-align: center;
    font-family: var(--font-mono);
    font-size: 11.5px;
    color: var(--accent);
  }

  .name {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    text-transform: capitalize;
  }

  .target:hover {
    color: var(--heading);
  }

  .target.active {
    background: var(--soft);
    color: var(--heading);
  }

  aside.focused .target.active {
    outline-color: var(--ring);
  }

  .result {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: 14px;
    margin-top: 14px;
    padding: 24px 10px 0;
    border-top: 1px solid var(--hairline);
    font-family: var(--font-reading);
    font-size-adjust: var(--letter-height, none);
    font-variant-numeric: var(--reading-figures, oldstyle-nums);
    font-size: 15.5px;
    line-height: 1.62;
    scrollbar-width: none;
  }

  .result p {
    margin: 0;
  }

  .quiet {
    color: var(--muted);
  }

  .failed {
    color: var(--error);
    white-space: pre-wrap;
    overflow-wrap: anywhere;
  }

  .answer {
    color: var(--text);
    text-wrap: pretty;
    overflow-wrap: break-word;
  }

  .head {
    font-size: 1.25em;
    font-weight: 500;
    line-height: 1.3;
    color: var(--heading);
  }

  .entry {
    margin-top: 0.4em;
    font-style: italic;
    color: var(--muted);
  }

  .blank {
    height: 0.62em;
  }

  .terminal {
    margin: 0;
    font-family: var(--font-mono);
    font-size-adjust: none;
    font-size: 12.5px;
    line-height: 1.55;
    white-space: pre-wrap;
    overflow-wrap: anywhere;
  }

  .hints {
    flex: none;
    min-height: 36px;
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 6px 16px;
    padding: 0 10px;
    font-size: 11.5px;
    color: var(--muted);
  }

  .hints span {
    display: flex;
    align-items: center;
    gap: 6px;
  }
</style>
