/** How tightly the view follows its target, in rad/s. Higher is snappier; lower is softer. */
const STIFFNESS = 24;
/** A press shorter than this is a tap: exactly one line. Longer, and the view starts gliding. */
const HOLD_DELAY_MS = 140;
/** Glide speed in lines per second: when it starts, and once fully ramped up. */
const HOLD_SPEED = { slow: 28, fast: 80 };
/** How long a key has been held when the speed starts and finishes ramping. */
const HOLD_RAMP_MS = { start: 500, end: 3000 };

function smoothstep(t: number): number {
  const x = Math.max(0, Math.min(1, t));
  return x * x * (3 - 2 * x);
}

/** Moves a scroll container like a critically damped spring chasing a target:
    motion starts and stops without a jolt, and a held key moves the target
    continuously, so speed does not depend on the keyboard's repeat rate. */
export class SmoothScroller {
  private target = 0;
  private position = 0;
  private velocity = 0;
  private frame = 0;
  private last = 0;
  private period = 1 / 60;
  private held: 1 | -1 | 0 = 0;
  private heldAt = 0;
  private lineHeight = 0;

  constructor(private el: HTMLElement) {
    el.addEventListener('wheel', () => this.stop(), { passive: true });
  }

  private clamp(y: number): number {
    return Math.max(0, Math.min(y, this.el.scrollHeight - this.el.clientHeight));
  }

  /** Where the view is, or is on its way to. */
  get destination(): number {
    return this.frame ? this.target : this.el.scrollTop;
  }

  by(delta: number) {
    this.to(this.destination + delta);
  }

  to(y: number) {
    this.target = this.clamp(y);
    if (this.frame) return;
    this.position = this.el.scrollTop;
    this.velocity = 0;
    this.last = performance.now();
    this.frame = requestAnimationFrame(this.step);
  }

  hold(direction: 1 | -1, lineHeight: number) {
    this.by(direction * lineHeight);
    this.held = direction;
    this.heldAt = performance.now();
    this.lineHeight = lineHeight;
  }

  release() {
    this.held = 0;
  }

  jump(y: number) {
    this.stop();
    this.el.scrollTop = this.clamp(y);
  }

  /** What is above the view grew by `delta`: the view goes down with its
      text, and so does where it is heading. */
  shift(delta: number) {
    this.position += delta;
    this.target += delta;
    this.el.scrollTop += delta;
  }

  stop() {
    cancelAnimationFrame(this.frame);
    this.frame = 0;
    this.held = 0;
  }

  private holdSpeed(now: number): number {
    const heldFor = now - this.heldAt;
    if (heldFor < HOLD_DELAY_MS) return 0;
    const ramp = smoothstep(
      (heldFor - HOLD_RAMP_MS.start) / (HOLD_RAMP_MS.end - HOLD_RAMP_MS.start),
    );
    return this.lineHeight * (HOLD_SPEED.slow + (HOLD_SPEED.fast - HOLD_SPEED.slow) * ramp);
  }

  // Frame timestamps here are only millisecond-precise, which at a 6 ms frame
  // is an 8% error that shows up as uneven travel. The display's real frame
  // interval is steady, so use a running estimate of it whenever a frame's
  // measured interval is close to that estimate.
  private frameInterval(now: number): number {
    const measured = Math.min(now - this.last, 50) / 1000;
    this.last = now;
    this.period += (measured - this.period) * 0.2;
    return Math.abs(measured - this.period) < this.period * 0.25 ? this.period : measured;
  }

  private step = (now: number) => {
    const dt = this.frameInterval(now);
    if (this.held) {
      this.target = this.clamp(this.target + this.held * this.holdSpeed(now) * dt);
    }

    // Exact solution of a critically damped spring over dt, so the motion is
    // the same at any refresh rate.
    const offset = this.position - this.target;
    const c = this.velocity + STIFFNESS * offset;
    const decay = Math.exp(-STIFFNESS * dt);
    this.position = this.target + (offset + c * dt) * decay;
    this.velocity = (this.velocity - STIFFNESS * c * dt) * decay;

    const settled =
      !this.held && Math.abs(this.position - this.target) < 0.4 && Math.abs(this.velocity) < 8;
    if (settled) {
      this.position = this.target;
      this.velocity = 0;
      this.frame = 0;
    } else {
      this.frame = requestAnimationFrame(this.step);
    }
    this.el.scrollTop = Math.round(this.position);
  };
}
