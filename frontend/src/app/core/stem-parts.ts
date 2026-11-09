import { Separation, StemMix } from './models/stems';

/** A part of a separated track that a deck mutes: one stem, or the instrumental (everything
 * but the vocals). */
export type StemPart = keyof StemMix | 'instrumental';

const INSTRUMENTS = ['drums', 'bass', 'other'] as const;

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
  return part === 'instrumental' ? INSTRUMENTS.some((s) => mix[s]) : mix[part];
}

/** `mix` with `part` toggled: the instrumental mutes all its instruments, or plays them all. */
export function togglePart(mix: StemMix, part: StemPart): StemMix {
  if (part !== 'instrumental') return { ...mix, [part]: !mix[part] };
  const on = !partPlaying(mix, part);
  return { ...mix, drums: on, bass: on, other: on };
}
