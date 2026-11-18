import { api, type ReaderPrefs } from './api';
import { fontFigures, fontStack, loadFaces } from './fonts';
import { BUILTIN_THEMES, applyTheme, customTheme, findTheme, type Theme } from './themes';

const DEFAULT_THEME = 'Kanagawa Dragon';

export const SIZE = { min: 12, max: 32, step: 1 };
export const LEADING = { min: 1.2, max: 2.2, step: 0.05 };
export const MEASURE = { min: 40, max: 110, step: 4 };
export const NAV_WIDTH = { min: 180, max: 560, step: 20, initial: 280 };

/** Saved reading preferences, plus the look currently on screen, which can
    run ahead of them while a typeface or theme is being previewed. */
class Preferences {
  saved = $state<ReaderPrefs>({
    theme: null,
    font: 'Literata',
    font_size: 18,
    line_spacing: 1.7,
    measure: 68,
    justify: false,
    drop_caps: true,
    nav_width: NAV_WIDTH.initial,
    library_sort: 'name',
    pdf_spread: false,
    pdf_zoom: 1,
    pdf_paged: false,
    zen_border: false,
  });
  themes = $state<Theme[]>(BUILTIN_THEMES);
  previewTheme = $state<string | null>(null);
  previewFont = $state<string | null>(null);

  private terminalTheme: string | null = null;
  private saveTimer: ReturnType<typeof setTimeout> | undefined;
  onerror: (message: string) => void = () => {};

  get theme(): Theme {
    return (
      findTheme(this.themes, this.previewTheme) ??
      findTheme(this.themes, this.saved.theme) ??
      findTheme(this.themes, this.terminalTheme) ??
      findTheme(this.themes, DEFAULT_THEME)!
    );
  }

  get font(): string {
    return this.previewFont ?? this.saved.font;
  }

  async load(): Promise<string[]> {
    const appearance = await api.appearance();
    const custom = appearance.custom_themes
      .map((t) => customTheme(t.name, t.colors))
      .filter((t): t is Theme => t !== null);
    this.themes = [...BUILTIN_THEMES, ...custom];
    this.terminalTheme = appearance.terminal_theme;
    this.saved = appearance.prefs;
    return appearance.warnings;
  }

  /** Paints the current look. Reads reactive state, so run it inside an effect. */
  apply() {
    applyTheme(this.theme);
    const style = document.documentElement.style;
    style.setProperty('--font-reading', fontStack(this.font));
    loadFaces(this.font);
    style.setProperty('--reading-figures', fontFigures(this.font));
    style.setProperty('--reading-size', `${this.saved.font_size}px`);
    style.setProperty('--reading-leading', String(this.saved.line_spacing));
    style.setProperty('--measure', `${this.saved.measure}ch`);
  }

  update(change: Partial<ReaderPrefs>) {
    this.saved = { ...this.saved, ...change };
    clearTimeout(this.saveTimer);
    this.saveTimer = setTimeout(() => void this.flush(), 400);
  }

  /** Writes down now a change that is still waiting to be saved. */
  async flush() {
    if (this.saveTimer === undefined) return;
    clearTimeout(this.saveTimer);
    this.saveTimer = undefined;
    await api.savePrefs($state.snapshot(this.saved)).catch((e) => this.onerror(String(e)));
  }

  step(key: 'font_size' | 'line_spacing' | 'measure' | 'nav_width', direction: 1 | -1) {
    const range = { font_size: SIZE, line_spacing: LEADING, measure: MEASURE, nav_width: NAV_WIDTH }[
      key
    ];
    const next = this.saved[key] + direction * range.step;
    const clamped = Math.max(range.min, Math.min(range.max, next));
    this.update({ [key]: Math.round(clamped * 100) / 100 });
  }

  commitPreview() {
    this.update({ theme: this.previewTheme ?? this.saved.theme, font: this.font });
    this.discardPreview();
  }

  discardPreview() {
    this.previewTheme = null;
    this.previewFont = null;
  }
}

export const prefs = new Preferences();
