<script lang="ts" module>
  /** A line of the list: what it does, and where its keys come from. A line
      that covers several actions (down and up, say) shows one key for each. */
  type Row = {
    label: string;
    actions?: [context: string, action: string][];
    /** Keys that are fixed rather than bound, for the look-up panel. */
    fixed?: string[][];
  };

  const SECTIONS: { title: string; rows: Row[] }[] = [
    {
      title: 'Reading',
      rows: [
        { label: 'Scroll down, up', actions: [['content', 'scroll_down'], ['content', 'scroll_up']] },
        { label: 'Half a page down, up', actions: [['content', 'scroll_half_down'], ['content', 'scroll_half_up']] },
        { label: 'A page down, up', actions: [['content', 'scroll_page_down'], ['content', 'scroll_page_up']] },
        { label: 'Top, bottom of the chapter', actions: [['content', 'go_top'], ['content', 'go_bottom']] },
        { label: 'Previous, next chapter', actions: [['content', 'prev_chapter'], ['content', 'next_chapter']] },
        { label: 'Previous, next paragraph', actions: [['content', 'paragraph_backward'], ['content', 'paragraph_forward']] },
        { label: 'Back, forward to where you were', actions: [['content', 'jump_backward'], ['content', 'jump_forward']] },
        { label: 'Follow a link under the cursor', actions: [['content', 'follow_link']] },
        { label: 'Open a picture large', fixed: [['click']] },
      ],
    },
    {
      title: 'Finding',
      rows: [
        { label: 'Find in the chapter', actions: [['content', 'start_search']] },
        { label: 'Next, previous place found', fixed: [['n'], ['N']] },
        { label: 'Stop finding', fixed: [['Esc']] },
        { label: 'Search the whole book', actions: [['global', 'open_book_search_fresh']] },
        { label: 'Back to the last search', actions: [['global', 'open_book_search']] },
      ],
    },
    {
      title: 'Cursor and selection',
      rows: [
        { label: 'Show or hide the cursor', actions: [['content', 'toggle_normal_mode']] },
        { label: 'Left, down, up, right', actions: [['epub_normal', 'move_left'], ['epub_normal', 'move_down'], ['epub_normal', 'move_up'], ['epub_normal', 'move_right']] },
        { label: 'Next word, previous word, end of word', actions: [['epub_normal', 'word_forward'], ['epub_normal', 'word_backward'], ['epub_normal', 'word_end']] },
        { label: 'Start, end of the line', actions: [['epub_normal', 'line_start'], ['epub_normal', 'line_end']] },
        { label: 'Find a character ahead, behind', actions: [['epub_normal', 'find_forward'], ['epub_normal', 'find_backward']] },
        { label: 'Select, select whole lines', actions: [['epub_normal', 'enter_visual_mode'], ['epub_normal', 'enter_visual_line_mode']] },
        { label: 'Copy the selection', actions: [['epub_normal', 'copy_selection']] },
        { label: 'Copy with a motion (yank)', actions: [['epub_normal', 'start_yank']] },
        { label: 'Copy the whole chapter', actions: [['global', 'copy_chapter_text']] },
      ],
    },
    {
      title: 'Look up',
      rows: [
        { label: 'Look up the selection', actions: [['global', 'lookup_selection']] },
        { label: 'Choose where to look', fixed: [['j'], ['k']] },
        { label: 'Look there', fixed: [['Enter']] },
        { label: 'Back to the text, panel stays open', fixed: [['Tab']] },
        { label: 'Close the panel', fixed: [['Esc']] },
      ],
    },
    {
      title: 'Highlights, notes and bookmarks',
      rows: [
        { label: 'Highlight the selection, or recolour one', actions: [['content', 'open_highlight_palette']] },
        { label: 'Write a note on the selection, or edit one', actions: [['content', 'add_comment']] },
        { label: 'Remove the highlight or note here', actions: [['content', 'delete_comment']] },
        { label: 'List every highlight, note and bookmark', actions: [['global', 'toggle_comments_viewer']] },
        { label: 'In that list: filter, copy, export', fixed: [['/'], ['c'], ['Space', 'e']] },
        { label: 'Set a bookmark, then a letter', actions: [['content', 'set_mark']] },
        { label: 'Go to a bookmark, then its letter', actions: [['content', 'goto_mark']] },
      ],
    },
    {
      title: 'Contents',
      rows: [
        { label: 'Move between the text and the contents', actions: [['content', 'switch_focus']] },
        { label: 'Down, up', actions: [['nav', 'move_down'], ['nav', 'move_up']] },
        { label: 'Go to the chapter', actions: [['nav', 'select']] },
        { label: 'Find an entry, then the next, the one before', actions: [['nav', 'start_search'], ['nav', 'next_search_match'], ['nav', 'prev_search_match']] },
        { label: 'Close, open a section', actions: [['nav', 'collapse'], ['nav', 'expand']] },
        { label: 'Close, open every section', actions: [['nav', 'collapse_all'], ['nav', 'expand_all']] },
        { label: 'Narrower, wider panel', actions: [['global', 'shrink_nav_panel'], ['global', 'expand_nav_panel']] },
        { label: 'Go to the library', actions: [['nav', 'switch_nav_mode']] },
      ],
    },
    {
      title: 'Library',
      rows: [
        { label: 'Left, down, up, right', actions: [['nav', 'collapse'], ['nav', 'move_down'], ['nav', 'move_up'], ['nav', 'expand']] },
        { label: 'Open the book', actions: [['nav', 'select']] },
        { label: 'Find a book, then the next, the one before', actions: [['nav', 'start_search'], ['nav', 'next_search_match'], ['nav', 'prev_search_match']] },
        { label: 'Change the order', actions: [['nav', 'toggle_sort_order']] },
        { label: 'Reading history', actions: [['global', 'toggle_reading_history']] },
        { label: 'Back to the book you were reading', actions: [['nav', 'switch_nav_mode']] },
      ],
    },
    {
      title: 'PDF and DjVu',
      rows: [
        { label: 'Previous, next page', actions: [['pdf', 'prev_page'], ['pdf', 'next_page']] },
        { label: 'Go to a page by its number', actions: [['global', 'go_to_page']] },
        { label: 'Larger, smaller', actions: [['pdf', 'zoom_in'], ['pdf', 'zoom_out']] },
        { label: 'The usual size, the width of the window', actions: [['pdf', 'zoom_reset'], ['pdf', 'zoom_fit_width']] },
        { label: 'Move left, right when zoomed in', actions: [['pdf', 'pan_left'], ['pdf', 'pan_right']] },
        { label: 'One page across, or two', actions: [['global', 'toggle_pdf_page_layout']] },
        { label: 'Scroll, or one page at a time', actions: [['global', 'toggle_pdf_render_mode']] },
        { label: 'Theme colours, or the document\u2019s own', actions: [['pdf', 'toggle_pdf_theming']] },
        { label: 'Pictures in theme colours, or their own', actions: [['pdf', 'toggle_invert_images']] },
        { label: 'Outline the links', actions: [['pdf', 'toggle_pdf_link_highlight']] },
        { label: 'Reload when the file changes', actions: [['global', 'toggle_pdf_watching']] },
        { label: 'Jump to the TeX source', actions: [['pdf', 'synctex_inverse']] },
        { label: 'Copy the page\u2019s text', actions: [['global', 'copy_chapter_text']] },
      ],
    },
    {
      title: 'Window',
      rows: [
        { label: 'Every command that starts with Space', fixed: [['Space']] },
        { label: 'Zen mode: nothing but the text', actions: [['global', 'toggle_zen_mode']] },
        { label: 'Keep the page\u2019s border in zen mode', actions: [['global', 'toggle_zen_border']] },
        { label: 'Appearance: typeface, theme, text', actions: [['global', 'open_theme_selector']] },
        { label: 'Justify text', actions: [['global', 'toggle_justify_text']] },
        { label: 'Narrower, wider column of text', actions: [['content', 'increase_margin'], ['content', 'decrease_margin']] },
        { label: 'How long each chapter is', actions: [['global', 'toggle_book_stats']] },
        { label: 'The chapter\u2019s HTML as the book has it', actions: [['content', 'toggle_raw_html']] },
        { label: 'Open the book in the system viewer', actions: [['global', 'open_external_viewer']] },
        { label: 'Read keybindings.toml again', actions: [['global', 'reload_keybindings']] },
        { label: 'These shortcuts', actions: [['global', 'toggle_help']] },
        { label: 'Quit', actions: [['content', 'quit']] },
      ],
    },
  ];
