<script lang="ts" module>
  const NUMBER_WORDS =
    'one|two|three|four|five|six|seven|eight|nine|ten|eleven|twelve|thirteen|fourteen|fifteen|sixteen|seventeen|eighteen|nineteen|(?:twenty|thirty|forty|fifty)(?:-\\w+)?';
  const NUMBERED_HEADING = new RegExp(
    `^((?:chapter|part|book)\\s+(?:\\d+|[ivxlcdm]+|${NUMBER_WORDS}))(\\s*[:.\u2014\u2013-]\\s*)(\\S.*)$`,
    'iu',
  );
  const LEAD_PHRASE = /^\s*\S+(?:\s+\S+){0,4}?[.,;:!?\u2026\u2014](?=\s)/u;
  const LEAD_WORDS = /^\s*\S+(?:\s+\S+){0,2}/u;
  const STARTS_WITH_LETTER = /^\s*["'\u201c\u2018]?\p{L}/u;
  const OPENING_MIN_LENGTH = 200;
</script>

<script lang="ts">
  import type { Cell, Inline, Node } from './api';
  import { highlight } from './code';
  import Inlines from './Inlines.svelte';
  import Self from './Blocks.svelte';

  // `top` marks the chapter's own blocks: their position is the node index
  // that comments and marks are stored against, counted from `base` when
  // the chapter is given in several runs. Blocks inside them carry
  // their place among their siblings, so a paragraph in a wrapping block can
  // be addressed the way the terminal app addresses it. `opening` sets the start of
  // a chapter the way a printed book does.
  let {
    nodes,
    top = false,
    base = 0,
    opening = false,
  }: { nodes: Node[]; top?: boolean; base?: number; opening?: boolean } = $props();

  /** Whether a paragraph holds nothing but pictures, as a map or a plate
      on its own line does; such a paragraph is centred. */
  function onlyPictures(items: Inline[]): boolean {
    let pictures = 0;
    for (const item of items) {
      if (item.t === 'img') pictures += 1;
      else if (item.t === 'link' && onlyPictures(item.children)) pictures += 1;
      else if (item.t === 'text' ? item.s.trim() !== '' : item.t === 'link') return false;
    }
    return pictures > 0;
  }

  function plainLength(items: Inline[]): number {
    let n = 0;
    for (const item of items) {
      if (item.t === 'text') n += item.s.length;
      else if (item.t === 'link') n += plainLength(item.children);
    }
    return n;
  }

  /** The heading that opens the chapter, split into label, separator and
      title when it is written as "Chapter 1: The Discovery". */
  let numbered = $derived.by(() => {
    const first = opening ? nodes[0] : undefined;
    if (first?.t !== 'h' || first.level > 2 || first.text.length !== 1) return null;
    const only = first.text[0]!;
    if (only.t !== 'text' || only.style) return null;
    const match = NUMBERED_HEADING.exec(only.s);
    return match ? { label: match[1]!, sep: match[2]!, title: match[3]! } : null;
  });

  /** The paragraph that follows the chapter's headings, if it is long
      enough to carry an initial, with its first words split off. */
  let lead = $derived.by(() => {
    if (!opening) return null;
    const index = nodes.findIndex((node) => node.t !== 'h');
    const node = nodes[index];
    if (index < 1 || node?.t !== 'p') return null;
    const first = node.text[0];
    if (first?.t !== 'text' || first.style || !STARTS_WITH_LETTER.test(first.s)) return null;
    if (plainLength(node.text) < OPENING_MIN_LENGTH) return null;
    const words = (LEAD_PHRASE.exec(first.s) ?? LEAD_WORDS.exec(first.s))?.[0] ?? '';
    const rest: Inline[] = [{ t: 'text', s: first.s.slice(words.length) }, ...node.text.slice(1)];
    return { index, words, rest };
  });
</script>

{#snippet cell(c: Cell, align: string | undefined)}
  <svelte:element
    this={c.header ? 'th' : 'td'}
    rowspan={c.rowspan > 1 ? c.rowspan : undefined}
    colspan={c.colspan > 1 ? c.colspan : undefined}
    style:text-align={align && align !== 'none' ? align : undefined}
  >
    {#if c.text}<Inlines items={c.text} />{:else if c.blocks}<Self nodes={c.blocks} />{/if}
  </svelte:element>
{/snippet}

{#each nodes as node, i}
  {@const index = top ? base + i : undefined}
  {@const child = top ? undefined : i}
  {#if node.t === 'h' && i === 0 && numbered}
    <svelte:element this={`h${node.level}`} data-node={index} data-child={child} data-anchor={node.id}><span class="eyebrow">{numbered.label}<span class="sep">{numbered.sep}</span></span>{numbered.title}</svelte:element>
  {:else if node.t === 'h'}
    <svelte:element this={`h${node.level}`} data-node={index} data-child={child} data-anchor={node.id}><Inlines items={node.text} /></svelte:element>
  {:else if node.t === 'p' && lead?.index === i}
    <p class="opening" data-node={index} data-child={child} data-anchor={node.id}><span class="lead">{lead.words}</span><Inlines items={lead.rest} /></p>
  {:else if node.t === 'p'}
    <p class:figure={onlyPictures(node.text)} data-node={index} data-child={child} data-anchor={node.id}><Inlines items={node.text} /></p>
  {:else if node.t === 'code'}
    {@const marked = highlight(node.code, node.lang)}
    <pre class:math={node.lang === 'math'} data-node={index} data-child={child} data-anchor={node.id} data-lang={node.lang}><code>{#if marked}{@html marked}{:else}{node.code}{/if}</code></pre>
  {:else if node.t === 'quote'}
    <blockquote data-node={index} data-child={child} data-anchor={node.id}><Self nodes={node.blocks} /></blockquote>
  {:else if node.t === 'list'}
    <svelte:element
      this={node.ordered ? 'ol' : 'ul'}
      start={node.ordered && node.start !== 1 ? node.start : undefined}
      data-node={index} data-child={child}
      data-anchor={node.id}
    >
      {#each node.items as item}
        <li><Self nodes={item} /></li>
      {/each}
    </svelte:element>
  {:else if node.t === 'table'}
    <div class="table-scroll" data-node={index} data-child={child} data-anchor={node.id}>
      <table>
        {#if node.header}
          <thead>
            <tr>
              {#each node.header as c, column}{@render cell(c, node.align[column])}{/each}
            </tr>
          </thead>
        {/if}
        <tbody>
          {#each node.rows as row}
            <tr>
              {#each row as c, column}{@render cell(c, node.align[column])}{/each}
            </tr>
          {/each}
        </tbody>
      </table>
    </div>
  {:else if node.t === 'dl'}
    <dl data-node={index} data-child={child} data-anchor={node.id}>
      {#each node.items as item}
        <dt><Inlines items={item.term} /></dt>
        {#each item.defs as def}
          <dd><Self nodes={def} /></dd>
        {/each}
      {/each}
    </dl>
  {:else if node.t === 'epub'}
    <div class="epub-block" data-epub={node.kind} data-node={index} data-child={child} data-anchor={node.id}>
      <Self nodes={node.blocks} />
    </div>
  {:else if node.t === 'hr'}
    <hr data-node={index} data-child={child} />
  {/if}
{/each}
