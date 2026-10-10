/** Taps further apart than this start a new count. */
const RESET_MS = 2000;
/** Taps needed before a tempo is given. */
const MIN_TAPS = 4;
/** Only the latest taps count, so the tempo follows a correction. */
const MAX_TAPS = 16;

/** Tempo from taps on a key or button. */
export class TapTempo {
  private taps: number[] = [];

  /** Records a tap at `nowMs`; returns the tempo (BPM) once there are enough taps. */
  tap(nowMs: number): number | null {
    const last = this.taps.at(-1);
    if (last !== undefined && nowMs - last > RESET_MS) this.taps = [];
    this.taps.push(nowMs);
    if (this.taps.length > MAX_TAPS) this.taps.shift();
    if (this.taps.length < MIN_TAPS) return null;
    return (60_000 * (this.taps.length - 1)) / (nowMs - this.taps[0]);
  }
}
