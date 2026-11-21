import type { MeterLevel } from '../core/models/backend';

/** A segment of a level meter (`.pr-meter`): off, lit, hot or clipping. */
export type Segment = '' | 'is-lit' | 'is-hot' | 'is-clip';

/** The segments drawn amber: the top eighth of the scale (from −6 dB of its 48). */
const HOT = 0.875;

/** The `count` segments of `meter`, lowest first: lit up to its level and at its highest of
 * late, the last one red while a clip shows. */
export function segments(meter: MeterLevel | undefined, count: number): Segment[] {
  const held = meter && meter.held > 0 ? Math.floor(meter.held * (count - 1)) : -1;
  return Array.from({ length: count }, (_, i) => {
    if (i === count - 1) return meter?.clip ? 'is-clip' : '';
    const at = i / (count - 1);
    const lit = (!!meter && meter.level > 0 && meter.level >= at) || i === held;
    return lit ? (at >= HOT ? 'is-hot' : 'is-lit') : '';
  });
}

/** Whether two meters show the same segments: a meter is drawn again only when one changed. */
export function sameSegments(a: Segment[] | null, b: Segment[] | null): boolean {
  return a === b || (!!a && !!b && a.length === b.length && a.every((s, i) => s === b[i]));
}
