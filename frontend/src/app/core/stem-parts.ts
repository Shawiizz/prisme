import { Separation, StemMix } from './models/stems';

/** A part of a separated track that a deck mutes or turns down: one stem, or the
 * instrumental (everything but the vocals). */
export type StemPart = keyof StemMix | 'instrumental';

const INSTRUMENTS = ['drums', 'bass', 'other'] as const;

/** The stems `part` stands for. */
function stemsOf(part: StemPart): (keyof StemMix)[] {
  return part === 'instrumental' ? [...INSTRUMENTS] : [part];
}

/** Whether a track separated this far has `part` to mute. */
export function hasPart(separation: Separation, part: StemPart): boolean {
  switch (separation) {
    case 'none':
      return false;
    case 'vocals':
      return part === 'vocals' || part === 'instrumental';
    case 'full':
      return true;
  }
}

/** Whether `part` plays: the instrumental while any of its instruments does. */
export function partPlaying(mix: StemMix, part: StemPart): boolean {
  return stemsOf(part).some((s) => mix[s].on);
}

/** `mix` with `part` muted or played again: the instrumental mutes all its instruments, or
 * plays them all. The levels stay. */
export function togglePart(mix: StemMix, part: StemPart): StemMix {
  const on = !partPlaying(mix, part);
  return stemsOf(part).reduce((m, s) => ({ ...m, [s]: { ...m[s], on } }), mix);
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

/** `mix` with the level of `part` set (the instrumental's instruments together). */
export function setPartLevel(mix: StemMix, part: StemPart, level: number): StemMix {
  const clamped = Math.min(1, Math.max(0, level));
  return stemsOf(part).reduce((m, s) => ({ ...m, [s]: { ...m[s], level: clamped } }), mix);
}
