import hljs from 'highlight.js/lib/common';

/** Languages guessed at when a code block does not name its own. */
const GUESSES = [
  'bash',
  'c',
  'cpp',
  'csharp',
  'css',
  'go',
  'java',
  'javascript',
  'json',
  'kotlin',
  'php',
  'python',
  'ruby',
  'rust',
  'sql',
  'swift',
  'typescript',
  'xml',
  'yaml',
];
/** How sure a guess must be before the block is coloured by it. */
const GUESS_RELEVANCE = 7;
/** Guessing reads the block once per language, so long blocks are left plain. */
const GUESS_LIMIT = 6000;

const done = new Map<string, string | null>();

/** A code block as HTML with its syntax marked up, or null to show it plain. */
export function highlight(code: string, lang: string | undefined): string | null {
  const name = lang?.toLowerCase().replace(/^(language|lang)-/, '').trim() || '';
  if (name === 'math' || name.startsWith('failed')) return null;
  const key = `${name}\n${code}`;
  const known = done.get(key);
  if (known !== undefined) return known;
  let html: string | null = null;
  try {
    if (name && hljs.getLanguage(name)) {
      html = hljs.highlight(code, { language: name, ignoreIllegals: true }).value;
    } else if (code.length <= GUESS_LIMIT) {
      const guess = hljs.highlightAuto(code, GUESSES);
      if (guess.relevance >= GUESS_RELEVANCE) html = guess.value;
    }
  } catch (e) {
    console.warn(`Could not colour a ${name || 'code'} block: ${e}`);
  }
  done.set(key, html);
  return html;
}
