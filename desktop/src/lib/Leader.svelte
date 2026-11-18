<script lang="ts" module>
  /** What each leader command is called, and the group it is listed in.
      A command with no entry here is still listed, under its own name. */
  const NAMES: Record<string, [group: string, book: string, pdf?: string]> = {
    toggle_reading_history: ['Go', 'Reading history'],
    open_book_search: ['Go', 'Reopen book search', 'Reopen document search'],
    open_book_search_fresh: ['Go', 'New book search', 'New document search'],
    toggle_comments_viewer: ['Go', 'Notes and bookmarks'],
    toggle_book_stats: ['Go', 'Book statistics', 'Document statistics'],
    lookup_selection: ['Text', 'Look up selection'],
    copy_chapter_text: ['Text', 'Copy chapter', 'Copy page text'],
    copy_toc_item: ['Text', 'Copy chapter', 'Copy contents entry'],
    toggle_justify_text: ['Text', 'Justify text'],
    open_external_viewer: ['Text', 'Open in system viewer'],
    toggle_zen_mode: ['View', 'Zen mode'],
    toggle_zen_border: ['View', 'Zen border'],
    open_theme_selector: ['View', 'Appearance'],
    open_settings: ['View', 'Settings'],
    reset_nav_panel_width: ['View', 'Reset sidebar width'],
    go_to_page: ['PDF', 'Go to page'],
    toggle_pdf_page_layout: ['PDF', 'Single or dual page'],
    toggle_pdf_render_mode: ['PDF', 'Scroll or page mode'],
    toggle_pdf_watching: ['PDF', 'Watch file and reload'],
  };
  const GROUPS = ['Go', 'Text', 'View', 'PDF', 'Other'];
  /** Commands that do nothing for the kind of book that is open. */
  const BOOK_ONLY = ['toggle_justify_text', 'copy_toc_item'];
</script>

<script lang="ts">
  import { keyLabel, type Keymap } from './keys';

  let {
    keymap,
    prefix,
    pdf,
  }: {
    keymap: Keymap;
    /** The keys pressed so far. */
    prefix: string[];
    /** Whether a PDF is open, which has commands of its own. */
    pdf: boolean;
  } = $props();

  let groups = $derived.by(() => {
    const rows = new Map<string, { label: string; keys: string[][] }>();
    const byGroup = new Map<string, string[]>();
    for (const { keys, action } of keymap.after('global', prefix)) {
      const [group, book, paged] = NAMES[action] ?? ['Other', action.replaceAll('_', ' ')];
      if (group === 'PDF' && !pdf) continue;
      if (pdf && BOOK_ONLY.includes(action)) continue;
      // Two keys for one command (Space < and Space >) share a line.
      const label = (pdf ? paged : undefined) ?? book;
      const id = `${group} ${label}`;
      const row = rows.get(id);
      if (row) {
        row.keys.push(keys);
      } else {
        rows.set(id, { label, keys: [keys] });
        byGroup.set(group, [...(byGroup.get(group) ?? []), id]);
      }
    }
    // Listed in the order the commands are named above, not the order
    // the bindings happen to come in; a lower-case key before its capital.
    const listed = Object.values(NAMES).flatMap(([group, book, paged]) => [
      `${group} ${book}`,
      `${group} ${paged}`,
    ]);
    const rank = (id: string) => (listed.indexOf(id) + 1 || listed.length + 1);
    const lowerFirst = (a: string[], b: string[]) =>
      a.join('').toLowerCase().localeCompare(b.join('').toLowerCase()) || b.join('').localeCompare(a.join(''));
    return GROUPS.flatMap((title) => {
      const ids = byGroup.get(title);
      if (!ids) return [];
      const sorted = [...ids].sort((a, b) => rank(a) - rank(b) || a.localeCompare(b));
      return [
        {
          title,
          rows: sorted.map((id) => {
            const row = rows.get(id)!;
            return { ...row, keys: [...row.keys].sort(lowerFirst) };
          }),
        },
      ];
    });
  });
</script>

<section class="leader" aria-label="Leader commands">
  <div class="head">
    <span class="pressed">{#each prefix as key}<kbd>{keyLabel(key)}</kbd>{/each}</span>
    <span class="then">then press a key</span>
    <span class="grow"></span>
    <span class="note">Listed from your keybindings</span>
    <span class="note"><kbd>Esc</kbd> cancel</span>
  </div>
  <div class="grid">
    {#each groups as group}
      <div class="group">
        <h2>{group.title}</h2>
        {#each group.rows as row}
          <div class="row">
            <span class="keys">
              {#each row.keys as keys}
                <span class="binding">{#each keys as key}<kbd>{keyLabel(key)}</kbd>{/each}</span>
              {/each}
            </span>
            <span>{row.label}</span>
          </div>
        {/each}
      </div>
    {/each}
  </div>
</section>

<style>
  .leader {
    flex: none;
    margin: 8px 8px 0;
    padding: 16px 24px 20px;
    display: flex;
    flex-direction: column;
    gap: 14px;
    border-radius: 12px;
    background: var(--raised);
    box-shadow: 0 0 0 1px var(--line);
  }

  .head {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 8px 12px;
  }

  .pressed {
    display: flex;
    gap: 4px;
  }

  .then {
    font-weight: 600;
    color: var(--heading);
  }

  .grow {
    flex: 1;
  }

  .note {
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: 12px;
    color: var(--muted);
  }

  .grid {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(min(230px, 100%), 1fr));
    gap: 16px 32px;
  }

  .group {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }

  h2 {
    margin: 0;
    padding-bottom: 2px;
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
    gap: 10px;
    min-height: 22px;
  }

  .keys {
    display: flex;
    gap: 6px;
  }

  .binding {
    display: flex;
    gap: 3px;
  }

  kbd {
    background: var(--chrome);
  }
</style>
