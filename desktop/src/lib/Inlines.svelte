<script lang="ts">
  import { bookResource, type Inline } from './api';
  import Self from './Inlines.svelte';

  type TextItem = Extract<Inline, { t: 'text' }>;

  let { items }: { items: Inline[] } = $props();
</script>

{#snippet styled(item: TextItem)}{#if item.style === 'em'}<em>{item.s}</em>{:else if item.style === 'strong'}<strong>{item.s}</strong>{:else if item.style === 'code'}<code>{item.s}</code>{:else if item.style === 'strike'}<s>{item.s}</s>{:else}{item.s}{/if}{/snippet}

<!-- Kept on one line: whitespace between these pieces would show up as stray spaces in the text. -->
{#each items as item}{#if item.t === 'text'}{#if item.script === 'sup'}<sup data-chars={item.chars}>{@render styled(item)}</sup>{:else if item.script === 'sub'}<sub data-chars={item.chars}>{@render styled(item)}</sub>{:else}{@render styled(item)}{/if}{:else if item.t === 'link'}<a href={item.url} data-kind={item.kind} data-chapter={item.chapter} data-target={item.anchor} data-part={item.part}><Self items={item.children} /></a>{:else if item.t === 'img'}<img src={bookResource(item.src)} alt={item.alt} loading="lazy" decoding="async" />{:else if item.t === 'anchor'}<span data-anchor={item.id}></span>{:else if item.t === 'br'}<br />{:else}{' '}{/if}{/each}
