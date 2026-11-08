import { DirEntry } from './models/library';
import { camelotCompatible, camelotEnergyBoost, transposeCamelot } from '../shared/camelot';
import { octaveRatio } from './sync';

/** The reference deck as it sounded when the ranking was made. */
export interface MixReference {
  /** Its tempo then (`null`: unknown). */
  bpm: number | null;
  /** Its key as heard then. */
  camelot: string | null;
  /** The track on it: not a candidate. */
  path: string | null;
}

export interface MixContext {
  reference: MixReference;
  /** The travel of the tempo faders (0.08 = ±8 %). */
  pitchRange: number;
  /** Whether the deck the track would play on holds its key when the tempo changes. */
  keyLock: boolean;
  playedTonight: ReadonlySet<string>;
}

/**
 * How well a track follows the reference, lowest first, compared term by term:
 * 1. not played tonight (nor on the reference deck);
 * 2. the harmony of its key as it will be heard (moved by the pitch that matches the
 *    tempo, unless the key lock holds it): compatible, then an energy boost (+2 on the
 *    wheel), then the others, then unknown keys;
 * 3. a tempo the faders reach;
 * 4. the smallest pitch change (half and double tempos count).
 */
export function mixRank(track: DirEntry, context: MixContext): number[] {
  const { reference } = context;
  const played = track.path === reference.path || context.playedTonight.has(track.path) ? 1 : 0;
  const bpm = track.details?.bpm;
  const change =
    bpm && reference.bpm ? (octaveRatio(bpm, reference.bpm) * reference.bpm) / bpm - 1 : null;
  const camelot = track.details?.key?.camelot ?? null;
  const heard =
    camelot && change !== null && !context.keyLock
      ? (transposeCamelot(camelot, Math.round(12 * Math.log2(1 + change)))?.camelot ?? camelot)
      : camelot;
  const harmony =
    !heard || !reference.camelot
      ? 3
      : camelotCompatible(heard, reference.camelot)
        ? 0
        : camelotEnergyBoost(reference.camelot, heard)
          ? 1
          : 2;
  const pitch = change === null ? Number.MAX_SAFE_INTEGER : Math.abs(change);
  const unreachable = pitch <= context.pitchRange ? 0 : 1;
  return [played, harmony, unreachable, pitch];
}
