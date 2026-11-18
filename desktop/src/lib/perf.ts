import type { SmoothScroller } from './scroller';

type PhaseStats = {
  frames: number;
  mean_ms: number;
  p50_ms: number;
  p95_ms: number;
  p99_ms: number;
  max_ms: number;
  late_frames: number;
};

function stats(deltas: number[]): PhaseStats {
  const sorted = [...deltas].sort((a, b) => a - b);
  const at = (q: number) => sorted[Math.min(sorted.length - 1, Math.floor(q * sorted.length))] ?? 0;
  const median = at(0.5);
  const round = (n: number) => Math.round(n * 100) / 100;
  return {
    frames: deltas.length,
    mean_ms: round(deltas.reduce((a, b) => a + b, 0) / Math.max(1, deltas.length)),
    p50_ms: round(median),
    p95_ms: round(at(0.95)),
    p99_ms: round(at(0.99)),
    max_ms: round(sorted[sorted.length - 1] ?? 0),
    late_frames: deltas.filter((d) => d > median * 1.5).length,
  };
}

/** How evenly the view moved: per-frame travel, and how much that travel
    changed from one frame to the next (0 would be perfectly even motion). */
function motion(positions: number[]) {
  const travel = positions.slice(1).map((p, i) => p - positions[i]!);
  const moving = travel.filter((t) => t !== 0);
  const mean = moving.reduce((a, b) => a + b, 0) / Math.max(1, moving.length);
  const changes = travel.slice(1).map((t, i) => Math.abs(t - travel[i]!));
  const rms = Math.sqrt(changes.reduce((a, b) => a + b * b, 0) / Math.max(1, changes.length));
  const round = (n: number) => Math.round(n * 100) / 100;
  return {
    total_px: round((positions[positions.length - 1] ?? 0) - (positions[0] ?? 0)),
    stalled_frames: travel.filter((t) => t === 0).length,
    mean_px_per_frame: round(mean),
    rms_change_px: round(rms),
    max_change_px: round(Math.max(0, ...changes)),
    per_frame_px: travel.map(round),
  };
}

/** Runs `onFrame` every animation frame for `duration` ms and returns the frame intervals. */
function record(duration: number, onFrame: (dt: number) => void): Promise<number[]> {
  return new Promise((resolve) => {
    const deltas: number[] = [];
    const start = performance.now();
    let last = start;
    const tick = (now: number) => {
      const dt = now - last;
      last = now;
      deltas.push(dt);
      onFrame(dt);
      if (now - start < duration) requestAnimationFrame(tick);
      else resolve(deltas.slice(1));
    };
    requestAnimationFrame(tick);
  });
}

export async function measureScrolling(
  el: HTMLElement,
  scroller: SmoothScroller,
  lineHeight: number,
): Promise<Record<string, unknown>> {
  const max = () => el.scrollHeight - el.clientHeight;
  const out: Record<string, unknown> = {};

  out.idle = stats(await record(1000, () => {}));

  let direction = 1;
  out.steady_1200px_per_s = stats(
    await record(4000, (dt) => {
      if (el.scrollTop >= max() - 1) direction = -1;
      if (el.scrollTop <= 0) direction = 1;
      el.scrollTop += direction * 1.2 * dt;
    }),
  );

  scroller.jump(0);
  await record(300, () => {});
  const positions: number[] = [];
  scroller.hold(1, lineHeight);
  out.held_line_key = stats(await record(3000, () => positions.push(el.scrollTop)));
  scroller.release();
  await record(500, () => positions.push(el.scrollTop));
  out.held_line_key_motion = motion(positions);

  scroller.jump(0);
  direction = 1;
  const paging = setInterval(() => {
    if (el.scrollTop >= max() - 1) direction = -1;
    if (el.scrollTop <= 0) direction = 1;
    scroller.by((direction * el.clientHeight) / 2);
  }, 350);
  out.half_page_jumps = stats(await record(3000, () => {}));
  clearInterval(paging);
  scroller.jump(0);

  return out;
}
