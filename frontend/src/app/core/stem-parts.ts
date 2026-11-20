import { StemMix } from './models/stems';

/** A part of a separated track that a deck mutes or turns down: one stem, or the
 * instrumental (everything but the vocals). */
export type StemPart = keyof StemMix | 'instrumental';

const INSTRUMENTS = ['drums', 'bass', 'other'] as const;

/** The stems `part` stands for. */
function stemsOf(part: StemPart): (keyof StemMix)[] {
  return part === 'instrumental' ? [...INSTRUMENTS] : [part];
}

/** Whether `part` plays: the instrumental while any of its instruments does. */
export function partPlaying(mix: StemMix, part: StemPart): boolean {
  return stemsOf(part).some((s) => mix[s].on);
}

/** The order of the stems in the engine's bit sets (`DeckStatus.stemsWaiting`). */
const STEM_BITS: (keyof StemMix)[] = ['vocals', 'drums', 'bass', 'other'];

/** Whether any stem of `part` is in the bit set `bits`. */
export function partIn(bits: number, part: StemPart): boolean {
  return stemsOf(part).some((s) => (bits & (1 << STEM_BITS.indexOf(s))) !== 0);
}

/** The level of `part`: the instrumental's loudest instrument. */
export function partLevel(mix: StemMix, part: StemPart): number {
  return Math.max(...stemsOf(part).map((s) => mix[s].level));
}

/** The name of `part` in the actions' ids: `melody` for the other instruments. */
export function partId(part: StemPart): string {
  return part === 'other' ? 'melody' : part;
}
