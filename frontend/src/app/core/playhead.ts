import { DeckStatus } from './models/engine';

/** A playhead as drawn: at `positionS` at `at` (performance.now()), moving at `speed`. */
export interface Playhead {
  at: number;
  positionS: number;
  speed: number;
}

/** A status reports the position of its last audio block, a few milliseconds old by a
 * varying amount: drawn as reported, the waveform would shake, even step back. The drawn
 * playhead moves on at its speed and closes this share of its gap at each status... */
const PLAYHEAD_PULL = 0.1;
/** ...unless the gap is larger (s), or the speed changed by more: a jump, a scratch, a stop. */
const PLAYHEAD_JUMP_S = 0.015;
const PLAYHEAD_SPEED_CHANGE = 0.02;

/** The playhead to draw after `status`, from the one drawn before. */
export function followPlayhead(
  before: Playhead | null,
  status: DeckStatus,
  at = performance.now(),
): Playhead | null {
  if (!status.loaded) return null;
  const reported = { at, positionS: status.positionS, speed: status.speed };
  if (!before) return reported;
  const predicted = before.positionS + before.speed * ((at - before.at) / 1000);
  const gap = status.positionS - predicted;
  const steady =
    Math.abs(gap) < PLAYHEAD_JUMP_S &&
    Math.abs(status.speed - before.speed) < PLAYHEAD_SPEED_CHANGE;
  return steady ? { ...reported, positionS: predicted + gap * PLAYHEAD_PULL } : reported;
}