</script>

<script lang="ts">
  import type { Snippet } from 'svelte';
  import { keyLabel, type Keymap } from './keys';
  import { SmoothScroller } from './scroller';

  let {
    keymap,
    status,
    onclose,
  }: {
    /** The bindings in force, so the list shows the reader's own keys. */
    keymap: Keymap;
    /** The window's status line, drawn under the list. */
    status: Snippet;
    onclose: () => void;
  } = $props();

  const LINE = 34;

  let scrollEl: HTMLElement;
  let scroller: SmoothScroller | undefined;

  $effect(() => {
    scroller = new SmoothScroller(scrollEl);
    return () => scroller?.stop();
  });

  /** Each line's bindings: a list of ways to press it, each a list of keys. */
  let sections = $derived(
    SECTIONS.map(({ title, rows }) => ({
      title,
      rows: rows
        .map(({ label, actions, fixed }) => {
          const found = (actions ?? []).map(([context, action]) => keymap.keysFor(context, action));
          // One action shows up to two ways to press it; several show one each.
          const single = found.length === 1;
          const bindings = fixed ?? (single ? found[0]!.slice(0, 2) : found.flatMap((keys) => keys.slice(0, 1)));
          return { label, bindings, alternatives: single && !fixed };
        })
        .filter((row) => row.bindings.length > 0),
    })).filter((section) => section.rows.length > 0),
  );

  /** Handles a keymap action; returns false if it means nothing here. */
  export function handle(action: string): boolean {
    const page = scrollEl.clientHeight;
    switch (action) {
      case 'move_down':
        scroller?.by(LINE * 2);
        return true;
      case 'move_up':
        scroller?.by(-LINE * 2);
        return true;
      case 'scroll_half_down':
        scroller?.by(page / 2);
        return true;
      case 'scroll_half_up':
        scroller?.by(-page / 2);
        return true;
      case 'scroll_page_down':
        scroller?.by(page - LINE);
        return true;
      case 'scroll_page_up':
        scroller?.by(LINE - page);
        return true;
      case 'go_top':
        scroller?.to(0);
        return true;
      case 'go_bottom':
        scroller?.to(scrollEl.scrollHeight);
        return true;
      case 'cancel':
        onclose();
        return true;
      default:
        return false;
    }
  }
