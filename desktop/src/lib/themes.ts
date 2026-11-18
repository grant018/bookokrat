/** A theme is sixteen Base16 colors, the same scheme the terminal app and
    its custom_themes use. Every color in the interface derives from them. */
export type Theme = { name: string; colors: string[]; light: boolean; custom: boolean };

type Rgb = [number, number, number];

function rgb(hex: string): Rgb {
  const n = parseInt(hex.slice(1), 16);
  return [(n >> 16) & 255, (n >> 8) & 255, n & 255];
}

function hex([r, g, b]: Rgb): string {
  return '#' + [r, g, b].map((v) => Math.round(v).toString(16).padStart(2, '0')).join('');
}

/** `amount` of `a` blended into `b`. */
function mix(a: string, b: string, amount: number): string {
  const [x, y] = [rgb(a), rgb(b)];
  return hex([0, 1, 2].map((i) => x[i]! * amount + y[i]! * (1 - amount)) as Rgb);
}

function luminance(color: string): number {
  const [r, g, b] = rgb(color).map((v) => {
    const s = v / 255;
    return s <= 0.03928 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4;
  }) as Rgb;
  return 0.2126 * r + 0.7152 * g + 0.0722 * b;
}

function contrast(a: string, b: string): number {
  const [hi, lo] = [luminance(a), luminance(b)].sort((x, y) => y - x) as [number, number];
  return (hi + 0.05) / (lo + 0.05);
}

function theme(name: string, colors: string, custom = false): Theme {
  const list = colors.split(' ').map((c) => '#' + c.toLowerCase());
  return { name, colors: list, light: luminance(list[0]!) > 0.4, custom };
}

// Themes the terminal app also has use its exact palettes, so the two match.
export const BUILTIN_THEMES: Theme[] = [
  theme('Kanagawa Dragon', '181616 0d0c0c 2d4f67 a6a69c 7fb4ca c5c9c5 c5c9c5 c5c9c5 c4746e e46876 c4b28a 8a9a7b 8ea4a2 8ba4b0 a292a3 7aa89f'),
  theme('Kanagawa Wave', '1f1f28 2a2a37 223249 727169 c8c093 dcd7ba dcd7ba e6e0c2 c34043 ffa066 dca561 98bb6c 7fb4ca 7e9cd8 957fb8 d27e99'),
  theme('Catppuccin Mocha', '1e1e2e 313244 45475a 6c7086 7f849c a6adc8 cdd6f4 f5e0dc f38ba8 fab387 f9e2af a6e3a1 94e2d5 89b4fa cba6f7 eba0ac'),
  theme('Oceanic Next', '1b2b34 343d46 4f5b66 65737e a7adba c0c5ce cdd3de f0f4f8 ec5f67 f99157 fac863 99c794 5fb3b3 6699cc c594c5 ab7967'),
  theme('Gruvbox Dark', '282828 3c3836 504945 665c54 bdae93 ebdbb2 ebdbb2 fbf1c7 fb4934 fe8019 fabd2f b8bb26 8ec07c 83a598 d3869b d65d0e'),
  theme('Nord', '2e3440 3b4252 434c5e 686f7a d8dee9 e5e9f0 eceff4 eceff4 bf616a d08770 ebcb8b a3be8c 8fbcbb 81a1c1 b48ead 5e81ac'),
  theme('Tokyo Night', '1a1b26 16161e 283457 565f89 a9b1d6 c0caf5 c0caf5 c0caf5 f7768e ff9e64 e0af68 9ece6a 7dcfff 7aa2f7 bb9af7 db4b4b'),
  theme('Rosé Pine', '191724 1f1d2e 403d52 6e6a86 908caa e0def4 e0def4 e0def4 eb6f92 f6c177 ebbcba 31748f 9ccfd8 c4a7e7 f6c177 524f67'),
  theme('Rosé Pine Moon', '18191a 212223 363738 6e6a86 908caa e0def4 e0def4 e0def4 eb6f92 c4a7e7 ea9a97 f6c177 9ccfd8 3e8fb0 c4a7e7 908caa'),
  theme('Moonfly', '080808 1c1c1c 323437 808080 9e9e9e c6c6c6 e4e4e4 e4e4e4 ff5189 de935f e3c78a 8cc85f 79dac8 80a0ff cf87e8 f09479'),
  theme('Black', '000000 0a0a0a 26323f 7a7a7a 9a9a9a c8c8c8 d8d8d8 e8e8e8 e06c75 d19a66 d7ba7d 98c379 56b6c2 7aa7d8 c586c0 be5046'),
  theme('Paper', 'faf9f6 f0eee8 d6e0ef 8a8782 5f5d58 1f1f1f 1f1f1f 000000 b3261e b25b00 8a6a00 2e7d32 00777a 2f5fa3 6b4ea0 8d5a3b'),
  theme('Sepia', 'f4ecd8 eadfc6 e0d2ae 9a8a73 7a6a55 5b4636 4a3728 3b2a1e a63d2f a85a1a 8a5a2b 5d7230 3f7a6e 3d6a94 7a4f8a 8b5e3c'),
  theme('Kanagawa Lotus', 'f2ecbc e5ddb0 c9cbd1 8a8980 716e61 545464 43436c 1f1f28 c84053 cc6d00 77713f 6f894e 597b75 4d699b 624c83 b35b79'),
  theme('Catppuccin Latte', 'eff1f5 e6e9ef ccd0da bcc0cc acb0be 4c4f69 4c4f69 4c4f69 d20f39 fe640b df8e1d 40a02b 179299 1e66f5 8839ef dd7878'),
  theme('Gruvbox Light', 'fbf1c7 ebdbb2 d5c4a1 bdae93 665c54 3c3836 3c3836 282828 9d0006 af3a03 b57614 79740e 427b58 076678 8f3f71 d65d0e'),
  theme('Solarized Light', 'fdf6e3 eee8d5 93a1a1 839496 657b83 586e75 073642 002b36 dc322f cb4b16 b58900 859900 2aa198 268bd2 6c71c4 d33682'),
];

