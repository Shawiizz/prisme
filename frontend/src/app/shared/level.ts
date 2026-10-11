/** A segment of a level meter (`.pr-meter`): off, lit, hot or clipping. */
export type Segment = '' | 'is-lit' | 'is-hot' | 'is-clip';

/** Range of a meter: its lowest segment lights at this level (dBFS). */
const FLOOR_DB = -48;

/**
 * The `count` segments of a meter for a peak (1 = full scale), lowest first: grey up to
 * -6 dB, amber up to 0, the last one red at full scale.
 */
export function levelSegments(peak: number, count: number): Segment[] {
  const db = peak > 0 ? 20 * Math.log10(peak) : -Infinity;
  return Array.from({ length: count }, (_, i) => {
    const at = FLOOR_DB * (1 - i / (count - 1));
    if (db < at) return '';
    return i === count - 1 ? 'is-clip' : at >= -6 ? 'is-hot' : 'is-lit';
  });
}