</script>

<section class="help" aria-label="Keyboard shortcuts">
  <header>
    <button class="back" aria-label="Close keyboard shortcuts" onclick={onclose}>
      <svg aria-hidden="true" width="16" height="16" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round"><path d="M9.5 4L5.5 8l4 4"></path></svg>
    </button>
    <div>
      <h1>Keyboard shortcuts</h1>
      <p>Right-click anywhere for the same things in a menu.</p>
    </div>
  </header>

  <div class="scroll" bind:this={scrollEl}>
    <div class="columns">
      {#each sections as section}
        <div class="group">
          <h2>{section.title}</h2>
          {#each section.rows as row}
            <div class="row">
              <span class="label">{row.label}</span>
              <span class="keys">
                {#each row.bindings as binding, index}
                  {#if index > 0 && row.alternatives}<span class="or">or</span>{/if}
                  <span class="binding">{#each binding as key}<kbd>{keyLabel(key)}</kbd>{/each}</span>
                {/each}
              </span>
            </div>
          {/each}
        </div>
      {/each}
    </div>
  </div>

  {@render status()}
</section>

<style>
  .help {
    flex: 1;
    min-width: 0;
    min-height: 0;
    display: flex;
    flex-direction: column;
  }

  header {
    flex: none;
    display: flex;
    align-items: flex-start;
    gap: 10px;
    padding: 40px 64px 0 24px;
  }

  .back {
    flex: none;
    width: 32px;
    height: 32px;
    margin-top: 4px;
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

  h1 {
    margin: 0;
    font-family: var(--font-reading);
    font-size-adjust: var(--letter-height, none);
    font-size: 32px;
    font-weight: 500;
    line-height: 1.2;
    letter-spacing: -0.01em;
    color: var(--heading);
  }

  p {
    margin: 6px 0 0;
    color: var(--muted);
  }

  .scroll {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    padding: 20px 64px 48px 66px;
    scrollbar-width: none;
  }

  .columns {
    columns: 3 320px;
    column-gap: 64px;
  }

  .group {
    break-inside: avoid;
    padding-top: 16px;
  }

  h2 {
    margin: 0;
    padding: 0 0 8px;
    font-family: var(--font-reading);
    font-size-adjust: var(--letter-height, none);
    font-size: 13px;
    font-weight: 400;
    line-height: 18px;
    font-variant-caps: all-small-caps;
    letter-spacing: 0.14em;
    color: var(--accent);
  }

  .row {
    display: flex;
    align-items: center;
    gap: 16px;
    min-height: 34px;
    border-top: 1px solid var(--hairline);
  }

  .label {
    flex: 1;
    min-width: 0;
  }

  .keys {
    flex: none;
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .binding {
    display: flex;
    gap: 3px;
  }

  .or {
    font-size: 11.5px;
    color: var(--muted);
  }
</style>
