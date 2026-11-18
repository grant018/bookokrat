export type LinkTarget =
  | { kind: 'external'; url: string }
  | { kind: 'chapter'; chapter: number; anchor?: string; part?: number }
  | { kind: 'anchor'; anchor: string };
