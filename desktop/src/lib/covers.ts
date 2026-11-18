/** Cloth and ink for a cover drawn from a book's title, used wherever a
    book is shown as an object. */
const CLOTHS: [string, string][] = [
  ['#1f3a4d', '#e9e2cc'],
  ['#5a2f2c', '#ecdcc8'],
  ['#3d4a34', '#e6e3cc'],
  ['#2b2a33', '#cfd2de'],
  ['#6b5530', '#f3ead0'],
  ['#5d4a5e', '#f0e3e6'],
  ['#2f4640', '#e2e6d4'],
  ['#56602f', '#f0eccd'],
  ['#d9d1bc', '#2a2826'],
  ['#4a1f22', '#e8d3c9'],
  ['#3b4a63', '#e8e6da'],
  ['#7a3b2a', '#f3e6d0'],
  ['#4f5d52', '#eeeadb'],
  ['#2f5d6b', '#f0ead6'],
  ['#4a3f33', '#eadfca'],
];

/** The same book always gets the same cover: the choice hangs on its file name. */
export function coverFor(path: string): { cloth: string; ink: string } {
  const name = path.slice(path.lastIndexOf('/') + 1);
  let hash = 0;
  for (const ch of name) hash = (hash * 31 + ch.codePointAt(0)!) >>> 0;
  const [cloth, ink] = CLOTHS[hash % CLOTHS.length]!;
  return { cloth, ink };
}
