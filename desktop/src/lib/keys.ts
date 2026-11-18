import type { Binding } from './api';

type TrieNode = { action?: string; next: Map<string, TrieNode> };

export type Lookup = { kind: 'found'; action: string } | { kind: 'prefix' } | { kind: 'none' };

/** The core's keymap, including the user's keybindings.toml overrides. */
export class Keymap {
  private contexts = new Map<string, TrieNode>();

  constructor(private dump: Record<string, Binding[]>) {
    for (const [context, bindings] of Object.entries(dump)) {
      const root: TrieNode = { next: new Map() };
      for (const { keys, action } of bindings) {
        if (typeof action !== 'string') continue;
        let node = root;
        for (const key of keys) {
          let child = node.next.get(key);
          if (!child) {
            child = { next: new Map() };
            node.next.set(key, child);
          }
          node = child;
        }
        node.action = action;
      }
      this.contexts.set(context, root);
    }
  }

  /** The keys bound to an action, simplest first: single keys before
      sequences, letters before arrows and other named keys. */
  keysFor(context: string, action: string): string[][] {
    const named = (keys: string[]) => keys.filter((key) => [...key].length > 1).length;
    return (this.dump[context] ?? [])
      .filter((binding) => binding.action === action)
      .map((binding) => binding.keys)
      .sort((a, b) => a.length - b.length || named(a) - named(b));
  }

  /** What can follow a prefix such as Space: the keys still to press and
      what they do, in the order the bindings are listed. */
  after(context: string, prefix: string[]): { keys: string[]; action: string }[] {
    return (this.dump[context] ?? [])
      .filter(
        ({ keys, action }) =>
          typeof action === 'string' &&
          keys.length > prefix.length &&
          prefix.every((key, index) => keys[index] === key),
      )
      .map(({ keys, action }) => ({ keys: keys.slice(prefix.length), action: action as string }));
  }

  lookup(context: string, keys: string[]): Lookup {
    let node = this.contexts.get(context);
    for (const key of keys) {
      node = node?.next.get(key);
    }
    if (!node) return { kind: 'none' };
    if (node.action) return { kind: 'found', action: node.action };
    return node.next.size > 0 ? { kind: 'prefix' } : { kind: 'none' };
  }
}

const NAMED: Record<string, string> = {
  ' ': 'Space',
  Enter: 'Enter',
  Escape: 'Esc',
  Tab: 'Tab',
  Backspace: 'Backspace',
  Delete: 'Delete',
  ArrowUp: 'Up',
  ArrowDown: 'Down',
  ArrowLeft: 'Left',
  ArrowRight: 'Right',
  PageUp: 'PageUp',
  PageDown: 'PageDown',
  Home: 'Home',
  End: 'End',
};

const SHOWN: Record<string, string> = { Up: '↑', Down: '↓', Left: '←', Right: '→' };

/** A binding's key as it is shown to the reader: `C-d` becomes `Ctrl+d`. */
export function keyLabel(token: string): string {
  const parts = token.length > 1 ? token.split('-') : [token];
  const name = parts.pop() || '-';
  const modifiers = parts.map((m) => ({ C: 'Ctrl', A: 'Alt', S: 'Shift' })[m] ?? m);
  return [...modifiers, SHOWN[name] ?? name].join('+');
}

/** Names a key press the way the backend names bindings, or null for a bare modifier. */
export function keyToken(event: KeyboardEvent): string | null {
  const { key } = event;
  let name: string;
  let isChar = false;
  if (key in NAMED) {
    name = NAMED[key]!;
  } else if (/^F\d{1,2}$/.test(key)) {
    name = key;
  } else if ([...key].length === 1) {
    name = key;
    isChar = true;
  } else {
    return null;
  }
  let token = '';
  if (event.ctrlKey) token += 'C-';
  if (event.altKey) token += 'A-';
  if (event.shiftKey && !isChar) token += 'S-';
  return token + name;
}

/** Keys typed so far in a multi-key binding such as `gg`; forgotten after a second. */
export class KeySequence {
  private pending: string[] = [];
  private timer: ReturnType<typeof setTimeout> | undefined;

  constructor(private onChange: (keys: string[]) => void) {}

  get keys(): string[] {
    return this.pending;
  }

  /** `stay` keeps the keys until the next one comes, however long that
      takes: the leader key opens a list to choose from, with no hurry. */
  push(key: string, stay = false) {
    this.pending = [...this.pending, key];
    clearTimeout(this.timer);
    if (!stay) this.timer = setTimeout(() => this.clear(), 1000);
    this.onChange(this.pending);
  }

  clear() {
    if (this.pending.length === 0) return;
    this.pending = [];
    clearTimeout(this.timer);
    this.onChange(this.pending);
  }
}
