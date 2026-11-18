import { api } from './api';

/** One step of a replay script (see `BOOKOKRAT_READER_SCRIPT`): press keys,
    click, double-click, drag or right-click at points in the window, type
    into the focused field, pause, write down what is selected, or capture
    the screen. The mouse steps send events the page listens for; they do
    not move the real pointer, so they cannot make the web view's own
    text selection. */
export type ScriptStep =
  | { keys: string; hold?: number }
  | { click: [x: number, y: number]; ctrl?: boolean }
  | { dblclick: [x: number, y: number] }
  | { drag: [x1: number, y1: number, x2: number, y2: number] }
  | { menu: [x: number, y: number] }
  /** The same with real mouse input, which the web view's own text
      selection answers to: a drag, or `times` clicks in one place. */
  | { realdrag: [x1: number, y1: number, x2: number, y2: number]; hold?: number }
  | { realclick: [x: number, y: number]; times?: number }
  | { realmouse: [kind: 'press' | 'move' | 'hover' | 'release', x: number, y: number] }
  /** A real click on the middle of the first link that leads to the anchor
      `link`, after resting on it for `hover` ms when given; `hoverOnly`
      rests on it without clicking. `within` (a selector, the page's text by default)
      says where to look, e.g. `.card` for a preview card. */
  | { link: string; hover?: number; hoverOnly?: boolean; within?: string }
  /** Notes where the anchor `check` is now, as pixels below the top of the
      view (or that it is not on the page), in `checks.txt` beside the script. */
  | { check: string }
  | { selection: string }
  | { type: string }
  | { wait: number }
  | { shot: string };

const KEY_NAMES: Record<string, string> = {
  Space: ' ',
  Esc: 'Escape',
  Up: 'ArrowUp',
  Down: 'ArrowDown',
  Left: 'ArrowLeft',
  Right: 'ArrowRight',
};

const sleep = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));
const painted = () =>
  new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));

/** Presses one key written the way bindings are named, e.g. `j`, `C-d`, `S-Tab`. */
async function press(token: string, hold: number) {
  const parts = token.length > 1 ? token.split('-') : [token];
  const name = parts.pop() || '-';
  const key = KEY_NAMES[name] ?? name;
  const init: KeyboardEventInit = {
    key,
    code: key.length === 1 ? `Key${key.toUpperCase()}` : key,
    ctrlKey: parts.includes('C'),
    altKey: parts.includes('A'),
    shiftKey: parts.includes('S'),
    bubbles: true,
    cancelable: true,
  };
  const target = document.activeElement ?? document.body;
  target.dispatchEvent(new KeyboardEvent('keydown', init));
  await sleep(hold);
  target.dispatchEvent(new KeyboardEvent('keyup', init));
}

/** Sends the events of a mouse press and release to whatever is at a point. */
function mouse(
  names: string[],
  [x, y]: [number, number],
  button: number,
  more: { detail?: number; ctrlKey?: boolean } = {},
) {
  const target = document.elementFromPoint(x, y) ?? document.body;
  for (const name of names) {
    const init = { clientX: x, clientY: y, button, bubbles: true, cancelable: true, ...more };
    const event = name.startsWith('pointer') ? new PointerEvent(name, init) : new MouseEvent(name, init);
    target.dispatchEvent(event);
  }
}

/** Presses at one point, moves to another in steps, and lets go. */
async function drag([x1, y1, x2, y2]: [number, number, number, number]) {
  mouse(['pointerdown', 'mousedown'], [x1, y1], 0);
  for (let step = 1; step <= 8; step++) {
    const at: [number, number] = [x1 + ((x2 - x1) * step) / 8, y1 + ((y2 - y1) * step) / 8];
    mouse(['pointermove', 'mousemove'], at, 0);
    await sleep(16);
  }
  mouse(['pointerup', 'mouseup'], [x2, y2], 0);
}

