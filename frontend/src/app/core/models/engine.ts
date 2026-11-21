import type { Band, ControlChanges, EngineStatus } from './backend';

/** Shapes of the Rust side the page keeps beside those it declares (`./backend`). */

/** The pads of a deck: a page of hot cues or sounds. */
export const PADS = 8;

/** What the backend tells ~60 times per second: what changed of the engine's status since the
 * time before (see `withChanges`), nothing while nothing does; and the sections of what the app
 * mirrors that changed, each whole: the mixer (with the quantize and the tempo range), the
 * tracks (with their loads), the pads, the preview... All of it the first time. */
export type StatusChanges = Told<EngineStatus> & { controls?: ControlChanges };

/** What changed of `T`: each field sent, its decks with `null` for those that did not change. */
export type Told<T extends { decks: readonly unknown[] }> = Partial<Omit<T, 'decks'>> & {
  decks?: (T['decks'][number] | null)[];
};

/** What the app mirrors of the decks, their tracks, the mixer and the DJ's preferences, once
 * the backend told it all. */
export type ControlState = Required<ControlChanges>;

/** A waveform as it is drawn (sent in binary, see `decodeWaveform`): for each slice known,
 * from the start of the track, the height of its envelope and its colour, a byte each. */
export interface Waveform {
  slicesPerSecond: number;
  /** The track's slices: those known, and those still read. */
  slices: number;
  height: Uint8Array;
  red: Uint8Array;
  green: Uint8Array;
  blue: Uint8Array;
  /** Grown from the waveform before it, whose slices it shares (the same buffers): the first
   * slice that changed. From then on, the slices before it never change again. */
  changedFrom: number;
}

/** The track's slices, `slices_per_second` and the first slice sent (u32 little-endian each),
 * then the height, red, green and blue of each slice sent. Sent from a slice after the first,
 * it is a piece of the waveform `before`, which keeps the slices before it; `null` without it. */
export function decodeWaveform(
  buffer: ArrayBuffer,
  before: Waveform | null = null,
): Waveform | null {
  if (buffer.byteLength <= 12) return null;
  const header = new DataView(buffer);
  const [slices, slicesPerSecond, from] = [0, 4, 8].map((at) => header.getUint32(at, true));
  const sent = (buffer.byteLength - 12) / 4;
  const known = from + sent;
  const grows = from > 0 && !!before && before.height.length >= from;
  if (from > 0 && !grows) return null;
  // the slices kept in the buffers of the waveform grown, unless it outgrew them
  const shared = grows && before.height.buffer.byteLength >= known;
  const bands = [before?.height, before?.red, before?.green, before?.blue].map((kept, i) => {
    const all = shared ? new Uint8Array(kept!.buffer) : new Uint8Array(Math.max(slices, known));
    if (grows && !shared) all.set(kept!.subarray(0, from));
    all.set(new Uint8Array(buffer, 12 + i * sent, sent), from);
    return all.subarray(0, known);
  });
  return {
    slices: Math.max(slices, known),
    slicesPerSecond,
    height: bands[0],
    red: bands[1],
    green: bands[2],
    blue: bands[3],
    changedFrom: shared ? from : 0,
  };
}

export const FX_SLOTS = 3;

/** EQ bands, in the order of `ChannelStrip.eq`. */
export const BANDS: readonly Band[] = ['low', 'mid', 'high'];

/** The decks there can be: two are shown, or all four (a preference). */
export const MAX_DECKS = 4;
