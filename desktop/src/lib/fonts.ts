export type FontOption = {
  name: string;
  family: string;
  kind: 'serif' | 'sans';
  note: string;
  /** Set where the face's old-style figures hurt reading. */
  figures?: 'lining';
};

/** Typefaces shipped inside the app. All are openly licensed text faces. */
export const FONTS: FontOption[] = [
  {
    name: 'Literata',
    family: "'Literata Variable'",
    kind: 'serif',
    note: 'Designed for e-book reading. The default.',
  },
  {
    name: 'Source Serif',
    family: "'Source Serif 4 Variable'",
    kind: 'serif',
    note: 'Even and neutral. Stays out of the way.',
  },
  {
    name: 'Newsreader',
    family: "'Newsreader Variable'",
    kind: 'serif',
    note: 'Drawn for long reading on screens.',
  },
  {
    name: 'Charis',
    family: "'Charis SIL'",
    kind: 'serif',
    note: 'Sturdy, from the Charter family. Crisp at small sizes.',
  },
  {
    name: 'EB Garamond',
    family: "'EB Garamond Variable'",
    kind: 'serif',
    note: 'The classic book page. Lighter and more literary.',
  },
  {
    name: 'Libre Caslon',
    family: "'Libre Caslon Text'",
    kind: 'serif',
    note: 'Warm English book face, tuned for screen text.',
  },
  {
    name: 'Merriweather',
    family: "'Merriweather Variable'",
    kind: 'serif',
    note: 'Wide and sturdy. Used in Google Play Books.',
  },
  {
    name: 'Vollkorn',
    family: "'Vollkorn Variable'",
    kind: 'serif',
    note: 'Dark and full-bodied. Used in Google Play Books.',
    figures: 'lining',
  },
  {
    name: 'Gelasio',
    family: "'Gelasio Variable'",
    kind: 'serif',
    note: "Drawn to Georgia's proportions.",
  },
  {
    name: 'Pagella',
    family: "'TeX Gyre Pagella'",
    kind: 'serif',
    note: 'A free take on Palatino. Broad and calligraphic.',
  },
  {
    name: 'Atkinson Hyperlegible',
    family: "'Atkinson Hyperlegible Next Variable'",
    kind: 'sans',
    note: 'Built for the clearest possible letterforms.',
  },
  {
    name: 'Source Sans',
    family: "'Source Sans 3 Variable'",
    kind: 'sans',
    note: 'A calm humanist sans for long passages.',
  },
];

/** CSS font stack for a saved font name: a bundled face, or one installed on this computer. */
export function fontStack(name: string): string {
  const bundled = FONTS.find((f) => f.name === name);
  if (bundled) {
    return `${bundled.family}, ${bundled.kind === 'serif' ? 'Georgia, serif' : 'system-ui, sans-serif'}`;
  }
  return `"${name.replaceAll('"', '')}", 'Literata Variable', Georgia, serif`;
}

/** A face that arrives once a chapter is on the page (the italic, at the
    first italic word to come into view) makes the web view lay the whole
    chapter out again, which in a long one stops everything for half a
    second. So the faces book text is set in are fetched as soon as the
    typeface is chosen, for Latin text with and without its rarer accents. */
export function loadFaces(name: string) {
  const family = fontStack(name).split(',')[0]!;
  for (const face of ['', 'italic ', 'bold ', 'italic bold ']) {
    document.fonts.load(`${face}1em ${family}`, 'a\u0101').catch(() => {});
  }
}

/** Book text uses old-style figures, except in faces like Vollkorn whose
    old-style 1 is drawn as a small capital I, so that "11:47" reads "II:47". */
export function fontFigures(name: string): string {
  return FONTS.find((f) => f.name === name)?.figures === 'lining' ? 'lining-nums' : 'oldstyle-nums';
}

/** Typefaces differ in how tall their lowercase letters are, so the same
    point size can look a size apart. This makes every reading face render
    with Literata's proportions, so changing typeface does not change how
    large the text looks. */
export async function matchLetterHeight() {
  const reference = "'Literata Variable'";
  await document.fonts.load(`100px ${reference}`);
  const probe = document.createElement('div');
  probe.style.cssText = `position:absolute;visibility:hidden;font:100px ${reference};height:1ex`;
  document.body.append(probe);
  const ratio = probe.getBoundingClientRect().height / 100;
  probe.remove();
  if (ratio > 0.3 && ratio < 0.7) {
    document.documentElement.style.setProperty('--letter-height', ratio.toFixed(3));
  }
}