export async function runScript(steps: ScriptStep[]) {
  const checks: string[] = [];
  for (const step of steps) {
    if ('keys' in step) {
      for (const token of step.keys.split(' ').filter(Boolean)) {
        await press(token, step.hold ?? 30);
        await sleep(40);
      }
    } else if ('click' in step) {
      const names = ['pointerdown', 'mousedown', 'pointerup', 'mouseup', 'click'];
      mouse(names, step.click, 0, { detail: 1, ctrlKey: step.ctrl });
    } else if ('dblclick' in step) {
      const names = ['pointerdown', 'mousedown', 'pointerup', 'mouseup', 'click'];
      mouse(names, step.dblclick, 0, { detail: 1 });
      mouse([...names, 'dblclick'], step.dblclick, 0, { detail: 2 });
    } else if ('drag' in step) {
      await drag(step.drag);
    } else if ('realdrag' in step) {
      const [x1, y1, x2, y2] = step.realdrag;
      await api.scriptMouse('press', x1, y1);
      for (let n = 1; n <= 10; n++) {
        await sleep(20);
        await api.scriptMouse('move', x1 + ((x2 - x1) * n) / 10, y1 + ((y2 - y1) * n) / 10);
      }
      await sleep(step.hold ?? 20);
      await api.scriptMouse('release', x2, y2);
      await sleep(60);
    } else if ('realclick' in step) {
      for (let n = 0; n < (step.times ?? 1); n++) {
        await api.scriptMouse('press', ...step.realclick);
        await sleep(20);
        await api.scriptMouse('release', ...step.realclick);
        await sleep(60);
      }
    } else if ('link' in step) {
      const scope = step.within ?? '.scroll .prose';
      const link = [...document.querySelectorAll<HTMLElement>(`${scope} a[data-target]`)].find(
        (a) => a.dataset.target === step.link,
      );
      if (!step.within) {
        link?.scrollIntoView({ block: 'center' });
        await painted();
      }
      const box = link?.getClientRects()[0];
      if (!box) throw new Error(`No link to ${step.link} on the page`);
      const at: [number, number] = [box.left + box.width / 2, box.top + box.height / 2];
      if (step.hover !== undefined) {
        await api.scriptMouse('hover', at[0] - 2, at[1]);
        await api.scriptMouse('hover', ...at);
        await sleep(step.hover);
      }
      if (step.hoverOnly) continue;
      await api.scriptMouse('press', ...at);
      await sleep(20);
      await api.scriptMouse('release', ...at);
      await sleep(60);
    } else if ('check' in step) {
      const el = document.querySelector(`.prose [data-anchor="${CSS.escape(step.check)}"]`);
      const view = document.querySelector('.scroll')?.getBoundingClientRect();
      const top = el && view ? Math.round(el.getBoundingClientRect().top - view.top) : null;
      checks.push(`${step.check}\t${top === null ? 'not on the page' : top}`);
      await api.scriptNote('checks', checks.join('\n'));
    } else if ('realmouse' in step) {
      await api.scriptMouse(...step.realmouse);
    } else if ('selection' in step) {
      // With how far the text has scrolled, to tell a selection that
      // grew by scrolling from one that grew in place.
      const scrolled = document.querySelector('.scroll')?.scrollTop ?? 0;
      const text = getSelection()?.toString() ?? '';
      await api.scriptNote(step.selection, `${text}\n[scrolled ${Math.round(scrolled)}]`);
    } else if ('menu' in step) {
      mouse(['contextmenu'], step.menu, 2);
    } else if ('type' in step) {
      const field = document.activeElement;
      if (field instanceof HTMLInputElement || field instanceof HTMLTextAreaElement) {
        field.value += step.type;
        field.dispatchEvent(new Event('input', { bubbles: true }));
      }
    } else if ('wait' in step) {
      await sleep(step.wait);
    } else {
      await document.fonts.ready;
      await painted();
      await api.scriptShot(step.shot);
    }
  }
  await api.quit();
}
