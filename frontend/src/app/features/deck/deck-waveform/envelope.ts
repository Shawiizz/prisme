import { Waveform } from '../../../core/models/engine';

/** The low band's energy is averaged over this long (s): a slice is shorter than a period of a
 * kick's fundamental, and the energy within it swings with the phase. */
const LOW_SMOOTHING_S = 0.047;
/** The low band counts twice in a slice's level: the kick and the bass carry the groove. */
const LOW_WEIGHT = 2;
/** Loudness grows as the amplitude to this power (Stevens' power law). */
const LOUDNESS_EXPONENT = 0.6;
/** The height of the track's median slice, among those with sound. */
const MEDIAN_HEIGHT = 0.35;
/** The share of the track's slices with sound that reach the full height. */
const TOP_SHARE = 0.005;

/**
 * The height of the envelope at each slice, 0 to 1: its loudness (from the energy of its
 * bands) set against the track's own, the median slice at `MEDIAN_HEIGHT` and the loudest
 * `TOP_SHARE` at the top. A dynamic track keeps its shape; a limited master, every peak at
 * the ceiling, unfolds into its kicks.
 */
export function envelopeHeights(w: Waveform): Float32Array {
  const n = w.low.length;
  const heights = new Float32Array(n);
  const radius = Math.floor((LOW_SMOOTHING_S * w.slicesPerSecond) / 2);
  const low = (k: number) => w.low[k] * w.low[k];
  // the low band's energy over the slices from i - radius to i + radius
  let lowSum = 0;
  for (let k = 0; k < Math.min(radius, n); k++) lowSum += low(k);
  for (let i = 0; i < n; i++) {
    if (i + radius < n) lowSum += low(i + radius);
    if (i > radius) lowSum -= low(i - radius - 1);
    const count = Math.min(i + radius, n - 1) - Math.max(i - radius, 0) + 1;
    const energy = (LOW_WEIGHT * lowSum) / count + w.mid[i] * w.mid[i] + w.high[i] * w.high[i];
    heights[i] = Math.pow(energy, LOUDNESS_EXPONENT / 2);
  }

  const sounding = heights.filter((l) => l > 0).sort();
  if (sounding.length === 0) return heights;
  const median = sounding[Math.floor(sounding.length / 2)];
  const top =
    sounding[Math.min(Math.floor((1 - TOP_SHARE) * sounding.length), sounding.length - 1)];
  for (let i = 0; i < n; i++) {
    const l = heights[i];
    heights[i] =
      l <= median
        ? (l / median) * MEDIAN_HEIGHT
        : top > median
          ? Math.min(MEDIAN_HEIGHT + ((l - median) / (top - median)) * (1 - MEDIAN_HEIGHT), 1)
          : 1;
  }
  return heights;
}