/** Names the terminal app uses for themes that are listed differently here. */
const TERMINAL_NAMES: Record<string, string> = {
  Kanagawa: 'Kanagawa Wave',
  'Rosepine Moon': 'Rosé Pine Moon',
};

export function customTheme(name: string, colors: string[]): Theme | null {
  if (colors.length !== 16 || !colors.every((c) => /^#[0-9a-f]{6}$/.test(c))) return null;
  return theme(name, colors.map((c) => c.slice(1)).join(' '), true);
}

export function findTheme(themes: Theme[], name: string | null | undefined): Theme | undefined {
  if (!name) return undefined;
  const wanted = TERMINAL_NAMES[name] ?? name;
  return themes.find((t) => t.name === wanted);
}

export type Roles = Record<
  | 'page'
  | 'chrome'
  | 'raised'
  | 'line'
  | 'text'
  | 'heading'
  | 'muted'
  | 'accent'
  | 'link'
  | 'note'
  | 'selection'
  | 'cursor'
  | 'error',
  string
>;

/** Turns sixteen colors into the roles the interface paints with, falling
    back whenever a theme's own choice would be too faint to read. */
export function roles({ colors: c, light }: Theme): Roles {
  const page = c[0]!;
  const text = c[5]!;
  const readable = (candidates: (string | undefined)[], minimum: number) =>
    candidates.find((color) => color && contrast(color, page) >= minimum) ?? text;

  let muted = text;
  for (let amount = 0.62; amount < 1; amount += 0.04) {
    muted = mix(text, page, amount);
    if (contrast(muted, page) >= 4.6) break;
  }

  const selection =
    contrast(text, c[2]!) >= 4.5 && contrast(c[2]!, page) >= 1.12
      ? c[2]!
      : mix(c[13]!, page, 0.3);

  return {
    page,
    chrome: mix(page, c[1]!, 0.5),
    raised: mix(text, page, 0.12),
    line: mix(text, page, 0.2),
    text,
    heading: mix(light ? '#000000' : '#ffffff', text, 0.3),
    muted,
    accent: readable([c[10], c[9], c[13], c[14], c[8]], 4.5),
    link: readable([c[13], c[12], c[14]], 4.5),
    note: readable([c[14], c[15], c[13]], 3),
    selection,
    // The block cursor is blended as a difference; page plus text is the
    // color that comes out as the text color over the page.
    cursor: hex(rgb(page).map((v, i) => Math.min(255, v + rgb(text)[i]!)) as Rgb),
    error: readable([c[8]], 4.5),
  };
}

/** Base16 slots for the five highlight colours the terminal app has. */
const HIGHLIGHT_SLOTS = { red: 8, green: 11, blue: 13, yellow: 10, purple: 14 } as const;

/** The highlight pseudo-elements cannot read the theme's variables in every
    web view, so their colours are written out in a stylesheet of their own. */
function highlightRules(theme: Theme, note: string): string {
  const tint = (color: string, alpha: number) => `rgb(${rgb(color).join(' ')} / ${alpha})`;
  const strength = theme.light ? 0.42 : 0.36;
  const rules = Object.entries(HIGHLIGHT_SLOTS).map(
    ([name, slot]) => `::highlight(mark-${name}){background-color:${tint(theme.colors[slot]!, strength)}}`,
  );
  rules.push(
    `::highlight(note){background-color:${tint(note, 0.16)};text-decoration:underline;text-decoration-color:${note}}`,
  );
  // What `/` found: every match tinted, the one the reader is on solid.
  const { accent, page } = roles(theme);
  rules.push(
    `::highlight(find){background-color:${tint(accent, 0.3)}}`,
    `::highlight(find-current){background-color:${accent};color:${page}}`,
  );
  return rules.join('\n');
}

/** Base16 slots for the parts of a program, as Base16 themes for editors
    assign them. */
const SYNTAX_SLOTS = {
  comment: 3,
  variable: 8,
  number: 9,
  type: 10,
  string: 11,
  escape: 12,
  function: 13,
  keyword: 14,
  meta: 15,
} as const;

export function applyTheme(theme: Theme) {
  const style = document.documentElement.style;
  for (const [role, color] of Object.entries(roles(theme))) {
    style.setProperty(`--${role}`, color);
  }
  style.setProperty('--shadow', theme.light ? 'rgb(0 0 0 / 0.1)' : 'rgb(0 0 0 / 0.4)');
  for (const [name, slot] of Object.entries(HIGHLIGHT_SLOTS)) {
    style.setProperty(`--mark-${name}`, theme.colors[slot]!);
  }
  const { page, muted, text } = roles(theme);
  for (const [name, slot] of Object.entries(SYNTAX_SLOTS)) {
    // A theme's comment grey can be too faint to read on its own page.
    const color = theme.colors[slot]!;
    const faint = contrast(color, page) < 3;
    style.setProperty(`--syntax-${name}`, faint ? (name === 'comment' ? muted : text) : color);
  }
  let sheet = document.getElementById('highlight-colors');
  if (!sheet) {
    sheet = document.createElement('style');
    sheet.id = 'highlight-colors';
    document.head.append(sheet);
  }
  sheet.textContent = highlightRules(theme, roles(theme).note);
  style.setProperty('color-scheme', theme.light ? 'light' : 'dark');
}

/** The page and text colours a PDF's pages are drawn in, as 0xRRGGBB. */
export function pageColors(theme: Theme): { paper: number; ink: number } {
  const number = (color: string) => parseInt(color.slice(1), 16);
  return { paper: number(theme.colors[0]!), ink: number(theme.colors[5]!) };
}
