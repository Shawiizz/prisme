import { Waveform } from '../../../core/models/engine';

/**
 * Exponent from the level of the sound (its amplitude) to its height, close to how loud it
 * sounds: a sound a tenth as loud as the loudest still stands a fifth as high, so that a
 * breakdown keeps a shape, while a kick still towers over what rings between two kicks.
 */
export const LOUDNESS = 0.7;
/** The low band is measured over slices shorter than a bass wave: smoothed over this long (s)
 * on each side, so that a kick is one shape and not a comb. */
const LOW_SMOOTHING_S = 0.0125;

/**
 * How high each band shows in each slice, as a share of the room (0 to 1), the bands nested:
 * the whole sound as high as it is loud, the mid and high bands within it, the high band at
 * its core, each band taking its share of the height. The low band shows outside the mid's,
 * the mid's outside the high's.
 */
export interface BandShapes {
  low: Float32Array;
  mid: Float32Array;
  high: Float32Array;
}

export function bandShapes(w: Waveform): BandShapes {
  const low = smoothed(w.low, Math.round(LOW_SMOOTHING_S * w.slicesPerSecond));
  const mid = smoothed(w.mid, 0);
  const high = smoothed(w.high, 0);
  const n = low.length;
  const loudness = new Float32Array(n);
  let loudest = 0;
  for (let i = 0; i < n; i++) {
    loudness[i] = Math.hypot(low[i], mid[i], high[i]);
    loudest = Math.max(loudest, loudness[i]);
  }
  const shapes = { low: new Float32Array(n), mid: new Float32Array(n), high: new Float32Array(n) };
  for (let i = 0; i < n; i++) {
    const sum = low[i] + mid[i] + high[i];
    if (sum === 0) continue;
    const height = Math.pow(loudness[i] / loudest, LOUDNESS);
    shapes.low[i] = height;
    shapes.mid[i] = (height * (mid[i] + high[i])) / sum;
    shapes.high[i] = (height * high[i]) / sum;
  }
  return shapes;
}

/** Each level mixed with its `reach` neighbours on each side as a power is. */
function smoothed(levels: Uint8Array, reach: number): Float32Array {
  const n = levels.length;
  const out = new Float32Array(n);
  for (let i = 0; i < n; i++) {
    const from = Math.max(i - reach, 0);
    const to = Math.min(i + reach, n - 1);
    let power = 0;
    for (let k = from; k <= to; k++) power += levels[k] * levels[k];
    out[i] = Math.sqrt(power / (to - from + 1));
  }
  return out;
}

/**
 * The height of `band` under each column of `into`, the first column starting at slice `from`,
 * the columns `per` slices wide: on a column narrower than a slice, the line through the
 * neighbouring slices' heights; on a wider one, the highest it covers. Nothing outside the track.
 */
export function columnHeights(band: Float32Array, from: number, per: number, into: Float32Array) {
  const n = band.length;
  const at = (i: number) => (i >= 0 && i < n ? band[i] : 0);
  for (let x = 0; x < into.length; x++) {
    const start = from + x * per;
    if (per < 1) {
      const centre = start + per / 2 - 0.5;
      const i = Math.floor(centre);
      const t = centre - i;
      into[x] = start < 0 || start >= n ? 0 : at(i) * (1 - t) + at(i + 1) * t;
      continue;
    }
    const first = Math.max(Math.floor(start), 0);
    const last = Math.min(Math.ceil(start + per), n);
    let highest = 0;
    for (let i = first; i < last; i++) highest = Math.max(highest, band[i]);
    into[x] = highest;
  }
}
